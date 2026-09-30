//! Development-only allocation and cancellation measurements; never part of the application.
use serde_json::json;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};
struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;
fn measure(work: impl FnOnce()) -> serde_json::Value {
    let baseline = LIVE.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let allocations = ALLOCS.load(Ordering::Relaxed);
    let start = Instant::now();
    work();
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
    // Read counters before building the JSON result, which allocates memory itself.
    let allocation_count = ALLOCS.load(Ordering::Relaxed) - allocations;
    let peak_bytes = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
    let retained_bytes = LIVE.load(Ordering::Relaxed).saturating_sub(baseline);
    json!({"elapsed_ms":elapsed,"allocations":allocation_count,"peak_additional_live_bytes":peak_bytes,"retained_bytes":retained_bytes})
}
fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let work = || match args.get(1).map(String::as_str) {
        Some("plan") => {
            let repetitions = args.get(2).unwrap().parse::<usize>().unwrap();
            let request = json!({"capability":"throughput","layers":[{"target":"fixture.invalid","tcp_streams":vec![1;repetitions],"tcp_windows_bytes":vec![Option::<u32>::None;repetitions]}]});
            std::hint::black_box(lantern_runtime::plan_request(&request).unwrap());
        }
        Some("report") => {
            std::hint::black_box(
                lantern_runtime::reports::read(std::path::Path::new(args.get(2).unwrap())).unwrap(),
            );
        }
        _ => panic!("usage: resource_measurement plan REPETITIONS | report FILE"),
    };
    work();
    let results = (0..10).map(|_| measure(work)).collect::<Vec<_>>();
    println!(
        "{}",
        json!({"warmups":1,"repetitions":10,"samples":results})
    );
}
