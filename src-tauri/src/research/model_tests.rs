use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::model::{
    estimate_tokens_conservatively, select_model, AppleResearchModel, ModelCapabilities,
    ModelFinish, ResearchModelPort, ResearchModelSelectionError,
};
use crate::chat::{ChatMessage, ChatRole};
use crate::providers::apple_foundation::{
    AppleGenerationRequest, HelperExit, HelperOutputRecord, HelperPoll, HelperPort, HelperProcess,
};

#[derive(Clone)]
struct FakeAppleHelper {
    records: Arc<Mutex<VecDeque<HelperOutputRecord>>>,
}

impl FakeAppleHelper {
    fn successful() -> Self {
        Self {
            records: Arc::new(Mutex::new(VecDeque::from([
                HelperOutputRecord::Token("answer".into()),
                HelperOutputRecord::Done {
                    context_size: Some(4096),
                    prompt_tokens: Some(11),
                },
            ]))),
        }
    }
}

impl HelperPort for FakeAppleHelper {
    fn availability(&self) -> Result<HelperExit, String> {
        panic!("research model tests do not query availability")
    }

    fn capabilities(&self) -> Result<HelperExit, String> {
        Ok(HelperExit {
            stdout: b"{\"contextSize\":4096,\"exactTokenCountAvailable\":true}\n".to_vec(),
            stderr: Vec::new(),
            success: true,
        })
    }

    fn start_generation(
        &self,
        _request: AppleGenerationRequest,
    ) -> Result<Box<dyn HelperProcess>, String> {
        Ok(Box::new(FakeAppleProcess(self.records.clone())))
    }
}

struct FakeAppleProcess(Arc<Mutex<VecDeque<HelperOutputRecord>>>);

impl HelperProcess for FakeAppleProcess {
    fn recv(&mut self, _timeout: Duration) -> Result<HelperPoll, String> {
        Ok(self
            .0
            .lock()
            .expect("fake records")
            .pop_front()
            .map(HelperPoll::Record)
            .unwrap_or(HelperPoll::Eof))
    }

    fn try_wait(&mut self) -> Result<Option<bool>, String> {
        Ok(Some(true))
    }

    fn kill_and_wait(&mut self) {}

    fn stderr_tail(&self) -> String {
        String::new()
    }
}

fn messages() -> Vec<ChatMessage> {
    vec![ChatMessage {
        role: ChatRole::User,
        content: "question".into(),
    }]
}

#[test]
fn conservative_counter_never_underestimates_its_documented_byte_ratio() {
    assert_eq!(estimate_tokens_conservatively(""), 0);
    assert_eq!(estimate_tokens_conservatively("a"), 1);
    assert_eq!(estimate_tokens_conservatively("abc"), 2);
    assert_eq!(estimate_tokens_conservatively("🪶"), 2);
}

#[test]
fn apple_model_maps_capabilities_and_terminal_telemetry() {
    let helper = FakeAppleHelper::successful();
    let model = AppleResearchModel::new(&helper, true);
    assert_eq!(
        model.capabilities().expect("capabilities"),
        ModelCapabilities {
            context_tokens: 4096,
            exact_token_count: true,
        }
    );
    let turn = model
        .complete(
            &messages(),
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        )
        .expect("turn");
    assert_eq!(turn.text, "answer");
    assert_eq!(turn.prompt_tokens, Some(11));
    assert_eq!(turn.output_tokens, Some(3));
    assert_eq!(turn.finish, ModelFinish::Stop);
}

#[test]
fn selection_requires_the_exact_provider_model_handle_shape() {
    let helper = FakeAppleHelper::successful();
    assert!(select_model("apple-foundation", "system", None, Some(&helper), true).is_ok());
    assert!(matches!(
        select_model(
            "apple-foundation",
            "system",
            Some("unexpected"),
            Some(&helper),
            true
        ),
        Err(ResearchModelSelectionError::UnexpectedHandle)
    ));
    assert!(matches!(
        select_model("mlx-lm", "qwen-coder-1.5b-mlx-4bit", None, None, true),
        Err(ResearchModelSelectionError::MissingHandle)
    ));
    assert!(matches!(
        select_model("unknown", "model", None, None, true),
        Err(ResearchModelSelectionError::UnsupportedModel)
    ));
}
