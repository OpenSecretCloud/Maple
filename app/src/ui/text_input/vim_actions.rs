//! GPUI transport for the composer-local Vim engine.
//!
//! The pure engine in [`super::vim`] never sees physical keys. These typed
//! actions are the temporary fixed keymap for the focused composer; the
//! shortcut-customization layer can later replace the bindings without
//! changing editor behavior.

use gpui::{App, Context, Div, InteractiveElement, KeyBinding, actions};

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

/// Install the fixed composer-only map after the ordinary TextInput map.
/// The mode predicates are more specific, so ordinary inputs remain exactly
/// on the existing bindings and Insert retains platform editing shortcuts.
pub fn register_key_bindings(cx: &mut App) {
    let mut bindings = Vec::new();

    for context in [NORMAL_CONTEXT, VISUAL_CONTEXT] {
        for (key, motion) in [
            ("h", Motion::Left),
            ("j", Motion::Down),
            ("k", Motion::Up),
            ("l", Motion::Right),
            ("b", Motion::WordBackward),
            ("e", Motion::WordEnd),
            ("$", Motion::LineEnd),
            ("g g", Motion::FirstLine),
            ("G", Motion::LastLine),
            ("left", Motion::Left),
            ("down", Motion::Down),
            ("up", Motion::Up),
            ("right", Motion::Right),
        ] {
            bindings.push(KeyBinding::new(key, VimMotion { motion }, Some(context)));
        }
        bindings.push(KeyBinding::new(
            "w",
            VimContextual {
                token: ContextualToken::WordOrTextObject,
            },
            Some(context),
        ));
        bindings.push(KeyBinding::new(
            "i",
            VimContextual {
                token: ContextualToken::InnerOrInsert,
            },
            Some(context),
        ));
        bindings.push(KeyBinding::new(
            "a",
            VimContextual {
                token: ContextualToken::AroundOrAppend,
            },
            Some(context),
        ));
        for (key, operator) in [
            ("d", Operator::Delete),
            ("c", Operator::Change),
            ("y", Operator::Yank),
        ] {
            bindings.push(KeyBinding::new(
                key,
                VimBeginOperator { operator },
                Some(context),
            ));
        }
        for digit in 0..=9 {
            bindings.push(KeyBinding::new(
                &digit.to_string(),
                VimCountDigit { digit },
                Some(context),
            ));
        }
        bindings.push(KeyBinding::new("v", VimToggleVisual, Some(context)));
        bindings.push(KeyBinding::new("x", VimDeleteChars, Some(context)));
        bindings.push(KeyBinding::new(
            "p",
            VimPaste {
                placement: PastePlacement::After,
            },
            Some(context),
        ));
        bindings.push(KeyBinding::new(
            "P",
            VimPaste {
                placement: PastePlacement::Before,
            },
            Some(context),
        ));
    }

    for (key, placement) in [
        ("I", InsertEntry::FirstNonWhitespace),
        ("A", InsertEntry::LineEnd),
    ] {
        bindings.push(KeyBinding::new(
            key,
            VimEnterInsert { placement },
            Some(NORMAL_CONTEXT),
        ));
    }
    for (key, placement) in [
        ("o", OpenLinePlacement::Below),
        ("O", OpenLinePlacement::Above),
    ] {
        bindings.push(KeyBinding::new(
            key,
            VimOpenLine { placement },
            Some(NORMAL_CONTEXT),
        ));
    }
    bindings.push(KeyBinding::new("u", VimUndo, Some(NORMAL_CONTEXT)));
    bindings.push(KeyBinding::new("ctrl-r", VimRedo, Some(NORMAL_CONTEXT)));
    bindings.push(KeyBinding::new(".", VimRepeat, Some(NORMAL_CONTEXT)));
    bindings.push(KeyBinding::new("escape", VimCancel, Some(NORMAL_CONTEXT)));
    bindings.push(KeyBinding::new("escape", VimCancel, Some(VISUAL_CONTEXT)));
    bindings.push(KeyBinding::new("escape", VimCancel, Some(INSERT_CONTEXT)));

    cx.bind_keys(bindings);
}
