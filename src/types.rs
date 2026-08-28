use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

/// HTTP method.
///
/// In addition to the standard nine, any valid HTTP token (RFC 7230
/// `tchar`) is representable as [`Method::Custom`] — so `PURGE`, `LINK`,
/// `PROPFIND`, etc. load-test the write path they name instead of silently
/// degrading to `GET` (k6 passes any token through to the HTTP client).
/// Custom serde keeps the wire format a plain uppercase string (`"GET"`,
/// `"PURGE"`), matching the old derived `rename_all = "UPPERCASE"` shape.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Method {
    GET,
    HEAD,
    POST,
    PUT,
    PATCH,
    DELETE,
    OPTIONS,
    TRACE,
    CONNECT,
    /// Any valid non-standard HTTP token (e.g. `PURGE`, `LINK`), preserved
    /// as written after trimming. Never constructed for the standard nine.
    Custom(String),
}

impl Method {
    /// Return the method as a `&str` without allocating (the standard nine
    /// are static strings; `Custom` returns its stored token). Hot-path
    /// alternative to `to_string()` — avoids a String alloc per request.
    pub fn as_str(&self) -> &str {
        match self {
            Method::GET => "GET",
            Method::HEAD => "HEAD",
            Method::POST => "POST",
            Method::PUT => "PUT",
            Method::PATCH => "PATCH",
            Method::DELETE => "DELETE",
            Method::OPTIONS => "OPTIONS",
            Method::TRACE => "TRACE",
            Method::CONNECT => "CONNECT",
            Method::Custom(m) => m.as_str(),
        }
    }
}

impl std::fmt::Display for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Serialize for Method {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Method {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Method::parse(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid HTTP method {:?}", s)))
    }
}

impl Method {
    /// Parse a method from a string (case-insensitive).
    ///
    /// Named `parse` (not `from_str`) to avoid colliding with the standard
    /// `FromStr::from_str` — this variant returns `Option`, not `Result`,
    /// so it is deliberately NOT the trait impl.
    ///
    /// - Leading/trailing whitespace is trimmed (`" GET"` → `GET`).
    /// - Any RFC 7230 `tchar` token is accepted: the standard nine map to
    ///   their variants, anything else becomes [`Method::Custom`] (so a
    ///   write-path method like `PURGE` is sent as-is).
    /// - `None` only for genuinely invalid input: empty, whitespace inside
    ///   the token, or characters outside the HTTP token set (a typo like
    ///   `"POTS"` is still a VALID token and round-trips as `Custom`).
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        // RFC 7230 tchar = "!#$%&'*+-.^_`|~" / DIGIT / ALPHA.
        if !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c))
        {
            return None;
        }
        match s.to_uppercase().as_str() {
            "GET" => Some(Method::GET),
            "HEAD" => Some(Method::HEAD),
            "POST" => Some(Method::POST),
            "PUT" => Some(Method::PUT),
            "PATCH" => Some(Method::PATCH),
            "DELETE" => Some(Method::DELETE),
            "OPTIONS" => Some(Method::OPTIONS),
            "TRACE" => Some(Method::TRACE),
            "CONNECT" => Some(Method::CONNECT),
            _ => Some(Method::Custom(s.to_string())),
        }
    }
}

/// How the response body should be handled for a request.
///
/// Mirrors k6's per-request `params.responseType`:
/// - `Text` (default): body kept as raw bytes; decoded to text lazily on
///   access (`body_text()` / `body_json()`).
/// - `Binary`: body kept as raw bytes, surfaced as-is to scripts that want
///   binary payloads (base64 etc.).
/// - `None`: body is discarded entirely — `execute()` drains it off the wire
///   (so the pooled connection stays reusable) but stores no bytes. Pairs
///   with the global `discardResponseBodies`; scripts see an empty body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ResponseType {
    #[default]
    Text,
    Binary,
    None,
}

impl ResponseType {
    /// Parse from a k6-style string (`"text"`, `"binary"`, `"none"`).
    /// Unrecognized/empty values fall back to `Text` (k6's default).
    pub fn from_k6(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "binary" => ResponseType::Binary,
            "none" => ResponseType::None,
            _ => ResponseType::Text,
        }
    }
}

/// A single HTTP request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    /// URL string (may contain {{variables}}).
    pub url: String,
    pub method: Method,
    /// Request headers, in DECLARATION ORDER with duplicates preserved
    /// (W2 #203: the old `HashMap` collapsed two `Cookie:` headers into one
    /// and let header order vary request-to-request). Deserializes from
    /// BOTH the legacy object form (`{"name":"value"}`) and the
    /// order/duplicate-preserving array-of-pairs form; serialization emits
    /// the array form so duplicates and order survive a round-trip.
    #[serde(default, deserialize_with = "de_headers")]
    pub headers: Vec<(String, String)>,
    /// Query parameters.
    #[serde(default)]
    pub query_params: HashMap<String, String>,
    /// Request body.
    pub body: Option<Body>,
    /// Auth configuration.
    pub auth: Option<AuthConfig>,
    /// Certificate configuration.
    pub certificate: Option<CertificateConfig>,
    /// Whether to follow redirects.
    #[serde(default = "default_true")]
    pub follow_redirects: bool,
    /// Host override — k6 `req.Host` (TR-230). A `Host` key in the request
    /// headers sets this instead of sending a plain `Host` header: the wire
    /// carries `Host: <value>` (reqwest honors a user-set Host header), but
    /// `res.request.headers` must not list it (k6 keeps Host off
    /// `req.Header`). `None` → the URL's host is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Request cookies — k6 `params.cookies` (TR-230). Carried separately
    /// (not folded into the Cookie header) so the client can implement k6's
    /// jar merge: a `replace: false` (default) request cookie is sent
    /// ALONGSIDE the per-VU jar cookie of the same name; `replace: true`
    /// suppresses the jar's. `req.Host`-style: kept off the header list so
    /// the wire is built in one place.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cookies: Vec<RequestCookie>,
    /// Connection/read timeout.
    pub timeout: Option<Duration>,
    /// How to handle the response body (k6 `params.responseType`).
    #[serde(default)]
    pub response_type: ResponseType,
}

