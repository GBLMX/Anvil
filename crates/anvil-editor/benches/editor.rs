use std::hint::black_box;

use anvil_api::Viewport;
use anvil_core::{LineIndex, PieceTable, TextBuffer};
use anvil_editor::EditorState;
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

fn long_line(megabytes: usize) -> String {
    "x".repeat(megabytes * 1024 * 1024)
}

fn editor_benches(criterion: &mut Criterion) {
    let text = generate_file(100, 80);
    let base = EditorState::from_text(&text);
    let midpoint = text.len() / 2;
    let pasted = "pasted line\n".repeat(10_000);
    let split_paste = "split line\n".repeat(1_100);

    let mut group = criterion.benchmark_group("editor/edit/100MB");
    group.sample_size(100);
    group.bench_function("A1_middle_insert", |bencher| {
        bencher.iter_batched(
            || base.clone(),
            |mut state| {
                state.insert(midpoint, "x");
                black_box(state.revision())
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("A2_split_insert", |bencher| {
        bencher.iter_batched(
            || base.clone(),
            |mut state| {
                state.insert(midpoint, &split_paste);
                black_box(state.index().chunk_count())
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("A3_merge_delete", |bencher| {
        bencher.iter_batched(
            || base.clone(),
            |mut state| {
                state.delete(midpoint..midpoint + 80_000);
                black_box(state.index().chunk_count())
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("A4_paste_10k_lines", |bencher| {
        bencher.iter_batched(
            || base.clone(),
            |mut state| {
                state.insert(midpoint, &pasted);
                black_box(state.index().chunk_count())
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("A5_append", |bencher| {
        bencher.iter_batched(
            || base.clone(),
            |mut state| {
                let end = state.text().len_bytes();
                state.insert(end, "x");
                black_box(state.revision())
            },
            BatchSize::LargeInput,
        );
    });
    let base_buffer = PieceTable::from_text(&text);
    let base_index = LineIndex::from_buffer(&base_buffer);
    group.bench_function("A3_piece_delete", |bencher| {
        bencher.iter_batched(
            || base_buffer.clone(),
            |mut buffer| {
                buffer.delete(midpoint..midpoint + 80_000);
                black_box(buffer.len_bytes())
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("A3_line_index_delete", |bencher| {
        bencher.iter_batched(
            || {
                let mut buffer = base_buffer.clone();
                let index = base_index.clone();
                buffer.delete(midpoint..midpoint + 80_000);
                (buffer, index)
            },
            |(buffer, mut index)| {
                index.apply_edit(&buffer, midpoint..midpoint + 80_000);
                black_box(index.chunk_count())
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("A5_piece_append", |bencher| {
        bencher.iter_batched(
            || base_buffer.clone(),
            |mut buffer| {
                let end = buffer.len_bytes();
                buffer.insert(end, "x");
                black_box(buffer.len_bytes())
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("A5_line_index_append", |bencher| {
        bencher.iter_batched(
            || {
                let mut buffer = base_buffer.clone();
                let index = base_index.clone();
                let end = buffer.len_bytes();
                buffer.insert(end, "x");
                (buffer, index, end)
            },
            |(buffer, mut index, end)| {
                index.apply_edit(&buffer, end..end);
                black_box(index.len_lines())
            },
            BatchSize::LargeInput,
        );
    });
    group.finish();

    let mut group = criterion.benchmark_group("editor/view/100MB");
    group.sample_size(100);
    let snapshot = base.snapshot();
    group.bench_function("C1_single_caret_10k_views", |bencher| {
        bencher.iter(|| {
            for _ in 0..10_000 {
                black_box(snapshot.view());
            }
        });
    });

    let mut selected = base.clone();
    selected.set_selections_bytes((0..100).map(|index| (index, index)).collect());
    let selected_snapshot = selected.snapshot();
    group.bench_function("C2_100_selections_10k_views", |bencher| {
        bencher.iter(|| {
            for _ in 0..10_000 {
                black_box(selected_snapshot.view());
            }
        });
    });

    let long = long_line(1);
    let mut long_state = EditorState::from_text(&long);
    long_state.set_viewport(Viewport {
        first_line: 0,
        first_column: 0,
        line_count: 24,
        width: 80,
        height: 24,
    });
    let long_snapshot = long_state.snapshot();
    group.bench_function("C3_long_line_view", |bencher| {
        bencher.iter(|| black_box(long_snapshot.view()));
    });
    group.finish();
}

criterion_group!(benches, editor_benches);
criterion_main!(benches);
