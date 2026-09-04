//! Motion primitives that keep the frame rate low.
//!
//! gpui's own animation element asks for a frame on every display tick,
//! and each frame re-renders the whole window. Two patterns keep motion
//! cheap:
//!
//! - Looping effects (spinners, pulsing dots) run through [`ticker`]: a
//!   shared phase clock plus one coalesced repaint per view per interval,
//!   at a rate the eye still reads as continuous (`LOOP_FPS`).
//! - One-shot effects (a popup fading in) use [`fade_in`], which wraps
//!   gpui's oneshot `with_animation`: it requests frames only until the
//!   animation ends, then the window goes idle again.
//!
//! Nothing here animates hover or press: gpui has no style transitions,
//! and a stepped scale change on hover reads as flicker.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use gpui::{
    AnimationElement, AnimationExt, AnyElement, App, Bounds, Element, ElementId, EntityId,
    GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels, Styled, Window,
};

/// Frames per second a looping effect asks for. Twelve to fifteen steps a
/// second still read as motion at a fraction of the display rate.
pub const LOOP_FPS: u32 = 15;

/// Duration of a one-shot reveal.
pub const REVEAL: Duration = Duration::from_millis(140);

/// Phase clock every looping effect reads, so they all stay in step and
/// their frame timers line up.
fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// Position in `[0, 1)` within a cycle of `period`, from the shared clock.
pub fn phase(period: Duration) -> f32 {
    let period = period.as_millis().max(1);
    (epoch().elapsed().as_millis() % period) as f32 / period as f32
}

thread_local! {
    /// Views with a loop frame already scheduled. Any number of looping
    /// effects in one view cost one repaint per interval.
    static FRAME_PENDING: RefCell<HashSet<EntityId>> = RefCell::new(HashSet::new());
}

/// Ask for one repaint of the view being rendered after a loop interval,
/// unless one is already on its way.
pub fn schedule_loop_frame(window: &mut Window, cx: &mut App) {
    let view = window.current_view();
    let scheduled = FRAME_PENDING.with(|pending| !pending.borrow_mut().insert(view));
    if scheduled {
        return;
    }
    let interval = Duration::from_secs(1) / LOOP_FPS;
    window
        .spawn(cx, async move |cx| {
            cx.background_executor().timer(interval).await;
            FRAME_PENDING.with(|pending| pending.borrow_mut().remove(&view));
            cx.update(|_, cx| cx.notify(view)).ok();
        })
        .detach();
}

/// A looping element rebuilt at `LOOP_FPS` from the shared clock. `build`
/// receives the phase in `[0, 1)` of a cycle of `period` and returns the
/// element for that instant. It runs at layout time, so keep it cheap.
pub fn ticker(
    id: impl Into<ElementId>,
    period: Duration,
    build: impl Fn(f32) -> AnyElement + 'static,
) -> Ticker {
    Ticker {
        id: id.into(),
        period,
        build: Box::new(build),
    }
}

pub struct Ticker {
    id: ElementId,
    period: Duration,
    build: Box<dyn Fn(f32) -> AnyElement>,
}

impl IntoElement for Ticker {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Ticker {
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
        let mut element = (self.build)(phase(self.period));
        schedule_loop_frame(window, cx);
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

/// Fade an element in over [`REVEAL`] the first time it appears. The
/// animation is one-shot: gpui stops requesting frames when it ends. `id`
/// must be stable across frames and unique among animations on screen.
pub fn fade_in<E>(element: E, id: impl Into<ElementId>) -> AnimationElement<E>
where
    E: IntoElement + Styled + 'static,
{
    element.with_animation(
        id,
        gpui::Animation::new(REVEAL).with_easing(gpui::ease_out_quint()),
        |element, delta| element.opacity(delta),
    )
}

/// Fade in and slide up a few pixels, for panels that open above their
/// anchor. Same one-shot cost profile as [`fade_in`].
pub fn rise_in<E>(element: E, id: impl Into<ElementId>) -> AnimationElement<E>
where
    E: IntoElement + Styled + 'static,
{
    element.with_animation(
        id,
        gpui::Animation::new(REVEAL).with_easing(gpui::ease_out_quint()),
        |element, delta| {
            element
                .opacity(delta)
                .mt(gpui::px(6. * (1. - delta)))
                .mb(gpui::px(-6. * (1. - delta)))
        },
    )
}

/// A sine pulse in `[low, high]` offset by `shift` cycles, for dots that
/// breathe out of step with each other.
pub fn pulse(phase: f32, shift: f32, low: f32, high: f32) -> f32 {
    let wave = ((phase + shift) * std::f32::consts::TAU).sin() * 0.5 + 0.5;
    low + (high - low) * wave
}
