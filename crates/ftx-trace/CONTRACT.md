# ftx-trace

**Purpose.** Tracing pattern sheets (plan §13, spec 04 §10): a raster sheet (an image model's pattern, a scan,
a screenshot) becomes an UNSCALED `fabric-pattern/1` draft, 1 unit = 1 crop pixel, which is then named,
scaled and audited, all from the reference's `vectorize.py`: the trace itself (`trace_image`: channel and
threshold, contours, stroke centres, dashed-edge bridging, corners, edges, marks), `names.json`
(`annotate`), the reference grammar (`measure_ref`, `resolve_side`, `twin`), `fit_scale`, the round-0
`audit` with its label consensus, and `parse_length`. Not yet: the trace's overlay and label images, the
audit's PNG, the adaptive threshold, JPEG and WebP input.

**Units and frames.** A traced draft is in crop pixels, y up, loops counter-clockwise; `trace.to_px` maps
piece coordinates back to source pixels and is kept current through mirroring and scaling. After scale-fit
the pattern is in millimetres (`meta.scale_status` FITTED).

**Reading documents.** Drafts and patterns are typed `PatternDoc`s parsed from text (vertex order and unknown
keys such as `trace` survive); `names.json` is an `OValue`, a JSON value whose objects keep their order,
because renames, splits and the reference's error messages follow it (a `serde_json::Value` sorts keys).

**annotate** (spec 04 §10.10). Per piece, in the reference's order: `merge_at` and `split` with the traced
ids, renames (ids checked, no collisions), notch and mark edge renames, `cut` (qty 1 or 2, `on_fold` renamed
and checked), `drawn_side: L` mirroring about the bbox centre (vertices r3, edges reversed with mirrored
curves, notch fractions, grain and its angle, mark points, `to_px` composed), the copied keys and the new id.
Merges and splits re-fit their edges: a line when every point lies within max(1e-3·size, 1e-6) of the chord,
else a relative poly of the points GEOS's Douglas–Peucker keeps at a third of that (r5). Then seams (sides
resolved through the rename maps, shorthand expanded, `band` → `ratio_band`, left twins when either side was
shorthand) and checks; the result must pass the structure check. Errors are the reference's text.

**measure_ref** (spec 04 §10.11). `INST/eA[+eB]` a seam side as sewn, `PIECE/eA[+eB]` edges as drawn (fold
edges allowed), `PIECE:cA-cB[:x|:y]`, `PIECE:width|height`, `seam:ID[:a|:b]`; `KeyError` texts print quoted
as Python's `str(KeyError)` does.

