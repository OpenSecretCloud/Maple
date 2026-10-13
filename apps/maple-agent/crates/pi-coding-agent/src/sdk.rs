//! A session in one call, as Pi's `createAgentSession` makes one: the settings and
//! resources of the agent's folder and the project, a new session file unless the host
//! opens one, and the model and thinking level a resumed session recorded.

use std::path::PathBuf;
use std::sync::Arc;

use pi_ai::{Model, ThinkingLevel};

use crate::agent_session::{
    AgentSession, AgentSessionError, AgentSessionOptions, DEFAULT_THINKING_LEVEL, SetupChoice,
};
use crate::extensions::{Extension, ExtensionUi, NoUi, RegisteredTool, SessionStart};
use crate::models::ModelRegistry;
use crate::resources::{ResourcePaths, Resources};
use crate::session::{SessionManager, SessionProjection, default_session_dir};
use crate::settings::{Settings, SettingsError};
use crate::tools::{ToolsOptions, resolve_to_cwd};

/// What a session starts with when no model can be found, as Pi's
/// `formatNoModelsAvailableMessage` without its login help, which is the host's.
pub const NO_MODELS_AVAILABLE: &str = "No models available.";

/// How [`create_agent_session`] sets a session up, as Pi's `CreateAgentSessionOptions`.
/// What has a default is optional; the rest is handed to the session as
/// [`AgentSessionOptions`] takes it.
pub struct CreateAgentSessionOptions {
    /// The working directory: the session's own when `None` and one is given, or else
    /// the process's.
    pub cwd: Option<PathBuf>,
    /// The agent's folder, where the user's settings, resources and sessions are, and
    /// the project folder's name.
    pub paths: ResourcePaths,
    pub app_name: String,
    pub models: ModelRegistry,
    /// The model to start with. Without one, a session that has a conversation starts
    /// with the model it recorded, when its provider has credentials, and anything else
    /// with [`find_initial_model`]'s.
    pub model: Option<Model>,
    /// The thinking level to start at, clamped to the model. Without one, a session
    /// that has a conversation keeps the level it recorded, and a new one starts at
    /// the model's own level from the settings, or else at the default.
    pub thinking_level: Option<ThinkingLevel>,
    /// The session to continue; a new one kept in [`default_session_dir`] when `None`.
    pub session: Option<SessionManager>,
    /// The settings; when `None`, the user's `settings.json` in the agent's folder,
    /// overlaid with the project's when the project is trusted. Settings changed in
    /// the session are then saved to the user's file, as in Pi.
    pub settings: Option<Settings>,
    /// Where settings the host gives are saved; see [`AgentSessionOptions`].
    pub settings_file: Option<PathBuf>,
    /// The resources; loaded for the working directory when `None`.
    pub resources: Option<Resources>,
    /// Whether the project's own settings and resources load. The host decides, as it
    /// does for [`AgentSessionOptions`].
    pub project_trusted: bool,
    /// What extensions hear as the session starts.
    pub session_start: SessionStart,
    pub tool_options: ToolsOptions,
    pub builtin_tools: Option<Vec<String>>,
    pub tools: Vec<RegisteredTool>,
    pub extensions: Vec<Arc<dyn Extension>>,
    pub ui: Arc<dyn ExtensionUi>,
    pub custom_prompt: Option<String>,
    pub skill_load_hint: Option<String>,
}

impl CreateAgentSessionOptions {
    pub fn new(app_name: &str, paths: ResourcePaths, models: ModelRegistry) -> Self {
        Self {
            cwd: None,
            paths,
            app_name: app_name.to_string(),
            models,
            model: None,
            thinking_level: None,
            session: None,
            settings: None,
            settings_file: None,
            resources: None,
            project_trusted: false,
            session_start: SessionStart::default(),
            tool_options: ToolsOptions::default(),
            builtin_tools: None,
            tools: Vec::new(),
            extensions: Vec::new(),
            ui: Arc::new(NoUi),
            custom_prompt: None,
            skill_load_hint: None,
        }
    }
}

/// What [`create_agent_session`] made, as Pi's `CreateAgentSessionResult`.
pub struct CreateAgentSessionResult {
    pub session: AgentSession,
    /// Why the session did not start with the model it recorded, and with which one it
    /// did, or that no model was found, as Pi's `modelFallbackMessage`.
    pub model_fallback_message: Option<String>,
    /// The settings files that could not be loaded; the session goes on without them.
    pub settings_errors: Vec<SettingsError>,
}

