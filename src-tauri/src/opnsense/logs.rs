//! Live firewall log streaming over the OPNsense SSE endpoint
//! (`GET /api/diagnostics/firewall/streamLog`).
//!
//! Unlike the MikroTik log stream (which polls a memory buffer), OPNsense
//! pushes log lines as server-sent events: one request stays open for the
//! life of the session with no total response deadline. The reader below
//! consumes the byte stream incrementally (frames may split across arbitrary
//! chunk boundaries), normalizes each JSON record into an
//! [`OpnsenseLogEvent`], and forwards it on the event channel. The UI keeps
//! at most its newest 500 entries client-side; there is deliberately NO DB
//! session behind a log stream — it is live-only.
//!
//! Failure posture: an open failure (404/403/auth/network) is reported as a
//! terminal `Error` status — never as an empty log. A stream that fails
//! mid-read likewise emits `Error` and stops, freeing its slot so the view
//! can offer a reconnect. Normal shutdown (explicit stop) emits only
//! `Stopped`. `Stopped` is emitted exactly once per session, and the shared
//! activity registry (`log_enter`/`log_exit`) is always balanced so profile
//! deletion stays guarded while a stream is live.

use std::collections::HashMap;
use std::sync::Arc;

use futures_util::StreamExt;
use serde_json::Value;
use tokio::sync::{oneshot, watch, Mutex};
use tokio::task::JoinHandle;

use crate::db::now_rfc3339;
use crate::opnsense::client::{OpnsenseError, SseByteStream};
use crate::opnsense::manager::{OpnsenseActivityRegistry, OpnsenseManager};
use crate::opnsense::types::{
    OpnsenseLogEvent, OpnsenseLogEventSink, OpnsenseLogStartDto, OpnsenseLogStatusEvent,
    OpnsenseLogStatusSink, OpnsenseManagerError,
};

/// Upper bound for buffered frame data (256 KiB). A frame exceeding this is
/// a fatal stream error: the reader stops with an `Error` status instead of
/// buffering unbounded data from a misbehaving endpoint.
const MAX_FRAME_BYTES: usize = 256 * 1024;

#[derive(Clone)]
pub struct OpnsenseLogStreamManager {
    manager: OpnsenseManager,
    activity: OpnsenseActivityRegistry,
    inner: Arc<Mutex<HashMap<i64, ActiveLog>>>,
}

struct ActiveLog {
    stop_tx: watch::Sender<bool>,
    join_handle: JoinHandle<Result<(), OpnsenseManagerError>>,
}

