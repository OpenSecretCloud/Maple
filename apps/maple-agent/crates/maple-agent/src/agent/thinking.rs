//! Composer thinking controls backed by Pi's session and account defaults.

use pi_coding_agent::AgentSession;

use super::config::{load_agent_config_inner, save_agent_config_inner};
use super::{AgentRuntimeHandle, AgentThinkingState, ThinkingLevel};

impl AgentRuntimeHandle {
    async fn thinking_session(
        &self,
        session_id: &str,
        model_id: &str,
    ) -> Result<AgentSession, String> {
        let runtime = self.runtime().await?;
        if let Some(session) = runtime.loaded_session(session_id).await
            && session.model().is_some_and(|model| model.id == model_id)
        {
            return Ok(session);
        }
        let row = runtime
            .store
            .get(session_id)?
            .ok_or_else(|| format!("Failed to find Agent task {session_id}"))?;
        let model_id = if row.message_count > 0 {
            row.model.as_deref().unwrap_or(model_id)
        } else {
            model_id
        };
        let model = runtime.pi_model(model_id, None, None).await;
        runtime.task_session(&row, model).await
    }

    /// A task reads its persisted Pi level. A new-task composer previews
    /// the saved default, or Pi's Medium default, clamped by its model.
    pub async fn thinking_state(
        &self,
        session_id: Option<&str>,
        model_id: &str,
    ) -> Result<AgentThinkingState, String> {
        if let Some(session_id) = session_id {
            let session = self.thinking_session(session_id, model_id).await?;
            let model = session.model().ok_or("No model selected")?;
            return Ok(AgentThinkingState {
                levels: model.thinking_levels(),
                current: session.thinking_level(),
            });
        }
        let runtime = self.runtime().await?;
        let model = runtime.pi_model(model_id, None, None).await;
        let default = self
            .load_config()
            .await?
            .default_thinking_level
            .unwrap_or(ThinkingLevel::Medium);
        Ok(AgentThinkingState {
            levels: model.thinking_levels(),
            current: model.clamp_thinking_level(default),
        })
    }

    /// Pi records the task's selection. Save the actual clamped level for
    /// future tasks without overwriting other account settings.
    pub async fn set_thinking_level(
        &self,
        session_id: Option<&str>,
        model_id: &str,
        level: ThinkingLevel,
    ) -> Result<AgentThinkingState, String> {
        self.verify_generation().await?;
        self.ensure_accepting_new_work()?;
        let state = if let Some(session_id) = session_id {
            let session = self.thinking_session(session_id, model_id).await?;
            session.set_thinking_level(level).await;
            AgentThinkingState {
                levels: session
                    .model()
                    .ok_or("No model selected")?
                    .thinking_levels(),
                current: session.thinking_level(),
            }
        } else {
            let runtime = self.runtime().await?;
            let model = runtime.pi_model(model_id, None, None).await;
            AgentThinkingState {
                levels: model.thinking_levels(),
                current: model.clamp_thinking_level(level),
            }
        };
        self.verify_generation().await?;
        let _settings = self.lock_settings().await;
        let mut config = load_agent_config_inner(self.paths(), &self.user_id)
            .map_err(|error| error.to_string())?;
        config.default_thinking_level = Some(state.current);
        save_agent_config_inner(self.paths(), &self.user_id, &config)
            .map_err(|error| error.to_string())?;
        Ok(state)
    }
}
