//! Pi's one-call session setup.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pi_ai::faux::FauxProvider;
use pi_ai::{Model, ThinkingLevel};
use pi_coding_agent::extensions::{SessionStart, SessionStartReason, extension};
use pi_coding_agent::resources::ResourcePaths;
use pi_coding_agent::sdk::NO_MODELS_AVAILABLE;
use pi_coding_agent::session::{SessionManager, default_session_dir};
use pi_coding_agent::{
    CreateAgentSessionOptions, ModelRegistry, PromptOptions, StaticKeys, create_agent_session,
};
use serde_json::json;

fn model(provider: &str, id: &str, reasoning: bool) -> Model {
    let mut model = FauxProvider::default_model();
    model.provider = provider.into();
    model.id = id.into();
    model.reasoning = reasoning;
    model
}

/// A registry of `faux/faux-1` and `other/other-1`, with keys for `keyed` providers.
fn registry(faux: &FauxProvider, keyed: &[&str]) -> ModelRegistry {
    let keys = keyed
        .iter()
        .map(|provider| (provider.to_string(), "key".to_string()))
        .collect::<HashMap<_, _>>();
    let models = ModelRegistry::new(Arc::new(StaticKeys(keys)));
    models.register_api("faux", Arc::new(faux.clone()));
    models.register_models([
        model("faux", "faux-1", true),
        model("other", "other-1", true),
    ]);
    models
}

struct Folders {
    _root: tempfile::TempDir,
    agent: PathBuf,
    project: PathBuf,
}

fn folders() -> Folders {
    let root = tempfile::tempdir().unwrap();
    let agent = root.path().join("agent");
    let project = root.path().join("project");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    Folders {
        agent,
        project,
        _root: root,
    }
}

fn write_json(path: &Path, value: serde_json::Value) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, value.to_string()).unwrap();
}

fn setup(folders: &Folders, models: ModelRegistry) -> CreateAgentSessionOptions {
    let mut paths = ResourcePaths::new(&folders.agent, ".maple");
    paths.home_dir = None;
    let mut options = CreateAgentSessionOptions::new("Maple", paths, models);
    options.cwd = Some(folders.project.clone());
    options.builtin_tools = Some(Vec::new());
    options
}

/// A stored session in the project's session folder, with one exchange on
/// `other/other-1` at `level`; its file.
async fn stored_session(folders: &Folders, level: ThinkingLevel) -> PathBuf {
    let faux = FauxProvider::new();
    let mut options = setup(folders, registry(&faux, &["faux", "other"]));
    options.model = Some(model("other", "other-1", true));
    options.thinking_level = Some(level);
    let session = create_agent_session(options).await.unwrap().session;
    faux.push_text("answer");
    session
        .prompt("question", PromptOptions::default())
        .await
        .unwrap();
    session
        .with_session(|tree| tree.session_file().map(Path::to_path_buf))
        .unwrap()
}

