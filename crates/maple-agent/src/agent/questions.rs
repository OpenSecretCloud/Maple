//! Broker for agent-initiated questions (the ask_user tool). The runtime
//! registers a pending question and blocks; the UI answers through the
//! service and the tool call resumes with the user's text.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use tokio::sync::{oneshot, Mutex};

use crate::agent::{emit_agent_event, AgentEventDispatcher, AgentServiceEvent};

/// A question the agent asked the user, routed to the UI.
#[derive(Debug, Clone)]
pub(crate) struct AgentQuestion {
    pub session_id: String,
    pub request_id: String,
    pub question: String,
}

#[derive(Clone)]
pub(crate) struct QuestionBroker {
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>,
    dispatcher: AgentEventDispatcher,
    next_id: Arc<std::sync::atomic::AtomicU64>,
}

/// Process-wide broker: the service initializes it once; tool clients
/// fetch it lazily. A desktop process hosts exactly one agent service.
static GLOBAL_BROKER: OnceLock<QuestionBroker> = OnceLock::new();

pub(crate) fn init_global(dispatcher: AgentEventDispatcher) {
    let _ = GLOBAL_BROKER.set(QuestionBroker::new(dispatcher));
}

pub(crate) fn global() -> Option<QuestionBroker> {
    GLOBAL_BROKER.get().cloned()
}

impl QuestionBroker {
    pub(crate) fn new(dispatcher: AgentEventDispatcher) -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
            dispatcher,
            next_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    /// Ask the user a question and wait for the answer. Emits a service
    /// event the UI renders as a question card.
    pub(crate) async fn ask(&self, session_id: &str, question: String) -> String {
        let request_id = format!(
            "question_{}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis())
                .unwrap_or(0),
            self.next_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(request_id.clone(), tx);
        emit_agent_event(
            &self.dispatcher,
            AgentServiceEvent::Question {
                session_id: session_id.to_string(),
                request_id: request_id.clone(),
                question: question.clone(),
            },
        );
        let answer = rx.await.unwrap_or_default();
        self.pending.lock().await.remove(&request_id);
        answer
    }
}

impl QuestionBroker {
    /// Deliver the user's answer; true when a pending question matched.
    pub(crate) async fn answer(&self, request_id: &str, answer: String) -> bool {
        let sender = self.pending.lock().await.remove(request_id);
        match sender {
            Some(tx) => tx.send(answer).is_ok(),
            None => false,
        }
    }
}
