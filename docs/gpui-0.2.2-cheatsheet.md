# gpui 0.2.2 cheat sheet for a chat app

All paths are relative to the crate root:
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-0.2.2/`.
Every item below is checked against that exact source. Items that exist in
Zed-main gpui but not in 0.2.2 are marked **NOT IN 0.2.2**.

Common imports:

```rust
use gpui::{
    actions, div, prelude::*, px, rgb, App, Context, Entity, EventEmitter, FocusHandle,
    Focusable, FontWeight, HighlightStyle, KeyBinding, KeyDownEvent, Render, ScrollHandle,
    SharedString, StyledText, Window,
};
```

---

## 1. Scrollable container + programmatic scroll to bottom

Mechanism: a `ScrollHandle` (`src/elements/div.rs:3068`) that you attach with
`.track_scroll(&handle)` (`src/elements/div.rs:1077`). The handle stores a
request flag. The div reads the flag in its prepaint pass and sets the offset
to `-scroll_max.height` (`src/elements/div.rs:1734`, `:1755`).

`.overflow_y_scroll()` alone (`src/elements/div.rs:1062`) only sets the style.
It does not expose an offset. You need the handle.

**NOT IN 0.2.2:** `window.scroll_element_to_top`, `window.scroll_to`, or any
`Window` scroll method. `grep scroll_element src/` returns nothing.

```rust
struct MessageList {
    scroll: ScrollHandle,          // ScrollHandle::new()  div.rs:3078
    messages: Vec<SharedString>,
}

impl MessageList {
    fn push(&mut self, msg: SharedString, cx: &mut Context<Self>) {
        self.messages.push(msg);
        self.scroll.scroll_to_bottom();   // div.rs:3203 – applied next prepaint
        cx.notify();                      // context.rs:229 – trigger a re-render
    }
}

impl Render for MessageList {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("messages")                // scroll needs a Stateful div
            .flex_1()
            .overflow_y_scroll()           // div.rs:1062
            .track_scroll(&self.scroll)    // div.rs:1077
            .children(self.messages.iter().map(|m| div().child(m.clone())))
    }
}
```

Other `ScrollHandle` methods (all `src/elements/div.rs`):

| Method | Line | Note |
|---|---|---|
| `offset() -> Point<Pixels>` | 3083 | Negative y = scrolled down |
| `max_offset() -> Size<Pixels>` | 3088 | |
| `set_offset(Point<Pixels>)` | 3211 | Direct. Div clamps it in prepaint (line 1758) |
| `scroll_to_item(ix)` | 3141 | Minimal scroll so child `ix` is visible |
| `scroll_to_top_of_item(ix)` | 3151 | Child `ix` at top |
| `top_item()` / `bottom_item()` | 3092 / 3110 | Visible child index range |

"Stick to bottom" heuristic: compare `offset().y` with `-max_offset().height`
before you push. If they are equal (user is at bottom), call `scroll_to_bottom()`.

---

## 2. Mixed styling in one line: `StyledText` + `HighlightStyle`

`StyledText` (`src/elements/text.rs:148`) is an element. Ranges are **byte**
ranges, and must fall on char boundaries (debug_assert at `text.rs:196`).

```rust
// text.rs:157 new, text.rs:188 with_highlights
StyledText::new("alice: hello world").with_highlights([
    (0..5, HighlightStyle { font_weight: Some(FontWeight::BOLD), ..Default::default() }),
    (7..12, HighlightStyle { color: Some(rgb(0x3b82f6).into()), ..Default::default() }),
])
```

`HighlightStyle` fields (`src/style.rs:496`): `color`, `font_weight`,
`font_style`, `background_color`, `underline`, `strikethrough`, `fade_out`.
`FontWeight::BOLD.into()` and `FontStyle::Italic.into()` produce a
`HighlightStyle` (used in `examples/text_layout.rs:75-78`).

Two constructors, do not mix them (debug_assert at `text.rs:178`, `:192`):

- `with_highlights(iter)` (`text.rs:188`): base style comes from the parent
  div at layout time. Use this one.
- `with_default_highlights(&TextStyle, iter)` (`text.rs:173`): you supply the
  base style, for example `window.text_style()` (`src/window.rs:1440`).

Clickable ranges (links, mentions): `InteractiveText::new(id, styled_text)`
(`text.rs:653`) then `.on_click(vec![ranges], |range_ix, window, cx| ..)`
(`text.rs:667`) and `.tooltip(..)` (`text.rs:695`).

Lower level: `TextRun` (`src/text_system.rs:733`) with fields `len`, `font`,
`color`, `background_color`, `underline`, `strikethrough`. `StyledText`
builds these for you; a custom element is not needed.

---

## 3. Composer keybindings: Enter sends while the input has focus

Pattern from `examples/input.rs`. Three parts: an action, a key context on the
div, and a binding with a context predicate.

```rust
// examples/input.rs:13 – defines unit structs that derive Action
actions!(composer, [Send, NewLine]);

