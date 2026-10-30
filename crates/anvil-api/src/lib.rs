//! Stable request and response types for the Anvil plugin API.
//!
//! The crate currently defines the interaction-mode and view contracts from
//! ADR-0003 and ADR-0008. It deliberately does not depend on the editor, TUI,
//! or platform crates.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The public plugin API version carried by generated stubs.
pub const API_VERSION: &str = "0.1.0";

/// Stable JSON-RPC method names.
pub mod method {
    pub const EDITOR_GET_VIEW: &str = "editor.get_view";
    pub const EDITOR_SET_MODE: &str = "editor.set_mode";
    pub const EDITOR_APPLY_EDIT: &str = "editor.apply_edit";
    pub const EDITOR_REGISTER_COMMAND: &str = "editor.register_command";
    pub const UI_NOTIFY: &str = "ui.notify";
    pub const UI_STATUS: &str = "ui.status";
    pub const EVENT_SUBSCRIBE: &str = "event.subscribe";
    pub const SYS_READ_FILE: &str = "sys.read_file";
    pub const SYS_FETCH: &str = "sys.fetch";
    pub const SYS_EXEC: &str = "sys.exec";
}

/// Stable plugin-facing error kinds.
///
/// The numeric code mapping follows ADR-0007. `InvalidMode` and `Unsupported`
/// are method-level kinds that reuse the closest transport code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ApiError {
    InvalidMode,
    Unsupported,
    ParseError,
    InvalidRequest,
    MethodNotFound,
    InvalidParams,
    Unauthorized,
    PolicyDenied,
    SessionRequired,
    Cancelled,
    Timeout,
    LimitExceeded,
    NotFound,
    Unavailable,
    Conflict,
    InternalError,
}

impl ApiError {
    /// Stable numeric code from ADR-0007.
    pub const fn code(self) -> i32 {
        match self {
            Self::ParseError => -32700,
            Self::InvalidRequest => -32600,
            Self::MethodNotFound => -32601,
            Self::InvalidMode | Self::InvalidParams => -32602,
            Self::InternalError => -32603,
            Self::Unauthorized => -32001,
            Self::PolicyDenied => -32002,
            Self::SessionRequired => -32003,
            Self::Cancelled => -32004,
            Self::Timeout => -32005,
            Self::LimitExceeded => -32006,
            Self::NotFound => -32007,
            Self::Unavailable | Self::Unsupported => -32008,
            Self::Conflict => -32009,
        }
    }

    /// Whether the caller may retry without changing the request.
    pub const fn retryable(self) -> bool {
        matches!(self, Self::Timeout | Self::Unavailable)
    }
}

/// The interaction model active for the current editor session.
///
/// Phase 0 has one native model, `select`. A caret is an empty selection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum InteractionMode {
    /// Selection-first editing. Insertion and navigation both act on a selection.
    #[default]
    Select,
}

/// A position in the document.
///
/// All indices are zero-based. `byte_offset` is a UTF-8 byte offset;
/// `grapheme_column` counts extended grapheme clusters; `display_column`
/// counts terminal cells and accounts for wide characters. The editor owns the
/// conversions and guarantees the fields describe the same cursor position.
/// Plugins receive positions from `editor.get_view`; construction stays
/// editor-owned until a future `set_selection` contract validates it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct TextPosition {
    /// UTF-8 byte offset from the start of the document.
    #[ts(type = "number")]
    #[schemars(range(max = 9007199254740991i64))]
    pub byte_offset: u64,
    /// Zero-based line number.
    pub line: u32,
    /// Zero-based grapheme-cluster column.
    pub grapheme_column: u32,
    /// Zero-based terminal display-cell column.
    pub display_column: u32,
}

/// The visible region of the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct Viewport {
    /// First visible line.
    pub first_line: u32,
    /// First visible display column for horizontal scrolling.
    pub first_column: u32,
    /// Number of visible lines.
    pub line_count: u32,
    /// Visible width in terminal cells.
    pub width: u32,
    /// Visible height in terminal rows.
    pub height: u32,
}

/// One selection. A caret is an empty selection where `anchor == head`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct Selection {
    /// Fixed end of the selection.
    pub anchor: TextPosition,
    /// Moving end of the selection.
    pub head: TextPosition,
}

/// Result of `editor.get_view`.
///
/// Phase 0 has at least one selection and one native mode (`select`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct EditorView {
    /// Monotonic document revision at snapshot time.
    #[ts(type = "number")]
    #[schemars(range(max = 9007199254740991i64))]
    pub revision: u64,
    /// Session interaction mode.
    pub interaction_mode: InteractionMode,
    /// Visible document region.
    pub viewport: Viewport,
    /// Current selections; Phase 0 normally contains one caret/selection.
    pub selections: Vec<Selection>,
}

/// Payload for `editor.set_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct SetModeRequest {
    /// The mode to activate for the current session.
    pub mode: InteractionMode,
}

/// Result of `editor.set_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct SetModeResponse {
    /// The mode active after the request.
    pub mode: InteractionMode,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position() -> TextPosition {
        TextPosition {
            byte_offset: 12,
            line: 1,
            grapheme_column: 4,
            display_column: 6,
        }
    }

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

    #[test]
    fn view_round_trips_with_caret() {
        let view = EditorView {
            revision: 7,
            interaction_mode: InteractionMode::Select,
            viewport: Viewport {
                first_line: 1,
                first_column: 0,
                line_count: 24,
                width: 80,
                height: 24,
            },
            selections: vec![Selection {
                anchor: position(),
                head: position(),
            }],
        };
        let encoded = serde_json::to_string(&view).unwrap();
        let decoded: EditorView = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, view);
    }

    #[test]
    fn schema_exposes_mode_and_safe_integer_bounds() {
        let schema = serde_json::to_value(schemars::schema_for!(EditorView)).unwrap();
        assert_eq!(
            schema["properties"]["interaction_mode"]["$ref"],
            "#/$defs/InteractionMode"
        );
        assert_eq!(
            schema["properties"]["revision"]["maximum"].as_u64(),
            Some(9_007_199_254_740_991)
        );
    }

    #[test]
    fn caret_is_empty_selection() {
        let position = position();
        let selection = Selection {
            anchor: position,
            head: position,
        };
        assert_eq!(selection.anchor, selection.head);
    }

    #[test]
    fn method_names_are_stable() {
        assert_eq!(method::EDITOR_GET_VIEW, "editor.get_view");
        assert_eq!(method::EDITOR_SET_MODE, "editor.set_mode");
        assert_eq!(method::SYS_EXEC, "sys.exec");
    }

    #[test]
    fn api_errors_map_to_stable_codes() {
        assert_eq!(ApiError::InvalidMode.code(), -32602);
        assert_eq!(ApiError::Unsupported.code(), -32008);
        assert_eq!(ApiError::Timeout.code(), -32005);
        assert_eq!(ApiError::Conflict.code(), -32009);
        assert!(ApiError::Timeout.retryable());
        assert!(!ApiError::InvalidMode.retryable());
    }
}