/// A single request cookie from k6 `params.cookies` — `{name: value}` or the
/// `{name: {value, replace}}` form (TR-230).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestCookie {
    pub name: String,
    pub value: String,
    /// `replace: true` → this cookie replaces the per-VU jar cookie of the
    /// same name; `false` (k6's default) → both are sent.
    #[serde(default)]
    pub replace: bool,
}

/// Deserialize request headers from EITHER the legacy JSON object form
/// (`{"name": "value"}`) or the duplicate/order-preserving array form
/// (`[["name", "value"], ...]`) (W2 #203).
fn de_headers<'de, D>(deserializer: D) -> Result<Vec<(String, String)>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Headers {
        List(Vec<(String, String)>),
        Map(std::collections::BTreeMap<String, String>),
    }
    Ok(match Headers::deserialize(deserializer)? {
        Headers::List(list) => list,
        Headers::Map(map) => map.into_iter().collect(),
    })
}

fn default_protocol() -> String {
    "HTTP/1.1".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for Request {
    fn default() -> Self {
        Self {
            url: String::new(),
            method: Method::GET,
            headers: Vec::new(),
            query_params: HashMap::new(),
            body: None,
            auth: None,
            certificate: None,
            follow_redirects: true,
            host: None,
            cookies: Vec::new(),
            timeout: None,
            response_type: ResponseType::Text,
        }
    }
}

/// Request body variants.
///
/// Custom serde: `#[serde(untagged)]` made `Json(Value)` match ANY JSON, so
/// `FormData`/`UrlEncoded`/`Binary`/`GraphQL` were unreachable on
/// deserialize — every JSON round-trip (distributed workers, spool, replay)
/// silently converted `UrlEncoded` → `Json`: Content-Type flipped and wire
/// bytes changed from `a=1&b=2` to `{"a":"1"}` (a form endpoint 400s on the
/// worker and passes locally).
///
/// Wire format (backward compatible for the two common cases):
/// - `Raw(s)` → JSON string (unchanged)
/// - `Json(v)` → the raw JSON value (unchanged)
/// - the other four → a tagged object `{"__tropel_body": "<kind>", …}` so
///   they survive a round-trip intact instead of collapsing into `Json`.
///
/// Known limitation: a user `Json` body that legitimately contains a
/// `__tropel_body` key with a recognized tag value (e.g.
/// `{"__tropel_body": "url_encoded", "fields": …}`) is interpreted as that
/// variant. Unknown tag values are treated as plain `Json` (the key is
/// preserved), so only the four exact tag strings are ambiguous.
/// A single `multipart/form-data` part.
///
/// Line 198: form-data parts must distinguish text fields from file
/// uploads. A file part carries its `filename`, per-part Content-Type and
/// RAW bytes — the old `HashMap<String, String>` forced `from_utf8_lossy`
/// (a PNG became U+FFFD soup of a different length, so `data_sent` was
/// wrong) and the multipart builder wrote no `filename=` (mainstream
/// parsers key the file branch off `filename`).
///
/// NOTE: this CHANGED the Body wire format for `form_data` — `fields` was
/// a JSON object (name → value) and is now a JSON ARRAY of part objects.
/// Old spooled/worker-serialized bodies with the object shape fail
/// `serde_json::from_value` and collapse to an empty vec; all consumers
/// were upgraded together in line 198.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormDataPart {
    pub name: String,
    /// Text-field value (`None` for file parts).
    pub value: Option<String>,
    /// File part: original file name (`None` for text fields).
    pub filename: Option<String>,
    /// File part: per-part Content-Type (`None` for text fields).
    pub mime: Option<String>,
    /// File part: raw bytes (`None` for text fields or missing files).
    pub data: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub enum Body {
    Raw(String),
    Json(serde_json::Value),
    FormData(Vec<FormDataPart>),
    /// Duplicate keys preserved in declaration order (W2 #203: the old
    /// `HashMap` collapsed `tag=a`+`tag=b` into one field).
    UrlEncoded(Vec<(String, String)>),
    Binary(Vec<u8>),
    GraphQL {
        query: String,
        variables: Option<HashMap<String, serde_json::Value>>,
    },
}

impl Serialize for Body {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        match self {
            Body::Raw(s) => serializer.serialize_str(s),
            Body::Json(v) => {
                // Preserve Json(String) round-trip: a plain string collides with Raw.
                // String payloads are wrapped with a discriminator so Raw vs Json(String)
                // survive a distributed round-trip with different wire bytes preserved.
                if let serde_json::Value::String(s) = v {
                    let mut map = serializer.serialize_map(Some(2))?;
                    map.serialize_entry("__tropel_body", "json")?;
                    map.serialize_entry("value", s)?;
                    return map.end();
                }
                v.serialize(serializer)
            }
            Body::FormData(fields) => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("__tropel_body", "form_data")?;
                map.serialize_entry("fields", fields)?;
                map.end()
            }
            Body::UrlEncoded(fields) => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("__tropel_body", "url_encoded")?;
                map.serialize_entry("fields", fields)?;
                map.end()
            }
            Body::Binary(data) => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("__tropel_body", "binary")?;
                map.serialize_entry("data", data)?;
                map.end()
            }
            Body::GraphQL { query, variables } => {
                let mut map = serializer.serialize_map(Some(3))?;
                map.serialize_entry("__tropel_body", "graphql")?;
                map.serialize_entry("query", query)?;
                map.serialize_entry("variables", variables)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Body {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            // A JSON string is a Raw body (unchanged wire format).
            serde_json::Value::String(s) => Ok(Body::Raw(s)),
            serde_json::Value::Object(mut obj) => {
                // Backlog line 247: peek at __tropel_body before removing —
                // if it's NOT a valid discriminator string, the key must be
                // preserved in the Json body. The old code unconditionally
                // removed it, silently deleting any user key named
                // __tropel_body.
                let tag = obj
                    .get("__tropel_body")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                match tag.as_deref() {
                    Some("form_data") => {
                        obj.remove("__tropel_body");
                        let fields = obj.remove("fields").map(de_form_fields).unwrap_or_default();
                        Ok(Body::FormData(fields))
                    }
                    Some("url_encoded") => {
                        obj.remove("__tropel_body");
                        let fields = obj
                            .remove("fields")
                            .map(de_urlencoded_fields)
                            .unwrap_or_default();
                        Ok(Body::UrlEncoded(fields))
                    }
                    Some("binary") => {
                        obj.remove("__tropel_body");
                        let data = obj
                            .remove("data")
                            .and_then(|d| serde_json::from_value(d).ok())
                            .unwrap_or_default();
                        Ok(Body::Binary(data))
                    }
                    Some("graphql") => {
                        obj.remove("__tropel_body");
                        let query = obj
                            .remove("query")
                            .and_then(|q| q.as_str().map(str::to_string))
                            .unwrap_or_default();
                        let variables = obj
                            .remove("variables")
                            .and_then(|v| serde_json::from_value(v).ok());
                        Ok(Body::GraphQL { query, variables })
                    }
                    Some("json") => {
                        obj.remove("__tropel_body");
                        let s = obj
                            .remove("value")
                            .and_then(|v| v.as_str().map(str::to_string))
                            .unwrap_or_default();
                        Ok(Body::Json(serde_json::Value::String(s)))
                    }
                    // No discriminator or non-string value → Json body.
                    // The __tropel_body key is preserved (not removed) so
                    // user payloads that happen to carry this key are not
                    // silently mutated.
                    _ => Ok(Body::Json(serde_json::Value::Object(obj))),
                }
            }
            other => Ok(Body::Json(other)),
        }
    }
}

