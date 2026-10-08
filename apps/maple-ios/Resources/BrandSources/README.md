# Native brand artwork

`app-icon.svg` is the original Maple glyph and gradient. The app icon is rendered
with librsvg at 4× its final resolution and downsampled to 1024×1024. Its original
transparency is flattened over Neutral 0 (`#FFFFFF`) for an opaque native app
icon; the glyph, gradient and vector geometry are unchanged. The visible app name
remains Maple Native Dev.

`AppIcon-PROVENANCE.json` records the exact source and output hashes and renderer.
Do not substitute a renderer that omits SVG gradient transforms. The wordmark in
the UI resource asset catalog remains a vector template, so native foreground
colors can follow light and dark appearance.
