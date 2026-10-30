use std::{
    hint::black_box,
    os::unix::net::UnixStream,
    thread,
    time::{Duration, Instant},
};

use anvil_ipc::{JSONRPC_VERSION, Response, read_frame, write_frame};
use base64::{Engine as _, engine::general_purpose};
use serde_json::json;

const CHUNK: usize = 5 * 1024 * 1024 + 512 * 1024;
const SAMPLES: usize = 20;

fn main() {
    let raw = vec![b'a'; CHUNK];
    let encoded = general_purpose::STANDARD.encode(&raw);
    let response = Response {
        jsonrpc: JSONRPC_VERSION.to_owned(),
        id: Some("bench".to_owned()),
        result: Some(json!({
            "data": encoded,
            "next_offset": CHUNK,
            "eof": true
        })),
        error: None,
    };
    let body = serde_json::to_vec(&response).expect("response");

    measure("json_decode", SAMPLES, || {
        let decoded: Response = serde_json::from_slice(black_box(&body)).expect("decode");
        black_box(decoded);
    });
    measure("base64_decode", SAMPLES, || {
        let decoded = general_purpose::STANDARD
            .decode(black_box(encoded.as_bytes()))
            .expect("base64");
        black_box(decoded);
    });
    measure("append_string", SAMPLES, || {
        let mut output = Vec::with_capacity(CHUNK);
        output.extend_from_slice(black_box(&raw));
        let output = String::from_utf8(output).expect("utf8");
        black_box(output);
    });
    measure("client_decode", SAMPLES, || {
        let response: Response = serde_json::from_slice(black_box(&body)).expect("decode");
        let data = response
            .result
            .as_ref()
            .and_then(|result| result.get("data"))
            .and_then(serde_json::Value::as_str)
            .expect("data");
        let decoded = general_purpose::STANDARD
            .decode(black_box(data.as_bytes()))
            .expect("base64");
        let mut output = Vec::with_capacity(decoded.len());
        output.extend_from_slice(&decoded);
        let output = String::from_utf8(output).expect("utf8");
        black_box(output);
    });

    let (mut writer, mut reader) = UnixStream::pair().expect("socket pair");
    let socket_body = body.clone();
    let writer = thread::spawn(move || {
        for _ in 0..SAMPLES {
            write_frame(&mut writer, &socket_body).expect("write");
        }
    });
    let start = Instant::now();
    for _ in 0..SAMPLES {
        let frame = read_frame(&mut reader).expect("read").expect("frame");
        black_box(frame);
    }
    let socket_elapsed = start.elapsed();
    writer.join().expect("writer");
    println!(
        "socket_{}MiB_frame,avg_us={:.1}",
        CHUNK / (1024 * 1024),
        socket_elapsed.as_secs_f64() * 1_000_000.0 / SAMPLES as f64
    );
}

fn measure(name: &str, samples: usize, mut run: impl FnMut()) {
    let mut elapsed = Duration::ZERO;
    for _ in 0..samples {
        let start = Instant::now();
        run();
        elapsed += start.elapsed();
    }
    println!(
        "{name},avg_us={:.1}",
        elapsed.as_secs_f64() * 1_000_000.0 / samples as f64
    );
}
