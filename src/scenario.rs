use crate::types::{AuthConfig, Request};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// A protocol-agnostic scenario produced by an input adapter.
/// The executor iterates items in folder-order with setNextRequest flow control.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScenarioInfo {
    pub name: String,
    pub description: Option<String>,
    pub schema: Option<String>,
}

/// An item in a scenario — either a single request or a folder of items.
/// `Default` is the forward-compatible way to build one of these.
///
/// Adding a public field to a struct with no `#[non_exhaustive]` breaks every
/// exhaustive literal — that is real (it is why 0.4.0 exists) and it is not
/// something this crate can promise not to do again before 1.0. What it can
/// do is give callers a construction that survives it:
///
/// ```
/// # use tropel_sdk::scenario::ScenarioItem;
/// let item = ScenarioItem { name: "GET /ping".into(), ..Default::default() };
/// ```
///
/// A literal written that way keeps compiling across a field addition. One
/// that names every field does not — which is a choice the caller gets to
/// make, and could not before, because these types had no `Default` at all.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// What the SPEC DECLARES, where a spec is what the item came from.
    ///
    /// See [`RequestContract`]. `None` for every producer that has no
    /// declaration to carry — which is every importer reading a collection
    /// rather than a schema — and skipped on the wire, so a Scenario is
    /// byte-identical to before unless an adapter fills it in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<RequestContract>,
}

/// What a SPEC declares about an operation, as against what a request sends.
///
/// The third view of one item, and the distinction is the point:
///
/// | field       | question it answers                    |
/// |-------------|----------------------------------------|
/// | [`Request`] | what gets SENT                         |
/// | `authoring` | what the user WROTE                    |
/// | `contract`  | what the SOURCE DECLARED               |
///
/// `tropel-input-openapi` reduces an OpenAPI operation to a Request by
/// SYNTHESIZING an example: a schema `{"name": {"type": "string"}}` becomes a
/// body `{"name": "string"}`, and a parameter becomes a query entry with a
/// placeholder value. That is exactly right for executing the spec — a load
/// run needs a body, not a schema — and it discards the declaration itself.
/// The adapter has been parsing and dropping it for that reason: eleven
/// `parsed for spec fidelity but not consumed` markers, `#[allow(dead_code)]`
/// on the structs holding responses, parameter types and `required` flags.
///
/// A consumer that wants to compare a COLLECTION against a SPEC needs the
/// declaration and cannot recover it from the example. `{"age": "integer"}`
/// as a body value is a type name that happens to look like data; reading it
/// back as a type would call a collection sending `{"age": 30}` a mismatch,
/// which is backwards. And a spec's declared RESPONSES have no representation
/// in a Request at all — there is nowhere for them to go.
///
/// Beside `Request` rather than inside it, for the reason `authoring` is:
/// widening `Request` would push a spec-only concern through every runtime
/// call site that builds one, and the runtime has no use for a declaration it
/// has already turned into an example.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RequestContract {
    /// Declared parameters, in declaration order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<DeclaredParameter>,
    /// The declared request body, when the operation has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<DeclaredBody>,
    /// Declared responses, keyed by status as the spec writes it — `"200"`,
    /// `"4XX"`, `"default"`.
    ///
    /// A map keyed by the SPEC'S OWN string rather than a `u16`: `default`
    /// and `4XX` are legal keys and neither parses as a number, so a numeric
    /// key would silently drop the two cases a drift check most wants to see.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub responses: BTreeMap<String, DeclaredResponse>,
    /// The path as the SPEC WRITES IT — `/users/{id}`, placeholders intact.
    ///
    /// `Request::url` cannot answer this. Building an executable example
    /// substitutes every path parameter (`/users/{id}` becomes
    /// `/users/example`), and that is right for a load run: you cannot GET a
    /// template. But the substituted value is indistinguishable from a real
    /// segment afterwards, so a consumer matching a collection's
    /// `/users/{id}` against a spec sees two different endpoints and reports
    /// every parameterized path as both "missing from the collection" and
    /// "not in the spec" — the one shape of false positive that makes a
    /// drift check useless, since parameterized paths are most of any REST
    /// API.
    ///
    /// Not recoverable from `name` either: that is `operation_id` or
    /// `summary` when the spec supplies one, and only falls back to
    /// `"GET /path"`.
    ///
    /// `None` for a source with no notion of a path template.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_template: Option<String>,
}

