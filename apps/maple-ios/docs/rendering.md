# Message rendering

`MessageView(text:theme:font:accountGeneration:onOpenLink:)` is the shared
SwiftUI message component on iOS26. Pass the active account's revocable generation.
The parent owns confirmation before opening any callback URL. The renderer admits
only HTTP(S) links without embedded credentials and provides no image loading.

Foundation Markdown plus pinned Textual handles structure, native text selection,
lists, tables and inline styling. App normalization matches Research's Markdown
configuration: soft source line breaks become rendered line breaks, real GFM task
markers become read-only checked/unchecked symbols, and accidental indentation
becomes ordinary prose with inline formatting. Source positions distinguish
escaped task markers and quoted/nested fences. Inline code and link destinations
retain their parsed meaning; normalization does not rewrite their source globally.
Fence classification reads the parsed node's opening source line, including when
Foundation reports an unusable ending position at a following blank line. Its
lookup honors CRLF, LF and CR line endings without rewriting the original source.

Tables measure bounded cell widths before the horizontal scroller proposes its
viewport width. Cells scale with Dynamic Type; the outer grid keeps each Markdown
column's alignment and lets wide tables scroll rather than squeeze every column.

Fenced code uses the app's bounded native lexer and explicit copy action. Unknown
languages and excessive highlight work retain the entire code as plain text.
Every actual code block, including quoted and nested fences, receives an owned
hint so it cannot enter implicit math or JavaScript highlighting paths in the
dependency. Fenced `math` remains literal code; indentation alone does not suppress
ordinary prose or formulas.

Textual's exact pinned runtime is vendored under `Vendor/textual`, with the MIT
license and per-file upstream/local hashes. Three bounded Copy changes preserve
explicit breaks as LF in plain text, preserve literal LF as `<br />` in ordinary
HTML text nodes, and publish plain text as concrete UTF-8 bytes alongside HTML.
Native selection can preserve visible LF while dropping its inline break intent;
HTML must retain that break even when it arrives without the semantic attribute.
Escaping, code whitespace and the upstream selection implementation are unchanged.
Runtime fixtures exercise the rendered-text-to-formatter and system-pasteboard
boundaries separately from formatter tests. The upstream standalone snapshot-test
tooling is omitted; app regression tests cover the patches. See `Vendor/textual/MAPLE-PATCH.md` and `UPSTREAM.json` for provenance.
Do not patch generated SwiftPM checkouts or reformat the vendored runtime.

Native partial-selection Copy writes plain text and HTML to the system pasteboard
through Textual. The app's explicit whole-message and code Copy actions use
local-only pasteboard entries with a two-minute expiry.

A message-owned actor parses Markdown away from the UI actor. A main-actor owner
throttles publication without restarting its deadline for every streamed token.
Equation enrichment is separate and canceled by a new revision. Generation
revocation clears retained output and rejects late preparation/enrichment results.
Disappearance cancels work. While the exact account generation and selected chat
owner remain current, completed message-local projections survive lazy scrolling
and temporary menu occlusion. Revisiting a completed reply reuses its formatting
instead of switching to raw Markdown and changing its height. Incomplete work
still restarts on return; replaced owners clear or release their view state.
There is no document or formula cache shared across accounts.

## Equations

The app recognizes dollar, double-dollar, `\(...\)` and `\[...\]` delimiters.
Numeric-leading single-dollar expressions stay literal for compatibility with the
existing Research renderer; explicit backslash delimiters can express those
formulas. Inline/fenced code and link destinations are never interpreted as math. Incomplete
or rejected formulas retain their complete original source, including backslashes.

The evaluated native equation libraries do not cover the required command set:
SwiftUIMath0.1.0 and SwiftMath1.7.3 reject `operatorname`, `Bigl`, `tag`, `boxed`
and `newcommand`. An iOS26.5 simulator probe confirmed blank output for these
commands with SwiftUIMath, while a fraction/square-root control rendered.

Only equations use bundled KaTeX0.18.10 in a short-lived WKWebView. The transcript
remains native. The equation process receives formula strings and a display-mode
boolean through typed JavaScript arguments. It has no account identity, SDK,
credentials, message handlers, arbitrary HTML input or network loader. A
nonpersistent data store, restrictive CSP, WebKit content blocker, navigation
policy and exact packaged-resource allowlist enforce the boundary. External
images, fonts, frames, media, fetches, forms, popups and navigation are denied.
Only packaged JS, CSS and woff2 fonts are admitted by the custom URL scheme.

Each `MessageView` retains its own model and noninteractive UIKit host. The
renderer attaches outside that host's exact active window before loading the
bundled page; initial mounting shares the existing 12-second navigation bound.
Reparenting, removal, scene suspension and cancellation invalidate that host's
renderer. No application window search or shared formula host is used.

After font-ready measurement, one bounded snapshot temporarily detaches only the
owned web view and uses `afterScreenUpdates:false` for page capture. Its exact
host, container and active window must remain current before reattachment or
publication; failure and cancellation leave the web view detached. The existing
three-second capture bound, eight-second enrichment budget and raster leases
remain unchanged. Equation pixels stay in memory.

KaTeX uses `trust:false`, strict errors, fresh macro dictionaries, a finite
expansion limit and bounded size. Input, depth, cells, generated HTML, DOM nodes,
image dimensions and execution time are bounded. Successful equation snapshots
stay in memory with their owning message; original TeX supplies copy and
accessibility text. Parsed equation accessibility sources remain available while
rasterization is pending or a formula retains its original source. Cancellation and timeout discard the web view. A global
content-free admission queue permits one live renderer and at most16 waiting
jobs. Reference-counted content-free leases bound retained decoded raster cost
to16MiB across messages; failed admission or reservation leaves selectable TeX.
The WebKit content-rule store contains only the fixed network policy.

Asset provenance, tarball integrity and license notices are bundled under
`Sources/MapleAppUI/Resources/EquationRenderer`. KaTeX's
[security guidance](https://katex.org/docs/security.html) and
[rendering options](https://katex.org/docs/options.html) describe the upstream
controls; native resource and lifecycle restrictions are app-owned.

## Verification

Run the component unit recipe in the pinned Apple shell. Renderer tests cover
source-preserving tokenization, final Markdown output, safe URLs, GFM intent,
code ownership, soft/hard line-break Copy in both export formats, escaped task
markers, indented prose, streaming publication, generation fencing, work admission,
raster reservations and the exact bundled KaTeX trust/macro configuration.

The debug-only `--maple-renderer-fixture` launch argument presents synthetic
content without account access. `RendererUITests` exercises native selection,
streamed append and code copy on the real app. Native simulator checks must also
verify equation pixels, blocked network attempts, light/dark output, Dynamic
Type, RTL and selection across paragraph/code/table boundaries. Build or unit
success alone does not prove those runtime behaviors.

`ShippingViewportUITests/testCompletedRichRepliesStayPreparedDuringRepeatedHistoryScrolling`
uses six completed rich turns through the production chat screen. It checks
repeated earlier/later scrolling, native code/link controls, settled geometry,
and the exact prepared projection retained when a lazy row disappears. Its
debug-only diagnostics contain counts and booleans, never message contents.
