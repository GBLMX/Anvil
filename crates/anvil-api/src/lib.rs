//! Stable request and response types for the Anvil plugin API.
//!
//! The crate currently defines the interaction-mode and view contracts from
//! ADR-0003 and ADR-0008. It deliberately does not depend on the editor, TUI,
//! or platform crates.

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

/// A position in the document.
///
/// All indices are zero-based. `byte_offset` is a UTF-8 byte offset;
/// `grapheme_column` counts extended grapheme clusters; `display_column`
/// counts terminal cells and accounts for wide characters. The editor owns the
/// conversions and guarantees the fields describe the same cursor position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextPosition {
    /// UTF-8 byte offset from the start of the document.
    pub byte_offset: u64,
    /// Zero-based line number.
    pub line: u32,
    /// Zero-based grapheme-cluster column.
    pub grapheme_column: u32,
    /// Zero-based terminal display-cell column.
    pub display_column: u32,
}

/// The visible region of the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    /// Fixed end of the selection.
    pub anchor: TextPosition,
    /// Moving end of the selection.
    pub head: TextPosition,
}

/// Result of `editor.get_view`.
///
/// Phase 0 has at least one selection and one native mode (`select`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorView {
    /// Monotonic document revision at snapshot time.
    pub revision: u64,
    /// Session interaction mode.
    pub interaction_mode: InteractionMode,
    /// Visible document region.
    pub viewport: Viewport,
    /// Current selections; Phase 0 normally contains one caret/selection.
    pub selections: Vec<Selection>,
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
    fn caret_is_empty_selection() {
        let position = position();
        let selection = Selection {
            anchor: position,
            head: position,
        };
        assert_eq!(selection.anchor, selection.head);
    }
}
