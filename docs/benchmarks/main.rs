use ingress_gateway_bridge_rs::{class_name, convert, model::Input};
use std::{hint::black_box, time::Instant};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let kind = &args[1];
    let n: usize = args[2].parse().unwrap();
    let data = std::fs::read_to_string(&args[3]).unwrap();
    let size = if kind == "convert22" { 22 } else { 1 };
    let inputs: Vec<Input> = (0..size)
        .map(|i| {
            let mut input: Input = serde_json::from_str(&data).unwrap();
            input.ingress.metadata.name = format!("bench-{i}");
            input.ingress.metadata.uid = format!("uid-{i}");
            input.ingress.spec.rules[0].host = format!("bench-{i}.example.test");
            input
        })
        .collect();
    let op = || {
        if kind == "class" {
            black_box(class_name(black_box(&inputs[0].ingress)).unwrap());
        } else {
            for input in &inputs {
                black_box(convert(black_box(input)).unwrap());
            }
        }
    };
    for _ in 0..1000 {
        op();
    }
    let start = Instant::now();
    if kind == "class" {
        for _ in 0..n {
            black_box(class_name(black_box(&inputs[0].ingress)).unwrap());
        }
    } else {
        for _ in 0..n {
            for input in &inputs {
                black_box(convert(black_box(input)).unwrap());
            }
        }
    }
    let elapsed = start.elapsed();
    println!(
        "{}",
        serde_json::json!({"case":kind,"iterations":n,"ns_per_op":elapsed.as_nanos() as f64/n as f64,"sample":{"routes":convert(&inputs[inputs.len()-1]).unwrap()},"class":class_name(&inputs[0].ingress).unwrap()})
    );
}
