mod common;

use futures::StreamExt;
use opensecret::{
    AgentItemsListParams, AgentSseEvent, ConversationItem, CreateSubagentRequest,
    ListSubagentsParams, OpenSecretClient, Result,
};
use std::env;
use uuid::Uuid;

fn conversation_item_id(item: &ConversationItem) -> Uuid {
    match item {
        ConversationItem::Message { id, .. }
        | ConversationItem::FunctionToolCall { id, .. }
        | ConversationItem::FunctionToolCallOutput { id, .. }
        | ConversationItem::Reasoning { id, .. } => *id,
    }
}

async fn setup_authenticated_client() -> Result<OpenSecretClient> {
    let env_path = std::path::Path::new("../.env.local");
    if env_path.exists() {
        dotenvy::from_path(env_path).ok();
    } else {
        dotenvy::dotenv().ok();
    }

    let base_url = env::var("VITE_OPEN_SECRET_API_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());
    let client = common::new_test_client(base_url)?;
    client.perform_attestation_handshake().await?;

    let email = env::var("VITE_TEST_EMAIL").expect("VITE_TEST_EMAIL must be set");
    let password = env::var("VITE_TEST_PASSWORD").expect("VITE_TEST_PASSWORD must be set");
    let name = env::var("VITE_TEST_NAME").ok();
    let client_id = env::var("VITE_TEST_CLIENT_ID")
        .expect("VITE_TEST_CLIENT_ID must be set")
        .parse::<Uuid>()
        .expect("Invalid client_id format");

    match client
        .login(email.clone(), password.clone(), client_id)
        .await
    {
        Ok(_) => {}
        Err(_) => {
            client.register(email, password, client_id, name).await?;
        }
    }

    Ok(client)
}

async fn create_test_subagent(client: &OpenSecretClient) -> Result<opensecret::SubagentResponse> {
    let suffix = Uuid::new_v4();

    client
        .create_subagent(CreateSubagentRequest {
            display_name: Some(format!("Rust SDK Test {}", suffix)),
            purpose: format!("Rust SDK integration test subagent {}", suffix),
        })
        .await
}

#[tokio::test]
#[ignore = "Requires agent API on server"]
async fn test_create_and_delete_subagent() {
    let client = setup_authenticated_client()
        .await
        .expect("Failed to setup client");

    let subagent = create_test_subagent(&client)
        .await
        .expect("Failed to create subagent");

    assert_eq!(subagent.object, "agent.subagent");
    assert!(!subagent.display_name.is_empty());
    assert!(!subagent.purpose.is_empty());

    let deleted = client
        .delete_subagent(subagent.id)
        .await
        .expect("Failed to delete subagent");

    assert!(deleted.deleted);
    assert_eq!(deleted.id, subagent.id);
    assert_eq!(deleted.object, "agent.subagent.deleted");
}

#[tokio::test]
#[ignore = "Requires agent API on server"]
async fn test_get_main_agent_and_items() {
    let client = setup_authenticated_client()
        .await
        .expect("Failed to setup client");

    let main_agent = client
        .get_main_agent()
        .await
        .expect("Failed to get main agent");

    assert_eq!(main_agent.object, "agent.main");
    assert_eq!(main_agent.kind, "main");
    assert!(!main_agent.display_name.is_empty());

    let items = client
        .list_main_agent_items(Some(AgentItemsListParams {
            limit: Some(10),
            order: Some("desc".to_string()),
            ..Default::default()
        }))
        .await
        .expect("Failed to list main agent items");

    assert_eq!(items.object, "list");

    if let Some(first_item) = items.data.first() {
        let item_id = conversation_item_id(first_item);
        let item = client
            .get_main_agent_item(item_id)
            .await
            .expect("Failed to get main agent item");

        assert_eq!(conversation_item_id(&item), item_id);
    }
}

#[tokio::test]
#[ignore = "Requires agent API on server"]
async fn test_delete_main_agent_resets_agent_tree() {
    let client = setup_authenticated_client()
        .await
        .expect("Failed to setup client");

    let main_agent = client
        .get_main_agent()
        .await
        .expect("Failed to get main agent");

    let subagent = create_test_subagent(&client)
        .await
        .expect("Failed to create subagent");

    let deleted = client
        .delete_main_agent()
        .await
        .expect("Failed to delete main agent");

    assert!(deleted.deleted);
    assert_eq!(deleted.id, main_agent.id);
    assert_eq!(deleted.object, "agent.main.deleted");

    let subagents = client
        .list_subagents(Some(ListSubagentsParams {
            limit: Some(10),
            ..Default::default()
        }))
        .await
        .expect("Failed to list subagents after main deletion");

    assert!(!subagents.data.iter().any(|item| item.id == subagent.id));

    let recreated = client
        .get_main_agent()
        .await
        .expect("Failed to recreate main agent");

    assert_eq!(recreated.object, "agent.main");
}

#[tokio::test]
#[ignore = "Requires agent API on server"]
async fn test_list_and_get_subagents() {
    let client = setup_authenticated_client()
        .await
        .expect("Failed to setup client");

    let subagent = create_test_subagent(&client)
        .await
        .expect("Failed to create subagent");

    let list = client
        .list_subagents(Some(ListSubagentsParams {
            limit: Some(10),
            created_by: Some("user".to_string()),
            ..Default::default()
        }))
        .await
        .expect("Failed to list subagents");

    assert_eq!(list.object, "list");
    assert!(list.data.iter().any(|item| item.id == subagent.id));

    let fetched = client
        .get_subagent(subagent.id)
        .await
        .expect("Failed to get subagent");

    assert_eq!(fetched.id, subagent.id);
    assert_eq!(fetched.kind, "subagent");

    client
        .delete_subagent(subagent.id)
        .await
        .expect("Failed to delete subagent");
}

#[tokio::test]
#[ignore = "Requires agent API on server"]
async fn test_agent_chat_sse() {
    let client = setup_authenticated_client()
        .await
        .expect("Failed to setup client");

    let mut stream = client
        .agent_chat("Hello, please respond with just the word 'pong'.")
        .await
        .expect("Failed to start agent chat");

    let mut got_message = false;
    let mut got_done = false;
    let mut all_messages: Vec<String> = Vec::new();

    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => match event {
                AgentSseEvent::Message(msg) => {
                    got_message = true;
                    all_messages.push(msg.message.clone());
                    println!("Agent message {}: {:?}", msg.message_id, all_messages);
                }
                AgentSseEvent::Reaction(reaction) => {
                    println!(
                        "Agent reaction on item {}: {}",
                        reaction.item_id, reaction.emoji
                    );
                }
                AgentSseEvent::Typing(_) => {
                    println!("Agent typing");
                }
                AgentSseEvent::Done(_) => {
                    got_done = true;
                    println!("Agent done");
                }
                AgentSseEvent::Error(err) => {
                    panic!("Agent error: {}", err.error);
                }
            },
            Err(e) => {
                panic!("Stream error: {:?}", e);
            }
        }
    }

    assert!(
        got_message,
        "Should have received at least one message event"
    );
    assert!(got_done, "Should have received a done event");
    assert!(!all_messages.is_empty(), "Should have at least one message");
}

