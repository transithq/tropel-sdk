use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Contract config types ─────────────────────────────────────────
// Referenced by the SDK's own extension contract (DriverDeclaredOptions,
// Output::configure) — they must live in the leaf. tropel-core re-exports
// them so engine crates keep resolving tropel_core::config::*.

///
/// Controls how long a VU waits before starting the next iteration.
/// - `delay`: fixed delay after each iteration (e.g. "2s")
/// - `min_delay` / `max_delay`: random delay in range [min, max]
/// - `iteration_pacing`: target iteration duration. If the iteration
///   finishes faster than this, the VU waits to hit the target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct ThinkTimeConfig {
    /// Fixed delay after each iteration (e.g., "2s", "500ms").
    /// If set, min/max_delay are ignored.
    pub delay: Option<String>,
    /// Minimum delay for random range (e.g., "1s").
    #[serde(alias = "minDelay")]
    pub min_delay: Option<String>,
    /// Maximum delay for random range (e.g., "3s").
    #[serde(alias = "maxDelay")]
    pub max_delay: Option<String>,
    /// Target iteration duration for pacing (e.g., "5s").
    /// If the iteration finishes faster than this, the VU waits
    /// to hit the target duration before starting the next iteration.
    #[serde(alias = "iterationPacing")]
    pub iteration_pacing: Option<String>,
}

/// Configuration for a single named scenario within a multi-scenario run.
/// Each scenario has its own executor, input, env, tags, and optional start time.
/// When only a single scenario is running, the top-level `execution` field is used
/// instead and no `ScenarioConfig` is needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioConfig {
    /// Which executor to use.
    pub execution: ExecutionConfig,
    /// Optional input file override (defaults to the job-level `input`).
    pub input: Option<String>,
    /// Per-scenario environment variables (merged with job-level env).
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Per-scenario tags applied to all metrics emitted by this scenario.
    #[serde(default)]
    pub tags: HashMap<String, String>,
    /// When to start this scenario (e.g. "5s", "30s").
    /// Defaults to "0s" — starts immediately alongside other scenarios.
    /// Use staggered values to sequence scenario start times.
    #[serde(default = "default_start_time", alias = "startTime")]
    pub start_time: String,
    /// k6 `exec` selection — which exported function/flow this scenario runs.
    /// Drivers that support named entry points (e.g. the k6 driver) install
    /// this export as the iteration function; when absent, the script's
    /// `default` export runs. Ignored by declarative (adapter) scenarios.
    #[serde(default)]
    pub exec: Option<String>,
}

