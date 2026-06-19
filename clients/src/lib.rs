//! External API clients — one module per service.

/// Google Cloud Platform log client.
pub mod gcp;
/// GitHub REST and GraphQL clients.
pub mod github;
/// Linear GraphQL client.
pub mod linear;
/// Loki log query client.
pub mod loki;

/// Private API clients (feature-gated).
#[cfg(feature = "private")]
pub mod private;
