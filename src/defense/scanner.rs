use super::model::{DefenseFinding, DefenseMode, DefenseResult};
use super::provider::secrets_scanner::{self, Match};
use crate::kernel::error::Error;
use crate::record::{EvidenceRef, RecordDraft};
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

#[derive(Deserialize)]
struct ScannedContent {
    payload: Value,
    evidence: Vec<EvidenceRef>,
    tags: Vec<String>,
}

pub fn apply(store_root: &Path, draft: &mut RecordDraft) -> Result<DefenseResult, Error> {
    let policy = super::policy::load(store_root)?;
    let content = serde_json::to_string(&serde_json::json!({
        "payload": &draft.payload,
        "evidence": &draft.evidence,
        "tags": &draft.tags,
    }))?;
    let mut matches = secrets_scanner::scan_bundled(&content)?.matches;
    if let Some(rules) = super::policy::custom_rules(store_root)? {
        matches.extend(secrets_scanner::scan_custom(&rules, &content)?.matches);
    }
    let findings = findings(&matches);
    if policy.mode == DefenseMode::Redact && !findings.is_empty() {
        let redacted = redact(&content, matches)?;
        let sanitized: ScannedContent = serde_json::from_str(&redacted).map_err(|_| {
            Error::MemoryDefense("redaction produced an invalid record body".into())
        })?;
        draft.payload = sanitized.payload;
        draft.evidence = sanitized.evidence;
        draft.tags = sanitized.tags;
    }
    Ok(DefenseResult {
        mode: policy.mode,
        findings,
    })
}

fn findings(matches: &[Match]) -> Vec<DefenseFinding> {
    matches
        .iter()
        .map(|item| DefenseFinding {
            rule: item.rule.clone(),
            line: item.line,
            column: item.column,
        })
        .collect()
}

fn redact(content: &str, mut matches: Vec<Match>) -> Result<String, Error> {
    matches.sort_by_key(|item| (item.start, std::cmp::Reverse(item.end)));
    let mut ranges: Vec<(usize, usize, String)> = Vec::new();
    for item in matches {
        if item.start >= item.end
            || item.end > content.len()
            || !content.is_char_boundary(item.start)
            || !content.is_char_boundary(item.end)
        {
            return Err(Error::MemoryDefense(
                "scanner returned an invalid redaction range".into(),
            ));
        }
        if let Some(previous) = ranges.last_mut() {
            if item.start < previous.1 {
                previous.1 = previous.1.max(item.end);
                if previous.2 != item.rule {
                    previous.2.push('+');
                    previous.2.push_str(&item.rule);
                }
                continue;
            }
        }
        ranges.push((item.start, item.end, item.rule));
    }
    let mut output = content.to_owned();
    for (start, end, rule) in ranges.into_iter().rev() {
        let marker = format!("[REDACTED:{}]", safe_rule(&rule));
        output.replace_range(start..end, &marker);
    }
    Ok(output)
}

fn safe_rule(rule: &str) -> String {
    rule.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}
