use std::{
    io::Write,
    process::{Command, Stdio},
};
fn run(input: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ingress-gateway-bridge-rs"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}
#[test]
fn cli_contract() {
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!("cases.json")).unwrap();
    for case in cases {
        let output = run(&case["input"].to_string());
        let code = match case["expected"].as_str().unwrap() {
            "ok" => 0,
            "rejected" => 2,
            "dependency" => 3,
            _ => panic!(),
        };
        assert_eq!(output.status.code(), Some(code), "{}", case["name"]);
        assert!(output.stderr.is_empty());
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap();
    }
    let output = run("not json");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let mut input: serde_json::Value =
        serde_json::from_str(include_str!("../examples/basic.json")).unwrap();
    input["binding"]["parentRefs"] = serde_json::json!([]);
    assert_eq!(run(&input.to_string()).status.code(), Some(1));
}
