use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::apple_foundation::{stream_chat_with, AppleChatError, StreamOutcome};
use crate::chat::{ChatMessage, ChatRole};
use crate::providers::apple_foundation::{
    parse_generation_record, AppleGenerationRequest, HelperExit, HelperOutputRecord, HelperPoll,
    HelperPort, HelperProcess,
};

#[derive(Clone)]
struct FakeHelper {
    process: Arc<Mutex<FakeProcess>>,
    starts: Arc<AtomicUsize>,
}

impl FakeHelper {
    fn records(records: impl IntoIterator<Item = HelperOutputRecord>) -> Self {
        Self {
            process: Arc::new(Mutex::new(FakeProcess {
                records: records.into_iter().map(Ok).collect(),
                hang: false,
                exited: false,
                killed: false,
            })),
            starts: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn hang_after_token(token: &str) -> Self {
        Self {
            process: Arc::new(Mutex::new(FakeProcess {
                records: VecDeque::from([Ok(HelperOutputRecord::Token(token.into()))]),
                hang: true,
                exited: false,
                killed: false,
            })),
            starts: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn killed(&self) -> bool {
        self.process.lock().expect("fake lock").killed
    }

    fn starts(&self) -> usize {
        self.starts.load(Ordering::SeqCst)
    }
}

impl HelperPort for FakeHelper {
    fn availability(&self) -> Result<HelperExit, String> {
        panic!("chat tests must not ask availability")
    }

    fn start_generation(
        &self,
        _request: AppleGenerationRequest,
    ) -> Result<Box<dyn HelperProcess>, String> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(FakeProcessHandle(self.process.clone())))
    }
}

struct FakeProcess {
    records: VecDeque<Result<HelperOutputRecord, String>>,
    hang: bool,
    exited: bool,
    killed: bool,
}

struct FakeProcessHandle(Arc<Mutex<FakeProcess>>);

impl HelperProcess for FakeProcessHandle {
    fn recv(&mut self, _timeout: Duration) -> Result<HelperPoll, String> {
        let mut process = self.0.lock().expect("fake lock");
        if let Some(record) = process.records.pop_front() {
            return record.map(HelperPoll::Record);
        }
        if process.hang {
            Ok(HelperPoll::Timeout)
        } else {
            Ok(HelperPoll::Eof)
        }
    }

    fn try_wait(&mut self) -> Result<Option<bool>, String> {
        let mut process = self.0.lock().expect("fake lock");
        if process.hang && !process.killed {
            Ok(None)
        } else {
            process.exited = true;
            Ok(Some(true))
        }
    }

    fn kill_and_wait(&mut self) {
        let mut process = self.0.lock().expect("fake lock");
        process.killed = true;
        process.exited = true;
    }

    fn stderr_tail(&self) -> String {
        String::new()
    }
}

fn messages() -> Vec<ChatMessage> {
    vec![ChatMessage {
        role: ChatRole::User,
        content: "hello".into(),
    }]
}

#[test]
fn apple_stream_forwards_tokens_and_emits_one_stop() {
    let helper = FakeHelper::records([
        HelperOutputRecord::Token("A".into()),
        HelperOutputRecord::Token("B".into()),
        HelperOutputRecord::Done,
    ]);
    let mut deltas = Vec::new();
    let outcome = stream_chat_with(
        &helper,
        &messages(),
        Arc::new(AtomicBool::new(false)),
        |delta| deltas.push(delta.to_string()),
        Instant::now() + Duration::from_secs(1),
        true,
    )
    .expect("stream must finish");
    assert_eq!(deltas, ["A", "B"]);
    assert_eq!(outcome, StreamOutcome::Done);
}

#[test]
fn token_after_done_is_a_protocol_error_and_reaps_the_helper() {
    let helper = FakeHelper::records([
        HelperOutputRecord::Done,
        HelperOutputRecord::Token("late".into()),
    ]);
    let err = stream_chat_with(
        &helper,
        &messages(),
        Arc::new(AtomicBool::new(false)),
        |_| {},
        Instant::now() + Duration::from_secs(1),
        true,
    )
    .expect_err("a token after done must reject the helper stream");
    assert!(matches!(err, AppleChatError::Protocol(_)));
    assert!(helper.killed());
}

#[test]
fn duplicate_done_is_a_protocol_error_and_reaps_the_helper() {
    let helper = FakeHelper::records([HelperOutputRecord::Done, HelperOutputRecord::Done]);
    let err = stream_chat_with(
        &helper,
        &messages(),
        Arc::new(AtomicBool::new(false)),
        |_| {},
        Instant::now() + Duration::from_secs(1),
        true,
    )
    .expect_err("a second done must reject the helper stream");
    assert!(matches!(err, AppleChatError::Protocol(_)));
    assert!(helper.killed());
}

#[test]
fn post_done_burst_beyond_channel_capacity_is_a_protocol_error_not_a_deadline() {
    let helper = FakeHelper::records(
        std::iter::once(HelperOutputRecord::Done)
            .chain(std::iter::repeat_with(|| HelperOutputRecord::Token("late".into())).take(65)),
    );
    let err = stream_chat_with(
        &helper,
        &messages(),
        Arc::new(AtomicBool::new(false)),
        |_| {},
        Instant::now() + Duration::from_millis(100),
        true,
    )
    .expect_err("a post-done burst must not become a deadline wait");
    assert!(matches!(err, AppleChatError::Protocol(_)));
    assert!(helper.killed());
}

#[test]
fn cancelled_apple_stream_kills_helper_and_finishes_cancelled() {
    let helper = FakeHelper::hang_after_token("APP");
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_for_thread = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        cancel_for_thread.store(true, Ordering::SeqCst);
    });
    let outcome = stream_chat_with(
        &helper,
        &messages(),
        cancel,
        |_| {},
        Instant::now() + Duration::from_secs(1),
        true,
    )
    .expect("cancel is an ordinary terminal outcome");
    assert_eq!(outcome, StreamOutcome::Cancelled);
    assert!(helper.killed());
}

