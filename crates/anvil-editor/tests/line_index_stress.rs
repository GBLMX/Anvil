use std::time::Instant;

use anvil_core::{LineIndex, PieceTable, TextBuffer};
use anvil_editor::EditorState;

fn generate_file(megabytes: usize) -> String {
    let target = megabytes * 1024 * 1024;
    let line = format!("{}\n", "x".repeat(79));
    let mut text = String::with_capacity(target + line.len());
    while text.len() < target {
        text.push_str(&line);
    }
    text
}

fn assert_index_matches(text: &PieceTable, index: &LineIndex) {
    assert_eq!(index.len_bytes(), text.len_bytes());
    assert_eq!(index.len_lines(), text.len_lines());
    for line in 0..index.len_lines() {
        let offset = index.line_to_offset(line).unwrap();
        let mapped = index.offset_to_line(offset).unwrap();
        assert_eq!(mapped.line, line, "line {line} at offset {offset}");
        assert_eq!(mapped.start, offset, "line {line} start");
    }
}

fn next(seed: &mut u64) -> u64 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *seed
}

fn stress(edits: usize) {
    let mut state = EditorState::from_text(&generate_file(100));
    let initial_chunks = state.index().chunk_count();
    assert_eq!(
        state.index().len_lines(),
        state.text().len_lines(),
        "initial line count"
    );
    let mut seed = 42_u64;
    for index in 0..edits {
        let len = state.text().len_bytes();
        let at = (next(&mut seed) as usize) % len;
        let op = next(&mut seed) % 10;
        match op {
            0..=6 => state.insert(at, "x"),
            7..=8 => {
                let end = (at + 1).min(len);
                if at < end {
                    state.delete(at..end);
                }
            }
            _ => state.insert(at, "x\n"),
        }
        assert_eq!(
            state.index().len_lines(),
            state.text().len_lines(),
            "line-count drift at edit {index}, at {at}, op {op}, len {len}"
        );
        if index % 1_000 == 0 {
            let chunks = state.index().chunk_count();
            println!("chunk-curve,{index},{chunks}");
            assert_eq!(
                state.index().len_lines(),
                state.text().len_lines(),
                "line-count drift at edit {index}"
            );
            assert!(chunks <= initial_chunks * 2 + 1);
            state.index().assert_chunk_bounds();
            let full_check_every = if edits > 10_000 { 10_000 } else { 1_000 };
            if index % full_check_every == 0 {
                assert_index_matches(state.text(), state.index());
            }
        }
    }
    let mut samples = Vec::new();
    for _ in 0..10_000 {
        let at = (next(&mut seed) as usize) % state.text().len_bytes();
        let start = Instant::now();
        let _ = state.index().offset_to_line(at);
        samples.push(start.elapsed().as_nanos());
    }
    samples.sort_unstable();
    let p99 = samples[samples.len() * 99 / 100];
    println!(
        "final-chunks,{},{}",
        state.index().chunk_count(),
        state.index().len_lines()
    );
    println!("offset-p99-ns,{p99}");
    assert!(p99 < 10_000, "offset_to_line p99 = {p99} ns");
}

#[test]
fn stress_10k_random_edits() {
    stress(10_000);
}

#[test]
#[ignore = "manual/nightly 100k stress"]
fn stress_100k_random_edits() {
    stress(100_000);
}
