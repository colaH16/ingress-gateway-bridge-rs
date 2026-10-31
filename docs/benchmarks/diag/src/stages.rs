//! Diagnostic only (system allocator, no instrumentation): per-stage timing interleaved in
//! one process, so stage costs share the same host conditions. Not a headline measurement.
use bridge::{convert, model::Input};
use serde_json::Value;
use std::{hint::black_box, time::Instant};
fn inputs(data: &str, size: usize) -> Vec<Input> {
    (0..size).map(|i| {
        let mut input: Input = serde_json::from_str(data).unwrap();
        input.ingress.metadata.name = format!("bench-{i}");
        input.ingress.metadata.uid = format!("uid-{i}");
        input.ingress.spec.rules[0].host = format!("bench-{i}.example.test");
        input
    }).collect()
}
fn batch(n: u32, mut f: impl FnMut()) -> f64 {
    let start = Instant::now();
    for _ in 0..n { f(); }
    start.elapsed().as_nanos() as f64 / n as f64
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = std::fs::read_to_string(&args[1]).unwrap();
    let one = inputs(&data, 1);
    let i = &one[0];
    let routes = convert(i).unwrap();
    let value: Value = serde_json::to_value(&routes).unwrap();
    let n = 20000;
    let names = ["convert1(+drop)", "to_vec(prebuilt convert output)", "to_value(prebuilt convert output)+drop", "to_vec(prebuilt Value)", "convert1+to_vec"];
    let mut samples: Vec<Vec<f64>> = vec![Vec::new(); names.len()];
    for round in 0..31 {
        let t = [
            batch(n, || drop(black_box(convert(black_box(i)).unwrap()))),
            batch(n, || drop(black_box(serde_json::to_vec(black_box(&routes)).unwrap()))),
            batch(n, || drop(black_box(serde_json::to_value(black_box(&routes)).unwrap()))),
            batch(n, || drop(black_box(serde_json::to_vec(black_box(&value)).unwrap()))),
            batch(n, || drop(black_box(serde_json::to_vec(&convert(black_box(i)).unwrap()).unwrap()))),
        ];
        if round > 0 { for (s, x) in samples.iter_mut().zip(t) { s.push(x); } }
    }
    for (name, mut s) in names.into_iter().zip(samples) {
        s.sort_by(f64::total_cmp);
        println!("{{\"stage\":\"{name}\",\"batches\":{},\"median_ns\":{:.1},\"min_ns\":{:.1},\"max_ns\":{:.1}}}", s.len(), s[s.len()/2], s[0], s[s.len()-1]);
    }
}
