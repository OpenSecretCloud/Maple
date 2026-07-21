# Maple patches to pdf-extract 0.12.0

This directory starts from the published `pdf-extract` 0.12.0 crate:

- upstream commit: `b95bf9f6268772d5088f09b0034e488e64294835`
- crates.io archive SHA-256:
  `417e8fdc940f1d5bc62c5f89864c3a2255f74f69aa353c98509213d67df61e73`

Maple carries four narrowly scoped changes:

1. Prefer an explicit PDF `/Encoding` over an embedded CFF font's built-in
   encoding, while preserving that built-in encoding as the base for symbolic
   or nonsymbolic embedded-font `Differences` dictionaries, using the standard
   Symbol and ZapfDingbats built-in encodings when those fonts are not embedded,
   leaving unmapped built-in slots empty, and treating every `Differences` entry
   as a replacement for its base slot. Out-of-range embedded Type 1 codes are
   ignored. Unknown FontAwesome glyph names retain an explicit `/ToUnicode`
   mapping, while ZapfDingbats-only names are not applied to unrelated fonts.
   This prevents confirmed character corruption
   introduced after 0.10.0 and shipped in 0.12.0. The implementation is based on
   [upstream PR #155](https://github.com/jrmuizel/pdf-extract/pull/155) at
   commit `d87b1f4c05778bd45b17aec995ed00a0dab37624`, with the embedded-font
   implicit-base and unmapped-slot corrections required by PDF 32000-1 Table
   114.
2. Cache fonts by the resolved font dictionary rather than by resource name.
   Resource names are local to each PDF resource dictionary, so a document may
   legally reuse `/F1` for different fonts across pages or nested XObjects.
3. Do not log the contents of Type 4 calculator-function streams. Those bytes
   are user-controlled document content, and the upstream UTF-8 conversion can
   itself panic.
4. Reject recursive Form XObject-and-resource pairs and cap Form XObject
   nesting at 100 levels. Cycles otherwise recurse until the native stack
   overflows and aborts the process, which cannot be recovered by a Rust panic
   boundary. Including the effective resources in the cycle key preserves
   valid finite re-entry through inherited resource dictionaries.

The Type 4 handling and `lopdf` 0.42 dependency otherwise remain exactly as
published in 0.12.0. Maple's PDF extractor tests cover the Type 4 crash and
both page-level and nested-XObject resource-name collisions. They also cover
the CFF named, implicit, and `Differences` encoding modes above; Symbol and
ZapfDingbats `Differences`; preservation of explicit `/ToUnicode` mappings;
recursive and excessively deep Form XObject errors; and valid finite Form
re-entry and sibling reuse.