fn de_form_fields(value: serde_json::Value) -> Vec<FormDataPart> {
    match value {
        serde_json::Value::Array(arr) => arr
            .into_iter()
            .filter_map(|v| {
                // Lenient: numeric `value` fields are stringified rather than dropping the whole form.
                // Try strict first, then fallback to manual conversion.
                if let Ok(part) = serde_json::from_value::<FormDataPart>(v.clone()) {
                    return Some(part);
                }
                // Fallback: object with name/value where value may be non-string.
                let obj = v.as_object()?;
                let name = obj.get("name")?.as_str()?.to_string();
                let value = obj.get("value").map(|x| match x {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    _ => x.to_string(),
                });
                let filename = obj
                    .get("filename")
                    .and_then(|x| x.as_str())
                    .map(str::to_string);
                let mime = obj.get("mime").and_then(|x| x.as_str()).map(str::to_string);
                let data = obj
                    .get("data")
                    .and_then(|x| serde_json::from_value(x.clone()).ok());
                Some(FormDataPart {
                    name,
                    value,
                    filename,
                    mime,
                    data,
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Deserialize urlencoded fields from EITHER the legacy JSON object form
/// (`{"a": "1"}`) or the duplicate-preserving array-of-pairs form
/// (`[["a", "1"], ...]`) (W2 #203).
fn de_urlencoded_fields(value: serde_json::Value) -> Vec<(String, String)> {
    match value {
        serde_json::Value::Array(pairs) => pairs
            .into_iter()
            .filter_map(|p| match p {
                serde_json::Value::Array(mut kv) if kv.len() == 2 => {
                    let key = kv.remove(0).as_str().unwrap_or_default().to_string();
                    let val = kv.remove(0).as_str().unwrap_or_default().to_string();
                    Some((key, val))
                }
                _ => None,
            })
            .collect(),
        serde_json::Value::Object(map) => map
            .into_iter()
            .map(|(k, v)| (k, v.as_str().unwrap_or_default().to_string()))
            .collect(),
        _ => Vec::new(),
    }
}

impl Body {
    /// Serialize a GraphQL body to its wire JSON.
    ///
    /// Returns `{"query": "..."}` plus a `"variables"` key ONLY when the
    /// variables map is present and non-empty — strict GraphQL servers reject
    /// an empty `"variables": {}` and Postman/k6 omit the key too. This is
    /// the SINGLE source of truth for the GraphQL wire body: the HTTP client's
    /// `body_to_reqwest` and `body_size` both call it, so the bytes sent and
    /// the `data_sent` accounting can never diverge (the old code dropped
    /// `variables` entirely and estimated size with `query.len() + 50`).
    pub fn graphql_json_string(
        query: &str,
        variables: &Option<HashMap<String, serde_json::Value>>,
    ) -> String {
        let mut body = serde_json::Map::new();
        body.insert(
            "query".to_string(),
            serde_json::Value::String(query.to_string()),
        );
        if let Some(vars) = variables {
            if !vars.is_empty() {
                let mut obj = serde_json::Map::new();
                for (k, v) in vars {
                    obj.insert(k.clone(), v.clone());
                }
                body.insert("variables".to_string(), serde_json::Value::Object(obj));
            }
        }
        serde_json::Value::Object(body).to_string()
    }
}

/// HTTP response with lazy, memoized body decoding.
///
/// The body is stored as raw `Vec<u8>`. `body_text()` and `body_json()`
/// decode on first access and memoize the result in a `OnceLock`, so
/// repeated script access (assert + extract + log on the same response —
/// the normal Postman/k6 pattern) decodes ONCE instead of re-cloning a
/// multi-MB body and re-parsing it on every call. The caches are
/// `#[serde(skip)]` (never serialized): a distributed/spool round-trip
/// carries only the raw `body` bytes and the cache is rebuilt on demand.
#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    /// The URL that produced THIS response. For a redirect chain each hop
    /// carries its own URL; the final response carries the final URL (what
    /// scripts see via pm.response / k6 res.url).
    #[serde(default)]
    pub url: String,
    pub status_code: u16,
    pub status_text: String,
    /// Actual protocol version (e.g. "HTTP/1.1", "HTTP/2").
    #[serde(default = "default_protocol")]
    pub protocol: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    /// Memoized UTF-8 decode of `body` (see `body_text()`).
    #[serde(skip)]
    pub text_cache: OnceLock<Option<String>>,
    /// Memoized JSON parse of `body` (see `body_json()`).
    #[serde(skip)]
    pub json_cache: OnceLock<Option<serde_json::Value>>,
    pub response_time: Duration,
    pub timings: Option<Timings>,
    pub cookies: Vec<Cookie>,
    pub size: u64,
    /// Number of bytes sent in the request body (for `data_sent` tracking).
    /// Carried on the response so the executor can emit `data_sent` without
    /// reaching into the HTTP layer (P4 decoupling: the executor talks to
    /// HTTP only through `DriverHttpClient`).
    #[serde(default)]
    pub request_body_size: u64,
    /// Intermediate redirect hops, in order, each captured as its own
    /// request (k6 parity: a 302 chain counts as hops + 1 requests, not
    /// just the final). Empty when the request did not redirect. Callers
    /// emit one http_req_* sample set PER hop plus the final response.
    #[serde(default)]
    pub redirects: Vec<Response>,
}

// Hand-written Clone: the derived Clone would copy the OnceLock memoization
// caches (text_cache, json_cache), so a clone-then-mutate-body returns the
// stale text from the original body. Skipping the caches lets them be
// rebuilt on demand by the cloned Response's own body.
impl Clone for Response {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            status_code: self.status_code,
            status_text: self.status_text.clone(),
            protocol: self.protocol.clone(),
            headers: self.headers.clone(),
            body: self.body.clone(),
            text_cache: OnceLock::new(),
            json_cache: OnceLock::new(),
            response_time: self.response_time,
            timings: self.timings.clone(),
            cookies: self.cookies.clone(),
            size: self.size,
            request_body_size: self.request_body_size,
            redirects: self.redirects.clone(),
        }
    }
}

impl Response {
    /// Decode the body as UTF-8 text (lazy — decodes once, then memoized).
    ///
    /// Postman parity (backlog line 171): an EMPTY body yields `Some("")`
    /// (Postman's `pm.response.text()` returns `''`, not `undefined`), and a
    /// non-UTF-8 body is decoded LOSSILY instead of becoming `null` — so
    /// `res.body.includes(...)` on a binary/odd-encoding response doesn't
    /// throw `undefined` method errors.
    pub fn body_text(&self) -> Option<String> {
        self.text_cache
            .get_or_init(|| {
                if self.body.is_empty() {
                    Some(String::new())
                } else {
                    Some(String::from_utf8_lossy(&self.body).into_owned())
                }
            })
            .clone()
    }

    /// Parse the body as JSON using simd-json (lazy — parses once, then
    /// memoized).
    ///
    /// Parses directly from the raw `Vec<u8>` body, skipping the intermediate
    /// `String::from_utf8` step that `body_text()` requires. Uses `simd-json`
    /// for ~2-4x faster parsing on typical payloads.
    pub fn body_json(&self) -> Option<serde_json::Value> {
        self.json_cache
            .get_or_init(|| {
                if self.body.is_empty() {
                    return None;
                }
                let mut body_bytes = self.body.clone();
                simd_json::serde::from_slice(&mut body_bytes).ok()
            })
            .clone()
    }
}

/// Request sub-timings, matching k6's http_req_* breakdown.
///
/// All durations are wall-clock, measured from the start of the HTTP request
/// (`execute()`). Metric samples derived from these are emitted in
/// MILLISECONDS — the public unit end-to-end (backlog §0). `blocked`, `dns`,
/// and `connecting` are filled from real connector instrumentation (reqwest's
/// `dns_resolver` and `connector_layer` hooks — see `tropel-http::subtimings`);
/// they are ZERO when a pooled keep-alive connection is reused (no connection
/// work).
///
/// The measurable phases:
/// - **blocked**: request start until connection attempt begins (pool wait)
/// - **dns**: real DNS resolution time
/// - **connecting**: TCP connect (plus TLS for https, folded into the
///   connector call by reqwest)
/// - **waiting** (TTFB): response headers received
/// - **receiving**: response headers received until full body received
/// - **total**: full request lifecycle (start until body fully received)
///
/// **tls_handshaking** is not measurable from reqwest alone — reqwest's
/// sealed connector folds the TLS handshake into the same connector call as
/// the TCP connect, so it is reported within `connecting` for https (and is
/// genuinely 0 for plain http / reused connections). A hyper-based custom
/// connector would be required to split it out.
///
/// **sending** is measured for real by the HTTP client's timed body wrapper,
/// so `http_req_sending` / `res.timings.sending` carry the actual wire-write
/// duration (0 for requests without a body, which k6 also reports as sub-µs
/// header-only writes).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timings {
    /// Time blocked before connection starts (pool wait / queueing).
    #[serde(default)]
    pub blocked: Duration,
    /// DNS resolution time.
    #[serde(default)]
    pub dns: Duration,
    /// TCP connect time (TLS included for https).
    #[serde(default)]
    pub connecting: Duration,
    /// TLS handshake time. ZERO for plain http / reused connections; folded
    /// into `connecting` for fresh https (reqwest's sealed connector cannot
    /// split the TLS handshake from the TCP connect).
    #[serde(default)]
    pub tls_handshaking: Duration,
    /// Time to send the request body. Real for requests with a body (measured
    /// via a timed body wrapper); 0 for bodyless requests (the header-only
    /// write is sub-µs, matching k6's near-zero sending for GET).
    #[serde(default)]
    pub sending: Duration,
    /// Time to first byte (TTFB) — from request start to response head received.
    #[serde(default)]
    pub waiting: Duration,
    /// Time to receive the full response body.
    #[serde(default)]
    pub receiving: Duration,
    /// Total request duration (start to full body received).
    pub total: Duration,
}

impl Timings {
    /// Create a new Timings from measured phases.
    /// blocked/dns/connecting/tls_handshaking/sending default to ZERO.
    pub fn from_measured(waiting: Duration, receiving: Duration, total: Duration) -> Self {
        Self {
            blocked: Duration::ZERO,
            dns: Duration::ZERO,
            connecting: Duration::ZERO,
            tls_handshaking: Duration::ZERO,
            sending: Duration::ZERO,
            waiting,
            receiving,
            total,
        }
    }
}

/// A cookie.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: Option<String>,
    pub path: Option<String>,
    pub http_only: Option<bool>,
    pub secure: Option<bool>,
    pub same_site: Option<String>,
    pub expires: Option<String>,
    /// `Max-Age` attribute in seconds. `#[serde(default)]` keeps old wire
    /// payloads (distributed workers, spool, replay) deserializing cleanly.
    #[serde(default)]
    pub max_age: Option<i64>,
}

/// Auth configuration.
///
/// P1 line 150: manual Debug impl that redacts secret fields so credentials
/// never leak into logs, summary output, or debug traces.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AuthConfig {
    /// Explicitly NO auth — Postman's `{"type":"noauth"}`. Distinct from
    /// `None` (which means "no auth configured → inherit from the parent
    /// scope"): a request marked noauth must NOT inherit collection/folder
    /// auth. The runner treats `Some(NoAuth)` as "no signer, and don't fall
    /// back to scenario auth" — the inverse of Postman semantics that the
    /// old `None` mapping produced.
    #[serde(rename = "noauth")]
    NoAuth,
    #[serde(rename = "bearer")]
    Bearer { token: String },
    #[serde(rename = "basic")]
    Basic { username: String, password: String },
    #[serde(rename = "apikey")]
    ApiKey {
        key: String,
        value: String,
        location: ApiKeyLocation,
    },
    #[serde(rename = "digest")]
    Digest { username: String, password: String },
    #[serde(rename = "oauth1")]
    OAuth1 {
        consumer_key: String,
        consumer_secret: String,
        token: Option<String>,
        token_secret: Option<String>,
        /// Signature method: HMAC-SHA1, HMAC-SHA256, PLAINTEXT, RSA-SHA1 etc.
        /// `None` defaults to HMAC-SHA1 for backwards compat (pre-TR-409).
        #[serde(default)]
        signature_method: Option<String>,
    },
    #[serde(rename = "oauth2")]
    OAuth2 {
        access_token: String,
        token_type: Option<String>,
    },
    #[serde(rename = "aws-sigv4")]
    AwsSigV4 {
        access_key: String,
        secret_key: String,
        region: Option<String>,
        service: Option<String>,
        session_token: Option<String>,
    },
    #[serde(rename = "hawk")]
    Hawk {
        auth_id: String,
        auth_key: String,
        algorithm: Option<String>,
    },
    /// NTLM (TR-409: reported as unsupported until implemented — never silently
    /// degraded to `NoAuth`).
    #[serde(rename = "ntlm")]
    Ntlm {
        #[serde(default)]
        username: Option<String>,
        #[serde(default)]
        password: Option<String>,
        #[serde(flatten)]
        extra: std::collections::HashMap<String, serde_json::Value>,
    },
    /// WSSE UsernameToken (TR-409: distinct from `noauth`; requires WSSE signing
    /// which lives in `tropel-auth::oauth::sign_wsse` — until wired through the
    /// signer builder it is reported as unsupported).
    #[serde(rename = "wsse")]
    Wsse {
        #[serde(default)]
        username: Option<String>,
        #[serde(default)]
        password: Option<String>,
        #[serde(flatten)]
        extra: std::collections::HashMap<String, serde_json::Value>,
    },
    /// JWT bearer (TR-409: `Authorization: Bearer <jwt>` where the token is a
    /// signed JWT — until the picker's JWT flow is wired to `sign_jwt` it is
    /// reported as unsupported rather than degraded to bearer/none).
    #[serde(rename = "jwt")]
    Jwt {
        #[serde(default)]
        token: Option<String>,
        #[serde(flatten)]
        extra: std::collections::HashMap<String, serde_json::Value>,
    },
    /// Akamai EdgeGrid (TR-409: `Authorization: EG1-HMAC-SHA256 ...` — not yet
    /// implemented; reported as unsupported).
    #[serde(rename = "akamai-edgegrid")]
    AkamaiEdgeGrid {
        #[serde(default)]
        access_token: Option<String>,
        #[serde(default)]
        client_token: Option<String>,
        #[serde(flatten)]
        extra: std::collections::HashMap<String, serde_json::Value>,
    },
}

