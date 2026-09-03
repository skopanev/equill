use super::qdrant::{
    CollectionSchema, ProviderHit, ProviderMetadata, ProviderPoint, Query, Transport,
};
use crate::kernel::error::Error;
use crate::vector::config::VectorConfig;
use crate::vector::model::{
    CollectionReport, VectorPoint, VectorPointMetadata, VectorSearchRequest, valid_collection_name,
    valid_sha256, validate_point, validate_search, vector_error,
};

pub(crate) struct Collection<T> {
    config: VectorConfig,
    transport: T,
}

pub(crate) struct AliasChange {
    previous: Option<String>,
    target: String,
    changed: bool,
}

impl<T: Transport> Collection<T> {
    pub(crate) fn new(config: VectorConfig, transport: T) -> Self {
        Self { config, transport }
    }

    pub(crate) fn prepare(&self, physical: &str) -> Result<CollectionReport, Error> {
        validate_name(physical)?;
        let expected = self.expected_schema();
        let created = match self.transport.collection_schema(physical)? {
            Some(actual) if actual == expected => false,
            Some(_) => {
                return Err(vector_error(
                    "existing collection has incompatible vector parameters",
                ));
            }
            None => {
                self.transport
                    .create_collection(physical, expected.clone())?;
                true
            }
        };
        Ok(CollectionReport {
            collection: physical.to_owned(),
            created,
        })
    }

    pub(crate) fn upsert(&self, physical: &str, points: &[VectorPoint]) -> Result<(), Error> {
        validate_name(physical)?;
        for point in points {
            validate_point(point, self.config.dimensions)?;
        }
        if points.is_empty() {
            return Ok(());
        }
        self.ensure_compatible(physical, "cannot upsert into an incompatible collection")?;
        let points = points
            .iter()
            .cloned()
            .map(|point| ProviderPoint {
                point,
                store_id: self.config.store_id,
                model_sha256: self.config.embedding.model_sha256().to_owned(),
            })
            .collect::<Vec<_>>();
        self.transport.upsert(physical, &points)
    }

    pub(crate) fn search(&self, request: &VectorSearchRequest) -> Result<Vec<ProviderHit>, Error> {
        validate_search(request, self.config.dimensions)?;
        validate_filters(&request.namespaces)?;
        validate_filters(&request.type_names)?;
        self.ensure_compatible(
            &self.config.collection_alias,
            "cannot query an incompatible collection",
        )?;
        let hits = self.transport.query(Query {
            collection: self.config.collection_alias.clone(),
            store_id: self.config.store_id,
            model_sha256: self.config.embedding.model_sha256().to_owned(),
            vector: request.vector.clone(),
            namespaces: request.namespaces.clone(),
            type_names: request.type_names.clone(),
            limit: request.limit,
        })?;
        hits.into_iter().map(|hit| self.validate_hit(hit)).collect()
    }

    pub(crate) fn active(&self) -> Result<String, Error> {
        let physical = self
            .transport
            .alias_target(&self.config.collection_alias)?
            .ok_or_else(rebuild_required)?;
        self.ensure_compatible(
            &physical,
            "active compatible collection is missing; run vector rebuild",
        )?;
        Ok(physical)
    }

    pub(crate) fn metadata(
        &self,
        physical: &str,
        record_ids: &[uuid::Uuid],
    ) -> Result<Vec<VectorPointMetadata>, Error> {
        validate_name(physical)?;
        self.ensure_compatible(physical, "cannot read an incompatible collection")?;
        let point_ids = record_ids
            .iter()
            .map(|record_id| super::point::physical_id(self.config.store_id, *record_id))
            .collect::<Vec<_>>();
        self.transport
            .metadata(physical, &point_ids)?
            .into_iter()
            .map(|item| self.validate_metadata(item))
            .collect()
    }

    pub(crate) fn require_active(&self, physical: &str) -> Result<(), Error> {
        if self
            .transport
            .alias_target(&self.config.collection_alias)?
            .as_deref()
            != Some(physical)
        {
            return Err(rebuild_required());
        }
        self.ensure_compatible(
            physical,
            "active collection is incompatible; run vector rebuild",
        )
    }

    pub(crate) fn activate(&self, physical: &str) -> Result<AliasChange, Error> {
        validate_name(physical)?;
        let expected = self.expected_schema();
        if self.transport.collection_schema(physical)? != Some(expected) {
            return Err(vector_error("cannot activate an incompatible collection"));
        }
        let previous = self.transport.alias_target(&self.config.collection_alias)?;
        let changed = previous.as_deref() != Some(physical);
        if changed {
            self.transport.retarget_alias(
                &self.config.collection_alias,
                previous.as_deref(),
                Some(physical),
            )?;
        }
        Ok(AliasChange {
            previous,
            target: physical.to_owned(),
            changed,
        })
    }

    pub(crate) fn restore(&self, change: &AliasChange) -> Result<(), Error> {
        if !change.changed {
            return Ok(());
        }
        self.transport.retarget_alias(
            &self.config.collection_alias,
            Some(&change.target),
            change.previous.as_deref(),
        )
    }

    fn validate_hit(&self, hit: ProviderHit) -> Result<ProviderHit, Error> {
        if hit.store_id != self.config.store_id
            || hit.model_sha256 != self.config.embedding.model_sha256()
            || !valid_sha256(&hit.record_sha256)
            || !valid_sha256(&hit.input_sha256)
            || !hit.score.is_finite()
        {
            return Err(vector_error("query returned invalid point metadata"));
        }
        Ok(hit)
    }

    fn validate_metadata(&self, item: ProviderMetadata) -> Result<VectorPointMetadata, Error> {
        if item.store_id != self.config.store_id
            || !valid_sha256(&item.model_sha256)
            || !valid_sha256(&item.record_sha256)
            || !valid_sha256(&item.input_sha256)
        {
            return Err(vector_error("retrieval returned invalid point metadata"));
        }
        Ok(VectorPointMetadata {
            record_id: item.record_id,
            record_sha256: item.record_sha256,
            input_sha256: item.input_sha256,
            model_sha256: item.model_sha256,
        })
    }

    fn expected_schema(&self) -> CollectionSchema {
        CollectionSchema {
            dimensions: self.config.dimensions,
            distance: self.config.distance,
            store_id: self.config.store_id,
            model_sha256: self.config.embedding.model_sha256().to_owned(),
        }
    }

    fn ensure_compatible(&self, collection: &str, reason: &str) -> Result<(), Error> {
        if self.transport.collection_schema(collection)? != Some(self.expected_schema()) {
            return Err(vector_error(reason));
        }
        Ok(())
    }
}

fn rebuild_required() -> Error {
    vector_error("active compatible collection is missing; run vector rebuild")
}

fn validate_name(name: &str) -> Result<(), Error> {
    valid_collection_name(name)
        .then_some(())
        .ok_or_else(|| vector_error("invalid collection name"))
}

fn validate_filters(values: &[String]) -> Result<(), Error> {
    if values
        .iter()
        .any(|value| value.trim().is_empty() || value.chars().any(char::is_control))
    {
        return Err(vector_error("invalid search filter"));
    }
    Ok(())
}
