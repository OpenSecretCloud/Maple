use gtk::cairo;
use gtk::prelude::*;
use gtk4 as gtk;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Color {
    pub const fn rgb(hex: u32) -> Self {
        Self {
            r: ((hex >> 16) & 0xff) as f64 / 255.0,
            g: ((hex >> 8) & 0xff) as f64 / 255.0,
            b: (hex & 0xff) as f64 / 255.0,
            a: 1.0,
        }
    }

    pub const fn rgba(hex: u32, a: f64) -> Self {
        Self {
            a,
            ..Self::rgb(hex)
        }
    }

    pub const fn with_alpha(self, a: f64) -> Self {
        Self { a, ..self }
    }

    pub fn css_rgba(self) -> String {
        format!(
            "rgba({:.0}, {:.0}, {:.0}, {:.3})",
            self.r * 255.0,
            self.g * 255.0,
            self.b * 255.0,
            self.a
        )
    }

    pub fn pango_rgb(self) -> String {
        format!(
            "#{:02X}{:02X}{:02X}",
            (self.r * 255.0).round() as u8,
            (self.g * 255.0).round() as u8,
            (self.b * 255.0).round() as u8,
        )
    }

    pub fn set_source(self, cr: &cairo::Context) {
        cr.set_source_rgba(self.r, self.g, self.b, self.a);
    }
}

pub const MAPLE_400: Color = Color::rgb(0xFFA88A);
pub const MAPLE_500: Color = Color::rgb(0xFF9771);
pub const MAPLE_600: Color = Color::rgb(0xF67D57);
pub const MAPLE_700: Color = Color::rgb(0xE8633D);
pub const PEBBLE_50: Color = Color::rgb(0xF7F7F9);
pub const PEBBLE_100: Color = Color::rgb(0xE8E8ED);
pub const PEBBLE_300: Color = Color::rgb(0xBABCCB);
pub const PEBBLE_400: Color = Color::rgb(0x9C9DAB);
pub const PEBBLE_600: Color = Color::rgb(0x757689);
pub const PEBBLE_700: Color = Color::rgb(0x5E5F6E);
pub const PEBBLE_800: Color = Color::rgb(0x474854);
pub const BARK_300: Color = Color::rgb(0xC29A8D);
pub const NEUTRAL_200: Color = Color::rgb(0xD4D4D4);
pub const NEUTRAL_800: Color = Color::rgb(0x171717);
pub const WHITE: Color = Color::rgb(0xFFFFFF);
pub const MAPLE_ERROR: Color = Color::rgb(0xD05E41);

pub const SPACE_XS: i32 = 8;
pub const SPACE_SM: i32 = 12;
pub const SPACE_MD: i32 = 20;
pub const SPACE_LG: i32 = 36;
pub const RADIUS_MD: i32 = 12;
pub const RADIUS_XL: i32 = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundKind {
    Splash,
    Login,
    Chat,
}

#[derive(Clone, Copy, Debug)]
pub struct ThemeConfig {
    pub dark_mode: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct LoginPalette {
    pub background_base: Color,
    pub glow_stops: &'static [(f64, Color)],
    pub card_background: Color,
    pub card_highlight: Color,
    pub card_border: Color,
    pub wordmark: Color,
    pub supporting_text: Color,
    pub tertiary_text: Color,
    pub divider: Color,
    pub field_background: Color,
    pub field_border: Color,
    pub field_text: Color,
    pub secondary_button_background: Color,
    pub secondary_button_border: Color,
    pub secondary_button_foreground: Color,
}

#[derive(Clone, Copy, Debug)]
pub struct ChatPalette {
    pub background_base: Color,
    pub glow_stops: &'static [(f64, Color)],
    pub chrome_highlight: Color,
    pub chrome_background: Color,
    pub chrome_border: Color,
    pub header_wordmark: Color,
    pub secondary_icon: Color,
    pub compose_background: Color,
    pub compose_border: Color,
    pub compose_text: Color,
    pub compose_placeholder: Color,
    pub metadata_text: Color,
    pub assistant_text: Color,
    pub user_bubble_color: Color,
    pub user_text: Color,
    pub surface_text: Color,
    pub sheet_background: Color,
    pub sheet_divider: Color,
}

#[derive(Clone, Copy, Debug)]
pub struct ToastPalette {
    pub background: Color,
    pub text: Color,
    pub border: Color,
}

const SPLASH_STOPS: &[(f64, Color)] = &[
    (0.0, MAPLE_500),
    (0.48, Color::rgb(0xCE9A8E)),
    (1.0, PEBBLE_400),
];

const LOGIN_LIGHT_STOPS: &[(f64, Color)] = &[
    (0.0, MAPLE_500.with_alpha(0.18)),
    (0.52, BARK_300.with_alpha(0.10)),
    (0.78, PEBBLE_300.with_alpha(0.10)),
    (1.0, WHITE.with_alpha(0.0)),
];

const LOGIN_DARK_STOPS: &[(f64, Color)] = &[
    (0.0, MAPLE_500.with_alpha(0.07)),
    (0.42, Color::rgba(0x5D4036, 0.11)),
    (0.76, PEBBLE_800.with_alpha(0.12)),
    (1.0, WHITE.with_alpha(0.0)),
];

const CHAT_LIGHT_STOPS: &[(f64, Color)] = &[
    (0.0, Color::rgba(0xFF9771, 0.35)),
    (0.12, Color::rgba(0xECB8A5, 0.20)),
    (0.24, Color::rgba(0xDADADA, 0.10)),
    (1.0, WHITE.with_alpha(0.0)),
];

const CHAT_DARK_STOPS: &[(f64, Color)] = &[
    (0.0, MAPLE_500.with_alpha(0.07)),
    (0.42, Color::rgba(0x5D4036, 0.10)),
    (0.76, PEBBLE_800.with_alpha(0.12)),
    (1.0, WHITE.with_alpha(0.0)),
];

impl ThemeConfig {
    pub const fn new(dark_mode: bool) -> Self {
        Self { dark_mode }
    }

