//! Apple Foundation Models streaming adapter.
//!
//! The helper reader owns blocking pipe reads on dedicated threads. This
//! routing loop only receives from a bounded channel, so cancellation and the
//! existing overall deadline are checked at least every 50 ms.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::chat::ChatMessage;
use crate::providers::apple_foundation::{
    AppleGenerationRequest, HelperOutputRecord, HelperPoll, HelperPort, HelperProcess,
};

const POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StreamOutcome {
    Done,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // The research run wiring lands after its bounded model port.
pub(crate) struct CollectedTurn {
    pub text: String,
    pub context_size: Option<u32>,
    pub prompt_tokens: Option<u64>,
    pub outcome: StreamOutcome,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TerminalTelemetry {
    context_size: Option<u32>,
    prompt_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AppleChatError {
    OsUnsupported,
    Deadline,
    Remote(String),
    Protocol(String),
    Process(String),
}

pub(crate) fn stream_chat_with(
    helper: &dyn HelperPort,
    messages: &[ChatMessage],
    cancel: Arc<AtomicBool>,
    mut emit_token: impl FnMut(&str),
    deadline: Instant,
    os_supported: bool,
) -> Result<StreamOutcome, AppleChatError> {
    stream_generation_with(
        helper,
        messages,
        cancel,
        &mut emit_token,
        deadline,
        os_supported,
    )
    .map(|(outcome, _)| outcome)
}

#[allow(dead_code)] // The research run wiring lands after its bounded model port.
pub(crate) fn collect_turn_with(
    helper: &dyn HelperPort,
    messages: &[ChatMessage],
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    os_supported: bool,
) -> Result<CollectedTurn, AppleChatError> {
    let mut text = String::new();
    let (outcome, telemetry) = stream_generation_with(
        helper,
        messages,
        cancel,
        &mut |delta| text.push_str(delta),
        deadline,
        os_supported,
    )?;
    Ok(CollectedTurn {
        text,
        context_size: telemetry.and_then(|value| value.context_size),
        prompt_tokens: telemetry.and_then(|value| value.prompt_tokens),
        outcome,
    })
}

fn stream_generation_with(
    helper: &dyn HelperPort,
    messages: &[ChatMessage],
    cancel: Arc<AtomicBool>,
    emit_token: &mut impl FnMut(&str),
    deadline: Instant,
    os_supported: bool,
) -> Result<(StreamOutcome, Option<TerminalTelemetry>), AppleChatError> {
    if !os_supported {
        return Err(AppleChatError::OsUnsupported);
    }
    let request =
        AppleGenerationRequest::from_messages(messages).map_err(AppleChatError::Protocol)?;
    let mut process = helper
        .start_generation(request)
        .map_err(AppleChatError::Process)?;
    // `done` ends generation, not the stdout protocol. Keep receiving until
    // EOF so a queued post-terminal record cannot be hidden by child exit.
    let mut saw_done = false;
    let mut terminal_telemetry = None;

    loop {
        if cancel.load(Ordering::SeqCst) {
            process.kill_and_wait();
            return Ok((StreamOutcome::Cancelled, None));
        }
        if Instant::now() >= deadline {
            process.kill_and_wait();
            return Err(AppleChatError::Deadline);
        }
        let timeout = POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now()));
        match process.recv(timeout) {
            Ok(HelperPoll::Record(HelperOutputRecord::Token(delta))) if !saw_done => {
                emit_token(&delta)
            }
            Ok(HelperPoll::Record(HelperOutputRecord::Done {
                context_size,
                prompt_tokens,
            })) if !saw_done => {
                saw_done = true;
                terminal_telemetry = Some(TerminalTelemetry {
                    context_size,
                    prompt_tokens,
                });
            }
            Ok(HelperPoll::Record(HelperOutputRecord::Error(code))) if !saw_done => {
                process.kill_and_wait();
                return Err(AppleChatError::Remote(code));
            }
            Ok(HelperPoll::Record(_)) => {
                process.kill_and_wait();
                return Err(AppleChatError::Protocol(
                    "Apple helper emitted a record after done".into(),
                ));
            }
            Ok(HelperPoll::Eof) => {
                if saw_done {
                    return wait_for_helper_exit(process.as_mut(), &cancel, deadline)
                        .map(|outcome| (outcome, terminal_telemetry));
                }
                process.kill_and_wait();
                return Err(AppleChatError::Protocol(
                    "Apple helper ended before a terminal record".into(),
                ));
            }
            Ok(HelperPoll::Timeout) => continue,
            Err(error) => {
                process.kill_and_wait();
                return Err(AppleChatError::Process(error));
            }
        }
    }
}

fn wait_for_helper_exit(
    process: &mut dyn HelperProcess,
    cancel: &Arc<AtomicBool>,
    deadline: Instant,
) -> Result<StreamOutcome, AppleChatError> {
    loop {
        if cancel.load(Ordering::SeqCst) {
            process.kill_and_wait();
            return Ok(StreamOutcome::Cancelled);
        }
        if Instant::now() >= deadline {
            process.kill_and_wait();
            return Err(AppleChatError::Deadline);
        }
        match process.try_wait() {
            Ok(Some(true)) => return Ok(StreamOutcome::Done),
            Ok(Some(false)) => {
                let _stderr_was_captured = !process.stderr_tail().is_empty();
                process.kill_and_wait();
                return Err(AppleChatError::Process(
                    "Apple helper exited unsuccessfully".into(),
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                process.kill_and_wait();
                return Err(AppleChatError::Process(error));
            }
        }
    }
}
