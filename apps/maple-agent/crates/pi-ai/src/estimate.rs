//! Token estimates for fitting a request into a model's context window, after Pi's: 3.5
//! characters per token, a fixed size for each image, and tool declarations counted as
//! the JSON they are sent as.

use serde::Serialize;

use crate::transcript::system_message_text;
use crate::types::{AssistantContent, Content, Context, Message, Model, Timestamp, Tool};

/// What an image is assumed to cost, in characters.
const IMAGE_CHARS: usize = 4_800;

/// `chars` at 3.5 characters per token, rounded up.
fn tokens_for_chars(chars: usize) -> u64 {
    (chars as u64 * 2).div_ceil(7)
}

pub fn estimate_text_tokens(text: &str) -> u64 {
    tokens_for_chars(text.chars().count())
}

fn content_chars(content: &[Content]) -> usize {
    content
        .iter()
        .map(|block| match block {
            Content::Text(text) => text.text.chars().count(),
            Content::Image(_) => IMAGE_CHARS,
        })
        .sum()
}

fn json_tokens(value: &impl Serialize) -> u64 {
    estimate_text_tokens(&serde_json::to_string(value).unwrap_or_default())
}

pub fn estimate_tool_tokens(tools: &[Tool]) -> u64 {
    if tools.is_empty() {
        0
    } else {
        json_tokens(&tools)
    }
}

pub fn estimate_message_tokens(message: &Message) -> u64 {
    let chars = match message {
        Message::System(system) => {
            let removed = if system.tools_removed.is_empty() {
                0
            } else {
                json_tokens(&system.tools_removed)
            };
            return estimate_text_tokens(&system_message_text(system))
                + estimate_tool_tokens(&system.tools_added)
                + removed;
        }
        Message::User(user) => content_chars(&user.content),
        Message::ToolResult(result) => content_chars(&result.content),
        Message::Assistant(assistant) => assistant
            .content
            .iter()
            .map(|block| match block {
                AssistantContent::Text(text) => text.text.chars().count(),
                AssistantContent::Thinking(thinking) => thinking.thinking.chars().count(),
                AssistantContent::ToolCall(call) => {
                    call.name.chars().count()
                        + serde_json::to_string(&call.arguments)
                            .unwrap_or_default()
                            .chars()
                            .count()
                }
            })
            .sum(),
    };
    tokens_for_chars(chars)
}

/// The context `messages` fill: the last usage a response reported, plus estimates for
/// the messages after it.
///
/// A response's usage counts only while no message before it is newer than it: a newer
/// one, such as a compaction summary, changed the prefix that usage measured.
pub(crate) fn estimate_context_tokens(messages: &[Message]) -> u64 {
    let mut latest_prefix = Timestamp::MIN;
    let mut last_usage = None;
    for (index, message) in messages.iter().enumerate() {
        if let Message::Assistant(assistant) = message
            && assistant.timestamp >= latest_prefix
            && !assistant.is_failure()
            && assistant.usage.context_tokens() > 0
        {
            last_usage = Some((index, assistant.usage.context_tokens()));
        }
        latest_prefix = latest_prefix.max(message.timestamp());
    }
    match last_usage {
        Some((index, tokens)) => {
            tokens
                + messages[index + 1..]
                    .iter()
                    .map(estimate_message_tokens)
                    .sum::<u64>()
        }
        None => messages.iter().map(estimate_message_tokens).sum(),
    }
}

/// Room left in the window beside the estimate, for its error.
const CONTEXT_SAFETY_TOKENS: u64 = 4_096;

