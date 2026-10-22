//! MCP editor state contains names and presence only until the user enters a
//! replacement. Keep, Set and Clear remain distinct at the request boundary.

use gpui::{Context, Div, Entity, div, prelude::*};
use maple_agent::store::records::{
    McpDefinition, McpSecretEdit, McpSecretUpdate, McpServerEdit, SecretString,
};
use zeroize::Zeroizing;

use super::{SettingsScreen, TextInput, pill_button, theme};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum McpSecretAction {
    Keep,
    Set,
    Clear,
}

pub(super) struct McpSecretField {
    pub name: String,
    pub has_value: bool,
    pub action: McpSecretAction,
    pub input: Entity<TextInput>,
}

impl SettingsScreen {
    pub(super) fn render_mcp_secret_fields(
        &self,
        fields: &[McpSecretField],
        headers: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let remote = self.integrations_backend.remote_host().is_some();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .children(fields.iter().enumerate().map(|(index, field)| {
                let mut row = div().flex().flex_col().gap_1().child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().flex_1().text_sm().child(field.name.clone()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(gpui::rgb(theme::text_muted()))
                                .child(match field.action {
                                    McpSecretAction::Keep if field.has_value => "Saved value",
                                    McpSecretAction::Keep => "No saved value",
                                    McpSecretAction::Set => "New value",
                                    McpSecretAction::Clear => "Will clear",
                                }),
                        ),
                );
                if !remote {
                    row = row.child(
                        div().flex().gap_2().children(
                            [
                                (McpSecretAction::Keep, "Keep"),
                                (McpSecretAction::Set, "Replace"),
                                (McpSecretAction::Clear, "Clear"),
                            ]
                            .into_iter()
                            .map(|(action, label)| {
                                pill_button(
                                    format!(
                                        "mcp-secret-{}-{index}-{label}",
                                        if headers { "header" } else { "env" }
                                    ),
                                    label,
                                    field.action == action,
                                    cx.listener(move |this, _event, _window, cx| {
                                        if let Some(editor) = &mut this.mcp_editor {
                                            let fields = if headers {
                                                &mut editor.saved_headers
                                            } else {
                                                &mut editor.saved_environment
                                            };
                                            if let Some(field) = fields.get_mut(index) {
                                                field.action = action;
                                                if action != McpSecretAction::Set {
                                                    field
                                                        .input
                                                        .update(cx, |input, cx| input.clear(cx));
                                                }
                                            }
                                        }
                                        cx.notify();
                                    }),
                                )
                            }),
                        ),
                    );
                }
                if field.action == McpSecretAction::Set {
                    row = row.child(field.input.clone());
                }
                row
            }))
    }
}

pub(super) fn definition_edit(definition: &McpDefinition) -> McpServerEdit {
    let keep = |presence: &maple_agent::store::records::McpSecretPresence| McpSecretEdit {
        name: presence.name.clone(),
        update: McpSecretUpdate::Keep,
    };
    McpServerEdit {
        key: definition.key.clone(),
        name: definition.name.clone(),
        description: definition.description.clone(),
        enabled: definition.enabled,
        timeout_seconds: definition.timeout_seconds,
        target: definition.target.clone(),
        environment: definition.environment.iter().map(keep).collect(),
        headers: definition.headers.iter().map(keep).collect(),
    }
}

pub(super) fn name_collides(servers: &[McpDefinition], original: Option<&str>, name: &str) -> bool {
    let key = maple_agent::agent::AgentMcpServer::key_for_name(name);
    servers
        .iter()
        .any(|server| server.key == key && Some(server.name.as_str()) != original)
}

pub(super) fn field_edits(
    fields: &[McpSecretField],
    remote: bool,
    cx: &Context<SettingsScreen>,
) -> Result<Vec<McpSecretEdit>, String> {
    fields
        .iter()
        .map(|field| {
            if remote && field.action != McpSecretAction::Keep {
                return Err("Saved values can be changed on the host computer.".into());
            }
            let update = match field.action {
                McpSecretAction::Keep => McpSecretUpdate::Keep,
                McpSecretAction::Clear => McpSecretUpdate::Clear,
                McpSecretAction::Set => {
                    let value = Zeroizing::new(field.input.read(cx).text());
                    if value.is_empty() {
                        return Err(format!(
                            "Enter a new value for {} or choose Clear.",
                            field.name
                        ));
                    }
                    McpSecretUpdate::Set(SecretString::new(value.to_string()))
                }
            };
            Ok(McpSecretEdit {
                name: field.name.clone(),
                update,
            })
        })
        .collect()
}

