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
//! differ. `body_matches_pinned_goose_system_prompt_byte_for_byte` compares
//! this template against goose's compile-time-embedded stock `system.md` and
//! fails on any such drift until the Maple header is re-applied.

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

# Response Guidelines

Use Markdown formatting for all responses.
"#;

#[cfg(test)]
mod tests {
    use super::MAPLE_SYSTEM_PROMPT_TEMPLATE;
    use goose::agents::prompt_manager::PromptManager;
    use goose::prompt_template::get_template;

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

    /// Everything after the two-line identity header.
    fn template_body(template: &str) -> &str {
        template
            .splitn(3, '\n')
            .nth(2)
            .expect("template must have a two-line identity header plus a body")
    }

    #[test]
    fn body_matches_pinned_goose_system_prompt_byte_for_byte() {
        // goose's get_template embeds the prompts directory at compile time,
        // so default_content is the exact pinned upstream system.md and is
        // unaffected by any on-disk user prompt override.
        let stock = get_template("system.md")
            .expect("the pinned goose crate ships system.md")
            .default_content;
        assert_eq!(
            template_body(MAPLE_SYSTEM_PROMPT_TEMPLATE),
            template_body(&stock),
            "Maple's system prompt drifted from the pinned goose system.md; \
             re-diff the template and re-apply only the two-line Maple header"
        );
    }

    #[test]
    fn template_has_maple_branding_without_upstream_identity() {
        // The goose turn-context block (which mentions goose machinery) comes
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
    }
}
