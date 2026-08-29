use super::model::{
    EmbeddingDescriptor, EmbeddingDocument, VectorPoint, validate_descriptor, validate_vector,
};
use crate::kernel::error::Error;

pub trait Embedder {
    fn descriptor(&self) -> &EmbeddingDescriptor;

    fn embed(&self, documents: &[EmbeddingDocument]) -> Result<Vec<Vec<f32>>, Error>;
}

pub fn embed_batch(
    embedder: &impl Embedder,
    documents: &[EmbeddingDocument],
) -> Result<Vec<VectorPoint>, Error> {
    let descriptor = embedder.descriptor();
    validate_descriptor(descriptor)?;
    let vectors = embedder.embed(documents)?;
    if vectors.len() != documents.len() {
        return Err(super::model::vector_error(
            "embedder returned the wrong batch size",
        ));
    }
    documents
        .iter()
        .zip(vectors)
        .map(|(document, vector)| {
            validate_vector(&vector, descriptor.dimensions)?;
            Ok(VectorPoint {
                record_id: document.record_id,
                namespace: document.namespace.clone(),
                type_name: document.type_name.clone(),
                record_sha256: document.record_sha256.clone(),
                input_sha256: document.input_sha256.clone(),
                vector,
            })
        })
        .collect()
}
