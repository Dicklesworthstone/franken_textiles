# ftx-export

**Purpose.** Every deliverable of a pattern as a projection of one model (spec 03). So far: the text and
number helpers (§2) and the seam-allowance cut outline (§3–4). Piece geometry, layouts, the master SVG and
the other targets follow (FTX-15 to FTX-17).

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

**Error model.** `Refusal` `invalid_document` for a missing vertex, a malformed curve or a seam allowance
outline without area.

**No claims.** GEOS's exact vertex lists for union outlines (its ring starts where its overlay starts and
its sliver clean moves points by up to 1e-4 mm); anything after §4.
