//! One scrollbar for every scrolling surface: the transcript and sidebar
//! lists (`ListState`) and the settings pane (`ScrollHandle`). A thin
//! thumb rides the right edge and widens under the pointer; dragging it
//! scrolls, and the drag keeps working after the pointer leaves the
//! track because the thumb captures the pointer. A press on the track
//! pages, the way the platform's own bars do. The wheel passes through
//! to the content underneath.
//!
//! The bar is a plain element, not an entity: its only state is the drag
//! in progress, kept in element state, so a surface adds a scrollbar with
//! one child and no bookkeeping of its own.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, BorderStyle, Bounds, DispatchPhase, Element, ElementId, GlobalElementId, Hitbox,
    HitboxBehavior, IntoElement, ListState, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, ScrollHandle, Style, Window, div, point, prelude::*, px, quad, size,
};

use super::theme;

/// Width of the strip the bar occupies at the right edge.
pub const WIDTH: Pixels = px(12.);
const THUMB_WIDTH: Pixels = px(6.);
const THUMB_WIDTH_WIDE: Pixels = px(9.);
const THUMB_MIN_HEIGHT: Pixels = px(25.);
const EDGE_INSET: Pixels = px(2.);

/// What the bar drives: any surface with a vertical offset. Offsets are
/// distances from the top, never negative, whatever the surface's own
/// sign convention.
pub trait ScrollTarget: Clone + 'static {
    /// How far the content extends past the viewport.
    fn scroll_range(&self) -> Pixels;
    /// How far the viewport is scrolled from the top.
    fn scroll_top(&self) -> Pixels;
    fn scroll_to(&self, top: Pixels);
    /// The thumb was pressed; a list may freeze its content height so
    /// the drag stays stable while rows measure.
    fn drag_started(&self) {}
    fn drag_ended(&self) {}
}

impl ScrollTarget for ListState {
    fn scroll_range(&self) -> Pixels {
        self.max_offset_for_scrollbar().y.max(px(0.))
    }

    fn scroll_top(&self) -> Pixels {
        (-self.scroll_px_offset_for_scrollbar().y).max(px(0.))
    }

    fn scroll_to(&self, top: Pixels) {
        self.set_offset_from_scrollbar(point(px(0.), -top));
    }

    fn drag_started(&self) {
        self.scrollbar_drag_started();
    }

    fn drag_ended(&self) {
        self.scrollbar_drag_ended();
    }
}

impl ScrollTarget for ScrollHandle {
    fn scroll_range(&self) -> Pixels {
        self.max_offset().y.max(px(0.))
    }

    fn scroll_top(&self) -> Pixels {
        (-self.offset().y).max(px(0.))
    }

    fn scroll_to(&self, top: Pixels) {
        let x = self.offset().x;
        self.set_offset(point(x, -top));
    }
}

/// A scrollbar overlaid on the right edge of a `relative` container. It
/// paints nothing and blocks nothing while the content fits.
pub fn scrollbar<T: ScrollTarget>(id: impl Into<ElementId>, target: T) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .right_0()
        .bottom_0()
        .w(WIDTH)
        .flex()
        .flex_col()
        .child(Scrollbar {
            id: id.into(),
            target,
        })
}

pub struct Scrollbar<T> {
    id: ElementId,
    target: T,
}

/// Thumb placement for one frame, in track-local pixels.
#[derive(Clone, Copy, Debug)]
struct Geometry {
    /// Track height; also the distance one track press pages by.
    track: Pixels,
    thumb: Pixels,
    thumb_top: Pixels,
    /// How far the thumb top can travel.
    scrollable: Pixels,
    /// How far the content can scroll.
    range: Pixels,
}

fn geometry<T: ScrollTarget>(bounds: Bounds<Pixels>, target: &T) -> Option<Geometry> {
    let track = bounds.size.height;
    let range = target.scroll_range();
    if range <= px(1.) || track <= px(0.) {
        return None;
    }
    let ratio = f32::from(track) / f32::from(track + range);
    let thumb = (track * ratio).max(THUMB_MIN_HEIGHT).min(track);
    let scrollable = (track - thumb).max(px(0.));
    let progress = (f32::from(target.scroll_top()) / f32::from(range)).clamp(0., 1.);
    Some(Geometry {
        track,
        thumb,
        thumb_top: scrollable * progress,
        scrollable,
        range,
    })
}

/// The content offset that puts the thumb top at `thumb_top`.
fn offset_for_thumb_top(geometry: &Geometry, thumb_top: Pixels) -> Pixels {
    if geometry.scrollable <= px(0.) {
        return px(0.);
    }
    let progress = (f32::from(thumb_top) / f32::from(geometry.scrollable)).clamp(0., 1.);
    geometry.range * progress
}

