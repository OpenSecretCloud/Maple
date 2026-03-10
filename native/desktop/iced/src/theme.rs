use iced::{color, Border, Color};

// Maple (Primary) - "Primary brand energy"
pub const MAPLE_50: Color = color!(0xFFF4F0);
pub const MAPLE_100: Color = color!(0xFFE8E0);
pub const MAPLE_200: Color = color!(0xFFD1C1);
pub const MAPLE_300: Color = color!(0xFFBAA2);
pub const MAPLE_400: Color = color!(0xFFA88A);
pub const MAPLE_500: Color = color!(0xFF9771);
pub const MAPLE_600: Color = color!(0xF67D57);
pub const MAPLE_700: Color = color!(0xE8633D);
pub const MAPLE_800: Color = color!(0xD04926);
pub const MAPLE_900: Color = color!(0xA83515);

// Pebble (Secondary) - "Ethereal balance"
pub const PEBBLE_50: Color = color!(0xF7F7F9);
pub const PEBBLE_100: Color = color!(0xE8E8ED);
pub const PEBBLE_200: Color = color!(0xD1D2DC);
pub const PEBBLE_300: Color = color!(0xBABCCB);
pub const PEBBLE_400: Color = color!(0x9C9DAB);
pub const PEBBLE_500: Color = color!(0x8A8B9A);
pub const PEBBLE_600: Color = color!(0x757689);
pub const PEBBLE_700: Color = color!(0x5E5F6E);
pub const PEBBLE_800: Color = color!(0x474854);
pub const PEBBLE_900: Color = color!(0x30313A);

// Bark (Tertiary) - "Grounded structure"
pub const BARK_50: Color = color!(0xF8F5F4);
pub const BARK_100: Color = color!(0xEADED9);
pub const BARK_200: Color = color!(0xD4BCAF);
pub const BARK_300: Color = color!(0xC29A8D);
pub const BARK_400: Color = color!(0xB0877C);
pub const BARK_500: Color = color!(0x9E7469);
pub const BARK_600: Color = color!(0x8A6055);
pub const BARK_700: Color = color!(0x704D43);
pub const BARK_800: Color = color!(0x583A32);
pub const BARK_900: Color = color!(0x3D2821);

// Grove (Tertiary) - "Organic calming"
pub const GROVE_50: Color = color!(0xF7F6F0);
pub const GROVE_100: Color = color!(0xE8E4D4);
pub const GROVE_200: Color = color!(0xD3CCB0);
pub const GROVE_300: Color = color!(0xBEB48C);
pub const GROVE_400: Color = color!(0xAEA375);
pub const GROVE_500: Color = color!(0x9E925E);
pub const GROVE_600: Color = color!(0x8A7F4C);
pub const GROVE_700: Color = color!(0x726B3C);
pub const GROVE_800: Color = color!(0x5A542D);
pub const GROVE_900: Color = color!(0x3F3B1F);

// Neutral - "Focus and clarity"
pub const NEUTRAL_0: Color = color!(0xFAFAFA);
pub const NEUTRAL_50: Color = color!(0xF5F5F5);
pub const NEUTRAL_100: Color = color!(0xE5E5E5);
pub const NEUTRAL_200: Color = color!(0xD4D4D4);
pub const NEUTRAL_300: Color = color!(0xA3A3A3);
pub const NEUTRAL_400: Color = color!(0x737373);
pub const NEUTRAL_500: Color = color!(0x525252);
pub const NEUTRAL_600: Color = color!(0x404040);
pub const NEUTRAL_700: Color = color!(0x262626);
pub const NEUTRAL_800: Color = color!(0x171717);
pub const NEUTRAL_900: Color = color!(0x0A0A0A);
pub const WHITE: Color = color!(0xFFFFFF);

pub const DARK_BACKGROUND: Color = color!(0x1A110E);
pub const DARK_SURFACE_LOW: Color = color!(0x231A16);
pub const DARK_SURFACE: Color = color!(0x271D1A);
pub const DARK_SURFACE_HIGH: Color = color!(0x322824);
pub const DARK_SURFACE_HIGHEST: Color = color!(0x3D322F);
pub const DARK_OUTLINE: Color = color!(0x53433E);
pub const DARK_ON_SURFACE: Color = color!(0xF1DFD9);
pub const DARK_ON_SURFACE_VARIANT: Color = color!(0xD8C2BB);

