//! UI-neutral provider identity, cancellation, and registration.

mod cancel;
mod completion;
mod error;
mod exec_format;
mod format;
mod id;
mod picker;
mod registry;

pub use cancel::CancellationToken;
pub use completion::{
    CompletionContext, CompletionItem, CompletionKind, CompletionPathEntry, CompletionSource,
};
pub use error::{ProviderError, RegistryError};
pub use exec_format::{ExecFormatter, ExecOutput, ExecV2};
pub use format::{FormatContext, FormattedText, FormatterKind, FormatterProvider};
pub use id::{ProviderId, ProviderOrigin, ProviderPriority};
pub use picker::{
    PickerBuffer, PickerContext, PickerEntry, PickerPreview, PickerQuery, PickerSource,
};
pub use registry::{ProviderRegistry, RegisteredProvider};