/// How to execute the load test.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ExecutionConfig {
    #[serde(rename = "constant-vus")]
    ConstantVus {
        vus: u32,
        duration: String,
        /// How long to wait for in-flight iterations to finish after the
        /// test duration expires. Defaults to 30s if not set.
        #[serde(default, alias = "gracefulStop")]
        graceful_stop: Option<String>,
        /// Think time / pacing configuration between iterations.
        #[serde(default, alias = "thinkTime")]
        think_time: ThinkTimeConfig,
    },
    #[serde(rename = "ramping-vus")]
    RampingVus {
        stages: Vec<Stage>,
        #[serde(alias = "startVUs")]
        start_vus: u32,
        /// How long to wait for a VU to finish its current iteration during
        /// a ramp-down stage before moving on. Defaults to 30s.
        #[serde(default, alias = "gracefulRampDown")]
        graceful_ramp_down: Option<String>,
        /// How long to wait for in-flight iterations to finish after the
        /// final stage completes. Defaults to 30s.
        #[serde(default, alias = "gracefulStop")]
        graceful_stop: Option<String>,
        /// Think time / pacing configuration between iterations.
        #[serde(default, alias = "thinkTime")]
        think_time: ThinkTimeConfig,
    },
    #[serde(rename = "constant-arrival-rate")]
    ConstantArrivalRate {
        rate: f64,
        /// Time unit for the rate (e.g. "1s", "1m"). The `rate` is
        /// expressed PER this unit, not per second — the scheduler converts
        /// to per-second at dispatch. Defaults to "1s" (k6 parity).
        #[serde(default = "default_time_unit", alias = "timeUnit")]
        time_unit: String,
        duration: String,
        #[serde(alias = "preAllocatedVUs")]
        pre_alloc_vus: u32,
        #[serde(alias = "maxVUs")]
        max_vus: u32,
        /// How long to wait for in-flight iterations to finish after the
        /// test duration expires. Defaults to 30s if not set.
        #[serde(default, alias = "gracefulStop")]
        graceful_stop: Option<String>,
        /// Think time / pacing configuration between iterations.
        #[serde(default, alias = "thinkTime")]
        think_time: ThinkTimeConfig,
    },
    #[serde(rename = "shared-iterations")]
    SharedIterations {
        iterations: u64,
        #[serde(alias = "maxDuration")]
        max_duration: Option<String>,
        vus: u32,
        /// How long to wait for in-flight iterations to finish after the
        /// iteration budget is exhausted or max_duration is reached.
        #[serde(default, alias = "gracefulStop")]
        graceful_stop: Option<String>,
        /// Think time / pacing configuration between iterations.
        #[serde(default, alias = "thinkTime")]
        think_time: ThinkTimeConfig,
    },
    /// Ramping arrival rate — stages of target rate (iterations/sec).
    /// Similar to k6's `ramping-arrival-rate` executor.
    #[serde(rename = "ramping-arrival-rate")]
    RampingArrivalRate {
        /// Starting rate (iterations/sec).
        #[serde(default, alias = "startRate")]
        start_rate: f64,
        /// Stages defining how the rate changes over time.
        stages: Vec<ArrivalRateStage>,
        /// Time unit for the rate (e.g. "1s", "1m").
        #[serde(default = "default_time_unit", alias = "timeUnit")]
        time_unit: String,
        /// Pre-allocated VUs.
        #[serde(default = "default_pre_alloc", alias = "preAllocatedVUs")]
        pre_alloc_vus: u32,
        /// Maximum VUs.
        #[serde(default = "default_max_vus", alias = "maxVUs")]
        max_vus: u32,
        /// How long to wait for in-flight iterations to finish after the
        /// test duration expires. Defaults to 30s if not set.
        #[serde(default, alias = "gracefulStop")]
        graceful_stop: Option<String>,
        /// Think time / pacing configuration between iterations.
        #[serde(default, alias = "thinkTime")]
        think_time: ThinkTimeConfig,
    },
    /// Each VU runs exactly N iterations independently.
    /// Similar to k6's `per-vu-iterations` executor.
    #[serde(rename = "per-vu-iterations")]
    PerVUIterations {
        /// Number of VUs to spawn.
        vus: u32,
        /// Number of iterations per VU (each VU runs exactly this many).
        iterations: u64,
        /// Optional overall time limit for the test.
        #[serde(default, alias = "maxDuration")]
        max_duration: Option<String>,
        /// How long to wait for in-flight iterations to finish after the
        /// iteration budget is exhausted or max_duration is reached.
        #[serde(default, alias = "gracefulStop")]
        graceful_stop: Option<String>,
        /// Think time / pacing configuration between iterations.
        #[serde(default, alias = "thinkTime")]
        think_time: ThinkTimeConfig,
    },
    /// Externally-controlled VUs — the VU count can be adjusted AT RUNTIME
    /// via the control API (k6's `externally-controlled` executor / REST
    /// `/v1/status` parity). Starts with `vus`, may grow up to `max_vus` and
    /// shrink below `vus` as the controller commands. When `duration` is
    /// unset the run continues until the controller (or signal) stops it.
    #[serde(rename = "externally-controlled")]
    ExternallyControlled {
        /// Initial VU count.
        vus: u32,
        /// Maximum VU count the control API may scale up to.
        #[serde(alias = "maxVUs")]
        max_vus: u32,
        /// Optional wall-clock limit. When unset, the run continues until
        /// the control API requests a stop (or a signal / threshold aborts).
        #[serde(default, alias = "duration")]
        duration: Option<String>,
        /// How long to wait for in-flight iterations to finish after a
        /// stop / shrink command. Defaults to 30s.
        #[serde(default, alias = "gracefulStop")]
        graceful_stop: Option<String>,
        /// Think time / pacing configuration between iterations.
        #[serde(default, alias = "thinkTime")]
        think_time: ThinkTimeConfig,
    },
}