impl Render for Composer {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("Composer")                    // div.rs:658
            .track_focus(&self.focus_handle)            // div.rs:616
            .on_action(cx.listener(Self::send))         // div.rs:854, listener: context.rs:252
            .on_action(cx.listener(Self::newline))
            .on_key_down(cx.listener(Self::on_key))     // div.rs:881 (raw path, see below)
            .child(/* text */ "")
    }
}

impl Composer {
    fn send(&mut self, _: &Send, _w: &mut Window, cx: &mut Context<Self>) { /* ... */ }
    fn newline(&mut self, _: &NewLine, _w: &mut Window, cx: &mut Context<Self>) { /* ... */ }
    fn on_key(&mut self, ev: &KeyDownEvent, _w: &mut Window, cx: &mut Context<Self>) {
        // KeyDownEvent: src/interactive.rs:22  { keystroke: Keystroke, is_held: bool }
        // Keystroke:    src/platform/keystroke.rs:18 { modifiers, key: String, key_char: Option<String> }
        if let Some(ch) = &ev.keystroke.key_char { /* insert text */ }
    }
}

// At startup (examples/input.rs:680):
cx.bind_keys([
    KeyBinding::new("enter", Send, Some("Composer")),        // keymap/binding.rs:33
    KeyBinding::new("shift-enter", NewLine, Some("Composer")),
]);
```

`KeyBinding::new(keystrokes: &str, action: A, context: Option<&str>)`
(`src/keymap/binding.rs:33`). Predicate syntax (`src/keymap/context.rs:217-238`):
`"Composer"`, `"Composer && mode == edit"`, `"!Composer"`, `"Sidebar > Item"`.
The predicate matches when the focused element, or one of its ancestors, has
that `key_context`. So the binding only fires while the composer has focus.

`on_action` handlers run in the bubble phase. Global fallback:
`cx.on_action(|_: &Quit, cx| cx.quit())` (`src/app.rs:1696`).

Use `on_key_down` for character input. Use actions + `bind_keys` for
commands. Do not match `"enter"` inside `on_key_down`; a keymap binding is
cheaper and user-configurable.

---

## 4. Focus management

```rust
// Trait: src/window.rs:440
pub trait Focusable: 'static {
    fn focus_handle(&self, cx: &App) -> FocusHandle;
}
```

| What | API | Path |
|---|---|---|
| Create a handle | `cx.focus_handle()` | `src/app.rs:2029` |
| Attach to a div | `.track_focus(&handle)` | `src/elements/div.rs:616` |
| Move focus | `window.focus(&handle)` or `handle.focus(window)` | `src/window.rs:1386` / `:340` |
| Clear focus | `window.blur()` | `src/window.rs:1396` |
| Query | `handle.is_focused(window)` / `handle.contains_focused(window, cx)` | `src/window.rs:345` / `:351` |
| Focused-only style | `.focus(\|s\| s.border_color(..))` / `.in_focus(..)` | `src/elements/div.rs:1020` / `:1030` |
| Tab order | `handle.tab_index(n).tab_stop(true)` or `div().tab_index(n)` | `src/window.rs:312`,`:323` / `div.rs:640` |
| Tab navigation | `window.focus_next()` / `window.focus_prev()` | `src/window.rs:1413` / `:1424` |
| Focus events | `window.on_focus_in(&handle, \|w, cx\| ..)` / `on_focus_out` | `src/window.rs:3481` / `:3501` |

Sidebar / composer switch: hold both handles in the root view, bind
`KeyBinding::new("cmd-1", FocusSidebar, None)` and call
`window.focus(&self.sidebar.focus_handle(cx))` in the handler
(`Entity<V: Focusable>` implements `Focusable`, `src/window.rs:446`).
Initial focus at open: `window.focus(&view.composer.focus_handle(cx))`
(`examples/input.rs:739`). `examples/tab_stop.rs` shows `Tab` / `Shift-Tab`.

---

## 5. Buttons

```rust
div()
    .id("send")                                 // on_click needs a Stateful div
    .px_3().py_1().rounded_md()
    .bg(rgb(0x2563eb))
    .hover(|s| s.bg(rgb(0x1d4ed8)))             // div.rs:670
    .active(|s| s.bg(rgb(0x1e40af)))            // div.rs:1089
    .cursor_pointer()
    .on_click(cx.listener(|this, _ev: &ClickEvent, window, cx| {   // div.rs:1117
        this.send(&Send, window, cx);
    }))
    .child("Send")
