use std::{hint::black_box, time::Instant};

use anvil_core::{GapBuffer, PieceTable, RopeBuffer, TextBuffer};

const VIEWPORT_LINES: usize = 24;
const LINE_WIDTH: usize = 80;
const EDITS_PER_ANCHOR: usize = 16;

fn synthetic_text(megabytes: usize) -> String {
    let target = megabytes * 1024 * 1024;
    let line = format!("{}\n", "x".repeat(LINE_WIDTH - 1));
    let mut text = String::with_capacity(target + line.len());
    while text.len() < target {
        text.push_str(&line);
    }
    text
}

fn run<B: TextBuffer>(name: &str, megabytes: usize, text: &str) {
    let open_start = Instant::now();
    let mut buffer = B::from_text(text);
    let open = open_start.elapsed();

    let lines = buffer.len_lines();
    let viewport_range = 0..(VIEWPORT_LINES * LINE_WIDTH).min(buffer.len_bytes());
    let viewport_start = Instant::now();
    let viewport = buffer.slice(viewport_range);
    let viewport_time = viewport_start.elapsed();

    let length = buffer.len_bytes();
    let edit_start = Instant::now();
    for anchor in [length / 100, length / 2, length * 99 / 100] {
        for _ in 0..EDITS_PER_ANCHOR {
            buffer.insert(anchor, "x");
            buffer.delete(anchor..anchor + 1);
        }
    }
    let edits = edit_start.elapsed();

    black_box(viewport);
    black_box(buffer.len_bytes());
    println!(
        "{name},{megabytes},{},{},{},{lines}",
        open.as_micros(),
        viewport_time.as_micros(),
        edits.as_micros()
    );
}

fn main() {
    let sizes: Vec<usize> = std::env::args()
        .skip(1)
        .filter_map(|argument| argument.parse().ok())
        .collect();
    let sizes = if sizes.is_empty() {
        vec![1, 10, 100]
    } else {
        sizes
    };

    println!("backend,mb,open_us,viewport_us,edits_us,lines");
    for megabytes in sizes {
        let text = synthetic_text(megabytes);
        run::<RopeBuffer>("rope", megabytes, &text);
        run::<GapBuffer>("gap", megabytes, &text);
        run::<PieceTable>("piece", megabytes, &text);
    }
}
