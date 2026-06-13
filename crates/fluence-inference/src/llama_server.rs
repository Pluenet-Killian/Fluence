// SPDX-License-Identifier: Apache-2.0

//! [`LlamaServerBackend`] — the local llama.cpp backend realised as a thin HTTP
//! client to a supervised `llama-server` subprocess (ADR-0007 amendment).
//!
//! The hub spawns `llama-server` (the official llama.cpp binary) as a child
//! process and this backend talks to it over loopback HTTP. That choice keeps
//! our build free of any C++/CMake compilation, isolates a GGML crash behind a
//! process boundary (D-2.6 « le clavier parle toujours »), and stays portable
//! across Windows and Linux.
//!
//! Two endpoints of `llama-server` are used, both via `POST /completion`:
//! - streaming generation (`stream: true`) drives [`LlmBackend::generate`],
//!   emitting each `content` delta and honouring [`CancelToken`] between SSE
//!   frames;
//! - a single-token request with `n_probs` drives [`LlmBackend::next_chars`]:
//!   the per-token log-probabilities are aggregated by **first character** into
//!   the warm-KV next-character distribution (§5.A), without ever generating a
//!   full completion.
//!
//! The client is synchronous (`ureq`), matching the synchronous [`LlmBackend`]
//! trait; the hub drives it from a blocking task. Connections are pooled, so
//! `cache_prompt: true` lets `llama-server` reuse the conversation's KV cache
//! across calls (D-5.3).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::time::Duration;

use fluence_protocol::Normalized;
use fluence_protocol::api::suggest::CharProb;
use serde_json::{Value, json};

use crate::backend::{BackendError, CancelToken, GenerateOutcome, GenerateRequest, LlmBackend};

/// Sampling temperature for suggestion generation: low, to stay focused on the
/// user's intent rather than wander (§5.A rephrase/continue).
const GENERATE_TEMPERATURE: f64 = 0.2;

/// Spread for the `next_chars` probe: ask `llama-server` for this many ×
/// `top_k` raw token candidates, because many distinct tokens collapse onto the
/// same first character once aggregated. Clamped to a sane absolute range.
const NEXT_CHARS_SPREAD: usize = 6;
/// Lower/upper bounds for the requested `n_probs` count.
const NEXT_CHARS_MIN_PROBS: usize = 24;
const NEXT_CHARS_MAX_PROBS: usize = 100;

/// HTTP timeouts for the [`LlamaServerBackend`] client.
///
/// Defaults are production safety ceilings (not latency targets — the §5.A
/// budgets are measured elsewhere); the point is to fail *eventually* rather
/// than hang forever, so the hub can degrade to the n-gram fallback (D-2.6).
/// `connect` and `recv_response` are applied at the agent level: together they
/// bound the dangerous case of a server that accepts the socket but never sends
/// a response, **without** capping a long streaming body (`generate` may
/// legitimately stream for many seconds — a mid-stream wedge is caught by the
/// supervisor's liveness probe instead). `unary_body` additionally caps the body
/// of the non-streaming `next_chars` probe; `health` bounds each `GET /health`
/// so a wedged server is detected within a few supervisor probe cycles.
#[derive(Debug, Clone, Copy)]
pub struct BackendTimeouts {
    /// Establishing the TCP connection (loopback connect is instant when up).
    pub connect: Duration,
    /// Receiving the response status and headers, but not the body.
    pub recv_response: Duration,
    /// Receiving the full body of a unary (non-streaming) call.
    pub unary_body: Duration,
    /// A whole `GET /health` call (readiness gate and liveness probe).
    pub health: Duration,
}

impl Default for BackendTimeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(5),
            recv_response: Duration::from_secs(30),
            unary_body: Duration::from_secs(30),
            health: Duration::from_secs(3),
        }
    }
}

