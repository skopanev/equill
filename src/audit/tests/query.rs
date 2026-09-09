use super::super::{
    Scope, list_at, stats_at,
    tests::{event, root},
    writer,
};
use std::fs;

#[test]
fn identical_scope_across_rotation_filters_and_rebuilt_indexes() {
    let root = root();
    let first = event("2026-01-31T23:59:59Z");
    let mut second = event("2026-02-01T00:00:00Z");
    second.outcome = "error".into();
    second.error_class = Some("validation".into());
    second.duration_us = 480;
    let mut excluded = event("2026-02-01T00:00:01Z");
    excluded.project = Some("other".into());
    for item in [&first, &second, &excluded] {
        writer::append(&root, item).expect("append");
    }
    let scope = Scope {
        project: Some("demo".into()),
        ..Scope::default()
    };
    let listing = list_at(&root, &scope, 100).expect("list");
    assert_eq!(listing.events, vec![second.clone(), first.clone()]);
    assert_eq!(
        list_at(&root, &scope, 1).expect("limited").events,
        vec![second.clone()]
    );
    let stats = stats_at(&root, &scope).expect("stats");
    assert_eq!(stats.count, 2);
    assert_eq!(stats.failures, 1);
    assert_eq!(stats.error_rate, 0.5);
    assert_eq!(stats.duration_min_us, 120);
    assert_eq!(stats.duration_max_us, 480);
    assert_eq!(stats.duration_mean_us, 300.0);
    assert_eq!(stats.duration_p95_us, 480);
    for corrupt in [false, true] {
        if corrupt {
            fs::write(root.join("index.sqlite3"), b"corrupt").expect("corrupt projection");
        } else {
            fs::remove_file(root.join("index.sqlite3")).expect("drop projection");
        }
        assert_eq!(
            list_at(&root, &scope, 100).expect("rebuilt list").events,
            listing.events
        );
        assert_eq!(stats_at(&root, &scope).expect("rebuilt stats"), stats);
    }
    let exact = Scope {
        since: Some(first.observed_at.clone()),
        until: Some(excluded.observed_at.clone()),
        surface: Some("cli".into()),
        operation: Some("status".into()),
        project: Some("demo".into()),
        role: Some("reviewer".into()),
        process: Some("equill".into()),
        outcome: Some("error".into()),
        instance: Some("instance-a".into()),
        session: Some("session-a".into()),
        actor: Some("owner".into()),
        lane: Some("lane-a".into()),
    };
    assert_eq!(
        list_at(&root, &exact, 100).expect("exact scope").events,
        vec![second]
    );
    assert_eq!(stats_at(&root, &exact).expect("same scope").count, 1);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn every_scope_field_excludes_mismatches_and_time_boundaries_are_numeric() {
    let root = root();
    writer::append(&root, &event("2026-01-01T00:00:00.1Z")).expect("append");
    let mut scopes = Vec::new();
    for field in 0..10 {
        let mut scope = Scope::default();
        let target = match field {
            0 => &mut scope.surface,
            1 => &mut scope.operation,
            2 => &mut scope.project,
            3 => &mut scope.role,
            4 => &mut scope.process,
            5 => &mut scope.outcome,
            6 => &mut scope.instance,
            7 => &mut scope.session,
            8 => &mut scope.actor,
            _ => &mut scope.lane,
        };
        *target = Some("different".into());
        scopes.push(scope);
    }
    for scope in scopes {
        assert!(list_at(&root, &scope, 10).expect("list").events.is_empty());
        assert_eq!(stats_at(&root, &scope).expect("stats").count, 0);
    }
    let scope = Scope {
        since: Some("2026-01-01T00:00:00Z".into()),
        until: Some("2026-01-01T00:00:00.2Z".into()),
        ..Scope::default()
    };
    assert_eq!(stats_at(&root, &scope).expect("fractional time").count, 1);
    assert!(list_at(&root, &Scope::default(), 0).is_err());
    let scope = Scope {
        since: Some("invalid".into()),
        ..Scope::default()
    };
    assert!(stats_at(&root, &scope).is_err());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn submicrosecond_bounds_and_order_remain_exact_across_epoch() {
    for second in [
        "0001-01-01T00:00:00",
        "1969-12-31T23:59:59",
        "2026-01-01T00:00:00",
        "9999-01-01T00:00:00",
    ] {
        let root = root();
        // Reverse append order makes UUID order disagree with event time.
        let later = event(&format!("{second}.000000002Z"));
        let earlier = event(&format!("{second}.000000001Z"));
        writer::append(&root, &later).expect("later");
        writer::append(&root, &earlier).expect("earlier");
        let scope = Scope {
            since: Some(earlier.observed_at.clone()),
            until: Some(later.observed_at.clone()),
            ..Scope::default()
        };
        assert_eq!(
            list_at(&root, &scope, 10).expect("exact interval").events,
            vec![earlier.clone()]
        );
        assert_eq!(stats_at(&root, &scope).expect("same interval").count, 1);
        assert_eq!(
            list_at(&root, &Scope::default(), 10).expect("order").events,
            vec![later, earlier]
        );
        fs::remove_dir_all(root).expect("cleanup");
    }
}
