//! First Rust-to-Go gateway handshake.
//!
//! Run the gateway with the SHA-256 of the 32-byte token
//! `0123456789abcdef0123456789abcdef`, then run:
//!
//! `cargo run -p anvil-ipc --example gateway_handshake -- /path/to/gateway.sock`

use std::env;
use std::error::Error;

use anvil_ipc::{Connection, JSONRPC_VERSION, Request, Response};
use serde_json::{Value, json};

const TOKEN_BASE64URL: &str = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY";

fn main() -> Result<(), Box<dyn Error>> {
    let socket = env::args()
        .nth(1)
        .ok_or("usage: gateway_handshake <gateway.sock>")?;
    let mut connection = Connection::connect(socket)?;
    let request = Request {
        jsonrpc: JSONRPC_VERSION.to_owned(),
        id: Some("handshake-1".to_owned()),
        method: "session.hello".to_owned(),
        params: json!({
            "token": TOKEN_BASE64URL,
            "api_version": "0.1.0",
            "protocol": "anvil-jsonrpc-1"
        }),
        meta: None,
    };
    connection.send(&serde_json::to_vec(&request)?)?;
    let frame = connection
        .recv()?
        .ok_or("gateway closed before handshake response")?;
    let response: Response = serde_json::from_slice(&frame)?;
    response.validate()?;
    let value: Value = serde_json::to_value(response)?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
