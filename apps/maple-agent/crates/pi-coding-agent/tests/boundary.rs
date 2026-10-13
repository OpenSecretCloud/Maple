//! Pi's boundary drafts: entries extensions add at a turn's end and before a run
//! settles, and the one more request they can ask for.

mod common;

use std::sync::{Arc, Mutex};

use common::{Harness, entry_kinds};
use pi_ai::{Content, Message, content_text};
use pi_coding_agent::extensions::{
    ActivityOutcome, AgentBeforeSettle, BoundaryDraft, BoundaryResult, TurnEnd, extension,
};
use pi_coding_agent::session::EntryKind;
use pi_coding_agent::{AgentSessionEvent, PromptOptions, SessionMessage};
use serde_json::json;

fn note(text: &str) -> BoundaryDraft {
    BoundaryDraft::CustomMessage {
        custom_type: "note".into(),
        content: vec![Content::text(text)],
        display: false,
        details: None,
    }
}

/// The texts of the user and custom messages of request `index`.
fn sent(harness: &Harness, index: usize) -> Vec<String> {
    harness.faux.requests()[index]
        .context
        .messages
        .iter()
        .filter_map(|message| match message {
            Message::User(user) => Some(content_text(&user.content)),
            _ => None,
        })
        .collect()
}

fn errors(session: &pi_coding_agent::AgentSession) -> Arc<Mutex<Vec<String>>> {
    let errors = Arc::new(Mutex::new(Vec::new()));
    let sink = errors.clone();
    session.subscribe(move |event| {
        if let AgentSessionEvent::ExtensionError(report) = event {
            sink.lock().unwrap().push(format!(
                "{}:{}:{}",
                report.extension, report.event, report.error
            ));
        }
    });
    errors
}

#[tokio::test]
async fn turn_end_handlers_add_entries_and_ask_for_another_request() {
    let harness = Harness::new();
    harness.faux.push_text("first answer");
    harness.faux.push_text("second answer");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (first_seen, second_seen) = (seen.clone(), seen.clone());
    let first = extension("first", move |api| {
        let seen = first_seen.clone();
        api.on(move |event: TurnEnd, _ctx| {
            let seen = seen.clone();
            async move {
                seen.lock().unwrap().push(format!(
                    "first:{}:{:?}:{}",
                    event.turn_index,
                    event.outcome,
                    event.entries.len()
                ));
                if event.turn_index > 0 || event.message_entry_id.is_empty() {
                    return Ok(BoundaryResult::default());
                }
                Ok(BoundaryResult {
                    entries: Some(vec![note("keep going")]),
                    continue_run: Some(true),
                })
            }
        });
    });
    let second = extension("second", move |api| {
        let seen = second_seen.clone();
        api.on(move |event: TurnEnd, _ctx| {
            let seen = seen.clone();
            async move {
                // It sees what the handler before asked for, and its preview.
                let previewed = event
                    .context
                    .context_messages
                    .iter()
                    .any(|message| message.text() == "keep going");
                seen.lock().unwrap().push(format!(
                    "second:{}:{}:{previewed}:{}",
                    event.entries.len(),
                    event.continue_run,
                    event.context.can_continue
                ));
                Ok(BoundaryResult::default())
            }
        });
    });
    let session = harness.session_with_extensions(vec![first, second]).await;
    session
        .prompt("hello", PromptOptions::default())
        .await
        .unwrap();

    assert_eq!(
        *seen.lock().unwrap(),
        [
            "first:0:Completed:0",
            "second:1:true:true:true",
            "first:1:Completed:0",
            "second:0:false:true:false",
        ]
    );
    // The note went into the session after the reply, and into the next request.
    assert_eq!(harness.faux.requests().len(), 2);
    let request = &harness.faux.requests()[1].context.messages;
    assert!(request.iter().any(
        |message| matches!(message, Message::User(user) if content_text(&user.content) == "keep going")
    ));
    assert_eq!(
        entry_kinds(&session)
            .into_iter()
            .filter(|kind| kind != "model_change" && kind != "thinking_level_change")
            .collect::<Vec<_>>(),
        [
            "message:system",
            "message:user",
            "message:assistant",
            "custom_message",
            "message:assistant",
        ]
    );
}

