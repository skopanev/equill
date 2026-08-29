use qdrant_client::config::QdrantConfig;
use qdrant_client::qdrant as api;
use qdrant_client::qdrant::collections_client::CollectionsClient;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::service::Interceptor;
use tonic::transport::{ClientTlsConfig, Endpoint};
use tonic::{Request, Status};

#[derive(Clone)]
struct ApiKey(Option<MetadataValue<Ascii>>);

impl Interceptor for ApiKey {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        if let Some(value) = &self.0 {
            request.metadata_mut().insert("api-key", value.clone());
        }
        Ok(request)
    }
}

pub(super) async fn retarget(
    config: QdrantConfig,
    alias: String,
    previous: Option<String>,
    target: Option<String>,
) -> Result<(), ()> {
    let key = config
        .api_key
        .as_deref()
        .map(str::parse::<MetadataValue<Ascii>>)
        .transpose()
        .map_err(|_| ())?;
    let endpoint = Endpoint::from_shared(config.uri)
        .map_err(|_| ())?
        .connect_timeout(config.connect_timeout)
        .timeout(config.timeout);
    let endpoint = if endpoint.uri().scheme_str() == Some("https") {
        endpoint
            .tls_config(ClientTlsConfig::new().with_native_roots())
            .map_err(|_| ())?
    } else {
        endpoint
    };
    let channel = endpoint.connect_lazy();
    let mut client = CollectionsClient::with_interceptor(channel, ApiKey(key));
    let mut actions = Vec::with_capacity(2);
    if previous.is_some() {
        actions.push(operation(api::alias_operations::Action::DeleteAlias(
            api::DeleteAlias {
                alias_name: alias.clone(),
            },
        )));
    }
    if let Some(collection_name) = target {
        actions.push(operation(api::alias_operations::Action::CreateAlias(
            api::CreateAlias {
                collection_name,
                alias_name: alias,
            },
        )));
    }
    if actions.is_empty() {
        return Ok(());
    }
    client
        .update_aliases(api::ChangeAliases {
            actions,
            timeout: Some(config.timeout.as_secs().max(1)),
        })
        .await
        .map_err(|_| ())?;
    Ok(())
}

fn operation(action: api::alias_operations::Action) -> api::AliasOperations {
    api::AliasOperations {
        action: Some(action),
    }
}