impl OpnsenseLogStreamManager {
    pub fn new(manager: OpnsenseManager) -> Self {
        Self {
            activity: manager.activity(),
            manager,
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Open the SSE log stream for one profile and start its reader task.
    /// One stream per profile; a second start on the same profile is a
    /// typed [`OpnsenseManagerError::AlreadyMonitoring`]. An open failure
    /// (404/403/auth/network) is reported as a terminal `Error` status and
    /// no slot is claimed — the view must never fake an empty log.
    pub async fn start(
        &self,
        profile_id: i64,
        on_event: OpnsenseLogEventSink,
        on_status: OpnsenseLogStatusSink,
    ) -> Result<OpnsenseLogStartDto, OpnsenseManagerError> {
        {
            let inner = self.inner.lock().await;
            if inner.contains_key(&profile_id) {
                return Err(OpnsenseManagerError::AlreadyMonitoring(profile_id));
            }
        }
        // Not under the lock: profile lookup, credential fetch and the
        // HTTP open must not block stop/is_active for other profiles.
        let profile = self.manager.require_profile(profile_id).await?;
        let conn = self.manager.connection_for(&profile).await?;
        let api = match (self.manager.api_factory())(conn).await {
            Ok(api) => api,
            Err(err) => {
                on_status(OpnsenseLogStatusEvent::Error {
                    profile_id,
                    message: format!("{err}"),
                });
                return Err(err);
            }
        };
        let stream = match api.open_log_stream().await {
            Ok(stream) => stream,
            Err(err) => {
                let message = match &err {
                    OpnsenseError::NotFound(_) => {
                        "Firewall log stream unavailable (404): the diagnostics API is missing \
                         on this OPNsense version"
                            .to_owned()
                    }
                    OpnsenseError::Forbidden => {
                        "Access denied (403): the API key lacks privileges for the firewall \
                         log stream"
                            .to_owned()
                    }
                    _ => format!("{err}"),
                };
                on_status(OpnsenseLogStatusEvent::Error {
                    profile_id,
                    message,
                });
                return Err(err.into());
            }
        };

        // Re-validate under the lock: another start may have claimed the
        // profile while the stream was opening.
        let mut inner = self.inner.lock().await;
        if inner.contains_key(&profile_id) {
            return Err(OpnsenseManagerError::AlreadyMonitoring(profile_id));
        }
        // The reader waits for `go` before touching the registry, emitting
        // status events or registering activity, so the slot below is
        // always published before the reader can observe (or clean up) it.
        let (go_tx, go_rx) = oneshot::channel();
        let (stop_tx, stop_rx) = watch::channel(false);
        let join_handle = tokio::spawn(run_reader(ReaderContext {
            profile_id,
            activity: Arc::clone(&self.activity),
            inner: Arc::clone(&self.inner),
            on_event,
            on_status,
            stream,
            stop_rx,
            go: go_rx,
        }));
        inner.insert(
            profile_id,
            ActiveLog {
                stop_tx,
                join_handle,
            },
        );
        drop(inner);
        let _ = go_tx.send(());

        Ok(OpnsenseLogStartDto { profile_id })
    }

    /// Cancel the stream for ONE profile and wait for its reader task. A
    /// reader that already terminated on its own (terminal `Error`) removed
    /// its own slot, so a late stop reports [`OpnsenseManagerError::NoActiveMonitor`]
    /// — same idempotent-stop semantics as the MikroTik log stream.
    pub async fn stop(&self, profile_id: i64) -> Result<(), OpnsenseManagerError> {
        let active = {
            self.inner
                .lock()
                .await
                .remove(&profile_id)
                .ok_or(OpnsenseManagerError::NoActiveMonitor)?
        };
        let _ = active.stop_tx.send(true);
        // The terminal `Error` status event is the report channel for
        // failed readers; a panicked task surfaces as a join error which
        // stop() deliberately swallows to stay idempotent from the UI.
        let _ = active.join_handle.await;
        Ok(())
    }

    pub async fn is_active(&self, profile_id: i64) -> bool {
        self.inner.lock().await.contains_key(&profile_id)
    }
}

struct ReaderContext {
    profile_id: i64,
    activity: OpnsenseActivityRegistry,
    inner: Arc<Mutex<HashMap<i64, ActiveLog>>>,
    on_event: OpnsenseLogEventSink,
    on_status: OpnsenseLogStatusSink,
    stream: SseByteStream,
    stop_rx: watch::Receiver<bool>,
    go: oneshot::Receiver<()>,
}

async fn run_reader(mut ctx: ReaderContext) -> Result<(), OpnsenseManagerError> {
    // Wait until start() has published the slot — or stop wins the race
    // before the reader began (then nothing was ever registered).
    tokio::select! {
        _ = &mut ctx.go => {}
        _ = ctx.stop_rx.changed() => return Ok(()),
    }

    ctx.activity.lock().await.log_enter(ctx.profile_id);
    (ctx.on_status)(OpnsenseLogStatusEvent::Started {
        profile_id: ctx.profile_id,
    });

    let result = read_loop(&mut ctx).await;

    // Exactly-once shutdown: `Stopped` fires on every exit path after the
    // session started, and the slot + activity registration are always
    // released together.
    (ctx.on_status)(OpnsenseLogStatusEvent::Stopped {
        profile_id: ctx.profile_id,
    });
    // Release the slot and the activity registration together. If stop()
    // already removed the slot, this remove is a no-op.
    ctx.inner.lock().await.remove(&ctx.profile_id);
    ctx.activity.lock().await.log_exit(ctx.profile_id);
    result
}

/// Consume SSE chunks until cancellation, stream end, or a fatal error.
/// Events parsed from complete frames are stamped with the profile id and
/// forwarded on the event channel.
///
/// The event sink returns `false` when its Tauri channel receiver has
/// dropped; this stops the stream without requiring a view-unmount command.
async fn read_loop(ctx: &mut ReaderContext) -> Result<(), OpnsenseManagerError> {
    let profile_id = ctx.profile_id;
    let mut parser = SseParser::new();
    loop {
        tokio::select! {
            changed = ctx.stop_rx.changed() => {
                // Both an explicit `true` send and a dropped sender mean
                // "stop" — either way the consumer is done with us.
                let _ = changed;
                return Ok(());
            }
            item = ctx.stream.next() => match item {
                None => {
                    // The firewall closed the response (reload, reboot,
                    // session expiry). Not an auth failure: report and end.
                    (ctx.on_status)(OpnsenseLogStatusEvent::Warning {
                        profile_id,
                        message: "log stream closed by the firewall — reconnect to resume"
                            .to_owned(),
                    });
                    return Ok(());
                }
                Some(Ok(bytes)) => match parser.feed(&bytes) {
                    Ok(events) => {
                        for mut event in events {
                            event.profile_id = profile_id;
                            if !(ctx.on_event)(event) {
                                return Ok(());
                            }
                        }
                    }
                    Err(ParseFailure::OversizedFrame) => {
                        (ctx.on_status)(OpnsenseLogStatusEvent::Error {
                            profile_id,
                            message: format!(
                                "a firewall log frame exceeded the {} KiB limit — \
                                 the stream was stopped",
                                MAX_FRAME_BYTES / 1024
                            ),
                        });
                        return Err(OpnsenseManagerError::Api(OpnsenseError::TooLarge(
                            MAX_FRAME_BYTES,
                        )));
                    }
                },
                Some(Err(err)) => {
                    (ctx.on_status)(OpnsenseLogStatusEvent::Error {
                        profile_id,
                        message: format!("{err}"),
                    });
                    return Err(err.into());
                }
            },
        }
    }
}

/// The single way a frame can be undeliverable: buffered data past the cap.
/// Everything else (malformed JSON, non-object payloads, unknown fields) is
/// dropped per-frame without disturbing the stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParseFailure {
    /// Buffered frame data exceeded [`MAX_FRAME_BYTES`]: fatal for the
    /// stream — a misbehaving endpoint must not force unbounded buffering.
    OversizedFrame,
}

/// Incremental SSE frame parser. Operates on raw bytes because frames may
/// split across arbitrary chunk boundaries, including inside a multi-byte
/// UTF-8 sequence; bytes are only decoded (lossily) once a frame completes.
///
/// Frame rules implemented: lines end at `\n` (a trailing `\r` is tolerated
/// for CRLF), a blank line terminates a frame, `data:` payloads within one
/// frame join with `\n`, comment lines (`:`), `event:`/`id:`/`retry:` and
/// unknown fields are ignored, and comment-only frames (heartbeats) produce
/// nothing.
struct SseParser {
    /// The line currently being read (no terminator seen yet).
    line: Vec<u8>,
    /// Joined `data:` payloads of the frame in progress.
    data: Vec<u8>,
    /// Whether any `data:` line was seen for the frame in progress.
    has_data: bool,
}

impl SseParser {
    fn new() -> Self {
        Self {
            line: Vec::new(),
            data: Vec::new(),
            has_data: false,
        }
    }

