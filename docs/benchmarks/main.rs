// Same source builds against the pre-optimization library (`--cfg bridge_before`, where
// `convert` returned `Vec<serde_json::Value>`) and the typed library. Only `api_values`
// differs: it produces exactly the Values the controller puts into `Action::Create`.
use ingress_gateway_bridge_rs::{
    class_name,
    controller::{Action, Config, Snapshot, plan},
    convert,
    model::Input,
};
use serde_json::{Value, json};
use std::{hint::black_box, time::Instant};

#[cfg(bridge_before)]
fn api_values(input: &Input) -> Vec<Value> {
    convert(input).unwrap()
}
#[cfg(not(bridge_before))]
fn api_values(input: &Input) -> Vec<Value> {
    convert(input)
        .unwrap()
        .iter()
        .map(|route| serde_json::to_value(route).unwrap())
        .collect()
}

/// Ingress Values plus a snapshot already containing their routes, so `plan` is the
/// steady-state resync: deserialize, translate, compare, and return no actions.
fn steady_state(data: &str, inputs: &[Input]) -> (Vec<Value>, Config, Snapshot) {
    let raw: Value = serde_json::from_str(data).unwrap();
    let ingresses: Vec<Value> = inputs
        .iter()
        .map(|input| {
            let mut ingress = raw["ingress"].clone();
            ingress["metadata"]["name"] = json!(input.ingress.metadata.name);
            ingress["metadata"]["uid"] = json!(input.ingress.metadata.uid);
            ingress["spec"]["rules"][0]["host"] = json!(input.ingress.spec.rules[0].host);
            ingress
        })
        .collect();
    let class = inputs[0].ingress.spec.ingress_class_name.clone().unwrap();
    let cfg: Config =
        serde_json::from_value(json!({"classes":{class.clone():raw["binding"].clone()}})).unwrap();
    let mut s = Snapshot {
        classes: vec![json!({"metadata":{"name":class},"spec":{"controller":cfg.controller_name}})],
        gateways: vec![
            json!({"metadata":{"name":"public","namespace":"gateway-system"},"spec":{"listeners":[{"name":"https","protocol":"HTTPS","port":443,"allowedRoutes":{"namespaces":{"from":"All"}}}]}}),
        ],
        ingresses: ingresses.clone(),
        ..Default::default()
    };
    for (n, ingress) in ingresses.iter().enumerate() {
        for action in plan(ingress, &cfg, &s).unwrap() {
            let Action::Create(mut route) = action else {
                panic!("expected create")
            };
            route["metadata"]["uid"] = json!(format!("route-uid-{n}"));
            route["metadata"]["resourceVersion"] = json!("1");
            s.routes.push(route);
        }
    }
    for ingress in &ingresses {
        assert!(plan(ingress, &cfg, &s).unwrap().is_empty());
    }
    (ingresses, cfg, s)
}

fn time(n: usize, mut op: impl FnMut()) -> std::time::Duration {
    for _ in 0..1000 {
        op();
    }
    let start = Instant::now();
    for _ in 0..n {
        op();
    }
    start.elapsed()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let kind = args[1].as_str();
    let n: usize = args[2].parse().unwrap();
    let data = std::fs::read_to_string(&args[3]).unwrap();
    let size = if kind.ends_with("22") { 22 } else { 1 };
    let inputs: Vec<Input> = (0..size)
        .map(|i| {
            let mut input: Input = serde_json::from_str(&data).unwrap();
            input.ingress.metadata.name = format!("bench-{i}");
            input.ingress.metadata.uid = format!("uid-{i}");
            input.ingress.spec.rules[0].host = format!("bench-{i}.example.test");
            input
        })
        .collect();
    let steady = (kind == "plan22").then(|| steady_state(&data, &inputs));
    // Dispatch once; each case times its own monomorphized loop. Every result is
    // consumed by black_box and dropped inside the timed region.
    let elapsed = match kind {
        "class" => time(n, || {
            black_box(class_name(black_box(&inputs[0].ingress)).unwrap());
        }),
        "convert1" | "convert22" => time(n, || {
            for input in &inputs {
                black_box(convert(black_box(input)).unwrap());
            }
        }),
        "ser1" | "ser22" => time(n, || {
            for input in &inputs {
                black_box(serde_json::to_vec(&convert(black_box(input)).unwrap()).unwrap());
            }
        }),
        "create1" | "create22" => time(n, || {
            for input in &inputs {
                for route in api_values(black_box(input)) {
                    black_box(serde_json::to_vec(&route).unwrap());
                }
            }
        }),
        "plan22" => time(n, || {
            let (ingresses, cfg, snapshot) = steady.as_ref().unwrap();
            for ingress in ingresses {
                black_box(plan(black_box(ingress), cfg, snapshot).unwrap());
            }
        }),
        _ => panic!("unknown case {kind}"),
    };
    let last = &inputs[inputs.len() - 1];
    let sample = match kind {
        "plan22" => {
            let (ingresses, cfg, snapshot) = steady.as_ref().unwrap();
            json!({"actions": plan(&ingresses[21], cfg, snapshot).unwrap().len()})
        }
        "ser1" | "ser22" => {
            let bytes = serde_json::to_vec(&convert(last).unwrap()).unwrap();
            json!({"routes": serde_json::from_slice::<Value>(&bytes).unwrap(), "bytes": bytes.len()})
        }
        "create1" | "create22" => json!({"routes": api_values(last)}),
        _ => json!({"routes": convert(last).unwrap()}),
    };
    println!(
        "{}",
        json!({"case":kind,"iterations":n,"ns_per_op":elapsed.as_nanos() as f64/n as f64,"sample":sample,"class":class_name(&inputs[0].ingress).unwrap()})
    );
}
