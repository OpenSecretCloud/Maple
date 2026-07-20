# Maple patches to pdf-extract 0.12.0

This directory starts from the published `pdf-extract` 0.12.0 crate:

- upstream commit: `b95bf9f6268772d5088f09b0034e488e64294835`
- crates.io archive SHA-256:
  `417e8fdc940f1d5bc62c5f89864c3a2255f74f69aa353c98509213d67df61e73`

Maple carries three narrowly scoped changes:

1. Prefer an explicit PDF `/Encoding` over an embedded CFF font's built-in
   encoding. This prevents confirmed accented-text corruption introduced in
   0.11.0.
2. Cache fonts by the resolved font dictionary rather than by resource name.
   Resource names are local to each PDF resource dictionary, so a document may
   legally reuse `/F1` for different fonts across pages or nested XObjects.
3. Do not log the contents of Type 4 calculator-function streams. Those bytes
   are user-controlled document content, and the upstream UTF-8 conversion can
   itself panic.

The Type 4 handling and `lopdf` 0.42 dependency otherwise remain exactly as
published in 0.12.0. Maple's PDF extractor tests cover the Type 4 crash and
both page-level and nested-XObject resource-name collisions. The CFF fix was
also compared against the upstream affected corpus during the upgrade audit.
