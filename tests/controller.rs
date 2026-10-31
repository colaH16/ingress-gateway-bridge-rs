use ingress_gateway_bridge_rs::{controller::*, convert, model::Input};
use serde_json::{Value, json};
fn fixture() -> (Value, Config, Snapshot) {
    let input: Input = serde_json::from_str(include_str!("../examples/basic.json")).unwrap();
    let source: Value = serde_json::from_str::<Value>(include_str!("../examples/basic.json"))
        .unwrap()["ingress"]
        .clone();
    let cfg:Config=serde_json::from_value(json!({"classes":{"public-apps":{"parentRefs":[{"name":"public","namespace":"gateway-system","sectionName":"https"}]}}})).unwrap();
    let mut route = convert(&input).unwrap().remove(0);
    route["metadata"]["uid"] = json!("route-uid");
    route["metadata"]["resourceVersion"] = json!("12");
    let s = Snapshot {
        ingresses: vec![source.clone()],
        classes: vec![
            json!({"metadata":{"name":"public-apps"},"spec":{"controller":cfg.controller_name}}),
        ],
        routes: vec![route],
        gateways: vec![
            json!({"metadata":{"name":"public","namespace":"gateway-system"},"spec":{"listeners":[{"name":"https","protocol":"HTTPS","port":443,"allowedRoutes":{"namespaces":{"from":"All"}}}]}}),
        ],
        ..Default::default()
    };
    (source, cfg, s)
}
#[test]
fn idempotence_and_preserve_other_metadata() {
    let (i, c, mut s) = fixture();
    s.routes[0]["metadata"]["labels"]["external"] = json!("retained");
    assert!(plan(&i, &c, &s).unwrap().is_empty());
    s.routes[0]["spec"]["rules"][0]["backendRefs"][0]["port"] = json!(81);
    let actions = plan(&i, &c, &s).unwrap();
    assert_eq!(actions.len(), 1);
    assert!(
        matches!(&actions[0],Action::Update(v) if v["metadata"]["resourceVersion"]=="12" && v["metadata"]["labels"]["external"]=="retained")
    );
}
#[test]
fn creation_and_stale_cleanup() {
    let (i, c, mut s) = fixture();
    let mut stale = s.routes[0].clone();
    stale["metadata"]["name"] = json!("stale");
    s.routes = vec![stale];
    let actions = plan(&i, &c, &s).unwrap();
    assert_eq!(actions.len(), 2);
    assert!(matches!(actions[0], Action::Create(_)));
    assert!(matches!(actions[1], Action::Delete(_)));
}
#[test]
fn foreign_route_collision_prevents_all_writes() {
    let (i, c, mut s) = fixture();
    s.routes[0]["metadata"]["ownerReferences"][0]["uid"] = json!("other");
    assert!(plan(&i, &c, &s).is_err());
}
#[test]
fn ownership_guards_and_class_lifecycle() {
    let (mut i, c, mut s) = fixture();
    s.classes.clear();
    assert!(matches!(plan(&i, &c, &s).unwrap()[0], Action::Delete(_)));
    s.routes[0]["metadata"]["ownerReferences"][0]["uid"] = json!("other");
    assert!(plan(&i, &c, &s).unwrap().is_empty());
    let (original, c, s) = fixture();
    i = original;
    i["spec"]["ingressClassName"] = json!("foreign");
    assert!(matches!(plan(&i, &c, &s).unwrap()[0], Action::Delete(_)));
    i["metadata"]["deletionTimestamp"] = json!("2026-10-31T00:00:00Z");
    assert!(plan(&i, &c, &s).unwrap().is_empty());
}
#[test]
fn dependencies_preserve_and_rejections_withdraw() {
    let (mut i, c, s) = fixture();
    i["spec"]["rules"][0]["http"]["paths"][0]["backend"]["service"]["port"] =
        json!({"name":"http"});
    assert!(plan(&i, &c, &s).is_err());
    i["spec"]["rules"][0]["host"] = json!("*.example.com");
    assert!(matches!(plan(&i, &c, &s).unwrap()[0], Action::Delete(_)));
    let (i, c, mut s) = fixture();
    s.gateways.clear();
    assert!(plan(&i, &c, &s).is_err());
}
#[test]
fn namespace_selector_and_listener_constraints() {
    let (i, c, mut s) = fixture();
    s.gateways[0]["spec"]["listeners"][0]["allowedRoutes"] = json!({"namespaces":{"from":"Same"}});
    assert!(plan(&i, &c, &s).is_err());
    s.gateways[0]["spec"]["listeners"][0]["allowedRoutes"] = json!({"namespaces":{"from":"Selector","selector":{"matchLabels":{"allow":"yes"},"matchExpressions":[{"key":"blocked","operator":"DoesNotExist"}]}}});
    s.namespaces
        .push(json!({"metadata":{"name":"apps","labels":{"allow":"yes"}}}));
    assert!(plan(&i, &c, &s).unwrap().is_empty());
    s.namespaces[0]["metadata"]["labels"]["blocked"] = json!("yes");
    assert!(plan(&i, &c, &s).is_err());
}
#[test]
fn configuration_rejects_ambiguous_parents() {
    let (_, mut c, _) = fixture();
    assert!(c.validate().is_ok());
    let b = c.classes.get_mut("public-apps").unwrap();
    let mut p = b.parent_refs[0].clone();
    p.section_name.clear();
    b.parent_refs.push(p);
    assert!(c.validate().is_err());
}

#[test]
fn invalid_namespace_selectors_do_not_authorize_routes() {
    for selector in [
        json!({"matchExpressions":[{"key":"team","operator":"NotIn","values":[]}]}),
        json!({"matchExpressions":[{"key":"team","operator":"DoesNotExist","values":["x"]}]}),
        json!({"matchLabels":{"invalid key":"x"}}),
    ] {
        let (i, c, mut s) = fixture();
        s.namespaces =
            vec![json!({"metadata":{"name":namespace(&i),"labels":{"invalid key":"x"}}})];
        s.gateways[0]["spec"]["listeners"][0]["allowedRoutes"] =
            json!({"namespaces":{"from":"Selector","selector":selector}});
        assert!(plan(&i, &c, &s).is_err());
    }
}
