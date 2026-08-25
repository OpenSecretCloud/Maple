# gpui 0.2.2 usage review

Scope: `app/src/ui/text_input.rs`, `app/src/ui/chat.rs`,
`app/src/ui/markdown.rs`, `app/src/main.rs`.
Reference: crate source at
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-0.2.2/` (paths
below marked `gpui:` are relative to that root) and
`docs/gpui-0.2.2-cheatsheet.md`.

Totals: 1 BLOCKER, 5 SHOULD-FIX, 6 NIT.

---

## 1. BLOCKER — Enter inserts `\n` into the composer on macOS after send

**File:** `app/src/ui/text_input.rs:701-705`

**Problem.** The Enter handler is an `on_key_down` listener that calls
`enter_pressed` but never calls `cx.stop_propagation()`. On macOS the
platform layer computes `key_char = Some("\n")` for Return
(`gpui:src/platform/mac/events.rs:328`). The key-down callback returns
`!propagate` as "handled" (`gpui:src/platform/mac/window.rs:1671-1680`). When
the callback reports not-handled, the platform forwards the event to the
NSTextInputContext (`mac/window.rs:1757-1765`), which calls `insert_text` →
`replace_text_in_range(None, "\n")` (`mac/window.rs:2234-2247`). Order of
events: `send()` runs, `composer.clear()` runs, then `"\n"` is inserted. The
composer is left with a newline after every send. `paste()` filters `\n`
(`text_input.rs:216`) but `replace_text_in_range` does not.

On Linux `key_char` is `None` for control characters
(`gpui:src/platform/linux/platform.rs:919-921`), so the bug does not show
there. That is why it was not seen in local testing.

**Fix (either, both is safest):**

1. Stop propagation in the listener. `App::stop_propagation` is at
   `gpui:src/app.rs:1712`:
   ```rust
   .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
       if event.keystroke.key == "enter" && !event.keystroke.modifiers.modified() {
           cx.stop_propagation();
           this.enter_pressed(window, cx);
       }
   }))
   ```
2. Better: make Enter an action. Add `Enter` to the `actions!` list, bind
   `KeyBinding::new("enter", Enter, Some("TextInput"))`
   (`gpui:src/keymap/binding.rs:33`), and handle it with `.on_action`. When an
   action handler runs, gpui sets `propagate_event = false` and returns before
   the key-down/IME path (`gpui:src/window.rs:3834-3847`).
3. Defensive: reject `\n` / `\r` in `replace_text_in_range`
   (`text_input.rs:398-417`) for a single-line input.

---

## 2. SHOULD-FIX — Markdown text ignores the parent `text_color` / font

**File:** `app/src/ui/markdown.rs:82-93`, `:125-126`; consumer at
`app/src/ui/chat.rs:743-746`

**Problem.** `base_highlight` reads `window.text_style()` while
`ChatScreen::render` runs. `Window::text_style` folds `text_style_stack`
(`gpui:src/window.rs:1440-1446`). That stack is only populated during
layout/paint of ancestor divs. During `render()` it is empty, so the result is
`TextStyle::default()` (default color, default font). `styled_paragraph` then
puts `color: Some(default)` and `font_weight: Some(default)` on **every** span
and passes them to `with_default_highlights(&window.text_style(), ..)`
(`gpui:src/elements/text.rs:173-183`), which bakes them into `TextRun`s. The
`.text_color(theme::TEXT_PRIMARY)` on the parent (`chat.rs:745`) and any
inherited font family are discarded for all assistant messages.

**Fix.** Use `with_highlights` (`gpui:src/elements/text.rs:188`). It defers
run construction to `TextLayout::layout`, which is called inside the parent
style scope and uses the ambient style for unstyled text
(`gpui:src/elements/text.rs:325-338`). Only set the fields you override:

```rust
fn base_highlight() -> HighlightStyle { HighlightStyle::default() } // all None
...
let text = StyledText::new(SharedString::new(paragraph.text))
    .with_highlights(highlights);
