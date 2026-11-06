//! Stable request and response types for the Anvil plugin API.
//!
//! The crate currently defines the interaction-mode and view contracts from
//! ADR-0003 and ADR-0008. It deliberately does not depend on the editor, TUI,
//! or platform crates.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The public plugin API version carried by generated stubs.
pub const API_VERSION: &str = "1.0.0";

/// Stable JSON-RPC method names.
pub mod method {
    pub const EDITOR_GET_VIEW: &str = "editor.get_view";
    pub const EDITOR_GET_SNAPSHOT: &str = "editor.get_snapshot";
    pub const EDITOR_SET_MODE: &str = "editor.set_mode";
    pub const EDITOR_APPLY_EDIT: &str = "editor.apply_edit";
    pub const EDITOR_REGISTER_COMMAND: &str = "editor.register_command";
    pub const UI_NOTIFY: &str = "ui.notify";
    pub const UI_STATUS: &str = "ui.status";
    pub const EVENT_SUBSCRIBE: &str = "event.subscribe";
    pub const SYS_READ_FILE: &str = "sys.read_file";
    pub const SYS_CREATE_DIR: &str = "sys.create_dir";
    pub const SYS_RENAME: &str = "sys.rename";
    pub const SYS_REMOVE: &str = "sys.remove";
    pub const SYS_COPY: &str = "sys.copy";
    pub const SYS_REMOVE_ALL: &str = "sys.remove_all";
    pub const SYS_FETCH: &str = "sys.fetch";
    pub const SYS_EXEC: &str = "sys.exec";
}

/// A method parameter/result type in the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodType {
    /// No parameter object is sent.
    Empty,
    /// Named DTO in `anvil-api`.
    Named(&'static str),
}

/// One method's generated-catalog entry.
#[derive(Debug, Clone, Copy)]
pub struct MethodSpec {
    /// Stable JSON-RPC method name.
    pub name: &'static str,
    /// Parameter type.
    pub params: MethodType,
    /// Result type.
    pub result: MethodType,
}

/// The single source of truth for the plugin method catalog.
pub const METHODS: &[MethodSpec] = &[
    MethodSpec {
        name: method::EDITOR_GET_VIEW,
        params: MethodType::Empty,
        result: MethodType::Named("EditorView"),
    },
    MethodSpec {
        name: method::EDITOR_GET_SNAPSHOT,
        params: MethodType::Empty,
        result: MethodType::Named("DocumentSnapshot"),
    },
    MethodSpec {
        name: method::EDITOR_SET_MODE,
        params: MethodType::Named("SetModeRequest"),
        result: MethodType::Named("SetModeResponse"),
    },
    MethodSpec {
        name: method::EDITOR_APPLY_EDIT,
        params: MethodType::Named("ApplyEditRequest"),
        result: MethodType::Named("ApplyEditResponse"),
    },
    MethodSpec {
        name: method::EDITOR_REGISTER_COMMAND,
        params: MethodType::Named("RegisterCommandRequest"),
        result: MethodType::Named("RegisterCommandResponse"),
    },
    MethodSpec {
        name: method::UI_NOTIFY,
        params: MethodType::Named("NotifyRequest"),
        result: MethodType::Named("EmptyResponse"),
    },
    MethodSpec {
        name: method::UI_STATUS,
        params: MethodType::Named("StatusRequest"),
        result: MethodType::Named("EmptyResponse"),
    },
    MethodSpec {
        name: method::EVENT_SUBSCRIBE,
        params: MethodType::Named("EventSubscribeRequest"),
        result: MethodType::Named("EventSubscribeResponse"),
    },
    MethodSpec {
        name: method::SYS_READ_FILE,
        params: MethodType::Named("ReadFileRequest"),
        result: MethodType::Named("ReadFileResponse"),
    },
    MethodSpec {
        name: method::SYS_CREATE_DIR,
        params: MethodType::Named("PathRequest"),
        result: MethodType::Named("EmptyResponse"),
    },
    MethodSpec {
        name: method::SYS_RENAME,
        params: MethodType::Named("RenameRequest"),
        result: MethodType::Named("EmptyResponse"),
    },
    MethodSpec {
        name: method::SYS_REMOVE,
        params: MethodType::Named("PathRequest"),
        result: MethodType::Named("EmptyResponse"),
    },
    MethodSpec {
        name: method::SYS_COPY,
        params: MethodType::Named("CopyRequest"),
        result: MethodType::Named("EmptyResponse"),
    },
    MethodSpec {
        name: method::SYS_REMOVE_ALL,
        params: MethodType::Named("PathRequest"),
        result: MethodType::Named("EmptyResponse"),
    },
    MethodSpec {
        name: method::SYS_FETCH,
        params: MethodType::Named("FetchRequest"),
        result: MethodType::Named("FetchResponse"),
    },
    MethodSpec {
        name: method::SYS_EXEC,
        params: MethodType::Named("ExecRequest"),
        result: MethodType::Named("ExecResponse"),
    },
];

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