/// `max_tokens`, lowered so the estimated input and the output stay inside the model's
/// context window with a safety margin, but never below one token.
pub(crate) fn clamp_max_tokens_to_context(
    model: &Model,
    context: &Context,
    max_tokens: u64,
) -> u64 {
    if model.context_window == 0 {
        return max_tokens.max(1);
    }
    let available = model
        .context_window
        .saturating_sub(estimate_context_tokens(&context.messages))
        .saturating_sub(CONTEXT_SAFETY_TOKENS);
    max_tokens.min(available.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        AssistantMessage, SystemMessage, ToolReference, ToolResultMessage, UserMessage,
    };
    use serde_json::json;

    fn model() -> Model {
        serde_json::from_value(json!({
            "id": "m", "name": "M", "api": "test", "provider": "p",
            "contextWindow": 10_000, "maxTokens": 8_000,
        }))
        .unwrap()
    }

    fn user(text: &str, timestamp: Timestamp) -> Message {
        Message::User(UserMessage {
            content: vec![Content::text(text)],
            timestamp,
        })
    }

    fn reply(context_tokens: u64, timestamp: Timestamp) -> Message {
        let mut reply = AssistantMessage::empty(&model());
        reply.usage.total_tokens = context_tokens;
        reply.timestamp = timestamp;
        Message::Assistant(reply)
    }

    #[test]
    fn text_is_three_and_a_half_characters_per_token_rounded_up() {
        assert_eq!(estimate_text_tokens(""), 0);
        assert_eq!(estimate_text_tokens("abcdefg"), 2);
        assert_eq!(estimate_message_tokens(&user("abcdefgh", 0)), 3);
    }

    #[test]
    fn images_count_as_a_fixed_size() {
        let message = Message::ToolResult(ToolResultMessage {
            tool_call_id: "c".into(),
            tool_name: "t".into(),
            content: vec![Content::image("AAAA", "image/png")],
            details: None,
            usage: None,
            is_error: false,
            timestamp: 0,
        });
        assert_eq!(estimate_message_tokens(&message), 1_372);
    }

    #[test]
    fn tool_declarations_count_as_their_json() {
        let tools = vec![Tool::new(
            "read",
            "Read a file",
            json!({ "type": "object" }),
        )];
        let json = serde_json::to_string(&tools).unwrap();
        assert_eq!(estimate_tool_tokens(&tools), estimate_text_tokens(&json));
        let system = Message::System(SystemMessage {
            content: "base".into(),
            tools_added: tools,
            tools_removed: vec![ToolReference {
                name: "bash".into(),
            }],
            ..SystemMessage::default()
        });
        assert_eq!(
            estimate_message_tokens(&system),
            estimate_text_tokens("base")
                + estimate_text_tokens(&json)
                + estimate_text_tokens("[{\"name\":\"bash\"}]")
        );
    }

    #[test]
    fn context_tokens_start_from_the_last_usage_that_still_applies() {
        let messages = vec![user("a", 1), reply(1_000, 2), user("abcdefgh", 3)];
        assert_eq!(estimate_context_tokens(&messages), 1_003);

        // A summary put in front after the reply makes its usage stale.
        let summarized = vec![user("summary", 10), reply(1_000, 2), user("abcdefgh", 11)];
        assert_eq!(
            estimate_context_tokens(&summarized),
            estimate_text_tokens("summary") + 3
        );

        let mut failed = AssistantMessage::failed(&model(), crate::StopReason::Error, "boom");
        failed.usage.total_tokens = 5_000;
        failed.timestamp = 4;
        let messages = vec![user("a", 1), reply(1_000, 2), Message::Assistant(failed)];
        assert_eq!(estimate_context_tokens(&messages), 1_000);
    }

    #[test]
    fn the_output_cap_is_fitted_to_the_window() {
        let model = model();
        let context = Context::from_messages(vec![user("a", 0), reply(1_000, 1)]);
        assert_eq!(clamp_max_tokens_to_context(&model, &context, 8_000), 4_904);
        assert_eq!(clamp_max_tokens_to_context(&model, &context, 2_000), 2_000);
        let full = Context::from_messages(vec![user("a", 0), reply(9_000, 1)]);
        assert_eq!(clamp_max_tokens_to_context(&model, &full, 8_000), 1);
        let unknown = Model {
            context_window: 0,
            ..model
        };
        assert_eq!(clamp_max_tokens_to_context(&unknown, &full, 8_000), 8_000);
    }
}
