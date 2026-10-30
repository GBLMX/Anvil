use std::{hint::black_box, sync::Arc, time::Instant};

use anvil_core::{GapBuffer, PieceTable, RopeBuffer, TextBuffer};

const VIEWPORT_LINES: usize = 24;
const LINE_WIDTH: usize = 80;
const TRACE_EDITS: u64 = 256;

#[derive(Debug, Clone, Copy)]
enum Edit {
    Insert { at: usize, text: &'static str },
    Delete { start: usize, end: usize },
}

#[derive(Debug, Clone)]
struct EditTrace(Vec<Edit>);

fn synthetic_text(megabytes: usize) -> String {
    let target = megabytes * 1024 * 1024;
    let line = format!("{}\n", "x".repeat(LINE_WIDTH - 1));
    let mut text = String::with_capacity(target + line.len());
    while text.len() < target {
        text.push_str(&line);
    }
    text
}

fn next_random(seed: &mut u64) -> u64 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *seed
}

fn make_trace(length: usize) -> EditTrace {
    let mut seed = 0x5eed_1234_5678_9abc;
    let mut edits = Vec::with_capacity(TRACE_EDITS as usize);
    for _ in 0..TRACE_EDITS {
        let at = (next_random(&mut seed) % length as u64) as usize;
        match next_random(&mut seed) % 4 {
            0 => edits.push(Edit::Insert { at, text: "x" }),
            1 => edits.push(Edit::Insert { at, text: "abc\n" }),
            2 => edits.push(Edit::Delete {
                start: at,
                end: (at + 1).min(length),
            }),
            _ => edits.push(Edit::Delete {
                start: at,
                end: (at + 64).min(length),
            }),
        }
    }
    EditTrace(edits)
}

fn apply_trace<B: TextBuffer>(buffer: &mut B, trace: &EditTrace) {
    for edit in &trace.0 {
        match *edit {
            Edit::Insert { at, text } => {
                let at = at.min(buffer.len_bytes());
                buffer.insert(at, text);
            }
            Edit::Delete { start, end } => {
                let start = start.min(buffer.len_bytes());
                let end = end.min(buffer.len_bytes()).max(start);
                buffer.delete(start..end);
            }
        }
    }
}

fn percentile(samples: &mut [u128], percent: usize) -> u128 {
    samples.sort_unstable();
    let index = (samples.len() * percent / 100).min(samples.len() - 1);
    samples[index]
}

fn report(name: &str, megabytes: usize, workload: &str, mut samples: Vec<u128>) {
    let count = samples.len();
    println!(
        "{name},{megabytes},{workload},{p50},{p95},{p99},{count}",
        p50 = percentile(&mut samples, 50),
        p95 = percentile(&mut samples, 95),
        p99 = percentile(&mut samples, 99),
    );
}

fn measure<B: TextBuffer + Clone>(
    name: &str,
    megabytes: usize,
    samples_per_workload: usize,
    text: &str,
    trace: &EditTrace,
) {
    let mut times = Vec::with_capacity(samples_per_workload);
    for _ in 0..samples_per_workload {
        let start = Instant::now();
        black_box(B::from_text(black_box(text)));
        times.push(start.elapsed().as_nanos());
    }
    report(name, megabytes, "open", times);

    let buffer = B::from_text(text);
    let mut times = Vec::with_capacity(samples_per_workload);
    for _ in 0..samples_per_workload {
        let start = Instant::now();
        black_box(buffer.len_lines());
        times.push(start.elapsed().as_nanos());
    }
    report(name, megabytes, "len_lines", times);

    let end = (VIEWPORT_LINES * LINE_WIDTH).min(buffer.len_bytes());
    let mut times = Vec::with_capacity(samples_per_workload);
    for _ in 0..samples_per_workload {
        let start = Instant::now();
        black_box(buffer.slice(0..end));
        times.push(start.elapsed().as_nanos());
    }
    report(name, megabytes, "viewport", times);

    let shared = Arc::new(B::from_text(text));
    let mut times = Vec::with_capacity(samples_per_workload);
    for _ in 0..samples_per_workload {
        let start = Instant::now();
        black_box(Arc::clone(&shared));
        times.push(start.elapsed().as_nanos());
    }
    report(name, megabytes, "arc_snapshot", times);

    let mut times = Vec::with_capacity(samples_per_workload);
    for _ in 0..samples_per_workload {
        let mut buffer = B::from_text(text);
        apply_trace(&mut buffer, trace);
        let mut shared = Arc::new(buffer);
        let _snapshot = Arc::clone(&shared);
        let start = Instant::now();
        Arc::make_mut(&mut shared).insert(0, "x");
        times.push(start.elapsed().as_nanos());
    }
    report(name, megabytes, "first_edit_after_snapshot", times);

    let mut times = Vec::with_capacity(samples_per_workload);
    for _ in 0..samples_per_workload {
        let mut buffer = B::from_text(text);
        let start = Instant::now();
        apply_trace(&mut buffer, trace);
        times.push(start.elapsed().as_nanos());
        black_box(buffer.len_bytes());
    }
    report(name, megabytes, "random_edits", times);
}

fn main() {
    let megabytes = std::env::var("TEXT_P99_MB")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .map(|value| vec![value])
        .unwrap_or_else(|| vec![1, 10, 100]);
    let requested_samples = std::env::var("TEXT_P99_SAMPLES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok());

    println!("backend,mb,workload,p50_ns,p95_ns,p99_ns,samples");
    for size in megabytes {
        let text = synthetic_text(size);
        let trace = make_trace(text.len());
        let samples = requested_samples.unwrap_or(if size >= 100 { 30 } else { 100 });
        measure::<RopeBuffer>("rope", size, samples, &text, &trace);
        measure::<GapBuffer>("gap", size, samples, &text, &trace);
        measure::<PieceTable>("piece", size, samples, &text, &trace);
    }
}