impl std::fmt::Debug for AuthConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthConfig::NoAuth => write!(f, "NoAuth"),
            AuthConfig::Bearer { .. } => write!(f, "Bearer {{ token: [redacted] }}"),
            AuthConfig::Basic { username, .. } => {
                write!(
                    f,
                    "Basic {{ username: {:?}, password: [redacted] }}",
                    username
                )
            }
            AuthConfig::ApiKey { key, location, .. } => {
                write!(
                    f,
                    "ApiKey {{ key: {:?}, value: [redacted], location: {:?} }}",
                    key, location
                )
            }
            AuthConfig::Digest { username, .. } => {
                write!(
                    f,
                    "Digest {{ username: {:?}, password: [redacted] }}",
                    username
                )
            }
            AuthConfig::OAuth1 { consumer_key, .. } => {
                write!(
                    f,
                    "OAuth1 {{ consumer_key: {:?}, consumer_secret: [redacted] }}",
                    consumer_key
                )
            }
            AuthConfig::OAuth2 { token_type, .. } => {
                write!(
                    f,
                    "OAuth2 {{ token_type: {:?}, access_token: [redacted] }}",
                    token_type
                )
            }
            AuthConfig::AwsSigV4 {
                access_key,
                region,
                service,
                ..
            } => {
                write!(f, "AwsSigV4 {{ access_key: {:?}, secret_key: [redacted], region: {:?}, service: {:?} }}", access_key, region, service)
            }
            AuthConfig::Hawk {
                auth_id, algorithm, ..
            } => {
                write!(
                    f,
                    "Hawk {{ auth_id: {:?}, auth_key: [redacted], algorithm: {:?} }}",
                    auth_id, algorithm
                )
            }
            AuthConfig::Ntlm { username, .. } => {
                write!(
                    f,
                    "Ntlm {{ username: {:?}, password: [redacted] }}",
                    username
                )
            }
            AuthConfig::Wsse { username, .. } => {
                write!(
                    f,
                    "Wsse {{ username: {:?}, password: [redacted] }}",
                    username
                )
            }
            AuthConfig::Jwt { .. } => write!(f, "Jwt {{ token: [redacted] }}"),
            AuthConfig::AkamaiEdgeGrid {
                access_token,
                client_token,
                ..
            } => {
                write!(
                    f,
                    "AkamaiEdgeGrid {{ access_token: {:?}, client_token: {:?}, client_secret: [redacted] }}",
                    access_token, client_token
                )
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ApiKeyLocation {
    #[serde(rename = "header")]
    Header,
    #[serde(rename = "query")]
    Query,
}

/// Certificate configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificateConfig {
    pub cert: Option<String>,
    pub key: Option<String>,
    pub passphrase: Option<String>,
}

