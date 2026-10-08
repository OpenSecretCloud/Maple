# Pinned Textual runtime

Upstream: https://github.com/gonzalezreal/textual.git, revision `01b51875a5406eefc95f52a058cb059e7bc94dc4` (MIT, license retained verbatim).

The 193 production source/resource files are copied from that verified clean commit with four bounded changes to Copy export and embedded control interaction:

- `Internal/Formatting/Formatter+PlainText.swift` exports an explicit line break as LF instead of a space.
- `Internal/Formatting/Formatter+HTML.swift` exports literal LF in ordinary text nodes as `<br />`. Native attributed selection can retain the visible LF while losing the inline line-break intent. Encoding that LF as an HTML whitespace entity collapses it to a space on import. Escaping, code nodes and preformatted code blocks retain their existing behavior.
- `Internal/TextInteraction/UIKit/UITextInteractionView.swift` publishes the plain representation as explicit UTF-8 bytes under `public.utf8-plain-text`, alongside the HTML representation. Plain text retains canonical LF even when HTML consumers use other line-separator conventions.

- `StructuredText/Style/Overflow.swift` adds a scoped `textSelectionInteractionExcluded()` modifier for embedded controls. It publishes the control's actual bounds through the existing selection-overlay exclusion preference. Maple applies it to the code Copy button, whose fixed header is outside the horizontally scrolling code region. This allows ordinary button taps to reach the control while leaving the surrounding text selectable.

The changes preserve native selection, paragraph structure, inline formatting and rich HTML Copy. They do not modify transport, caching, JavaScript or dependency versions. Focused tests cover the native attributed-string bridge, plain and HTML breaks, escaped user text, Unicode, inline formatting and code whitespace. Native partial-selection Copy and embedded Copy-button delivery remain separate simulator validation gates; unit-level HTML import does not prove UIKit clipboard behavior.

The Swift 6.0 package manifest retains upstream platform/product/production target settings and dependency requirements, excluding only the upstream standalone SnapshotTesting dependency and test target. Those upstream snapshots/fixtures are not vendored. Maple retains its exact ConcurrencyExtras and SwiftUIMath revision pins and adds focused regression tests through its existing UI unit-test target. UPSTREAM.json records SHA-256 for every copied production file before and after these patches.

Do not hand-patch SwiftPM build checkouts. Future updates should regenerate provenance from an exact clean upstream revision and re-evaluate whether these Copy fixes are still needed. The upstream MIT notice must remain with distributed code.