/// Set up a session as Pi's `createAgentSession` does.
pub async fn create_agent_session(
    options: CreateAgentSessionOptions,
) -> Result<CreateAgentSessionResult, AgentSessionError> {
    let process_cwd = std::env::current_dir()
        .map_err(|error| AgentSessionError::Failed(format!("No working directory: {error}")))?;
    let cwd = options
        .cwd
        .or_else(|| {
            options
                .session
                .as_ref()
                .map(|session| PathBuf::from(session.cwd()))
        })
        .unwrap_or_else(|| process_cwd.clone());
    let cwd = resolve_to_cwd(&cwd.to_string_lossy(), &process_cwd);
    let mut paths = options.paths;
    paths.agent_dir = resolve_to_cwd(&paths.agent_dir.to_string_lossy(), &process_cwd);

    let (settings, settings_file, settings_errors) = match options.settings {
        Some(settings) => (settings, options.settings_file, Vec::new()),
        None => {
            let user = paths.agent_dir.join("settings.json");
            let project = paths.project_dir(&cwd).join("settings.json");
            let (settings, errors) = Settings::load(
                Some(&user),
                options.project_trusted.then_some(project.as_path()),
            );
            (settings, Some(user), errors)
        }
    };
    let session = options.session.unwrap_or_else(|| {
        SessionManager::create_in(
            cwd.to_string_lossy(),
            &default_session_dir(&cwd, &paths.agent_dir),
        )
    });
    let resources = options.resources.unwrap_or_else(|| {
        Resources::load(&cwd, &paths, &settings, &[], &[], options.project_trusted)
    });

    let mut session_options =
        AgentSessionOptions::new(&cwd, &options.app_name, session, options.models);
    session_options.settings = settings;
    session_options.settings_file = settings_file;
    session_options.resources = resources;
    session_options.project_trusted = options.project_trusted;
    session_options.session_start = options.session_start;
    session_options.tool_options = options.tool_options;
    session_options.builtin_tools = options.builtin_tools;
    session_options.tools = options.tools;
    session_options.extensions = options.extensions;
    session_options.ui = options.ui;
    session_options.custom_prompt = options.custom_prompt;
    session_options.skill_load_hint = options.skill_load_hint;
    let (session, model_fallback_message) = AgentSession::start(
        session_options,
        Some(SetupChoice {
            model: options.model,
            thinking_level: options.thinking_level,
        }),
    )
    .await?;
    Ok(CreateAgentSessionResult {
        session,
        model_fallback_message,
        settings_errors,
    })
}

/// The model a session starts with when it asks for none and restores none, as Pi's
/// `findInitialModel` finds it: the settings' default when its provider has
/// credentials, or else the first model whose provider has.
pub async fn find_initial_model(models: &ModelRegistry, settings: &Settings) -> Option<Model> {
    if let (Some(provider), Some(id)) = (&settings.default_provider, &settings.default_model)
        && let Some(model) = models.find(provider, id)
        && models.has_configured_auth(&model.provider).await
    {
        return Some(model);
    }
    models.available_models().await.into_iter().next()
}

/// The model and thinking level [`create_agent_session`] starts a session with.
pub(crate) struct ChosenModel {
    pub model: Option<Model>,
    pub thinking_level: ThinkingLevel,
    pub fallback_message: Option<String>,
}

/// Choose as Pi's `createAgentSession` does, with the models extensions provide already
/// registered, so a session can restore one of them.
pub(crate) async fn choose_model(
    models: &ModelRegistry,
    settings: &Settings,
    projection: &SessionProjection,
    has_thinking_entry: bool,
    requested_model: Option<Model>,
    requested_thinking_level: Option<ThinkingLevel>,
) -> ChosenModel {
    let has_conversation = !projection.messages.is_empty();
    let mut model = requested_model;
    let mut fallback_message = None;
    if model.is_none()
        && has_conversation
        && let Some((provider, id)) = &projection.model
    {
        match models.find(provider, id) {
            Some(restored) if models.has_configured_auth(&restored.provider).await => {
                model = Some(restored);
            }
            _ => fallback_message = Some(format!("Could not restore model {provider}/{id}")),
        }
    }
    if model.is_none() {
        model = find_initial_model(models, settings).await;
        match &model {
            None => fallback_message = Some(NO_MODELS_AVAILABLE.to_string()),
            Some(found) => {
                if let Some(message) = &mut fallback_message {
                    message.push_str(&format!(". Using {}/{}", found.provider, found.id));
                }
            }
        }
    }

    let mut thinking_level = requested_thinking_level;
    if thinking_level.is_none() && has_conversation {
        thinking_level = Some(if has_thinking_entry {
            projection.thinking_level
        } else {
            settings
                .default_thinking_level
                .unwrap_or(DEFAULT_THINKING_LEVEL)
        });
    }
    if thinking_level.is_none()
        && let Some(model) = &model
    {
        thinking_level = settings.model_thinking_level(model);
    }
    let thinking_level = thinking_level
        .or(settings.default_thinking_level)
        .unwrap_or(DEFAULT_THINKING_LEVEL);
    ChosenModel {
        thinking_level: model.as_ref().map_or(ThinkingLevel::Off, |model| {
            model.clamp_thinking_level(thinking_level)
        }),
        model,
        fallback_message,
    }
}
