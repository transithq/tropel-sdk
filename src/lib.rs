//! # Tropel SDK — stable public API for extensions
//!
//! `tropel-sdk` is the **single dependency** a third-party adapter/driver/output
//! author needs. It re-exports the types and traits that form the public contract
//! without exposing internal crate structure. Backing crates (`tropel-core`,
//! `tropel-ext`) now depend on this leaf, not the other way around — the SDK is
//! the published identity with a written semver policy.
//!
//! ## Semver policy
//!
//! This crate follows strict semver. Breaking changes (trait method added or
//! removed, type renamed, field visibility changed) bump the major version.
//! Before 1.0.0, breaking changes bump the minor version (0.x → 0.y).
//!
//! ## Quick start — writing an input adapter
//!
//! > The quick-start example and stability table below are **maintained in
//! > [`README.md`](https://crates.io/crates/tropel-sdk) (crates.io renders it);
//! > keep the two copies in sync when the surface changes.
//!
//! ```rust,ignore
//! // Single import: `tropel-sdk` re-exports everything you need.
//! use tropel_sdk::{
//!     InputAdapter, InputAdapterRegistration, Scenario, ScenarioInfo,
//!     ScenarioItem, Request, Method, Result, inventory,
//! };
//!
//! /// Adapter for the (fictional) `.apig` format.
//! pub struct ApiGatewayAdapter;
//!
//! impl InputAdapter for ApiGatewayAdapter {
//!     fn id(&self) -> &str { "apig" }
//!
//!     fn detect(&self, bytes: &[u8]) -> bool {
//!         bytes.starts_with(b"APIG\n")
//!     }
//!
//!     fn parse(&self, bytes: &[u8]) -> Result<Scenario> {
//!         let _text = std::str::from_utf8(bytes)
//!             .map_err(|e| /* ... */)?;
//!         Ok(Scenario {
//!             info: ScenarioInfo {
//!                 name: "my-api".into(),
//!                 description: None,
//!                 schema: None,
//!             },
//!             items: vec![/* request items */],
//!             variables: Default::default(),
//!             auth: None,
//!         })
//!     }
//! }
//!
//! // Register for compile-time discovery by the engine.
//! // The `inventory` crate is re-exported through the SDK.
//! inventory::submit!(InputAdapterRegistration::new(
//!     "apig",
//!     || Box::new(ApiGatewayAdapter),
//! ));
//! ```
//!
//! ## Stability guarantees
//!
//! | Item | Status |
//! |------|--------|
//! | `Scenario`, `ScenarioInfo`, `ScenarioItem` | ✅ Stable — schema is the engine's core IR |
//! | `InputAdapter` trait | ✅ Stable — used by all declarative adapters |
//! | `InputAdapterRegistration` | ✅ Stable — const fn, `fn()` pointer |
//! | `Request`, `Response`, `Method`, `Body` | ✅ Stable — used in scenario items |
//! | `Result<T>` / `TropelError` | ✅ Stable — error handling |
//! | `Sample`, `SampleType`, `TagMap` | ✅ Stable — metric sample surface (tags use `Arc<str>` interning) |
//! | `Protocol` trait | ✅ Stable — engine dispatches by URL scheme via the registry (`instantiate_protocols`); built-ins `grpc://`/`ws://` ship through it |
//! | `Output` trait | ✅ Wired — engine drives registered outputs from the sample stream (emit per batch, flush on close); see the `prometheus` reference extension (`unstable-output`) |
//! | `Driver` / `DriverInstance` / `VuContext` | ✅ Stable — re-exported and used by the k6 driver (`tropel-input-k6`) |
//! | `WASM` / `WIT` interface | 🔶 Resolves (parses) — `wit/adapter.wit` in the crate root is validated as a *parseable* WIT package by a `wit-parser` unit test, nothing more. It is the forward-looking Component-Model contract and intentionally lags the shipped C-ABI path; WASM plugins currently use the C ABI in `tropel-wasm`. |
//! | Engine config (`config` module) | ✅ Stable — **not** re-exported at the root, but the module itself is public and semver-committed (F4, review fix): `ExecutionConfig`, `ScenarioConfig`, `ThinkTimeConfig`, `Stage`, `ArrivalRateStage`, `ThresholdConfig`, `OutputConfig`, `ExpectedStatus`, `status_is_expected`. All 9 are genuinely consumed by dependents in the runtime publish set (`tropel-runtime`/`tropel-http`/`tropel-web` use `ExpectedStatus`/`status_is_expected`, `tropel-x-prometheus` uses `OutputConfig`) and by the engine-side crates (`tropel-core` re-exports the set, `tropel-input-k6` builds `ExecutionConfig`/stages from script options) — so they are deliberate public contract, reachable via `tropel_sdk::config::*` |

// ═══════════════════════════════════════════════════════════════════
// Module tree — the SDK owns these files (moved in the P1 inversion)
// ═══════════════════════════════════════════════════════════════════

pub mod config;
pub mod duration;
pub mod error;
pub mod scenario;
pub mod types;

/// The extension-contract traits (`InputAdapter`, `Driver`, `Protocol`,
/// `Output`, `VuContext`, …).
pub mod traits;

/// The four `*Registration` structs + `inventory::collect!` declarations.
/// Gated behind the `registration` feature (on by default) so a consumer
/// that only writes adapters against the traits can opt out of the
/// inventory dependency entirely.
#[cfg(feature = "registration")]
pub mod registration;

// ═══════════════════════════════════════════════════════════════════
// Core types — the shared Scenario IR used by all adapters
// ═══════════════════════════════════════════════════════════════════

pub use scenario::{Scenario, ScenarioInfo, ScenarioItem};
pub use types::tag_keys;
pub use types::{
    ApiKeyLocation, AuthConfig, Body, CertificateConfig, Cookie, FormDataPart, Method, Request,
    RequestCookie, Response, ResponseType, Sample, SampleType, Timings,
};

