# ftx-export

**Purpose.** Every deliverable of a pattern as a projection of one model (spec 03). So far: the text and
number helpers (§2), the seam-allowance cut outline (§3–4), piece geometry (§5), shelf layouts (§6), the
master SVG (§7.1), the PDFs (§7.2–7.6), the DXFs (§7.7–7.8), HPGL (§7.9), the cutter files (§7.10),
GarmentCode (§7.11), the manifest's piece reference (§7.13) and their acceptance checks (§8). The DST files,
the manifest file and the remaining checks follow (FTX-17).

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

**Layouts and the master SVG** (`layout.rs`, `ctx.rs`, `svg.rs`, §6, §7.1). `Ctx` holds the options and the
names every file uses (`safe_name(style)`, the ASCII size label; a pattern traced in pixels is refused with the
reference's message). The master layout puts per piece its drafted half when cut on the fold (unless
`unfold`), else its primary instance, on shelves (tallest first, ties by instance id; 15 mm gaps, 10 mm margin,
width automatic or `layout_width`). `master_svg` writes the reference's SVG line for line (one element per
line, `fnum` numbers, Python `repr` stroke widths, JSON-null material as `None`) and the manifest's `master`
entry. Against `tests/fixtures/oracle/export/master_svg.json` (every fixture pattern and the NOTE piece, plus
`--unfold` and `--layout-width 300` for four of them; 25 SVGs, 2,479 lines): 2,080 lines byte-identical, every
other line equal but for the cut line of a union outline (within 0.05 mm) and the main label's lines (its own
anchor and size); manifest entries within 0.002 mm.

**PDFs** (`pdf.rs`, §7.2–7.6). The reference's hand-written writer: one content-stream line per operator group
(`fnum` numbers), the two standard fonts, the six layers (`Size …`, notches, seam lines, grainlines & labels,
placement marks, soutache; the projector's calibration grid off), FlateDecode streams (`miniz_oxide`, level 9:
valid, not zlib's bytes), `/PrintScaling /None`, `/FPTile` page dictionaries. The creation date comes from
`created` (the options' or the pattern's `meta.created`), so every file is reproducible; the producer is
FrankenTextiles. Targets: tiled Letter and A4 (information page with the test squares, instructions, 1 in square and
tile map; tiles with watermark, clipped content 5 mm past the frame, join half-diamonds, footer), copy-shop A0 and
36 × 48 in (the orientation with fewer printed sheets, the band with squares and scale bars), projector light and
dark (every piece once, unfolded, grain vertical, text at least 12 mm high, test squares, calibration grid); each
with its manifest entry. A tile is blank when no cut line reaches it (shrunk by 0.01 mm); a piece is drawn on a
tile when its cut line comes within 1 mm of the tile's bleed box.

Against the reference's own files (`tests/fixtures/oracle/export/pdf/`: TESTBODICE, ROBUST also at
`--layout-width 300` with and without `--keep-blank-pages`, TESTBODICE at `--projector-width 1100`, the 92 cm bolero
and trousers; 42 PDFs): the same files and manifest entries; the same objects and dictionaries (lengths, the
date and the producer aside); of 36,867 inflated content lines 34,672 byte-identical, every other number equal to
0.000 mm but for 119 union cut lines (within 0.05 mm) and 2,076 main-label text lines (ftx's own anchor and
size).

**HPGL, cutter files, manifest pieces** (`hpgl.rs`, `cutter.rs`, `ctx.rs`, `pyjson.rs`, §7.9, §7.10, §7.13).
HPGL plots the master layout at 40 units per mm (ties to even), pen 1 (sew, internal draw, decor, grain, fold
lines, drills, label stroke text) before pen 2 (internal cuts, notches, the cut line) per piece, optionally
mirrored and turned. Cutter SVGs: per instance (labels naming the side), grain vertical, the cut polygon united with
a 4 × 3 mm outward triangle per notch, 1 mm margin, with the mat report. JSON is written like Python's
`json.dumps(indent=1)` (key order kept, ASCII, `repr` floats; `PyJson`). Against
`tests/fixtures/oracle/export/targets.json` (TESTBODICE also rotated and mirrored, ROBUST, the 92 cm fez, bolero
and trousers): every HPGL command and pen equal and every polyline identical but the main label's strokes (ftx's
anchor, the same strokes) and union cut lines (within 0.05 mm); cutter titles and reports equal, outlines within
0.05 mm as regions (where a notch triangle's base runs along a curved cut line GEOS, which grows the cut by 1e-4 mm
first, keeps the micrometre sliver between them as a hole); the manifest's `pieces`, `hpgl` and `cutter` entries
equal (cut perimeters within 0.05 mm).