/// One declared parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeclaredParameter {
    pub name: String,
    /// `query`, `header`, `path`, `cookie` — the spec's `in`, verbatim.
    #[serde(rename = "in")]
    pub location: String,
    /// `false` unless the spec says otherwise, matching OpenAPI's default.
    #[serde(default)]
    pub required: bool,
    /// The declared type (`"string"`, `"integer"`, …), when the schema names
    /// one. `None` for a schema that is a `$ref`, a composition, or absent —
    /// reported as unknown rather than guessed, because a wrong type is worse
    /// than no type in a drift report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
}

/// A declared request body.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DeclaredBody {
    #[serde(default)]
    pub required: bool,
    /// Content types the operation accepts, in declaration order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content_types: Vec<String>,
    /// Top-level property names and their declared types.
    ///
    /// Names AND types, because both are recoverable from the schema here and
    /// neither is recoverable from the synthesized example. Nested properties
    /// are deliberately not flattened: a `a.b.c` path convention invented in
    /// the SDK would be a second schema language, and a consumer that wants
    /// depth can ask for the schema instead.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<DeclaredField>,
}

/// One declared body property.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeclaredField {
    pub name: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
}

/// A declared response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DeclaredResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Content types this response is declared to return.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content_types: Vec<String>,
    /// Top-level property names and types of the response body schema.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<DeclaredField>,
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

#[cfg(test)]
mod contract_tests {
    //! `ScenarioItem.contract` — what a SPEC declares.
    //!
    //! The first test is the one that matters: an item without a contract must
    //! serialize EXACTLY as it did before the field existed. That is the whole
    //! non-breaking claim, and it is the claim a `#[serde(skip_serializing_if)]`
    //! is easy to get subtly wrong.

    use super::*;

    fn bare_item() -> ScenarioItem {
        ScenarioItem {
            id: None,
            name: "GET /users".into(),
            request: None,
            prerequest: vec![],
            test: vec![],
            assertions: vec![],
            items: vec![],
            authoring: None,
            contract: None,
        }
    }

    #[test]
    fn an_item_without_a_contract_serializes_as_it_did_before() {
        let json = serde_json::to_string(&bare_item()).expect("serializes");
        // Not merely "no contract key" — the whole document must be unchanged,
        // so a producer that fills none of this cannot be told apart from one
        // built before the field existed.
        assert!(!json.contains("contract"), "got {json}");
    }

    #[test]
    fn a_document_written_before_the_field_still_deserializes() {
        // The other direction: an old Scenario JSON has no `contract` key at
        // all, and `#[serde(default)]` is what keeps it readable.
        let old = r#"{"name":"GET /users","prerequest":[],"test":[],"assertions":[],"items":[]}"#;
        let item: ScenarioItem = serde_json::from_str(old).expect("old documents still read");
        assert!(item.contract.is_none());
    }

    #[test]
    fn responses_are_keyed_by_the_specs_own_string() {
        // `default` and `4XX` are legal OpenAPI response keys and neither
        // parses as a number. A `u16` key would drop exactly the two cases a
        // drift check most wants to see.
        let mut responses = BTreeMap::new();
        responses.insert(
            "200".to_string(),
            DeclaredResponse {
                description: Some("ok".into()),
                ..Default::default()
            },
        );
        responses.insert("4XX".to_string(), DeclaredResponse::default());
        responses.insert("default".to_string(), DeclaredResponse::default());

        let item = ScenarioItem {
            contract: Some(RequestContract {
                responses,
                ..Default::default()
            }),
            ..bare_item()
        };
        let json = serde_json::to_string(&item).expect("serializes");
        let back: ScenarioItem = serde_json::from_str(&json).expect("round-trips");
        let carried = back.contract.expect("contract survives");
        assert_eq!(carried.responses.len(), 3);
        assert!(carried.responses.contains_key("4XX"));
        assert!(carried.responses.contains_key("default"));
    }

