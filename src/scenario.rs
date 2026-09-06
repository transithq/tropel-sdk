use crate::types::{AuthConfig, Request};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A protocol-agnostic scenario produced by an input adapter.
/// The executor iterates items in folder-order with setNextRequest flow control.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    /// Scenario metadata.
    pub info: ScenarioInfo,
    /// Top-level items (requests or folders).
    pub items: Vec<ScenarioItem>,
    /// Global variables.
    #[serde(default)]
    pub variables: HashMap<String, serde_json::Value>,
    /// Auth configuration applied to all requests.
    pub auth: Option<AuthConfig>,
    /// TR-410: conversion notes — warnings about items that were skipped,
    /// degraded, or lost during import. Structured as a list of human-
    /// readable strings so the client can render them in the import UI.
    /// Each note names the item and the reason (e.g. "Skipped item 'WebSocket
    /// chat': unsupported protocol").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conversion_notes: Vec<String>,
}

/// Scenario metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioInfo {
    pub name: String,
    pub description: Option<String>,
    pub schema: Option<String>,
}

/// An item in a scenario — either a single request or a folder of items.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioItem {
    /// Postman item id (`item.id` / `_postman_id`), used by
    /// `setNextRequest` which resolves ids BEFORE names (backlog §4).
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub request: Option<Request>,
    /// Pre-request scripts (JS code), outer→inner (collection → folder →
    /// request), each run in its OWN lexical scope. Postman runs each
    /// script as a separate compilation, so a `const baseUrl` at collection
    /// level and at request level must NOT collide, a top-level `return`
    /// only exits its own script, and each script compiles/caches
    /// independently (backlog §4: the old joined single string shared one
    /// scope — a redeclared const killed the whole chain).
    #[serde(default)]
    pub prerequest: Vec<String>,
    /// Test scripts (JS code, pm.test), outer→inner, same per-script-scope
    /// semantics as [`Self::prerequest`].
    #[serde(default)]
    pub test: Vec<String>,
    /// Assertions (Postman-style).
    #[serde(default)]
    pub assertions: Vec<String>,
    /// Child items (for folders).
    #[serde(default)]
    pub items: Vec<ScenarioItem>,
    /// What the user WROTE, where that differs from what gets sent.
    ///
    /// See [`RequestAuthoring`]. `None` for every producer that has no such
    /// detail, and skipped on the wire, so a Scenario is byte-identical to
    /// before unless an importer fills it in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authoring: Option<RequestAuthoring>,
}

/// Authoring detail an import preserves that the RUNTIME deliberately drops.
///
/// [`Request`] is what to send. A disabled header is simply absent from it, a
/// disabled body field never reaches the wire, and a query string is free to
/// stay in the URL because the client re-appends `query_params` anyway. All of
/// that is correct for executing a scenario and lossy for editing one: an API
/// client also has to show the user the header they turned OFF, and turn it
/// back on later.
///
/// Carried beside the request rather than inside it because the two answer
/// different questions, and because widening `Request` would push an
/// authoring-only concern through every runtime call site that builds one.
/// Every field is optional and skipped when empty, so nothing that ignores
/// this sees any change.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RequestAuthoring {
    /// Every header as written, disabled ones included, in declaration order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<AuthoredEntry>,
    /// Every query parameter as written — those inline in the URL AND those
    /// declared separately — disabled ones included, in declaration order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query: Vec<AuthoredEntry>,
    /// Request-scoped variables (Bruno `vars:pre-request`, `params:path`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vars: Vec<AuthoredEntry>,
    /// Per-request settings the runtime does not model on [`Request`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<AuthoredSettings>,
}

/// One authored key/value, with the on/off state the user chose.
///
/// A list of these rather than a map: a map cannot hold two parameters of the
/// same name, cannot keep declaration order, and has nowhere to put `enabled`.
/// `query_params` on [`Request`] is a map for the runtime, which needs none of
/// those three.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthoredEntry {
    pub key: String,
    pub value: String,
    /// `false` for an entry the user disabled (Bruno's `~name:` prefix).
    #[serde(default = "crate::scenario::default_true")]
    pub enabled: bool,
}

/// Per-request settings that are authoring state, not send state.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AuthoredSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encode_url: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_redirects: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_redirects: Option<u32>,
}

pub(crate) fn default_true() -> bool {
    true
}
