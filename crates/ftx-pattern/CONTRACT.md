# ftx-pattern contract

**Purpose.** The `fabric-pattern/1` document and what every later stage reads from it: instances (mirror
pairs, on-fold unfolding), the structure checks with the reference's messages, seam sides, notch fractions
and notch-piecewise seam sampling (`docs/spec/01` §2, §6–8). Validation, alteration and drafting build on
it (FTX-04, FTX-05).

**Units and frames.** Millimetres, 2D, x right, y up, loops counter-clockwise; every piece is the
wearer's right side seen from its public side. `.L` instances are mirrored about x = 0.

**Invariants.**
- `Pattern::new` never panics: problems land in `build_errors`, and when there are any no instances are
  built (no partial geometry of an ambiguous pattern).
- Structure checks run on the raw JSON, so malformed input gets the reference's messages; the typed
  `PatternDoc` exists only for documents that type cleanly.
- Reading and writing a document is lossless up to key order and number spelling (semantic equality,
  spec 01 §14.6). `Pattern::from_json` keeps vertex order; a `serde_json::Value` input has sorted keys.
- Seams stay JSON objects read with Python semantics (`reverse: null` is false; an absent `ratio_band`
  differs from a `null` one).

**Oracle and fixtures** (`tests/fixtures/oracle/pattern/`, compared in `ftx-conformance`):
`instances` (F7–F10, curved pairs), `structure` (24 F11 cases, exact messages; check-1 messages as a
multiset because the reference iterates a set), `seams` (F12–F21, F26, F27: sides, notch fractions, seam
pairs at 5 and 7 mm, tangents, corner angles), `drafts` (bolero, trousers and fez for public reference
wearers of 80, 92 and 116 cm: every instance, side and seam pair, and the round trip). 96–99 % of the
numbers are bit-exact, the worst 5.7e-14 relative.

**Deviations from the reference** (each on input the reference crashes on or silently accepts; spec 01
§15): a piece without an id is `piece {i}: missing id`; `cut: null` reads as absent; `cut.qty` must be the
integer 1 or 2 (the reference truncates 2.7 to 2); a Bézier with the wrong number of control points is a
definition error; a seam ref without `/` has its own message; a zero-length seam side with notches reports
the implausible length instead of dividing by zero; an edge's `"curve": null` is written back as absent.

**Error model.** Structure checks return messages. Lookups return `PyError` (`Key` prints quoted, like
Python's `str(KeyError)`; `Value` prints as is).

**Determinism class.** Fixed-order f64 arithmetic over `ftx-core`; bit-identical across targets.