/// Half-open UTF-8 byte range used by document transactions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct TextRange {
    /// Inclusive UTF-8 byte offset.
    #[ts(type = "number")]
    #[schemars(range(max = 9007199254740991i64))]
    pub start: u64,
    /// Exclusive UTF-8 byte offset.
    #[ts(type = "number")]
    #[schemars(range(max = 9007199254740991i64))]
    pub end: u64,
}

/// One replacement in a document transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct DocumentEdit {
    pub range: TextRange,
    pub text: String,
}

/// Payload for `editor.apply_edit`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct ApplyEditRequest {
    /// Editor-owned buffer identity.
    #[ts(type = "number")]
    #[schemars(range(max = 9007199254740991i64))]
    pub buffer_id: u64,
    /// Revision the edits were computed against.
    #[ts(type = "number")]
    #[schemars(range(max = 9007199254740991i64))]
    pub base_revision: u64,
    pub edits: Vec<DocumentEdit>,
}

/// Result of `editor.apply_edit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct ApplyEditResponse {
    /// Revision after the transaction commits.
    #[ts(type = "number")]
    #[schemars(range(max = 9007199254740991i64))]
    pub revision: u64,
    /// Number of replacements applied.
    pub applied_edits: u32,
}

/// Result of `editor.get_snapshot`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct DocumentSnapshot {
    /// Editor-owned buffer identity.
    #[ts(type = "number")]
    #[schemars(range(max = 9007199254740991i64))]
    pub buffer_id: u64,
    /// Monotonic document revision at snapshot time.
    #[ts(type = "number")]
    #[schemars(range(max = 9007199254740991i64))]
    pub revision: u64,
    /// Full UTF-8 document text.
    pub text: String,
}

/// No result fields.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct EmptyResponse {}

/// Request to register a plugin command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct RegisterCommandRequest {
    pub id: String,
    pub title: String,
}

/// Result of registering a plugin command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct RegisterCommandResponse {
    pub command_id: String,
}

/// Notification severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum NotificationSeverity {
    Info,
    Warning,
    Error,
}

/// Request to show a transient notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct NotifyRequest {
    pub message: String,
    pub severity: NotificationSeverity,
}

/// Request to replace the status line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct StatusRequest {
    pub message: String,
}

/// Request to subscribe to editor events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct EventSubscribeRequest {
    pub events: Vec<String>,
}

/// Subscription identity returned by `event.subscribe`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct EventSubscribeResponse {
    #[ts(type = "number")]
    #[schemars(range(max = 9007199254740991i64))]
    pub subscription_id: u64,
}

/// Path-only filesystem request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct PathRequest {
    pub path: String,
}

/// Request to read a bounded file chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct ReadFileRequest {
    pub path: String,
    #[ts(type = "number")]
    #[schemars(range(max = 9007199254740991i64))]
    pub offset: u64,
    #[ts(type = "number")]
    #[schemars(range(max = 9007199254740991i64))]
    pub max_bytes: u64,
}

