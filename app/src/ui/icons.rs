//! Lucide-style line icons from the embedded asset set. The SVG is drawn
//! as a mask, so the color comes from `text_color` and the geometry from
//! the file.

use gpui::{AnimationExt, AnyElement, Pixels, Svg, prelude::*, svg};

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

/// A `loader-circle` icon that turns once per second while it is shown.
/// `id` must be unique among the animations on screen.
pub fn spinner(id: &str, size: Pixels, color: u32) -> AnyElement {
    icon("loader-circle", size, color)
        .with_animation(
            gpui::ElementId::Name(format!("spinner-{id}").into()),
            gpui::Animation::new(std::time::Duration::from_secs(1)).repeat(),
            |svg, delta| {
                svg.with_transformation(gpui::Transformation::rotate(gpui::radians(
                    delta * std::f32::consts::TAU,
                )))
            },
        )
        .into_any_element()
}
