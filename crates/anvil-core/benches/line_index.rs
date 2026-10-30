use std::hint::black_box;

use anvil_core::{LineIndex, PieceTable, TextBuffer};
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};

fn generate_file(megabytes: usize, line_len: usize) -> String {
    let target = megabytes * 1024 * 1024;
    let line = format!("{}\n", "x".repeat(line_len.saturating_sub(1)));
    let mut text = String::with_capacity(target + line.len());
    while text.len() < target {
        text.push_str(&line);
    }
    text
}

fn line_index_benches(criterion: &mut Criterion) {
    let text = generate_file(100, 80);
    let mut group = criterion.benchmark_group("line-index/100MB");
    group.sample_size(100);
    group.bench_function("build", |bencher| {
        bencher.iter(|| black_box(LineIndex::from_text(&text)));
    });
    let index = LineIndex::from_text(&text);
    group.bench_function("offset_to_line", |bencher| {
        bencher.iter(|| black_box(index.offset_to_line(text.len() / 2)));
    });
    group.bench_function("line_to_offset", |bencher| {
        bencher.iter(|| black_box(index.line_to_offset(index.len_lines() / 2)));
    });
    group.finish();

    let mut group = criterion.benchmark_group("line-index/edit/100MB");
    group.sample_size(100);
    let base = PieceTable::from_text(&text);
    let base_index = LineIndex::from_buffer(&base);
    group.bench_function("small_insert", |bencher| {
        bencher.iter_batched(
            || (base.clone(), base_index.clone()),
            |(mut buffer, mut index)| {
                let at = buffer.len_bytes() / 2;
                buffer.insert(at, "x");
                index.apply_edit(&buffer, at..at);
                black_box(index.chunk_count())
            },
            BatchSize::LargeInput,
        );
    });
    group.finish();
}

criterion_group!(benches, line_index_benches);
criterion_main!(benches);
