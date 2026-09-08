//! Session coordinates inherited by the local MCP adapter.
use super::arguments::{optional, strings};
use serde_json::Value;

const FALLBACKS: [(&str, &str); 4] = [
    ("project", "EQUILL_PROJECT"),
    ("role", "EQUILL_ROLE"),
    ("process", "EQUILL_PROCESS"),
    ("rules", "EQUILL_RULES"),
];

pub(super) fn context_coordinates(arguments: &Value) -> Vec<String> {
    with_environment(arguments, |name| std::env::var(name).ok())
}

fn with_environment(
    arguments: &Value,
    environment: impl Fn(&str) -> Option<String>,
) -> Vec<String> {
    let mut coordinates = strings(arguments, "coordinates");
    for key in ["project", "role", "phase", "harness", "process"] {
        if let Some(value) = optional(arguments, key) {
            coordinates.push(format!("{key}={value}"));
        }
    }
    for (key, variable) in FALLBACKS {
        if !has(&coordinates, key)
            && let Some(value) = environment(variable).filter(|value| !value.is_empty())
        {
            coordinates.push(format!("{key}={value}"));
        }
    }
    coordinates
}

fn has(coordinates: &[String], wanted: &str) -> bool {
    coordinates
        .iter()
        .any(|entry| entry.split_once('=').is_some_and(|(key, _)| key == wanted))
}

#[cfg(test)]
mod tests {
    use super::with_environment;
    use crate::context::inline_request;
    use serde_json::json;
    use std::collections::HashMap;

    fn request(arguments: serde_json::Value, values: &[(&str, &str)]) -> serde_json::Value {
        let environment = values.iter().copied().collect::<HashMap<_, _>>();
        let coordinates = with_environment(&arguments, |key| {
            environment.get(key).map(|value| (*value).to_owned())
        });
        let coordinates = inline_request(None, coordinates, Vec::new(), Vec::new(), None, false)
            .expect("request")
            .coordinates;
        serde_json::to_value(coordinates).expect("coordinates json")
    }

    #[test]
    fn context_inherits_the_mcp_launch_coordinates() {
        let coordinates = request(
            json!({ "role": "operator" }),
            &[
                ("EQUILL_ROLE", "operator"),
                ("EQUILL_PROCESS", "daily-review"),
                ("EQUILL_RULES", "alpha,beta"),
            ],
        );

        assert_eq!(coordinates["role"], json!("operator"));
        assert_eq!(coordinates["process"], json!("daily-review"));
        assert_eq!(coordinates["rules"], json!(["alpha", "beta"]));
    }

    #[test]
    fn explicit_coordinates_override_the_mcp_launch_environment() {
        let coordinates = request(
            json!({
                "process": "review",
                "coordinates": ["project=sample", "rules=code"]
            }),
            &[
                ("EQUILL_PROJECT", "other"),
                ("EQUILL_PROCESS", "daily-review"),
                ("EQUILL_RULES", "alpha,beta"),
            ],
        );

        assert_eq!(coordinates["project"], json!("sample"));
        assert_eq!(coordinates["process"], json!("review"));
        assert_eq!(coordinates["rules"], json!("code"));
    }
}