// Semantic States
pub const MAPLE_SUCCESS: Color = color!(0x7B8F4A);
pub const MAPLE_WARNING: Color = color!(0xD4A35A);
pub const MAPLE_ERROR: Color = color!(0xD05E41);
pub const MAPLE_INFO: Color = color!(0x7E8DA1);

// Spacing
pub const SPACE_XS: f32 = 8.0;
pub const SPACE_SM: f32 = 12.0;
pub const SPACE_MD: f32 = 20.0;
pub const SPACE_LG: f32 = 36.0;
pub const SPACE_XL: f32 = 56.0;
pub const SPACE_XXL: f32 = 88.0;

// Border Radius
pub const RADIUS_SM: f32 = 8.0;
pub const RADIUS_MD: f32 = 12.0;
pub const RADIUS_LG: f32 = 16.0;
pub const RADIUS_XL: f32 = 24.0;
pub const RADIUS_CARD: f32 = 44.0;
pub const RADIUS_FULL: f32 = 999.0;

pub fn primary_button_style(
    _theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let (bg, opacity) = match status {
        iced::widget::button::Status::Active => (MAPLE_500, 1.0),
        iced::widget::button::Status::Hovered => (MAPLE_600, 1.0),
        iced::widget::button::Status::Pressed => (MAPLE_700, 1.0),
        iced::widget::button::Status::Disabled => (MAPLE_300, 0.5),
    };
    iced::widget::button::Style {
        background: Some(iced::Background::Color(Color { a: opacity, ..bg })),
        text_color: WHITE,
        border: Border {
            radius: RADIUS_FULL.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn secondary_button_style(
    _theme: &iced::Theme,
    status: iced::widget::button::Status,
    dark_mode: bool,
) -> iced::widget::button::Style {
    if dark_mode {
        let (bg_alpha, border_color, text_color, opacity) = match status {
            iced::widget::button::Status::Active => (0.96, DARK_OUTLINE, DARK_ON_SURFACE, 1.0),
            iced::widget::button::Status::Hovered => (
                0.98,
                Color {
                    a: 0.9,
                    ..DARK_ON_SURFACE_VARIANT
                },
                WHITE,
                1.0,
            ),
            iced::widget::button::Status::Pressed => (
                1.0,
                Color {
                    a: 0.95,
                    ..DARK_ON_SURFACE_VARIANT
                },
                WHITE,
                1.0,
            ),
            iced::widget::button::Status::Disabled => (
                0.72,
                Color {
                    a: 0.5,
                    ..DARK_OUTLINE
                },
                DARK_ON_SURFACE_VARIANT,
                0.5,
            ),
        };
        return iced::widget::button::Style {
            background: Some(iced::Background::Color(Color {
                a: bg_alpha,
                ..DARK_SURFACE_HIGH
            })),
            text_color: Color {
                a: opacity,
                ..text_color
            },
            border: Border {
                radius: RADIUS_FULL.into(),
                width: 1.0,
                color: border_color,
            },
            ..Default::default()
        };
    }

    let (bg_alpha, border_color, text_color, opacity) = match status {
        iced::widget::button::Status::Active => (
            0.12,
            Color {
                a: 0.2,
                ..PEBBLE_300
            },
            PEBBLE_700,
            1.0,
        ),
        iced::widget::button::Status::Hovered => (
            0.18,
            Color {
                a: 0.3,
                ..PEBBLE_400
            },
            PEBBLE_800,
            1.0,
        ),
        iced::widget::button::Status::Pressed => (
            0.22,
            Color {
                a: 0.35,
                ..PEBBLE_500
            },
            PEBBLE_900,
            1.0,
        ),
        iced::widget::button::Status::Disabled => (
            0.06,
            Color {
                a: 0.1,
                ..NEUTRAL_200
            },
            PEBBLE_400,
            0.5,
        ),
    };
    iced::widget::button::Style {
        background: Some(iced::Background::Color(Color {
            a: bg_alpha,
            ..PEBBLE_100
        })),
        text_color: Color {
            a: opacity,
            ..text_color
        },
        border: Border {
            radius: RADIUS_FULL.into(),
            width: 1.0,
            color: border_color,
        },
        ..Default::default()
    }
}

pub fn ghost_button_style(
    _theme: &iced::Theme,
    status: iced::widget::button::Status,
    dark_mode: bool,
) -> iced::widget::button::Style {
    let text_color = match (dark_mode, status) {
        (true, iced::widget::button::Status::Active) => DARK_ON_SURFACE_VARIANT,
        (true, iced::widget::button::Status::Hovered) => DARK_ON_SURFACE,
        (true, iced::widget::button::Status::Pressed) => WHITE,
        (true, iced::widget::button::Status::Disabled) => Color {
            a: 0.45,
            ..DARK_ON_SURFACE_VARIANT
        },
        (false, iced::widget::button::Status::Active) => PEBBLE_500,
        (false, iced::widget::button::Status::Hovered) => PEBBLE_700,
        (false, iced::widget::button::Status::Pressed) => PEBBLE_800,
        (false, iced::widget::button::Status::Disabled) => NEUTRAL_400,
    };
    iced::widget::button::Style {
        background: None,
        text_color,
        border: Border::default(),
        ..Default::default()
    }
}

pub fn text_input_style(
    _theme: &iced::Theme,
    status: iced::widget::text_input::Status,
    dark_mode: bool,
) -> iced::widget::text_input::Style {
    if dark_mode {
        let border_color = match status {
            iced::widget::text_input::Status::Active => Color {
                a: 0.45,
                ..DARK_ON_SURFACE_VARIANT
            },
            iced::widget::text_input::Status::Hovered => Color {
                a: 0.7,
                ..DARK_ON_SURFACE_VARIANT
            },
            iced::widget::text_input::Status::Focused { is_hovered: _ } => MAPLE_500,
            iced::widget::text_input::Status::Disabled => Color {
                a: 0.3,
                ..DARK_OUTLINE
            },
        };
        return iced::widget::text_input::Style {
            background: iced::Background::Color(Color {
                a: 0.96,
                ..DARK_SURFACE_LOW
            }),
            border: Border {
                radius: RADIUS_MD.into(),
                width: 1.0,
                color: border_color,
            },
            icon: DARK_ON_SURFACE_VARIANT,
            placeholder: Color {
                a: 0.75,
                ..DARK_ON_SURFACE_VARIANT
            },
            value: DARK_ON_SURFACE,
            selection: Color {
                a: 0.2,
                ..MAPLE_500
            },
        };
    }

    let border_color = match status {
        iced::widget::text_input::Status::Active => NEUTRAL_300,
        iced::widget::text_input::Status::Hovered => PEBBLE_400,
        iced::widget::text_input::Status::Focused { is_hovered: _ } => MAPLE_500,
        iced::widget::text_input::Status::Disabled => NEUTRAL_200,
    };
    iced::widget::text_input::Style {
        background: iced::Background::Color(NEUTRAL_0),
        border: Border {
            radius: RADIUS_MD.into(),
            width: 1.0,
            color: border_color,
        },
        icon: NEUTRAL_600,
        placeholder: NEUTRAL_400,
        value: NEUTRAL_900,
        selection: Color {
            a: 0.2,
            ..MAPLE_500
        },
    }
}

pub fn user_bubble_style(_theme: &iced::Theme, dark_mode: bool) -> iced::widget::container::Style {
    let bg = if dark_mode {
        Color {
            a: 0.96,
            ..DARK_SURFACE_HIGH
        }
    } else {
        PEBBLE_100
    };
    iced::widget::container::Style {
        background: Some(iced::Background::Color(bg)),
        border: Border {
            radius: RADIUS_LG.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn header_style(_theme: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(NEUTRAL_0)),
        border: Border {
            radius: 0.0.into(),
            width: 0.0,
            color: NEUTRAL_200,
        },
        ..Default::default()
    }
}

pub fn branded_header_style(_theme: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(Color {
            a: 0.88,
            ..PEBBLE_800
        })),
        border: Border {
            width: 0.0,
            color: Color {
                a: 0.1,
                ..PEBBLE_600
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn divider_style(_theme: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(NEUTRAL_200)),
        ..Default::default()
    }
}

pub fn toast_container_style(
    _theme: &iced::Theme,
    dark_mode: bool,
) -> iced::widget::container::Style {
    let background = if dark_mode {
        Color {
            a: 0.92,
            ..DARK_SURFACE
        }
    } else {
        Color { a: 0.94, ..WHITE }
    };

    iced::widget::container::Style {
        background: Some(iced::Background::Color(background)),
        border: Border {
            radius: RADIUS_FULL.into(),
            width: 1.0,
            color: Color {
                a: if dark_mode { 0.35 } else { 0.18 },
                ..MAPLE_ERROR
            },
        },
        shadow: iced::Shadow {
            color: Color {
                a: if dark_mode { 0.18 } else { 0.08 },
                ..Color::BLACK
            },
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: if dark_mode { 16.0 } else { 10.0 },
        },
        ..Default::default()
    }
}
