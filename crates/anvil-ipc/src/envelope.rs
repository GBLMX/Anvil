//! JSON-RPC envelope and stable error codes from ADR-0007.
//!
//! The envelope is intentionally independent from the chosen transport. A
//! request with no `id` is a notification and receives no response.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// JSON-RPC version accepted by Anvil.
pub const JSONRPC_VERSION: &str = "2.0";

/// Maximum request id length.
pub const MAX_ID_LEN: usize = 64;

/// Stable Anvil error codes. Standard JSON-RPC codes remain unchanged.
pub mod codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;

    pub const UNAUTHORIZED: i32 = -32001;
    pub const POLICY_DENIED: i32 = -32002;
    pub const SESSION_REQUIRED: i32 = -32003;
    pub const CANCELLED: i32 = -32004;
    pub const TIMEOUT: i32 = -32005;
    pub const LIMIT_EXCEEDED: i32 = -32006;
    pub const NOT_FOUND: i32 = -32007;
    pub const UNAVAILABLE: i32 = -32008;
    pub const CONFLICT: i32 = -32009;
}

/// Optional transport metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_ms: Option<u64>,
}

/// A JSON-RPC request or notification.
///
/// Unknown top-level members are rejected so a capability token cannot
/// accidentally be put on every request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    #[serde(default)]
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub method: String,
    #[serde(default = "empty_params")]
    pub params: Value,
    #[serde(default)]
    pub meta: Option<Meta>,
}

impl Request {
    /// Validate the envelope independently of a method schema.
    pub fn validate(&self) -> Result<(), EnvelopeError> {
        if self.jsonrpc != JSONRPC_VERSION {
            return Err(EnvelopeError::Version);
        }
        if self.method.trim().is_empty() {
            return Err(EnvelopeError::Method);
        }
        if let Some(id) = &self.id
            && !valid_id(id)
        {
            return Err(EnvelopeError::Id);
        }
        if !self.params.is_object() {
            return Err(EnvelopeError::Params);
        }
        Ok(())
    }

    /// A request without an id is a notification.
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

/// A successful or failed JSON-RPC response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    #[serde(default)]
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<ErrorObject>,
}

impl Response {
    /// Validate the envelope and the result/error exclusivity.
    pub fn validate(&self) -> Result<(), EnvelopeError> {
        if self.jsonrpc != JSONRPC_VERSION {
            return Err(EnvelopeError::Version);
        }
        if let Some(id) = &self.id
            && !valid_id(id)
        {
            return Err(EnvelopeError::Id);
        }
        if self.result.is_some() == self.error.is_some() {
            return Err(EnvelopeError::ResponseShape);
        }
        Ok(())
    }
}

/// A JSON-RPC error object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorObject {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Envelope validation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvelopeError {
    Json(String),
    Version,
    Id,
    Method,
    Params,
    ResponseShape,
}

impl std::fmt::Display for EnvelopeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid JSON-RPC JSON: {error}"),
            Self::Version => write!(formatter, "unsupported JSON-RPC version"),
            Self::Id => write!(formatter, "invalid request id"),
            Self::Method => write!(formatter, "empty method"),
            Self::Params => write!(formatter, "params must be an object"),
            Self::ResponseShape => write!(
                formatter,
                "response must contain exactly one of result/error"
            ),
        }
    }
}

impl std::error::Error for EnvelopeError {}

/// Parse and validate one request from a JSON string.
pub fn parse_request(input: &str) -> Result<Request, EnvelopeError> {
    let request: Request =
        serde_json::from_str(input).map_err(|error| EnvelopeError::Json(error.to_string()))?;
    request.validate()?;
    Ok(request)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_ID_LEN && id.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
}

fn empty_params() -> Value {
    Value::Object(Map::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_a_request_with_meta() {
        let request = parse_request(
            r#"{"jsonrpc":"2.0","id":"01J","method":"sys.read_file","params":{},"meta":{"trace_id":"t","deadline_ms":5000}}"#,
        )
        .expect("request");
        assert_eq!(request.id.as_deref(), Some("01J"));
        assert_eq!(request.method, "sys.read_file");
        assert_eq!(
            request.meta.as_ref().and_then(|meta| meta.deadline_ms),
            Some(5000)
        );
        assert!(!request.is_notification());
    }

    #[test]
    fn missing_id_is_a_notification() {
        let request = parse_request(r#"{"jsonrpc":"2.0","method":"event.deliver","params":{}}"#)
            .expect("notification");
        assert!(request.is_notification());
    }

    #[test]
    fn rejects_unknown_top_level_members() {
        assert!(matches!(
            parse_request(
                r#"{"jsonrpc":"2.0","id":"1","method":"x","params":{},"token":"secret"}"#
            ),
            Err(EnvelopeError::Json(_))
        ));
    }

    #[test]
    fn rejects_bad_versions_ids_methods_and_params() {
        for input in [
            r#"{"jsonrpc":"1.0","id":"1","method":"x","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":"","method":"x","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":"1","method":"","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":"1","method":"x","params":[]}"#,
        ] {
            assert!(parse_request(input).is_err(), "{input}");
        }
    }

    #[test]
    fn response_requires_exactly_one_payload() {
        let response: Response = serde_json::from_value(json!({
            "jsonrpc":"2.0",
            "id":"1",
            "result": {"ok": true}
        }))
        .expect("response");
        assert!(response.validate().is_ok());

        let invalid: Response = serde_json::from_value(json!({
            "jsonrpc":"2.0",
            "id":"1"
        }))
        .expect("response");
        assert_eq!(invalid.validate(), Err(EnvelopeError::ResponseShape));
    }

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(codes::PARSE_ERROR, -32700);
        assert_eq!(codes::UNAUTHORIZED, -32001);
        assert_eq!(codes::CONFLICT, -32009);
    }
}
