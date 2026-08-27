//! Lucide-style line icons from the embedded asset set. The SVG is drawn
//! as a mask, so the color comes from `text_color` and the geometry from
//! the file.

use gpui::{Pixels, Svg, prelude::*, svg};

/// An icon element sized to a square of `size` and filled with `color`.
pub fn icon(name: &str, size: Pixels, color: u32) -> Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(size)
        .flex_none()
        .text_color(gpui::rgb(color))
}

/// The Maple wordmark at the given height; the width follows the 248:50
/// aspect ratio of the artwork.
pub fn wordmark(height: Pixels, color: u32) -> Svg {
    svg()
        .path("icons/maple-wordmark.svg")
        .h(height)
        .w(height * (248. / 50.))
        .flex_none()
        .text_color(gpui::rgb(color))
}