impl ExecutionConfig {
    /// k6-style executor type name (matches the serde tag used for this
    /// variant). Exposed to scripts via `exec.scenario.executor()`.
    pub fn executor_name(&self) -> &'static str {
        match self {
            ExecutionConfig::ConstantVus { .. } => "constant-vus",
            ExecutionConfig::RampingVus { .. } => "ramping-vus",
            ExecutionConfig::ConstantArrivalRate { .. } => "constant-arrival-rate",
            ExecutionConfig::SharedIterations { .. } => "shared-iterations",
            ExecutionConfig::RampingArrivalRate { .. } => "ramping-arrival-rate",
            ExecutionConfig::PerVUIterations { .. } => "per-vu-iterations",
            ExecutionConfig::ExternallyControlled { .. } => "externally-controlled",
        }
    }

    /// Estimated wall-clock duration of this executor's PLANNED phase — the
    /// target the live progress bar fills toward, reaching 100% when the
    /// planned phase ends (graceful-stop drain then holds the bar full, k6
    /// style). Grace is deliberately NOT included: an 8s run should show
    /// 8s as its target, not 8s + 30s grace. `None` when the run has no
    /// fixed duration: externally-controlled without a `duration`, or
    /// shared-/per-vu-iterations without a `max_duration`.
    pub fn total_duration(&self) -> Option<std::time::Duration> {
        use std::time::Duration;
        let parse = |s: &str| crate::parse_duration(s).ok();
        match self {
            ExecutionConfig::ConstantVus { duration, .. } => parse(duration),
            ExecutionConfig::RampingVus { stages, .. } => {
                let mut total = Duration::ZERO;
                for st in stages {
                    total += parse(&st.duration)?;
                }
                Some(total)
            }
            ExecutionConfig::ConstantArrivalRate { duration, .. } => parse(duration),
            ExecutionConfig::SharedIterations { max_duration, .. } => {
                max_duration.as_deref().and_then(parse)
            }
            ExecutionConfig::RampingArrivalRate { stages, .. } => {
                let mut total = Duration::ZERO;
                for st in stages {
                    total += parse(&st.duration)?;
                }
                Some(total)
            }
            ExecutionConfig::PerVUIterations { max_duration, .. } => {
                max_duration.as_deref().and_then(parse)
            }
            ExecutionConfig::ExternallyControlled { duration, .. } => {
                duration.as_deref().and_then(parse)
            }
        }
    }

    /// Build an `ExecutionConfig` from a k6-style executor `mode` plus the
    /// load-profile knobs (`vus` / `duration` / `iterations` / `stages`).
    ///
    /// Canonical mode→executor mapping, shared by the CLI (`cli.rs`) and the
    /// k6 env-file builder (`config_file.rs`) so the precedence rules live in
    /// exactly one place. `stages` is the raw JSON array string (if any);
    /// `duration` is the human duration string (if any).
    pub fn from_mode(
        mode: &str,
        vus: Option<u32>,
        duration: Option<String>,
        iterations: Option<u64>,
        stages: Option<String>,
    ) -> Self {
        let think_time = ThinkTimeConfig::default();
        match mode {
            "ramping-vus" => {
                let start_vus = vus.unwrap_or(1);
                let stages_list = stages
                    .as_deref()
                    .and_then(|s| serde_json::from_str::<Vec<Stage>>(s).ok())
                    .unwrap_or_else(|| {
                        vec![Stage {
                            duration: duration.clone().unwrap_or_else(|| "30s".to_string()),
                            target: vus.unwrap_or(10),
                        }]
                    });
                ExecutionConfig::RampingVus {
                    stages: stages_list,
                    start_vus,
                    graceful_ramp_down: Some("30s".to_string()),
                    graceful_stop: Some("30s".to_string()),
                    think_time,
                }
            }
            "shared-iterations" => ExecutionConfig::SharedIterations {
                iterations: iterations.unwrap_or(100),
                max_duration: duration,
                vus: vus.unwrap_or(1),
                graceful_stop: Some("30s".to_string()),
                think_time,
            },
            "arrival-rate" | "constant-arrival-rate" => ExecutionConfig::ConstantArrivalRate {
                rate: vus.unwrap_or(1) as f64,
                time_unit: "1s".to_string(),
                duration: duration.unwrap_or_else(|| "30s".to_string()),
                pre_alloc_vus: 1,
                max_vus: vus.unwrap_or(10).max(10),
                graceful_stop: Some("30s".to_string()),
                think_time,
            },
            _ => ExecutionConfig::ConstantVus {
                vus: vus.unwrap_or(1),
                duration: duration.unwrap_or_else(|| "30s".to_string()),
                graceful_stop: Some("30s".to_string()),
                think_time,
            },
        }
    }
}

/// A ramping stage (for VU count — used by RampingVus).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stage {
    pub duration: String,
    pub target: u32,
}

/// A ramping arrival rate stage.
/// The rate linearly interpolates from the previous stage's target (or start_rate)
/// to this stage's target over the stage duration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArrivalRateStage {
    pub duration: String,
    pub target: f64,
}

fn default_time_unit() -> String {
    "1s".to_string()
}

fn default_start_time() -> String {
    "0s".to_string()
}

fn default_pre_alloc() -> u32 {
    1
}

fn default_max_vus() -> u32 {
    10
}

/// Threshold configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdConfig {
    /// Threshold expression.
    pub expression: String,
    /// Whether to abort the test on failure.
    #[serde(default, alias = "abortOnFail")]
    pub abort_on_fail: bool,
    /// Grace period before abortOnFail activates (e.g. "30s").
    /// During this time metrics are collected but failures won't abort.
    #[serde(default, alias = "delayAbortEval")]
    pub delay_abort_eval: Option<String>,
}

