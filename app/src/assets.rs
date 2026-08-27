//! Embedded assets: brand fonts and SVG icons. Files live in `app/assets`
//! and are compiled into the binary so the app has no runtime asset path.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

macro_rules! assets {
    ($($path:literal),* $(,)?) => {
        const ASSETS: &[(&str, &[u8])] = &[
            $(($path, include_bytes!(concat!("../assets/", $path))),)*
        ];
    };
}

assets!(
    "icons/archive-restore.svg",
    "icons/archive.svg",
    "icons/arrow-up.svg",
    "icons/chevron-down.svg",
    "icons/chevron-right.svg",
    "icons/ellipsis.svg",
    "icons/folder-open.svg",
    "icons/folder-plus.svg",
    "icons/folder.svg",
    "icons/globe.svg",
    "icons/image.svg",
    "icons/loader-circle.svg",
    "icons/lock.svg",
    "icons/maple-wordmark.svg",
    "icons/maximize-2.svg",
    "icons/minimize-2.svg",
    "icons/paperclip.svg",
    "icons/plus.svg",
    "icons/puzzle.svg",
    "icons/search.svg",
    "icons/panel-left.svg",
    "icons/pencil.svg",
    "icons/plug.svg",
    "icons/settings.svg",
    "icons/shield-check.svg",
    "icons/square-pen.svg",
    "icons/square.svg",
    "icons/trash-2.svg",
    "icons/x.svg",
    "icons/zap.svg",
);

/// Font files registered with the text system at startup.
pub const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/Manrope-Regular.ttf"),
    include_bytes!("../assets/fonts/Manrope-Medium.ttf"),
    include_bytes!("../assets/fonts/Manrope-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Manrope-Bold.ttf"),
    include_bytes!("../assets/fonts/ArrayWide.ttf"),
    include_bytes!("../assets/fonts/Mondwest.ttf"),
];

/// Body font for the whole app.
pub const FONT_BODY: &str = "Manrope";
/// Pixel display font for the empty-state heading.
pub const FONT_DISPLAY: &str = "Array Wide";

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ASSETS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ASSETS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}