/// A set of metric tags backed by a fast hash map with `Arc<str>` key/value
/// interning to reduce per-iteration allocation churn.
///
/// Tag keys are almost always string literals ("url", "method", "status_code",
/// etc.) and tag values are often reused across iterations (status codes,
/// group names, etc.). Using `Arc<str>` avoids cloning the underlying string
/// data when the tag map is cloned (e.g., when building both a duration sample
/// and a counter sample from the same tags).
///
/// Internally backed by `FxHashMap` (faster than std's SipHash for small
/// string keys) with `Arc<str>` values.
#[derive(Debug, Clone)]
pub struct TagMap {
    pub(crate) inner: FxHashMap<Arc<str>, Arc<str>>,
}

impl TagMap {
    /// Create an empty tag map.
    pub fn new() -> Self {
        Self {
            inner: FxHashMap::default(),
        }
    }

    /// Create a tag map with the given pre-allocated capacity.
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            inner: FxHashMap::with_capacity_and_hasher(cap, Default::default()),
        }
    }

    /// Create a tag map from an iterator of (key, value) pairs.
    /// Both keys and values are interned as `Arc<str>`.
    pub fn from_pairs(
        pairs: impl IntoIterator<Item = (impl Into<Arc<str>>, impl Into<Arc<str>>)>,
    ) -> Self {
        let mut map = FxHashMap::default();
        for (k, v) in pairs {
            map.insert(k.into(), v.into());
        }
        Self { inner: map }
    }

    /// Insert a tag pair. Both key and value are interned as `Arc<str>`.
    pub fn insert(&mut self, key: impl Into<Arc<str>>, value: impl Into<Arc<str>>) {
        self.inner.insert(key.into(), value.into());
    }

    /// Get a tag value by key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.inner.get(key).map(|s| s.as_ref())
    }

    /// Returns true if the map contains no elements.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Number of tag pairs.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Iterate over (key, value) pairs as &str references.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> + '_ {
        self.inner.iter().map(|(k, v)| (k.as_ref(), v.as_ref()))
    }

    /// Collect into a sorted Vec of (Arc<str>, Arc<str>) for MetricKey construction.
    /// The Arc references are cloned (ref-count bump only, no string copy).
    pub fn to_sorted_arc_vec(&self) -> Vec<(Arc<str>, Arc<str>)> {
        let mut pairs: Vec<(Arc<str>, Arc<str>)> = self
            .inner
            .iter()
            .map(|(k, v)| (Arc::clone(k), Arc::clone(v)))
            .collect();
        pairs.sort();
        pairs
    }
}