/// Output/reporter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    /// Reporters to use (e.g. ["stdout", "json"]).
    pub reporters: Vec<String>,
    /// Output file path (for json/csv reporters).
    pub output_file: Option<String>,
    /// Whether to show detailed summary.
    pub summary: bool,
    /// Whether to show trend statistics.
    pub trends: bool,
    /// Prometheus remote-write endpoint (e.g. `http://localhost:9090`).
    /// When set, samples are streamed to Prometheus via the remote-write API.
    #[serde(default)]
    pub prometheus_remote_write_url: Option<String>,
    /// OTLP/HTTP collector endpoint (e.g. `http://localhost:4318`).
    /// When set, samples are exported to the collector as OTLP metrics.
    #[serde(default)]
    pub otlp_endpoint: Option<String>,
    /// Path for the `--summary-export` JSON export (k6 semantics).
    ///
    /// When the script declares a `handleSummary(data)` function (k6), the
    /// script's returned file map governs output and this is ignored unless
    /// the script also prints to `stdout`; otherwise the aggregated summary
    /// data object is written here as JSON.
    #[serde(default)]
    pub summary_export: Option<String>,
    /// NDJSON streaming output file: every sample is appended as one JSON
    /// line while the run is in progress (k6 `--out json=file` equivalent).
    #[serde(default)]
    pub json_stream: Option<String>,
    /// StatsD / Datadog agent address (`host:port`, e.g. `localhost:8125`)
    /// for streaming datagram output with Datadog-style tags.
    #[serde(default)]
    pub statsd_addr: Option<String>,
    /// InfluxDB line-protocol UDP address (`host:port`, e.g. `localhost:8089`)
    /// for streaming line-protocol datagrams.
    #[serde(default)]
    pub influxdb_addr: Option<String>,
    /// Tag-key allowlist for network outputs (prometheus/otlp/statsd/
    /// influxdb). Only these tag keys are forwarded; empty (default) forwards
    /// all tags. Bounds label cardinality at the backend.
    #[serde(default)]
    pub tag_allowlist: Vec<String>,
    /// Max tag keys per sample forwarded to network outputs. When a sample
    /// carries more, tags are kept deterministically (sorted by key, first
    /// `cap` kept). `None` (default) = no cap.
    #[serde(default)]
    pub max_tags_per_sample: Option<usize>,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            reporters: vec!["stdout".to_string()],
            output_file: None,
            summary: true,
            trends: true,
            prometheus_remote_write_url: None,
            otlp_endpoint: None,
            summary_export: None,
            json_stream: None,
            statsd_addr: None,
            influxdb_addr: None,
            tag_allowlist: Vec::new(),
            max_tags_per_sample: None,
        }
    }
}

