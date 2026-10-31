// SPDX-License-Identifier: Apache-2.0
use ingress_gateway_bridge_rs::{Error, class_name, convert, model::Input, route_name};
use serde_json::{Value, json};
#[test]
fn shared_scenarios() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("cases.json")).unwrap();
    for case in cases {
        let input: Input = serde_json::from_value(case["input"].clone()).unwrap();
        let result = class_name(&input.ingress).and_then(|_| convert(&input));
        match case["expected"].as_str().unwrap() {
            "ok" => {
                assert!(
                    !result
                        .unwrap_or_else(|e| panic!("{}: {e}", case["name"]))
                        .is_empty()
                );
            }
            "rejected" => assert!(
                matches!(result, Err(Error::Rejected(_))),
                "{}: {result:?}",
                case["name"]
            ),
            "dependency" => assert!(
                matches!(result, Err(Error::Dependency(_))),
                "{}: {result:?}",
                case["name"]
            ),
            _ => panic!("unknown expectation"),
        }
    }
}
#[test]
fn output_semantics() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("cases.json")).unwrap();
    let output = |name: &str| {
        let case = cases.iter().find(|c| c["name"] == name).unwrap();
        convert(&serde_json::from_value(case["input"].clone()).unwrap()).unwrap()
    };
    assert_eq!(
        output("named port")[0]["spec"]["rules"][0]["backendRefs"][0]["port"],
        8080
    );
    assert_eq!(
        output("Prefix/wp-admin/")[0]["spec"]["rules"][0]["matches"][0]["path"]["value"],
        "/wp-admin"
    );
    assert_eq!(
        output("Exact/exact/")[0]["spec"]["rules"][0]["matches"][0]["path"]["value"],
        "/exact/"
    );
    assert_eq!(
        output("duplicate collapsed")[0]["spec"]["rules"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(output("foreign annotation"), output("basic"));
    assert!(output("hostless")[0]["spec"].get("hostnames").is_none());
    let hosts = output("separate hosts");
    assert_eq!(hosts.len(), 2);
    assert_eq!(hosts[0]["spec"]["hostnames"], json!(["admin.example.com"]));
    assert_eq!(hosts[0]["spec"]["rules"].as_array().unwrap().len(), 1);
    assert_eq!(
        hosts[0]["spec"]["rules"][0]["backendRefs"][0]["name"],
        "admin"
    );
    assert_eq!(
        hosts[0]["metadata"]["ownerReferences"][0]["blockOwnerDeletion"],
        false
    );
    assert_ne!(route_name("website", "a"), route_name("website", "b"));
    assert!(route_name(&"a".repeat(253), "a").len() <= 63);
    let case = cases
        .iter()
        .find(|c| c["name"] == "unknown annotations redacted")
        .unwrap();
    let error = convert(&serde_json::from_value(case["input"].clone()).unwrap())
        .unwrap_err()
        .to_string();
    assert!(!error.contains("secret"));
    assert!(error.find("/a").unwrap() < error.find("/z").unwrap());
}