    pub fn login_palette(self) -> LoginPalette {
        if self.dark_mode {
            LoginPalette {
                background_base: Color::rgb(0x1A110E),
                glow_stops: LOGIN_DARK_STOPS,
                card_background: Color::rgba(0x271D1A, 0.90),
                card_highlight: WHITE.with_alpha(0.04),
                card_border: Color::rgb(0x53433E),
                wordmark: PEBBLE_50,
                supporting_text: Color::rgb(0xD8C2BB),
                tertiary_text: Color::rgba(0xD8C2BB, 0.80),
                divider: Color::rgb(0x53433E),
                field_background: Color::rgba(0x231A16, 0.96),
                field_border: Color::rgba(0xA08D86, 0.45),
                field_text: Color::rgb(0xF1DFD9),
                secondary_button_background: Color::rgba(0x322824, 0.96),
                secondary_button_border: Color::rgb(0x53433E),
                secondary_button_foreground: Color::rgb(0xF1DFD9),
            }
        } else {
            LoginPalette {
                background_base: Color::rgb(0xFBF8F6),
                glow_stops: LOGIN_LIGHT_STOPS,
                card_background: WHITE.with_alpha(0.74),
                card_highlight: WHITE.with_alpha(0.42),
                card_border: WHITE.with_alpha(0.72),
                wordmark: PEBBLE_800,
                supporting_text: PEBBLE_600,
                tertiary_text: PEBBLE_400,
                divider: NEUTRAL_200,
                field_background: WHITE.with_alpha(0.84),
                field_border: NEUTRAL_200.with_alpha(0.95),
                field_text: PEBBLE_800,
                secondary_button_background: WHITE.with_alpha(0.56),
                secondary_button_border: WHITE.with_alpha(0.68),
                secondary_button_foreground: PEBBLE_700,
            }
        }
    }

    pub fn chat_palette(self) -> ChatPalette {
        if self.dark_mode {
            ChatPalette {
                background_base: Color::rgb(0x1A110E),
                glow_stops: CHAT_DARK_STOPS,
                chrome_highlight: Color::rgba(0x271D1A, 0.78),
                chrome_background: Color::rgba(0x271D1A, 0.78),
                chrome_border: Color::rgb(0x53433E),
                header_wordmark: PEBBLE_50,
                secondary_icon: Color::rgb(0xD8C2BB),
                compose_background: Color::rgba(0x271D1A, 0.78),
                compose_border: Color::rgb(0x53433E),
                compose_text: Color::rgb(0xF1DFD9),
                compose_placeholder: Color::rgba(0xD8C2BB, 0.78),
                metadata_text: Color::rgba(0xD8C2BB, 0.82),
                assistant_text: Color::rgb(0xF1DFD9),
                user_bubble_color: Color::rgba(0x322824, 0.96),
                user_text: Color::rgb(0xF1DFD9),
                surface_text: Color::rgb(0xF1DFD9),
                sheet_background: Color::rgb(0x271D1A),
                sheet_divider: Color::rgb(0x53433E),
            }
        } else {
            ChatPalette {
                background_base: WHITE,
                glow_stops: CHAT_LIGHT_STOPS,
                chrome_highlight: WHITE.with_alpha(0.98),
                chrome_background: Color::rgba(0xFFF1EC, 0.84),
                chrome_border: WHITE.with_alpha(0.92),
                header_wordmark: PEBBLE_800,
                secondary_icon: PEBBLE_800,
                compose_background: WHITE.with_alpha(0.90),
                compose_border: WHITE.with_alpha(0.94),
                compose_text: NEUTRAL_800,
                compose_placeholder: Color::rgb(0x878787),
                metadata_text: PEBBLE_400,
                assistant_text: PEBBLE_800,
                user_bubble_color: PEBBLE_100,
                user_text: NEUTRAL_800,
                surface_text: NEUTRAL_800,
                sheet_background: WHITE.with_alpha(0.96),
                sheet_divider: NEUTRAL_200,
            }
        }
    }

