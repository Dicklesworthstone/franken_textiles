# 03 — Exports: every deliverable as a projection of one pattern model

Status: extracted spec (essence extraction for the Rust port). Implementers should not need to read the
legacy code. The legacy oracle is **the Python reference implementation (fabric-pattern-creation-and-alteration
skill, scripts/export.py)** and its test file `scripts/tests/test_export.py`. Behaviour described here was read
from that code and confirmed by running it on the test fixtures (Python 3.12, numpy 2, shapely 2.2 / GEOS,
ezdxf 1.4.4).

Notation: "mm" everywhere unless stated. **Model frame** = the pattern's own frame (x right, y up, pieces drafted
as the wearer's right side, edge loops counter-clockwise). **Sheet frame** = a layout frame with y pointing
down (SVG, PDF content, HPGL before its flip). `k` = the PDF scale factor of a target (1 for home tiles, 2 for
copyshop). `IN` = 25.4. `PT_PER_MM` = 72 / 25.4. `CAP` = 0.718 (Helvetica cap height / font size).

Contents

1. Inputs, context, options, CLI
2. Text and number helpers
3. Model access, instances, edge flattening
4. Seam-allowance cut outline
5. Piece geometry (`Geom`): notches, grain, fold marks, internal lines, drills, decor, labels
6. Layouts
7. Targets (SVG, PDF writer, tiled PDFs, copyshop, projector, DXF-ASTM/AAMA, flat DXF, HPGL, cutter, GarmentCode, embroidery DST, manifest)
8. Acceptance checks
9. Determinism and timestamps
10. Conformance oracle
11. Exclusions / not to port, and quirks to decide

---

## 1. Inputs, context, options, CLI

### 1.1 Input

One `fabric-pattern/1` pattern document (see the pattern-model chapter of this spec set): `style`, `meta`
(`size_label`, `sa_mm`, `round`, `created`, `scale_status`), `fabric` (`name`), `pieces[]`, `seams[]`, optional
top-level `placement3d`. Piece fields used here: `id`, `name`, `vertices`, `edges[]` (`id`, `from`, `to`, `curve`,
`sa`), `sa`, `cut` (`qty` 1|2, `on_fold` edge id, `material`), `notches[]` (`id`, `edge`, `s`, `type`, `depth`,
`width`), `grain` (`[[x0,y0],[x1,y1]]`, legacy `{p0,p1}` also accepted), `internal[]` (`kind` draw|cut, `pts`,
`closed`, `label`), `drills[]` (`p`, `d`), `decor[]` (`kind`, `pts`, `width_mm`, `closed`), `note`, `sim`,
`placement3d`.

### 1.2 Export context (`Ctx`)

| Field | Rule |
|---|---|
| Refusal | If `meta.scale_status == "UNSCALED"` raise with the exact message `pattern is UNSCALED (1 unit = 1 px of the traced sheet): run `vectorize scale-fit` before exporting; its PDFs and machine files would be printed and cut at the wrong size`. CLI prints `error: <msg>` to stderr and exits 2. |
| Output dir | Created (with parents) if missing. Never cleaned. |
| `style` | `safe_name(pattern.style or "STYLE")` (max 20 chars). Every file stem uses it. |
| `size` | `ascii_text(meta.size_label or "base")`, falling back to `"base"` if empty. Display text. File names use `safe_name(size)`. |
| `sa` (default SA) | `--sa` override, else `meta.sa_mm`, else 10.0. |
| `unfold` | `--unfold`: master, tiles, copyshop and HPGL show on-fold pieces unfolded. |
| `mitre` | `--mitre-limit`, default 4.0 (multiples of SA). |
| `projector_width` | `--projector-width`, default 1500.0 mm. |
| `layout_width` | `--layout-width` (master shelf max width), default none (auto). |
| `drop_blank` | `not --keep-blank-pages` (blank tiles omitted by default). |
| `hpgl_rotate90`, `hpgl_mirror` | flags. |
| `created` | explicit argument, else `meta.created`, else local now formatted `%Y-%m-%dT%H:%M`. Used **only** in the manifest. |
| Manifest | If `<style>_export_manifest.json` already exists in the output dir it is loaded and updated (old `files` entries persist). Then set `schema = "fabric-pattern-export/1"`, `style`, `size`, `created`, `source_schema = pattern.schema`; ensure `files = {}` exists; set `pieces = piece_reference()` (§7.13). |

### 1.3 Targets and CLI

`export <target> PATTERN.json --out DIR [options]`; targets in run order:

| Target | Writes | Sub-targets |
|---|---|---|
| `svg` | `<S>_master_mm.svg` | |
| `pdf` | `<S>_LETTER.pdf`, `<S>_A4.pdf` | `letter`, `a4` (one paper) |
| `copyshop` | `<S>_COPYSHOP_A0.pdf`, `<S>_COPYSHOP_36x48in.pdf` | |
| `projector` | `<S>_PROJECTOR.pdf`, `<S>_PROJECTOR_DARK.pdf` | |
| `dxf` | `<S>_ASTM_METRIC.dxf`, `<S>_AAMA_METRIC.dxf` | `astm`, `aama` |
| `flat-dxf` | `<S>_flat_mm.dxf` | |
| `hpgl` | `<S>_plot.plt` | |
| `cutter` | `<S>_cutter/<inst>_<size>.svg` …, `<S>_cutter/cutter_report.json` | |
| `garmentcode` | `<S>.garmentcode.json` | |
| `embroidery` | `<S>_<inst>_<size>_soutache.dst` per braided instance + `<S>_soutache_README.txt` | |
| `all` | every target above in this order, manifest, then the acceptance checks | |
| `check DIR` | acceptance checks only (`--pattern-ref P`, `--json`) | |

`<S>` = `Ctx.style`; `<inst>` = `safe_name(instance_id with "." → "_", 40)`; `<size>` = `safe_name(Ctx.size)`.

After **each** target the manifest is saved. CLI behaviour:

- Non-`check`: `--out` is required (usage error, exit 2). The pattern is loaded; each structure error is printed as
  `warning: <error>` to stderr and is **not** fatal. Each written path is printed on its own line.
- `all` (and `check`): print `FAIL <check> [<file>] <detail>` per failed check, then a summary line
  `PASS: <n_ok>/<n_total> checks` or `FAIL: <n_ok>/<n_total> checks`; exit 0 if all pass, else 1. `--json` (check
  only) first prints the full item list as JSON (indent 1).
- The round driver parses the last `PASS:`/`FAIL` line and the `FAIL ` lines; keep this text contract.
- Errors raised inside a target (e.g. the projector 200 in limit) propagate (traceback, non-zero exit).

Library entry points (behavioural contract): `cut_outline(piece_or_instance, sa_default, mitre_limit)`,
`build_geom(pattern, src, sa_default, mitre_limit, text_size)`, `export_all(pattern|path, out, **options)` (runs every
target, saves the manifest, returns written files + manifest; **does not** run the checks), `check_dir(out,
pattern_ref=None)`.

---

## 2. Text and number helpers

### 2.1 `fnum(v, nd=3)`

Format `float(v)` fixed-point with `nd` decimals (Python `f"{v:.{nd}f}"`: exact binary value, ties to even), strip
trailing zeros and then a trailing `.`; `"-0"`, `""`, `"-"` become `"0"`. Never exponent notation.
Goldens: `fnum(12.0)="12"`, `fnum(-0.0004)="0"`, `fnum(1e-9)="0"`, `fnum(0.1+0.2)="0.3"`, `fnum(0.125,2)="0.12"`,
`fnum(2.5,0)="2"`. The Rust port must reproduce ties-to-even on the exact binary value; add these as unit tests.

### 2.2 `ascii_text(s)`

1. Unicode NFKD normalize.
2. Replace `×`→`x`, `—`→`-`, `–`→`-`, `·`→`-`.
3. Keep only code points 32..126.

Golden: `"Café × — – · ñ ø ß ﬁ ½"` → `"Cafe x - - - n   fi 12"` (ø and ß vanish; ½ → `1`,`2`).

### 2.3 `safe_name(s, n=20)`

`ascii_text(s)`, replace every run of characters outside `[A-Za-z0-9_-]` by one `_`, strip leading/trailing `_`,
use `"X"` if empty, then truncate to `n` (truncation happens after stripping, so a result may end in `_`).
Goldens: `"Front bodice (left) ×2"`→`Front_bodice_left_x2`, `"___"`→`X`, `"920 mm stature"`→`920_mm_stature`.

### 2.4 Text metrics

`text_width(s, size, bold=False)` = Σ advance widths (1/1000 em) × size / 1000, × 1.06 if bold (an approximation
of Helvetica-Bold). Characters outside 32..126 count 556. Widths are the standard Helvetica AFM widths for
WinAnsi 32..126, in code-point order:

```
32-47 : 278 278 355 556 556 889 667 191 333 333 389 584 278 333 278 278
48-64 : 556 556 556 556 556 556 556 556 556 556 278 278 584 584 584 556 1015
65-81 : 667 667 722 722 667 611 778 722 278 500 667 556 833 722 778 667 778
82-96 : 722 667 611 722 667 944 667 667 611 278 278 278 469 556 333
97-113: 556 556 500 556 556 278 556 556 222 222 500 222 833 556 556 556 556
114-126: 333 500 278 556 500 722 500 500 500 334 260 334 584
```

Golden: `text_width("PLACE ON FOLD", 4, bold) = 33.92848`, `text_width("Front bodice", 6, bold) = 35.34888`.

`wrap(s, size, width)`: greedy word wrap on whitespace; a word is moved to a new line when the candidate line
(`line + " " + word`) is wider than `width` and the current line is non-empty.

### 2.5 Single-stroke font (HPGL and flat-DXF text)

Grid 4 wide × 6 high, y up, baseline y = 0. `stroke_text(s, x, y, cap, anchor, y_down)`:
`u = cap/6`, advance = `6u` per character (glyph 4u + 2u spacing), text = `ascii_text(s).upper()`,
`total = len·6u − 2u`, `x0 = x − total/2` (middle) | `x − total` (end) | `x` (start); spaces are skipped (but
advance); unknown characters use the `?` glyph; each stroke point `(gx, gy)` maps to
`(x0 + i·6u + gx·u, y + sy·gy·u)` with `sy = −1` if `y_down` else `+1`. One polyline per stroke. Glyphs (strokes
separated by `;`, points `x,y`):

```
A: 0,0 0,4 2,6 4,4 4,0 ; 0,3 4,3          B: 0,0 0,6 3,6 4,5 4,4 3,3 0,3 ; 3,3 4,2 4,1 3,0 0,0
C: 4,5 3,6 1,6 0,5 0,1 1,0 3,0 4,1        D: 0,0 0,6 2,6 4,4 4,2 2,0 0,0
E: 4,6 0,6 0,0 4,0 ; 0,3 3,3              F: 4,6 0,6 0,0 ; 0,3 3,3
G: 4,5 3,6 1,6 0,5 0,1 1,0 3,0 4,1 4,3 2,3
H: 0,0 0,6 ; 4,0 4,6 ; 0,3 4,3            I: 1,6 3,6 ; 2,6 2,0 ; 1,0 3,0
J: 4,6 4,1 3,0 1,0 0,1                    K: 0,0 0,6 ; 4,6 0,2 ; 1,3 4,0
L: 0,6 0,0 4,0                            M: 0,0 0,6 2,3 4,6 4,0
N: 0,0 0,6 4,0 4,6                        O: 1,0 0,1 0,5 1,6 3,6 4,5 4,1 3,0 1,0
P: 0,0 0,6 3,6 4,5 4,4 3,3 0,3            Q: 1,0 0,1 0,5 1,6 3,6 4,5 4,1 3,0 1,0 ; 2,2 4,0
R: 0,0 0,6 3,6 4,5 4,4 3,3 0,3 ; 2,3 4,0  S: 4,5 3,6 1,6 0,5 0,4 1,3 3,3 4,2 4,1 3,0 1,0 0,1
T: 0,6 4,6 ; 2,6 2,0                      U: 0,6 0,1 1,0 3,0 4,1 4,6
V: 0,6 2,0 4,6                            W: 0,6 1,0 2,3 3,0 4,6
X: 0,0 4,6 ; 0,6 4,0                      Y: 0,6 2,3 4,6 ; 2,3 2,0
Z: 0,6 4,6 0,0 4,0
0: 1,0 0,1 0,5 1,6 3,6 4,5 4,1 3,0 1,0 ; 0,1 4,5      1: 1,5 2,6 2,0 ; 1,0 3,0
2: 0,5 1,6 3,6 4,5 4,4 0,0 4,0            3: 0,5 1,6 3,6 4,5 4,4 3,3 1,3 ; 3,3 4,2 4,1 3,0 1,0 0,1
4: 3,0 3,6 0,2 4,2                        5: 4,6 0,6 0,3 3,3 4,2 4,1 3,0 0,0
6: 3,6 1,6 0,5 0,1 1,0 3,0 4,1 4,2 3,3 0,3   7: 0,6 4,6 1,0
8: 1,3 0,4 0,5 1,6 3,6 4,5 4,4 3,3 1,3 0,2 0,1 1,0 3,0 4,1 4,2 3,3
9: 4,3 1,3 0,4 0,5 1,6 3,6 4,5 4,1 3,0 1,0
-: 1,3 3,3            .: 2,0 2,0.5          ,: 2,0.5 1.5,-1        :: 2,1 2,1.5 ; 2,4 2,4.5
/: 0,0 4,6            (: 3,6 2,4.5 2,1.5 3,0                       ): 1,6 2,4.5 2,1.5 1,0
+: 2,1 2,5 ; 0,3 4,3  =: 0,2 4,2 ; 0,4 4,4  _: 0,-0.5 4,-0.5       ': 2,6 2,5
#: 1,0 1,6 ; 3,0 3,6 ; 0,2 4,2 ; 0,4 4,4
%: 0,0 4,6 ; 0,6 1,6 1,5 0,5 0,6 ; 3,1 4,1 4,0 3,0 3,1
?: 0,5 1,6 3,6 4,5 4,4 2,3 2,2 ; 2,0 2,0.5      !: 2,6 2,2 ; 2,0 2,0.5
<: 4,6 0,3 4,0        >: 0,6 4,3 0,0        *: 2,1 2,5 ; 0.5,2 3.5,4 ; 0.5,4 3.5,2
|: 2,-1 2,7           &: 2,1 2,5 ; 0,3 4,3   (same strokes as "+")
```

