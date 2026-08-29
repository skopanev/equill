use super::super::{
    DistanceMetric, Embedder, EmbeddingDescriptor, EmbeddingDocument, INPUT_SCHEMA, embed_batch,
};
use crate::kernel::error::Error;
use uuid::Uuid;

struct FakeEmbedder {
    descriptor: EmbeddingDescriptor,
    mode: Mode,
}

enum Mode {
    Deterministic,
    WrongDimensions,
    NonFinite,
}

impl Embedder for FakeEmbedder {
    fn descriptor(&self) -> &EmbeddingDescriptor {
        &self.descriptor
    }

    fn embed(&self, documents: &[EmbeddingDocument]) -> Result<Vec<Vec<f32>>, Error> {
        Ok(documents
            .iter()
            .map(|document| match self.mode {
                Mode::Deterministic => {
                    let sum = document.text.bytes().map(u32::from).sum::<u32>() as f32;
                    vec![sum, document.text.len() as f32, sum % 17.0]
                }
                Mode::WrongDimensions => vec![1.0, 2.0],
                Mode::NonFinite => vec![1.0, f32::NAN, 3.0],
            })
            .collect())
    }
}

#[test]
fn fake_embedder_is_deterministic_and_drops_source_text() {
    let embedder = fake(Mode::Deterministic);
    let documents = vec![document("private synthetic payload")];

    let first = embed_batch(&embedder, &documents).expect("embed");
    let second = embed_batch(&embedder, &documents).expect("repeat");

    assert_eq!(first, second);
    let serialized = serde_json::to_string(&first).expect("point JSON");
    assert!(!serialized.contains("private synthetic payload"));
}

#[test]
fn embedder_output_is_strictly_validated() {
    let documents = vec![document("synthetic")];

    let dimensions = embed_batch(&fake(Mode::WrongDimensions), &documents)
        .expect_err("reject dimensions")
        .to_string();
    let finite = embed_batch(&fake(Mode::NonFinite), &documents)
        .expect_err("reject NaN")
        .to_string();
    let mut invalid = fake(Mode::Deterministic);
    invalid.descriptor.input_schema = "unknown".into();
    let descriptor = embed_batch(&invalid, &documents)
        .expect_err("reject descriptor")
        .to_string();

    assert!(dimensions.contains("dimensions"));
    assert!(finite.contains("non-finite"));
    assert!(descriptor.contains("descriptor"));
}

fn fake(mode: Mode) -> FakeEmbedder {
    FakeEmbedder {
        descriptor: EmbeddingDescriptor {
            model_id: "synthetic-v1".into(),
            model_sha256: "a".repeat(64),
            tokenizer_sha256: "b".repeat(64),
            dimensions: 3,
            distance: DistanceMetric::Cosine,
            input_schema: INPUT_SCHEMA.into(),
        },
        mode,
    }
}

fn document(text: &str) -> EmbeddingDocument {
    EmbeddingDocument {
        record_id: Uuid::now_v7(),
        namespace: "agent.memory".into(),
        type_name: "agent.lesson.v1".into(),
        record_sha256: "c".repeat(64),
        input_sha256: "d".repeat(64),
        text: text.into(),
    }
}
