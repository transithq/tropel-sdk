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
}
