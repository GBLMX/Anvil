//! Stable request and response types for the Anvil plugin API.
//!
//! The crate currently defines the interaction-mode contract from ADR-0008.
//! It deliberately does not depend on the editor, TUI, or platform crates.

use serde::{Deserialize, Serialize};

/// The public plugin API version carried by generated stubs.
pub const API_VERSION: &str = "0.1.0";

/// The interaction model active for the current editor session.
///
/// Phase 0 has one native model, `select`. A caret is an empty selection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum InteractionMode {
    /// Selection-first editing. Insertion and navigation both act on a selection.
    #[default]
    Select,
}

impl InteractionMode {
    /// Returns the wire representation used by the plugin API.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Select => "select",
        }
    }
}

/// Payload for `editor.set_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetModeRequest {
    /// The mode to activate for the current session.
    pub mode: InteractionMode,
}

/// Result of `editor.set_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetModeResponse {
    /// The mode active after the request.
    pub mode: InteractionMode,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_mode_round_trips() {
        let encoded = serde_json::to_string(&InteractionMode::Select).unwrap();
        assert_eq!(encoded, r#""select""#);
        let decoded: InteractionMode = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, InteractionMode::Select);
    }

    #[test]
    fn unknown_mode_is_rejected() {
        assert!(serde_json::from_str::<InteractionMode>(r#""vim""#).is_err());
    }

    #[test]
    fn set_mode_round_trips() {
        let request = SetModeRequest {
            mode: InteractionMode::Select,
        };
        let encoded = serde_json::to_string(&request).unwrap();
        assert_eq!(encoded, r#"{"mode":"select"}"#);
        let decoded: SetModeRequest = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, request);

        let response = SetModeResponse {
            mode: InteractionMode::Select,
        };
        let encoded = serde_json::to_string(&response).unwrap();
        assert_eq!(encoded, r#"{"mode":"select"}"#);
    }
}