**fit_scale** (spec 04 §10.14). `k = Σaᵢ / Σaᵢ²`, `aᵢ = Lᵢ/Tᵢ` (numpy's pairwise sums); vertices, grain and
mark points r4, offsets r3, `to_px` columns divided by k; the report (`k`, `mm_per_px`, targets with
residuals, rms, the 2 % warning) is also stored as `meta.scale_fit`.

**audit** (spec 04 §10.12–10.13). Scale basis FITTED, LABELS (a non-weak consensus) or NOMINAL (the median
seam side, or instance edge, as 250 mm); findings in the reference's order and words: structure, seam
mismatch (band, declared ratio, plain walk; gathers unchecked), ratio checks and bad check refs, fold
squareness (Richardson-extrapolated end tangents), declared symmetry (GEOS's discrete Hausdorff distance
over 41 axis offsets, with `DistanceToPoint`'s alternating segment ends), cut-off pieces, inventory,
unsewn pieces, no seams, and the label findings. The label consensus is exhaustive RANSAC over the labels'
implied scales (ties by the smallest log spread; a disjoint group as large makes it weak), the scale the
geometric median of the winners. Logs, exponentials and angles go through `ftx_core::det`.

**Parity.** `tests/fixtures/oracle/trace/post.json` (captured by `tools/oracle/capture_tracing.py` from
drafts the reference traced off its own synthetic test sheets: plain, navy and filled) is reproduced
exactly by `ftx-conformance/tests/tracing.rs`: 408 cases (52 `parse_length`, 298 `measure_ref`, 33
`annotate` including 20 refusals, 7 `scale-fit`, 18 audits covering every finding kind), all 6,241 numbers
bit-identical and every message equal. `parse_length` reads ASCII digits (Python's `\d` also takes other
Unicode decimal digits).

**Raster** (`raster/`, for the trace; oracle `trace/raster_*` from `tools/oracle/capture_raster.py`, test
`ftx-conformance/tests/trace_raster.rs`). `load_png`: Pillow 12.3's `load_image` semantics (palette, gray+alpha
and RGBA composited on white with Pillow's fixed-point `alpha_composite`; 1/2/4-bit gray scaled, 16-bit gray
clipped at 255, 16-bit colour to its high byte; gray/RGB `tRNS` ignored), every filter and Adam7: 55 PNGs
bit-identical. JPEG and WebP are refused (convert to PNG): matching libjpeg-turbo and libwebp is not done.
Colour (OpenCV 5.0 `cvtColor`, 8-bit): Lab is OpenCV's integer formula over tables built as OpenCV builds them,
including its truncating `softfloat` cube root, bit-identical over all 16,777,216 colours; the `auto`
channel (float32 Lab distance) bit-identical. Gray and HSV saturation are OpenCV's own fixed-point code,
but the reference's arm64 OpenCV runs them through its KleidiCV/carotene HAL, which rounds 0.24 % (gray) and
0.02 % (saturation) of pixels one level differently and even depends on a row's memory offset: they agree
within one level, not bit for bit (on x86 the reference's own results would differ again). Otsu, binary
threshold, elliptical and rectangular morphology, `findContours` (`RETR_EXTERNAL`, `RETR_CCOMP` with its
hierarchy), `contourArea`, `arcLength`, `moments`, `boundingRect`, `approxPolyDP`, `convexHull`,
`distanceTransform` (L2, masks 3 and 5), `connectedComponentsWithStats` (8-connected, OpenCV's label order),
`HoughLinesP` (its random sequence, and the accumulator's `j·cos + i·sin` as the reference's clang build fuses
it), `fillPoly`, filled `drawContours` and thick lines are bit-identical to OpenCV 5.0 on the three noisy
benchmark sheets and synthetic cases (33 binarize, 13 contour, 12 distance, 6 Hough, 58 drawing cases).

**Buffer** (`buffer.rs`). shapely's `Polygon.buffer` for a shell: GEOS 3.14.1's ring offset curve line for
line (`BufferInputLineSimplifier`, `OffsetSegmentGenerator` with mitre, limited mitre, bevel and round joins,
`LineIntersector`, the double-double line intersection, `sinCosSnap`), then the curve's self-intersection
loops cut away where GEOS nodes and polygonizes. On 42 cases (the sheets' stroke-centre offsets and marks
insets, stars, ells and slivers both ways round with both joins) every stroke-centre ring is identical and
24 cases in all; the rest are the same ring from another start point or within 1.4e-13 px (round joins use
`det::sin_cos`). Plus `LineString.distance` and `project(normalized=True)` for points.

**Trace** (`trace.rs`, spec 04 §10.2–10.9). `trace_image` in the reference's order: crop, erase (painted
with the background), the channel (`auto`/`bgdist` Lab distance from the frame's modal colour, `dark`,
`light`, `gray`, `sat`, `r`/`g`/`b`, `edge`), Otsu with the frame deciding polarity and a closing, the pieces
(`RETR_CCOMP` outer contours above the area floor, ring or fill by ink fraction, the stroke width from the
hole's area or the distance transform's ridge, each solid opened then closed), dashed runs bridged
additively, the panel retry (a contour over 60 % of the crop re-estimates the background inside it), the
reading order, and per piece: the contour resampled at 1 px and smoothed, offset to the stroke centre
(mitre, limit 4), flipped to y up and counter-clockwise, resampled at max(0.5, size/1500), corners (sharp by
turning over L, soft over 3L, splits to reach three) with numpy's `argsort` tie order, edges (a line, or a
relative `poly` of GEOS's Douglas–Peucker points when the run's total-least-squares deviation exceeds the
tolerance), confidence, and the marks (dashed groups and fold candidates, grainline or the Hough fallback,
dimension arrows, parallel inner lines, notches with their edge fraction). numpy's evaluation is kept where
it decides bits: `interp` fused, the Gaussian through `small_correlate` (a sequential fused sum, exact up to 11
taps, i.e. `smooth_px` ≤ 5/3; longer kernels go to BLAS in the reference and are not reproduced bit for
bit), float32 medians and percentiles of distance values, `argsort`'s introsort. Values the reference gets
from Accelerate (`np.cov` and `eigh` for a mark's axis, `svd` for straightness, BLAS dot products) agree to
about 1e-16 and decide only thresholds before rounding. `trace_mask` continues from a binarized mask.
The draft is a `PyJson` written as the reference's `json.dump(indent=1)`.

**Trace parity.** `tests/fixtures/oracle/trace/raster_trace.json` (22 cases: outlines on white and navy,
fills, a panel retry, a dashed fold with and without bridging, crop and erase, every channel, a forced
invert, a given background and stroke, changed knobs, a border piece, the Hough grain fallback, the
adaptive threshold and a bad channel; and the three noisy benchmark sheets from their masks): 21 drafts
and diagnostics byte-identical with identical masks, the adaptive case refused (`ftx-conformance/tests/
trace_image.rs`). `ftx trace` on the rendered noisy sheets themselves writes `draft.pattern.json` byte for
byte as `vectorize.py trace` does, and the same mask. The reference's own grain fallback raised on OpenCV
5 (it indexed `HoughLinesP`'s old `(N, 1, 4)` layout), dropping every mark of the piece; fixed in the skill
(`vectorize.py`, with a regression test) before this port and the oracle.

**Error model.** `PyError` (`Key` prints quoted, `Value` as is) for `measure_ref`, `resolve_side`, `twin`,
`fit_scale`; `annotate` returns the reference's message as a `String`; `audit` reports every problem as a
finding and never fails.