```

Signatures: `on_click(impl Fn(&ClickEvent, &mut Window, &mut App))`
(`src/elements/div.rs:1117`); `cx.listener(f)` wraps a
`Fn(&mut T, &E, &mut Window, &mut Context<T>)` and upgrades the weak entity
(`src/app/context.rs:252`). `.hover` takes
`FnOnce(StyleRefinement) -> StyleRefinement`. Examples:
`examples/data_table.rs:317`, `examples/image_gallery.rs:75-80`.

---

## 6. Lists

Three options, from simple to specialised:

**Plain `div().children(..)`** — fine for a sidebar with tens of items and
for a message list under a few hundred elements. All children get laid out
each frame. Combine with section 1 for scrolling.

**`uniform_list`** (`src/elements/uniform_list.rs:22`) — for many rows of the
**same height** (sidebar conversation list, member list). Renders only the
visible range.

```rust
pub fn uniform_list<R: IntoElement>(
    id: impl Into<ElementId>,
    item_count: usize,
    f: impl 'static + Fn(Range<usize>, &mut Window, &mut App) -> Vec<R>,
) -> UniformList
```

Usage (`examples/uniform_list.rs`):

```rust
uniform_list("conversations", self.items.len(), cx.processor(  /* context.rs:264 */|this, range, _w, _cx| {
    range.map(|ix| div().child(this.items[ix].clone())).collect()
}))
.track_scroll(self.list_scroll.clone())   // UniformListScrollHandle, uniform_list.rs:675
```

`UniformListScrollHandle::scroll_to_item(ix, ScrollStrategy::Top|Center|Bottom)`
(`uniform_list.rs:146`, enum at `:84`). Note: `track_scroll` takes the handle by
value here, but by reference on `div`.

**`list(ListState, render_item)`** (`src/elements/list.rs:24`) — variable
height rows, virtualised. Built for chat logs:
`ListState::new(item_count, ListAlignment::Bottom, overdraw_px)`
(`list.rs:216`, `ListAlignment::Bottom` doc says "like a chat log", `:81`).
Then `state.splice(old_range, count)` (`:264`) when messages change,
`state.scroll_to_reveal_item(ix)` (`:360`), `state.scroll_to(ListOffset)`
(`:348`). Reach for this only when plain div + `ScrollHandle` gets slow.

**NOT IN 0.2.2:** no tree / recursive element. `examples/tree.rs` builds a tree
from nested `div`s.

---

## 7. Entity events: `cx.emit` / `cx.subscribe`

```rust
// Marker trait, no methods: src/gpui.rs:237
pub trait EventEmitter<E: Any>: 'static {}

pub enum ChatEvent { MessageSent(SharedString) }
impl EventEmitter<ChatEvent> for Composer {}

// Emit (src/app/context.rs:723). Requires T: EventEmitter<Evt>.
impl Composer {
    fn send(&mut self, _: &Send, _w: &mut Window, cx: &mut Context<Self>) {
        cx.emit(ChatEvent::MessageSent(self.text.clone().into()));
    }
}

// Subscribe from another entity (src/app/context.rs:98):
// on_event: FnMut(&mut T, Entity<T2>, &Evt, &mut Context<T>)  -> Subscription
impl Root {
    fn new(cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| Composer::new(cx));
        let sub = cx.subscribe(&composer, |this, _composer, event: &ChatEvent, cx| {
            match event { ChatEvent::MessageSent(m) => this.messages.update(cx, |l, cx| l.push(m.clone(), cx)) }
        });
        Self { composer, _subs: vec![sub], /* .. */ }
    }
}
```

Keep the `Subscription` alive (store it), or call `.detach()`. Dropping it
unsubscribes.

Related signatures:

| API | Signature | Path |
|---|---|---|
| `Context::subscribe_self` | `FnMut(&mut T, &Evt, &mut Context<T>)` | `context.rs:120` |
| `Context::observe` | `FnMut(&mut T, Entity<W>, &mut Context<T>)` — fires on `notify` | `context.rs:63` |
| `Context::notify` | re-render + wake observers | `context.rs:229` |
| `App::subscribe` | `FnMut(Entity<T>, &Event, &mut App)` | `app.rs:865` |
| `Context::subscribe_in` | adds `&mut Window` param | `context.rs:355` |
| `Entity::update` | `entity.update(cx, \|state, cx\| ..)` | `app/entity_map.rs:430` |

Events are queued (`Effect::Emit`, `context.rs:729`) and delivered after the
current update returns, so `emit` inside `update` is safe.

---

## Quick "does it exist?" table

| Name | 0.2.2 | Use instead |
|---|---|---|
| `window.scroll_element_to_top` / `Window::scroll_to` | no | `ScrollHandle::scroll_to_bottom` / `set_offset` |
| `ScrollHandle::scroll_to_bottom` | yes (`div.rs:3203`) | |
| `ScrollAnchor::scroll_to(window, cx)` | yes (`div.rs:3029`) | scroll to a non-direct child |
| `div().on_key_down` | yes (`div.rs:881`) | |
| `div().on_focus` / `on_blur` | no | `window.on_focus_in/out(&handle, ..)` |
| `cx.subscribe` returns `Subscription` | yes | store it or `.detach()` |
| `Render::render(&mut self, &mut Window, &mut Context<Self>)` | yes (`element.rs:133`) | |
| tree / recursive list element | no | nested `div`, see `examples/tree.rs` |
