//! The composer previews and changes the runtime's persisted thinking level.

use super::*;

impl ChatScreen {
    pub(super) fn thinking_menu(&self) -> Menu<Self> {
        let mut menu = Menu::new("thinking-menu", px(220.))
            .label("Thinking")
            .application_vim(self.application_vim_enabled);
        if let Some(state) = &self.thinking {
            menu = menu.items(state.levels.iter().copied().map(|level| {
                MenuItem::new(
                    SharedString::from(format!("thinking-{}", level.as_str())),
                    level.as_str(),
                    move |this: &mut Self, _: &mut Window, cx: &mut Context<Self>| {
                        this.pick_thinking(level, cx);
                    },
                )
                .current(state.current == level)
                .enabled(!self.thinking_saving)
            }));
        }
        menu
    }

    /// Drop responses for a task or model that is no longer selected, or
    /// for a read that started before the latest pick.
    pub(super) fn refresh_thinking(&mut self, cx: &mut Context<Self>) {
        self.thinking_generation += 1;
        self.thinking = None;
        let generation = self.thinking_generation;
        let Some(model) = self.selected_model.clone() else {
            return;
        };
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let session = self.selected_session.clone();
        self.call(
            async move {
                backend
                    .thinking_state(&user_id, session.as_deref(), &model)
                    .await
            },
            cx,
            move |this, result, cx| {
                if this.thinking_generation == generation {
                    match result {
                        Ok(state) => this.thinking = Some(state),
                        Err(error) => log::warn!("Failed to load thinking controls: {error}"),
                    }
                    cx.notify();
                }
            },
        );
    }

    fn pick_thinking(&mut self, level: ThinkingLevel, cx: &mut Context<Self>) {
        self.popup.close(cx);
        if self.thinking_saving
            || !self
                .thinking
                .as_ref()
                .is_some_and(|state| state.levels.contains(&level))
        {
            return;
        }
        let Some(model) = self.selected_model.clone() else {
            return;
        };
        self.thinking_generation += 1;
        self.thinking_saving = true;
        let generation = self.thinking_generation;
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let session = self.selected_session.clone();
        self.call(
            async move {
                backend
                    .set_thinking_level(&user_id, session.as_deref(), &model, level)
                    .await
            },
            cx,
            move |this, result, cx| {
                this.thinking_saving = false;
                match result {
                    Ok(state) if this.thinking_generation == generation => {
                        this.thinking = Some(state)
                    }
                    Ok(_) => this.refresh_thinking(cx),
                    Err(error) => {
                        this.notice =
                            Some(format!("Could not save the thinking level: {error}").into());
                        this.refresh_thinking(cx);
                    }
                }
                cx.notify();
            },
        );
    }
}