/// The local llama.cpp backend, speaking HTTP to a `llama-server` subprocess.
///
/// Construct one with [`LlamaServerBackend::new`] pointing at the server's base
/// URL (e.g. `http://127.0.0.1:8080`). The backend is cheap to clone-by-`Arc`
/// and safe to share across threads (`ureq::Agent` is itself shareable).
#[derive(Debug, Clone)]
pub struct LlamaServerBackend {
    /// Fully-qualified `…/completion` endpoint, computed once.
    completion_url: String,
    /// Fully-qualified `…/health` endpoint, computed once.
    health_url: String,
    /// Connection-pooling HTTP agent (keep-alive → warm KV reuse). Carries the
    /// agent-level `connect`/`recv_response` timeouts.
    agent: ureq::Agent,
    /// Timeouts; the per-request ones (`unary_body`, `health`) are applied at
    /// the call sites.
    timeouts: BackendTimeouts,
}

/// Whether a `/completion` call streams (long-lived) or is a single unary probe.
#[derive(Debug, Clone, Copy)]
enum CallMode {
    /// Streaming generation: no body timeout (it runs for the whole completion).
    Streaming,
    /// A single non-streaming response: the body is additionally time-bounded.
    Unary,
}

impl LlamaServerBackend {
    /// A backend talking to the `llama-server` reachable at `base_url`
    /// (scheme + host + port, with or without a trailing slash), with the
    /// default production [`BackendTimeouts`].
    #[must_use]
    pub fn new(base_url: &str) -> Self {
        Self::new_with_timeouts(base_url, BackendTimeouts::default())
    }

    /// As [`new`](Self::new) but with explicit timeouts — tests use short ones
    /// to exercise the timeout paths in milliseconds.
    #[must_use]
    pub fn new_with_timeouts(base_url: &str, timeouts: BackendTimeouts) -> Self {
        let base = base_url.trim_end_matches('/');
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_connect(Some(timeouts.connect))
                .timeout_recv_response(Some(timeouts.recv_response))
                .build(),
        );
        Self {
            completion_url: format!("{base}/completion"),
            health_url: format!("{base}/health"),
            agent,
            timeouts,
        }
    }

    /// Whether the server answers `GET /health` with a success status.
    ///
    /// `llama-server` returns 200 only once the model is loaded and it is
    /// ready to serve; while loading (or down) the call is a non-2xx or a
    /// connection error, both of which read as `false`. The hub's supervisor
    /// polls this to gate readiness before routing traffic to the backend.
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        self.agent
            .get(&self.health_url)
            .config()
            .timeout_global(Some(self.timeouts.health))
            .build()
            .call()
            .is_ok()
    }

    /// POSTs `body` to `/completion` and returns the response body reader.
    ///
    /// `ureq` treats a non-2xx status as an error by default, so a server that
    /// is up but refusing the request surfaces here as [`BackendError`]. The
    /// agent-level `connect`/`recv_response` timeouts bound a server that
    /// accepts the socket but never replies; a [`CallMode::Unary`] call also
    /// caps the body so a stalled non-streaming probe cannot hang.
    fn open_completion(&self, body: &Value, mode: CallMode) -> Result<impl Read, BackendError> {
        let request = self
            .agent
            .post(&self.completion_url)
            .header("content-type", "application/json");
        let request = match mode {
            // A streaming generation runs for the whole completion, so it must
            // not carry a body timeout — only connect/recv-response apply, and a
            // mid-stream wedge is caught by the supervisor's liveness probe.
            CallMode::Streaming => request,
            CallMode::Unary => request
                .config()
                .timeout_recv_body(Some(self.timeouts.unary_body))
                .build(),
        };
        let response = request.send(body.to_string()).map_err(|err| {
            BackendError::Unavailable(format!("llama-server request failed: {err}"))
        })?;
        Ok(response.into_body().into_reader())
    }
}

/// Strips the SSE `data:` field prefix (with or without the conventional
/// trailing space), returning the JSON payload of a frame line.
fn sse_payload(line: &str) -> Option<&str> {
    line.strip_prefix("data:").map(str::trim_start)
}

