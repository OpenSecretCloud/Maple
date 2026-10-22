//! Save failures are independent of successful StoreChange notifications. A
//! watch snapshot coalesces repeated failures without losing the current latch.

use std::sync::Arc;

use super::{AgentEventDispatcher, AgentServiceEvent, MapleAgentService, emit_agent_event};

pub(super) fn start(service: &MapleAgentService) {
    let Some(store) = service.host.store.as_ref() else {
        return;
    };
    let Ok(executor) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let mut failures = store.subscribe_persistence_failures();
    let sink = Arc::downgrade(&service.host.events.sink);
    let subscribers = service.host.events.subscribers.clone();
    executor.spawn(async move {
        let initial = failures.borrow_and_update().clone();
        if initial.failure.is_some() {
            let Some(sink) = sink.upgrade() else { return };
            emit_agent_event(
                &AgentEventDispatcher {
                    sink,
                    subscribers: subscribers.clone(),
                },
                AgentServiceEvent::PersistenceFailureChanged { status: initial },
            );
        }
        loop {
            if failures.changed().await.is_err() {
                break;
            }
            let status = failures.borrow_and_update().clone();
            let Some(sink) = sink.upgrade() else { break };
            emit_agent_event(
                &AgentEventDispatcher {
                    sink,
                    subscribers: subscribers.clone(),
                },
                AgentServiceEvent::PersistenceFailureChanged { status },
            );
        }
    });
}