    /// Feed a raw chunk; returns the events completed by this chunk. Only
    /// an oversized frame is an error — malformed JSON inside an otherwise
    /// valid frame is dropped silently (the stream continues).
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<OpnsenseLogEvent>, ParseFailure> {
        let mut events = Vec::new();
        for &byte in bytes {
            if byte == b'\n' {
                self.handle_line(&mut events);
            } else {
                self.line.push(byte);
                // Data only ever grows by moving a complete line into it,
                // so checking the pending line here caps the total.
                if self.line.len() + self.data.len() > MAX_FRAME_BYTES {
                    return Err(ParseFailure::OversizedFrame);
                }
            }
        }
        Ok(events)
    }

    fn handle_line(&mut self, events: &mut Vec<OpnsenseLogEvent>) {
        let line = std::mem::take(&mut self.line);
        // Tolerate CRLF line endings.
        let line = line.strip_suffix(b"\r").unwrap_or(&line);
        if line.is_empty() {
            self.end_frame(events);
            return;
        }
        // Comment / heartbeat line: the frame continues after it.
        if line[0] == b':' {
            return;
        }
        let (name, value) = match line.iter().position(|&b| b == b':') {
            Some(pos) => {
                let value = &line[pos + 1..];
                // A single leading space after the colon is part of the
                // field syntax, not the value.
                let value = value.strip_prefix(b" ").unwrap_or(value);
                (&line[..pos], value)
            }
            // A field line without a colon is a name with an empty value.
            None => (line, &[][..]),
        };
        if name == b"data" {
            if self.has_data {
                self.data.push(b'\n');
            }
            self.data.extend_from_slice(value);
            self.has_data = true;
        }
        // `event:`, `id:`, `retry:` and unknown fields are ignored.
    }