    #[test]
    fn an_unknown_parameter_type_is_none_rather_than_a_guess() {
        // A `$ref` or a composition names no type. Reporting it as unknown is
        // right; guessing `"string"` would make a drift report assert a type
        // the spec never declared.
        let p = DeclaredParameter {
            name: "filter".into(),
            location: "query".into(),
            required: true,
            r#type: None,
        };
        let json = serde_json::to_string(&p).expect("serializes");
        assert!(
            !json.contains("type"),
            "an absent type is omitted, got {json}"
        );
        let back: DeclaredParameter = serde_json::from_str(&json).expect("round-trips");
        assert_eq!(back.r#type, None);
        assert!(back.required);
    }

    #[test]
    fn required_defaults_to_false_as_openapi_says() {
        // OpenAPI's default for `required` is false. Defaulting to true would
        // make every optional parameter read as a drift finding.
        let p: DeclaredParameter =
            serde_json::from_str(r#"{"name":"page","in":"query"}"#).expect("reads");
        assert!(!p.required);
        let f: DeclaredField = serde_json::from_str(r#"{"name":"email"}"#).expect("reads");
        assert!(!f.required);
    }

    #[test]
    fn an_empty_contract_serializes_to_an_empty_object() {
        // Every field skips when empty, so `Some(RequestContract::default())`
        // is `{}` rather than three empty collections. A consumer can then
        // tell "the adapter filled nothing" from "the adapter did not run".
        let json = serde_json::to_string(&RequestContract::default()).expect("serializes");
        assert_eq!(json, "{}");
    }

    #[test]
    fn a_body_carries_its_content_types_and_field_types() {
        let body = DeclaredBody {
            required: true,
            content_types: vec!["application/json".into()],
            fields: vec![
                DeclaredField {
                    name: "name".into(),
                    required: true,
                    r#type: Some("string".into()),
                },
                DeclaredField {
                    name: "age".into(),
                    required: false,
                    r#type: Some("integer".into()),
                },
            ],
        };
        let back: DeclaredBody =
            serde_json::from_str(&serde_json::to_string(&body).expect("ser")).expect("de");
        assert_eq!(back, body);
        // The distinction the whole type exists for: a DECLARED integer,
        // beside a synthesized example whose value would be the string
        // "integer".
        assert_eq!(back.fields[1].r#type.as_deref(), Some("integer"));
    }

    #[test]
    fn a_path_template_survives_where_the_sent_url_cannot_carry_it() {
        // The pair this field exists to keep apart. A Request holds the
        // SUBSTITUTED url because that is what gets sent; the template is the
        // only thing that identifies the endpoint, and one is not derivable
        // from the other — `/users/example` could just as well be a literal
        // path segment named "example".
        let contract = RequestContract {
            path_template: Some("/users/{id}".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&contract).expect("serializes");
        assert_eq!(json, r#"{"path_template":"/users/{id}"}"#);
        let back: RequestContract = serde_json::from_str(&json).expect("reads");
        assert_eq!(back.path_template.as_deref(), Some("/users/{id}"));
    }

    #[test]
    fn a_contract_with_no_path_template_still_serializes_to_nothing() {
        // The `an_empty_contract_serializes_to_an_empty_object` guarantee has
        // to survive each field added to this struct, or a source with no
        // notion of paths starts emitting `"path_template":null`.
        assert_eq!(
            serde_json::to_string(&RequestContract::default()).expect("ser"),
            "{}"
        );
    }
}
