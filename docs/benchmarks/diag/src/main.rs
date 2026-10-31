//! Diagnostic only: counting allocator and optional sampling. Never used for headline timing.
use bridge::{convert, model::Input, options, route_name};
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
struct Counting;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static REALLOCS: AtomicU64 = AtomicU64::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(l.size() as u64, Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        REALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(n.saturating_sub(l.size()) as u64, Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
#[cfg(not(feature = "profile"))]
#[global_allocator]
static A: Counting = Counting;
fn count(label: &str, n: u64, mut f: impl FnMut()) {
    for _ in 0..100 { f(); }
    let (a, b, r) = (ALLOCS.load(Relaxed), BYTES.load(Relaxed), REALLOCS.load(Relaxed));
    for _ in 0..n { f(); }
    println!(
        "{{\"stage\":\"{label}\",\"allocs_per_op\":{},\"reallocs_per_op\":{},\"bytes_per_op\":{}}}",
        (ALLOCS.load(Relaxed) - a) as f64 / n as f64,
        (REALLOCS.load(Relaxed) - r) as f64 / n as f64,
        (BYTES.load(Relaxed) - b) as f64 / n as f64
    );
}
fn inputs(data: &str, size: usize) -> Vec<Input> {
    (0..size).map(|i| {
        let mut input: Input = serde_json::from_str(data).unwrap();
        input.ingress.metadata.name = format!("bench-{i}");
        input.ingress.metadata.uid = format!("uid-{i}");
        input.ingress.spec.rules[0].host = format!("bench-{i}.example.test");
        input
    }).collect()
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = std::fs::read_to_string(&args[1]).unwrap();
    let one = inputs(&data, 1);
    let many = inputs(&data, 22);
    let i = &one[0];
    #[cfg(not(feature = "profile"))]
    {
    count("convert1(+drop)", 20000, || drop(black_box(convert(black_box(i)).unwrap())));
    count("convert22(+drop)", 2000, || for x in &many { drop(black_box(convert(black_box(x)).unwrap())) });
    count("convert1+to_vec", 20000, || drop(black_box(serde_json::to_vec(&convert(black_box(i)).unwrap()).unwrap())));
    count("options::parse(empty annotations)", 20000, || { black_box(options::parse(black_box(&i.ingress.metadata.annotations)).ok()); });
    count("route_name", 20000, || { black_box(route_name(black_box("bench-0"), black_box("bench-0.example.test"))); });
    }
    let _ = &many;
    #[cfg(feature = "profile")]
    {
        let n: usize = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(400000);
        let guard = pprof::ProfilerGuardBuilder::default().frequency(4999).build().unwrap();
        for _ in 0..n { drop(black_box(convert(black_box(i)).unwrap())); }
        let report = guard.report().build().unwrap();
        let f = std::fs::File::create(args.get(3).map(String::as_str).unwrap_or("flame.svg")).unwrap();
        report.flamegraph(f).unwrap();
        // Self/inclusive tallies by symbol name.
        let mut incl: std::collections::HashMap<String, isize> = Default::default();
        let mut leaf: std::collections::HashMap<String, isize> = Default::default();
        let mut total = 0;
        for (frames, c) in &report.data {
            total += *c;
            let mut seen = std::collections::HashSet::new();
            for (depth, sym) in frames.frames.iter().flatten().enumerate() {
                let name = sym.name();
                if depth == 0 { *leaf.entry(name.clone()).or_default() += *c; }
                if seen.insert(name.clone()) { *incl.entry(name).or_default() += *c; }
            }
        }
        for (title, map) in [("inclusive", incl), ("self", leaf)] {
            let mut v: Vec<_> = map.into_iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            println!("== {title} (samples total {total})");
            for (k, c) in v.into_iter().take(400) {
                println!("{:6.2}% {}", 100.0 * c as f64 / total as f64, k.chars().take(150).collect::<String>());
            }
        }
    }
}
