//! Release-only durable confirmation through finite real MCP sessions.
//! Cold means the first record in a fresh store/session; warm keeps the session.
mod harness;

#[cfg(not(debug_assertions))]
mod release {
    use super::harness;
    use harness::confirmation::{CALLS, Timings, draft, failure_mark, stalled, warm, write};
    use harness::provider::SlowProvider;
    use harness::session::Session;
    use harness::{exclusive_measurement, settles, store_against};

    const HISTORY: usize = 400;

    #[test]
    fn cold_and_warm_confirmations_stay_under_the_ceiling() {
        let _measuring = exclusive_measurement();
        let mut cold = Vec::with_capacity(CALLS);
        let mut warmed = None;
        for index in 0..CALLS {
            let provider = SlowProvider::start();
            let root = store_against("confirm-cold", &provider.endpoint());
            let mut session = Session::open(&root);
            let mark = failure_mark(&root);
            cold.push(write(&mut session, &root, 0));
            stalled(&root, &provider, mark);
            if index == CALLS - 1 {
                warmed = Some(warm(&mut session, &root, mark, 1));
            }
            drop(session);
            provider.release();
            assert!(settles(&root, harness::WORKER_PATIENCE), "worker cleanup");
            std::fs::remove_dir_all(root).expect("fixture cleanup");
        }
        let cold = Timings::new(cold);
        let warmed = warmed.expect("warm sample");
        cold.report("cold record");
        warmed.report("warm record");
        cold.check();
        warmed.check();
    }

    #[test]
    fn confirmation_does_not_get_slower_as_the_store_grows() {
        let _measuring = exclusive_measurement();
        let measure = |history| {
            let provider = SlowProvider::start();
            let root = store_against("confirm-history", &provider.endpoint());
            // Setup seeds durable truth only: no failed worker or cooldown
            // from seeding can make the measured stalled-worker proof vacuous.
            for index in 0..history {
                equill::record::append_only(
                    &root,
                    serde_json::from_value(draft(index)).expect("seed draft"),
                    "owner",
                )
                .expect("seed durable record");
            }
            let mut session = Session::open(&root);
            let mark = failure_mark(&root);
            let first = write(&mut session, &root, history);
            stalled(&root, &provider, mark);
            let rest = warm(&mut session, &root, mark, history + 1);
            eprintln!("history={history} first record: {first:?}");
            drop(session);
            provider.release();
            assert!(settles(&root, harness::WORKER_PATIENCE), "worker cleanup");
            std::fs::remove_dir_all(root).expect("fixture cleanup");
            assert!(
                first <= harness::confirmation::CEILING,
                "cold history call exceeded ceiling"
            );
            rest
        };
        let empty = measure(0);
        let loaded = measure(HISTORY);
        empty.report("on an empty store");
        loaded.report("on a store with history");
        empty.check();
        loaded.check();
        let ratio = loaded.p50.as_secs_f64() / empty.p50.as_secs_f64().max(f64::EPSILON);
        eprintln!("p50 ratio aged/empty: {ratio:.2}");
        assert!(
            ratio <= 1.5,
            "confirmation cost grows with history: {ratio:.2}x"
        );
    }
}
