# Software notices

These license texts are reproduced verbatim for the Maple app and the Swift
runtime packages it consumes. `PROVENANCE.json` records the exact source
revision and SHA-256 of each notice. The MapleAppUI resource target copies this
whole directory into the application without localization or text conversion.

Textual is the locally vendored runtime. `Textual-UPSTREAM.json` records every
production file's upstream and local digest; `Textual-MAPLE-PATCH.md` describes
the bounded Copy export and embedded-control interaction corrections. Textual also packages Prism.js 1.29.0. Its
complete MIT notice is included even though Maple uses its own code highlighting path.
The Prism notice was verified against both its official v1.29.0 tag and the
resolved commit. The packaged JS digest matches Textual's pinned upstream file.

SwiftUIMath and ConcurrencyExtras are pinned Swift package runtime dependencies.
SwiftUIMath's copied math font bundle already contains its original MIT, OFL,
and GUST notices. KaTeX and its fonts retain their original notices in the
EquationRenderer resource directory. Array and Manrope notices remain in the
app's FontLicenses directory. The corresponding paths and hashes are also
listed in `PROVENANCE.json`; the font files and notices are not modified here.

Upstream test-only packages such as SnapshotTesting are not runtime products
in Maple's resolved package graph. System frameworks are supplied by iOS.
Updating a dependency requires refreshing the relevant notice and provenance,
and checking the final app bundle; a source copy alone is not packaging proof.
