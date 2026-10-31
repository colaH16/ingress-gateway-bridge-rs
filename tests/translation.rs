// SPDX-License-Identifier: Apache-2.0
use ingress_gateway_bridge_rs::{
    ANNOTATION_PREFIX, CONTROLLER_ANNOTATION, Error, REDIRECT_ANNOTATION, SOURCE_ANNOTATION,
    class_name, convert, model::Input, route_name,
};
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
    // Assert on the serialized JSON, which is what the API and CLI receive.
    let output = |name: &str| -> Vec<Value> {
        let case = cases.iter().find(|c| c["name"] == name).unwrap();
        let input: Input = serde_json::from_value(case["input"].clone()).unwrap();
        convert(&input)
            .unwrap()
            .iter()
            .map(|route| serde_json::to_value(route).unwrap())
            .collect()
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

fn case_input(name: &str) -> Value {
    let cases: Vec<Value> = serde_json::from_str(include_str!("cases.json")).unwrap();
    cases.into_iter().find(|c| c["name"] == name).unwrap()["input"].clone()
}

/// Header filters, redirect, several hosts, a hostless rule and mixed sectionName parents.
fn combined_input() -> Value {
    let mut input = case_input("redirect 301");
    let headers = case_input("request-headers valid")["ingress"]["metadata"]["annotations"].clone();
    let response =
        case_input("response-headers valid")["ingress"]["metadata"]["annotations"].clone();
    let annotations = input["ingress"]["metadata"]["annotations"]
        .as_object_mut()
        .unwrap();
    for source in [headers, response] {
        annotations.extend(source.as_object().unwrap().clone());
    }
    let rules = input["ingress"]["spec"]["rules"].as_array_mut().unwrap();
    let mut second = rules[0].clone();
    second["host"] = json!("z.example.com");
    rules.push(second);
    rules.push(json!({"http":{"paths":[{"path":"/hostless/","pathType":"Prefix","backend":{"service":{"name":"hostless","port":{"number":81}}}}]}}));
    input["binding"]["parentRefs"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"internal","namespace":"gateway-system"}));
    input
}

#[test]
fn typed_output_serializes_like_sorted_json_value() {
    // The former output was a BTreeMap-backed Value, which always serializes keys sorted.
    // Equal strings prove field order, renames and skipped fields kept CLI bytes stable.
    let cases: Vec<Value> = serde_json::from_str(include_str!("cases.json")).unwrap();
    let inputs = cases
        .iter()
        .filter(|c| c["expected"] == "ok")
        .map(|c| c["input"].clone())
        .chain([combined_input()]);
    for value in inputs {
        let input: Input = serde_json::from_value(value).unwrap();
        let routes = convert(&input).unwrap();
        let typed = serde_json::to_string(&routes).unwrap();
        let sorted = serde_json::to_string(&serde_json::to_value(&routes).unwrap()).unwrap();
        assert_eq!(typed, sorted);
    }
}

#[test]
fn typed_output_optional_fields() {
    let input: Input = serde_json::from_value(combined_input()).unwrap();
    let routes: Vec<Value> = convert(&input)
        .unwrap()
        .iter()
        .map(|r| serde_json::to_value(r).unwrap())
        .collect();
    // Sorted hosts: "" (hostless), then two named hosts; each followed by its redirect.
    assert_eq!(routes.len(), 6);
    let (hostless, redirect) = (&routes[0], &routes[1]);
    assert!(hostless["spec"].get("hostnames").is_none());
    assert!(redirect["spec"].get("hostnames").is_none());
    assert_eq!(
        hostless["spec"]["rules"][0]["matches"][0]["path"]["value"],
        "/hostless"
    );
    let filters = hostless["spec"]["rules"][0]["filters"].as_array().unwrap();
    assert_eq!(
        filters
            .iter()
            .map(|f| f["type"].clone())
            .collect::<Vec<_>>(),
        [
            json!("RequestHeaderModifier"),
            json!("ResponseHeaderModifier")
        ]
    );
    for filter in filters {
        assert_eq!(
            filter.as_object().unwrap().len(),
            2,
            "one modifier per filter"
        );
    }
    let parents = hostless["spec"]["parentRefs"].as_array().unwrap();
    assert_eq!(parents[0]["sectionName"], "https");
    assert!(parents[1].get("sectionName").is_none());
    let rule = &redirect["spec"]["rules"][0];
    assert!(rule.get("backendRefs").is_none());
    assert_eq!(
        rule["filters"],
        json!([{"type":"RequestRedirect","requestRedirect":{"scheme":"https","port":443,"statusCode":301}}])
    );
    assert_ne!(
        redirect["spec"]["parentRefs"],
        hostless["spec"]["parentRefs"]
    );
    assert_eq!(
        redirect["metadata"]["ownerReferences"],
        hostless["metadata"]["ownerReferences"]
    );
    assert_eq!(
        redirect["metadata"]["annotations"],
        hostless["metadata"]["annotations"]
    );
    assert_eq!(
        routes[2]["spec"]["hostnames"],
        json!([input.ingress.spec.rules[0].host])
    );
    assert_eq!(routes[4]["spec"]["hostnames"], json!(["z.example.com"]));
    let names: std::collections::BTreeSet<_> = routes
        .iter()
        .map(|r| r["metadata"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), routes.len());
    // Plain routes omit filters entirely rather than emitting an empty list.
    let basic: Input = serde_json::from_value(case_input("basic")).unwrap();
    let basic = serde_json::to_value(convert(&basic).unwrap()).unwrap();
    assert!(basic[0]["spec"]["rules"][0].get("filters").is_none());
    assert_eq!(
        basic[0]["metadata"]["annotations"],
        json!({CONTROLLER_ANNOTATION: "ingress-gateway-bridge.colah16.github.io/controller", SOURCE_ANNOTATION: "apps/website"})
    );
}

#[test]
fn annotation_constants_and_route_names_are_stable() {
    for (constant, suffix) in [
        (CONTROLLER_ANNOTATION, "controller"),
        (SOURCE_ANNOTATION, "source"),
        (REDIRECT_ANNOTATION, "https-redirect"),
    ] {
        assert_eq!(constant, format!("{ANNOTATION_PREFIX}{suffix}"));
    }
    // Values produced by the previous format!-based implementation (and Go RouteName).
    let long = "a".repeat(253);
    for (name, host, expected) in [
        (
            "website",
            "website.example.com",
            "website-bridge-fbc452e8dda6f7f7",
        ),
        ("website", "", "website-bridge-640f5e5cbbfa0957"),
        (
            "abcdefghijabcdefghijabcdefghijabcdefgh-.x",
            "h",
            "abcdefghijabcdefghijabcdefghijabcdefgh-bridge-6d8dff95a3120e3d",
        ),
        (
            &long,
            "a",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-bridge-e0855d2e98d11746",
        ),
        (
            "가나다라마바사아자차카타파하가나다라마바",
            "x\0https-redirect",
            "가나다라마바사아자차카타파-bridge-9546e859e8eb5ccf",
        ),
    ] {
        assert_eq!(route_name(name, host), expected);
    }
}
