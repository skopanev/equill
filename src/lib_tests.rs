#[test]
fn doctor_is_machine_readable() {
    let output = super::run(["equill", "doctor", "--json"]).expect("doctor output");
    let value: serde_json::Value = serde_json::from_str(&output).expect("valid json");

    assert_eq!(value["ok"], true);
    assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));

    let human = super::run(["equill", "doctor"]).expect("human doctor output");
    assert!(human.starts_with("Equill doctor (quick) — OK"));
}