#[derive(Default)]
struct State {
    drag: Option<Drag>,
    /// Pointer over the bar as of the last move, so widening repaints
    /// once per change rather than per move.
    hovered: bool,
}

#[derive(Clone, Copy)]
struct Drag {
    pointer_y: Pixels,
    thumb_top: Pixels,
}

pub struct Prepaint {
    hitbox: Hitbox,
    state: Rc<RefCell<State>>,
    geometry: Option<Geometry>,
}

impl<T: ScrollTarget> IntoElement for Scrollbar<T> {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl<T: ScrollTarget> Element for Scrollbar<T> {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
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
        let style = Style {
            flex_grow: 1.,
            size: size(WIDTH.into(), gpui::Length::Auto),
            ..Style::default()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        window: &mut Window,
        _cx: &mut App,
    ) -> Prepaint {
        let geometry = geometry(bounds, &self.target);
        // Swallow presses only while there is a bar to press; the wheel
        // always reaches the content underneath.
        let behavior = if geometry.is_some() {
            HitboxBehavior::BlockMouseExceptScroll
        } else {
            HitboxBehavior::Normal
        };
        let hitbox = window.insert_hitbox(bounds, behavior);
        let id = id.expect("the scrollbar element has an id");
        let state = window.with_element_state::<Rc<RefCell<State>>, _>(id, |state, _window| {
            let state = state.unwrap_or_default();
            (state.clone(), state)
        });
        Prepaint {
            hitbox,
            state,
            geometry,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        prepaint: &mut Prepaint,
        window: &mut Window,
        _cx: &mut App,
    ) {
        let state = prepaint.state.clone();
        let Some(geometry) = prepaint.geometry else {
            // The content shrank to fit mid-drag: let the target go.
            if state.borrow_mut().drag.take().is_some() {
                self.target.drag_ended();
                window.release_pointer();
            }
            return;
        };
        let hitbox = prepaint.hitbox.clone();
        let dragging = state.borrow().drag.is_some();
        let wide = dragging || hitbox.is_hovered(window);
        let width = if wide { THUMB_WIDTH_WIDE } else { THUMB_WIDTH };
        let thumb_bounds = Bounds::new(
            point(
                bounds.right() - EDGE_INSET - width,
                bounds.top() + geometry.thumb_top,
            ),
            size(width, geometry.thumb),
        );
        let color = if wide {
            theme::scrollbar_thumb_active()
        } else {
            theme::scrollbar_thumb()
        };
        window.paint_quad(quad(
            thumb_bounds,
            width / 2.,
            color,
            px(0.),
            gpui::transparent_black(),
            BorderStyle::Solid,
        ));

        let target = self.target.clone();
        window.on_mouse_event({
            let hitbox = hitbox.clone();
            let state = state.clone();
            let target = target.clone();
            move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble
                    || event.button != MouseButton::Left
                    || !hitbox.is_hovered(window)
                {
                    return;
                }
                let local_y = event.position.y - bounds.top();
                let on_thumb =
                    local_y >= geometry.thumb_top && local_y <= geometry.thumb_top + geometry.thumb;
                if on_thumb {
                    state.borrow_mut().drag = Some(Drag {
                        pointer_y: event.position.y,
                        thumb_top: geometry.thumb_top,
                    });
                    target.drag_started();
                    window.capture_pointer(hitbox.id);
                } else if local_y < geometry.thumb_top {
                    target.scroll_to((target.scroll_top() - geometry.track).max(px(0.)));
                } else {
                    target.scroll_to((target.scroll_top() + geometry.track).min(geometry.range));
                }
                cx.stop_propagation();
                window.refresh();
            }
        });
        window.on_mouse_event({
            let hitbox = hitbox.clone();
            let state = state.clone();
            let target = target.clone();
            move |event: &MouseMoveEvent, phase, window, _cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                let drag = state.borrow().drag;
                if let Some(drag) = drag {
                    let thumb_top = drag.thumb_top + (event.position.y - drag.pointer_y);
                    target.scroll_to(offset_for_thumb_top(&geometry, thumb_top));
                    window.refresh();
                    return;
                }
                let hovered = hitbox.is_hovered(window);
                let mut state = state.borrow_mut();
                if state.hovered != hovered {
                    state.hovered = hovered;
                    window.refresh();
                }
            }
        });
        window.on_mouse_event({
            let state = state.clone();
            move |event: &MouseUpEvent, phase, window, _cx| {
                if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                    return;
                }
                if state.borrow_mut().drag.take().is_some() {
                    target.drag_ended();
                    window.release_pointer();
                    window.refresh();
                }
            }
        });
    }
}
