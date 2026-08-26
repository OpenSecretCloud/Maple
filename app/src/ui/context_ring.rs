//! Circular context-window indicator for the composer: a progress ring
//! showing how much of the model's context window the session occupies.

use gpui::{
    App, Bounds, Element, ElementId, GlobalElementId, IntoElement, Pixels, Point, Style, Window,
    div, point, prelude::*, px, relative,
};

use super::theme;

/// Arc geometry helpers: build a stroked circle arc from `start` to `end`
/// fractions (0.0 = 12 o'clock, clockwise).
fn arc_path(center: Point<Pixels>, radius: Pixels, start: f32, end: f32) -> gpui::Path<Pixels> {
    let mut path = gpui::PathBuilder::stroke(px(1.5));
    // Clamp the arc so a full circle stays representable.
    let start = start.fract();
    let end = end.clamp(0.0, 1.0).fract();
    let (large_arc, drawn) = if end > start && end - start >= 0.5 {
        (true, end - start)
    } else if end < start && 1.0 - start + end >= 0.5 {
        (true, end)
    } else {
        (false, end)
    };
    let _ = drawn;
    let a = |fraction: f32| -> Point<Pixels> {
        // Start at 12 o'clock, go clockwise.
        let angle = fraction * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
        point(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        )
    };
    path.move_to(a(start));
    // SVG-style arc: for a full circle we draw two half arcs.
    if end == 0.0 || (end - start).abs() >= 0.999 {
        // full circle
        path.arc_to(point(radius, radius), px(0.), false, true, a(start + 0.5));
        path.arc_to(point(radius, radius), px(0.), false, true, a(start + 1.0));
    } else {
        path.arc_to(point(radius, radius), px(0.), large_arc, true, a(end));
    }
    path.build().expect("valid arc path")
}

pub struct ContextRing {
    /// Fraction of the context window in use, 0.0..1.0.
    pub fraction: f32,
}

impl ContextRing {
    pub fn new(fraction: f32) -> Self {
        Self {
            fraction: fraction.clamp(0.0, 1.0),
        }
    }

    fn ring_color(fraction: f32) -> gpui::Hsla {
        if fraction >= 0.9 {
            gpui::rgb(theme::STATUS_ERROR).into()
        } else if fraction >= 0.75 {
            gpui::rgb(theme::STATUS_WARNING).into()
        } else {
            gpui::rgb(theme::ACCENT).into()
        }
    }
}

impl IntoElement for ContextRing {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ContextRing {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui::LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = px(14.).into();
        style.size.height = px(14.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        _prepaint: &mut (),
        window: &mut Window,
        _cx: &mut App,
    ) {
        let center = bounds.center();
        let radius = bounds.size.width.min(bounds.size.height) / 2.0 - px(1.0);
        // Track: full circle in a dim color.
        let track = arc_path(center, radius, 0.0, 0.999);
        window.paint_path(track, gpui::rgb(theme::BORDER));
        // Usage arc from the top.
        let used = self.fraction.clamp(0.0, 1.0);
        if used > 0.01 {
            let arc = arc_path(center, radius, 0.0, used);
            window.paint_path(arc, Self::ring_color(used));
        }
    }
}

/// Labeled wrapper: ring plus a percentage when space allows.
pub fn context_indicator(fraction: f32) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_1()
        .w(relative(1.))
        .child(ContextRing::new(fraction))
}