/// New entries always mean Set. Blank fields mean no new entries; malformed
/// entries never disappear silently and errors never quote the supplied value.
pub(super) fn new_value_edits(text: &str, remote: bool) -> Result<Vec<McpSecretEdit>, String> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    if remote {
        return Err("New values can be entered on the host computer.".into());
    }
    text.split(';')
        .filter(|entry| !entry.trim().is_empty())
        .map(|entry| {
            let (name, value) = entry.trim().split_once('=').ok_or_else(|| {
                "Enter new values as NAME=value, separated by semicolons.".to_string()
            })?;
            if name.trim().is_empty() || value.is_empty() {
                return Err("Each new entry needs a name and value.".into());
            }
            Ok(McpSecretEdit {
                name: name.trim().to_string(),
                update: McpSecretUpdate::Set(SecretString::new(value.to_owned())),
            })
        })
        .collect()
}

pub(super) fn require_target_choice(
    original: Option<&McpDefinition>,
    edit: &McpServerEdit,
) -> Result<(), String> {
    let Some(original) = original else {
        return Ok(());
    };
    if original.target == edit.target && original.key == edit.key {
        return Ok(());
    }
    for (saved, edits) in [
        (&original.environment, &edit.environment),
        (&original.headers, &edit.headers),
    ] {
        if saved
            .iter()
            .filter(|presence| presence.has_value)
            .any(|presence| {
                edits
                    .iter()
                    .find(|entry| entry.name.eq_ignore_ascii_case(&presence.name))
                    .is_some_and(|entry| matches!(entry.update, McpSecretUpdate::Keep))
            })
        {
            return Err("The server target or name changed. Replace or clear each saved value before saving.".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use maple_agent::store::records::{McpSecretPresence, McpTarget};

    fn saved() -> McpDefinition {
        McpDefinition {
            key: "example".into(),
            name: "Example".into(),
            description: String::new(),
            enabled: true,
            timeout_seconds: 30,
            target: McpTarget::Stdio {
                command: "example --stdio".into(),
            },
            environment: vec![McpSecretPresence {
                name: "TOKEN".into(),
                has_value: true,
            }],
            headers: vec![],
        }
    }

    #[test]
    fn metadata_echo_keeps_saved_values_without_serializing_values() {
        let definition = saved();
        let response = serde_json::to_string(&definition).unwrap();
        assert!(response.contains("hasValue"));
        assert!(!response.contains("synthetic-secret"));
        let edit = definition_edit(&definition);
        assert!(matches!(edit.environment[0].update, McpSecretUpdate::Keep));
        assert!(require_target_choice(Some(&definition), &edit).is_ok());
    }

    #[test]
    fn changed_target_requires_explicit_replacement_or_clear() {
        let definition = saved();
        let mut edit = definition_edit(&definition);
        edit.target = McpTarget::Stdio {
            command: "different --stdio".into(),
        };
        assert!(require_target_choice(Some(&definition), &edit).is_err());
        edit.environment[0].update =
            McpSecretUpdate::Set(SecretString::new("synthetic-new".into()));
        assert!(require_target_choice(Some(&definition), &edit).is_ok());
        edit.environment[0].update = McpSecretUpdate::Clear;
        assert!(require_target_choice(Some(&definition), &edit).is_ok());
        let mut renamed = definition_edit(&definition);
        renamed.key = "renamed".into();
        assert!(require_target_choice(Some(&definition), &renamed).is_err());
    }

    #[test]
    fn remote_new_values_and_malformed_local_entries_are_rejected_without_echo() {
        assert!(new_value_edits("TOKEN=synthetic-secret", true).is_err());
        let error = new_value_edits("synthetic-secret", false).unwrap_err();
        assert!(!error.contains("synthetic-secret"));
        assert!(new_value_edits("TOKEN=", false).is_err());
        assert!(new_value_edits("", true).unwrap().is_empty());
        let edits = new_value_edits("TOKEN=synthetic-secret", false).unwrap();
        assert!(
            matches!(&edits[0].update,McpSecretUpdate::Set(value) if value.expose()=="synthetic-secret")
        );
        assert!(!format!("{edits:?}").contains("synthetic-secret"));
    }
}
