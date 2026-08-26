use super::model::{DefenseFinding, DefenseMode, DefensePolicy, DefenseResult};
use crate::kernel::error::Error;
use crate::record::RecordDraft;
use serde_json::Value;
use std::path::Path;

const REDACTED: &str = "[REDACTED BY EQUILL]";

pub fn apply(store_root: &Path, draft: &mut RecordDraft) -> Result<DefenseResult, Error> {
    let policy = super::policy::load(store_root)?;
    let mut findings = Vec::new();
    scan_value(&policy, &mut draft.payload, "/payload", &mut findings);
    for (index, evidence) in draft.evidence.iter_mut().enumerate() {
        scan_string(
            &policy,
            &mut evidence.kind,
            &format!("/evidence/{index}/kind"),
            &mut findings,
        );
        scan_string(
            &policy,
            &mut evidence.reference,
            &format!("/evidence/{index}/reference"),
            &mut findings,
        );
    }
    for (index, tag) in draft.tags.iter_mut().enumerate() {
        scan_string(
            &policy,
            tag,
            &format!("/tags/{index}"),
            &mut findings,
        );
    }
    Ok(DefenseResult {
        mode: policy.mode,
        findings,
    })
}

fn scan_value(
    policy: &DefensePolicy,
    value: &mut Value,
    path: &str,
    findings: &mut Vec<DefenseFinding>,
) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                let child_path = format!("{path}/{}", pointer_segment(key));
                if sensitive_key(policy, key) && !empty(child) {
                    findings.push(DefenseFinding {
                        path: child_path.clone(),
                        rule: format!("sensitive-key:{key}"),
                    });
                    if policy.mode == DefenseMode::Redact {
                        *child = Value::String(REDACTED.into());
                        continue;
                    }
                }
                scan_value(policy, child, &child_path, findings);
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter_mut().enumerate() {
                scan_value(policy, child, &format!("{path}/{index}"), findings);
            }
        }
        Value::String(text) => scan_string(policy, text, path, findings),
        _ => {}
    }
}

fn scan_string(
    policy: &DefensePolicy,
    text: &mut String,
    path: &str,
    findings: &mut Vec<DefenseFinding>,
) {
    let lowercase = text.to_ascii_lowercase();
    let mut matched = false;
    for pattern in &policy.literal_patterns {
        if lowercase.contains(&pattern.literal.to_ascii_lowercase()) {
            findings.push(DefenseFinding {
                path: path.to_owned(),
                rule: pattern.id.clone(),
            });
            matched = true;
        }
    }
    if matched && policy.mode == DefenseMode::Redact {
        *text = REDACTED.into();
    }
}

fn sensitive_key(policy: &DefensePolicy, key: &str) -> bool {
    policy
        .sensitive_keys
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(key))
}

fn empty(value: &Value) -> bool {
    matches!(value, Value::Null) || matches!(value, Value::String(text) if text.is_empty())
}

fn pointer_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