    fn end_frame(&mut self, events: &mut Vec<OpnsenseLogEvent>) {
        if !self.has_data {
            // Heartbeat or comment-only frame — nothing to deliver.
            return;
        }
        self.has_data = false;
        let data = std::mem::take(&mut self.data);
        let payload = String::from_utf8_lossy(&data);
        let parsed: Value = match serde_json::from_str(payload.trim()) {
            Ok(value) => value,
            // Malformed JSON in one frame must not kill the stream.
            Err(_) => return,
        };
        match parsed {
            Value::Object(map) => events.push(event_from_object(&map)),
            Value::Array(items) => {
                for item in items {
                    if let Value::Object(map) = item {
                        events.push(event_from_object(&map));
                    }
                }
            }
            // A scalar payload carries no log record — drop it.
            _ => {}
        }
    }
}

/// Normalize one JSON log record. Current OPNsense `read_log.py` emits
/// `__timestamp__`, `interface`, `protoname`, `src`/`srcport`,
/// `dst`/`dstport`, `label` and `rid`; older/alternative spellings remain
/// accepted. Missing fields stay `None`, never fabricated.
///
/// `action` is lowercased and mapped: `pass` → "pass" (allowed),
/// `block`/`reject` → "block" (blocked), anything else or missing →
/// "other" (All tab only). An unknown action is NEVER reinterpreted as
/// allowed. `profile_id` is stamped by the reader; `received_at` is the
/// local parse time.
fn event_from_object(map: &serde_json::Map<String, Value>) -> OpnsenseLogEvent {
    let text = |keys: &[&str]| -> Option<String> {
        keys.iter()
            .find_map(|key| {
                map.get(*key)
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
            })
            .map(str::to_owned)
    };
    let action = match map
        .get("action")
        .and_then(Value::as_str)
        .map(str::to_lowercase)
        .as_deref()
    {
        Some("pass") => "pass",
        Some("block") | Some("reject") => "block",
        _ => "other",
    };
    OpnsenseLogEvent {
        profile_id: 0,
        received_at: now_rfc3339(),
        action: action.to_owned(),
        timestamp: text(&["__timestamp__", "timestamp", "time"]),
        interface_name: text(&["interface_name", "interface", "if"]),
        dir: text(&["dir", "direction"]),
        proto: text(&["protoname", "proto", "protocol"]),
        src: text(&["src", "src_addr"]),
        src_port: text(&["srcport", "src_port"]),
        dst: text(&["dst", "dst_addr"]),
        dst_port: text(&["dstport", "dst_port"]),
        label: text(&["label", "rule", "rid"]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex as SyncMutex;

    use crate::db::{Database, NewOpnsenseProfile};
    use crate::opnsense::client::{
        FirmwareInfoPayload, InterfaceInfoPage, OpnsenseApi, OpnsenseConnection, PfRuleId,
        RuleSearchPage, RuleStat, SystemTimePayload,
    };
    use crate::opnsense::types::{
        OpnsenseApiFactory, OpnsenseInterfaceDto, OpnsenseIpsecSessionDto,
        OpnsenseOpenvpnSessionDto, OpnsensePfStatesDto, OpnsenseResourcesDto, OpnsenseRevisionDto,
        OpnsenseTailscaleStatusDto, OpnsenseWireguardStatusDto,
    };
    use crate::secrets::{MemoryStore, SecretStore};

    #[test]
    fn opnsense_read_log_fields_include_timestamp_protocol_and_rule_id_fallback() {
        let mut parser = SseParser::new();
        let frame = b"event: message\ndata: {\"__timestamp__\":\"Oct 4 12:00:00\",\"action\":\"pass\",\"interface\":\"igb0\",\"dir\":\"in\",\"protoname\":\"tcp\",\"src\":\"192.0.2.2\",\"srcport\":\"12345\",\"dst\":\"198.51.100.1\",\"dstport\":\"443\",\"label\":\"\",\"rid\":\"rule-uuid\"}\n\n";
        let entries = parser.feed(frame).expect("SSE frame");
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.timestamp.as_deref(), Some("Oct 4 12:00:00"));
        assert_eq!(entry.proto.as_deref(), Some("tcp"));
        assert_eq!(entry.interface_name.as_deref(), Some("igb0"));
        assert_eq!(entry.label.as_deref(), Some("rule-uuid"));
    }

    // ---- Pure parser tests --------------------------------------------------

    /// One SSE frame delivered in three chunks, split mid-line and mid-UTF-8
    /// (the label's `ü` is cut between its two bytes). Feeding raw bytes —
    /// never `&str` — must reassemble the frame exactly.
    #[test]
    fn parser_reassembles_frame_split_across_chunks() {
        let frame = "data: {\"action\":\"pass\",\"timestamp\":\"2026-10-04T10:00:00Z\",\
                     \"interface_name\":\"lan\",\"dir\":\"in\",\"proto\":\"tcp\",\
                     \"src\":\"10.0.0.5\",\"srcport\":\"51000\",\"dst\":\"192.0.2.1\",\
                     \"dstport\":\"443\",\"label\":\"allow tüb\"}\n\n";
        let bytes = frame.as_bytes();
        // Deliberately awkward split points, including inside "ü".
        let chunks = [&bytes[..17], &bytes[17..113], &bytes[113..]];
        let mut parser = SseParser::new();
        assert!(parser.feed(chunks[0]).expect("chunk 1").is_empty());
        assert!(parser.feed(chunks[1]).expect("chunk 2").is_empty());
        let events = parser.feed(chunks[2]).expect("chunk 3");
        assert_eq!(events.len(), 1);

        let event = &events[0];
        assert_eq!(event.action, "pass");
        assert_eq!(event.timestamp.as_deref(), Some("2026-10-04T10:00:00Z"));
        assert_eq!(event.interface_name.as_deref(), Some("lan"));
        assert_eq!(event.dir.as_deref(), Some("in"));
        assert_eq!(event.proto.as_deref(), Some("tcp"));
        assert_eq!(event.src.as_deref(), Some("10.0.0.5"));
        assert_eq!(event.src_port.as_deref(), Some("51000"));
        assert_eq!(event.dst.as_deref(), Some("192.0.2.1"));
        assert_eq!(event.dst_port.as_deref(), Some("443"));
        assert_eq!(event.label.as_deref(), Some("allow tüb"));
        // The multibyte character survived the mid-byte split.
        assert!(event.label.as_ref().unwrap().contains('ü'));
        // Parse-time metadata is stamped.
        assert!(!event.received_at.is_empty());
    }

    /// A frame whose payload is a JSON array produces one event per object.
    #[test]
    fn parser_emits_one_event_per_object_in_array_frame() {
        let frame = "data: [{\"action\":\"pass\",\"src\":\"10.0.0.1\"},\
                     {\"action\":\"block\",\"src\":\"198.51.100.7\"}]\n\n";
        let mut parser = SseParser::new();
        let events = parser.feed(frame.as_bytes()).expect("frame");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].action, "pass");
        assert_eq!(events[0].src.as_deref(), Some("10.0.0.1"));
        assert_eq!(events[1].action, "block");
        assert_eq!(events[1].src.as_deref(), Some("198.51.100.7"));
    }

