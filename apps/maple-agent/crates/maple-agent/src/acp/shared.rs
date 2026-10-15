//! Projection of shared host tasks and the steering extension used by the
//! Codex and Claude ACP adapters. ACP v1 prompt responses still end with a turn.

use super::*;
use serde_json::{Value, json};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SteeringRequest {
    session_id: SessionId,
    prompt: Vec<agent_client_protocol::schema::v1::ContentBlock>,
    #[serde(rename = "_meta", default)]
    meta: Value,
}

impl AcpConnectionContext {
    pub(super) async fn steer(
        self: &Arc<Self>,
        cx: &ConnectionTo<Client>,
        params: Value,
    ) -> Result<Value, agent_client_protocol::Error> {
        let request: SteeringRequest = serde_json::from_value(params)
            .map_err(|_| agent_client_protocol::Error::invalid_params())?;
        let idle = request
            .meta
            .pointer("/steering/idleBehavior")
            .and_then(Value::as_str);
        if idle.is_some_and(|behavior| behavior != "promptRequired") {
            return Err(agent_client_protocol::Error::invalid_params()
                .data("Unsupported steering idleBehavior"));
        }
        let session_id = canonical_session_id(&request.session_id)?;
        let (access, model) = {
            let sessions = self.sessions.lock().await;
            let session = sessions.get(&session_id).ok_or_else(|| {
                agent_client_protocol::Error::resource_not_found(Some(session_id.clone()))
            })?;
            (
                session
                    .lease
                    .as_ref()
                    .ok_or_else(agent_client_protocol::Error::invalid_request)?
                    .access(),
                session.model.clone(),
            )
        };
        let catalog = self
            .agent
            .model_catalog_entry(&model)
            .await
            .map_err(internal_acp_error)?;
        let injected = self
            .agent
            .steer_surface_message(
                &access,
                AgentSendMessageRequest {
                    session_id: session_id.clone(),
                    text: prompt_text(&request.prompt)?,
                    attachments: prompt_images(&request.prompt),
                    model: Some(model),
                    context_limit: None,
                    vision_capable: catalog.and_then(|entry| entry.vision) == Some(true),
                    steer: true,
                    queue_id: None,
                },
            )
            .await
            .map_err(internal_acp_error)?;
        if injected {
            return Ok(json!({"outcome": "injected"}));
        }
        if idle == Some("promptRequired") {
            return Ok(json!({"outcome": "promptRequired", "reason": "noRunningTurn"}));
        }
        // Legacy adapters start a new turn when steering races with completion.
        // Clients opting into promptRequired retain ownership of that fallback.
        let prompt = PromptRequest::new(request.session_id, request.prompt);
        let admission = self.begin_prompt(&prompt).await?;
        let context = Arc::clone(self);
        let cx = cx.clone();
        self.background_tasks.lock().await.spawn(async move {
            if let Err(error) = context.prompt(&cx, admission).await {
                log::debug!("ACP steering fallback ended: {error}");
            }
        });
        Ok(json!({"outcome": "startedNewTurn"}))
    }
}

impl AcpConnectionContext {
    pub(super) async fn start_observing(self: &Arc<Self>, cx: ConnectionTo<Client>) {
        if self.observing.swap(true, Ordering::SeqCst) {
            return;
        }
        if self.client_supports_form_elicitation.load(Ordering::SeqCst) {
            *self.question_responder.lock().await = Some(self.agent.question_responder());
        }
        let context = self.clone();
        let mut events = self.agent.subscribe();
        self.background_tasks.lock().await.spawn(async move {
            loop {
                let event = tokio::select! {
                    _ = context.lifetime.cancelled() => break,
                    event = events.recv() => match event {
                        Ok(event) => event,
                        Err(_) => { context.lifetime.cancel(); break; }
                    },
                };
                if context.observe(&cx, event).await.is_err() {
                    context.lifetime.cancel();
                    break;
                }
            }
        });
    }