impl LlmBackend for LlamaServerBackend {
    fn id(&self) -> &'static str {
        "llama-server"
    }

    fn generate(
        &self,
        request: &GenerateRequest,
        cancel: &CancelToken,
        sink: &mut dyn FnMut(&str),
    ) -> Result<GenerateOutcome, BackendError> {
        let body = json!({
            "prompt": request.prompt,
            "n_predict": request.max_tokens,
            "temperature": GENERATE_TEMPERATURE,
            "cache_prompt": true,
            "stream": true,
        });
        let reader = BufReader::new(self.open_completion(&body, CallMode::Streaming)?);

        for line in reader.lines() {
            // Cooperative cancellation between frames: a newer request on the
            // slot trips the token, we stop and drop the reader — closing the
            // socket tells `llama-server` to abort and free the slot.
            if cancel.is_cancelled() {
                return Ok(GenerateOutcome::Cancelled);
            }
            let line = line.map_err(|err| {
                BackendError::Unavailable(format!("reading llama-server stream: {err}"))
            })?;
            let Some(payload) = sse_payload(&line) else {
                continue; // blank separators and `event:` lines
            };
            let chunk: Value = serde_json::from_str(payload).map_err(|err| {
                BackendError::Unavailable(format!("parsing llama-server frame: {err}"))
            })?;
            if let Some(content) = chunk.get("content").and_then(Value::as_str)
                && !content.is_empty()
            {
                sink(content);
            }
            if chunk.get("stop").and_then(Value::as_bool) == Some(true) {
                return Ok(GenerateOutcome::Completed);
            }
        }
        Ok(GenerateOutcome::Completed)
    }

    fn next_chars(&self, context: &str, top_k: usize) -> Result<Vec<CharProb>, BackendError> {
        if top_k == 0 {
            return Ok(Vec::new());
        }
        let n_probs = (top_k * NEXT_CHARS_SPREAD).clamp(NEXT_CHARS_MIN_PROBS, NEXT_CHARS_MAX_PROBS);
        let body = json!({
            "prompt": context,
            "n_predict": 1,
            "n_probs": n_probs,
            "cache_prompt": true,
            "stream": false,
        });

        let mut raw = String::new();
        self.open_completion(&body, CallMode::Unary)?
            .read_to_string(&mut raw)
            .map_err(|err| {
                BackendError::Unavailable(format!("reading llama-server response: {err}"))
            })?;
        let parsed: Value = serde_json::from_str(&raw).map_err(|err| {
            BackendError::Unavailable(format!("parsing llama-server response: {err}"))
        })?;

        // The first (and only) generated position carries the candidate
        // distribution in `top_logprobs`: each `{ token, logprob }` is a vocab
        // entry with its natural-log probability.
        let candidates = parsed
            .get("completion_probabilities")
            .and_then(Value::as_array)
            .and_then(|positions| positions.first())
            .and_then(|first| first.get("top_logprobs"))
            .and_then(Value::as_array)
            .ok_or_else(|| {
                BackendError::Unavailable("llama-server returned no token probabilities".to_owned())
            })?;

        Ok(aggregate_by_first_char(candidates, top_k))
    }
}