impl OutputConfig {
    /// Build the output config dispatched to distributed worker agents.
    ///
    /// The controller owns ALL output — agents must not stream to the same
    /// endpoints/files the controller or other agents use (a shared NDJSON
    /// file written by N processes, or N parallel remote-write pushes).
    /// This constructor nulls every streaming/reporting field in one place,
    /// so adding a new output field can't silently leak into worker configs.
    pub fn into_worker(self) -> Self {
        Self {
            reporters: Vec::new(),
            output_file: None,
            prometheus_remote_write_url: None,
            otlp_endpoint: None,
            summary_export: None,
            json_stream: None,
            statsd_addr: None,
            influxdb_addr: None,
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executor_names_match_serde_tags() {
        use ExecutionConfig::*;
        assert_eq!(
            ConstantVus {
                vus: 1,
                duration: "1s".into(),
                graceful_stop: None,
                think_time: Default::default()
            }
            .executor_name(),
            "constant-vus"
        );
        assert_eq!(
            RampingVus {
                stages: vec![],
                start_vus: 1,
                graceful_ramp_down: None,
                graceful_stop: None,
                think_time: Default::default()
            }
            .executor_name(),
            "ramping-vus"
        );
        assert_eq!(
            ConstantArrivalRate {
                rate: 1.0,
                time_unit: "1s".into(),
                duration: "1s".into(),
                pre_alloc_vus: 1,
                max_vus: 10,
                graceful_stop: None,
                think_time: Default::default()
            }
            .executor_name(),
            "constant-arrival-rate"
        );
        assert_eq!(
            SharedIterations {
                iterations: 10,
                max_duration: None,
                vus: 1,
                graceful_stop: None,
                think_time: Default::default()
            }
            .executor_name(),
            "shared-iterations"
        );
        assert_eq!(
            RampingArrivalRate {
                start_rate: 1.0,
                stages: vec![],
                time_unit: "1s".into(),
                pre_alloc_vus: 1,
                max_vus: 10,
                graceful_stop: None,
                think_time: Default::default()
            }
            .executor_name(),
            "ramping-arrival-rate"
        );
        assert_eq!(
            PerVUIterations {
                vus: 1,
                iterations: 10,
                max_duration: None,
                graceful_stop: None,
                think_time: Default::default()
            }
            .executor_name(),
            "per-vu-iterations"
        );
        assert_eq!(
            ExternallyControlled {
                vus: 1,
                max_vus: 10,
                duration: None,
                graceful_stop: None,
                think_time: Default::default()
            }
            .executor_name(),
            "externally-controlled"
        );
    }

    #[test]
    fn total_duration_sums_ramping_stages_and_handles_grace() {
        use std::time::Duration;
        // Grace is deliberately NOT included (progress bar target).
        let cv = ExecutionConfig::ConstantVus {
            vus: 1,
            duration: "8s".into(),
            graceful_stop: Some("30s".into()),
            think_time: Default::default(),
        };
        assert_eq!(cv.total_duration(), Some(Duration::from_secs(8)));

        // Ramping-vus sums stage durations.
        let rv = ExecutionConfig::RampingVus {
            stages: vec![
                Stage {
                    duration: "5s".into(),
                    target: 1,
                },
                Stage {
                    duration: "3s".into(),
                    target: 5,
                },
            ],
            start_vus: 1,
            graceful_ramp_down: Some("30s".into()),
            graceful_stop: Some("30s".into()),
            think_time: Default::default(),
        };
        assert_eq!(rv.total_duration(), Some(Duration::from_secs(8)));

        // Iteration-budget executors without max_duration → None.
        let si = ExecutionConfig::SharedIterations {
            iterations: 100,
            max_duration: None,
            vus: 1,
            graceful_stop: Some("30s".into()),
            think_time: Default::default(),
        };
        assert_eq!(si.total_duration(), None);

        // Externally-controlled without duration → None.
        let ec = ExecutionConfig::ExternallyControlled {
            vus: 1,
            max_vus: 10,
            duration: None,
            graceful_stop: None,
            think_time: Default::default(),
        };
        assert_eq!(ec.total_duration(), None);
    }

    #[test]
    fn from_mode_maps_k6_modes_with_defaults() {
        // constant-vus (unknown modes fall back here).
        let cv = ExecutionConfig::from_mode("constant-vus", Some(5), None, None, None);
        assert_eq!(cv.executor_name(), "constant-vus");
        assert_eq!(
            cv.total_duration(),
            Some(std::time::Duration::from_secs(30))
        );

        // ramping-vus: stages JSON wins when present; start_vus defaults to 1.
        let rv = ExecutionConfig::from_mode(
            "ramping-vus",
            Some(3),
            Some("1m".into()),
            None,
            Some(r#"[{"duration":"10s","target":50}]"#.into()),
        );
        match &rv {
            ExecutionConfig::RampingVus {
                stages, start_vus, ..
            } => {
                assert_eq!(stages.len(), 1);
                assert_eq!(stages[0].target, 50);
                assert_eq!(*start_vus, 3);
            }
            _ => panic!("expected ramping-vus"),
        }

        // arrival-rate: rate from vus, max_vus floors at 10.
        let ar = ExecutionConfig::from_mode("arrival-rate", Some(4), None, None, None);
        match &ar {
            ExecutionConfig::ConstantArrivalRate { rate, max_vus, .. } => {
                assert_eq!(*rate, 4.0);
                assert_eq!(*max_vus, 10);
            }
            _ => panic!("expected constant-arrival-rate"),
        }

        // shared-iterations: iterations default 100, vus default 1.
        let si = ExecutionConfig::from_mode("shared-iterations", None, None, Some(500), None);
        match &si {
            ExecutionConfig::SharedIterations {
                iterations, vus, ..
            } => {
                assert_eq!(*iterations, 500);
                assert_eq!(*vus, 1);
            }
            _ => panic!("expected shared-iterations"),
        }
    }

    #[test]
    fn output_into_worker_nulls_streaming_fields() {
        let cfg = OutputConfig {
            reporters: vec!["stdout".into(), "json".into()],
            output_file: Some("out.json".into()),
            summary_export: Some("summary.json".into()),
            json_stream: Some("stream.ndjson".into()),
            statsd_addr: Some("localhost:8125".into()),
            influxdb_addr: Some("localhost:8089".into()),
            ..Default::default()
        };
        let worker = cfg.into_worker();
        assert!(worker.reporters.is_empty());
        assert!(worker.output_file.is_none());
        assert!(worker.summary_export.is_none());
        assert!(worker.json_stream.is_none());
        assert!(worker.statsd_addr.is_none());
        assert!(worker.influxdb_addr.is_none());
        // Non-streaming fields survive (defaults copied by ..self).
        assert!(worker.summary);
    }
}

// ── Expected HTTP status semantics (P3c) ───────────────────────────
// Moved from tropel-core: `HttpConfig.expected_statuses` (tropel-http)
// embeds these and tropel-runtime evaluates them for http_req_failed, so
// the leaf SDK is the only crate both can share without dragging
// tropel-core into the publish set. The modularization doc's first cut
// proposed tropel-runtime as the home — impossible: tropel-http cannot
// depend on tropel-runtime (P5b wasm gate) and tropel-runtime cannot
// depend on tropel-http in production (reqwest would enter the wasm
// graph). SDK it is.

/// Expected status code or range for determining http_req_failed.
/// A request fails (http_req_failed=1) when the response status code
/// does NOT fall within any of the expected entries.
///
/// Each entry can be:
/// - A single code: `200`
/// - A range: `"200-399"`
/// - A pattern with wildcard: `"2xx"`, `"3xx"`
///
/// Default: `["200-399"]` — all 2xx and 3xx are considered success.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ExpectedStatus {
    Single(u16),
    Range(String),
}

impl ExpectedStatus {
    /// Strict, fallible parse of an expected-status pattern (W0 P0#3).
    /// Accepts a single code (`"200"`), a range (`"200-399"`), or a
    /// wildcard (`"2xx"`). Rejects malformed input that the old `matches`
    /// silently degraded to a 0..=65535 all-match — `"200-"`, `"-"`,
    /// `"abc-def"`, `"2xx-3xx"`, `"20-30-40"` all used to make a server
    /// that returned nothing but 500s pass every check. Also rejects a
    /// wildcard base whose `base * 100` overflows `u16` (prefix ≥ 656 used
    /// to panic in debug / wrap in release: `"656xx"` matched 64–163).
    pub fn parse(s: &str) -> std::result::Result<ExpectedStatus, String> {
        let s = s.trim();
        if s.is_empty() {
            return Err("expected-status pattern is empty".into());
        }
        if let Some((lo, hi)) = s.split_once('-') {
            // Range "200-399": exactly one dash, both sides valid u16, lo <= hi.
            if hi.contains('-') {
                return Err(format!("invalid expected-status range '{s}'"));
            }
            let lo: u16 = lo
                .trim()
                .parse()
                .map_err(|_| format!("invalid expected-status range '{s}'"))?;
            let hi: u16 = hi
                .trim()
                .parse()
                .map_err(|_| format!("invalid expected-status range '{s}'"))?;
            if lo > hi {
                return Err(format!("invalid expected-status range '{s}' (lo > hi)"));
            }
            return Ok(ExpectedStatus::Range(s.to_string()));
        }
        if let Some(prefix) = s.strip_suffix("xx") {
            // Wildcard "2xx": prefix parses as u16 AND the full 100-wide
            // span [base*100, base*100+99] fits u16 (base <= 654). A base of
            // 655 passes base*100 (65500) but base*100+99 overflows, so the
            // pattern could never match anything — reject it rather than
            // accepting an unmatchable entry (W0 P0#3).
            let base: u16 = prefix
                .parse()
                .map_err(|_| format!("invalid expected-status wildcard '{s}'"))?;
            let lo = base
                .checked_mul(100)
                .ok_or_else(|| format!("expected-status wildcard '{s}' overflows"))?;
            lo.checked_add(99)
                .ok_or_else(|| format!("expected-status wildcard '{s}' overflows"))?;
            return Ok(ExpectedStatus::Range(s.to_string()));
        }
        // Single code "200" (also "200" as a bare string).
        s.parse::<u16>()
            .map(ExpectedStatus::Single)
            .map_err(|_| format!("invalid expected status '{s}'"))
    }

    /// Check if a given status code matches this expected status entry.
    ///
    /// Fail-closed (W0 P0#3): a malformed pattern NEVER matches — the old
    /// `unwrap_or(0)` / `unwrap_or(u16::MAX)` degraded `"200-"` etc. to
    /// `0..=65535`, so a config typo produced a perfect green run against a
    /// server returning nothing but 500s. Wildcard math is checked, so a
    /// prefix ≥ 656 can neither panic in debug nor silently wrap in release.
    pub fn matches(&self, code: u16) -> bool {
        match self {
            ExpectedStatus::Single(c) => *c == code,
            ExpectedStatus::Range(s) => {
                // Support patterns: "200-399" (range), "2xx" (wildcard), "200" (single)
                if let Some((lo, hi)) = s.split_once('-') {
                    if hi.contains('-') {
                        return false; // "20-30-40" — never all-match
                    }
                    let Ok(lo) = lo.trim().parse::<u16>() else {
                        return false;
                    };
                    let Ok(hi) = hi.trim().parse::<u16>() else {
                        return false;
                    };
                    lo <= hi && code >= lo && code <= hi
                } else if let Some(prefix) = s.strip_suffix("xx") {
                    let Ok(base) = prefix.parse::<u16>() else {
                        return false;
                    };
                    let Some(lo) = base.checked_mul(100) else {
                        return false;
                    };
                    let Some(hi) = lo.checked_add(99) else {
                        return false;
                    };
                    code >= lo && code <= hi
                } else if let Ok(c) = s.parse::<u16>() {
                    c == code
                } else {
                    false
                }
            }
        }
    }
}

impl<'de> serde::Deserialize<'de> for ExpectedStatus {
    /// Validate at config load (W0 P0#3): a malformed pattern string is a
    /// hard config error, not a silent all-match. A JSON number becomes
    /// [`ExpectedStatus::Single`]; a string goes through [`ExpectedStatus::parse`]
    /// and errors on anything malformed.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct ExpectedStatusVisitor;
        impl<'de> serde::de::Visitor<'de> for ExpectedStatusVisitor {
            type Value = ExpectedStatus;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(
                    "a status code number, or a pattern string like \"200\", \"2xx\", \"200-399\"",
                )
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Self::Value, E> {
                let c = u16::try_from(v)
                    .map_err(|_| E::custom(format!("status code out of range: {v}")))?;
                Ok(ExpectedStatus::Single(c))
            }

            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Self::Value, E> {
                let c = u16::try_from(v)
                    .map_err(|_| E::custom(format!("status code out of range: {v}")))?;
                Ok(ExpectedStatus::Single(c))
            }

            fn visit_str<E: serde::de::Error>(
                self,
                s: &str,
            ) -> std::result::Result<Self::Value, E> {
                ExpectedStatus::parse(s).map_err(E::custom)
            }
        }
        d.deserialize_any(ExpectedStatusVisitor)
    }
}

/// Check if a response status code is expected (successful) according to the
/// list of expected statuses. Returns true if the code matches ANY expected entry.
/// Returns false if the list is empty (never succeeds — all requests fail).
pub fn status_is_expected(code: u16, expected: &[ExpectedStatus]) -> bool {
    if expected.is_empty() {
        return false;
    }
    expected.iter().any(|e| e.matches(code))
}

#[cfg(test)]
mod expected_status_tests {
    use super::*;

