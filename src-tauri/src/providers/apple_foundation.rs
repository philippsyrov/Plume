//! Bounded bridge to Plume's bundled Apple Foundation Models helper.
//!
//! Rust owns process lifetime and protocol validation. The Swift helper gets
//! only the already assembled chat messages and has no project authority.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::chat::{ChatMessage, ChatRole};

pub const APPLE_PROVIDER_ID: &str = "apple-foundation";
pub const APPLE_MODEL_ID: &str = "system";
pub(crate) const MAX_HELPER_LINE_BYTES: usize = 1024 * 1024;
const MAX_HELPER_REQUEST_BYTES: usize = 1024 * 1024;
const MAX_HELPER_STDERR_BYTES: usize = 32 * 1024;
const MAX_HELPER_DETAIL_BYTES: usize = 512;
const HELPER_QUERY_BUDGET: Duration = Duration::from_secs(2);
const MAX_REPORTED_CONTEXT_TOKENS: u32 = 1_000_000;
const HELPER_CHANNEL_CAPACITY: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppleAvailabilityReason {
    OsUnsupported,
    DeviceIneligible,
    AppleIntelligenceDisabled,
    ModelNotReady,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppleAvailability {
    pub available: bool,
    pub reason: Option<AppleAvailabilityReason>,
    /// Helper text is bounded and control-character-free before it reaches the
    /// IPC/catalog surface. Stderr never crosses this boundary.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // The research run wiring lands after its bounded model port.
pub(crate) struct AppleCapabilities {
    pub context_tokens: u32,
    pub exact_token_count: bool,
}

impl AppleAvailability {
    pub fn unavailable(reason: AppleAvailabilityReason) -> Self {
        Self {
            available: false,
            reason: Some(reason),
            detail: None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct HelperExit {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub success: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HelperOutputRecord {
    Token(String),
    Done {
        context_size: Option<u32>,
        prompt_tokens: Option<u64>,
    },
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HelperPoll {
    Record(HelperOutputRecord),
    Eof,
    Timeout,
}

/// Small process seam shared by availability and chat tests. The real port is
/// the only implementation that resolves and spawns the bundled executable.
pub(crate) trait HelperPort: Send + Sync {
    fn availability(&self) -> Result<HelperExit, String>;
    #[allow(dead_code)]
    fn capabilities(&self) -> Result<HelperExit, String>;
    fn start_generation(
        &self,
        request: AppleGenerationRequest,
    ) -> Result<Box<dyn HelperProcess>, String>;
}

/// A live helper process. `recv` is timeout-bounded so the routing loop never
/// waits for a hung stdout read before it can notice cancellation/deadline.
pub(crate) trait HelperProcess: Send {
    fn recv(&mut self, timeout: Duration) -> Result<HelperPoll, String>;
    fn try_wait(&mut self) -> Result<Option<bool>, String>;
    fn kill_and_wait(&mut self);
    fn stderr_tail(&self) -> String;
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppleGenerationRequest {
    request_id: String,
    messages: Vec<AppleGenerationMessage>,
    max_output_tokens: u32,
}

#[derive(Debug, Clone, Serialize)]
struct AppleGenerationMessage {
    role: AppleGenerationRole,
    content: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum AppleGenerationRole {
    System,
    User,
    Assistant,
}

impl AppleGenerationRequest {
    pub(crate) fn from_messages(messages: &[ChatMessage]) -> Result<Self, String> {
        if messages.len() > 128 {
            return Err("Apple helper request has too many messages".into());
        }
        let mut encoded_messages = Vec::with_capacity(messages.len());
        for message in messages {
            if message.content.len() > 256 * 1024 {
                return Err("Apple helper request contains an oversized message".into());
            }
            let role = match message.role {
                ChatRole::System => AppleGenerationRole::System,
                ChatRole::User => AppleGenerationRole::User,
                ChatRole::Assistant => AppleGenerationRole::Assistant,
                ChatRole::Tool => {
                    return Err("Apple helper request contains an unsupported role".into())
                }
            };
            encoded_messages.push(AppleGenerationMessage {
                role,
                content: message.content.clone(),
            });
        }
        let request = Self {
            request_id: "plume-chat".into(),
            messages: encoded_messages,
            max_output_tokens: 1024,
        };
        let bytes = serde_json::to_vec(&request)
            .map_err(|error| format!("could not encode Apple helper request: {error}"))?;
        if bytes.len() > MAX_HELPER_REQUEST_BYTES {
            return Err("Apple helper request exceeds the byte cap".into());
        }
        Ok(request)
    }
}

/// Resolve the helper only from the Tauri resource directory. There is no PATH
/// or developer-environment fallback, so release cannot run an arbitrary tool.
#[derive(Debug, Clone)]
pub(crate) struct NativeHelperPort {
    program: PathBuf,
}

impl NativeHelperPort {
    pub(crate) fn from_resource_dir(resource_dir: &Path) -> Self {
        Self {
            program: resource_dir.join("apple-model").join("plume-apple-model"),
        }
    }
}

impl HelperPort for NativeHelperPort {
    fn availability(&self) -> Result<HelperExit, String> {
        run_bounded_query(&self.program, "availability")
    }

    fn capabilities(&self) -> Result<HelperExit, String> {
        run_bounded_query(&self.program, "capabilities")
    }

    fn start_generation(
        &self,
        request: AppleGenerationRequest,
    ) -> Result<Box<dyn HelperProcess>, String> {
        let request_bytes = serde_json::to_vec(&request)
            .map_err(|error| format!("could not encode Apple helper request: {error}"))?;
        if request_bytes.len() > MAX_HELPER_REQUEST_BYTES {
            return Err("Apple helper request exceeds the byte cap".into());
        }
        let mut child = Command::new(&self.program)
            .arg("generate")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("could not start bundled Apple helper: {error}"))?;
        let stdin_pipe = child.stdin.take();
        let stdin = take_pipe_or_reap(&mut child, stdin_pipe, "stdin")?;
        let stdout_pipe = child.stdout.take();
        let stdout = take_pipe_or_reap(&mut child, stdout_pipe, "stdout")?;
        let stderr_pipe = child.stderr.take();
        let helper_stderr = take_pipe_or_reap(&mut child, stderr_pipe, "stderr")?;

        let (sender, receiver) = mpsc::sync_channel(HELPER_CHANNEL_CAPACITY);
        let writer_sender = sender.clone();
        std::thread::spawn(move || {
            let mut stdin = stdin;
            if let Err(error) = stdin.write_all(&request_bytes) {
                let _ =
                    writer_sender.send(Err(format!("Apple helper request write failed: {error}")));
            }
            // Drop stdin even on write failure so a well-behaved helper can
            // terminate instead of waiting forever for an EOF it will not get.
        });
        let reader_sender = sender.clone();
        std::thread::spawn(move || read_generation_stdout(stdout, reader_sender));
        drop(sender);

        let stderr = Arc::new(Mutex::new(Vec::new()));
        let stderr_for_thread = stderr.clone();
        std::thread::spawn(move || drain_stderr(helper_stderr, stderr_for_thread));

        Ok(Box::new(NativeGenerationProcess {
            child: Mutex::new(child),
            receiver,
            stderr,
            reaped: AtomicBool::new(false),
        }))
    }
}

fn take_pipe_or_reap<T>(child: &mut Child, pipe: Option<T>, name: &str) -> Result<T, String> {
    pipe.ok_or_else(|| {
        let _ = child.kill();
        let _ = child.wait();
        format!("bundled Apple helper did not expose {name}")
    })
}

pub(crate) fn platform_supports_apple_models() -> bool {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("/usr/bin/sw_vers")
            .arg("-productVersion")
            .output();
        output
            .ok()
            .filter(|value| value.status.success())
            .and_then(|value| String::from_utf8(value.stdout).ok())
            .and_then(|version| version.trim().split('.').next()?.parse::<u32>().ok())
            .is_some_and(|major| major >= 26)
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

pub(crate) fn availability_with(port: &dyn HelperPort, os_supported: bool) -> AppleAvailability {
    if !os_supported {
        return AppleAvailability::unavailable(AppleAvailabilityReason::OsUnsupported);
    }
    let Ok(exit) = port.availability() else {
        return AppleAvailability::unavailable(AppleAvailabilityReason::Failed);
    };
    // Stderr is deliberately consumed only to keep the child pipe bounded and
    // drainable. It is never surfaced through availability/catalog IPC.
    let _bounded_stderr_bytes = exit.stderr.len();
    if !exit.success {
        return AppleAvailability::unavailable(AppleAvailabilityReason::Failed);
    }
    parse_availability_line(&exit.stdout)
        .unwrap_or_else(|_| AppleAvailability::unavailable(AppleAvailabilityReason::Failed))
}

#[allow(dead_code)] // The research run wiring lands after its bounded model port.
pub(crate) fn capabilities_with(port: &dyn HelperPort) -> Result<AppleCapabilities, String> {
    let exit = port.capabilities()?;
    let _bounded_stderr_bytes = exit.stderr.len();
    if !exit.success {
        return Err("Apple helper capabilities query failed".into());
    }
    parse_capabilities_line(&exit.stdout)
}

pub(crate) fn parse_availability_line(bytes: &[u8]) -> Result<AppleAvailability, String> {
    let line = one_json_line(bytes)?;
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct WireAvailability {
        available: bool,
        reason: Option<WireReason>,
        detail: Option<String>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "kebab-case")]
    enum WireReason {
        OsUnsupported,
        DeviceIneligible,
        AppleIntelligenceDisabled,
        ModelNotReady,
        Failed,
    }
    let wire: WireAvailability = serde_json::from_slice(line)
        .map_err(|error| format!("Apple availability response did not parse: {error}"))?;
    let reason = wire.reason.map(|reason| match reason {
        WireReason::OsUnsupported => AppleAvailabilityReason::OsUnsupported,
        WireReason::DeviceIneligible => AppleAvailabilityReason::DeviceIneligible,
        WireReason::AppleIntelligenceDisabled => AppleAvailabilityReason::AppleIntelligenceDisabled,
        WireReason::ModelNotReady => AppleAvailabilityReason::ModelNotReady,
        WireReason::Failed => AppleAvailabilityReason::Failed,
    });
    if wire.available != reason.is_none() {
        return Err("Apple availability response has inconsistent available/reason fields".into());
    }
    Ok(AppleAvailability {
        available: wire.available,
        reason,
        detail: wire.detail.and_then(safe_detail),
    })
}

#[allow(dead_code)] // The research run wiring lands after its bounded model port.
pub(crate) fn parse_capabilities_line(bytes: &[u8]) -> Result<AppleCapabilities, String> {
    let line = one_json_line(bytes)?;
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct WireCapabilities {
        context_size: u32,
        exact_token_count_available: bool,
    }
    let wire: WireCapabilities = serde_json::from_slice(line)
        .map_err(|error| format!("Apple capabilities response did not parse: {error}"))?;
    if wire.context_size == 0 || wire.context_size > MAX_REPORTED_CONTEXT_TOKENS {
        return Err("Apple capabilities response has an invalid context size".into());
    }
    Ok(AppleCapabilities {
        context_tokens: wire.context_size,
        exact_token_count: wire.exact_token_count_available,
    })
}

fn safe_detail(detail: String) -> Option<String> {
    if detail.is_empty()
        || detail.len() > MAX_HELPER_DETAIL_BYTES
        || detail.chars().any(char::is_control)
    {
        None
    } else {
        Some(detail)
    }
}

fn run_bounded_query(program: &Path, mode: &'static str) -> Result<HelperExit, String> {
    let mut child = Command::new(program)
        .arg(mode)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not start bundled Apple helper: {error}"))?;
    let stdout_pipe = child.stdout.take();
    let stdout = take_pipe_or_reap(&mut child, stdout_pipe, "stdout")?;
    let stderr_pipe = child.stderr.take();
    let stderr = take_pipe_or_reap(&mut child, stderr_pipe, "stderr")?;
    let (stdout_sender, stdout_receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = stdout_sender.send(read_exactly_one_line(stdout));
    });
    let (stderr_sender, stderr_receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = stderr_sender.send(read_stderr_bounded(stderr));
    });

    let deadline = Instant::now() + HELPER_QUERY_BUDGET;
    let success = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Apple helper {mode} query timed out"));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("could not inspect Apple helper exit: {error}"));
            }
        }
    };
    let remaining = deadline.saturating_duration_since(Instant::now());
    let stdout = stdout_receiver
        .recv_timeout(remaining)
        .map_err(|_| format!("Apple helper {mode} stdout did not close"))??;
    let stderr = stderr_receiver
        .recv_timeout(remaining)
        .map_err(|_| format!("Apple helper {mode} stderr did not close"))?;
    Ok(HelperExit {
        stdout,
        stderr,
        success,
    })
}

struct NativeGenerationProcess {
    child: Mutex<Child>,
    receiver: Receiver<Result<HelperOutputRecord, String>>,
    stderr: Arc<Mutex<Vec<u8>>>,
    reaped: AtomicBool,
}

impl HelperProcess for NativeGenerationProcess {
    fn recv(&mut self, timeout: Duration) -> Result<HelperPoll, String> {
        match self.receiver.recv_timeout(timeout) {
            Ok(Ok(record)) => Ok(HelperPoll::Record(record)),
            Ok(Err(error)) => Err(error),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(HelperPoll::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => Ok(HelperPoll::Eof),
        }
    }

    fn try_wait(&mut self) -> Result<Option<bool>, String> {
        let mut child = self
            .child
            .lock()
            .map_err(|_| "Apple helper child lock poisoned")?;
        child
            .try_wait()
            .map(|status| status.map(|status| status.success()))
            .map_err(|error| format!("could not inspect Apple helper exit: {error}"))
    }

    fn kill_and_wait(&mut self) {
        if self.reaped.swap(true, Ordering::SeqCst) {
            return;
        }
        let Ok(mut child) = self.child.lock() else {
            return;
        };
        if child.try_wait().ok().flatten().is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
    }

    fn stderr_tail(&self) -> String {
        let Ok(stderr) = self.stderr.lock() else {
            return String::new();
        };
        String::from_utf8_lossy(&stderr).into_owned()
    }
}

impl Drop for NativeGenerationProcess {
    fn drop(&mut self) {
        self.kill_and_wait();
    }
}

fn read_generation_stdout(
    stdout: std::process::ChildStdout,
    sender: mpsc::SyncSender<Result<HelperOutputRecord, String>>,
) {
    let mut reader = BufReader::new(stdout);
    loop {
        let line = match read_bounded_line(&mut reader) {
            Ok(Some(line)) => line,
            Ok(None) => return,
            Err(error) => {
                let _ = sender.send(Err(error));
                return;
            }
        };
        let record = parse_generation_record(&line);
        if sender.send(record).is_err() {
            // Consumer loss means the routing task is terminal. Returning
            // closes stdout, which gives the helper SIGPIPE if it keeps writing.
            return;
        }
    }
}

pub(crate) fn parse_generation_record(line: &[u8]) -> Result<HelperOutputRecord, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct WireRecord {
        kind: String,
        delta: Option<String>,
        error: Option<String>,
        context_size: Option<u32>,
        prompt_tokens: Option<u64>,
    }
    let wire: WireRecord = serde_json::from_slice(line)
        .map_err(|error| format!("Apple helper stream record did not parse: {error}"))?;
    match wire.kind.as_str() {
        "token"
            if wire.error.is_none()
                && wire.context_size.is_none()
                && wire.prompt_tokens.is_none() =>
        {
            wire.delta
                .filter(|delta| !delta.is_empty())
                .map(HelperOutputRecord::Token)
                .ok_or_else(|| "Apple helper token record has no delta".into())
        }
        "done" if wire.delta.is_none() && wire.error.is_none() => {
            if wire.context_size == Some(0)
                || wire
                    .context_size
                    .is_some_and(|value| value > MAX_REPORTED_CONTEXT_TOKENS)
            {
                return Err("Apple helper done record has an invalid context size".into());
            }
            Ok(HelperOutputRecord::Done {
                context_size: wire.context_size,
                prompt_tokens: wire.prompt_tokens,
            })
        }
        "error"
            if wire.delta.is_none()
                && wire.context_size.is_none()
                && wire.prompt_tokens.is_none() =>
        {
            wire.error
                .filter(|code| !code.is_empty() && code.len() <= 256)
                .map(HelperOutputRecord::Error)
                .ok_or_else(|| "Apple helper error record has no bounded code".into())
        }
        _ => Err("Apple helper stream record has an invalid shape".into()),
    }
}

fn read_exactly_one_line(stdout: std::process::ChildStdout) -> Result<Vec<u8>, String> {
    let mut reader = BufReader::new(stdout);
    let Some(line) = read_bounded_line(&mut reader)? else {
        return Err("Apple helper availability response is empty".into());
    };
    if read_bounded_line(&mut reader)?.is_some() {
        return Err("Apple helper availability response has more than one line".into());
    }
    Ok(line)
}

fn read_bounded_line<R: BufRead>(reader: &mut R) -> Result<Option<Vec<u8>>, String> {
    let mut line = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .map_err(|error| format!("could not read Apple helper stdout: {error}"))?;
        if available.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err("Apple helper stdout ended without a line break".into())
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map(|index| index + 1).unwrap_or(available.len());
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        let content_len = if newline.is_some() {
            line.len().saturating_sub(1)
        } else {
            line.len()
        };
        if content_len > MAX_HELPER_LINE_BYTES {
            return Err(format!(
                "Apple helper stdout line exceeds {MAX_HELPER_LINE_BYTES} bytes"
            ));
        }
        if newline.is_some() {
            return Ok(Some(line));
        }
    }
}

fn one_json_line(bytes: &[u8]) -> Result<&[u8], String> {
    let Some(line) = bytes.strip_suffix(b"\n") else {
        return Err("Apple helper response must end with exactly one newline".into());
    };
    if line.contains(&b'\n') {
        return Err("Apple helper response contains more than one line".into());
    }
    if line.len() > MAX_HELPER_LINE_BYTES {
        return Err("Apple helper response exceeds the line cap".into());
    }
    Ok(line)
}

fn read_stderr_bounded(stderr: std::process::ChildStderr) -> Vec<u8> {
    let captured = Arc::new(Mutex::new(Vec::new()));
    drain_stderr(stderr, captured.clone());
    captured
        .lock()
        .map(|value| value.clone())
        .unwrap_or_default()
}

fn drain_stderr<R: Read>(mut stderr: R, target: Arc<Mutex<Vec<u8>>>) {
    let mut chunk = [0_u8; 4096];
    loop {
        let Ok(read) = stderr.read(&mut chunk) else {
            return;
        };
        if read == 0 {
            return;
        }
        let Ok(mut captured) = target.lock() else {
            return;
        };
        append_stderr_bounded(&mut captured, &chunk[..read]);
    }
}

pub(crate) fn append_stderr_bounded(target: &mut Vec<u8>, bytes: &[u8]) {
    let remaining = MAX_HELPER_STDERR_BYTES.saturating_sub(target.len());
    target.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
}
