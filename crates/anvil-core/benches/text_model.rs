use std::{hint::black_box, sync::Arc};

use anvil_core::{GapBuffer, PieceTable, RopeBuffer, TextBuffer};
use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};

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
            2 => {
                let end = (at + 1).min(length);
                edits.push(Edit::Delete { start: at, end });
            }
            _ => {
                let end = (at + 64).min(length);
                edits.push(Edit::Delete { start: at, end });
            }
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

fn run_backend<B: TextBuffer + Clone>(
    criterion: &mut Criterion,
    name: &str,
    megabytes: usize,
    text: &str,
    trace: &EditTrace,
) {
    let mut group = criterion.benchmark_group(format!("{name}/{megabytes}MB"));
    group.throughput(Throughput::Bytes(text.len() as u64));
    group.sample_size(if megabytes >= 100 { 10 } else { 30 });

    group.bench_function("open", |bencher| {
        bencher.iter(|| black_box(B::from_text(black_box(text))));
    });

    let buffer = B::from_text(text);
    group.bench_function("len_lines", |bencher| {
        bencher.iter(|| black_box(buffer.len_lines()));
    });

    group.bench_function("viewport", |bencher| {
        let end = (VIEWPORT_LINES * LINE_WIDTH).min(buffer.len_bytes());
        bencher.iter(|| black_box(buffer.slice(0..end)));
    });

    let shared = Arc::new(B::from_text(text));
    group.bench_function("arc_snapshot", |bencher| {
        bencher.iter(|| black_box(Arc::clone(&shared)));
    });

    group.bench_function("first_edit_after_snapshot", |bencher| {
        bencher.iter_batched(
            || {
                let mut buffer = B::from_text(text);
                apply_trace(&mut buffer, trace);
                let shared = Arc::new(buffer);
                let snapshot = Arc::clone(&shared);
                (shared, snapshot)
            },
            |(mut shared, _snapshot)| {
                Arc::make_mut(&mut shared).insert(0, "x");
                black_box(shared.len_bytes())
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("random_edits", |bencher| {
        bencher.iter_batched(
            || B::from_text(text),
            |mut buffer| {
                apply_trace(&mut buffer, trace);
                black_box(buffer.len_bytes())
            },
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

fn text_model(criterion: &mut Criterion) {
    let sizes = std::env::var("TEXT_BENCH_MB")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .map(|value| vec![value])
        .unwrap_or_else(|| vec![1, 10, 100]);

    for megabytes in sizes {
        let text = synthetic_text(megabytes);
        let trace = make_trace(text.len());
        run_backend::<RopeBuffer>(criterion, "rope", megabytes, &text, &trace);
        run_backend::<GapBuffer>(criterion, "gap", megabytes, &text, &trace);
        run_backend::<PieceTable>(criterion, "piece", megabytes, &text, &trace);
    }
}

criterion_group!(benches, text_model);
criterion_main!(benches);