    #[test]
    fn expected_status_single_range_wildcard_and_invalid() {
        // Single code.
        assert!(ExpectedStatus::Single(200).matches(200));
        assert!(!ExpectedStatus::Single(200).matches(404));
        // Range "200-399" (default) — 2xx and 3xx succeed, 4xx fails.
        let default = ExpectedStatus::Range("200-399".into());
        assert!(default.matches(200));
        assert!(default.matches(304));
        assert!(default.matches(399));
        assert!(!default.matches(400));
        assert!(!default.matches(199));
        // Wildcard "2xx" → 200-299.
        let xx = ExpectedStatus::Range("2xx".into());
        assert!(xx.matches(200));
        assert!(xx.matches(299));
        assert!(!xx.matches(300));
        assert!(!xx.matches(199));
        // W0 P0#3: malformed patterns NEVER match (fail-closed). The old
        // unwrap_or(0)/unwrap_or(u16::MAX) degraded "200-", "-", "abc-def",
        // "2xx-3xx", "20-30-40" to 0..=65535 — a config typo produced a
        // perfect green run against a server returning nothing but 500s.
        // "656xx" used to panic in debug / wrap to 64-163 in release.
        for bad in [
            "", "abc", "-5", "99999", "200-", "-", "abc-def", "2xx-3xx", "20-30-40", "655xx",
            "656xx",
        ] {
            assert!(!ExpectedStatus::Range(bad.into()).matches(200), "{bad}");
            assert!(!ExpectedStatus::Range(bad.into()).matches(500), "{bad}");
            assert!(
                ExpectedStatus::parse(bad).is_err(),
                "parse must reject '{bad}'"
            );
        }
        // Valid patterns still parse and match.
        assert!(ExpectedStatus::parse("200-399").is_ok());
        assert!(ExpectedStatus::parse("2xx").is_ok());
        assert!(ExpectedStatus::parse("200").is_ok());
        // "654xx" is the widest wildcard whose full 100-wide span fits u16
        // (65400-65499); "655xx" and above overflow and are rejected above.
        assert!(ExpectedStatus::parse("654xx").is_ok());
    }