#[test]
fn deadline_kills_hung_apple_helper() {
    let helper = FakeHelper::hang_after_token("APP");
    let err = stream_chat_with(
        &helper,
        &messages(),
        Arc::new(AtomicBool::new(false)),
        |_| {},
        Instant::now() + Duration::from_millis(20),
        true,
    )
    .expect_err("deadline must terminate helper");
    assert!(matches!(err, AppleChatError::Deadline));
    assert!(helper.killed());
}

#[test]
fn helper_error_and_eof_are_terminal_errors_without_fallback() {
    let helper = FakeHelper::records([HelperOutputRecord::Error("generation-failed".into())]);
    let err = stream_chat_with(
        &helper,
        &messages(),
        Arc::new(AtomicBool::new(false)),
        |_| {},
        Instant::now() + Duration::from_secs(1),
        true,
    )
    .expect_err("helper error stays on Apple route");
    assert!(matches!(err, AppleChatError::Remote(_)));
    assert!(helper.killed());
}

#[test]
fn unsupported_apple_os_never_starts_generation() {
    let helper = FakeHelper::records([HelperOutputRecord::Done]);
    let err = stream_chat_with(
        &helper,
        &messages(),
        Arc::new(AtomicBool::new(false)),
        |_| {},
        Instant::now() + Duration::from_secs(1),
        false,
    )
    .expect_err("unsupported host must reject before helper spawn");
    assert!(matches!(err, AppleChatError::OsUnsupported));
    assert_eq!(helper.starts(), 0);
}

#[test]
fn malformed_helper_record_is_rejected_before_any_terminal_mapping() {
    assert!(parse_generation_record(b"not json").is_err());
    assert!(parse_generation_record(b"{\"kind\":\"token\"}").is_err());
    assert!(
        parse_generation_record(b"{\"kind\":\"token\",\"delta\":\"x\",\"error\":\"bad\"}").is_err()
    );
    assert!(parse_generation_record(b"{\"kind\":\"done\",\"delta\":\"late\"}").is_err());
    assert!(
        parse_generation_record(b"{\"kind\":\"error\",\"delta\":\"late\",\"error\":\"bad\"}")
            .is_err()
    );
}