    /// Heartbeat (comment-only) frames, comment lines inside a data frame,
    /// and `event:`/`id:`/`retry:` lines produce nothing extra.
    #[test]
    fn parser_drops_heartbeats_comments_and_metadata_lines() {
        let mut parser = SseParser::new();
        let feed = parser
            .feed(
                b": heartbeat\n\n\
                  event: message\n\
                  id: 42\n\
                  retry: 3000\n\
                  : a comment inside the frame\n\
                  data: {\"action\":\"pass\"}\n\
                  \n",
            )
            .expect("feed");
        assert_eq!(feed.len(), 1, "only the data record survives");
        assert_eq!(feed[0].action, "pass");

        // A lone comment frame and a lone metadata frame: silence.
        assert!(parser.feed(b": still alive\n\n").expect("hb").is_empty());
        assert!(parser
            .feed(b"event: ping\nid: 7\n\n")
            .expect("meta")
            .is_empty());
        // CRLF endings behave exactly like LF.
        let crlf = parser
            .feed(b"data: {\"action\":\"block\"}\r\n\r\n")
            .expect("crlf");
        assert_eq!(crlf.len(), 1);
        assert_eq!(crlf[0].action, "block");
    }

    /// Frames larger than the 256 KiB cap are fatal: feed returns an error
    /// instead of buffering unboundedly.
    #[test]
    fn parser_rejects_oversized_frames() {
        let mut parser = SseParser::new();
        // "data: " (6 bytes) plus just-under-cap filler keeps the buffered
        // line below the cap (no terminator seen yet, no events).
        let under = vec![b'x'; MAX_FRAME_BYTES - 8];
        let mut chunk = b"data: ".to_vec();
        chunk.extend_from_slice(&under);
        assert!(parser.feed(&chunk).is_ok());
        // Three more bytes tip the buffered total past the cap.
        let result = parser.feed(&[b'x'; 3]);
        assert_eq!(result, Err(ParseFailure::OversizedFrame));
    }

    /// Malformed JSON drops the frame without poisoning the stream.
    #[test]
    fn parser_drops_malformed_json_and_continues() {
        let mut parser = SseParser::new();
        assert!(parser
            .feed(b"data: {not json}\n\n")
            .expect("malformed frame dropped")
            .is_empty());
        let events = parser
            .feed(b"data: {\"action\":\"pass\"}\n\n")
            .expect("stream continues");
        assert_eq!(events.len(), 1);
    }

    /// Action mapping: lowercase applied; `reject` joins `block` as blocked;
    /// unknown or missing actions become "other" — never "pass".
    #[test]
    fn parser_maps_actions_without_reinterpreting_unknowns() {
        let frame = "data: [{\"action\":\"PASS\"},{\"action\":\"Block\"},\
                     {\"action\":\"reject\"},{\"action\":\"drop\"},{}]\n\n";
        let mut parser = SseParser::new();
        let events = parser.feed(frame.as_bytes()).expect("frame");
        let actions: Vec<&str> = events.iter().map(|event| event.action.as_str()).collect();
        assert_eq!(actions, ["pass", "block", "block", "other", "other"]);
    }

