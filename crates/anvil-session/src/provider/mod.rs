//! UI-neutral provider identity, cancellation, and registration.

mod cancel;
mod error;
mod id;
mod registry;

pub use cancel::CancellationToken;
pub use error::{ProviderError, RegistryError};
pub use id::{ProviderId, ProviderOrigin, ProviderPriority};
pub use registry::{ProviderRegistry, RegisteredProvider};
