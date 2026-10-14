# Native trace review images

The trace, annotation and round-0 audit commands now produce their review images
in Rust. No Python, OpenCV, external font file or network request is needed.

```sh
ftx trace sheet.png --out traced --json
ftx annotate traced/draft.pattern.json --names names.json \
  --out named.pattern.json --overlay named.png --image sheet.png --json
ftx audit named.pattern.json --out audit-round0 --image sheet.png --json
```

## What to inspect

`ftx trace` writes the existing `draft.pattern.json`, `mask.png` and
`trace_px.json`, plus `overlay.png` and `labels/p1.png`, `labels/p2.png`, etc.
The overlay marks each piece, edge and corner, quarter/midpoint ticks, notch
guesses, grain arrows and internal marks. It keeps the source crop and the
piece-to-image affine transforms; mirrored annotations are not mirrored twice.
Its header preserves the scale status and labels the image **not for cutting**.

The label images are deliberately unannotated: source pixels inside each
stroke-centre polygon are preserved, while the outside is washed toward white.
Each crop has a 4% margin, clipped to the image, and is enlarged when needed to a
600-pixel long side. This helps read the source's printed text without new labels
covering it. Source-text interpretation and `names.json` authoring remain external.

`ftx annotate --overlay PNG` draws the named result. `--image` is optional; without
it the renderer uses a white background. It retains source transforms when all
pieces have them and otherwise packs the drawn pieces. The annotated pattern's
bytes are the same with and without the overlay option, including vertex order.

`ftx audit` writes `round0_audit.png` and the existing `round0_audit.json`. The PNG
shows grey outlines, red referenced geometry, finding IDs and a complete wrapped
legend. References without drawable geometry remain in the legend. A structure
failure can produce a text-only audit rather than invented geometry. The audit
status and exit code still come from the JSON findings, not from rendering.

Only an explicit `--image` authorizes reading a source image. A path stored in
pattern metadata is never followed automatically. The supplied image must match
the declared dimensions, crop and usable affine transforms. These checks do not
prove that an equally sized replacement image has the same content.

## Output protection and limits

Choose fresh artifact paths. Trace/audit refuse existing output artifacts and
trace also refuses an existing `labels` directory, including links, so stale
crops cannot masquerade as this run's pieces. Unrelated files in a non-symlink
destination directory are allowed. With annotation overlays, both the pattern
and PNG output must be new; ordinary annotation without `--overlay` is unchanged.

All computations and serialization finish before publication starts. Files are
created exclusively, with the primary JSON last. A write failure can leave an
incomplete new output for inspection; there is no claim of a multi-file atomic
filesystem transaction. Retry with fresh paths rather than reusing a partial run.

Review limits include 16 million source pixels, 16 million pixels per output and
across all label crops, 128 pieces, 100,000 sampled geometry points and an 8192-pixel
output side. Audits allow at most 512 findings and 1024 references, with bounded
text. Limits refuse rather than silently dropping findings, pieces or pixels.
The lower review limit applies to the CLI even though the general PNG decoder's
default remains 40 million pixels. No resize or reduced-resolution trace is hidden.

## Parity boundary

The native diagrams use the existing uppercase ASCII stroke font, clipped solid
lines and nearest-pixel image sampling. They are **not pixel-identical** to the
reference's OpenCV 5 TrueType/antialiased/cubic-rendered images. Exact, case-sensitive
identifiers and complete messages remain in JSON; unsupported font characters may
display as question marks. Source label crops contain no generated text.

Tracing geometry, masks, annotation documents, audit findings and scale-fit are
unchanged. Passing a visual review does not establish scale, sewability or fit;
UNSCALED traces still cannot produce true-size exports. The reduced-precision
buffer retry remains incomplete (JPEG and WebP: `ftx_trace::raster::{jpeg, webp}`). The
I/O-free `ftx_trace::review::{overlay, label_crops, audit_image}` APIs build for
WebAssembly, but this change does not add browser-page controls or wire the
separate skill repository to the native images.

Eight renderer tests and five real-CLI tests cover the new products, transform
and pixel checks, deterministic output, preserved documents/verdicts, unsupported
input, stale artifacts and output links. Existing tracing oracle fixtures remain
unchanged; they test geometry and documents, not native-diagram pixel parity.