/// Base64-encoded file chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct ReadFileResponse {
    pub data: String,
    #[ts(type = "number")]
    #[schemars(range(max = 9007199254740991i64))]
    pub next_offset: u64,
    pub eof: bool,
}

/// Request to move or rename one workspace entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct RenameRequest {
    pub from: String,
    pub to: String,
}

/// Request to copy one workspace entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct CopyRequest {
    pub from: String,
    pub to: String,
}

/// Request to perform one allowlisted HTTP fetch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct FetchRequest {
    pub url: String,
    #[ts(type = "number")]
    #[schemars(range(max = 9007199254740991i64))]
    pub max_bytes: u64,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

/// Bounded HTTP fetch result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct FetchResponse {
    pub status: u16,
    pub content_type: String,
    pub body: String,
    pub truncated: bool,
}

/// Request to run one allowlisted tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct ExecRequest {
    pub tool: String,
    pub args: Vec<String>,
    #[ts(type = "number")]
    #[schemars(range(max = 9007199254740991i64))]
    pub timeout_ms: u64,
}

/// Sandboxed process result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct ExecResponse {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i64,
    pub timed_out: bool,
    pub output_truncated: bool,
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
        assert_eq!(method::EDITOR_GET_SNAPSHOT, "editor.get_snapshot");
        assert_eq!(method::EDITOR_SET_MODE, "editor.set_mode");
        assert_eq!(method::EDITOR_APPLY_EDIT, "editor.apply_edit");
        assert_eq!(method::SYS_EXEC, "sys.exec");
        assert_eq!(method::SYS_COPY, "sys.copy");
        assert_eq!(method::SYS_REMOVE_ALL, "sys.remove_all");
    }

    #[test]
    fn method_catalog_covers_every_constant() {
        let names = [
            method::EDITOR_GET_VIEW,
            method::EDITOR_GET_SNAPSHOT,
            method::EDITOR_SET_MODE,
            method::EDITOR_APPLY_EDIT,
            method::EDITOR_REGISTER_COMMAND,
            method::UI_NOTIFY,
            method::UI_STATUS,
            method::EVENT_SUBSCRIBE,
            method::SYS_READ_FILE,
            method::SYS_CREATE_DIR,
            method::SYS_RENAME,
            method::SYS_REMOVE,
            method::SYS_COPY,
            method::SYS_REMOVE_ALL,
            method::SYS_FETCH,
            method::SYS_EXEC,
        ];
        assert_eq!(METHODS.len(), names.len());
        for name in names {
            assert!(
                METHODS.iter().any(|spec| spec.name == name),
                "missing method catalog entry: {name}"
            );
        }
        let view = METHODS
            .iter()
            .find(|spec| spec.name == method::EDITOR_GET_VIEW)
            .expect("view method");
        assert_eq!(view.params, MethodType::Empty);
        let exec = METHODS
            .iter()
            .find(|spec| spec.name == method::SYS_EXEC)
            .expect("exec method");
        assert_eq!(exec.params, MethodType::Named("ExecRequest"));
        assert_eq!(exec.result, MethodType::Named("ExecResponse"));
    }

    #[test]
    fn transaction_contract_round_trips() {
        let request = ApplyEditRequest {
            buffer_id: 3,
            base_revision: 8,
            edits: vec![DocumentEdit {
                range: TextRange { start: 2, end: 4 },
                text: "ok".to_owned(),
            }],
        };
        let encoded = serde_json::to_string(&request).unwrap();
        let decoded: ApplyEditRequest = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, request);

        let snapshot = DocumentSnapshot {
            buffer_id: 3,
            revision: 9,
            text: "hello".to_owned(),
        };
        let decoded: DocumentSnapshot =
            serde_json::from_str(&serde_json::to_string(&snapshot).unwrap()).unwrap();
        assert_eq!(decoded, snapshot);
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