#[tokio::test]
async fn a_request_with_nothing_to_answer_is_refused() {
    let harness = Harness::new();
    harness.faux.push_text("answer");
    let eager = extension("eager", |api| {
        api.on(|_event: TurnEnd, _ctx| async {
            Ok(BoundaryResult {
                entries: None,
                continue_run: Some(true),
            })
        });
    });
    let session = harness.session_with_extensions(vec![eager]).await;
    let errors = errors(&session);
    session
        .prompt("hello", PromptOptions::default())
        .await
        .unwrap();
    assert_eq!(harness.faux.requests().len(), 1);
    assert_eq!(
        *errors.lock().unwrap(),
        ["<boundary>:turn_end:turn_end requested continuation without runnable model context"]
    );
}

#[tokio::test]
async fn entries_that_do_not_apply_are_reported_and_dropped() {
    let harness = Harness::new();
    harness.faux.push_text("answer");
    let broken = extension("broken", |api| {
        api.on(|_event: TurnEnd, _ctx| async {
            Ok(BoundaryResult {
                entries: Some(vec![
                    note("never added"),
                    BoundaryDraft::ContextEdit {
                        target_id: "missing".into(),
                        replacement: None,
                    },
                ]),
                continue_run: Some(true),
            })
        });
    });
    let session = harness.session_with_extensions(vec![broken]).await;
    let errors = errors(&session);
    session
        .prompt("hello", PromptOptions::default())
        .await
        .unwrap();
    assert_eq!(harness.faux.requests().len(), 1);
    let errors = errors.lock().unwrap().clone();
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].starts_with("broken:turn_end:Invalid boundary entries: "),
        "{errors:?}"
    );
    assert!(
        !entry_kinds(&session)
            .iter()
            .any(|kind| kind == "custom_message")
    );
}

#[tokio::test]
async fn before_settling_extensions_can_add_a_turn() {
    let harness = Harness::new();
    harness.faux.push_text("answer");
    harness.faux.push_text("after the check");
    let outcomes = Arc::new(Mutex::new(Vec::new()));
    let sink = outcomes.clone();
    let checker = extension("checker", move |api| {
        let sink = sink.clone();
        api.on(move |event: AgentBeforeSettle, _ctx| {
            let sink = sink.clone();
            async move {
                let first = sink.lock().unwrap().is_empty();
                sink.lock().unwrap().push(event.outcome);
                if !first {
                    return Ok(BoundaryResult::default());
                }
                Ok(BoundaryResult {
                    entries: Some(vec![
                        BoundaryDraft::Custom {
                            custom_type: "checked".into(),
                            data: Some(json!({ "ok": true })),
                        },
                        note("check your answer"),
                    ]),
                    continue_run: Some(true),
                })
            }
        });
    });
    let session = harness.session_with_extensions(vec![checker]).await;
    session
        .prompt("hello", PromptOptions::default())
        .await
        .unwrap();
    assert_eq!(
        *outcomes.lock().unwrap(),
        [ActivityOutcome::Completed, ActivityOutcome::Completed]
    );
    assert_eq!(harness.faux.requests().len(), 2);
    assert_eq!(sent(&harness, 1), ["hello", "check your answer"]);
    assert!(
        session.with_session(|tree| tree.entries().iter().any(|entry| matches!(
            &entry.kind,
            EntryKind::Custom { custom_type, .. } if custom_type == "checked"
        )))
    );
    assert!(
        session
            .messages()
            .iter()
            .any(|message| matches!(message, SessionMessage::Llm(Message::Assistant(_)) if message.text() == "after the check"))
    );
}

#[tokio::test]
async fn a_compaction_draft_replaces_what_came_before() {
    let harness = Harness::new();
    harness.faux.push_text("answer");
    let compactor = extension("compactor", |api| {
        api.on(|_event: AgentBeforeSettle, _ctx| async {
            Ok(BoundaryResult {
                entries: Some(vec![BoundaryDraft::Compaction {
                    summary: "We said hello.".into(),
                    first_kept_entry_id: None,
                    details: None,
                    usage: None,
                }]),
                continue_run: None,
            })
        });
    });
    let session = harness.session_with_extensions(vec![compactor]).await;
    session
        .prompt("hello", PromptOptions::default())
        .await
        .unwrap();
    let messages = session.messages();
    assert!(
        !messages
            .iter()
            .any(|message| matches!(message, SessionMessage::Llm(Message::User(_))))
    );
    assert!(messages.iter().any(|message| matches!(
        message,
        SessionMessage::CompactionSummary(summary) if summary.summary == "We said hello."
    )));
    assert!(session.with_session(|tree| tree.entries().iter().any(|entry| matches!(
        &entry.kind,
        EntryKind::Compaction { from_extension: true, first_kept_entry_id, .. } if *first_kept_entry_id == entry.id
    ))));
}
