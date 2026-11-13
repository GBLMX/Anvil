//! UI-neutral provider identity, cancellation, and registration.

mod cancel;
mod completion;
mod error;
mod id;
mod picker;
mod registry;

pub use cancel::CancellationToken;
pub use completion::{CompletionContext, CompletionItem, CompletionKind, CompletionSource};
pub use error::{ProviderError, RegistryError};
pub use id::{ProviderId, ProviderOrigin, ProviderPriority};
pub use picker::{
    PickerBuffer, PickerContext, PickerEntry, PickerPreview, PickerQuery, PickerSource,
};
pub use registry::{ProviderRegistry, RegisteredProvider};
