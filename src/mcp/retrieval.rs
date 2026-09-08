use crate::kernel::error::Error;
use serde_json::Value;

pub(super) fn overrides(arguments: &Value) -> Result<crate::retrieval::Overrides, Error> {
    Ok(crate::retrieval::Overrides {
        query_instruction: string(arguments, "query_instruction")?,
        vector_enabled: boolean(arguments, "vector_enabled")?,
        vector_score_threshold: number(arguments, "vector_score_threshold")?,
        hybrid_order: order(arguments)?,
        hybrid_fill_remaining: boolean(arguments, "hybrid_fill_remaining")?,
        hybrid_deduplicate: boolean(arguments, "hybrid_deduplicate")?,
    })
}

fn string(arguments: &Value, key: &str) -> Result<Option<String>, Error> {
    let Some(value) = arguments.get(key) else {
        return Ok(None);
    };
    value
        .as_str()
        .map(|value| Some(value.to_owned()))
        .ok_or_else(|| malformed(key))
}

fn boolean(arguments: &Value, key: &str) -> Result<Option<bool>, Error> {
    optional(arguments, key, Value::as_bool)
}

fn number(arguments: &Value, key: &str) -> Result<Option<f32>, Error> {
    optional(arguments, key, Value::as_f64).map(|value| value.map(|number| number as f32))
}

fn order(arguments: &Value) -> Result<Option<[crate::retrieval::Source; 2]>, Error> {
    let Some(value) = arguments.get("hybrid_order") else {
        return Ok(None);
    };
    let values = value.as_array().ok_or_else(|| malformed("hybrid_order"))?;
    if values.len() != 2 {
        return Err(malformed("hybrid_order"));
    }
    Ok(Some([source(&values[0])?, source(&values[1])?]))
}

fn source(value: &Value) -> Result<crate::retrieval::Source, Error> {
    match value.as_str() {
        Some("vector") => Ok(crate::retrieval::Source::Vector),
        Some("fts") => Ok(crate::retrieval::Source::Fts),
        _ => Err(malformed("hybrid_order")),
    }
}

fn optional<T: Copy>(
    arguments: &Value,
    key: &str,
    read: impl FnOnce(&Value) -> Option<T>,
) -> Result<Option<T>, Error> {
    arguments
        .get(key)
        .map(|value| read(value).ok_or_else(|| malformed(key)))
        .transpose()
}

fn malformed(key: &str) -> Error {
    Error::InvalidSettings(format!("{key} has the wrong shape"))
}

#[cfg(test)]
mod tests {
    use super::overrides;
    use serde_json::json;

    #[test]
    fn explicit_mcp_retrieval_options_are_parsed() {
        let values = overrides(&json!({
            "query_instruction": "Find exact guidance",
            "vector_enabled": false,
            "vector_score_threshold": 0.75,
            "hybrid_order": ["fts", "vector"],
            "hybrid_fill_remaining": false,
            "hybrid_deduplicate": true
        }))
        .expect("overrides");

        assert_eq!(
            values.query_instruction.as_deref(),
            Some("Find exact guidance")
        );
        assert_eq!(values.vector_enabled, Some(false));
        assert_eq!(values.vector_score_threshold, Some(0.75));
        assert_eq!(
            values.hybrid_order,
            Some([
                crate::retrieval::Source::Fts,
                crate::retrieval::Source::Vector
            ])
        );
    }
}
