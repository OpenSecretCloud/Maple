//! Maple theme tokens for the gpui frontend.
//! Values measured from the Tauri app's dark theme (see docs/maple-theme-spec.md).
//! All constants are `u32` hex literals consumed by `gpui::rgb` (opaque).

pub const BG_APP: u32 = 0x0a0a0a;
pub const BG_SIDEBAR: u32 = 0x262626;
pub const BG_ELEVATED: u32 = 0x171717;
pub const BG_SIDEBAR_CARD: u32 = 0x303030;
pub const BG_SIDEBAR_PILL: u32 = 0x1c1c1c;
pub const BG_INPUT: u32 = 0x121212;
pub const BG_USER_BUBBLE: u32 = 0x171717;
pub const BG_CODE_BLOCK: u32 = 0x0d0d0d;
pub const BG_TOOL_CARD: u32 = 0x121212;

pub const BORDER: u32 = 0x2f2f37;
pub const BORDER_SUBTLE: u32 = 0x232323;

pub const TEXT_PRIMARY: u32 = 0xfafafa;
pub const TEXT_SECONDARY: u32 = 0xa3a3a3;
pub const TEXT_MUTED: u32 = 0x666666;
pub const TEXT_FAINT: u32 = 0x4a4a4a;

/// Maple coral: send button, focus, caret, permission prompts.
pub const ACCENT: u32 = 0xff9771;
pub const ACCENT_HOVER: u32 = 0xe77040;

pub const STATUS_RUNNING: u32 = 0xa3a3a3;
pub const STATUS_SUCCESS: u32 = 0x87a253;
pub const STATUS_ERROR: u32 = 0xd05e41;
pub const STATUS_WARNING: u32 = 0xce994b;

pub const CODE_TEXT: u32 = 0xe8e8e8;
pub const LINK: u32 = 0xb7b7b7;

/// Permission card fill/border from the Maple spec.
pub const PERMISSION_FILL: u32 = 0x191210;
pub const PERMISSION_BORDER: u32 = 0x784a38;

pub const USER_BUBBLE_BORDER: u32 = 0x262626;
