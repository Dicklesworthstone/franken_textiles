# ftx-trace

**Purpose.** Tracing pattern sheets (plan §13, spec 04 §10): a raster sheet (an image model's pattern, a scan,
a screenshot) becomes an UNSCALED `fabric-pattern/1` draft, 1 unit = 1 crop pixel, which is then named,
scaled and audited. So far: everything after the trace, from the reference's `vectorize.py`:
`names.json` (`annotate`), the reference grammar (`measure_ref`, `resolve_side`, `twin`), `fit_scale`, the
round-0 `audit` with its label consensus, and `parse_length`. The raster trace (thresholds, contours, stroke
centres, dashed-edge bridging, corners, marks, the overlay and label images) and the audit's PNG are next.

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
within one level, not bit for bit (on x86 the reference's own results would differ again).

**Error model.** `PyError` (`Key` prints quoted, `Value` as is) for `measure_ref`, `resolve_side`, `twin`,
`fit_scale`; `annotate` returns the reference's message as a `String`; `audit` reports every problem as a
finding and never fails.