/// Folds raw token log-probabilities into a next-character distribution: sum the
/// probability mass of every candidate token sharing a first character, keep the
/// `top_k` heaviest characters, and renormalise so the result sums to 1.
///
/// Robust to a misbehaving server: candidates whose probability is not finite (a
/// malformed or overflowing `logprob`, e.g. `exp(1000) = +inf`) are dropped, and
/// a non-finite total yields an empty distribution — never a NaN that would
/// panic [`Normalized::new`].
fn aggregate_by_first_char(candidates: &[Value], top_k: usize) -> Vec<CharProb> {
    let mut mass: HashMap<char, f64> = HashMap::new();
    for entry in candidates {
        let Some(token) = entry.get("token").and_then(Value::as_str) else {
            continue;
        };
        let Some(logprob) = entry.get("logprob").and_then(Value::as_f64) else {
            continue;
        };
        let Some(ch) = token.chars().next() else {
            continue; // empty token string
        };
        if ch == char::REPLACEMENT_CHARACTER {
            continue; // a partial byte-fallback token, not a real character
        }
        let probability = logprob.exp();
        if !probability.is_finite() {
            // A malformed or huge `logprob` overflowed `exp()` to ±inf. Drop it
            // rather than let a non-finite mass poison the sum into a NaN.
            continue;
        }
        *mass.entry(ch).or_insert(0.0) += probability;
    }

    let mut ranked: Vec<(char, f64)> = mass.into_iter().collect();
    // Heaviest first; tie-break on the character for deterministic output.
    ranked.sort_by(|(lc, lp), (rc, rp)| rp.total_cmp(lp).then(lc.cmp(rc)));
    ranked.truncate(top_k);

    let total: f64 = ranked.iter().map(|&(_, p)| p).sum();
    // A non-finite or non-positive total has no usable distribution. The
    // `is_finite` check also catches a sum that overflowed to +inf, which a bare
    // `<= 0.0` comparison silently misses (any comparison with NaN/inf is false).
    if !total.is_finite() || total <= 0.0 {
        return Vec::new();
    }
    ranked
        .into_iter()
        .map(|(ch, p)| CharProb {
            ch,
            p: Normalized::new((p / total).clamp(0.0, 1.0))
                .expect("a ratio in [0, 1] is a valid probability"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;

    /// Serves exactly one HTTP request with a fixed body, then closes — a
    /// hermetic stand-in for `llama-server`. Returns its base URL.
    fn mock_server(content_type: &str, body: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len(),
        );
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                // Drain the *entire* request (head + Content-Length body) before
                // replying: closing a socket with unread bytes makes Windows send
                // an RST, which the client would see as a failed write (10054).
                drain_request(&mut stream);
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        format!("http://{addr}")
    }

    /// Reads a full HTTP/1.1 request: headers up to `\r\n\r\n`, then the body
    /// announced by `Content-Length` (0 if absent).
    fn drain_request(stream: &mut std::net::TcpStream) {
        let mut buf = Vec::new();
        let mut chunk = [0_u8; 1024];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
            if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n")
                && buf.len() >= end + 4 + content_length(&buf[..end])
            {
                break;
            }
        }
    }

    /// Parses `Content-Length` (case-insensitive) from a request head.
    fn content_length(head: &[u8]) -> usize {
        String::from_utf8_lossy(head)
            .to_ascii_lowercase()
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(0)
    }

    fn collect(backend: &LlamaServerBackend, cancel: &CancelToken) -> (String, GenerateOutcome) {
        let request = GenerateRequest {
            prompt: "veu eau frache".to_owned(),
            max_tokens: 32,
        };
        let mut out = String::new();
        let outcome = backend
            .generate(&request, cancel, &mut |delta| out.push_str(delta))
            .expect("mock server responds");
        (out, outcome)
    }

    #[test]
    fn generate_streams_content_deltas_until_stop() {
        let sse = concat!(
            "data: {\"content\":\"Je \"}\n\n",
            "data: {\"content\":\"voudrais\"}\n\n",
            "data: {\"content\":\"\",\"stop\":true}\n\n",
        );
        let backend = LlamaServerBackend::new(&mock_server("text/event-stream", sse));
        let (text, outcome) = collect(&backend, &CancelToken::new());
        assert_eq!(text, "Je voudrais");
        assert_eq!(outcome, GenerateOutcome::Completed);
    }

    #[test]
    fn generate_tolerates_dataless_frames() {
        // `event:` lines and blank separators must be ignored, not parsed.
        let sse = concat!(
            "event: message\n",
            "data: {\"content\":\"ok\"}\n\n",
            "data: {\"stop\":true}\n\n",
        );
        let backend = LlamaServerBackend::new(&mock_server("text/event-stream", sse));
        let (text, outcome) = collect(&backend, &CancelToken::new());
        assert_eq!(text, "ok");
        assert_eq!(outcome, GenerateOutcome::Completed);
    }

    #[test]
    fn generate_stops_when_cancelled() {
        let sse = concat!(
            "data: {\"content\":\"un\"}\n\n",
            "data: {\"content\":\"deux\"}\n\n",
            "data: {\"stop\":true}\n\n",
        );
        let backend = LlamaServerBackend::new(&mock_server("text/event-stream", sse));
        let cancel = CancelToken::new();
        cancel.cancel(); // tripped before the first frame is processed
        let (text, outcome) = collect(&backend, &cancel);
        assert_eq!(outcome, GenerateOutcome::Cancelled);
        assert!(text.is_empty(), "nothing should be emitted after cancel");
    }

    #[test]
    fn next_chars_aggregates_tokens_by_first_character() {
        // 'v': ville(0.5) + v(0.1) = 0.6 ; ' ': " de"(0.3) ; ',': ","(0.1).
        let ln = |p: f64| p.ln();
        let json = format!(
            r#"{{"completion_probabilities":[{{"token":"ville","top_logprobs":[
                {{"token":"ville","logprob":{}}},
                {{"token":" de","logprob":{}}},
                {{"token":"v","logprob":{}}},
                {{"token":",","logprob":{}}}
            ]}}]}}"#,
            ln(0.5),
            ln(0.3),
            ln(0.1),
            ln(0.1),
        );
        let backend = LlamaServerBackend::new(&mock_server("application/json", &json));
        let dist = backend.next_chars("bonjou", 8).expect("mock responds");

        assert_eq!(dist.len(), 3, "three distinct first characters");
        assert_eq!(dist[0].ch, 'v');
        assert_eq!(dist[1].ch, ' ');
        assert_eq!(dist[2].ch, ',');
        let total: f64 = dist.iter().map(|c| c.p.get()).sum();
        assert!((total - 1.0).abs() < 1e-9, "must sum to 1, got {total}");
        for pair in dist.windows(2) {
            assert!(pair[0].p.get() >= pair[1].p.get(), "must be descending");
        }
    }

    #[test]
    fn next_chars_truncates_to_top_k_and_renormalises() {
        let ln = |p: f64| p.ln();
        let json = format!(
            r#"{{"completion_probabilities":[{{"token":"a","top_logprobs":[
                {{"token":"a","logprob":{}}},
                {{"token":"b","logprob":{}}},
                {{"token":"c","logprob":{}}}
            ]}}]}}"#,
            ln(0.6),
            ln(0.3),
            ln(0.1),
        );
        let backend = LlamaServerBackend::new(&mock_server("application/json", &json));
        let dist = backend.next_chars("x", 2).expect("mock responds");
        assert_eq!(dist.len(), 2);
        let total: f64 = dist.iter().map(|c| c.p.get()).sum();
        assert!((total - 1.0).abs() < 1e-9, "renormalised to 1, got {total}");
    }

    #[test]
    fn next_chars_with_zero_top_k_is_empty_without_a_request() {
        // Points at a dead port: if it returned empty without calling out, good;
        // if it tried to connect it would still error, so assert Ok+empty.
        let backend = LlamaServerBackend::new("http://127.0.0.1:1");
        assert_eq!(backend.next_chars("x", 0).expect("no request made"), vec![]);
    }

    #[test]
    fn missing_probabilities_field_is_unavailable() {
        let backend =
            LlamaServerBackend::new(&mock_server("application/json", r#"{"content":""}"#));
        assert!(matches!(
            backend.next_chars("x", 4),
            Err(BackendError::Unavailable(_))
        ));
    }

    #[test]
    fn next_chars_survives_a_non_finite_logprob() {
        // A buggy or hostile server can send a `logprob` so large that `exp()`
        // overflows to +inf. The aggregation must never panic — it used to:
        // inf/inf = NaN, and `Normalized::new(NaN)` is the `Err` the code
        // `expect`ed away. The non-finite mass is dropped; finite candidates
        // still yield a valid distribution.
        let json = r#"{"completion_probabilities":[{"token":"a","top_logprobs":[
            {"token":"a","logprob":1000.0},
            {"token":"b","logprob":-0.5}
        ]}]}"#;
        let backend = LlamaServerBackend::new(&mock_server("application/json", json));
        let dist = backend
            .next_chars("x", 4)
            .expect("a finite candidate remains");
        assert!(
            dist.iter().all(|c| c.p.get().is_finite()),
            "every probability must be finite"
        );
        assert!(
            dist.iter().all(|c| c.ch != 'a'),
            "the overflowing candidate is dropped, not ranked"
        );
    }

    /// Live smoke test against a running `llama-server`. Set
    /// `FLUENCE_LLAMA_SERVER_URL` (e.g. `http://127.0.0.1:8089`) and run with
    /// `cargo test -p fluence-inference -- --ignored`; without the variable it
    /// is a no-op, so the gated CI/nightly job can opt in explicitly.
    #[test]
    #[ignore = "requires a running llama-server (set FLUENCE_LLAMA_SERVER_URL)"]
    fn live_llama_server_generates_and_ranks_characters() {
        let Ok(url) = std::env::var("FLUENCE_LLAMA_SERVER_URL") else {
            return;
        };
        let backend = LlamaServerBackend::new(&url);

        let mut out = String::new();
        let outcome = backend
            .generate(
                &GenerateRequest {
                    prompt: "Bonjour, je m'appelle".to_owned(),
                    max_tokens: 16,
                },
                &CancelToken::new(),
                &mut |delta| out.push_str(delta),
            )
            .expect("live generate");
        assert_eq!(outcome, GenerateOutcome::Completed);
        assert!(!out.trim().is_empty(), "the model produced no text");

        let dist = backend.next_chars("Bonjou", 5).expect("live next_chars");
        assert!(!dist.is_empty(), "no next-character distribution");
        let total: f64 = dist.iter().map(|c| c.p.get()).sum();
        assert!((total - 1.0).abs() < 1e-6, "distribution must sum to 1");
        for pair in dist.windows(2) {
            assert!(pair[0].p.get() >= pair[1].p.get(), "must be descending");
        }
    }

    #[test]
    fn is_healthy_reflects_server_reachability() {
        let backend =
            LlamaServerBackend::new(&mock_server("application/json", r#"{"status":"ok"}"#));
        assert!(backend.is_healthy(), "a 200 from /health means ready");
        assert!(
            !LlamaServerBackend::new("http://127.0.0.1:1").is_healthy(),
            "a refused connection is not healthy"
        );
    }

    #[test]
    fn a_refused_connection_degrades_to_unavailable() {
        // Nothing listens on port 1 → connection refused → Unavailable (the hub
        // then degrades to the n-gram fallback, never a 5xx).
        let backend = LlamaServerBackend::new("http://127.0.0.1:1");
        assert!(matches!(
            backend.generate(
                &GenerateRequest {
                    prompt: "x".to_owned(),
                    max_tokens: 8
                },
                &CancelToken::new(),
                &mut |_| {},
            ),
            Err(BackendError::Unavailable(_))
        ));
    }

    /// A server that accepts the connection then never replies, holding the
    /// socket open — the "up but wedged" case the timeouts must survive. The
    /// listener thread lingers briefly, longer than the tests' short timeout.
    fn wedged_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                thread::sleep(Duration::from_secs(3));
                drop(stream);
            }
        });
        format!("http://{addr}")
    }

    /// Short timeouts so the wedged-server paths resolve in milliseconds.
    fn fast_timeouts() -> BackendTimeouts {
        BackendTimeouts {
            connect: Duration::from_secs(1),
            recv_response: Duration::from_millis(200),
            unary_body: Duration::from_millis(200),
            health: Duration::from_millis(200),
        }
    }

    #[test]
    fn generate_times_out_when_the_server_never_replies() {
        // Without a response timeout this call would hang forever, stalling
        // `/suggest`; the timeout turns it into a prompt Unavailable so the hub
        // degrades to the n-gram fallback (D-2.6 « le clavier parle toujours »).
        let backend = LlamaServerBackend::new_with_timeouts(&wedged_server(), fast_timeouts());
        let start = std::time::Instant::now();
        let result = backend.generate(
            &GenerateRequest {
                prompt: "x".to_owned(),
                max_tokens: 8,
            },
            &CancelToken::new(),
            &mut |_| {},
        );
        assert!(
            matches!(result, Err(BackendError::Unavailable(_))),
            "a wedged server must surface as Unavailable"
        );
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "must time out promptly, took {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn next_chars_times_out_when_the_server_never_replies() {
        let backend = LlamaServerBackend::new_with_timeouts(&wedged_server(), fast_timeouts());
        let start = std::time::Instant::now();
        let result = backend.next_chars("x", 4);
        assert!(matches!(result, Err(BackendError::Unavailable(_))));
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "must time out promptly, took {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn is_healthy_is_false_when_the_server_never_replies() {
        let backend = LlamaServerBackend::new_with_timeouts(&wedged_server(), fast_timeouts());
        let start = std::time::Instant::now();
        assert!(!backend.is_healthy(), "a wedged server is not healthy");
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "health probe must fail promptly, took {:?}",
            start.elapsed()
        );
    }
}