impl Default for TagMap {
    fn default() -> Self {
        Self::new()
    }
}

// ── Static interned tag keys (backlog line 352) ──
// Compile-time constant tag keys ("url", "method", etc.) are inserted
// on every HTTP request hop. Using static Arc<str> avoids re-allocating
// them from &str on each insert. The corresponding VALUES are still
// dynamic (per-response status, url, etc.).
pub mod tag_keys {
    use std::sync::{Arc, LazyLock};

    macro_rules! intern_key {
        ($name:ident, $val:expr) => {
            pub static $name: LazyLock<Arc<str>> = LazyLock::new(|| Arc::from($val));
        };
    }

    intern_key!(URL, "url");
    intern_key!(METHOD, "method");
    intern_key!(STATUS, "status");
    intern_key!(NAME, "name");
    intern_key!(GROUP, "group");
    intern_key!(PROTO, "proto");
    intern_key!(SERVICE, "service");
    intern_key!(METHOD_GRPC, "method");
}

impl Serialize for TagMap {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.inner.len()))?;
        for (k, v) in self.inner.iter() {
            map.serialize_entry(k.as_ref(), v.as_ref())?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for TagMap {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw: HashMap<String, String> = HashMap::deserialize(deserializer)?;
        let mut map = FxHashMap::with_capacity_and_hasher(raw.len(), Default::default());
        for (k, v) in raw {
            map.insert(Arc::from(k), Arc::from(v));
        }
        Ok(Self { inner: map })
    }
}