    pub fn toast_palette(self) -> ToastPalette {
        if self.dark_mode {
            ToastPalette {
                background: Color::rgba(0x271D1A, 0.92),
                text: PEBBLE_50,
                border: MAPLE_ERROR.with_alpha(0.35),
            }
        } else {
            ToastPalette {
                background: WHITE.with_alpha(0.94),
                text: NEUTRAL_800,
                border: MAPLE_ERROR.with_alpha(0.18),
            }
        }
    }

    pub fn splash_wordmark(self) -> Color {
        let _ = self;
        PEBBLE_50
    }

    pub fn splash_tagline(self) -> Color {
        let _ = self;
        PEBBLE_100.with_alpha(0.80)
    }

    pub fn css(self) -> String {
        let login = self.login_palette();
        let chat = self.chat_palette();
        let toast = self.toast_palette();

        format!(
            r#"
window.maple-window,
window.maple-window > * {{
  background: transparent;
  color: {};
  font-family: "Manrope", "Cantarell", sans-serif;
}}

button.flat {{
  box-shadow: none;
}}

label.maple-splash-tagline {{
  color: {};
  font-family: "Array-Bold", "Manrope", sans-serif;
  font-weight: 700;
  font-size: 16px;
}}

box.maple-login-card-shell {{
  background-image: linear-gradient(to bottom right, {}, {});
  border: 1px solid {};
  border-radius: {}px;
}}

box.maple-entry-shell {{
  background: {};
  border: 1px solid {};
  border-radius: {}px;
}}

entry.maple-entry {{
  background: transparent;
  border: none;
  color: {};
  caret-color: {};
  font-size: 16px;
  font-weight: 500;
}}

button.maple-primary-button {{
  background-image: linear-gradient(to bottom, {}, {});
  color: {};
  border: none;
  border-radius: 999px;
  font-size: 16px;
  font-weight: 600;
}}

button.maple-primary-button:disabled {{
  opacity: 0.5;
}}

button.maple-secondary-button {{
  background: {};
  color: {};
  border: 1px solid {};
  border-radius: 999px;
  font-size: 16px;
  font-weight: 500;
}}

button.maple-secondary-button:disabled {{
  opacity: 0.6;
}}

button.maple-link-button {{
  background: transparent;
  border: none;
  color: {};
  font-size: 14px;
  font-weight: 500;
}}

label.maple-divider {{
  color: {};
}}

separator.maple-divider {{
  background: {};
}}

box.maple-header-pill,
button.maple-header-pill-button {{
  background-image: linear-gradient(to bottom right, {}, {});
  border: 1px solid {};
  border-radius: 999px;
  color: {};
}}

button.maple-header-pill-button:disabled {{
  opacity: 0.45;
}}

box.maple-compose-shell {{
  background: {};
  border: 1px solid {};
  border-radius: {}px;
}}

textview.maple-compose-view,
textview.maple-compose-view text {{
  background: transparent;
  color: {};
  caret-color: {};
  font-size: 15px;
  font-weight: 500;
}}

label.maple-compose-placeholder {{
  color: {};
  font-size: 15px;
  font-weight: 500;
}}

button.maple-plus-button {{
  background: {};
  border: none;
  border-radius: 999px;
  color: {};
  min-width: 28px;
  min-height: 28px;
  padding: 0;
  font-size: 14px;
  font-weight: 700;
}}

button.maple-send-button {{
  background-image: linear-gradient(to bottom, {}, {});
  border: none;
  border-radius: 999px;
  color: {};
  min-width: 71px;
  min-height: 36px;
  padding: 0;
  font-size: 20px;
  font-weight: 800;
}}

button.maple-send-button:disabled {{
  opacity: 0.5;
}}

label.maple-assistant-message {{
  color: {};
  font-size: 16px;
  font-weight: 500;
}}

box.maple-user-bubble {{
  background: {};
  border-radius: 24px 24px 4px 24px;
}}

label.maple-user-message {{
  color: {};
  font-size: 16px;
  font-weight: 500;
}}

label.maple-meta {{
  color: {};
  font-size: 10px;
  font-weight: 500;
}}

button.maple-toast {{
  background: {};
  color: {};
  border-radius: 999px;
  border: 1px solid {};
}}

box.maple-settings-card {{
  background: {};
  border-radius: {}px;
}}

separator.maple-settings-divider {{
  background: {};
}}

button.maple-settings-action {{
  background: transparent;
  border: none;
  color: {};
  border-radius: 999px;
}}

button.maple-danger-action {{
  background: transparent;
  border: none;
  color: {};
  border-radius: 999px;
}}
"#,
            if self.dark_mode {
                chat.surface_text.css_rgba()
            } else {
                NEUTRAL_800.css_rgba()
            },
            self.splash_tagline().css_rgba(),
            login.card_highlight.css_rgba(),
            login.card_background.css_rgba(),
            login.card_border.css_rgba(),
            RADIUS_XL,
            login.field_background.css_rgba(),
            login.field_border.css_rgba(),
            RADIUS_MD,
            login.field_text.css_rgba(),
            MAPLE_500.css_rgba(),
            MAPLE_400.css_rgba(),
            MAPLE_600.css_rgba(),
            WHITE.css_rgba(),
            login.secondary_button_background.css_rgba(),
            login.secondary_button_foreground.css_rgba(),
            login.secondary_button_border.css_rgba(),
            login.supporting_text.css_rgba(),
            login.tertiary_text.css_rgba(),
            login.divider.css_rgba(),
            chat.chrome_highlight.css_rgba(),
            chat.chrome_background.css_rgba(),
            chat.chrome_border.css_rgba(),
            chat.secondary_icon.css_rgba(),
            chat.compose_background.css_rgba(),
            chat.compose_border.css_rgba(),
            RADIUS_XL,
            chat.compose_text.css_rgba(),
            MAPLE_500.css_rgba(),
            chat.compose_placeholder.css_rgba(),
            MAPLE_500.with_alpha(0.15).css_rgba(),
            MAPLE_500.css_rgba(),
            MAPLE_500.css_rgba(),
            MAPLE_700.css_rgba(),
            WHITE.css_rgba(),
            chat.assistant_text.css_rgba(),
            chat.user_bubble_color.css_rgba(),
            chat.user_text.css_rgba(),
            chat.metadata_text.css_rgba(),
            toast.background.css_rgba(),
            toast.text.css_rgba(),
            toast.border.css_rgba(),
            chat.sheet_background.css_rgba(),
            RADIUS_XL,
            chat.sheet_divider.css_rgba(),
            chat.surface_text.css_rgba(),
            MAPLE_ERROR.css_rgba(),
        )
    }
}

pub fn detect_dark_mode() -> bool {
    gtk::Settings::default()
        .map(|settings| {
            let theme_name = settings
                .property::<String>("gtk-theme-name")
                .to_ascii_lowercase();
            settings.is_gtk_application_prefer_dark_theme() || theme_name.contains("dark")
        })
        .unwrap_or(false)
}

pub fn install_css(theme: ThemeConfig) {
    if let Some(display) = gtk::gdk::Display::default() {
        let provider = gtk::CssProvider::new();
        provider.load_from_data(theme.css().as_str());
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

pub fn background_area(kind: BackgroundKind, dark_mode: bool) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.set_draw_func(move |_, cr, width, height| {
        paint_background(cr, width, height, kind, dark_mode);
    });
    area
}

pub fn paint_background(
    cr: &cairo::Context,
    width: i32,
    height: i32,
    kind: BackgroundKind,
    dark_mode: bool,
) {
    let theme = ThemeConfig::new(dark_mode);
    let (base, stops, center_x, center_y, radius) = match kind {
        BackgroundKind::Splash => (
            PEBBLE_400,
            SPLASH_STOPS,
            width as f64 / 2.0,
            height as f64,
            height.max(width) as f64,
        ),
        BackgroundKind::Login => {
            let palette = theme.login_palette();
            (
                palette.background_base,
                palette.glow_stops,
                width as f64 / 2.0,
                height as f64,
                500.0_f64.max(height as f64),
            )
        }
        BackgroundKind::Chat => {
            let palette = theme.chat_palette();
            (
                palette.background_base,
                palette.glow_stops,
                width as f64 / 2.0,
                0.0,
                500.0,
            )
        }
    };

    base.set_source(cr);
    let _ = cr.paint();

    let gradient = cairo::RadialGradient::new(center_x, center_y, 0.0, center_x, center_y, radius);
    for (stop, color) in stops.iter().copied() {
        gradient.add_color_stop_rgba(stop, color.r, color.g, color.b, color.a);
    }

    let _ = cr.set_source(&gradient);
    let _ = cr.paint();
}
