use std::{hint::black_box, time::Instant};

use anvil_api::Viewport;
use anvil_core::TextBuffer;
use anvil_editor::EditorState;

const VIEW_SAMPLES: usize = 10_000;

fn generate_file(megabytes: usize, line_len: usize) -> String {
    let target = megabytes * 1024 * 1024;
    let line = format!("{}\n", "x".repeat(line_len.saturating_sub(1)));
    let mut text = String::with_capacity(target + line.len());
    while text.len() < target {
        text.push_str(&line);
    }
    text
}

fn percentile(samples: &mut [u128], percent: usize) -> u128 {
    samples.sort_unstable();
    samples[(samples.len() * percent / 100).min(samples.len() - 1)]
}

fn report(case: &str, mut samples: Vec<u128>) {
    let count = samples.len();
    println!(
        "{case},{p50},{p95},{p99},{count}",
        p50 = percentile(&mut samples, 50),
        p95 = percentile(&mut samples, 95),
        p99 = percentile(&mut samples, 99),
    );
}

fn measure_edits(
    case: &str,
    count: usize,
    mut state: EditorState,
    mut action: impl FnMut(&mut EditorState),
) {
    let mut samples = Vec::with_capacity(count);
    for _ in 0..count {
        let start = Instant::now();
        action(&mut state);
        samples.push(start.elapsed().as_nanos());
        black_box(state.revision());
    }
    report(case, samples);
}

fn measure_edits_fresh(
    case: &str,
    count: usize,
    text: &str,
    mut action: impl FnMut(&mut EditorState),
) {
    let mut samples = Vec::with_capacity(count);
    for _ in 0..count {
        let mut state = EditorState::from_text(text);
        let start = Instant::now();
        action(&mut state);
        samples.push(start.elapsed().as_nanos());
        black_box(state.revision());
    }
    report(case, samples);
}

fn measure_views(case: &str, count: usize, snapshot: &anvil_editor::EditorSnapshot) {
    let mut samples = Vec::with_capacity(count);
    for _ in 0..count {
        let start = Instant::now();
        black_box(snapshot.view());
        samples.push(start.elapsed().as_nanos());
    }
    report(case, samples);
}

fn main() {
    let megabytes = std::env::var("TEXT_EDITOR_MB")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(100);
    let edit_samples = std::env::var("TEXT_EDITOR_EDIT_SAMPLES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(100);
    let text = generate_file(megabytes, 80);
    let base = EditorState::from_text(&text);
    let midpoint = text.len() / 2;
    let pasted = "pasted line\n".repeat(10_000);
    let split_paste = "split line\n".repeat(1_025);

    println!("case,p50_ns,p95_ns,p99_ns,samples");
    measure_edits(
        "A1_middle_insert",
        edit_samples,
        EditorState::from_text(&text),
        |state| {
            state.insert(midpoint, "x");
        },
    );
    measure_edits(
        "A2_split_insert",
        edit_samples,
        EditorState::from_text(&text),
        |state| {
            state.insert(midpoint, &split_paste);
        },
    );
    measure_edits_fresh("A3_merge_delete", edit_samples, &text, |state| {
        let len = state.text().len_bytes();
        let start = (len / 2).min(len.saturating_sub(1));
        let end = (start + 80_000).min(len);
        state.delete(start..end);
    });
    measure_edits(
        "A4_paste_10k_lines",
        edit_samples,
        EditorState::from_text(&text),
        |state| {
            state.insert(midpoint, &pasted);
        },
    );
    measure_edits(
        "A5_append",
        edit_samples,
        EditorState::from_text(&text),
        |state| {
            let end = state.text().len_bytes();
            state.insert(end, "x");
        },
    );

    let view_samples = std::env::var("TEXT_EDITOR_VIEW_SAMPLES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(VIEW_SAMPLES);
    let snapshot = base.snapshot();
    measure_views("C1_single_caret_view", view_samples, &snapshot);

    let mut selected = base.clone();
    selected.set_selections_bytes((0..100).map(|index| (index, index)).collect());
    let selected_snapshot = selected.snapshot();
    measure_views("C2_100_selections_view", view_samples, &selected_snapshot);

    let long = "x".repeat(megabytes * 1024 * 1024);
    let mut long_state = EditorState::from_text(&long);
    long_state.set_viewport(Viewport {
        first_line: 0,
        first_column: 0,
        line_count: 24,
        width: 80,
        height: 24,
    });
    let long_snapshot = long_state.snapshot();
    measure_views("C3_long_line_view", view_samples, &long_snapshot);
}