    pub(super) async fn observe(
        self: &Arc<Self>,
        cx: &ConnectionTo<Client>,
        event: crate::agent::AgentServiceEvent,
    ) -> Result<(), AcpOutboundSendError> {
        use crate::agent::AgentServiceEvent as Event;
        let summary = match &event {
            Event::SessionUpdated { session, .. }
            | Event::Run {
                event: AgentRunEvent::SessionUpdated(session),
                ..
            } => Some(session),
            _ => None,
        };
        if let Some(summary) = summary {
            let changed = {
                let mut sessions = self.sessions.lock().await;
                if let Some(session) = sessions.get_mut(&summary.id) {
                    let changed = session.message_count != summary.message_count
                        || summary
                            .model
                            .as_ref()
                            .is_some_and(|model| model != &session.model);
                    session.message_count = summary.message_count;
                    if let Some(model) = &summary.model
                        && model != &session.model
                    {
                        session.model = model.clone();
                        session.catalog = None;
                        session.thinking = None;
                    }
                    changed.then(|| session.config_options())
                } else {
                    None
                }
            };
            if let Some(options) = changed {
                self.send_session_update(
                    cx,
                    SessionNotification::new(
                        summary.id.clone(),
                        SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(options)),
                    ),
                    &self.lifetime,
                )
                .await?;
            }
        }
        let (session_id, item, direct) = match event {
            Event::AccountChanged(_) => {
                self.lifetime.cancel();
                return Ok(());
            }
            Event::Question {
                session_id,
                request_id,
                questions,
            } => {
                if self.client_supports_form_elicitation.load(Ordering::SeqCst)
                    && self.sessions.lock().await.contains_key(&session_id)
                    && self.eliciting.lock().await.insert(request_id.clone())
                {
                    let context = self.clone();
                    let cx = cx.clone();
                    self.background_tasks.lock().await.spawn(async move {
                        context
                            .elicit_question(&cx, session_id, request_id.clone(), questions)
                            .await;
                        context.eliciting.lock().await.remove(&request_id);
                    });
                }
                return Ok(());
            }
            Event::ThinkingChanged { session_id, state } => {
                let mut sessions = self.sessions.lock().await;
                let Some(session) = sessions.get_mut(&session_id) else {
                    return Ok(());
                };
                session.thinking = Some(state);
                let update = SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(
                    session.config_options(),
                ));
                drop(sessions);
                // Reasoning may change during a prompt; never queue it behind
                // the prompt's whole-turn history gate.
                self.send_session_update(
                    cx,
                    SessionNotification::new(session_id, update),
                    &self.lifetime,
                )
                .await?;
                return Ok(());
            }
            Event::Run {
                session_id,
                run_id,
                event,
            } => {
                // The originating prompt has its own terminal-aware projection.
                if self.owned_runs.lock().await.get(&session_id) == Some(&run_id)
                    || self.prompt_states.lock().await.contains_key(&session_id)
                {
                    return Ok(());
                }
                match event {
                    AgentRunEvent::TimelineItem(item) | AgentRunEvent::Error(item) => {
                        (session_id, Some(item), None)
                    }
                    AgentRunEvent::SessionUpdated(summary) => {
                        let update = self.title_update(&session_id, summary.title).await;
                        (session_id, None, update)
                    }
                    _ => return Ok(()),
                }
            }
            Event::TimelineItem {
                session_id, item, ..
            } => (session_id, Some(item), None),
            Event::SessionUpdated {
                session_id,
                session,
                ..
            } => {
                let update = self.title_update(&session_id, session.title).await;
                (session_id, None, update)
            }
            _ => return Ok(()),
        };
        // Titles do not touch history projection. Waiting on its gate while
        // our own prompt holds it would stop this observer from draining the
        // broadcast stream for the entire turn, even for a fast client.
        if let Some(update) = direct {
            return self
                .send_session_update(
                    cx,
                    SessionNotification::new(session_id, update),
                    &self.lifetime,
                )
                .await;
        }
        let Some(item) = item else {
            return Ok(());
        };
        let operation = self
            .session_operations
            .lock()
            .await
            .get(&session_id)
            .cloned();
        let Some(operation) = operation else {
            return Ok(());
        };
        let _gate = tokio::select! {
            _ = self.lifetime.cancelled() => return Ok(()),
            gate = operation.gate.lock() => gate,
        };
        let update = {
            let mut sessions = self.sessions.lock().await;
            let Some(session) = sessions.get_mut(&session_id) else {
                return Ok(());
            };
            (item.revision == 0 || item.revision > session.projection.snapshot_revision)
                .then(|| timeline_update(&item, &mut session.projection, true))
                .flatten()
        };
        if let Some(update) = update {
            self.send_session_update(
                cx,
                SessionNotification::new(session_id, update),
                &self.lifetime,
            )
            .await?;
        }
        Ok(())
    }

    async fn elicit_question(
        &self,
        cx: &ConnectionTo<Client>,
        session_id: String,
        request_id: String,
        questions: Vec<crate::agent::AgentQuestion>,
    ) {
        use agent_client_protocol::schema::v1::{
            CreateElicitationRequest, ElicitationFormMode, ElicitationSchema,
            ElicitationSessionScope, StringPropertySchema,
        };
        let mut resolved = self.agent.subscribe();
        if !self.agent.pending_questions().await.iter().any(|event| matches!(event, crate::agent::AgentServiceEvent::Question { request_id: id, .. } if id == &request_id)) { return; }
        let message = questions
            .iter()
            .map(|question| {
                let options = question
                    .options
                    .iter()
                    .map(|option| format!("- {}: {}", option.label, option.description))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("{}\n{}", question.question, options)
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let schema = ElicitationSchema::new().property(
            "answer",
            StringPropertySchema::new().title("Your answer"),
            true,
        );
        let request = CreateElicitationRequest::new(
            ElicitationFormMode::new(ElicitationSessionScope::new(session_id), schema),
            message,
        );
        let sent = cx.send_request(request);
        let response = sent.block_task();
        tokio::pin!(response);
        loop {
            tokio::select! {
                _ = self.lifetime.cancelled() => return,
                event = resolved.recv() => match event {
                    Ok(crate::agent::AgentServiceEvent::QuestionResolved { request_id: id }) if id == request_id => return,
                    Err(_) => return,
                    _ => {},
                },
                result = &mut response => {
                    if let Ok(result) = result {
                        match result.action {
                            ElicitationAction::Accept(action) => {
                                if let Some(ElicitationContentValue::String(answer)) = action.content.as_ref().and_then(|content| content.get("answer")) {
                                    let _ = self.agent.answer_question_via_handle(&request_id, answer.clone()).await;
                                }
                            }
                            ElicitationAction::Decline => { let _ = self.agent.answer_question_via_handle(&request_id, "The user declined to answer.".into()).await; }
                            _ => {}, // Closing a form is not a denial on other interfaces.
                        }
                    }
                    return;
                }
            }
        }
    }
}
