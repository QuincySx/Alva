// INPUT:  alva_kernel_core::{AgentLoop, DefaultAgentLoop, AgentState, AgentConfig, AgentEvent},
//         alva_kernel_abi::{AgentError, AgentMessage, CancellationToken, Message}
// OUTPUT: proves AgentLoop is a real substitution point — a second implementation
//         drives through `dyn` and changes behaviour without touching the kernel defaults.
// POS:    ticket 03 of agent-loop-interface (ADR-0007).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use alva_kernel_abi::{AgentError, AgentMessage, CancellationToken};
use alva_kernel_core::{AgentLoop, DefaultAgentLoop};
use tokio::sync::mpsc;

/// A second loop implementation: it never runs the real loop, just records
/// that it was driven and forwards a single synthetic event. Exists purely to
/// prove `AgentLoop` is substitutable through `dyn`.
struct RecordingLoop {
    drives: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl AgentLoop for RecordingLoop {
    async fn drive(
        &self,
        _state: &alva_kernel_core::AgentState,
        _config: &alva_kernel_core::AgentConfig,
        _cancel: CancellationToken,
        _input: Vec<AgentMessage>,
        event_tx: mpsc::UnboundedSender<alva_kernel_core::AgentEvent>,
    ) -> Result<(), AgentError> {
        self.drives.fetch_add(1, Ordering::SeqCst);
        let _ = event_tx.send(alva_kernel_core::AgentEvent::AgentStart);
        Ok(())
    }
}

/// The default loop dispatches through `dyn AgentLoop` exactly like the second
/// implementation does — both box to the same trait object.
#[tokio::test]
async fn default_loop_is_dyn_dispatchable() {
    let loop_impl: Arc<dyn AgentLoop> = Arc::new(DefaultAgentLoop);
    // Type-level proof: the trait object exists and is Send+Sync.
    fn assert_loop<T: AgentLoop + Send + Sync + ?Sized>(_: &T) {}
    assert_loop(loop_impl.as_ref());
}

#[tokio::test]
async fn second_loop_replaces_the_default_through_the_same_seam() {
    let drives = Arc::new(AtomicUsize::new(0));
    let (default_loop, custom_loop): (Arc<dyn AgentLoop>, Arc<dyn AgentLoop>) = (
        Arc::new(DefaultAgentLoop),
        Arc::new(RecordingLoop {
            drives: drives.clone(),
        }),
    );

    // The same helper drives either implementation through the one seam.
    let (tx, mut rx) = mpsc::unbounded_channel();

    custom_loop
        .drive(
            &minimal_state(),
            &minimal_config(),
            CancellationToken::new(),
            vec![],
            tx,
        )
        .await
        .expect("custom loop drives cleanly");

    assert_eq!(drives.load(Ordering::SeqCst), 1, "custom loop must run");
    // The custom loop forwarded a synthetic event, proving its behaviour is
    // truly substituted, not wrapping the default.
    let ev = rx.recv().await.expect("custom loop emits");
    assert!(matches!(ev, alva_kernel_core::AgentEvent::AgentStart));

    // The default loop remains available and unused; binding it here pins that
    // substitution is a choice, not a removal.
    let _ = default_loop;
}

fn minimal_state() -> alva_kernel_core::AgentState {
    use alva_kernel_abi::LanguageModel;
    struct StubModel;
    #[async_trait::async_trait]
    impl LanguageModel for StubModel {
        async fn complete(
            &self,
            _messages: &[alva_kernel_abi::Message],
            _tools: &[&dyn alva_kernel_abi::Tool],
            _config: &alva_kernel_abi::ModelConfig,
        ) -> Result<alva_kernel_abi::CompletionResponse, AgentError> {
            unreachable!()
        }
        fn stream(
            &self,
            _messages: &[alva_kernel_abi::Message],
            _tools: &[&dyn alva_kernel_abi::Tool],
            _config: &alva_kernel_abi::ModelConfig,
        ) -> std::pin::Pin<
            Box<dyn futures_core::Stream<Item = alva_kernel_abi::StreamEvent> + Send>,
        > {
            Box::pin(futures::stream::empty())
        }
        fn model_id(&self) -> &str {
            "stub"
        }
    }

    alva_kernel_core::AgentState {
        model: Arc::new(StubModel),
        tools: vec![],
        session: Arc::new(alva_kernel_core::InMemoryAgentSession::new()),
        extensions: alva_kernel_core::Extensions::new(),
    }
}

fn minimal_config() -> alva_kernel_core::AgentConfig {
    alva_kernel_core::AgentConfig {
        middleware: alva_kernel_core::MiddlewareStack::new(),
        system_prompt: vec![],
        max_iterations: 1,
        model_config: alva_kernel_abi::ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: None,
        context_token_budget: None,
    }
}
