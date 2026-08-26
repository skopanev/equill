use crate::kernel::error::Error;
use secrets_scanner::{Finding, ScanConfig, Scanner};
use std::sync::OnceLock;

const BUNDLED_RULES: &str = include_str!("patterns.toml");
static BUNDLED: OnceLock<Scanner> = OnceLock::new();
static DEEP: OnceLock<Scanner> = OnceLock::new();

#[derive(Debug)]
pub struct Match {
    pub rule: String,
    pub line: usize,
    pub column: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug)]
pub struct Scan {
    pub matches: Vec<Match>,
}

pub fn scan_inline(content: &str) -> Result<Scan, Error> {
    scan(bundled()?, content)
}

pub fn scan_deep(content: &str) -> Result<Scan, Error> {
    scan(deep()?, content)
}

pub fn scan_custom(rules: &str, content: &str) -> Result<Scan, Error> {
    let scanner = Scanner::from_toml(rules)
        .map_err(|error| Error::MemoryDefense(format!("store rules: {error}")))?
        .with_config(ScanConfig::proxy());
    scan(&scanner, content)
}

fn scan(scanner: &Scanner, content: &str) -> Result<Scan, Error> {
    let output = scanner
        .scan_proxy(content.as_bytes())
        .map_err(|error| Error::MemoryDefense(format!("scanner rejected input: {error}")))?;
    if output.findings_truncated {
        return Err(Error::MemoryDefense(
            "finding limit reached; refusing an incomplete receipt".into(),
        ));
    }
    Ok(Scan {
        matches: output.findings.into_iter().map(convert).collect(),
    })
}

fn bundled() -> Result<&'static Scanner, Error> {
    if let Some(scanner) = BUNDLED.get() {
        return Ok(scanner);
    }
    let scanner = Scanner::from_toml(BUNDLED_RULES)
        .map_err(|error| Error::MemoryDefense(format!("bundled rules: {error}")))?
        .with_config(ScanConfig::proxy());
    let _ = BUNDLED.set(scanner);
    BUNDLED
        .get()
        .ok_or_else(|| Error::MemoryDefense("bundled scanner did not initialize".into()))
}

fn deep() -> Result<&'static Scanner, Error> {
    if let Some(scanner) = DEEP.get() {
        return Ok(scanner);
    }
    let scanner = Scanner::from_bundled()
        .map_err(|error| Error::MemoryDefense(format!("deep catalog: {error}")))?
        .with_config(ScanConfig::proxy());
    let _ = DEEP.set(scanner);
    DEEP.get()
        .ok_or_else(|| Error::MemoryDefense("deep scanner did not initialize".into()))
}

fn convert(finding: Finding) -> Match {
    Match {
        rule: finding.rule_id,
        line: finding.line,
        column: finding.col,
        start: finding.secret_start_offset,
        end: finding.secret_end_offset,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn deep_catalog_detects_a_generated_provider_token() {
        let alphabet = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let tail: String = (0..36)
            .map(|index| alphabet[(index * 17) % alphabet.len()] as char)
            .collect();
        let token = format!("{}{}", "ghp_", tail);
        let result = super::scan_deep(&token).expect("scan with full catalog");

        assert!(!result.matches.is_empty());
    }
}