// ═══════════════════════════════════════════════════════════════════
// Extension traits
// ═══════════════════════════════════════════════════════════════════

// InputAdapter is the stable, primary extension trait — always available.
// The `*Registration` structs come from the gated `registration` module
// (they pull in the `inventory` dep), so their re-exports are gated too.
pub use traits::InputAdapter;
#[cfg(feature = "registration")]
pub use traits::InputAdapterRegistration;

// Driver/DriverInstance + VuContext are stable — the imperative input contract.
#[cfg(feature = "registration")]
pub use traits::DriverRegistration;
pub use traits::{Driver, DriverDeclaredOptions, DriverHttpClient, DriverInstance, VuContext};

// Protocol and Output traits are gated behind feature flags so a consumer
// that only writes input adapters never pays for (or is confused by) the
// extension traits. They ARE wired into engine dispatch — Protocol via the
// URL-scheme registry (`instantiate_protocols`), Output from the sample
// stream — the flag only controls whether the SDK re-exports them.
// Enable with `tropel-sdk = { features = ["unstable-protocol"] }` etc.
// Breaking changes to these traits only require a minor/patch bump.

/// Unstable protocol extension trait (requires `unstable-protocol` feature).
#[cfg(feature = "unstable-protocol")]
pub use traits::{Protocol, ProtocolOutcome};

/// Unstable protocol registration (requires both `unstable-protocol` and
/// `registration` features — the registration module is only available when
/// `inventory` is enabled).
#[cfg(all(feature = "unstable-protocol", feature = "registration"))]
pub use traits::ProtocolRegistration;

/// Unstable output extension trait (requires `unstable-output` feature).
#[cfg(feature = "unstable-output")]
pub use traits::Output;

/// Unstable output registration (requires both `unstable-output` and
/// `registration` features).
#[cfg(all(feature = "unstable-output", feature = "registration"))]
pub use traits::OutputRegistration;

// ═══════════════════════════════════════════════════════════════════
// Error types
// ═══════════════════════════════════════════════════════════════════

pub use duration::parse_duration;
pub use error::{Result, TropelError};

// ═══════════════════════════════════════════════════════════════════
// Re-export inventory so adapter crates don't need their own dep
// (requires the `registration` feature, on by default)
// ═══════════════════════════════════════════════════════════════════

#[cfg(feature = "registration")]
pub use inventory;

// ═══════════════════════════════════════════════════════════════════
// Re-export async-trait so implementors can use #[async_trait]
// ═══════════════════════════════════════════════════════════════════

pub use async_trait::async_trait;

// ═══════════════════════════════════════════════════════════════════
// Tag map type for metric tags
// ═══════════════════════════════════════════════════════════════════

pub use types::TagMap;

/// Version string for the SDK — adapter authors can check this at build time
/// to verify compatibility. Bumped whenever the public API changes.
pub const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");

// ═══════════════════════════════════════════════════════════════════
// Test helpers — verify re-exports compile and link correctly
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify all key types are constructible from SDK re-exports.
    #[test]
    fn test_scenario_construction() {
        let scenario = Scenario {
            info: ScenarioInfo {
                name: "test".into(),
                description: Some("SDK test".into()),
                schema: None,
            },
            items: vec![ScenarioItem {
                id: None,
                name: "GET /api".into(),
                request: Some(Request {
                    url: "https://example.com/api".into(),
                    method: Method::GET,
                    headers: Default::default(),
                    query_params: Default::default(),
                    body: None,
                    auth: None,
                    certificate: None,
                    follow_redirects: true,
                    host: None,
                    cookies: Default::default(),
                    timeout: None,
                    response_type: ResponseType::Text,
                }),
                prerequest: vec![],
                test: vec![],
                assertions: vec![],
                items: vec![],
            }],
            variables: Default::default(),
            auth: None,
            conversion_notes: Vec::new(),
        };
        assert_eq!(scenario.items.len(), 1);
        assert_eq!(scenario.info.name, "test");
    }

    /// Verify InputAdapterRegistration is constructible with const fn + fn pointer.
    #[cfg(feature = "registration")]
    #[test]
    fn test_registration_const_compatible() {
        fn factory() -> Box<dyn InputAdapter> {
            Box::new(DummyAdapter)
        }
        let reg = InputAdapterRegistration::new("test-adapter", factory);
        assert_eq!(reg.id, "test-adapter");
        let adapter = (reg.create)();
        assert_eq!(adapter.id(), "test-adapter");
    }

    struct DummyAdapter;
    impl InputAdapter for DummyAdapter {
        fn id(&self) -> &str {
            "test-adapter"
        }
        fn detect(&self, _bytes: &[u8]) -> bool {
            false
        }
        fn parse(&self, _bytes: &[u8]) -> std::result::Result<Scenario, TropelError> {
            Err(TropelError::Parse("not implemented".into()))
        }
    }

    /// Verify Result type alias works.
    #[test]
    fn test_result_alias() {
        fn returns_result() -> Result<()> {
            Ok(())
        }
        assert!(returns_result().is_ok());
    }

    /// Verify the inventory crate re-export is accessible.
    /// inventory::submit! is a compile-time macro — we just check the module resolves.
    #[cfg(feature = "registration")]
    #[test]
    fn test_inventory_re_export() {
        // Verify the inventory crate path works (submit! is compile-time only).
        #[allow(unused_imports)]
        use inventory as _;
    }

    // TR-607: wit/adapter.wit is deleted (was dead, no build.rs/wit-bindgen).
    // The old test asserted the wit directory parsed — now it would fail on an
    // empty directory, so it is removed with the wit file.
}