**DXF** (`dxf.rs`, §7.7–7.8). Written here, not from a template. ASTM / AAMA (R12): header (`$ACADVER AC1009`,
`$HANDLING 0`, extents computed, quirk Q1), LTYPE, LAYER (all colour 7), STYLE, APPID tables; one block per piece
(its primary instance unfolded, text size 10) with the cut and sew lines (2 dp, ASTM copies on 84 and 87),
turn and curve points (15°, 10 mm), notches as POINTs (depth, width, angle; ASTM typed layers 80–83) and LINEs,
grainline, internal lines, fold lines, decor, drills and piece text (ASTM notes on layer 15); style text and
INSERTs in modelspace; dates from `created`. Flat (R2000): handles, owners, symbol tables, block records,
`*Model_Space`/`*Paper_Space`, a root dictionary, `$INSUNITS 4`; every instance laid out grain vertical, y up,
as LWPOLYLINEs and CIRCLEs on CUT, DRAW, NOTCH, DRILL, GRAIN and TEXT (single-stroke text). Both open in ezdxf 1.4.4
with a clean audit. Against the reference's files (`targets.json`, clocks and random ids fixed): the same blocks,
and per block and layer the same entities within the 2 dp rounding (union cut lines and their turn points within
0.05 mm; piece texts that hang off the main label compared by string); the same modelspace; flat files entity
for entity (main-label strokes at ftx's anchor). `check_dxf` and `check_flat_dxf` (a small DXF reader) give
exactly the reference checker's 268 items on its files and pass on ftx's.

**GarmentCode** (`gc.rs`, §7.11). Panels of every simulated instance (net sew lines in cm, relative-curvature
Béziers, poly edges as straight sub-edges, placement from `placement3d`) and 1:1 stitches after splitting each
seam's sides at matched arclength points (notch knots, candidates merged within 0.05 mm; Béziers split by de
Casteljau at the parameter of an 801-sample arclength table), `right_wrong` for unreversed seams, warnings for
unresolvable or excluded seams. All five reference files (`targets.json`) come out byte-identical; the manifest
entries are equal; `check_garmentcode` reproduces the reference checker's items.

**Checks** (`checks.rs`, `xml.rs`, §8). `check_svg`: `svg.no_transform`, `svg.mm_units_viewbox`,
`svg.piece_subgroups` (master) or `cutter.single_closed_path` (cutter), with the reference's details, reading
the file back with a small XML reader. They pass on all 25 of ftx's and the reference's master SVGs, and the
reference's own mutation (a `transform` before the title), a piece without its fold group and a wrong viewBox
each fail exactly their check. `check_pdf` reads PDFs back as the reference does (objects, inflated streams,
OCG names, pages; a content interpreter for the CTM, tags, rectangles, tagged cut points and font sizes):
`pdf.print_scaling_none`, `pdf.ocg_unique`, `pdf.mediabox_exact`, `pdf.ocg_identical_across_pages`, the test
squares (page 1, every copy-shop sheet, the projector), `pdf.projector_text_cap_12mm`, `pdf.projector_under_200in`,
and with the master SVG's cut vertices `pdf.restitch_matches_master` and `pdf.tiles_cover_master`. On the
reference's 42 PDFs it gives exactly the reference checker's 294 items, details included; all 294 pass on ftx's;
a wrong paper, master vertices 1 mm off and a master vertex off every tile each fail exactly their check.
`check_hpgl` (same layout as the master, pen-2 bounding box against the master's) and the cutter SVG check give
exactly the reference checker's 113 items on its HPGL and cutter files and pass on ftx's.

**Error model.** `Refusal` `invalid_document` for a missing vertex, a malformed curve, a seam allowance
outline without area or a pattern traced in pixels; `out_of_range` for a projector page over 200 in.

**No claims.** GEOS's exact vertex lists for union outlines (its ring starts where its overlay starts and
its sliver clean moves points by up to 1e-4 mm); the reference's main label anchors; zlib's compressed bytes; the targets after §7.6, the manifest file and the
other checks.
