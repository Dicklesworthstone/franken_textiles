# ftx-export

**Purpose.** Every deliverable of a pattern as a projection of one model (spec 03). So far: the text and
number helpers (§2), the seam-allowance cut outline (§3–4) and piece geometry (§5). Layouts, the master SVG
and the other targets follow (FTX-15 to FTX-17).

**Units and frames.** Millimetres, the pattern's own frame (x right, y up, loops counter-clockwise).

**Text and numbers** (`text.rs`, §2). `fnum` (fixed-point, ties to even on the exact binary value, trailing
zeros stripped, `-0` → `0`), `ascii_text` (NFKD, `×` → `x`, `— – ·` → `-`, code points 32..126 kept),
`safe_name`, Helvetica `text_width`, greedy `wrap`, the single-stroke font (`stroke_text`), tile column
letters, page lists, PDF string literals and XML escapes. NFKD comes from `ascii_fold.tsv`, what
`ascii_text` keeps of each of the 2,138 non-ASCII code points that keep anything, generated with the
reference's Python 3.12 (Unicode 15.0.0) by `tools/oracle/gen_ascii_fold.py`: no dependency. Against
`tests/fixtures/oracle/export/helpers.json` every value is byte-exact (524 numbers and strings).

**Cut outline** (`outline.rs`, `union.rs`, §3–4). Edges are flattened as the reference does (Béziers at
`clip(ceil(control polygon / 1 mm), 24, 800)` samples, points closer than 1e-7 dropped); SA is
`edge.sa` → `piece.sa` → the default, 0 on the drafted fold edge of a half. The direct offset mitres each
corner (clipped square to the bisector at `mitre_limit × max(SA)`) and is kept when it is a simple polygon at
least as large as the sew ring; otherwise the union of the sew polygon, a single-sided buffer per edge and a
wedge per convex corner, whose largest part's exterior is the outline; then duplicate and collinear points
are dropped. The reference unions with GEOS; here (`union.rs`) every edge is split where it meets another,
the pieces with the union's outside on one side and inside on the other are kept (oriented with the inside
on the left), dangling chains are pruned and rings are traced turning as far left as possible. A polygon's
area is where its winding number has the sign of its signed area: GEOS's positive depth, which keeps the
lobes of a self-intersecting corner wedge that GEOS's `buffer(0)` keeps (the area rule predicted 184 of 184
random bow-ties; the turn at the top vertex, 117) and everything a self-overlapping edge buffer covers.
Edge buffers are built as GEOS builds them: the polyline and its offset, mitred at a joint turning left
(limit 10), joined at the crossing of the offset segments at one turning right, or by a closing segment
through the midpoints towards the vertex where they do not cross (a curve tighter than the SA); per-segment
rectangles were tried first and left sliver tails nanometres thin at nearly straight inside turns.

Against `tests/fixtures/oracle/export/outlines.json` (102 outlines: every piece as drafted and every
instance of TESTBODICE, also at SA 15 with mitre limit 2, ROBUST, the single test pieces and the drafted fez,
bolero and trousers at 80, 92 and 116 cm): sew rings and edge breaks equal (91 bit-exact, 11 within 1e-13 mm:
numpy raises to the third power with the platform `pow`), every method equal (58 mitre, 44 union), mitre
outlines within 1.2e-13 mm vertex for vertex and union outlines within a Hausdorff distance of 4.9e-6 mm
(the spec allows 0.002 and 0.05 mm).

**Piece geometry** (`geom.rs`, `pole.rs`, §5). `build_geom` of a piece as drafted (a piece on the fold shows
its half, fold edge SA 0, the place-on-fold bracket) or of an instance (unfolded, the fold line): facts
(qty, material, fold mode, SA values), cut and sew rings, notches on the cut line pointing inward with
their marks (V, T, castle, check, U, slit), the grainline (given, or by default 70 % of the vertical chord
through the pole of inaccessibility), fold marks and labels, internal lines, drills and decor through the
feature maps (the fold reflection, the mirror), and the main label block. The default grain's pole is
shapely's `polylabel`, GEOS 3.14's `MaximumInscribedCircle`: exact for a convex triangle or quadrilateral
(of the circles tangent to three edge lines, the last largest: the left end of a wide rectangle's plateau, the
top end of a tall one's), else its branch and bound with its interior-point seed and libc++'s heap order.
The main label goes where the largest block fits in the region the reference allows (the sew polygon inset by
2 mm, clear of the grainline by 7, internal lines by 4, drills by d + 4, fold marks by 5, decor by half its
width + 4), found by a branch and bound over anchors (the size that fits drops by at most
`e / (0.53 · min(wmax, hsum))` when the anchor moves by `e`) to 2 % of the text size, seeded with the region's
pole and its centroid. The reference instead takes GEOS's pole of the region's best part and a size bound it
does not check (spec 03 Q4), so its block can overlap the grainline; `main_label_fits` tells.

Against `tests/fixtures/oracle/export/geoms.json` (every piece as drafted and every instance of the outline
fixtures but SA 15, at text sizes 6 and 10; 376 geometries): 38,422 numbers within 1e-9 mm (worst 1.4e-14),
default grains included, every fact, notch, mark, fold, internal line, drill, decor and secondary label equal.
All 188 main labels fit their region with the same lines; 23 are more than 5 % larger than the reference's, 6
more than 5 % smaller where the reference's own block does not fit, the rest within 5 % (the spec asks for
"inside the region, no obstacle overlap, size within 5 %").

**Error model.** `Refusal` `invalid_document` for a missing vertex, a malformed curve or a seam allowance
outline without area.

**No claims.** GEOS's exact vertex lists for union outlines (its ring starts where its overlay starts and
its sliver clean moves points by up to 1e-4 mm); the reference's main label anchors; anything after §5.
