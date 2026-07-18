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

    loop {
        if cancel.load(Ordering::SeqCst) {
            process.kill_and_wait();
            return Ok(StreamOutcome::Cancelled);
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
            Ok(HelperPoll::Record(HelperOutputRecord::Done)) if !saw_done => saw_done = true,
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
                    return wait_for_helper_exit(process.as_mut(), &cancel, deadline);
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