    /// W0 P0#3: deserializing a malformed pattern string is a hard config
    /// error (validated at config load), not a silent all-match.
    #[test]
    fn expected_status_deserialize_rejects_malformed() {
        for bad in [
            "200-", "-", "abc-def", "2xx-3xx", "20-30-40", "655xx", "656xx", "x-y",
        ] {
            let res: std::result::Result<ExpectedStatus, _> =
                serde_json::from_str(&format!("\"{bad}\""));
            assert!(res.is_err(), "deserialize must reject '{bad}'");
        }
        // Valid forms still deserialize.
        assert!(serde_json::from_str::<ExpectedStatus>("200").is_ok());
        assert!(serde_json::from_str::<ExpectedStatus>("\"2xx\"").is_ok());
        assert!(serde_json::from_str::<ExpectedStatus>("\"200-399\"").is_ok());
    }

    #[test]
    fn status_is_expected_empty_list_never_succeeds() {
        // Documented contract: empty expected list = ALL requests fail.
        assert!(!status_is_expected(200, &[]));
        assert!(!status_is_expected(500, &[]));
        // Any-of semantics.
        let set = [
            ExpectedStatus::Single(200),
            ExpectedStatus::Range("4xx".into()),
        ];
        assert!(status_is_expected(200, &set));
        assert!(status_is_expected(404, &set));
        assert!(!status_is_expected(500, &set));
    }

    /// W0 P0#1: `timeUnit` must accept the k6 camelCase alias and default
    /// to "1s" when omitted (matching `RampingArrivalRate`'s sibling field).
    #[test]
    fn constant_arrival_rate_time_unit_alias_and_default() {
        // CamelCase alias round-trips (internally-tagged enum, tag = "type").
        let json = r#"{
            "type": "constant-arrival-rate",
            "rate": 100.0,
            "timeUnit": "1m",
            "duration": "30s",
            "pre_alloc_vus": 5,
            "max_vus": 20
        }"#;
        let exec: ExecutionConfig = serde_json::from_str(json).unwrap();
        match exec {
            ExecutionConfig::ConstantArrivalRate { time_unit, .. } => {
                assert_eq!(time_unit, "1m");
            }
            other => panic!("expected ConstantArrivalRate, got {other:?}"),
        }

