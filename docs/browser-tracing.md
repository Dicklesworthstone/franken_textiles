# Trace and review a sheet in the browser

Build the existing WebAssembly bundle, serve `web/`, and open `trace.html` (also
linked from the Pattern Workbench):

```sh
sh tools/build_web.sh
python3 -m http.server -d web 8000
```

The build needs the pinned Rust toolchain and `wasm-bindgen-cli` 0.2.128. An old
`web/pkg` without `WebTrace` will not work; rebuild it together with the page.
`CARGO_TARGET_DIR` is honored by the build script. The sheet, its names and its
measurements stay in the page's worker; no evidence is uploaded or stored in
browser persistence. Only the application module is fetched.

## Workflow

Choose an already correctly oriented PNG, JPEG or WebP and press **Trace sheet**. The controls
expose the native channels, global Otsu or adaptive Gaussian threshold, polarity,
optional crop/stroke width, area floor and dashed-line bridging. Inspect the
overlay and unannotated source-label crops before interpreting printed labels.
The initial pattern remains `UNSCALED`.

Apply the native `names.json` format to name pieces, edges, seams and construction,
for example:

```json
{"pieces":{"p1":{"id":"front","name":"Front"}}}
```

Known lengths use the renamed pattern's references and **explicit millimetres**:

```json
[{"reference":"front/e1","target_mm":200}]
```

Leave `[]` when no reliable length is known. Optional `labels.json` evidence uses
the native audit list. A missing/invalid reference is refused; disagreeing scale
targets retain their residuals and warning. Scaling is not sewability or fit.

Every **Apply edits and audit** starts from the original trace, not the previous
edit. Repeating a mirror or scale cannot accumulate it. Removing the scale targets
restores an unscaled draft. Changing the image or trace settings requires a new
trace and resets the edits. Failed edits can be corrected against the same original
sheet, but never leave the earlier review's downloads visible. Cancel terminates
the worker, discards its sheet and starts a new engine.

Download individual files or `trace-review.zip`. The archive contains the original
draft, mask and piece crops; current `pattern.json`, overlay and illustrated audit;
names, scale-target and label inputs; and `review.json` with the source SHA-256,
scale residuals and complete diagnostics. It excludes the original uploaded PNG.
The archive timestamp is fixed at 1980-01-01; no capture date is inferred.

To continue to manufacturing, choose **Check in Pattern Workbench** to open a new tab
with the current pattern. The tabs exchange an immutable file locally, using the same
origin, opener window and one-use request token. No pattern enters browser storage or
a server. The Workbench selects **Check my own pattern.json** and shows the source image
hash; choose measurement evidence and a fit contract, then explicitly **Draft and check**.
The handoff does not apply measurements, run a round or approve cutting. Source edits
before the receiving tab requests the file cancel the transfer. Choosing a local pattern
file, another garment or **Clear tracing handoff** cancels pending or imported data.
Blocked popups can use the ordinary file route instead: download `pattern.json`, return to the Pattern
Workbench and choose **Check my own pattern.json** with your measurement evidence
and fit contract. The workshop itself never creates PDF, DXF or other cutting
files. Its `REVIEW_ONLY` status is not a convergence verdict.

## Limits and verification

The browser rejects PNGs over 16 MiB or four million decoded pixels **before
inflation**; it never silently resizes. Crop rectangles must be wholly inside the
image. Each JSON input is at most 256 KiB, with at most 64 unique scale targets
and 512 label records. The native review's piece/geometry limits also apply; final
review products are capped at 64 MiB. JPEG (CMYK too) and WebP sheets decode as the reference decodes
them (`ftx_trace::raster::{jpeg, webp}`, ports of libjpeg-turbo's and libwebp's decode paths; the pixel
budget is checked before any raster is allocated). A WebP's transparency is composited on white, as
`load_image` does; an animated WebP gives its first frame, as `Image.open` shows it. Like the reference,
the trace ignores EXIF rotation. The review's `pattern.json` is byte for byte the file `vectorize.py
annotate` and then `scale-fit` write (`ftx_trace::py_order`). Inferred labels and interactive curve
editing are not implemented here.

**Save edits draft** downloads unfinished names, scale targets and audit labels before or
after applying them. To reopen it, select the original image, choose the original tracing
settings and **Trace sheet**, then **Load edits draft**. Loading checks the encoded image's
SHA-256 and exact tracing-options text before replacing all three editors. It does not apply
the edits; use **Apply edits and audit** and review all ordinary scale/audit findings. Invalid
JSON inside an editor can be saved for repair; it remains invalid when the Rust review runs.
Drafts contain no source image, cutting approval or body-fit evidence. Keep them privately.

The browser draft object has exactly six fields: `schema: "ftx-trace-edits/1"`,
`source_sha256`, and the original `options`, `names`, `targets`, `labels` JSON texts as strings.
The whole file is at most 8 MiB, each editor text at most 256 KiB. Restored editor text is
passed unchanged to Rust until that editor is changed. Wrong-source/settings or malformed
drafts preserve current edits; cancellation/source changes discard late reads. Editing
withdraws a prepared draft link, so prepare a fresh download after editing. A hand-edited
draft is not an authenticity certificate or an oracle fixture.

The APIs are `tracing::Sheet::{new,review}` (native-testable, no I/O) and
`WebTrace`/`WebTraceReview` (the wasm-bindgen boundary). Five native workflow tests
compare geometry/masks/review images with the native functions and exercise
repeatability, refusal isolation, scale disagreement and crop/budget checks.
Setting `FTX_TRACE_SAMPLE_DIR` while running `ftx-wasm --test tracing` writes those
synthetic examples for inspection; these are not oracle fixtures.

`node --experimental-vm-modules --test tools/check_trace_frontend.cjs` checks the
actual page/worker state transitions using explicit DOM/WASM doubles. Its eight
tests cover ownership, failures, stale replies, cancellation and download URL
invalidation. These tests are **not real browser rendering or WebAssembly
execution**. The hosted workflow separately checks browser-target compilation
and can build a bundle with a `[browser-bundle]` commit. Native trace diagrams
retain the documented stroke-font/nearest-pixel deviation from OpenCV rendering.