#[tokio::test]
async fn a_new_session_starts_from_the_agents_folder() {
    let folders = folders();
    write_json(
        &folders.agent.join("settings.json"),
        json!({ "defaultProvider": "other", "defaultModel": "other-1", "defaultThinkingLevel": "high" }),
    );
    let faux = FauxProvider::new();
    let result = create_agent_session(setup(&folders, registry(&faux, &["faux", "other"])))
        .await
        .unwrap();
    assert!(result.model_fallback_message.is_none());
    assert!(result.settings_errors.is_empty());
    let session = result.session;
    assert_eq!(session.model().unwrap().id, "other-1");
    assert_eq!(session.thinking_level(), ThinkingLevel::High);

    // The session is kept in Pi's folder for the project once there is a conversation.
    faux.push_text("hi");
    session
        .prompt("hello", PromptOptions::default())
        .await
        .unwrap();
    let file = session
        .with_session(|tree| tree.session_file().map(Path::to_path_buf))
        .unwrap();
    assert_eq!(
        file.parent().unwrap(),
        default_session_dir(&folders.project, &folders.agent)
    );
    assert!(file.exists());

    // What the session changes is saved to the user's settings.
    session.set_auto_retry_enabled(false);
    let saved: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(folders.agent.join("settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(saved["retry"]["enabled"], json!(false));
    assert_eq!(saved["defaultModel"], json!("other-1"));
}

#[tokio::test]
async fn without_a_default_the_first_model_with_credentials_is_used() {
    let folders = folders();
    // The default's provider has no credentials, nor has the first model's.
    write_json(
        &folders.agent.join("settings.json"),
        json!({ "defaultProvider": "faux", "defaultModel": "faux-1" }),
    );
    let faux = FauxProvider::new();
    let result = create_agent_session(setup(&folders, registry(&faux, &["other"])))
        .await
        .unwrap();
    assert!(result.model_fallback_message.is_none());
    assert_eq!(result.session.model().unwrap().id, "other-1");

    // With no credentials at all there is no model, and no thinking.
    let result = create_agent_session(setup(&folders, registry(&faux, &[])))
        .await
        .unwrap();
    assert_eq!(
        result.model_fallback_message.as_deref(),
        Some(NO_MODELS_AVAILABLE)
    );
    assert!(result.session.model().is_none());
    assert_eq!(result.session.thinking_level(), ThinkingLevel::Off);
}

#[tokio::test]
async fn a_resumed_session_restores_its_model_while_it_has_credentials() {
    let folders = folders();
    let file = stored_session(&folders, ThinkingLevel::Low).await;

    let faux = FauxProvider::new();
    let mut options = setup(&folders, registry(&faux, &["faux", "other"]));
    options.session = Some(SessionManager::open_file(&file, None).unwrap());
    let result = create_agent_session(options).await.unwrap();
    assert!(result.model_fallback_message.is_none());
    assert_eq!(result.session.model().unwrap().id, "other-1");
    assert_eq!(result.session.thinking_level(), ThinkingLevel::Low);

    // Without credentials for its provider, the session starts with another model and
    // says so, keeping its level.
    let mut options = setup(&folders, registry(&faux, &["faux"]));
    options.session = Some(SessionManager::open_file(&file, None).unwrap());
    let result = create_agent_session(options).await.unwrap();
    assert_eq!(
        result.model_fallback_message.as_deref(),
        Some("Could not restore model other/other-1. Using faux/faux-1")
    );
    assert_eq!(result.session.model().unwrap().id, "faux-1");
    assert_eq!(result.session.thinking_level(), ThinkingLevel::Low);

    // What the host asks for wins over the record, and the level is clamped.
    let mut options = setup(&folders, registry(&faux, &["faux", "other"]));
    options.session = Some(SessionManager::open_file(&file, None).unwrap());
    options.model = Some(model("faux", "plain", false));
    options.thinking_level = Some(ThinkingLevel::High);
    let result = create_agent_session(options).await.unwrap();
    assert!(result.model_fallback_message.is_none());
    assert_eq!(result.session.model().unwrap().id, "plain");
    assert_eq!(result.session.thinking_level(), ThinkingLevel::Off);
}

#[tokio::test]
async fn a_model_an_extension_provides_is_restored() {
    let folders = folders();
    let faux = FauxProvider::new();
    let provided = model("plugin", "plugin-1", true);
    let file = {
        let mut options = setup(&folders, registry(&faux, &["faux", "plugin"]));
        options.model = Some(provided.clone());
        let session = create_agent_session(options).await.unwrap().session;
        faux.push_text("answer");
        session
            .prompt("question", PromptOptions::default())
            .await
            .unwrap();
        session
            .with_session(|tree| tree.session_file().map(Path::to_path_buf))
            .unwrap()
    };

    let mut options = setup(&folders, registry(&faux, &["faux", "plugin"]));
    options.session = Some(SessionManager::open_file(&file, None).unwrap());
    let registered = provided.clone();
    options.extensions = vec![extension("plugin", move |api| {
        api.register_provider(vec![registered.clone()], None);
    })];
    let result = create_agent_session(options).await.unwrap();
    assert!(result.model_fallback_message.is_none());
    assert_eq!(result.session.model(), Some(provided));
}

#[tokio::test]
async fn project_settings_load_only_for_a_trusted_project() {
    let folders = folders();
    write_json(
        &folders.agent.join("settings.json"),
        json!({ "defaultThinkingLevel": "low" }),
    );
    write_json(
        &folders.project.join(".maple/settings.json"),
        json!({ "defaultThinkingLevel": "high" }),
    );
    let faux = FauxProvider::new();
    let session = create_agent_session(setup(&folders, registry(&faux, &["faux"])))
        .await
        .unwrap()
        .session;
    assert_eq!(session.thinking_level(), ThinkingLevel::Low);

    let mut options = setup(&folders, registry(&faux, &["faux"]));
    options.project_trusted = true;
    let session = create_agent_session(options).await.unwrap().session;
    assert_eq!(session.thinking_level(), ThinkingLevel::High);

    // A settings file that does not parse is reported, and the session goes on.
    std::fs::write(folders.agent.join("settings.json"), "{ not json").unwrap();
    let result = create_agent_session(setup(&folders, registry(&faux, &["faux"])))
        .await
        .unwrap();
    assert_eq!(result.settings_errors.len(), 1);
    assert_eq!(
        result.settings_errors[0].path,
        folders.agent.join("settings.json")
    );
}

#[tokio::test]
async fn extensions_hear_the_start_the_host_names() {
    let folders = folders();
    let faux = FauxProvider::new();
    let heard = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = heard.clone();
    let mut options = setup(&folders, registry(&faux, &["faux"]));
    options.session_start = SessionStart {
        reason: SessionStartReason::Resume,
        previous_session_file: Some(PathBuf::from("/before.jsonl")),
    };
    options.extensions = vec![extension("listener", move |api| {
        let sink = sink.clone();
        api.on(move |event: SessionStart, _ctx| {
            let sink = sink.clone();
            async move {
                sink.lock()
                    .unwrap()
                    .push((event.reason, event.previous_session_file));
                Ok(())
            }
        });
    })];
    create_agent_session(options).await.unwrap();
    assert_eq!(
        *heard.lock().unwrap(),
        [(
            SessionStartReason::Resume,
            Some(PathBuf::from("/before.jsonl"))
        )]
    );
}