```

`styled_paragraph` then no longer needs `window`. For headings keep
`.text_size(..)` on the wrapping div and pass `font_weight: Some(BOLD)` in the
highlight for the whole range.

---

## 3. SHOULD-FIX — Every streamed token re-parses and re-shapes the whole transcript

**File:** `app/src/ui/chat.rs:362-373`, `:397-400`, `:659-663`;
`app/src/ui/markdown.rs:146`

**Problem.** `handle_service_event` calls `cx.notify()` once per event
(`chat.rs:372`). `notify` marks `ChatScreen` dirty; on the next frame the
window calls `draw` (`gpui:src/window.rs:1044-1052`) and re-runs
`ChatScreen::render`. View-level reuse (`gpui:src/view.rs:205-222`) only
skips a view that is **not** in `dirty_views`, and only when it was rendered
through `AnyView::cached(..)` (`gpui:src/view.rs:103`, `:170-174`). Neither
holds here: `ChatScreen` is the dirty view, and timeline items are plain divs.

So per token: 500 × `pulldown_cmark::Parser` + 500 × `StyledText` layout
(`window.request_measured_layout` + line shaping per paragraph,
`gpui:src/elements/text.rs:340`). Shaping is the expensive part; expect
several ms to tens of ms per frame on a long transcript, which drops below
60 fps while streaming. Off-screen items are shaped too because the transcript
is a plain overflow div.

The `container = container.child(..)` pattern in `render_markdown` is **not**
the problem: `Div::child` pushes into the div's `SmallVec` children; moving
`Div` by value is a memcpy. No per-call allocation beyond vector growth.

**Fix (pick one, in order of payoff):**

1. Replace the transcript div with `list(ListState, render_item)`
   (`gpui:src/elements/list.rs:24`, state at `:216`,
   `ListAlignment::Bottom` at `:81`). Only visible rows plus overdraw are
   rendered and measured. Call `state.splice(..)` (`list.rs:264`) when
   `timeline` changes and `state.scroll_to_reveal_item(len-1)` (`list.rs:360`)
   instead of `ScrollHandle::scroll_to_bottom`.
2. Cheaper interim: cache the parsed markdown per item (parse result → a
   `Vec<Block>` of owned strings + spans keyed by `(item.id, text.len())`).
   This removes the parse but not the shaping.
3. Coalesce: when a burst of `TimelineItem` events arrives, one `notify` per
   frame is enough. gpui already collapses multiple `notify` calls into one
   draw (`gpui:src/window.rs:118-120`), so this only helps if the event pump
   yields between events; it does (`main.rs:119-121` awaits `rx.recv()`).

---

## 4. SHOULD-FIX — Model dropdown is painted under later siblings and has no outside-click dismiss

**File:** `app/src/ui/chat.rs:593-625`

**Positioning is fine.** gpui maps `.absolute()` to
`taffy::Position::Absolute` (`gpui:src/style.rs:1286`). gpui pins taffy
`=0.9.0` (`gpui:Cargo.toml:388-389`); taffy's default position is
`Relative`, so **every** parent is a containing block. No `.relative()` on the
header is required. `right_4()` + `mt_6()` resolve against the header box.

**Problems.**

- Paint order: the header is child 0 of the column; the transcript is child 1
  (`chat.rs:448-449`). Elements paint in tree order, so user bubbles with a
  background (`chat.rs:738`) and the composer paint **over** the menu.
- Hit testing: `hit_test` walks hitboxes in reverse paint order and only
  stops at `HitboxBehavior::BlockMouse` (`gpui:src/window.rs:775-797`). The
  menu items still receive clicks, but transcript hitboxes under the menu also
  report hover (session rows, buttons) at the same time.
- No dismiss on outside click or Escape.

**Fix.** Wrap the menu in `deferred(..)` so it paints after the rest of the
tree (`gpui:src/elements/deferred.rs:7`, `.with_priority(n)` at `:25`), and
mark the menu div `.occlude()` (`gpui:src/elements/div.rs:575`
`occlude_mouse`, fluent form on `InteractiveElement`) so it blocks hitboxes
below it. For dismiss, add `.on_mouse_down_out(MouseButton::Left, ..)`
(`gpui:src/elements/div.rs:777`) on the menu to set
`models_menu_open = false`. `anchored()` (`gpui:src/elements/anchored.rs:27`)
is the alternative if the menu should flip when it hits the window edge.

---

## 5. SHOULD-FIX — Global (`None`) key bindings shadow future contextual ones

**File:** `app/src/ui/text_input.rs:37-57`

**Problem.** A binding with `context: None` is enabled at depth
`contexts.len()` — the deepest possible — for every focus target
(`gpui:src/keymap.rs:209-214`). Matches sort deepest-first, then latest-added
first (`gpui:src/keymap.rs:165-167`). Any later `KeyBinding::new("ctrl-a", X,
Some("Sidebar"))` therefore loses to this global `SelectAll` unless it is
added after it. Today nothing else binds these keys, so there is no visible
bug: when no `on_action` handler is on the dispatch path,
`dispatch_action_on_node` is a no-op, `propagate_event` stays `true`, and the
key falls through to key-down listeners (`gpui:src/window.rs:3834-3847`). The
keystroke is **not** swallowed.

**Fix.** Scope every binding: `KeyBinding::new("ctrl-a", SelectAll,
Some("TextInput"))`. The div already sets `.key_context("TextInput")`
(`text_input.rs:681`), which is what the predicate matches
(`gpui:src/keymap/context.rs:253-275`).

---

## 6. SHOULD-FIX — Auto-scroll fights the user while streaming

**File:** `app/src/ui/chat.rs:364`, `:399`

**Problem.** `scroll_to_bottom()` is requested on every timeline event. The
request is a flag (`gpui:src/elements/div.rs:3203-3206`); the transcript div
consumes it in `clamp_scroll_position` and sets `offset.y = -scroll_max.height`
(`div.rs:1734`, `:1755-1756`). It is idempotent and does not call `notify`,
so there is no repaint loop. But a user who scrolled up to read is yanked to
the bottom on every token.

**Fix.** Only request when already at (or within a few px of) the bottom:

```rust
let at_bottom = self.scroll.offset().y <= -self.scroll.max_offset().height + px(4.);
if at_bottom { self.scroll.scroll_to_bottom(); }
```
`offset()` at `div.rs:3083`, `max_offset()` at `div.rs:3088`. Both are
updated each prepaint (`div.rs:1761-1764`). Keep the unconditional call in
`set_active_session` (`chat.rs:222`).

---

## 7. NIT — `TextElement` contract matches the reference exactly (no action)

**File:** `app/src/ui/text_input.rs:504-675`

Verified against `gpui:examples/input.rs:529-562`: `handle_input` is
registered first, selection quad, then line paint, then cursor only when
focused, then `last_layout` / `last_bounds` stored **after** paint. Identical
ordering. `request_layout` uses `relative(1.)` width and
`window.line_height()`; `Element::id` returns `None`, so no element state is
allocated. The cursor is always on: it is a `paint_quad` with no `notify`, so
there is no repaint loop. Focus handle is created once in `TextInput::new`
(`text_input.rs:79`) and cloned per frame; correct lifecycle.

---

## 8. NIT — Enter listener also fires for `shift-enter` / `ctrl-enter`

**File:** `app/src/ui/text_input.rs:702`

`keystroke.key == "enter"` ignores `keystroke.modifiers`
(`gpui:src/platform/keystroke.rs:18-21`). If you later add
`shift-enter` = newline, this branch still sends. Resolved for free by fix
option 2 in item 1 (action bindings are modifier-exact).

---

## 9. NIT — `.id("transcript")` is not required for `track_scroll`, but keep it

**File:** `app/src/ui/chat.rs:635-640`

With `tracked_scroll_handle` set, the div takes its offset from the handle
before consulting element state (`gpui:src/elements/div.rs:1601-1602`); the
`element_state` path (`:1603-1613`) is only for untracked `overflow_scroll`.
So the handle works with or without `.id()`. Keep the id anyway: it is the
documented pattern and future click/hover listeners need a stateful div.

---

## 10. NIT — `into_any_element()` on `Entity<_>` is correct

**File:** `app/src/main.rs:37-40`

`impl<V: Render> IntoElement for Entity<V>` is at `gpui:src/view.rs:296-302`.
`into_any_element()` comes from the `IntoElement` blanket method. A `match`
that returns `AnyElement` is the standard way to select between two view
types. No change.

---

## 11. NIT — `assert_eq!(last_layout.text, self.content)` can panic across a frame

**File:** `app/src/ui/text_input.rs:479`

`character_index_for_point` compares the previous frame's shaped text with the
current content. Between `set_text`/`clear` and the next paint they differ. The
example has the same assert (`gpui:examples/input.rs`), so this is inherited,
but a hosted IME query in that window would abort the app. Return `None`
instead of asserting.

---

## 12. NIT — Login subscription lifetime

**File:** `app/src/main.rs:84-96`

`cx.subscribe(login, ..).detach()` is fine: a detached subscription is released
when the emitter entity drops (`gpui:src/app.rs:865-877`,
`subscribe_internal` holds a weak handle). After `app.screen` is replaced the
`LoginScreen` entity has no strong refs and the listener goes away with it.
