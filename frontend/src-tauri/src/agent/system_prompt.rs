//! Maple-branded system prompt for Agent Mode.
//!
//! The pinned goose runtime identifies itself as "goose, created by AAIF
//! (Agentic AI Foundation)". That identity is wrong for Maple users: they
//! interact with Maple's Agent Mode and do not know what goose is.
//! [`MAPLE_SYSTEM_PROMPT_TEMPLATE`] is a line-for-line copy of the pinned
//! goose `crates/goose/src/prompts/system.md` with only the two-line identity
//! header rebranded. Every dynamic section (turn context, extensions,
//! tool-count suggestion, response guidelines) is preserved byte-for-byte so
//! the rendered prompt keeps goose's exact structure and prompt-cache
//! stability.
//!
//! When bumping the goose pin in `Cargo.toml`, diff this template against the
//! pinned `crates/goose/src/prompts/system.md`; only the first two lines may
//! differ. `template_keeps_stock_structure_and_maple_branding` enforces that
//! contract.

/// System prompt template applied to every freshly created Agent Mode agent
/// via goose's `Agent::override_system_prompt`.
pub(crate) const MAPLE_SYSTEM_PROMPT_TEMPLATE: &str = r#"You are a general-purpose AI agent called Maple, created by Maple AI.
You run in the Maple app's Agent Mode; users know you simply as Maple.

{% if moim_system_prompt_block is defined %}
{{ moim_system_prompt_block }}
{% endif %}

{% if include_extensions and not code_execution_mode %}

# Extensions

Extensions provide additional tools and context from different data sources and applications.
You can dynamically enable or disable extensions as needed to help complete tasks.

{% if (extensions is defined) and extensions %}
Because you dynamically load extensions, your conversation history may refer
to interactions with extensions that are not currently active. The currently
active extensions are below. Each of these extensions provides tools that are
in your tool specification.

{% for extension in extensions %}

## {{extension.name}}

{% if extension.has_resources %}
{{extension.name}} supports resources.
{% endif %}
{% if extension.instructions %}### Instructions
{{extension.instructions}}{% endif %}
{% endfor %}

{% else %}
No extensions are defined. You should let the user know that they should add extensions.
{% endif %}
{% endif %}

{% if include_extensions and extension_tool_limits is defined and not code_execution_mode %}
{% with (extension_count, tool_count) = extension_tool_limits  %}
# Suggestion

The user has {{extension_count}} extensions with {{tool_count}} tools enabled, exceeding recommended limits ({{max_extensions}} extensions or {{max_tools}} tools).
Consider asking if they'd like to disable some extensions to improve tool selection accuracy.
{% endwith %}
{% endif %}

# Response Guidelines

Use Markdown formatting for all responses.
"#;

#[cfg(test)]
mod tests {
    use super::MAPLE_SYSTEM_PROMPT_TEMPLATE;
    use goose::agents::prompt_manager::PromptManager;

    #[test]
    fn renders_maple_identity_with_stock_structure() {
        // Render through goose's real PromptManager so a template that
        // upstream's mini template engine rejects fails loudly here.
        let mut manager = PromptManager::new();
        manager.set_system_prompt_override(MAPLE_SYSTEM_PROMPT_TEMPLATE.to_string());
        let rendered = manager.builder().build();

        assert!(
            rendered.starts_with(
                "You are a general-purpose AI agent called Maple, created by Maple AI."
            ),
            "rendered prompt lost the Maple identity header: {rendered}"
        );
        for section in [
            "# Turn Context",
            "# Extensions",
            "# Response Guidelines",
            "Use Markdown formatting for all responses.",
        ] {
            assert!(
                rendered.contains(section),
                "rendered prompt lost stock goose section {section:?}: {rendered}"
            );
        }
    }

    #[test]
    fn template_keeps_stock_structure_and_maple_branding() {
        // Branding: Maple identity only, no upstream identity leaks. The
        // goose turn-context block (which mentions goose machinery) comes
        // from goose at render time and is intentionally out of scope here. The
        // stock template variable `moim_system_prompt_block` must stay intact,
        // so blank it out before scanning for the "Block" creator brand.
        let template = MAPLE_SYSTEM_PROMPT_TEMPLATE.replace("moim_system_prompt_block", "turnctx");
        let template = template.to_lowercase();
        for leak in ["goose", "aaif", "block"] {
            assert!(
                !template.contains(leak),
                "template leaks upstream brand {leak:?}"
            );
        }

        // Structure: every dynamic block of the pinned stock
        // crates/goose/src/prompts/system.md must survive in byte-exact form.
        for marker in [
            "{% if moim_system_prompt_block is defined %}",
            "{{ moim_system_prompt_block }}",
            "{% if include_extensions and not code_execution_mode %}",
            "# Extensions",
            "{% for extension in extensions %}",
            "## {{extension.name}}",
            "{% if extension.has_resources %}",
            "{{extension.instructions}}{% endif %}",
            "No extensions are defined. You should let the user know that they should add extensions.",
            "{% if include_extensions and extension_tool_limits is defined and not code_execution_mode %}",
            "{% with (extension_count, tool_count) = extension_tool_limits  %}",
            "# Suggestion",
            "# Response Guidelines",
            "Use Markdown formatting for all responses.",
        ] {
            assert!(
                MAPLE_SYSTEM_PROMPT_TEMPLATE.contains(marker),
                "template drifted from the pinned stock goose system.md: missing {marker:?}"
            );
        }
    }
}