Stroke text is always called with `cap = font_size × CAP`, so its height equals the Helvetica cap height.

### 2.6 Other helpers

- Column letters `col(c)`: bijective base 26, 0→`A`, 25→`Z`, 26→`AA`, 701→`ZZ`, 702→`AAA`. Tile name = `col(c) + (r+1)`.
- Page list `page_list([2,3,4,6,8,9])` = `"2-4, 6, 8-9"` (consecutive runs as `a-b`, joined by `", "`).
- PDF string literal: `(` + `ascii_text(s)` with `\`→`\\`, `(`→`\(`, `)`→`\)` + `)`.
- XML escape (SVG): `&`→`&amp;`, `<`→`&lt;`, `>`→`&gt;`, `"`→`&quot;`.
- Rounding to integers (HPGL units, DST 0.1 mm units) uses Python `round()`: **round half to even**.
- JSON files (manifest, cutter report, GarmentCode): Python `json.dumps(obj, indent=1)` + `"\n"`: one-space
  indent, `", "`/`": "` separators, ASCII-only output (non-ASCII escaped `\uXXXX`), floats in shortest round-trip
  `repr` (Python writes `1e-05`, `1e+16`; integers stay integers, floats keep `.0`).

---

## 3. Model access, instances, edge flattening

### 3.1 Instances (from the pattern model; restated for exports)

- `cut.qty == 2` → instances `<id>.R` (as drafted, side `R`) and `<id>.L` (side `L`, `mirrored`, every vertex
  `x → −x`, loop reversed, curve params mirrored so the loop stays CCW; edge ids unchanged).
- `cut.on_fold = E` → one unfolded instance `<id>` (side `C`): the edges after `E` in loop order (wrapping) get
  suffix `.R`; their reflection across the fold line (loop reversed) is appended with suffix `.L`; vertices on the
  fold line are shared, the others get a `.L` twin. The fold edge itself disappears.
- Both → `<id>.R` and `<id>.L`, each unfolded (the L one mirrored).
- Otherwise one instance `<id>` (side `C`).
- Instance order = pieces order, then `[C]` or `[R, L]`.
- **Drafted instance** of a piece dict: id = piece id, side `C`, not mirrored, the drafted vertices and edges
  (used for "as drafted / half on fold" geometry).
- **Primary instance** of a piece: first instance with side `C`, else first with side `R`, else the first.
- `stem(edge_id)` drops a trailing `.R`/`.L`.

### 3.2 Edge SA precedence

`edge.sa` → `piece.sa` → default SA (`Ctx.sa`). When geometry is built from a **piece dict** whose `cut.on_fold`
names an edge, that edge's SA is forced to 0. When built from an **instance**, no edge is forced (the fold edge
no longer exists in an unfolded instance; instance edges keep the `sa` key of their source edge).

### 3.3 Edge flattening (used by everything except GarmentCode)

- `line`: the two endpoints.
- `poly`: endpoints plus every relative point `p0 + t·d + h·perp(d)` (`d = p1 − p0`, `perp(d) = (−d.y, d.x)`).
- `quad`/`cubic`: Bézier with absolute controls `[p0, rel_to_abs(c_i)…, p1]`, evaluated by Bernstein polynomials
  at `n` **uniform parameter values** `t = linspace(0, 1, n)`, with
  `n = clip(ceil(control_polygon_length_mm / 1.0), 24, 800)`.
- Consecutive points closer than 1e−7 are dropped (if all collapse, keep first and last).
- **Sew ring** = concatenation over the loop of each edge's points without its last point (open CCW ring).
- **Sew breaks** = index in the sew ring where each edge starts.

The pattern model's own lengths (`Instance.perimeter`, `outline(dense)`) sample curves with a fixed count
(200 by default); the manifest references use those (§7.13).

---

## 4. Seam-allowance cut outline (`cut_outline`)

Input: a piece dict (as drafted, fold edge SA 0) or an instance; default SA; mitre limit (default 4).
Output: open CCW ring (no repeated first point), plus the method used (`mitre` or `union`).

Per-edge loop entries: `(edge, flattened points, sa)` (§3.2, §3.3). For a polyline, per segment: unit tangent
`t`, outward normal `n = (t.y, −t.x)` (right of travel = outside for a CCW loop).

### 4.1 Corner rule `corner(v, t_in, n_in, sa_i, t_out, n_out, sa_j, limit)`

At vertex `v` between the last segment of edge i and the first segment of edge j:

1. `cr = t_in × t_out` (2D cross). If `cr ≤ 1e−9` (straight-through, tangent or reflex) → no corner points.
2. `a1 = v + sa_i·n_in`, `a2 = v + sa_j·n_out`. Mitre point `mp` = intersection of line `a1 + s·t_in` with line
   `a2 + u·t_out`.
3. `big = max(sa_i, sa_j)`; if `big ≤ 0` → none. `lim = limit·big`.
4. If `|mp − v| ≤ lim` → `[mp]`.
5. Else clip square to the bisector: `b = unit(mp − v)`; for `(a, t)` in `((a1, t_in), (a2, t_out))`:
   `k = (lim − (a − v)·b) / (t·b)` (0 if `|t·b| ≤ 1e−12`), point `a + k·t`. Returns the two points (in that order).

### 4.2 Direct offset (method `mitre`)

For each edge (in loop order) with points `P` and SA `s`:
- first point `P0 + s·n_first`, last point `Pend + s·n_last`;
- interior points: `P_i + s·(n_a + n_b)/(1 + n_a·n_b)` with `n_a`, `n_b` the normals of the segments before and
  after `P_i`. If any `1 + n_a·n_b < 0.2` (a kink of more than ~145° inside one edge) → abort the direct method;
- append the edge's offset points, then the corner points (§4.1) between this edge and the next (cyclic).

Accept the direct ring only if it has ≥ 3 points, is a **valid** simple polygon, and its signed area ≥ sew-ring
signed area − 1e−6. Otherwise use the union method.

Note: at a reflex or collinear corner no corner point is added, so the ring jumps from one edge's offset end to the
next edge's offset start (a step when SAs differ; a self-intersection at reflex corners, which triggers the
fallback).

### 4.3 Union fallback (method `union`)

Union of:
- the sew polygon (made valid);
- for each edge with `sa > 0`: a single-sided buffer of its polyline on the **right** side at distance `sa`
  (shapely `buffer(−sa, single_sided=True, join_style=mitre, mitre_limit=10)`);
- for each convex corner with corner points `c`: wedge polygon `[v, v + sa_i·n_in, *c, v + sa_j·n_out]` (made valid,
  skipped if empty).

Then a sliver clean: buffer `+1e−4` then `−1e−4`, both with mitre joins (mitre limit 10). If the result has several
parts keep the one of largest area. Orient CCW; take the exterior ring without its closing point.

### 4.4 Final clean

1. Drop consecutive duplicates (distance ≤ 1e−4, cyclic).
2. Repeatedly drop collinear vertices while more than 3 remain: vertex b (neighbours a, c) is dropped when
   `|ab × bc| ≤ 1e−9·max(1, |ab|·|bc|)` **and** `ab·bc > 0` (straight-through, not a spike).
3. If the signed area is negative, reverse.

### 4.5 Required behaviour (from the tests)

| Case | Result |
|---|---|
| 100×50 rectangle, SA 10 | bbox 120×70 (abs 1e−6), method `mitre`, area 8400 (abs 1e−3) |
| same, `on_fold = left` edge (piece dict) | bbox 110×70, min x exactly 0 (abs 1e−9) |
| unfolded instance of that fold piece (200×50) | 220×70 |
| bottom edge `sa: 30`, default 10 | bbox (−10, −30, 110, 60) |
| triangle (0,0),(200,0),(200, 200·tan 20°), SA 10, limit 4 | the 20° spike at (0,0) would mitre to 10/sin 10° = 57.6; reach along the outward bisector is exactly 40.0 (abs 1e−6); min x < −30 |
| 100×80 box with a cubic scoop (60,80)→(40,80), controls (60,60),(40,60), SA 15 | method `union`, valid polygon, bbox 130×110 (abs 1e−3) |
| robustness yoke (reflex corner + poly edge) | valid polygon, either method |

Not implemented (do not add silently): the patternmaking "mirror the SA corner across the seam" jog.

---

## 5. Piece geometry (`Geom`)

### 5.1 Fields

| Field | Content |
|---|---|
| `pid`, `iid`, `name` | piece id, instance id (piece id for a drafted half), `piece.name` or id |
| `meta` | `qty`, `mirror` (= qty == 2), `material` (`cut.material`, else `fabric.name`, else `""`), `on_fold` (edge id or `""`), `fold_mode`, `side`, `sa_mm` (= default SA), `sa_values` (sorted unique per-edge SAs > 0, rounded to 0.1), `sim` (`piece.sim`, default true) |
| `cut` | cut ring (§4) |
| `sew` | sew ring (§3.3) |
| `sew_breaks` | edge start indices |
| `notches` | list of Notch (§5.3) |
| `grain` | `(a, b)` or none; `grain_default` flag |
| `fold_lines` | `(points, style)`, style `bracket` or `dashdot` |
| `internal_draw`, `internal_cut` | `(points, closed)` |
| `drills` | `(point, diameter)` |
| `labels` | Label: `p` (anchor), `lines` = `[(text, rel_size, bold)]`, `size` (base mm), `kind` (`label`/`fold`/`internal`) |
| `decor` | `{kind, pts, width, closed}` |

`fold_mode`: `half` when built from a piece dict with `on_fold`; `unfolded` when built from an instance whose piece
has `on_fold`; else `none`.

`bbox()` = bounding box of `cut` only.

**Transform** `Geom.transform(A, b)` applies `P ↦ A·P + b` to every point (cut, sew, notch base/marks/sew point,
grain, fold lines, internal lines, drills, label anchors, decor). Direction vectors (notch `inward`, `tangent`) map
to `unit(A·d)`. Label lines, sizes and kinds are unchanged.

### 5.2 Feature maps (internal lines, drills, decor, grain, fold marks)

