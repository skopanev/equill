use super::OllamaRuntime;
use crate::vector::config::{EmbeddingConfig, OllamaEmbeddingConfig, OllamaProvider, VectorConfig};
use crate::vector::model::{DistanceMetric, INPUT_SCHEMA};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use uuid::Uuid;

#[test]
fn pinned_model_embeds_a_prefixed_query_through_loopback() {
    let digest = "a".repeat(64);
    let (endpoint, received) = server(&digest);
    let config = config(endpoint, digest);
    let EmbeddingConfig::Ollama(embedding) = &config.embedding else {
        panic!("ollama config");
    };

    let runtime = OllamaRuntime::load(&config, embedding).expect("load provider");
    let vector = runtime
        .embed_query("how to verify a change")
        .expect("embed");
    let request = received.recv().expect("request body");

    assert_eq!(vector, vec![0.1, 0.2, 0.3]);
    assert_eq!(request["dimensions"], 3);
    assert_eq!(request["truncate"], false);
    assert_eq!(request["keep_alive"], -1);
    assert!(
        request["input"][0]
            .as_str()
            .expect("query")
            .starts_with("Instruct:")
    );
}

fn config(endpoint: String, digest: String) -> VectorConfig {
    VectorConfig {
        schema: "equill.qdrant-config.v1".into(),
        enabled: true,
        endpoint: "http://127.0.0.1:6334".into(),
        collection_alias: "equill_ollama_test".into(),
        store_id: Uuid::now_v7(),
        dimensions: 3,
        distance: DistanceMetric::Cosine,
        embedding: EmbeddingConfig::Ollama(OllamaEmbeddingConfig {
            provider: OllamaProvider::Ollama,
            endpoint,
            model_id: "qwen-test:8b-q8_0".into(),
            model_sha256: digest,
            input_schema: INPUT_SCHEMA.into(),
        }),
        api_key_env: None,
        allow_remote: false,
    }
}

fn server(digest: &str) -> (String, mpsc::Receiver<serde_json::Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    let digest = digest.to_owned();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut tags, _) = listener.accept().expect("tags connection");
        let _ = request(&mut tags);
        reply(
            &mut tags,
            &serde_json::json!({"models": [{
                "name": "qwen-test:8b-q8_0",
                "model": "qwen-test:8b-q8_0",
                "digest": digest
            }]}),
        );
        let (mut embed, _) = listener.accept().expect("embed connection");
        let body = request(&mut embed);
        sender.send(body).expect("capture");
        reply(
            &mut embed,
            &serde_json::json!({"embeddings": [[0.1, 0.2, 0.3]]}),
        );
    });
    (endpoint, receiver)
}

fn request(stream: &mut TcpStream) -> serde_json::Value {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream.read(&mut buffer).expect("read request");
        bytes.extend_from_slice(&buffer[..read]);
        let Some(split) = bytes.windows(4).position(|part| part == b"\r\n\r\n") else {
            continue;
        };
        let header = String::from_utf8_lossy(&bytes[..split]);
        let length = header
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length: ")
                    .map(str::to_owned)
            })
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or_default();
        if bytes.len() >= split + 4 + length {
            return if length > 0 {
                serde_json::from_slice(&bytes[split + 4..split + 4 + length]).expect("JSON")
            } else {
                serde_json::Value::Null
            };
        }
    }
}

fn reply(stream: &mut TcpStream, body: &serde_json::Value) {
    let body = serde_json::to_vec(body).expect("response JSON");
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .expect("headers");
    stream.write_all(&body).expect("body");
}