/// A single metric sample emitted during execution.
///
/// `metric` is a `Cow<'static, str>` so the ~12 static names emitted per
/// request ("http_req_duration", "http_reqs", …) are zero-alloc
/// `Cow::Borrowed` — no per-sample `String` churn on the hot path. `tags` is
/// an `Arc<TagMap>` so the repeated `tags.clone()` calls per request (one per
/// sample) become Arc ref-count bumps instead of full FxHashMap copies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sample {
    /// Metric name (e.g. "http_req_duration", "checks").
    pub metric: Cow<'static, str>,
    /// Metric value.
    pub value: f64,
    /// Tags (e.g. url, method, status_code, name).
    pub tags: Arc<TagMap>,
    /// Timestamp.
    pub timestamp: SystemTime,
    /// Sample type.
    #[serde(rename = "type")]
    pub sample_type: SampleType,
}

/// Type of metric sample.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum SampleType {
    #[default]
    Point,
    Counter,
    Trend,
    Rate,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_parse_accepts_custom_tokens_and_trims() {
        // Backlog line 95: unknown methods silently became GET. A valid HTTP
        // token (PURGE, LINK, …) must round-trip as Method::Custom — the
        // write path is preserved, not degraded to GET. Leading/trailing
        // whitespace is trimmed.
        assert_eq!(Method::parse("PURGE"), Some(Method::Custom("PURGE".into())));
        assert_eq!(Method::parse("purge"), Some(Method::Custom("purge".into())));
        assert_eq!(Method::parse("LINK"), Some(Method::Custom("LINK".into())));
        assert_eq!(Method::parse(" get "), Some(Method::GET));
        assert_eq!(Method::parse(" POST"), Some(Method::POST));
    }

    #[test]
    fn method_parse_rejects_invalid_tokens() {
        // Empty, whitespace-inside, and non-tchar chars are genuinely invalid
        // — these must be None (callers fail loudly), never silent GET.
        // NOTE 1: `!` IS a valid tchar (RFC 7230), so "POTS!" parses as
        // Custom and must NOT be here.
        // NOTE 2: trailing/leading whitespace is TRIMMED by design, so a
        // bare "GET\n" (CRLF artifact) parses as GET — the genuinely invalid
        // case is whitespace INSIDE the token ("GE\nT").
        for bad in [
            "", " ", "  ", "GE T", "GE\nT", "GET,", "{GET}", "POTS(", "\0GET",
        ] {
            assert!(
                Method::parse(bad).is_none(),
                "method {:?} must not parse",
                bad
            );
        }
        // Trailing newline is trimmed like any other outer whitespace.
        assert_eq!(Method::parse("GET\n"), Some(Method::GET));
        // Sanity: a punctuation-only token is still valid (Custom).
        assert_eq!(Method::parse("!*+"), Some(Method::Custom("!*+".into())));
    }

    #[test]
    fn method_custom_serde_roundtrip() {
        // Custom serde keeps the wire format a plain string, so a Custom
        // method survives a JSON round-trip without becoming an object.
        let m = Method::Custom("PURGE".into());
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(json, "\"PURGE\"");
        let back: Method = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);

        // Standard variants still serialize as plain uppercase strings.
        assert_eq!(serde_json::to_string(&Method::GET).unwrap(), "\"GET\"");
        let get: Method = serde_json::from_str("\"GET\"").unwrap();
        assert_eq!(get, Method::GET);

        // Deserializing a genuinely invalid token errors loudly.
        assert!(serde_json::from_str::<Method>("\"GE T\"").is_err());
    }

    #[test]
    fn body_text_lossy_and_empty_string() {
        // Regression (backlog line 171): non-UTF-8 bodies must decode LOSSILY
        // (never `null` — `res.body.includes(...)` would throw), and empty
        // bodies must yield `Some("")` (Postman's `pm.response.text()`
        // returns `''`, not `undefined`).
        //
        // NOTE: each Response gets FRESH caches (never `..base.clone()` — the
        // initialized OnceLock would memoize the first result onto the next).
        fn resp_with(body: Vec<u8>) -> Response {
            Response {
                url: String::new(),
                status_code: 200,
                status_text: "OK".into(),
                protocol: Default::default(),
                headers: Default::default(),
                body,
                text_cache: Default::default(),
                json_cache: Default::default(),
                response_time: Default::default(),
                timings: None,
                cookies: Vec::new(),
                size: 0,
                request_body_size: 0,
                redirects: Vec::new(),
            }
        }
        assert_eq!(resp_with(Vec::new()).body_text(), Some(String::new()));

        // 0xC3 is an invalid UTF-8 lead byte — must become U+FFFD, not None.
        let text = resp_with(vec![0xC3, 0x28, 0x41])
            .body_text()
            .expect("lossy decode must never be None");
        assert_eq!(text, "\u{FFFD}(A");

        // Valid UTF-8 passes through unchanged.
        assert_eq!(
            resp_with(b"ok".to_vec()).body_text(),
            Some("ok".to_string())
        );
    }

    #[test]
    fn body_roundtrip_preserves_all_variants() {
        // Regression (backlog line 92): #[serde(untagged)] made `Json(Value)`
        // match ANY JSON, so FormData/UrlEncoded/Binary/GraphQL were
        // unreachable on deserialize — every round-trip silently converted
        // them to Json (Content-Type flips, wire bytes change).
        let form = vec![
            FormDataPart {
                name: "a".into(),
                value: Some("1".into()),
                filename: None,
                mime: None,
                data: None,
            },
            FormDataPart {
                name: "file".into(),
                value: None,
                filename: Some("photo.png".into()),
                mime: Some("image/png".into()),
                data: Some(vec![0x89, 0x50, 0x4e, 0x47]),
            },
        ];
        let url = vec![("q".to_string(), "hello world".to_string())];
        let mut vars = HashMap::new();
        vars.insert("id".to_string(), serde_json::json!(42));

        let cases = vec![
            Body::Raw("raw body".into()),
            Body::Json(serde_json::json!({"a": 1, "b": [true, null]})),
            Body::FormData(form),
            Body::UrlEncoded(url),
            Body::Binary(vec![0u8, 1, 2, 255]),
            Body::GraphQL {
                query: "{ user { id } }".into(),
                variables: Some(vars),
            },
        ];

        for body in cases {
            let json = serde_json::to_string(&body).expect("serialize");
            let back: Body = serde_json::from_str(&json).expect("deserialize");
            match (&body, &back) {
                (Body::Raw(a), Body::Raw(b)) => assert_eq!(a, b),
                (Body::Json(a), Body::Json(b)) => assert_eq!(a, b),
                (Body::FormData(a), Body::FormData(b)) => assert_eq!(a, b),
                (Body::UrlEncoded(a), Body::UrlEncoded(b)) => {
                    assert_eq!(a, b, "UrlEncoded must not collapse into Json")
                }
                (Body::Binary(a), Body::Binary(b)) => assert_eq!(a, b),
                (
                    Body::GraphQL {
                        query: aq,
                        variables: av,
                    },
                    Body::GraphQL {
                        query: bq,
                        variables: bv,
                    },
                ) => {
                    assert_eq!(aq, bq);
                    assert_eq!(av, bv);
                }
                (a, b) => panic!("variant changed on round-trip: {a:?} -> {b:?}"),
            }
        }
    }

    #[test]
    fn body_untagged_wire_forms_still_parse() {
        // Backward compat: the OLD untagged serialization of Raw (a JSON
        // string) and Json (a bare JSON value) must still deserialize.
        let raw: Body = serde_json::from_str("\"hello\"").unwrap();
        assert!(matches!(raw, Body::Raw(_)));
        let json: Body = serde_json::from_str(r#"{"x":1}"#).unwrap();
        assert!(matches!(json, Body::Json(_)));
        let arr: Body = serde_json::from_str("[1,2,3]").unwrap();
        assert!(matches!(arr, Body::Json(_)));
    }
}