Affine maps from drafted piece coordinates to the geometry's frame:
- drafted piece dict: `[identity]`;
- instance: `[identity]`, plus (piece on fold) the reflection across the fold edge line
  (`A = 2·d·dᵀ − I`, `b = pa − A·pa`, `d = unit(pb − pa)`, `pa`/`pb` the fold edge's drafted endpoints);
  if the instance is mirrored, every map is pre-composed with `diag(−1, 1)`.
- Grain and fold marks use only the **first** map; internal lines, drills and decor use **every** map (both halves of
  an unfolded piece). Drills are de-duplicated (points within 1e−6 kept once); internal lines and decor lying on the
  fold line are **not** de-duplicated.

### 5.3 Notches

For each loop edge `(e, P, sa)` and each piece notch whose `edge == stem(e.id)`:

- `s' = 1 − s` when built from an instance and `(e.id ends with ".L") != instance.mirrored`; else `s' = s`.
- Point at arclength fraction `s'` along `P` (clipped to [0, 1]); its tangent `t` = direction of the containing
  segment (segment index = `searchsorted_left(cum, target) − 1` clipped to `[0, len−2]`).
- `n_out = (t.y, −t.x)`; **base** = `p + sa·n_out` (on the offset line of that edge, i.e. the cut line away from
  corners; on a half-view fold edge `sa = 0`, so the base sits on the fold line); **inward** = `−n_out`.
- `depth = notch.depth` else `clamp(0.5·sa, 3, 6)`; `width = notch.width` else 3; `type = notch.type` else `V`.
- `sew_point = p`.

Notches on the fold edge appear in the half view only (the unfolded instance has no fold edge).

Mark polylines (`apex = base + depth·inward`, `h = 0.5·width`):

| Type | Polylines |
|---|---|
| `V` | `[base + h·t, apex, base − h·t]` |
| `T` | `[base, apex]` and `[apex − 0.4·width·t, apex + 0.4·width·t]` |
| `castle` | `[base − h·t, apex − h·t, apex + h·t, base + h·t]` |
| `check` | `[base, apex, apex + 0.5·width·t − 0.3·depth·inward]` |
| `U` | `[base + h·t, arc…, base − h·t]`, arc = 9 points `apex − h·inward + h·(cos a·t + sin a·inward)`, `a = linspace(0, π, 9)` |
| `slit` and any other | `[base, apex]` |

### 5.4 Grainline

- `piece.grain` present: its two points mapped by the first feature map.
- Absent: `c` = pole of inaccessibility of the sew polygon (tolerance 1.0); intersect the vertical line
  `x = c.x` (from `miny − 1` to `maxy + 1`) with the sew polygon; take the longest part `qa → qb`; grain =
  `(mid − 0.35·(qb − qa), mid + 0.35·(qb − qa))` (the middle 70 %); set `grain_default = true`.
- Exactly one grain per piece.

`grain_marks(g, arrow=6)`: `t = unit(b − a)`, `n = (−t.y, t.x)`, `w = 0.4·arrow`:
`[a, b]`, `[b − arrow·t + w·n, b, b − arrow·t − w·n]`, `[a + arrow·t + w·n, a, a + arrow·t − w·n]`.

`grain_vertical_rotation(g)` (degrees, CCW): `rot = 90 − atan2(d)`; wrap to [−180, 180); if `rot > 90` subtract 180,
if `rot < −90` add 180. The grain ends up vertical (up or down) with |rotation| ≤ 90°. No grain → 0.

### 5.5 Fold marks

Fold edge endpoints `pa → pb` (drafted, first feature map), `d = pb − pa`, `tu = unit(d)`.

- `half` mode: `nin = unit(−d.y, d.x)` (left of travel = inside).
  - Bracket `[q1, q1 + 12·nin, q2 + 12·nin, q2]` with `q1 = pa + 0.2·d`, `q2 = pa + 0.8·d` (style `bracket`).
  - Two arrowheads, for `q` in `(q1, q2)`: `[q + 3·nin + 1.5·tu, q, q + 3·nin − 1.5·tu]` (style `bracket`).
  - Label `PLACE ON FOLD`, bold, base size 4.0, kind `fold`, at
    `pa + 0.9·d + shift·nin`, `shift = 2 + |nin.x|·text_width("PLACE ON FOLD", 4, bold)/2 + |nin.y|·3`.
- `unfolded` mode: line `[pa, pb]` (style `dashdot`); label `FOLD LINE (<edge id>)`, size 3.5, regular, kind `fold`,
  at `pa + 0.85·d + shift·nrm`, `nrm = unit(−d.y, d.x)`, `shift = 2 + |nrm.x|·text_width(txt, 3.5)/2 + |nrm.y|·3`.

### 5.6 Internal lines, drills, decor

- Internal item: `pts`; `closed = item.closed` if given, else (more than 2 points and first ≈ last). `kind == "cut"`
  → `internal_cut`, anything else → `internal_draw`. With `label`: one Label per map, size 3.0, kind `internal`, at the
  mean of the points (closed) or at `(max x + 2 + text_width(label, 3)/2, mean y)` (open).
- Drill: point `p`, diameter `d` (default 3.0).
- Decor: `kind` (default `soutache`), `pts`, `width = width_mm` (default 3.0), `closed` (default false).

### 5.7 Main label block

Lines (`text, rel, bold`), in order:

1. `(name, 1.0, bold)`
2. `(f"{pattern.style or 'STYLE'}  size {meta.size_label or ''}".strip(), 0.65)` — two spaces before `size`; raw style (not safe_name'd)
3. cut text, 0.65:

   | fold_mode | qty 1 | qty 2 |
   |---|---|---|
   | `half` | `Cut 1 on fold` | `Cut 2 on fold (2 folded pieces)` |
   | `unfolded` | `Cut 1 (shown unfolded)` | `Cut 2 mirror pair (shown unfolded)` |
   | `none` | `Cut 1` | `Cut 2 (mirror pair)` |

   Per-instance targets (flat DXF, cutter, DST) replace this line for sides R/L with `Cut 1 (R)` /
   `Cut 1 (L, mirrored)` (the `, mirrored` only when the instance is mirrored).
4. `Fabric: <material>` if material non-empty
5. `SA included: <a/b/c> mm` (each `fnum(v, 1)`, joined `/`) if any SA > 0
6. `Round <n>` if `meta.round` is set
7. `piece.note` if present (one short making-up instruction)

Placement (pole of inaccessibility with obstacles, then the strip rule):

1. Obstacles: grain line buffered 7; each internal line buffered 4 (as a filled polygon if closed with > 3 points,
   else as a line); each drill as a disc of radius `d + 4`; each fold line buffered 5; each decor line buffered
   `width/2 + 4`.
2. Region = sew polygon inset by 2 (buffer −2) minus the union of obstacles. Parts = polygons of the region with
   area ≥ 10 mm². None → `[sew polygon]`.
3. For each part: `c` = pole of inaccessibility (tolerance 1.0); `dist` = distance from `c` to the part's
   **exterior** ring. Best = largest `dist`.
4. `wmax = max text_width(text, rel, bold)` (width at base size 1), `hsum = Σ 1.3·rel`.
5. `fit(c, dist)`: `rr = max(dist, 2)`; `lo = min(text_size, 1.9·rr/wmax, 1.9·rr/hsum)`, `hi = text_size`;
   12 bisection steps on `mid`: if the box centred on `c` with half sizes `0.53·wmax·mid` × `0.53·hsum·mid` lies
   inside the best part, `lo = mid`, else `hi = mid`; return `lo`.
6. Anchor = pole, size = `fit(pole, dist)`.
7. **Strip rule** (long thin pieces such as casings and cuffs): if the best part contains its centroid, compute
   `fit(centroid, distance(centroid, exterior))`; if larger than the current size by more than 1e−6, use the
   centroid and that size.
8. Insert the Label at index 0 with size `max(size, 2.0)`, kind `label`.

`text_size` (maximum base size) per use: 6 (master, tiles, copyshop, cutter, DST), 10 (DXF-ASTM/AAMA), 8 (flat DXF),
17 (projector).

`label_texts(label, y_down, min_size=0, scale=1)` → per line `(x, y_baseline, ascii_text(text), size, bold)`:
`size_i = max(label.size·scale·rel_i, min_size)`, `total = Σ 1.3·size_i`, and
`y_i = p.y + σ·(−total/2 + acc_i + 0.15·size_i + CAP·size_i)` with `σ = +1` (y down) or `−1` (y up),
`acc_i` = Σ of `1.3·size` of previous lines; `x = p.x` (text is centre-anchored by its consumers).

---

## 6. Layouts

### 6.1 Shelf layout `shelf_layout(geoms, gap, margin, max_width, rotate_grain, y_down, top=0)`

1. Optionally rotate each geom about the origin by `grain_vertical_rotation`; record bbox width `w`, height `h`.
2. If `max_width` is unset: `max(max w, 1.25·sqrt(Σ w·h))`.
3. Placement order: sort by `(−h, iid)`.
4. `x = y = shelf_h = 0`. For each: if `x > 0` and `x + w > max_width` → new shelf: `y += shelf_h + gap`,
   `x = shelf_h = 0`. Target corner `sx = margin + x`, `sy = top + margin + y`.
   - y down: `A = diag(1, −1)`, `b = (sx − x0, sy + y1)` (the bbox top lands on `sy`, model y flips).
   - y up: `A = I`, `b = (sx − x0, sy − y0)`.
   Then `x += w + gap`, `used_w = max(used_w, x − gap)`, `shelf_h = max(shelf_h, h)`.
5. Sheet `W = used_w + 2·margin`, `H = top + y + shelf_h + 2·margin`.
6. Placed items keep the **input order** (not the placement order).

### 6.2 Layout per target

| Target | Geometry set | gap | margin | max width | rotate grain | frame |
|---|---|---|---|---|---|---|
| Master SVG, tiled PDFs, copyshop, HPGL ("master layout") | per piece: drafted half if on fold and not `--unfold`, else primary instance; text_size 6 | 15 | 10 | `--layout-width` or auto | no | y down |
| Projector | primary instance per piece (on-fold pieces always unfolded), text_size 17 | 50 | 0 | `projector_width − 100` | yes | y down, then translated by (50, 150) |
| Flat DXF | every instance, text_size 8 | 20 | 10 | auto | yes | y up |
| DXF-ASTM/AAMA | primary instance per piece, text_size 10 | row: next block at `x += width + 30` | — | — | no | model frame inside blocks |
| Cutter SVGs, DST | every instance, each in its own frame | — | — | — | cutter yes, DST no | — |

Which shape each output shows: master/tiles/copyshop/HPGL show on-fold pieces as the drafted half with the
`PLACE ON FOLD` bracket (or unfolded with `--unfold`), qty-2 pieces once labelled `Cut 2 (mirror pair)`.
Projector and DXF-ASTM/AAMA show each piece once, always unfolded. Flat DXF, cutter and GarmentCode have one entry
per physical instance.

---

## 7. Targets

### 7.1 Master SVG — `<S>_master_mm.svg`

Lines joined by `\n`, file ends with `\n`. Numbers via `fnum` (3 dp) unless stated.

```
<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:fp="urn:fabric-pattern:export:1" width="{W}mm" height="{H}mm" viewBox="0 0 {W} {H}" fp:schema="fabric-pattern/1" data-style="{style}" data-size="{size}" data-units="mm" data-fold-mode="{half|unfolded}">
<title>{style} master pattern, size {size} (1 unit = 1 mm)</title>
<rect x="0" y="0" width="{W}" height="{H}" fill="#fff"/>
…one piece group per layout item…
</svg>
```

No `transform` attribute anywhere. Piece group (`pid = safe_name(iid with "." → "_", 64)`), attributes in this
order: `id="piece-{pid}" data-piece data-instance data-name data-qty data-mirror(true|false) data-material
data-on-fold data-fold-mode data-size data-sa-mm(fnum) data-grain-default(true|false)`.

Sub-groups, each `<g id="piece-{pid}-{name}" class="{name}">`, one element per line, always present except
`decor-embroidery`:

| Sub-group | Elements (stroke `#000`, `fill="none"` unless noted; `line` = 0.25) |
|---|---|
| `cut` | closed path, stroke-width `0.25` |
| `sew` | closed path, stroke-width `0.175`, `stroke-dasharray="3 1.5"` |
| `notches` | one open path per mark, `data-notch="{id}" data-type="{type}"` first, width 0.25 |
| `grain` | the 3 grain-mark paths, width 0.25 |
| `fold` | fold paths, width 0.25; `dashdot` adds `stroke-dasharray="6 1.5 1 1.5"` |
| `internal-draw` | path (Z if closed), width `0.2`, `stroke-dasharray="2 1"` |
| `internal-cut` | path, width 0.25 |
| `drills` | per drill one line holding `<circle cx cy r=d/2 …width 0.25/>` immediately followed by `<path d="M x−d,y L x+d,y M x,y−d L x,y+d" stroke="#000" stroke-width="0.15"/>` |
| `labels` | `<text x y font-size={fnum(size,2)} font-family="Helvetica, Arial, sans-serif" text-anchor="middle" [font-weight="bold"] paint-order="stroke" stroke="#fff" stroke-width={fnum(0.12·size,2)} fill="#000" data-kind="{kind}">{text}</text>` per line of every label (main label first) |
| `decor-soutache` | per soutache decor: a wide path `stroke-opacity="0.35" stroke-width={width} stroke-linecap="round" stroke-linejoin="round" data-width-mm={width}` then a centre path width `0.15` |
| `decor-embroidery` | same for `embroidery` decor; group emitted only when such decor exists |

Path data: `M x,y L x,y …` with `" L"` separators and a trailing `" Z"` when closed. Decor of any other kind is not
drawn in the SVG (it is drawn in PDFs, DXFs and HPGL).

Manifest: `master = {file, width_mm: round(W,3), height_mm: round(H,3), cut_bbox: [minx, miny, maxx, maxy] of all
cut rings (sheet frame, round 3), fold_mode}`; `files.svg_master = {file}`.

### 7.2 The hand-written PDF writer

**Canvas** (content-stream builder; coordinates in mm, origin top-left, y down). Each call appends one line:

| Op | Emitted text |
|---|---|
| line width | `{fnum(w,4)} w` |
| dash | `[{fnum(a,3)} …] {fnum(phase)} d`; solid = `[] 0 d` |
| gray stroke / fill | `{g} G` / `{g} g` |
| RGB stroke / fill | `{r} {g} {b} RG` / `rg` |
| polyline | `x y m x y l … [h]` on one line, then a separate `S` line |
| rect | `x y w h re {op}` (op `S`, `f`) |
| clip rect | `x y w h re W n` |
| circle | 4 cubic Béziers with `k = 0.5523·r`, starting at `(cx + r, cy)`, then op |
| path then fill+stroke | path line, then `B` |
| text | see below |
| optional content | `/OC /oc{index} BDC` … `EMC` (index = position of the OCG name in the document's OCG list) |
| tag | `/{Name} BMC` … `EMC` (tags: `FPCut`, `FPTest4in`, `FPTest10cm`, `FPTest1in`) |
| save/restore/cm | `q`, `Q`, `a b c d e f cm` |

Text: `s = ascii_text(s)`; `x0 = x − w/2` (middle) or `x − w` (end) with `w = text_width(s, size, bold)`; font
`/F2` if bold else `/F1`; escaped string as §2.6. With a halo (gray `g`):
`q {g} G {fnum(0.12·size,3)} w 1 j BT {font} {fnum(size,3)} Tf 1 Tr 1 0 0 -1 {x0} {y} Tm ({s}) Tj ET Q`, then always
`BT {font} {size} Tf 0 Tr 1 0 0 -1 {x0} {y} Tm ({s}) Tj ET`. The `1 0 0 -1` text matrix un-flips glyphs in the
y-down CTM; `y` is the baseline.

**Document** (`PDFDoc(ocgs, off=(), title)`), bytes:

1. Header `%PDF-1.6\n%\xe2\xe3\xcf\xd3\n`.
2. Objects numbered from 1, each `N 0 obj\n{body}\nendobj\n`, in this order:
   - 1 Catalog, 2 Pages (bodies filled last);
   - 3 `<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>`;
   - 4 same with `/Helvetica-Bold`;
   - one per OCG: `<< /Type /OCG /Name ({name}) /Intent [/View /Design] >>`;
   - per page: content stream object, then page object;
   - Info last.
3. Content stream body: `q {72/25.4 :.10f} 0 0 {−72/25.4 :.10f} 0 {hp:.4f} cm\n1 j 1 J 10 M\n` + ops joined by
   `\n` + `\nQ\n` (latin-1), zlib-compressed at level 9; object body
   `<< /Length {n} /Filter /FlateDecode >>\nstream\n{data}\nendstream`.
4. Page: `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {wp:.4f} {hp:.4f}] /Resources << /Font << /F1 3 0 R /F2 4 0 R >>
   /Properties << /oc0 R0 0 R /oc1 R1 0 R … >> >> /Contents C 0 R {extra}>>` (`wp`, `hp` in pt; `extra` is empty or
   ends with a space, giving `… 0 R >>` or `… 0 R /FPTile << … >> >>`).
5. Pages: `<< /Type /Pages /Kids [k1 0 R …] /Count n >>`.
6. Catalog: `<< /Type /Catalog /Pages 2 0 R /PageMode /UseOC /OCProperties << /OCGs [all] /D << /Name (Layers)
   /Order [all] /ON [on] /OFF [off] /RBGroups [] >> >> /ViewerPreferences << /PrintScaling /None
   /PickTrayByPDFSize true >> >>` (no radio-button groups, so several sizes can be visible at once).
7. Info: `<< /Title ({title}) /Producer (fabric-pattern export.py) /CreationDate (D:YYYYMMDDHHMMSS) >>` (local wall
   clock, no time zone — see §9).
8. `xref\n0 {N}\n0000000000 65535 f \n` + per object `{offset:010d} 00000 n \n`, then
   `trailer\n<< /Size {N} /Root 1 0 R /Info {info} 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n`.

Only the two standard Type 1 fonts are referenced (not embedded). All text is ASCII.

**OCG names** (identical across files and pages; Pattern Projector merges OCGs by name):

| Index | Name | Default |
|---|---|---|
| 0 | `Size {size}` | on |
| 1 | `Notches` | on |
| 2 | `Seam lines` | on |
| 3 | `Grainlines & labels` | on |
| 4 | `Placement marks` | on |
| 5 | `Soutache placement` | on |
| 6 (projector only) | `Calibration grid (10 cm / 1 in)` | **off** |

### 7.3 Drawing one piece (`draw_piece_pdf`)

Style per target (`PdfStyle`):

| Field | Tiles (×k) | Projector |
|---|---|---|
| cut / sew / notch / thin width | 0.3k / 0.2k / 0.3k / 0.2k | 1.0 / 0.5 / 1.0 / 0.6 |
| sew dash | (3k, 1.5k) | (4, 2) |
| text_scale | 1.0 (not scaled by k) | 1.0 |
| min_text | 0 | 12/CAP + 0.05 = 16.763 |
| grain arrow | 6k | 20 |
| ink (lines) | black (gray 0) | light: black; dark: white `(1,1,1)` |
| cut ink | = ink | light: black; dark: green `(0,1,0)` |
| halo gray | 1.0 | light 1.0; dark 0.0 |

Sequence (OCs nested inside the size OC):

1. `begin OC Size`; set cut ink (stroke+fill); width cut; solid; `FPCut` tag around the closed cut polyline; then the
   internal **cut** polylines (same style, untagged).
2. Ink; `OC Seam lines`: width sew, sew dash, closed sew polyline, solid; end.
3. `OC Notches`: width notch; every mark polyline; end.
4. `OC Placement marks`: width thin; each fold line (dash-dot `(6, 1.5, 1, 1.5)·text_scale` before a `dashdot` line,
   solid after each fold line); dash `(2, 1)·text_scale`; internal draw polylines; solid; drills: circle `r = d/2`
   plus a cross of half-length `d`; fold and internal labels (`label_texts`, `min_text`, `text_scale`), middle-anchored,
   with halo; end.
5. `OC Grainlines & labels`: width `1.3·thin`; grain marks (`arrow`); main label lines with halo; end.
6. If the piece has decor: `OC Soutache placement`; per decor: width = braid width, stroke gray 0.7 (0.45 when an
   ink colour is set), polyline; ink; width thin; same polyline; end.
7. `end OC Size`.

### 7.4 Tiled home-print PDFs — `<S>_LETTER.pdf`, `<S>_A4.pdf`

Paper (frame = printable content area, trimmed on the right and bottom during assembly):

| Paper | Page (mm) | Frame x, y | Frame w × h | Insets L/R, top, bottom |
|---|---|---|---|---|
| LETTER | 215.9 × 279.4 | 12.7, 12.7 | 190.5 × 254.0 | 12.7, 12.7, 12.7 |
| A4 | 210 × 297 | 10, 10 | 190 × 277 | 10, 10, 10 |

**Tile grid** over the master layout (`W × H`): `cols = max(1, ceil(W/fw − 1e−9))`, `rows = max(1, ceil(H/fh − 1e−9))`;
tile `(c, r)` covers master `[c·fw, (c+1)·fw] × [r·fh, (r+1)·fh]`, named `col(c) + (r+1)`, row-major order (A1, B1, …,
A2, …). A tile is **blank** when the union of all cut polygons does not intersect the tile rectangle shrunk by 0.01.
Tiles butt at the frame line (no overlap); content bleeds 5 mm past the frame for alignment checking.

Pages kept: all non-blank tiles, plus blank ones when `--keep-blank-pages`. Page numbers: info page = 1, kept tiles
numbered from 2 in row-major order. Title `"{S} {LETTER|A4} tiles"`, kind text `home tiles`, `k = 1`.

**Page 1 (info page)**, with `fx, fy, fw, fh` the frame:

1. Fill/stroke gray 0. Title `{S} - size {size}` at `(fx, fy + 8k)`, size 8k, bold.
2. Subtitle at `(fx, fy + 14k)`, size 3.2k: `{PAPER} {kind}  |  {cols} columns x {rows} rows  |  fabric-pattern/1, mm`
   + `  |  round {n}` when `meta.round` is set.
3. Test block (§7.4.1) at `(fx, fy + 20k)`, vertical, text size 3.5.
4. Text column at `colx = fx + 101.6 + 8`, width `colw = fx + fw − colx`, size `ts = 2.9k`, start
   `y = fy + 20k + ts`; each paragraph word-wrapped to `colw`, line advance `1.3·ts`, `0.6·ts` extra after each
   paragraph. Paragraphs, verbatim:
   1. `PRINT AT 100% / ACTUAL SIZE (Scale: None). Do NOT use Fit, Shrink oversized pages or Scale to fit.`
   2. `Print this page first. Measure both squares across AND down: 4 in = 101.6 mm, 10 cm = 100 mm. If either is off by more than 1 mm, fix the printer settings first.`
   3. `PRINT PAGES: {page_list(printed pages)} ({n} tiles).` followed by, when blank tiles exist:
      kept → ` Pages {page_list(blank pages)} are blank placeholders (tiles {A, B}): do not print them.`;
      dropped → ` Blank tiles {A, B} have nothing on them and are omitted from this file.`
   4. `ASSEMBLE: trim the RIGHT and BOTTOM margins of each page along the frame line, lay it over its right / lower neighbour so the frame lines meet and the half-diamonds form whole diamonds with the same code (e.g. A1|B1). Tape. Pattern lines run 5 mm past the frame so you can check alignment.`
   5. `LAYERS: use the Layers panel (Acrobat, Preview, pdf.js) to hide Seam lines, Notches, Grainlines & labels, Placement marks or Soutache placement. One layer per size.`
   6. If no blank tile was dropped: `STITCHING APPS (Pattern Projector, PDFStitcher): pages {page_list(all tile pages)}, {cols} columns, `;
      else `STITCHING APPS (Pattern Projector, PDFStitcher): blank tiles are omitted, so for a {cols}-column grid use the export made with --keep-blank-pages; `;
      then `edge insets {fx} mm left/right, {fy} mm top, {page_h − fy − fh} mm bottom.`
   7. `Seam allowance is included; the cut line is solid, the seam line dashed.`
5. A 1-inch square (tag `FPTest1in`, width 0.3) at `(colx, y + 2)`; label `1 in` (bold, size ts) at
   `(colx + 25.4 + 3, y + 2 + 12.7)`; `y += 25.4 + 8`.
6. `Tile map` (bold, size 1.1·ts) at `(colx, y)`; tile map in the box `(colx, y + 2, colw, fy + fh − y − 4)` with label
   size `0.8·ts`: scale `s = min(bw/(cols·fw), bh/(rows·fh))`; translate; width 0.25; per tile a rectangle (blank tiles
   first filled gray 0.88); all layout cut rings at scale `s`, line width `0.3/s`, gray 0.35; per tile label
   `{name}  p{page}` (bold) or `{name}  blank` (regular) at `(tx + 1, ty + ts + 0.6)`.

#### 7.4.1 Test block (`draw_test_block(x, y, vertical, tsize, gap=8)`)

Two squares: `FPTest4in` 101.6 mm (`4 in x 4 in` / `101.6 mm`) and `FPTest10cm` 100 mm (`10 cm x 10 cm` / `100 mm`);
the second at `(x, y + 101.6 + 8)` (vertical) or `(x + 101.6 + 8, y)` (horizontal). Per square: width 0.3, tagged
rectangle; width 0.15; ruler ticks, each drawn twice — along the **bottom** edge pointing up (`(px+d, py+s)` →
`(px+d, py+s−len)`, X ruler) and along the **left** edge pointing right (`(px, py+s−d)` → `(px+len, py+s−d)`, Y ruler):

| Square | Ticks `d` | Length |
|---|---|---|
| 4 in | `k·25.4/8`, k = 1..31 | 6 if k % 8 = 0, 4 if k % 4 = 0, else 2 |
| 10 cm | `k` mm, k = 1..99 | 6 if k % 10 = 0, 4 if k % 5 = 0, else 2 |

Texts (centred): first label bold size `tsize` at `(cx, cy − 1)`; second label size `0.8·tsize` at
`(cx, cy + 1.5·tsize)`; `ticks: X along bottom, Y along left` size `0.6·tsize` at `(cx, cy + 3·tsize)`.

#### 7.4.2 Tile pages

For each kept tile `(c, r)`, origin `(ox, oy) = (c·fw, r·fh)`, `ts = 2.6k`:

1. Non-blank: watermark — fill gray 0.85, tile name, bold, size `big = 0.22·min(fw, fh)`, centred at
   `(fx + fw/2, fy + fh/2 + big·CAP/2)`. Then `q`, clip to the frame grown by 5 mm on every side, translate by
   `(fx − ox, fy − oy)`, draw every layout item whose cut polygon buffered 1 mm intersects the master rectangle
   `[ox − 5, ox + fw + 5] × [oy − 5, oy + fh + 5]`, `Q`.
2. Frame: gray 0, width 0.3, solid, rectangle `(fx, fy, fw, fh)`.
3. Tile name at `(fx + 1.5, fy + 1.5 + ts)`, size `1.3·ts`, bold.
4. Blank placeholder: `{name} - blank, do not print`, bold, size 6k, centred at the frame centre (no join markers).
5. Non-blank: **join markers** toward each existing non-blank neighbour, checked in order right, left, top, bottom.
   Code `{A}|{B}` with the two tile names ordered by (row, column). Half-diamond (`base 30k`, `height 14k`, text size
   2.5k): edge midpoint `mid` and inward unit vector — right `(fx+fw, fy+fh/2)`, `(−1,0)`; left `(fx, fy+fh/2)`, `(1,0)`;
   top `(fx+fw/2, fy)`, `(0,1)`; bottom `(fx+fw/2, fy+fh)`, `(0,−1)`. With `along = (−in.y, in.x)`: triangle
   `[mid + 0.5·base·along, mid + height·in, mid − 0.5·base·along]`, fill gray 0.82, stroke 0, width 0.25, op `B`.
   Code text bold, fill 0: on vertical edges (`|in.x| > 0.5`) at `(mid.x + 2·in.x, mid.y + 0.35·ts)` anchored start
   (in.x > 0) or end; on horizontal edges centred at `(mid.x, mid.y + in.y·(2 + (ts·CAP if in.y > 0 else 0)))`.
   Each neighbour pair therefore shows two halves that form a whole diamond after assembly.
6. Footer at `(fx, foot_y)` size ts: `{S} size {size} | tile {name} | page {p} of {total} | print at 100% (Actual size) | trim right + bottom`;
   `foot_y = fy + fh + 4 + ts` (home) or `fy + fh + 8` (copyshop band); `total` = pages in the file.
7. Copyshop band only (§7.5).
8. Page dictionary extra `/FPTile << /Name ({name}) /Col {c} /Row {r} /Frame [{fx} {fy} {fw} {fh}] /Origin [{ox} {oy}] /Blank {true|false} >> `
   (numbers via `fnum`) — a stitching-app aid and the basis of the re-stitch check.

Manifest: `files.pdf_letter` / `files.pdf_a4` = `{file, kind: "tiled", paper_mm: [w, h], cols, rows, tiles,
printed, blank: [names], pages}`.

### 7.5 Copyshop — `<S>_COPYSHOP_A0.pdf`, `<S>_COPYSHOP_36x48in.pdf`

Papers `A0` = 841 × 1189 and `36x48in` = 914.4 × 1219.2. Frame `fx = fy = 10`, `fw = w − 20`, `fh = h − 10 − 135`
(a 135 mm information band below the frame). Orientation: try `(w, h)` then `(h, w)`; keep the one with fewer non-blank
tiles (first wins ties). `k = 2`. An info page (§7.4, with `k = 2`, paper name `A0`/`36x48in`, kind `copyshop`) is
added only when more than one sheet is non-blank. Title `{S} copyshop {label}`.

Band on every tile sheet, `by = fy + fh + 16`:
- test block at `(fx, by)`, horizontal, text size 4.0;
- scale bars at `(fx + 101.6 + 130, by + 30)`, text size 6: a 300 mm bar with ticks every 10 mm (6 long at multiples of
  50, else 3, drawn upward from the bar), labels `0 … 30` (cm) every 50 mm at `y − 8` (size 0.7·6), `cm` at `x + 305`;
  an inch bar 22 mm below, 12 in long, ticks every 1/4 in (6 long every inch, else 3), labels `0 … 12` every inch,
  `in` at `x + 12·25.4 + 5`; line width 0.3;
- `Check both squares before cutting. Copyshop: print at 100%, no scaling.` size 5 at `(fx + 231.6, by + 75)`.

Manifest key `pdf_copyshop_A0` / `pdf_copyshop_36x48in`: `{file, kind: "copyshop", paper_mm: [round(w,3),
round(h,3)], cols, rows, tiles, printed, blank, pages}`.

### 7.6 Projector — `<S>_PROJECTOR.pdf`, `<S>_PROJECTOR_DARK.pdf`

- Geometry: primary instance per piece (text_size 17); drop `internal` labels; main label reduced to
  `[name line, cut line]`.
- Layout §6.2; page `width = projector_width`, `H = 150 + layout_height + 100`. If `H ≥ 5080` or `width ≥ 5080`
  (200 in, the PDF page limit) raise `projector page {w} x {H} mm exceeds the 200 in PDF page limit; split the pattern`.
- OCGs: the six common ones + `Calibration grid (10 cm / 1 in)` (OFF by default).
- Dark variant first paints the page black (`0 g`, full-page `re f`).
- Header text (size `min_text` = 16.763, so cap ≥ 12 mm): `{S} size {size} - PROJECTOR (true scale, grain vertical)`
  bold at `(50, 30)`; `Calibrate to your mat; check the squares.` at `(50, 30 + 1.3·min_text)`.
- Test squares at `x = width − 50 − (101.6 + 100 + 8)`, `y = 40`: width 0.6, `FPTest4in` 101.6 square at `(x, y)`,
  `FPTest10cm` 100 square at `(x + 109.6, y)`; labels `4 in` at `(x + 50.8, y + 50.8)` and `10 cm` at
  `(x + 101.6 + 58, y + 50)`, bold, size `12/CAP`.
- Pieces (§7.3, projector style).
- Grid OC: width 0.3; vertical lines every 100 mm for `x ∈ [0, width]` (inclusive) over the full height; horizontal
  lines every 100 mm for `y ∈ [0, H]`; inch ticks 8 mm long along the top edge (`x` every 25.4 from 0, `< width`) and
  the left edge.
- Titles `{S} projector` / `{S} projector dark`. Manifest keys `pdf_projector` / `pdf_projector_dark`:
  `{file, kind: "projector", paper_mm: [width, round(H, 3)]}`.

### 7.7 DXF-ASTM / AAMA (R12) — `<S>_ASTM_METRIC.dxf`, `<S>_AAMA_METRIC.dxf`

Units cannot be declared in R12 (`$INSUNITS` does not exist): they travel in `Units:METRIC`, the file name and the
sender's note. Coordinates rounded to 2 dp.

Header variables set: `$ACADVER AC1009`, `$HANDLING 0` (no entity handles; at most the library's template keeps a
couple of group-5 lines — the test allows ≤ 2 occurrences of `\n  5\n`), `$CLAYER "1"`, `$PDMODE 0`, `$PDSIZE 0.0`,
`$EXTMIN`/`$EXTMAX` (computed, see quirk Q1). The reference writes ezdxf's full R12 template (tables VPORT, LTYPE,
LAYER, STYLE, VIEW, UCS, APPID, DIMSTYLE).

Layers (all colour 7), in addition to `0`/`Defpoints`:

| Layer | ASTM | AAMA | Content |
|---|---|---|---|
| 1 | ✓ | ✓ | closed cut line POLYLINE; style TEXT (modelspace); piece TEXT (block); INSERTs |
| 2 | ✓ | ✓ | turn POINTs |
| 3 | ✓ | ✓ | curve POINTs |
| 4 | ✓ | ✓ | notch POINT (V, slit; all types in AAMA) and notch LINE (all types) |
| 7 | ✓ | ✓ | grain LINE (exactly one) |
| 8 | ✓ | ✓ | internal draw lines, fold lines, decor centre lines (POLYLINE) |
| 11 | ✓ | ✓ | internal cuts (POLYLINE) |
| 13 | ✓ | ✓ | drill POINT, z = diameter |
| 14 | ✓ | ✓ | closed sew line POLYLINE |
| 15 | ✓ | — | annotations TEXT |
| 80 / 81 / 82 / 83 | ✓ | — | typed-notch POINTs: T / castle / check / U |
| 84 / 87 | ✓ | — | copies of the cut (84) and sew (87) polylines |

Modelspace style text: TEXT on layer 1, height 2.5, all at `(0, 0)`, in order:
`Style Name:{S}`, `Creation Date:{dd-mm-YYYY}`, `Creation Time:{HH-MM}`, `Author:fabric-pattern;export.py;1.0`,
`Sample Size:{safe_name(size)}`, `Grade Rule Table:`, `Units:METRIC`, and ASTM only
`ASTM/D13Proposal 1 Version: D 6673-10`, `Curve Tolerance:0.05`.

One BLOCK per piece (pieces order), named by the manifest block name (§7.13), built from the primary instance
(text_size 10; on-fold pieces unfolded), base point (0,0):

1. Cut ring and sew ring rounded to 2 dp with consecutive duplicates removed (cyclic): POLYLINE (2D, closed, flag 70=1,
   VERTEX…SEQEND) on 1 and 14; ASTM also on 84 and 87.
2. Turn and curve points of the cut ring, the sew ring (closed) and every internal line (its own `closed`):
   `turn_and_curve_points(pts, closed, threshold 15°, spacing 10)`:
   - a vertex is a turn point when the signed turning angle between incoming and outgoing unit directions exceeds
     15° in magnitude; on open polylines the first and last points are always turns; no turns → `[index 0]`;
   - between consecutive turns (cyclic for closed), if the run has more than 2 points and positive length: curve
     points at arclength `10, 20, …` while `< length − 5`, plus the point of maximum distance from the run's chord when
     that distance exceeds 0.5 and it is not an end point.
   Turn points + every notch base → POINT layer 2; curve points → POINT layer 3.
3. Notches: POINT at `(base.x, base.y, z = depth)`, group 39 thickness = width (0 for `slit`, which the library then
   omits), group 50 angle = `atan2(inward)` in degrees mod 360 (2 dp); layer from the table above (ASTM) or 4 (AAMA).
   Plus LINE layer 4 from `base` to `base + depth·inward`.
4. Grain LINE layer 7.
5. Layer 8: internal draw (closed → deduped ring + closed flag), fold lines (open), decor (open or closed).
6. Layer 11: internal cuts. Layer 13: drills as POINT `(x, y, z = diameter)`.
7. Piece TEXT layer 1, height 2.5, at `(lp.x, lp.y − 4i)` (`lp` = main-label anchor): `Piece Name:{block}`,
   `Size:{safe_name(size)}`, `Quantity:1,{1 if qty == 2 else 0}` (R,L counts), and `Material:{safe_name(material, 40)}`
   if a material is set.
8. ASTM only, TEXT layer 15: notes at `(lp.x, lp.y − 4·len(piece_text) − 7·(i+1))`, height 5.0: `ascii(name)`,
   `ascii(cut-text line)`, and per decor `{kind} {fnum(width)} mm centre line (layer 8)`; plus every `internal` label as
   TEXT height 3.5 at its anchor.
9. INSERT (layer 1) in modelspace at `(x − bbox.x0, 40 − bbox.y0)`; then `x += bbox width + 30` (x starts at 0).

TEXT entities carry the insert point and an equal alignment point (groups 10/20/30 and 11/21/31). All strings ASCII.
Manifest `files.dxf_astm` / `dxf_aama` = `{file, units: "METRIC", blocks: n}`.

### 7.8 Flat DXF (R2000) — `<S>_flat_mm.dxf`

For laser beds, Zünd/Eastman front ends, Silhouette. R2000 (`AC1015`), `$INSUNITS = 4` (mm) (`$MEASUREMENT` is 1 in the
reference output). Layers and colours in order: `CUT` 1, `DRAW` 5, `NOTCH` 3, `DRILL` 6, `GRAIN` 4, `TEXT` 7. No TEXT
or MTEXT entities. Layout §6.2 (instances, y up, grain vertical). Per item, LWPOLYLINEs (2 dp):

| Layer | Entities |
|---|---|
| CUT | cut ring (closed, deduped); internal cuts |
| DRAW | sew ring (closed, deduped); internal draw; fold lines; decor |
| NOTCH | every notch mark polyline |
| DRILL | CIRCLE radius `round(d/2, 3)` |
| GRAIN | `grain_marks(g, arrow = 8)` (3 polylines) |
| TEXT | every label line as stroke text: `stroke_text(text, x, y_baseline, size·CAP, middle, y_down=false)` from `label_texts(label, y_down=false)` |

Manifest `files.dxf_flat = {file, units: "mm", pieces: n}`.

### 7.9 HPGL — `<S>_plot.plt`

Master layout (`W × H`). Point map: `x = p.x`, `y = H − p.y` (y up, origin bottom-left); `--hpgl-mirror`:
`x = W − x`; `--hpgl-rotate90`: `(x, y) = (y, W − x)` (after mirror); units `round(40·value)` (40 units/mm, ties to
even). One command per line, file ends with `\n`.

```
IN;  PU;  SP1;  LT;  PA;  PU0,0;          (header, one per line)
per item:  [SP1;] SP1 work …  [SP2;] SP2 work …
PU;  SP0;  PG;                              (trailer)
```

`SPn;` is emitted only when the pen changes. A polyline: `PU{x},{y};` (first point), then `PD{x},{y},{x},{y}…;` in
chunks of at most 100 points, then `PU;`; closed polylines repeat the first point at the end.

- Pen 1 (draw), in order: sew ring (closed), internal draw, decor, grain marks (arrow 6), fold lines, drills as one line
  `PU{x},{y};CI{round(d/2·40)};PU;`, label text as stroke text (`label_texts(y_down)`, `stroke_text(…, size·CAP, middle,
  y_down=true)`).
- Pen 2 (cut), in order: internal cuts, notch marks, cut ring last (closed).

Drawing before cutting keeps a knife-cut piece from shifting before its marks are drawn (differs from Valentina's
pen-only order). Manifest: `hpgl = {file, units_per_mm: 40, rotate_90, mirror, sheet_mm: [round(W,3), round(H,3)],
cut_bbox: [master-frame bbox of all cuts, round 3]}`; `files.hpgl = {file}`.

### 7.10 Cutter SVGs + report — `<S>_cutter/`

Per instance (every instance, text_size 6):

1. Rotate the geometry so the grain is vertical (§5.4).
2. Outline = union of the cut polygon (buffered +1e−4, mitre) with one external triangle per notch
   `[base + 2·tangent, base + 3·outward, base − 2·tangent]` (4 mm wide, 3 mm deep, `outward = −inward`), then buffered
   −1e−4 (mitre); keep the largest part; orient CCW; clean (§4.4). A rotary blade cannot cut 1 mm slits.
3. Margin 1 mm; `w, h` = outline bbox + 2; points flipped to y down: `(x − minx + 1, maxy − y + 1)`.
4. File `<S>_cutter/{safe_name(iid with "."→"_", 40)}_{safe_name(size)}.svg`:

```
<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{w}mm" height="{h}mm" viewBox="0 0 {w} {h}">
<title>{name} {iid} size {size}: cut line incl. seam allowance, grain vertical</title>
<path d="M… Z" fill="none" stroke="#000" stroke-width="0.1"/>
</svg>
```

Mat limits (cut area): `12x12` = 11.5 × 11.5 in (292.1 × 292.1 mm), `12x24` = 11.5 × 23.5 in (292.1 × 596.9 mm). With
`bw, bh` the outline size: `fits_sq = bw ≤ 292.1 ∧ bh ≤ 292.1`; `fits_long = bw ≤ 292.1 ∧ bh ≤ 596.9`;
`fits_crosswise = bh ≤ 292.1 ∧ bw ≤ 596.9`. Report record:
`{file: "<S>_cutter/<name>.svg", instance, width_mm: round(bw,1), height_mm: round(bh,1), mat: "12x12" | "12x24" | null,
too_large: !fits_long, fits_if_fabric_turned_crosswise: !fits_long ∧ fits_crosswise}`, plus
`note: "too large for mat (11.5 x 23.5 in): print, project, or split with an added seam"` when too large.
`cutter_report.json` = `{"mat_limit_in": [11.5, 23.5], "pieces": [...]}`. Never auto-split a piece. Manifest
`files.cutter = {file: "<S>_cutter/cutter_report.json", count, too_large: [instance ids]}`.

### 7.11 GarmentCode JSON — `<S>.garmentcode.json`

Net sew-line shapes for a 3D sewing check, in GarmentCode's specification format.

1. **Instances**: every instance whose piece has `sim` not equal to `false` (instance order).
2. **Segments** per edge: `line` → one line segment; `quad`/`cubic` → one Bézier segment with absolute controls;
   `poly` → straight segments between consecutive poly points. Segment length: chord (line) or the length of 201
   uniform-t samples (Bézier).
3. **Seam plan** (seams in order). Resolve both sides (`instance/edge[+edge…]`); on an error add warning
   `seam {id}: {error}` and skip; if either instance is excluded add `seam {id}: instance excluded from simulation (sim: false)`
   and skip. Then:
   - side fractions of internal segment boundaries: `ba` (side a, total length `la`), `bb_own` (side b);
   - knots: `rev = seam.reverse` (default true); `na`, `nb` = notch fractions of each side (pattern-model rule: notches
     in the side's own direction, mirrored halves reversed, end notches within 1e−4 excluded); if `rev`,
     `nb = sorted(1 − f)`; knots `ka = [0, *na, 1]`, `kb = [0, *nb, 1]` when `na` is non-empty and the counts match,
     else `[0, 1]` each;
   - `bb = sorted(1 − f for f in bb_own)` if `rev` else `bb_own`; `to_b`, `to_a` = piecewise-linear maps between the
     knot lists;
   - candidates `(fa, fb, src)`: each `ba` with `to_b`, each `bb` with `to_a`, each internal knot pair (`"k"`); sort as
     tuples;
   - `tol = 0.05 / la`; skip candidates with `fa ≤ tol` or `fa ≥ 1 − tol`; merge a candidate into the previous pair
     when `|fa − prev.fa| ≤ tol` (keep the new `fa` if it came from side a, the new `fb` if from side b; concatenate
     sources); otherwise append;
   - split fractions: `sa = [fa]`, `sb_own = sorted(1 − fb)` if `rev` else `[fb]`.
4. **Split** each side at its fractions (side a then side b, seams in order; later seams see earlier splits): for each
   segment covering side fractions `[f0, f1]`, split at every fraction strictly inside (`f0 + 1e−9 < f < f1 − 1e−9`),
   successively, with local parameter `uu = (u − done)/(1 − done)`. Line split = point at fraction. Bézier split: find
   `t` at arclength fraction `uu` by interpolating 801 uniform-t samples, then de Casteljau. If the two sides end up
   with different sub-segment counts: warning `seam {id}: could not pair sub-edges ({na} vs {nb})`, no stitches.
5. **Panels** (name = instance id with `.` → `_`): flatten all segments in loop order; `vertices[k]` = segment start in
   **cm** (`/10`, round 5); `edges[k] = {"endpoints": [k, (k+1) mod n]}` plus for Bézier segments
   `"curvature": {"type": "quadratic"|"cubic", "params": [[t, h] …]}` with `[t, h] = abs_to_rel(start, end, control)`
   rounded to 6 dp (relative coordinates are scale-free; GarmentCode's `rel_to_abs_2d` computes
   `start + t·edge + h·(−edge.y, edge.x)`, unnormalised — identical to ours). `edge_map[panel][k]` = source edge id.
   Placement: `pl = piece.placement3d` (if a dict: `pl[instance id]`, else `pl[side]`, else `pl` itself), overridden by
   top-level `placement3d[instance id]` when present; `translation` = mm/10 rounded 4 (default `[0,0,0]`), `rotation`
   = XYZ degrees (default `[0,0,0]`). Loops stay CCW, so at rotation 0 the public side faces +z; back panels need
   rotation `[0, 180, 0]`.
6. **Stitches**: for each paired side with `K` sub-segments, `k = 0..K−1`: pair `A[k]` with `B[K−1−k]` (rev) or `B[k]`;
   `[{"panel", "edge"}, {"panel", "edge"}]`, plus a third element `"right_wrong"` when not `rev` (GarmentCode's default
   `swap=True` already matches `reverse: true`).
7. Document:

```
{"pattern": {"panels": {...}, "stitches": [...], "panel_order": [names]},
 "parameters": {}, "parameter_order": [],
 "properties": {"curvature_coords": "relative", "normalize_panel_translation": false,
                "normalized_edge_loops": true, "units_in_meter": 100},
 "fabric_pattern_export": {"source_schema", "style", "size", "edge_map", "warnings", "notes": [
   "net sew-line shapes only: no seam allowance, notches, grain, internal lines or decor",
   "multi-edge seam sides are split at matched arclength points (incl. notches) into 1:1 stitches",
   "poly edges become straight sub-edges; translation/rotation are zero unless placement3d is given",
   "loops are CCW as drafted (public side toward +z for rotation 0)"]}}
```

Manifest `files.garmentcode = {file, panels, stitches, warnings}` (counts).

### 7.12 Embroidery (soutache couching, Tajima DST)

For each instance (instance order) that carries `soutache` decor:

1. Design centre `c` = centre of the sew ring's bounding box (the hoop centre).
2. Placement run: the sew ring (closed) minus `c`, resampled at 5.0 mm (`PLACE_STITCH_MM`).
3. Braid runs: soutache lines minus `c`, ordered and joined (below), each resampled at 2.5 mm (`COUCH_STITCH_MM`).
   - Resample: close the polyline if needed (append the first point when first and last differ by > 1e−6);
     `n = max(1, ceil(length/step))`; `n + 1` points at uniform arclength fractions.
   - Join: repeatedly find the first pair of open lines `(i < j)` whose ends meet within 0.5 mm, trying
     `(a, b)`, `(a, reversed b)`, `(reversed a, b)`, `(reversed a, reversed b)` in that order (end of the first to start
     of the second); replace `i` by the concatenation (dropping the shared point) and remove `j`; restart. (Joins the
     two half borders of an unfolded piece at the fold.)
   - Order: from the current point (start: the design centre), pick the line (and, for open lines, direction) whose
     start is nearest; ties keep the first candidate (lines in order, forward before reversed); continue from its end
     (closed lines: from its start).
4. File `<S>_{safe_name(iid with "."→"_", 40)}_{safe_name(size)}_soutache.dst`, label `"{S[:8]} {iid}"`, blocks
   `[run(placement), stop, run(braid 1), run(braid 2), …]`.

**DST writer.** Units 0.1 mm, y up, design starts at the hoop centre `(0, 0)`.

- `go(target, flag)`: `d = target − pos`; `n = max(1, ceil(max(|dx|, |dy|)/121))`; for `k = 1..n` step to
  `pos + (round(d·k/n) − round(d·(k−1)/n))` (per axis, ties to even), one record each; for stitches add the step
  length × 0.1 to the current run's sewn length. A zero move still produces one record.
- `stop` block: record `(0, 0, STOP)`.
- `run` block: points → integer units `round(v/0.1)`; start a new sewn-length entry; `go(first, JUMP)`; then one
  **stitch in place** `(0, 0)` (a jump does not sew, so the first segment would be lost); for each further point that
  differs from the current position: `go(point, stitch)`. **Tie-off**: if the last stitch vector `seg` is non-zero:
  `back = round(pos − unit(seg)·min(10, |seg|))` (1 mm back along the last stitch), `go(back)`, `go(end)`.

**Record encoding** (3 bytes, balanced ternary, max 121 units per axis per record):

| Axis step | Byte, `+` bit, `−` bit (X) | Byte, `+` bit, `−` bit (Y) |
|---|---|---|
| 81 | b2 0x04, 0x08 | b2 0x20, 0x10 |
| 27 | b1 0x04, 0x08 | b1 0x20, 0x10 |
| 9 | b0 0x04, 0x08 | b0 0x20, 0x10 |
| 3 | b1 0x01, 0x02 | b1 0x80, 0x40 |
| 1 | b0 0x01, 0x02 | b0 0x80, 0x40 |

Start `[0, 0, 0x03 | flag]` (`flag`: 0 stitch, `0x80` jump, `0xC0` stop). For each axis value `v`, for `p` in
`81, 27, 9, 3, 1` with `half(p) = 40, 13, 4, 1, 0`: if `v > half` set the `+` bit and `v −= p`; elif `v < −half` set
the `−` bit and `v += p`. A non-zero remainder is an error (`DST move dx,dy exceeds 121 steps`). END record =
`00 00 F3`. Decoding: sum `±p` per set bit; kind = `stop` if `(b2 & 0xC0) == 0xC0`, `jump` if `b2 & 0x80`, else `stitch`;
the exact bytes `00 00 F3` decode as `end`. Goldens: `(0,0)`→`00 00 03`, `(1,0)`→`01 00 03`, `(−1,0)`→`02 00 03`,
`(0,1)`→`80 00 03`, `(0,−1)`→`40 00 03`, `(121,121)`→`a5 a5 27`, `(−121,−121)`→`5a 5a 1b`, `(40,0)`→`05 05 03`,
`(41,0)`→`0a 0a 07`, `(13,−13)`→`55 41 03`, jump `(5,−7)`→`56 82 83`, stop→`00 00 c3`.

**Header** (512 bytes): ASCII text, fields separated by `\r`:
`LA:{label:<16}` (`ascii_text(label)[:16]`, left-justified), `ST:{records:7d}` (records excluding END), `CO:{stops:3d}`,
`+X:{max x:5d}`, `-X:{−min x:5d}`, `+Y:{max y:5d}`, `-Y:{−min y:5d}` (extents over all positions including the origin),
`AX:{±}{|last x|:5d}`, `AY:{±}{|last y|:5d}`, `MX:+{0:5d}`, `MY:+{0:5d}`, `PD:******`, each followed by `\r`; then
`0x1A`; padded with spaces (0x20) to 512 bytes. Then the records, then END. Example header start:
`LA:TESTBODI front.R\rST:    271\rCO:  1\r+X:  775\r…`.

Returned info: `records`, `stops`, `sewn_mm` (per run, round 2), `extent_mm` = `[round((maxx − minx)·0.1, 1),
round((maxy − miny)·0.1, 1)]`.

Hoop (smallest that fits in either orientation), from `[(100,100), (130,180), (160,260), (200,300), (240,360)]` →
`"{a}x{b} mm"`, else `"over 240x360 mm: a commercial frame"`.

Manifest key `dst_{iid}` (raw instance id, e.g. `dst_front.R`): `{file, kind: "soutache couching", instance,
centre_mm: [round 3], placement_mm: sewn[0], couch_mm: round(Σ sewn[1:], 2), braid_mm: round(Σ resampled run
lengths, 1), braid_lines, size_mm: [w, h], hoop, stitch_mm: 2.5, records, stops, sewn_mm, extent_mm}`.

**README** `<S>_soutache_README.txt` (only when at least one DST was written):

```
Soutache couching files (Tajima DST, 0.1 mm units, design centred on the hoop).
1. Hoop the UNCUT fabric (stabilizer behind). Colour 1 stitches the placement line = the sew line.
2. At the stop, lay the soutache braid along the printed braid line (PDF layer 'Soutache placement').
3. Colour 2 sews a 2.5 mm straight stitch down the braid's centre groove; use a braiding
   or cording foot / guide if the machine has one, otherwise couch by hand along the same line.
4. Cut the piece out with its seam allowance outside the placement line (cutter files / PDF cut line).
Braid ends that reach a seam line are caught in that seam. Left pieces are mirrored files.

{file}: {w} x {h} mm, hoop {hoop}, {n} braid line(s)        (one line per DST)
```

A pattern without soutache writes no DST and no README (the target returns nothing). Not written: PES, JEF, EXP
(convert the DST); colours (DST colour changes are stops); cording-device commands.

### 7.13 Export manifest — `<S>_export_manifest.json`

Top-level keys (fresh file order): `schema` (`fabric-pattern-export/1`), `style`, `size`, `created`, `source_schema`,
`files`, `pieces`, `master`, `hpgl`. `files` keys: `svg_master`, `pdf_letter`, `pdf_a4`, `pdf_copyshop_A0`,
`pdf_copyshop_36x48in`, `pdf_projector`, `pdf_projector_dark`, `dxf_astm`, `dxf_aama`, `dxf_flat`, `hpgl`, `cutter`,
`garmentcode`, `dst_<iid>`…; each has `file` (path relative to the output dir, `/` separators) plus the fields listed
per target.

`pieces` (the DXF reference values), one entry per piece in pieces order, keyed by **block name**:
`safe_name(piece id)`; on collision `safe_name(id, 17) + "_k"` for k = 2, 3, …. Value:
`{piece, instance (primary), sew_perimeter: round(instance perimeter, 3), sew_area: round(signed area of
instance.outline(200), 2), cut_perimeter: round(perimeter of cut_outline(primary, sa, mitre), 3), cut_area: round(signed
area of that cut ring, 2)}`.

---

## 8. Acceptance checks

### 8.1 Orchestration (`check_dir(out, pattern_ref=None)`)

1. Manifest: first of the sorted `*_export_manifest.json`; none → single failed item `manifest` (file = dir, detail
   `no *_export_manifest.json`) and stop.
2. With a pattern reference: recompute, per manifest block, the primary instance's unrounded sew perimeter and sew area
   (outline 200) as the sew reference.
3. Run, in order: DXF checks for `dxf_astm`, `dxf_aama`; flat DXF; master SVG (if `master` is listed and the file
   exists; its cut-path vertices become the restitch reference); every `pdf_*` entry with its `paper_mm` and `kind`;
   every `*.svg` (sorted) in the cutter directory; HPGL; GarmentCode; every `dst_*` entry (with a pattern reference:
   the instance's soutache lines rebuilt with the pattern's own `meta.sa_mm`).
4. No item at all → single failed item `check.nothing_checked` (`manifest lists no checkable files`).
5. Report items: `{check, file (file name), ok, detail}`; overall OK = all items OK.

A listed file that is missing raises (it is not reported as a failed check).

### 8.2 Every check

| Check | Applies to | Passes when | Tolerance | Detail on the item | Needs pattern ref |
|---|---|---|---|---|---|
| `manifest` | dir | (only emitted as a failure) | | `no *_export_manifest.json` | |
| `check.nothing_checked` | dir | (only emitted as a failure) | | `manifest lists no checkable files` | |
| `dxf.ascii` | ASTM, AAMA | every byte < 128 | | | |
| `dxf.no_lwpolyline` | ASTM, AAMA | bytes contain no `LWPOLYLINE` | | | |
| `dxf.r12` | ASTM, AAMA | version `AC1009` | | version string | |
| `dxf.block_count` | ASTM, AAMA | user blocks (names not starting `*` or `$`) = manifest pieces | | `{n} blocks vs {m} pieces` | |
| `dxf.inserts` | ASTM, AAMA | sorted INSERT names in modelspace = sorted block names | | `{n} INSERTs` | |
| `dxf.style_text` | ASTM, AAMA | modelspace TEXT includes `Units:METRIC` and at least one starting `Style Name:` | | | |
| `dxf.block_name` | each block | ≤ 20 chars, ASCII | | block name | |
| `dxf.layer1_closed` | each block | ≥ 1 layer-1 POLYLINE, all closed | | `{b}: {n} layer-1 polylines` | |
| `dxf.one_grain_line` | each block | exactly one LINE on layer 7 | | `{b}: {n}` | |
| `dxf.piece_text` | each block | exactly one TEXT starting `Piece Name:`, value ≤ 20 chars, all block TEXT ASCII | | block name | |
| `dxf.reference` | each block | (failure only) no manifest entry or no layer-1 polyline; skips the geometry checks of that block | | `{b}: no reference` | |
| `dxf.cut_perimeter` | each block | perimeter of the largest-area layer-1 polyline vs `cut_perimeter` | ≤ 0.1 mm | `{b}: {p:.3f} vs {ref:.3f}` | |
| `dxf.cut_area` | each block | its area vs `cut_area` | ≤ 0.1 × `cut_perimeter` (a 0.1 mm boundary shift) | `{b}: {a:.1f} vs {ref:.1f} mm2` | |
| `dxf.sew_perimeter` | each block | first layer-14 polyline perimeter vs sew reference (recomputed if a pattern is given, else manifest) | ≤ 0.1 mm | `{b}: {p:.3f} vs model {ref:.3f}` | optional |
| `dxf.sew_area` | each block | its area vs sew reference | ≤ 0.1 × sew perimeter | `{b}: {a:.1f} vs model {ref:.1f}` | optional |
| `dxf.sew_line` | each block | (failure only) no layer-14 polyline | | `{b}: no layer-14 sew line` | |
| `flatdxf.layers` | flat DXF | layers ⊇ {CUT, DRAW, NOTCH, DRILL, GRAIN, TEXT} | | | |
| `flatdxf.cut_closed` | flat DXF | ≥ 1 CUT LWPOLYLINE and at least one closed | | `{n} CUT polylines` | |
| `flatdxf.no_text_entities` | flat DXF | no TEXT/MTEXT in modelspace | | | |
| `svg.no_transform` | master + each cutter SVG | the substring `transform` occurs nowhere in the file | | | |
| `svg.mm_units_viewbox` | master + each cutter SVG | width/height end in `mm`; viewBox = `0 0 W H` with W, H equal to the width/height numbers | 1e−6 | `{w} x {h}, viewBox {vb}` | |
| `svg.piece_subgroups` | master | every top-level `<g id="piece-…">` has child-group classes ⊇ {cut, sew, notches, grain, fold, internal-draw, internal-cut, drills, labels, decor-soutache} | | ids lacking some, comma-joined | |
| `cutter.single_closed_path` | each cutter SVG | exactly one `<path>`, its `d` (right-trimmed) ends with `Z` | | | |
| `pdf.print_scaling_none` | every PDF | catalog contains `/PrintScaling /None` | | | |
| `pdf.ocg_unique` | every PDF | OCG names unique, at least one | | names, `, `-joined | |
| `pdf.mediabox_exact` | every PDF | every page MediaBox `[0 0 w h]` equals `paper_mm × 72/25.4`; ≥ 1 page | < 0.01 pt | `expected {w:.4f} x {h:.4f} pt, {n} pages` | |
| `pdf.ocg_identical_across_pages` | every PDF | the `/Properties << … >>` text is identical on every page | | | |
| `pdf.test_squares_page1` | tiled | page 1 has `FPTest4in` measuring 101.6 × 101.6 and `FPTest10cm` 100 × 100 | < 0.05 mm each | the measured dict | |
| `pdf.test_squares_every_sheet` | copyshop | every page passes the square test | < 0.05 mm | `{ok}/{n}` | |
| `pdf.test_squares` | projector | page 1 passes the square test | < 0.05 mm | the measured dict | |
| `pdf.projector_text_cap_12mm` | projector | text present and min effective font size (pt) / (72/25.4) × 0.718 ≥ 12 | −1e−3 | `min cap height {:.2f} mm` or `no text` | |
| `pdf.projector_under_200in` | projector | max(paper_mm) < 5080 | | | |
| `pdf.restitch_matches_master` | tiled, copyshop (needs the master SVG) | every `FPCut` path point of every tile, mapped back to master mm (below), lies within 0.2 mm of a master cut vertex | ≤ 0.2 mm | `max deviation {:.4f} mm over {n} tiles` | |
| `pdf.tiles_cover_master` | tiled, copyshop (needs the master SVG) | every master cut vertex lies inside some non-blank tile's `[origin, origin + frame size]` | ±1e−6 | `{n}/{m} master cut vertices on printed tiles` | |
| `hpgl.same_layout_as_master` | HPGL (when the manifest has `master`) | `hpgl.cut_bbox` absent or each value within 0.01 of `master.cut_bbox` | 0.01 mm | | |
| `hpgl.bbox` | HPGL | (failure only) no pen-2 coordinates | | `no SP2 coordinates` | |
| `hpgl.bbox_matches_master` | HPGL | pen-2 bbox / 40 vs `(x0, H − y1, x1, H − y0)` of the master cut bbox (`H` = master height); with rotate/mirror only the bbox **area** is compared | ≤ 0.1 mm per value; area within 0.1 × (w + h) | `plt (…) vs master (…)` (3 dp) | |
| `gc.units_cm_relative` | GarmentCode | `units_in_meter == 100` and `curvature_coords == "relative"` | | | |
| `gc.edge_loops_closed` | GarmentCode | every edge's end index = the next edge's start index (cyclic) in every panel | | | |
| `gc.stitches_valid_unique` | GarmentCode | both sides of every stitch name an existing panel and edge index, and no (panel, edge) is used twice | | `{n} stitches` | |
| `dst.header` | each DST | fields LA, ST, CO, +X, −X, +Y, −Y present and file ≥ 512 bytes | | `fields [sorted keys]` | |
| `dst.ends_once` | each DST | one END record and nothing after it | | `one END record, nothing after it` | |
| `dst.count` | each DST | `ST` is digits and equals the decoded record count (excluding END) | | `ST {st} vs {n}` | |
| `dst.extents` | each DST | decoded `(max x, −min x, max y, −min y)` (origin included) = header `+X −X +Y −Y` | exact | `decoded …, header …` | |
| `dst.one_stop` | each DST | exactly one stop record and `CO` = `1` | | `{n} stop(s)` | |
| `dst.stitch_length` | each DST | moving stitches (length > 0; stitches in place excluded) exist, min ≥ 0.5 mm, max ≤ 12.1 mm | +1e−9 | `{min:.2f}-{max:.2f} mm` | |
| `dst.placement_length` | each DST | decoded stitch length before the stop = manifest `placement_mm` | ≤ 0.05 mm | `{got:.2f} vs {want} mm` | |
| `dst.couch_length` | each DST | decoded stitch length after the stop = manifest `couch_mm` | ≤ 0.05 mm | `{got:.2f} vs {want} mm` | |
| `dst.braid_inside_placement` | each DST | more than 2 placement stitches, and every couching stitch lies inside the placement polygon grown by 0.5 mm | 0.5 mm | `{n} couching stitches` | |
| `dst.on_braid_lines` | each DST | farthest couching stitch from the nearest drafted braid line (pattern mm − `centre_mm`) | ≤ 0.2 mm | `farthest couching stitch {:.3f} mm from a drafted braid line` | **yes** |
| `dst.covers_braid` | each DST | couched length ≥ 0.99 × total drafted braid length (ties only add) | 1 % | `sewn {:.1f} of {:.1f} mm drafted` | **yes** |

Tile point mapping for the restitch check: for a content point `(x, y)` in page pt and page height `mbh` pt,
`master = (x/k_pt − frame.x + origin.x, (mbh − y)/k_pt − frame.y + origin.y)` with `k_pt = 72/25.4`. Nearest-vertex
search: hash grid of 1 mm cells over the master vertices, rings r = 0..3, stop once the best distance ≤ r; nothing within
the rings → infinite (fails). This compares **vertices to vertices**: the PDF cut polylines and the master SVG cut path
must come from the same vertex list (both written from the master layout at 3 dp).

DST decoding for checks: header = the first 512 bytes split on `\r`; an item with length > 3 and `:` at index 2 is
`key = item[0:2]`, value = rest trimmed. Records from byte 512 in steps of 3; after END, any further record marks
trailing data. Positions accumulate from (0, 0); stitches before the first stop are placement, after it couching;
jumps do not count.

### 8.3 PDF reader used by the checks

The checker re-reads its own PDFs with a minimal scanner tied to the writer's layout: objects found by
`(\d+) 0 obj\n`, a stream located by `stream\n` before `endobj`, sized by `/Length`, inflated when `/FlateDecode`.
OCG names come from `/Type /OCG /Name (…)` (escapes removed). Pages: dictionaries matching `/Type /Page\b` but not
`/Type /Pages`. The content interpreter tracks `q`/`Q`, `cm` (pre-multiplied onto the CTM), `BMC`/`BDC` tag stack
(`/OC` entries recorded as `OC:<name>`), `re` (rectangle size scaled by the CTM axis lengths), `m`/`l` points inside an
`FPCut` tag (transformed by the CTM), `Tm` (scale = `hypot(a, b)`) and `Tf` (effective size = size × Tm scale ×
`sqrt(|det CTM|)`). A Rust port may write a richer PDF only if its checker still reads it the same way.

---

## 9. Determinism and timestamps

Running the reference twice on the same input gives byte-identical master SVG, HPGL, cutter SVGs and report,
GarmentCode JSON, DST files, README and manifest (the manifest because `created` comes from `meta.created`). The
differences are all clocks or random ids:

| File | Non-deterministic part |
|---|---|
| every PDF | `/CreationDate (D:YYYYMMDDHHMMSS)` in the Info dictionary (local wall clock, 14 digits, so file length is stable) |
| ASTM/AAMA DXF | `Creation Date:dd-mm-YYYY` and `Creation Time:HH-MM` TEXT (local wall clock, not `created`); `$TDCREATE`/`$TDUPDATE` (Julian dates); library XDATA `EZDXF / CREATED_BY_EZDXF / WRITTEN_BY_EZDXF` with ISO timestamps |
| flat DXF | `$TDCREATE`/`$TDUPDATE`; `$FINGERPRINTGUID`/`$VERSIONGUID` (random); library metadata timestamps |
| manifest | `created` only when neither the argument nor `meta.created` is set (local now, minute resolution) |

Recommendation for the port: derive every date from `Ctx.created` (and use fixed GUIDs derived from content) so the
whole export is reproducible; keep the printed formats.

Floating-point notes: outputs are rounded at write time (3 dp SVG/PDF, 2 dp DXF, 1/40 mm HPGL, 0.1 mm DST, 5/6 dp
GarmentCode). Python formats the exact binary value with ties-to-even, and `round()` is ties-to-even. Geometry that
goes through GEOS (union fallback, buffers, pole of inaccessibility, intersections) will not match bit-for-bit with a
different geometry library.

---

## 10. Conformance oracle

### 10.1 Test inventory (`test_export.py`, 26 cases, all passing)

| # | Test | What it pins |
|---|---|---|
| 1 | rect SA 10 → 120×70 | direct offset, method `mitre`, area 8400 |
| 2 | rect fold edge zero SA | 110×70, fold edge stays at x = 0 |
| 3 | unfolded instance full SA | 220×70 |
| 4 | per-edge SA and mitre limit | hem 30 → bbox (−10,−30,110,60); 20° spike clipped at exactly 4 × SA = 40 along the bisector |
| 5 | concave curve → union | scoop with SA 15: method `union`, valid, 130×110 |
| 6 | notches point inward | sleeve.R: 4 notches; base within 0.05 of the cut exterior; apex inside the cut; depth < 10 |
| 7 | mirrored notches mirror | sleeve.L notch bases = sleeve.R bases with x negated (1e−3) |
| 8 | every exporter writes files | the 14 expected names; cutter files `back_2T.svg, front_L_2T.svg, front_R_2T.svg, sleeve_L_2T.svg, sleeve_R_2T.svg` |
| 9 | acceptance checks pass | no failure; the check names in §8.2 for DXF/PDF/SVG/HPGL/GC/cutter/projector/copyshop are present |
| 10 | check against the pattern file | `check_dir(out, pattern)` OK |
| 11 | CLI check + failure detection | `check` exits 0; inserting `<g transform="scale(1)"/>` before `<title>` makes `svg.no_transform` fail |
| 12 | master SVG structure | `width="…mm" height="…mm" viewBox="0 0 …"`, `id="piece-back"`, `data-on-fold="cb"`, `data-fold-mode="half"`, `PLACE ON FOLD`, `data-qty="2" data-mirror="true"`, `class="decor-soutache"`, `data-width-mm="3"` |
| 13 | piece note printed | `Elastic 25 mm wide: cut 524 mm` appears in the SVG |
| 14 | `--unfold` | `data-fold-mode="unfolded"`, no `PLACE ON FOLD`, has `FOLD LINE (cb)` |
| 15 | DXF-ASTM details | `$HANDLING 0`; ≤ 2 group-5 lines; sleeve: 3 layer-4 POINTs with z > 0 and thickness > 0, 1 layer-80 POINT (T), 4 layer-4 LINEs, texts `Piece Name:sleeve`, `Quantity:1,1`, `Size:2T`; front: 1 drill POINT z = 3 on 13, POLYLINEs on 11 and 8; back `Quantity:1,0`; AAMA has no layers 80, 84, 15 |
| 16 | HPGL pens/units | starts `IN;\nPU;\nSP1;`, ends `PU;\nSP0;\nPG;`, contains `SP2;` and `CI` |
| 17 | cutter report | back width > 292.1, too large, note mentions `split`; sleeve.R fits; sleeve_R SVG has one path and no transform |
| 18 | GarmentCode relative curvature | panels {front_R, front_L, back, sleeve_R, sleeve_L}; front_R `neck` control points re-derived with GarmentCode's `rel_to_abs_2d` match ours /10 (1e−4); type `cubic`; first vertex = p0/10 |
| 19 | GarmentCode stitches | no warnings; stitches ≥ seams; all 2-element; sleeve_R `cap_b` split into 3 stitches all against back `armhole.R`; panel perimeter (cm) = instance perimeter/10 (rel 2e−4) |
| 20 | projector + dark | MediaBox width = 1500 mm in pt (0.01), height < 14400 pt; `0 1 0 RG` present; OCGs include the grid and `Soutache placement`; `/OFF` non-empty |
| 21 | robustness pattern | yoke outline valid; all exports pass checks; no `stand` panel; a `sim: false` warning; a `right_wrong` stitch; 26 GC edges map to yoke `ef` (25 interior poly points); ≥ 26 yoke stitches against collar_R `neck.R`/`neck.L`; master SVG has `data-notch="n_cb"` |
| 22–23 | blank tiles omitted / kept | `layout_width = 300`, Letter: blank `["B1","B2"]`, 8 tiles, pages 1 + 6 (dropped) or 1 + 8 (kept); page-1 text contains `omitted` / `placeholders`; `/Blank true` occurs 0 / 2 times; checks pass |
| 24 | DST record round trip | every `dx ∈ [−121, 121]` × `dy ∈ {−121,−80,−40,−13,−4,−1,0,1,4,13,40,80,121}`; jump and stop flags; END; `(122, 0)` raises |
| 25 | soutache files retrace the braid | braided bolero: 5 DSTs + README; back `braid_lines = 3` (two half borders joined at the fold + 2 motifs); with the pattern: 55 DST checks (5 × 11) all pass; plain bolero → no files |
| 26 | shifted braid caught | rewriting front.R with the braid 4 mm off (+x): `dst.on_braid_lines` fails while `dst.couch_length` still passes |

### 10.2 Fixture patterns (all synthetic; reproduce them in the port's test data)

**TESTBODICE** (`schema fabric-pattern/1`, `units mm`, `meta {size_label "2T", sa_mm 10, round 0, created
"2026-10-11T12:00"}`, `fabric {name "wool melton", width_mm 1500}`). Builder conventions: vertices rounded to 3 dp,
cubic controls given absolute and stored relative (5 dp), notch `s` rounded to 4 dp.

| Piece | cut | Vertices | Edges (from→to, curve, SA) | Notches (edge, s, type) | Extras |
|---|---|---|---|---|---|
| front "Front bodice" | qty 2, material wool | side_hem (0,0), cf_hem (150,0), cf_neck (150,240), sh_neck (95,290), sh_tip (10,275), underarm (−5,170) | hem side_hem→cf_hem sa 25; cf cf_hem→cf_neck; neck cf_neck→sh_neck cubic (125,240),(100,262) sa 6; shoulder sh_neck→sh_tip; armhole sh_tip→underarm cubic (22,235),(18,172); side underarm→side_hem | n_arm armhole 0.5 V; n_side side 0.4 slit | grain (110,30)→(110,210); internal draw closed (40,60),(100,60),(100,110),(40,110),(40,60) label `pocket`; internal cut (138,190)→(138,205) label `buttonhole`; drill (70,140) d 3; soutache width 3 open (135,20),(120,60),(132,100),(118,140),(130,180) |
| back "Back bodice" | qty 1, on_fold cb, wool | cb_hem (0,0), side_hem (150,0), underarm (155,170), sh_tip (140,275), sh_neck (55,295), cb_neck (0,280) | hem cb_hem→side_hem sa 25; side; armhole underarm→sh_tip cubic (137,175),(128,235); shoulder sh_tip→sh_neck; neck sh_neck→cb_neck cubic (35,282),(15,280) sa 6; cb cb_neck→cb_hem | n_arm1 armhole 0.45 V; n_arm2 armhole 0.53 V; n_side side 0.6 slit | grain (40,30)→(40,230) |
| sleeve "Sleeve" | qty 2, wool | hem_l (20,0), hem_r (200,0), ua_r (220,180), cap_top (110,260), ua_l (0,180) | hem hem_l→hem_r sa 20; seam_r hem_r→ua_r; cap_b ua_r→cap_top cubic (190,190),(150,258); cap_f cap_top→ua_l cubic (65,260),(30,195); seam_l ua_l→hem_l | n_f cap_f 0.5 V; n_b1 cap_b 0.47 V; n_b2 cap_b 0.55 V; n_hem hem 0.5 T | grain (110,20)→(110,230) |

Seams, for s in R, L: `side_s` front.s/side ↔ back/side.s; `shoulder_s` front.s/shoulder ↔ back/shoulder.s;
`underarm_s` sleeve.s/seam_r ↔ sleeve.s/seam_l; `cap_f_s` sleeve.s/cap_f ↔ front.s/armhole; `cap_b_s` sleeve.s/cap_b ↔
back/armhole.s (all `reverse` default true).

**ROBUST** (`meta {size_label "M"}`, `fabric {}`): yoke (qty 1) a(0,0) b(200,0) c(200,60) d(120,60) e(120,120) f(0,120),
edges ab, bc, cd, de (reflex at d), ef `poly` with `c = [[t, 0.15·sin(πt)] for t in linspace(0.08, 0.92, 25)]`, fa,
notch n1 on ef s 0.3 type U; collar (qty 2, on_fold cb) cb0(0,0) tip(140,−10) tip2(145,40) cb1(0,45), edges outer
cb0→tip, point tip→tip2, neck tip2→cb1 cubic (100,50),(40,45), cb cb1→cb0, notches n_cb cb 0.5 V, n_sh neck 0.4 V;
stand "Interfacing" (qty 1, `sim: false`) 80×30 rectangle edges s0..s3. Seams: `collar_join` collar.R/outer.R ↔
collar.L/outer.L `reverse: false`; `yoke_collar` yoke/ef ↔ collar.R/neck.R+neck.L; `stand` stand/s0 ↔ yoke/ab.

**Single pieces:** rect 100×50 (a(0,0) b(100,0) c(100,50) d(0,50); edges bottom, right, top, left; optional on_fold
`left`); triangle a(0,0) b(200,0) c(200, 200·tan 20°); scoop a(0,0) b(100,0) c(100,80) d(60,80) e(40,80) f(0,80) with
de cubic (60,60),(40,60); NOTE = rect with `note`, pattern `{style NOTE, pieces [rect], seams []}`.

**Braided bolero:** the `zouave_bolero` drafting program run on the public reference wearer (stature 920 mm, age
30 months), with default style (soutache on) and with `{"soutache": false}`. The Rust exports suite should not depend
on the drafting port: capture both pattern JSONs from the Python reference once and commit them as fixtures.

### 10.3 Golden values captured from the reference (TESTBODICE, default options)

| Item | Value |
|---|---|
| Manifest pieces | front (front.R): sew 840.463 / 39711.18, cut 956.24 / 50745.2; back (back): sew 1153.701 / 83603.62, cut 1271.57 / 99881.45; sleeve (sleeve.R): sew 819.442 / 46006.69, cut 911.003 / 56537.28 (perimeter mm / area mm²) |
| Master | 375.502 × 654.737 mm, cut_bbox [10, 10, 365.502, 644.737], fold_mode half |
| Letter / A4 | 2 cols × 3 rows, 6 tiles, 0 blank, 7 pages each |
| Copyshop | A0 841 × 1189 (portrait) and 36x48in 914.4 × 1219.2, 1 sheet each, no info page |
| Projector | 1500 × 579.734 mm |
| DXF | 3 blocks each; INSERTs at (15.27, 65), (370.49, 65), (576.41, 60) |
| Flat DXF | 5 pieces: 7 CUT, 10 DRAW, 20 NOTCH, 15 GRAIN, 2 DRILL, 632 TEXT entities |
| GarmentCode | 5 panels, 18 stitches, 0 warnings; back has 16 edges |
| Cutter | front 176.0 × 325.9 (12x24), back 332.0 × 329.7 (too large, not crosswise), sleeve 241.5 × 293.0 (12x24) |
| DST | front.R and front.L (the front carries soutache): 271 records, 1 stop, extent 155.0 × 289.6, hoop 200x300 mm, centre (±72.5, 145.0), placement 837.81, couch 170.46, braid 168.4, 1 braid line |
| Checks | 146/146 without a pattern ref; 150/150 with it |

ROBUST: 125/125 checks; master 291.548 × 257.613; Letter 2×2 with A2, B2 blank (3 pages); A4 2×1 (3 pages); GC 3 panels,
27 stitches, 1 warning. Braided bolero (`size "920 mm stature"`): 183/183 checks with the pattern ref; DSTs back 554
records, 329.6 × 246.9, `over 240x360 mm: a commercial frame`, 3 braid lines; fronts 375 records, 160x260 mm, 2 lines;
sleeves 284 records, 240x360 mm, 1 line.

### 10.4 What the port must reproduce, and how exactly

| Level | Outputs |
|---|---|
| **Byte-exact** | DST record encoder and decoder (full 243 × 243 move table × 3 flags), END record, header layout for given counts/extents; README text; `ascii_text`, `safe_name`, `fnum`, column letters, page lists, `wrap`, `text_width`; stroke-font polylines; PDF writer skeleton for a fixed canvas (object order and numbering, dictionaries, xref offsets, trailer, with the date normalised); all file names; OCG names; layer names and colours; DXF style/piece text strings; info-page and band texts; CLI summary line formats; JSON formatting of manifest, cutter report and GarmentCode for equal values |
| **Exact structure** | SVG element tree, group ids/classes and attribute order; PDF object graph, page count, `FPTile` dictionaries, OC/tag nesting order in content streams, operator sequence; DXF sections, layers, blocks, per-layer entity counts per block, INSERT names; HPGL command sequence (pen changes, PU/PD/CI structure, ≤ 100 points per PD); GarmentCode panel names, edge counts, `edge_map`, stitch pairing, warnings; manifest keys and every integer/string field (cols, rows, tiles, blank names, pages, records, stops, braid_lines, hoop, mat, too_large); check names, counts and outcomes |
| **Numeric tolerance** | Cut outlines by the direct method: vertices within 0.002 mm of the reference after formatting (same flattening rule required); union-fallback outlines: Hausdorff ≤ 0.05 mm; manifest perimeters ±0.002 mm, areas ±0.05 mm²; label anchors and fitted sizes: algorithm-dependent (pole of inaccessibility) — require "inside the region, no obstacle overlap, size within 5 %" rather than equality; GarmentCode vertices ±1e−4 cm, params ±1e−5; DST decoded stitch positions ±1 unit (0.1 mm), record counts exact (investigate any difference); PDF/SVG coordinates ±0.002 mm |

### 10.5 Fixture capture plan

1. Pin the reference: Python 3.12, numpy ≥ 2, shapely 2.2 (GEOS), ezdxf 1.4.4; record the versions with the goldens.
2. Fixtures: TESTBODICE, ROBUST, the single pieces, NOTE, braided and plain bolero JSON, an UNSCALED copy, and option
   variants (`--unfold`; `--layout-width 300` with and without `--keep-blank-pages`; `--sa 15`; `--mitre-limit 2`;
   `--hpgl-rotate90`; `--hpgl-mirror`; `--projector-width 1100`).
3. Run `export all` per fixture into a fresh directory with `meta.created` set; run `check` with and without
   `--pattern-ref`; save the reports.
4. Normalise before comparing: PDF — blank the `/CreationDate` value, inflate streams, tokenise; DXF — parse group
   pairs, drop `$TDCREATE`, `$TDUPDATE`, GUIDs, the library XDATA, and the values of `Creation Date:` / `Creation Time:`;
   compare only header variables the exporter sets.
5. Store per-file **structural digests** (JSON: element/entity lists with rounded coordinates) as goldens, not raw bytes,
   except for the byte-exact items of §10.4.
6. Port tests: (a) unit goldens; (b) the 26 behavioural tests above; (c) Rust exporter on every fixture, Rust checker
   must report the same check names and counts (146/150/125/183) all passing; (d) **cross-checking**: run the Python
   checker on Rust outputs and the Rust checker on Python outputs — the strongest oracle, because each reads files
   independently; (e) digest comparison within the §10.4 tolerances; (f) mutation tests — shifted braid (+4 mm), injected
   `transform`, a dropped tile page, a block with two grain lines, a truncated DST — each must flip exactly the
   expected check.
7. External readers as extra oracles: ezdxf for both DXFs, pyembroidery for the DST (the reference's encoder is
   byte-identical to pyembroidery's for every tested move; pyembroidery reads couching stitches back within 0.064 mm of
   the drafted braid).

---

## 11. Exclusions / not to port, and quirks to decide

### 11.1 Not to port

- The DXF library's boilerplate as such: the port writes its own R12/R2000 files. Emit a valid R12 file with HEADER
  (the variables of §7.7), TABLES (at least LTYPE with CONTINUOUS, LAYER, STYLE STANDARD) and the BLOCKS/ENTITIES
  described; copying the reference's full template (VPORT, VIEW, UCS, APPID, DIMSTYLE, ~130 header variables) is
  optional and not checked.
- Library metadata (XDATA `EZDXF`, `$TDCREATE`/`$TDUPDATE`, random GUIDs), logger-level suppression.
- Python-only API shape (`return_method` flags, `export_all` return dict, module-path setup), the browser (Pyodide) glue
  and the round driver's subprocess call — only the CLI text contract (§1.3) matters to callers.
- Not implemented in the reference (do not invent): grading and `.RUL`, several sizes per file (the OCG structure is
  ready for `Size <label>` layers), marker/nesting and yardage, tech pack, print-and-cut PNG (150 px/in), PES/JEF/EXP
  embroidery, the SA corner jog, cutting-layout layers in the projector file, auto-splitting oversized cutter pieces.

### 11.2 Quirks (port faithfully unless the project decides otherwise)

| # | Behaviour | Recommendation |
|---|---|---|
| Q1 | `$EXTMIN`/`$EXTMAX` are computed (ASTM/AAMA: row extents; flat: cut extents) but the library writes its sentinels `1e+20` / `−1e+20` on save | write the computed values (no check depends on them) |
| Q2 | PDF and DXF dates use the wall clock, not `created` | use `created` (§9) |
| Q3 | Copyshop (k = 2) scales line widths and dashes of the sew line but not the dash-dot fold pattern, the internal-line dashes or label sizes (`text_scale` stays 1) | keep for parity |
| Q4 | Label placement uses the distance to the part's exterior ring only, so the initial size bound can ignore a nearby obstacle hole (the box test does respect holes) | keep; assert "no obstacle overlap" in tests |
| Q5 | Decor kinds other than `soutache`/`embroidery` are drawn in PDFs/DXFs/HPGL but not in the master SVG | keep |
| Q6 | `svg.no_transform` is a plain substring test (a piece named "transform" fails it) | keep |
| Q7 | `check_dir` raises when a listed file is missing; the manifest merges with an existing one, so stale `files` entries from earlier runs are checked again | keep the merge; consider reporting missing files as failures |
| Q8 | Structure errors are warnings in the CLI; a pattern whose instances cannot be built then fails later | keep the warning text |
| Q9 | Drills on the fold line are de-duplicated after unfolding; internal lines and decor are not | keep |
| Q10 | Notches on a fold edge show in the half view only | keep |
| Q11 | Cutter and DST file names are not de-duplicated: piece `front_R` (qty 1) and instance `front.R` both map to `front_R` | detect and fail loudly in the port |
| Q12 | Copyshop orientation ties keep the paper as listed (portrait) | keep |
| Q13 | HPGL and DST rounding is ties-to-even | keep (byte parity) |
| Q14 | The projector's 200 in error escapes the CLI's error handling (traceback) | report it like the UNSCALED refusal (exit 2) |