#[tokio::test]
#[ignore = "Requires agent API on server"]
async fn test_subagent_chat_sse() {
    let client = setup_authenticated_client()
        .await
        .expect("Failed to setup client");

    let subagent = create_test_subagent(&client)
        .await
        .expect("Failed to create subagent");

    let mut stream = client
        .subagent_chat(subagent.id, "Please reply with the word 'subpong'.")
        .await
        .expect("Failed to start subagent chat");

    let mut got_message = false;
    let mut got_done = false;
    let mut all_messages: Vec<String> = Vec::new();

    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => match event {
                AgentSseEvent::Message(msg) => {
                    got_message = true;
                    all_messages.push(msg.message.clone());
                    println!("Subagent message {}: {:?}", msg.message_id, all_messages);
                }
                AgentSseEvent::Reaction(reaction) => {
                    println!(
                        "Subagent reaction on item {}: {}",
                        reaction.item_id, reaction.emoji
                    );
                }
                AgentSseEvent::Typing(_) => {
                    println!("Subagent typing");
                }
                AgentSseEvent::Done(_) => {
                    got_done = true;
                    println!("Subagent done");
                }
                AgentSseEvent::Error(err) => {
                    panic!("Subagent error: {}", err.error);
                }
            },
            Err(e) => {
                panic!("Stream error: {:?}", e);
            }
        }
    }

    let items = client
        .list_subagent_items(
            subagent.id,
            Some(AgentItemsListParams {
                limit: Some(10),
                order: Some("desc".to_string()),
                ..Default::default()
            }),
        )
        .await
        .expect("Failed to list subagent items");

    assert_eq!(items.object, "list");

    if let Some(first_item) = items.data.first() {
        let item_id = conversation_item_id(first_item);
        let item = client
            .get_subagent_item(subagent.id, item_id)
            .await
            .expect("Failed to get subagent item");

        assert_eq!(conversation_item_id(&item), item_id);
    }

    let deleted = client
        .delete_subagent(subagent.id)
        .await
        .expect("Failed to delete subagent after chat");

    assert!(deleted.deleted);
    assert!(
        got_message,
        "Should have received at least one subagent message"
    );
    assert!(got_done, "Should have received a subagent done event");
    assert!(
        !all_messages.is_empty(),
        "Should have at least one subagent message"
    );
}