    /// Alternate field spellings map onto the same DTO fields.
    #[test]
    fn parser_accepts_alternate_field_spellings() {
        let frame = "data: {\"action\":\"pass\",\"time\":\"t\",\"interface\":\"wan\",\
                     \"direction\":\"out\",\"protocol\":\"udp\",\"src_addr\":\"1.1.1.1\",\
                     \"src_port\":\"53\",\"dst_addr\":\"2.2.2.2\",\"dst_port\":\"53\",\
                     \"rid\":\"abc123\"}\n\n";
        let mut parser = SseParser::new();
        let events = parser.feed(frame.as_bytes()).expect("frame");
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.timestamp.as_deref(), Some("t"));
        assert_eq!(event.interface_name.as_deref(), Some("wan"));
        assert_eq!(event.dir.as_deref(), Some("out"));
        assert_eq!(event.proto.as_deref(), Some("udp"));
        assert_eq!(event.src.as_deref(), Some("1.1.1.1"));
        assert_eq!(event.src_port.as_deref(), Some("53"));
        assert_eq!(event.dst.as_deref(), Some("2.2.2.2"));
        assert_eq!(event.dst_port.as_deref(), Some("53"));
        assert_eq!(event.label.as_deref(), Some("abc123"));
    }

    // ---- Manager-level tests ------------------------------------------------

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "verkkokyyla-opnsense-logs-{name}-{}-{stamp}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).expect("temp dir");
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Wrap payloads as single-record SSE frames.
    fn sse_frame(payload: &str) -> Vec<u8> {
        format!("data: {payload}\n\n").into_bytes()
    }

    /// A stream that yields the scripted chunks and then stays open forever,
    /// so the reader only exits via explicit stop or error.
    fn scripted_stream(chunks: Vec<Result<Vec<u8>, OpnsenseError>>) -> SseByteStream {
        Box::pin(
            futures_util::stream::iter(
                chunks
                    .into_iter()
                    .map(|chunk| chunk.map(bytes::Bytes::from)),
            )
            .chain(futures_util::stream::pending()),
        )
    }

    /// One-shot fake: `open_log_stream` returns the scripted result; every
    /// other endpoint is off-limits for the log manager.
    struct LogStreamApi {
        open: SyncMutex<Option<Result<SseByteStream, OpnsenseError>>>,
    }

    #[async_trait::async_trait]
    impl OpnsenseApi for LogStreamApi {
        async fn get_system_status(&self) -> Result<(), OpnsenseError> {
            unreachable!("log stream only calls open_log_stream")
        }
        async fn search_rules(&self) -> Result<RuleSearchPage, OpnsenseError> {
            unreachable!("log stream only calls open_log_stream")
        }
        async fn rule_stats(&self) -> Result<Vec<(String, RuleStat)>, OpnsenseError> {
            unreachable!("log stream only calls open_log_stream")
        }
        async fn list_rule_ids(&self) -> Result<Vec<PfRuleId>, OpnsenseError> {
            unreachable!("log stream only calls open_log_stream")
        }
        async fn interfaces_info(&self) -> Result<InterfaceInfoPage, OpnsenseError> {
            unreachable!("log stream only calls open_log_stream")
        }
        async fn interface_statistics(&self) -> Result<Vec<OpnsenseInterfaceDto>, OpnsenseError> {
            unreachable!("log stream only calls open_log_stream")
        }
        async fn pf_states(&self) -> Result<OpnsensePfStatesDto, OpnsenseError> {
            unreachable!("log stream only calls open_log_stream")
        }
        async fn system_resources(&self) -> Result<OpnsenseResourcesDto, OpnsenseError> {
            unreachable!("log stream only calls open_log_stream")
        }
        async fn wireguard_show(&self) -> Result<OpnsenseWireguardStatusDto, OpnsenseError> {
            unreachable!("log stream only calls open_log_stream")
        }
        async fn openvpn_sessions(&self) -> Result<Vec<OpnsenseOpenvpnSessionDto>, OpnsenseError> {
            unreachable!("log stream only calls open_log_stream")
        }
        async fn ipsec_phase1(&self) -> Result<Vec<OpnsenseIpsecSessionDto>, OpnsenseError> {
            unreachable!("log stream only calls open_log_stream")
        }
        async fn tailscale_status(&self) -> Result<OpnsenseTailscaleStatusDto, OpnsenseError> {
            unreachable!("log stream only calls open_log_stream")
        }
        async fn tailscale_service_status(&self) -> Result<Option<bool>, OpnsenseError> {
            unreachable!("log stream only calls open_log_stream")
        }
        async fn system_time(&self) -> Result<SystemTimePayload, OpnsenseError> {
            unreachable!("log stream only calls open_log_stream")
        }
        async fn cpu_usage_sample(&self) -> Result<f64, OpnsenseError> {
            unreachable!("log stream only calls open_log_stream")
        }
        async fn firmware_info(&self) -> Result<FirmwareInfoPayload, OpnsenseError> {
            unreachable!("log stream only calls open_log_stream")
        }
        async fn backup_revisions(&self) -> Result<Vec<OpnsenseRevisionDto>, OpnsenseError> {
            unreachable!("log stream only calls open_log_stream")
        }
        async fn download_revision(&self, _id: &str) -> Result<Vec<u8>, OpnsenseError> {
            unreachable!("log stream only calls open_log_stream")
        }
        async fn open_log_stream(&self) -> Result<SseByteStream, OpnsenseError> {
            self.open.lock().take().expect("one open_log_stream call")
        }
    }

    struct Harness {
        manager: OpnsenseLogStreamManager,
        profile_id: i64,
        event_tx: tokio::sync::mpsc::UnboundedSender<OpnsenseLogEvent>,
        status_tx: tokio::sync::mpsc::UnboundedSender<OpnsenseLogStatusEvent>,
        events: tokio::sync::mpsc::UnboundedReceiver<OpnsenseLogEvent>,
        statuses: tokio::sync::mpsc::UnboundedReceiver<OpnsenseLogStatusEvent>,
        _dir: TempDir,
    }

    /// Temp DB + MemoryStore + injected factory returning the scripted
    /// `open_log_stream` result. Does NOT start the stream — each test
    /// drives start/stop itself.
    async fn harness(name: &str, open: Result<SseByteStream, OpnsenseError>) -> Harness {
        let dir = TempDir::new(name);
        let db = Database::connect(&dir.0.join("logs.db")).await.expect("db");
        let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::new());
        let profile = db
            .create_opnsense_profile(&NewOpnsenseProfile {
                name: "lab".to_owned(),
                host: "192.0.2.10".to_owned(),
                port: 443,
                allow_invalid_certs: false,
                ssh_port: 22,
                created_at: now_rfc3339(),
            })
            .await
            .expect("profile");
        store
            .set(&profile.secret_key, r#"{"key":"k","secret":"s"}"#)
            .await
            .expect("credentials");

        let open = SyncMutex::new(Some(open));
        let factory: OpnsenseApiFactory = Arc::new(move |_conn: OpnsenseConnection| {
            let open = open.lock().take().expect("one factory call");
            Box::pin(async move {
                Ok(Arc::new(LogStreamApi {
                    open: SyncMutex::new(Some(open)),
                }) as Arc<dyn OpnsenseApi>)
            })
        });
        let manager = OpnsenseManager::new(db, store, factory);

        let (event_tx, events) = tokio::sync::mpsc::unbounded_channel();
        let (status_tx, statuses) = tokio::sync::mpsc::unbounded_channel();
        Harness {
            manager: OpnsenseLogStreamManager::new(manager),
            profile_id: profile.id,
            event_tx,
            status_tx,
            events,
            statuses,
            _dir: dir,
        }
    }

    fn event_sink(
        tx: &tokio::sync::mpsc::UnboundedSender<OpnsenseLogEvent>,
    ) -> OpnsenseLogEventSink {
        let tx = tx.clone();
        Arc::new(move |event| tx.send(event).is_ok())
    }

    fn status_sink(
        tx: &tokio::sync::mpsc::UnboundedSender<OpnsenseLogStatusEvent>,
    ) -> OpnsenseLogStatusSink {
        let tx = tx.clone();
        Arc::new(move |status| {
            let _ = tx.send(status);
        })
    }

    async fn next_event(
        receiver: &mut tokio::sync::mpsc::UnboundedReceiver<OpnsenseLogEvent>,
    ) -> OpnsenseLogEvent {
        tokio::time::timeout(std::time::Duration::from_secs(5), receiver.recv())
            .await
            .expect("event within 5s")
            .expect("event channel open")
    }

    async fn next_status(
        receiver: &mut tokio::sync::mpsc::UnboundedReceiver<OpnsenseLogStatusEvent>,
    ) -> OpnsenseLogStatusEvent {
        tokio::time::timeout(std::time::Duration::from_secs(5), receiver.recv())
            .await
            .expect("status within 5s")
            .expect("status channel open")
    }

    #[tokio::test]
    async fn stream_lifecycle_start_events_stop_releases_slot() {
        let stream = scripted_stream(vec![
            Ok(sse_frame(r#"{"action":"pass","src":"10.0.0.1"}"#)),
            Ok(sse_frame(
                r#"[{"action":"block","src":"198.51.100.7"},{"action":"reject"}]"#,
            )),
        ]);
        let mut h = harness("lifecycle", Ok(stream)).await;

        assert!(!h.manager.is_active(h.profile_id).await);

        h.manager
            .start(
                h.profile_id,
                event_sink(&h.event_tx),
                status_sink(&h.status_tx),
            )
            .await
            .expect("start");

        assert_eq!(
            next_status(&mut h.statuses).await,
            OpnsenseLogStatusEvent::Started {
                profile_id: h.profile_id
            }
        );
        assert!(h.manager.is_active(h.profile_id).await);

        // A second start on the same profile is refused without touching
        // the factory (the scripted open result is single-use).
        let err = h
            .manager
            .start(
                h.profile_id,
                event_sink(&h.event_tx),
                status_sink(&h.status_tx),
            )
            .await
            .expect_err("duplicate start refused");
        assert!(matches!(
            err,
            OpnsenseManagerError::AlreadyMonitoring(id) if id == h.profile_id
        ));

        // Events arrive with the profile id stamped by the reader.
        let event = next_event(&mut h.events).await;
        assert_eq!(event.profile_id, h.profile_id);
        assert_eq!(event.action, "pass");
        assert_eq!(event.src.as_deref(), Some("10.0.0.1"));
        let event = next_event(&mut h.events).await;
        assert_eq!(event.action, "block");
        assert_eq!(event.src.as_deref(), Some("198.51.100.7"));
        let event = next_event(&mut h.events).await;
        assert_eq!(event.action, "block", "reject maps to block");

        // Explicit stop: slot released, `Stopped` exactly once.
        h.manager.stop(h.profile_id).await.expect("stop");
        assert!(!h.manager.is_active(h.profile_id).await);
        assert_eq!(
            next_status(&mut h.statuses).await,
            OpnsenseLogStatusEvent::Stopped {
                profile_id: h.profile_id
            }
        );

        // Stopping again is a typed no-active error; activity was balanced
        // (profile deletion guard released).
        let err = h
            .manager
            .stop(h.profile_id)
            .await
            .expect_err("already stopped");
        assert!(matches!(err, OpnsenseManagerError::NoActiveMonitor));
        assert!(!h.manager.activity.lock().await.is_active(h.profile_id));
    }

    #[tokio::test]
    async fn stream_open_forbidden_reports_error_without_fake_entries() {
        let mut h = harness("forbidden", Err(OpnsenseError::Forbidden)).await;
        let result = h
            .manager
            .start(
                h.profile_id,
                event_sink(&h.event_tx),
                status_sink(&h.status_tx),
            )
            .await;
        assert!(
            result.is_err(),
            "a 403 must fail start, never fake an empty log"
        );

        match next_status(&mut h.statuses).await {
            OpnsenseLogStatusEvent::Error {
                profile_id,
                message,
            } => {
                assert_eq!(profile_id, h.profile_id);
                assert!(
                    message.contains("Access denied"),
                    "403 wording must say access denied, got: {message}"
                );
            }
            other => panic!("expected Error status, got {other:?}"),
        }
        // No fake entries, no lingering slot, no activity registration.
        assert!(h.events.try_recv().is_err());
        assert!(!h.manager.is_active(h.profile_id).await);
        assert!(!h.manager.activity.lock().await.is_active(h.profile_id));
    }

    #[tokio::test]
    async fn stream_open_not_found_reports_unavailable() {
        let mut h = harness(
            "notfound",
            Err(OpnsenseError::NotFound("streamLog".to_owned())),
        )
        .await;
        h.manager
            .start(
                h.profile_id,
                event_sink(&h.event_tx),
                status_sink(&h.status_tx),
            )
            .await
            .expect_err("404 must fail start");

        match next_status(&mut h.statuses).await {
            OpnsenseLogStatusEvent::Error { message, .. } => {
                assert!(
                    message.contains("unavailable"),
                    "404 wording must say the stream is unavailable, got: {message}"
                );
            }
            other => panic!("expected Error status, got {other:?}"),
        }
        assert!(h.events.try_recv().is_err());
        assert!(!h.manager.is_active(h.profile_id).await);
    }

    /// An oversized frame mid-stream terminates the reader with an `Error`
    /// status, releases the slot and emits `Stopped` exactly once — the UI
    /// can offer a reconnect.
    #[tokio::test]
    async fn oversized_frame_mid_stream_stops_with_error() {
        let giant = format!("data: {}", "x".repeat(MAX_FRAME_BYTES + 16));
        let stream = scripted_stream(vec![Ok(giant.into_bytes())]);
        let mut h = harness("oversized", Ok(stream)).await;
        h.manager
            .start(
                h.profile_id,
                event_sink(&h.event_tx),
                status_sink(&h.status_tx),
            )
            .await
            .expect("start");
        assert_eq!(
            next_status(&mut h.statuses).await,
            OpnsenseLogStatusEvent::Started {
                profile_id: h.profile_id
            }
        );
        match next_status(&mut h.statuses).await {
            OpnsenseLogStatusEvent::Error {
                profile_id,
                message,
            } => {
                assert_eq!(profile_id, h.profile_id);
                assert!(message.contains("256 KiB"), "got: {message}");
            }
            other => panic!("expected Error status, got {other:?}"),
        }
        assert_eq!(
            next_status(&mut h.statuses).await,
            OpnsenseLogStatusEvent::Stopped {
                profile_id: h.profile_id
            }
        );
        assert!(!h.manager.is_active(h.profile_id).await);
        assert!(!h.manager.activity.lock().await.is_active(h.profile_id));
    }
    #[tokio::test]
    async fn dropped_event_receiver_stops_the_stream() {
        let stream = scripted_stream(vec![Ok(sse_frame(r#"{"action":"pass"}"#))]);
        let mut h = harness("receiver-dropped", Ok(stream)).await;
        h.events.close();
        h.manager
            .start(
                h.profile_id,
                event_sink(&h.event_tx),
                status_sink(&h.status_tx),
            )
            .await
            .expect("start");
        assert!(matches!(
            next_status(&mut h.statuses).await,
            OpnsenseLogStatusEvent::Started { .. }
        ));
        assert!(matches!(
            next_status(&mut h.statuses).await,
            OpnsenseLogStatusEvent::Stopped { .. }
        ));
        assert!(!h.manager.is_active(h.profile_id).await);
        assert!(!h.manager.activity.lock().await.is_active(h.profile_id));
    }
}