        // Omitted → default "1s" (the identity case, k6 parity).
        let json = r#"{
            "type": "constant-arrival-rate",
            "rate": 10.0,
            "duration": "30s",
            "pre_alloc_vus": 5,
            "max_vus": 20
        }"#;
        let exec: ExecutionConfig = serde_json::from_str(json).unwrap();
        match exec {
            ExecutionConfig::ConstantArrivalRate { time_unit, .. } => {
                assert_eq!(time_unit, "1s");
            }
            other => panic!("expected ConstantArrivalRate, got {other:?}"),
        }
    }

    /// W0 P0#4: k6 camelCase scenario options must map onto the snake_case
    /// SDK fields (`startVUs` → `start_vus`, `preAllocatedVUs` →
    /// `pre_alloc_vus`, `maxVUs` → `max_vus`, `startRate` → `start_rate`,
    /// `maxDuration` → `max_duration`, `startTime` → `start_time`) instead of
    /// being silently dropped, and `start_time` must default to "0s" (the
    /// old `#[serde(default)]` on a String produced "" which failed
    /// `parse_duration` — the most common config errored).
    #[test]
    fn k6_camelcase_scenario_options_map_and_typos_error() {
        // RampingVus: startVUs → start_vus.
        let json = r#"{
            "type": "ramping-vus",
            "startVUs": 7,
            "stages": [{"duration": "30s", "target": 20}]
        }"#;
        match serde_json::from_str::<ExecutionConfig>(json).unwrap() {
            ExecutionConfig::RampingVus { start_vus, .. } => assert_eq!(start_vus, 7),
            other => panic!("expected RampingVus, got {other:?}"),
        }

        // ConstantArrivalRate: preAllocatedVUs + maxVUs → pre_alloc_vus/max_vus.
        let json = r#"{
            "type": "constant-arrival-rate",
            "rate": 100.0,
            "duration": "30s",
            "preAllocatedVUs": 500,
            "maxVUs": 2000
        }"#;
        match serde_json::from_str::<ExecutionConfig>(json).unwrap() {
            ExecutionConfig::ConstantArrivalRate {
                pre_alloc_vus,
                max_vus,
                ..
            } => {
                assert_eq!(pre_alloc_vus, 500);
                assert_eq!(max_vus, 2000);
            }
            other => panic!("expected ConstantArrivalRate, got {other:?}"),
        }

        // RampingArrivalRate: startRate + preAllocatedVUs + maxVUs.
        let json = r#"{
            "type": "ramping-arrival-rate",
            "startRate": 10,
            "timeUnit": "1s",
            "preAllocatedVUs": 500,
            "maxVUs": 2000,
            "stages": [{"duration": "30s", "target": 100}]
        }"#;
        match serde_json::from_str::<ExecutionConfig>(json).unwrap() {
            ExecutionConfig::RampingArrivalRate {
                start_rate,
                pre_alloc_vus,
                max_vus,
                ..
            } => {
                assert_eq!(start_rate, 10.0);
                assert_eq!(pre_alloc_vus, 500);
                assert_eq!(max_vus, 2000);
            }
            other => panic!("expected RampingArrivalRate, got {other:?}"),
        }

        // SharedIterations: maxDuration → max_duration.
        let json = r#"{
            "type": "shared-iterations",
            "iterations": 1000,
            "vus": 10,
            "maxDuration": "2m"
        }"#;
        match serde_json::from_str::<ExecutionConfig>(json).unwrap() {
            ExecutionConfig::SharedIterations { max_duration, .. } => {
                assert_eq!(max_duration.as_deref(), Some("2m"));
            }
            other => panic!("expected SharedIterations, got {other:?}"),
        }

        // ScenarioConfig.start_time: startTime alias + "0s" default.
        let json = r#"{
            "execution": {"type": "constant-vus", "vus": 1, "duration": "10s"},
            "startTime": "5s"
        }"#;
        let sc: ScenarioConfig = serde_json::from_str(json).unwrap();
        assert_eq!(sc.start_time, "5s");
        let json = r#"{
            "execution": {"type": "constant-vus", "vus": 1, "duration": "10s"}
        }"#;
        let sc: ScenarioConfig = serde_json::from_str(json).unwrap();
        assert_eq!(sc.start_time, "0s");

        // ExternallyControlled: maxVUs → max_vus.
        let json = r#"{
            "type": "externally-controlled",
            "vus": 5,
            "maxVUs": 50
        }"#;
        match serde_json::from_str::<ExecutionConfig>(json).unwrap() {
            ExecutionConfig::ExternallyControlled { max_vus, .. } => assert_eq!(max_vus, 50),
            other => panic!("expected ExternallyControlled, got {other:?}"),
        }

        // ThresholdConfig: abortOnFail → abort_on_fail.
        let json = r#"{"expression": "http_req_failed < 0.01", "abortOnFail": true}"#;
        let th: ThresholdConfig = serde_json::from_str(json).unwrap();
        assert!(th.abort_on_fail);

        // deny_unknown_fields: a typo'd camelCase option is a hard error, not
        // a silent drop (the W0 P0#4 failure mode: maxVU instead of maxVUs).
        let json = r#"{
            "type": "constant-arrival-rate",
            "rate": 100.0,
            "duration": "30s",
            "preAllocatedVUs": 500,
            "maxVU": 2000
        }"#;
        assert!(
            serde_json::from_str::<ExecutionConfig>(json).is_err(),
            "typo'd maxVU must be a hard config error"
        );
        let json = r#"{
            "type": "constant-vus",
            "vus": 1,
            "duration": "10s",
            "vuss": 2
        }"#;
        assert!(
            serde_json::from_str::<ExecutionConfig>(json).is_err(),
            "typo'd vuss must be a hard config error"
        );
    }
}
