//! Rendering who may change a store: its owner, its writers, the actors held
//! to reading, and the grants in force.
use crate::governance::{AuthorityReport, GrantReport, OwnerReport, ReaderReport};

pub fn authority(report: &AuthorityReport) -> String {
    let grants = report
        .grants
        .iter()
        .map(|grant| {
            format!(
                "  {} -> {} {}",
                grant.actors.join(","),
                grant.namespace,
                grant.types.join(",")
            )
        })
        .collect::<Vec<_>>();
    let mut text = format!(
        "owner {}\nwriters {}",
        report.owner,
        if report.writers.is_empty() {
            "none".into()
        } else {
            report.writers.join(", ")
        }
    );
    if !report.read_only.is_empty() {
        text.push_str(&format!("\nread-only {}", report.read_only.join(", ")));
    }
    if !grants.is_empty() {
        text.push_str("\ngrants\n");
        text.push_str(&grants.join("\n"));
    }
    text
}

pub fn owner(report: &OwnerReport) -> String {
    let revoked = if report.revoked_writers.is_empty() {
        String::new()
    } else {
        format!(
            " — {} lost {}",
            report.previous_owner,
            report.revoked_writers.join(" and ")
        )
    };
    format!(
        "{} handed the store to {}{}",
        report.previous_owner, report.owner, revoked
    )
}

pub fn grant(report: &GrantReport) -> String {
    if report.changed {
        format!("{} — {} grants in force", report.actor, report.grants)
    } else {
        format!(
            "{} — unchanged, {} grants in force",
            report.actor, report.grants
        )
    }
}

pub fn reader(report: &ReaderReport) -> String {
    let state = if report.changed { "" } else { "unchanged, " };
    format!(
        "{} — {}{} actor(s) held to reading",
        report.actor, state, report.readers
    )
}
