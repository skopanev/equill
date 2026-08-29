use super::qdrant::{QdrantTransport, Transport};
use super::test_support::{config, schema};
use qdrant_client::Qdrant;
use tokio::runtime::Builder;
use uuid::Uuid;

#[test]
fn transport_is_safe_inside_current_thread_runtime() {
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("outer runtime");
    runtime.block_on(async { exercise_transport() });
}

#[test]
fn transport_is_safe_inside_multi_thread_runtime() {
    let runtime = Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("outer runtime");
    runtime.block_on(async { exercise_transport() });
}

fn exercise_transport() {
    let transport = QdrantTransport::new(&config()).expect("lazy transport");
    let error = transport
        .collection_schema("equill_runtime_test")
        .expect_err("unreachable local endpoint");
    assert_eq!(
        error.to_string(),
        "projection failed: vector qdrant: read collection failed"
    );
    drop(transport);
}

#[test]
#[ignore = "requires EQUILL_QDRANT_E2E_ENDPOINT"]
fn live_transport_protocol_is_sequentially_safe() {
    let endpoint = std::env::var("EQUILL_QDRANT_E2E_ENDPOINT").expect("gated endpoint");
    let name = format!("equill_transport_probe_{}", Uuid::now_v7().simple());
    let mut config = config();
    config.endpoint = endpoint.clone();
    let expected = schema(&config);
    let transport = QdrantTransport::new(&config).expect("transport");
    assert_eq!(
        transport.collection_schema(&name).expect("first call"),
        None
    );
    transport
        .create_collection(&name, expected.clone())
        .expect("create");
    assert_eq!(
        transport.collection_schema(&name).expect("created schema"),
        Some(expected.clone())
    );
    assert_eq!(
        transport.collection_schema(&name).expect("second schema"),
        Some(expected)
    );
    drop(transport);
    delete_live_collection(endpoint, name);
}

fn delete_live_collection(endpoint: String, name: String) {
    Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("cleanup runtime")
        .block_on(async move {
            let client = Qdrant::from_url(&endpoint)
                .skip_compatibility_check()
                .build()
                .expect("cleanup client");
            client.delete_collection(name).await.expect("cleanup");
        });
}
