//! Lucide-style line icons from the embedded asset set. The SVG is drawn
//! as a mask, so the color comes from `text_color` and the geometry from
//! the file.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, Bounds, Element, ElementId, EntityId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, SharedString, Svg, Window, prelude::*, svg,
};

/// Asset paths built once per icon name. Render functions ask for icons
/// by name on every frame; without this each call formatted and
/// allocated a new path string.
fn icon_path(name: &'static str) -> SharedString {
    static PATHS: OnceLock<RwLock<HashMap<&'static str, SharedString>>> = OnceLock::new();
    let paths = PATHS.get_or_init(|| RwLock::new(HashMap::new()));
    if let Ok(cache) = paths.read()
        && let Some(path) = cache.get(name)
    {
        return path.clone();
    }
    let path = SharedString::from(format!("icons/{name}.svg"));
    if let Ok(mut cache) = paths.write() {
        cache.insert(name, path.clone());
    }
    path
}

/// An icon element sized to a square of `size` and filled with `color`.
pub fn icon(name: &'static str, size: Pixels, color: u32) -> Svg {
    svg()
        .path(icon_path(name))
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
    spinner_with_id(SharedString::from(format!("spinner-{id}")), size, color)
}

/// `spinner` with a prebuilt element id, for render paths that must not
/// format strings per frame.
pub fn spinner_with_id(id: SharedString, size: Pixels, color: u32) -> AnyElement {
    Spinner {
        id: ElementId::Name(id),
        size,
        color,
    }
    .into_any_element()
}

/// One turn of the spinner.
const SPINNER_TURN: Duration = Duration::from_secs(1);
/// Frames per second a spinner asks for. gpui's own animation element asks
/// for a frame on every tick, and each frame re-renders the whole window;
/// fifteen steps per turn still read as a spinner at a fifth of the cost.
const SPINNER_FPS: u32 = 15;

/// Phase clock every spinner reads, so they all show the same angle and
/// their frame timers line up.
fn spinner_epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

thread_local! {
    /// Views with a spinner frame already scheduled. Any number of
    /// spinners in one view cost one repaint per interval.
    static SPINNER_FRAME_PENDING: RefCell<HashSet<EntityId>> = RefCell::new(HashSet::new());
}

/// A rotating `loader-circle` that repaints at `SPINNER_FPS` from a shared
/// clock, rather than on every display frame.
struct Spinner {
    id: ElementId,
    size: Pixels,
    color: u32,
}

impl IntoElement for Spinner {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Spinner {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, AnyElement) {
        let turn = SPINNER_TURN.as_millis();
        let phase = (spinner_epoch().elapsed().as_millis() % turn) as f32 / turn as f32;
        let mut element = icon("loader-circle", self.size, self.color)
            .with_transformation(gpui::Transformation::rotate(gpui::radians(
                phase * std::f32::consts::TAU,
            )))
            .into_any_element();
        schedule_spinner_frame(window, cx);
        (element.request_layout(window, cx), element)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        element: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        element: &mut AnyElement,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        element.paint(window, cx);
    }
}

/// Ask for one repaint of the view being rendered after a spinner
/// interval, unless one is already on its way.
fn schedule_spinner_frame(window: &mut Window, cx: &mut App) {
    let view = window.current_view();
    let scheduled = SPINNER_FRAME_PENDING.with(|pending| !pending.borrow_mut().insert(view));
    if scheduled {
        return;
    }
    let interval = SPINNER_TURN / SPINNER_FPS;
    window
        .spawn(cx, async move |cx| {
            cx.background_executor().timer(interval).await;
            SPINNER_FRAME_PENDING.with(|pending| pending.borrow_mut().remove(&view));
            cx.update(|_, cx| cx.notify(view)).ok();
        })
        .detach();
}
