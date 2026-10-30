use std::hint::black_box;

use anvil_core::{GapBuffer, PieceTable, RopeBuffer, TextBuffer};

fn synthetic_text(megabytes: usize) -> String {
    let target = megabytes * 1024 * 1024;
    let line = format!("{}\n", "x".repeat(79));
    let mut text = String::with_capacity(target + line.len());
    while text.len() < target {
        text.push_str(&line);
    }
    text
}

fn status_value(name: &str) -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if key != name {
            return None;
        }
        value
            .split_whitespace()
            .next()
            .and_then(|value| value.parse().ok())
    })
}

fn measure<B: TextBuffer>(name: &str, megabytes: usize, text: &str) {
    let buffer = B::from_text(text);
    let _ = black_box(buffer.len_lines());
    println!(
        "{name},{megabytes},{},{}",
        status_value("VmRSS").unwrap_or(0),
        status_value("VmHWM").unwrap_or(0)
    );
    black_box(buffer.len_bytes());
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let backend = arguments.next().unwrap_or_else(|| "rope".to_owned());
    let megabytes = arguments
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);
    let text = synthetic_text(megabytes);

    println!("backend,mb,rss_kb,hwm_kb");
    match backend.as_str() {
        "rope" => measure::<RopeBuffer>("rope", megabytes, &text),
        "gap" => measure::<GapBuffer>("gap", megabytes, &text),
        "piece" => measure::<PieceTable>("piece", megabytes, &text),
        other => eprintln!("unknown backend: {other}"),
    }
}
