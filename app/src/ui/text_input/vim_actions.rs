//! GPUI transport for the composer-local Vim engine.
//!
//! The pure engine in [`super::vim`] never sees physical keys. These typed
//! actions are the temporary fixed keymap for the focused composer; the
//! shortcut-customization layer can later replace the bindings without
//! changing editor behavior.

use gpui::{Context, Div, InteractiveElement, actions};

use super::TextInput;
use super::vim::{
    ContextualToken, InsertEntry, Motion, OpenLinePlacement, Operator, PastePlacement,
};

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = composer_vim, no_json)]
pub struct VimMotion {
    pub motion: Motion,
}

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = composer_vim, no_json)]
pub struct VimBeginOperator {
    pub operator: Operator,
}

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = composer_vim, no_json)]
pub struct VimCountDigit {
    pub digit: u8,
}

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = composer_vim, no_json)]
pub struct VimContextual {
    pub token: ContextualToken,
}

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = composer_vim, no_json)]
pub struct VimEnterInsert {
    pub placement: InsertEntry,
}

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = composer_vim, no_json)]
pub struct VimOpenLine {
    pub placement: OpenLinePlacement,
}

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = composer_vim, no_json)]
pub struct VimPaste {
    pub placement: PastePlacement,
}

actions!(
    composer_vim,
    [
        VimToggleVisual,
        VimDeleteChars,
        VimUndo,
        VimRedo,
        VimRepeat,
        VimCancel,
    ]
);

pub const NORMAL_CONTEXT: &str = "TextInput && input_role == composer && editor_vim_mode == normal";
pub const VISUAL_CONTEXT: &str = "TextInput && input_role == composer && editor_vim_mode == visual";
pub const INSERT_CONTEXT: &str = "TextInput && input_role == composer && editor_vim_mode == insert";

/// Attach every composer-Vim action at the TextInput boundary. Keeping this
/// list behind one call site makes the optional editor layer straightforward
/// to remove without disturbing ordinary text-input actions.
pub(super) fn attach_actions(element: Div, cx: &mut Context<TextInput>) -> Div {
    element
        .on_action(cx.listener(TextInput::vim_motion))
        .on_action(cx.listener(TextInput::vim_begin_operator))
        .on_action(cx.listener(TextInput::vim_count_digit))
        .on_action(cx.listener(TextInput::vim_contextual))
        .on_action(cx.listener(TextInput::vim_enter_insert))
        .on_action(cx.listener(TextInput::vim_open_line))
        .on_action(cx.listener(TextInput::vim_toggle_visual))
        .on_action(cx.listener(TextInput::vim_delete_chars))
        .on_action(cx.listener(TextInput::vim_paste))
        .on_action(cx.listener(TextInput::vim_undo))
        .on_action(cx.listener(TextInput::vim_redo))
        .on_action(cx.listener(TextInput::vim_repeat))
        .on_action(cx.listener(TextInput::vim_cancel))
}
