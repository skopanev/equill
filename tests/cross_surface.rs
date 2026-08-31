//! The same question, asked three ways.
//!
//! A caller reaches context through the CLI as JSON, through the CLI as text,
//! or through an MCP session. Those are three surfaces over one selection, and
//! a difference between them is a difference nobody asked for: the same
//! request must choose the same records, whichever door it came through.
//!
//! Black box on purpose. Nothing here reaches into the matcher; every claim is
//! made from what a caller can actually see.
mod harness;
mod surfaces;

use surfaces::fixture::{ROLELESS, fixture};
use surfaces::{cli_json, cli_text, mcp};

#[test]
fn every_surface_selects_the_same_records_for_a_scalar_role() {
    let root = fixture("scalar");
    let asked = &["project=finik", "role=pm"];

    let json = cli_json(&root, asked);
    let session = mcp(&root, asked);
    let text = cli_text(&root, asked);

    assert_eq!(json, session, "CLI JSON and MCP disagree on a scalar role");
    assert_eq!(
        text, json.ids,
        "CLI text named a different set than the JSON answer"
    );
    assert!(!json.ids.is_empty(), "the fixture selected nothing at all");
    let _ = std::fs::remove_dir_all(root);
}

/// A request naming several roles. Written as one comma-separated coordinate,
/// which is how both the CLI and MCP express a set.
#[test]
fn every_surface_selects_the_same_records_for_a_role_set() {
    let root = fixture("set");
    for asked in [
        &["project=finik", "role=lane,backend"],
        &["project=finik", "role=lane,kyc"],
    ] {
        let json = cli_json(&root, asked);
        let session = mcp(&root, asked);
        let text = cli_text(&root, asked);

        assert_eq!(json, session, "CLI JSON and MCP disagree on {asked:?}");
        assert_eq!(
            text, json.ids,
            "CLI text named a different set than the JSON answer for {asked:?}"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}

/// A role nobody wrote. Every surface must answer with the roleless records
/// alone — and, whatever that number is, answer it identically.
#[test]
fn every_surface_agrees_on_a_role_no_record_carries() {
    let root = fixture("absent");
    let asked = &["project=finik", "role=nobody"];

    let json = cli_json(&root, asked);
    let session = mcp(&root, asked);

    assert_eq!(
        json, session,
        "the surfaces disagree about an unmatched role"
    );
    let _ = std::fs::remove_dir_all(root);
}

/// What the surfaces agree ON, not just that they agree.
///
/// Agreement is necessary and not sufficient: three surfaces over one matcher
/// agree on a wrong answer as readily as on a right one. This pins the answer
/// itself. The fixture writes two records with no role, one for each named
/// role, and one in another project.
///
/// A request naming one role must return the roleless records and that role.
/// A request naming several must return the roleless records and every named
/// role it asked for — a set is a list of alternatives, not a narrower filter.
#[test]
fn a_role_set_returns_every_role_it_names() {
    let root = fixture("membership");
    let roleless = ROLELESS;

    let pm = cli_json(&root, &["project=finik", "role=pm"]);
    let pair = cli_json(&root, &["project=finik", "role=lane,backend"]);
    let missing = cli_json(&root, &["project=finik", "role=lane,kyc"]);

    assert_eq!(
        pm.ids.len(),
        roleless + 1,
        "a scalar role must return the roleless records and its own"
    );
    assert_eq!(
        pair.ids.len(),
        roleless + 2,
        "a set naming two roles the store holds must return both, not neither"
    );
    assert_eq!(
        missing.ids.len(),
        roleless + 1,
        "a set naming one role the store holds and one it does not must return the one it holds"
    );
    let _ = std::fs::remove_dir_all(root);
}

/// The digest has to be worth comparing.
///
/// Two surfaces agreeing on a constant is not agreement. This asks two
/// different questions of the same store and requires the published digests to
/// differ — so the equality asserted above is an assertion about the answer
/// rather than about a field that never moves.
#[test]
fn the_published_digest_distinguishes_two_different_questions() {
    let root = fixture("digest");

    let pm = cli_json(&root, &["project=finik", "role=pm"]);
    let gm = cli_json(&root, &["project=finik", "role=gm"]);

    assert!(!pm.bundle_digest.is_empty(), "no digest was published");
    assert_ne!(
        pm.bundle_digest, gm.bundle_digest,
        "two different selections published the same digest"
    );
    assert_eq!(
        pm.bundle_digest,
        mcp(&root, &["project=finik", "role=pm"]).bundle_digest,
        "CLI and MCP published different digests for one request"
    );
    let _ = std::fs::remove_dir_all(root);
}

/// A matcher that widens passes every test about what it returns, and fails
/// only a test about what it does not.
///
/// Asking for one role must not drag in another. Checked through every door,
/// because a control that only ran on one surface would leave the others free
/// to widen quietly.
#[test]
fn asking_for_one_role_does_not_return_another() {
    let root = fixture("control");
    let pm = cli_json(&root, &["project=finik", "role=pm"]);
    let gm = cli_json(&root, &["project=finik", "role=gm"]);

    // The roleless records are universal and belong to both answers; the named
    // ones must not cross over.
    let only_pm: Vec<_> = pm.ids.difference(&gm.ids).cloned().collect();
    let only_gm: Vec<_> = gm.ids.difference(&pm.ids).cloned().collect();
    assert_eq!(only_pm.len(), 1, "asking for pm returned {only_pm:?}");
    assert_eq!(only_gm.len(), 1, "asking for gm returned {only_gm:?}");
    assert_eq!(
        pm.ids.intersection(&gm.ids).count(),
        ROLELESS,
        "the two answers share something other than the roleless records"
    );
    // And the same through the other two doors, so no surface widens alone.
    assert_eq!(mcp(&root, &["project=finik", "role=pm"]), pm);
    assert_eq!(cli_text(&root, &["project=finik", "role=pm"]), pm.ids);
    let _ = std::fs::remove_dir_all(root);
}
