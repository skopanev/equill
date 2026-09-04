//! One pinned, local tokenizer for reproducible context accounting.
use super::model::TokenizerCoordinate;
use crate::kernel::error::Error;

pub fn validate(coordinate: &TokenizerCoordinate) -> Result<(), Error> {
    let expected = TokenizerCoordinate::default();
    if coordinate != &expected {
        return Err(Error::Context(format!(
            "unsupported context tokenizer {}@{}; expected {}@{}",
            coordinate.id, coordinate.version, expected.id, expected.version
        )));
    }
    Ok(())
}

pub fn count(text: &str, coordinate: &TokenizerCoordinate) -> Result<usize, Error> {
    validate(coordinate)?;
    Ok(tiktoken_rs::o200k_base_singleton().count_ordinary(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_tokenizer_counts_structure_and_unicode_locally() {
        let coordinate = TokenizerCoordinate::default();
        assert_eq!(count("## ROLE\n- Ship", &coordinate).expect("tokens"), 5);
        assert_eq!(
            count("проверить контекст", &coordinate).expect("tokens"),
            count("проверить контекст", &coordinate).expect("tokens")
        );
    }

    #[test]
    fn an_unknown_tokenizer_is_refused() {
        let coordinate = TokenizerCoordinate {
            id: "other".into(),
            version: "1".into(),
        };
        assert!(count("text", &coordinate).is_err());
    }
}
