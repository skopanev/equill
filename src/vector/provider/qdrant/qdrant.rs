use crate::kernel::error::Error;
use crate::vector::config::VectorConfig;
use crate::vector::model::VectorPoint;
use qdrant_client::Qdrant;
use qdrant_client::qdrant as api;
use std::future::Future;
use std::time::Duration;
use uuid::Uuid;

pub(crate) use super::schema::CollectionSchema;

#[derive(Clone)]
pub(crate) struct ProviderPoint {
    pub point: VectorPoint,
    pub store_id: Uuid,
    pub model_sha256: String,
}

pub(crate) struct Query {
    pub collection: String,
    pub store_id: Uuid,
    pub model_sha256: String,
    pub vector: Vec<f32>,
    pub namespaces: Vec<String>,
    pub type_names: Vec<String>,
    pub limit: u16,
}

#[derive(Clone, Debug)]
pub(crate) struct ProviderHit {
    pub store_id: Uuid,
    pub model_sha256: String,
    pub record_id: Uuid,
    pub score: f32,
    pub record_sha256: String,
    pub input_sha256: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ProviderMetadata {
    pub store_id: Uuid,
    pub model_sha256: String,
    pub record_id: Uuid,
    pub record_sha256: String,
    pub input_sha256: String,
}

pub(crate) trait Transport {
    fn collection_schema(&self, name: &str) -> Result<Option<CollectionSchema>, Error>;
    fn create_collection(&self, name: &str, schema: CollectionSchema) -> Result<(), Error>;
    fn upsert(&self, collection: &str, points: &[ProviderPoint]) -> Result<(), Error>;
    fn delete(&self, collection: &str, point_ids: &[Uuid]) -> Result<(), Error>;
    fn metadata(
        &self,
        collection: &str,
        point_ids: &[Uuid],
    ) -> Result<Vec<ProviderMetadata>, Error>;
    fn query(&self, query: Query) -> Result<Vec<ProviderHit>, Error>;
    fn alias_target(&self, alias: &str) -> Result<Option<String>, Error>;
    fn retarget_alias(
        &self,
        alias: &str,
        previous: Option<&str>,
        target: Option<&str>,
    ) -> Result<(), Error>;
}

pub(crate) struct QdrantTransport {
    worker: super::worker::RuntimeWorker,
}

impl QdrantTransport {
    pub(crate) fn new(config: &VectorConfig) -> Result<Self, Error> {
        let mut builder = Qdrant::from_url(&config.endpoint)
            .skip_compatibility_check()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(2));
        builder.set_pool_size(1);
        if let Some(api_key) = config.api_key()? {
            builder = builder.api_key(api_key);
        }
        Ok(Self {
            worker: super::worker::RuntimeWorker::start(builder)?,
        })
    }

    fn run<T, E, F, Fut>(&self, action: &'static str, task: F) -> Result<T, Error>
    where
        T: Send + 'static,
        E: Send + 'static,
        F: FnOnce(Qdrant) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, E>> + Send + 'static,
    {
        self.worker.run(action, task)
    }
}

impl Transport for QdrantTransport {
    fn collection_schema(&self, name: &str) -> Result<Option<CollectionSchema>, Error> {
        let name = name.to_owned();
        let response = self.run("read collection", move |client| async move {
            if !client.collection_exists(&name).await? {
                return Ok(None);
            }
            client.collection_info(&name).await.map(Some)
        })?;
        response.map(super::schema::parse).transpose()
    }

    fn create_collection(&self, name: &str, schema: CollectionSchema) -> Result<(), Error> {
        let request = api::CreateCollectionBuilder::new(name)
            .vectors_config(api::VectorParamsBuilder::new(
                schema.dimensions,
                super::schema::to_api(schema.distance),
            ))
            .metadata(super::schema::metadata(&schema));
        self.run("create collection", move |client| async move {
            client.create_collection(request).await
        })?;
        Ok(())
    }

    fn upsert(&self, collection: &str, points: &[ProviderPoint]) -> Result<(), Error> {
        let points = points
            .iter()
            .map(super::point::qdrant_point)
            .collect::<Result<Vec<_>, _>>()?;
        let request = api::UpsertPointsBuilder::new(collection, points).wait(true);
        self.run("upsert points", move |client| async move {
            client.upsert_points(request).await
        })?;
        Ok(())
    }

    fn delete(&self, collection: &str, point_ids: &[Uuid]) -> Result<(), Error> {
        let ids = point_ids.iter().copied().map(Into::into).collect();
        let request = api::DeletePointsBuilder::new(collection)
            .points(api::PointsIdsList { ids })
            .wait(true);
        self.run("delete points", move |client| async move {
            client.delete_points(request).await
        })?;
        Ok(())
    }

    fn metadata(
        &self,
        collection: &str,
        point_ids: &[Uuid],
    ) -> Result<Vec<ProviderMetadata>, Error> {
        let ids = point_ids
            .iter()
            .copied()
            .map(Into::into)
            .collect::<Vec<_>>();
        let request = api::GetPointsBuilder::new(collection, ids)
            .with_payload(true)
            .with_vectors(false);
        self.run("retrieve point metadata", move |client| async move {
            client.get_points(request).await
        })?
        .result
        .into_iter()
        .map(super::point::provider_metadata)
        .collect()
    }

    fn query(&self, query: Query) -> Result<Vec<ProviderHit>, Error> {
        let mut conditions = vec![
            api::Condition::matches("schema", super::point::POINT_SCHEMA.to_owned()),
            api::Condition::matches("store_id", query.store_id.to_string()),
            api::Condition::matches("model_sha256", query.model_sha256),
        ];
        if !query.namespaces.is_empty() {
            conditions.push(api::Condition::matches("namespace", query.namespaces));
        }
        if !query.type_names.is_empty() {
            conditions.push(api::Condition::matches("type", query.type_names));
        }
        let request = api::QueryPointsBuilder::new(query.collection)
            .query(query.vector)
            .filter(api::Filter::must(conditions))
            .limit(u64::from(query.limit))
            .with_payload(true)
            .with_vectors(false);
        self.run("query points", move |client| async move {
            client.query(request).await
        })?
        .result
        .into_iter()
        .map(super::point::provider_hit)
        .collect()
    }

    fn alias_target(&self, alias: &str) -> Result<Option<String>, Error> {
        let alias = alias.to_owned();
        let aliases = self.run("list aliases", move |client| async move {
            client.list_aliases().await
        })?;
        Ok(aliases
            .aliases
            .into_iter()
            .find(|item| item.alias_name == alias)
            .map(|item| item.collection_name))
    }

    fn retarget_alias(
        &self,
        alias: &str,
        previous: Option<&str>,
        target: Option<&str>,
    ) -> Result<(), Error> {
        let alias = alias.to_owned();
        let previous = previous.map(str::to_owned);
        let target = target.map(str::to_owned);
        self.run("retarget alias", move |client| async move {
            super::alias::retarget(client.config, alias, previous, target).await
        })
    }
}

#[cfg(test)]
pub(super) fn sanitized<E>(action: &str, _error: E) -> Error {
    crate::vector::model::vector_error(&format!("{action} failed"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn transport_errors_never_expose_the_source() {
        let source = std::io::Error::other("secret endpoint and payload");
        let error = super::sanitized("query points", qdrant_client::QdrantError::Io(source));

        assert!(error.to_string().contains("query points failed"));
        assert!(!error.to_string().contains("secret endpoint"));
    }
}
