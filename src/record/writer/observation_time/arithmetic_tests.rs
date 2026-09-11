use super::validate;
use jiff::Timestamp;

#[test]
fn nanosecond_boundary_and_historical_observations() {
    let reference = "2026-01-01T00:00:00Z";
    for observation in [
        "1900-01-01T00:00:00Z",
        reference,
        "2026-01-01T00:00:59.999999999Z",
        "2026-01-01T00:01:00Z",
        "2026-01-01T01:01:00+01:00",
    ] {
        validate(observation, reference).expect(observation);
    }
    for observation in [
        "2026-01-01T00:01:00.000000001Z",
        "2026-01-01T00:05:00Z",
        "2026-01-01T01:01:00.000000001+01:00",
    ] {
        let error = validate(observation, reference).unwrap_err().to_string();
        assert!(error.contains("more than 60 seconds"));
        assert!(!error.contains(observation));
    }
}

#[test]
fn extreme_timestamps_and_malformed_input_do_not_panic() {
    let min = Timestamp::MIN.to_string();
    let max = Timestamp::MAX.to_string();
    validate(&min, &max).unwrap();
    validate(&max, &max).unwrap();
    validate(&min, &min).unwrap();
    assert!(validate(&max, &min).is_err());
    assert!(validate("not a timestamp", &max).is_err());
    assert!(validate(&min, "not a timestamp").is_err());
}

#[test]
fn clock_movement_is_decided_only_by_the_captured_writer_time() {
    let observation = "2026-01-01T00:01:00Z";
    validate(observation, "2026-01-01T00:00:00Z").unwrap();
    assert!(validate(observation, "2025-12-31T23:59:59.999999999Z").is_err());
    validate(observation, "2026-01-01T01:00:00Z").unwrap();
}
