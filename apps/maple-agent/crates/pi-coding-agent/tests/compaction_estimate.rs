//! Compaction's own token estimate, after Pi's compaction: characters over four.

use pi_ai::faux::FauxProvider;
use pi_ai::{
    AssistantContent, AssistantMessage, Content, Message, SystemMessage, Tool, UserMessage,
};
use pi_coding_agent::SessionMessage;
use pi_coding_agent::compaction::estimate_tokens;
use pi_coding_agent::messages::{BashExecutionMessage, CompactionSummaryMessage};
use serde_json::json;

#[test]
fn compaction_counts_characters_over_four_as_pis_compaction_does() {
    let user = SessionMessage::Llm(Message::User(UserMessage::text("abcdefgh")));
    assert_eq!(estimate_tokens(&user), 2);
    let image = SessionMessage::Llm(Message::User(UserMessage {
        content: vec![Content::image("AAAA", "image/png")],
        timestamp: 0,
    }));
    assert_eq!(estimate_tokens(&image), 1_200);

    // A command counts its text and output, and a summary its own text, without the
    // framing the model reads them in.
    let bash = SessionMessage::BashExecution(BashExecutionMessage {
        command: "make".into(),
        output: "error: x".into(),
        exit_code: Some(2),
        cancelled: false,
        truncated: false,
        full_output_path: None,
        timestamp: 0,
        exclude_from_context: false,
    });
    assert_eq!(estimate_tokens(&bash), 3);
    let summary = SessionMessage::CompactionSummary(CompactionSummaryMessage {
        summary: "did things".into(),
        tokens_before: 0,
        timestamp: 0,
    });
    assert_eq!(estimate_tokens(&summary), 3);

    // A system message counts its prompt, its sections and the tools it adds as JSON.
    let tools = vec![Tool::new("read", "Read", json!({ "type": "object" }))];
    let mut system = SystemMessage {
        content: "base".into(),
        tools_added: tools.clone(),
        ..SystemMessage::default()
    };
    system.sections.insert("cwd".into(), Some("/work".into()));
    let chars = "base".len() + "/work".len() + serde_json::to_string(&tools).unwrap().len();
    assert_eq!(
        estimate_tokens(&SessionMessage::Llm(Message::System(system))),
        chars.div_ceil(4) as u64
    );

    let mut reply = AssistantMessage::empty(&FauxProvider::default_model());
    reply.content = vec![
        AssistantContent::text("done"),
        AssistantContent::tool_call("c1", "read", json!({ "path": "a" })),
    ];
    assert_eq!(
        estimate_tokens(&SessionMessage::Llm(Message::Assistant(reply))),
        5
    );
}
