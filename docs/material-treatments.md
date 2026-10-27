# Material treatment, pattern shrinkage and cutting compensation

Treatment is an explicit transition between two frozen material states. Its directional dimensional
changes alter an ordinary `fabric-pattern/1` pattern, so the existing fit, drape, mass, thermal, cutting
and sewing consumers see the resulting geometry and material through their existing interfaces.
Each consumer still requires its own evidence. A successful shrinkage calculation does not supply
missing mechanics, friction, stock dimensions or seam qualifications.

## Run the measured denim example

The example combines a synthetic 100 × 200 mm panel with published five-cycle denim observations.
The source adapter supplies genuine material content locks. Run from the repository root and choose
output names that do not already exist:

```sh
cargo run -p ftx-pattern --example laundered_denim_treatment -- 1 pattern > panel.json
cargo run -p ftx-pattern --example laundered_denim_treatment -- 1 compensate > compensate.json
cargo run -p ftx-pattern --example laundered_denim_treatment -- 1 treat > treat.json

cargo run -p ftx-cli --bin ftx -- --json materials treatment-pattern \
  panel.json compensate.json compensated-panel.json
cargo run -p ftx-cli --bin ftx -- --json materials treatment-pattern \
  compensated-panel.json treat.json finished-panel.json
```

The compensated panel is larger along both material axes and keeps the **before** material for
cutting. Applying the documented treatment then recovers the intended panel dimensions and binds
the **after** material. Applying `treat.json` directly to the uncompensated panel instead produces a
94.5 × 190.34 mm panel: the grain is vertical, so the observed 4.83% warp and 5.50% weft contractions
act on height and width respectively. These are free dimensional changes, not a prediction of an
assembled garment's constrained washing behavior.

For the standalone material report:

```sh
cargo run -p ftx-cli --bin ftx -- --json materials treatment \
  tests/fixtures/materials/kanat-ozdil-2026-denim-1-treatment.json
```

The companion `ftx-materials treatment` and `ftx-materials treatment-pattern` commands accept the
same arguments and emit the shared report directly. The primary command puts it in the ordinary
JSON result envelope. The optional final pattern path is created exclusively; an existing file,
including the source pattern, is never replaced. Without that argument, the exact output remains
available in `pattern_json` in the report.

## Author a transition and assign it to pieces

`ftx-material-treatment/1` contains the following explicit inputs:

| Field | Meaning |
|---|---|
| `before`, `after` | Ordinary embedded material selections, each with its exact `expected_sha256` |
| `specimen` | Source-qualified identity of the specimen or published fabric group |
| `process`, `cycles` | Exact treatment and positive number of cycles represented by the observation |
| `conditioning_before`, `conditioning_after` | The conditioning attached to each state |
| `shrinkage_warp`, `shrinkage_weft` | Source-bearing quantities in `%` or dimensionless `1` |
| `sources` | The sources referenced by those dimensional-change quantities |
| `allow_estimates` | Explicit consent for estimated dimensional observations, independent of material consent |

Positive shrinkage means contraction; negative shrinkage means expansion. Five-cycle observations
describe one five-cycle transition. They do not define a per-cycle law or justify extrapolation to
another process. Every dimensional observation must carry the exact specimen, process, cycles and
conditioning. Every property or response in either state must qualify the same specimen and that
state's conditioning; a valid hash alone cannot substitute an unrelated before material. Post-state
evidence is independently supplied. The after record names `treatment_process` and `treatment_cycles`
in its structure. Frozen selections cannot pull mutable catalog records or layer property overlays
over this evidence boundary. See the checked fixtures for the complete ordinary quantity format.

The geometry wrapper is `ftx-pattern-treatment/1`. Its `transitions` object maps local names to
complete material-treatment requests; `pieces` maps source-piece IDs to those names. For example:

```json
{
  "schema": "ftx-pattern-treatment/1",
  "mode": "compensate",
  "transitions": {"wash": "REPLACE WITH THE COMPLETE TRANSITION OBJECT"},
  "pieces": {"front": "wash", "back": "wash"},
  "seam_tolerance_mm": 3.0
}
```

The placeholder is illustrative, not a valid transition. The runnable Rust example emits the
complete valid object. Every named piece must already resolve to the exact before-state hash.
Pieces omitted from the mapping keep their geometry and material. Separate transitions allow
shells, linings and other layers to retain their own measured responses.

## Geometry and downstream behavior

The transform acts in each piece's declared grain frame, about its placement anchor. It updates
vertices, absolute curve controls, grain, material marks and drill centers, while remapping notch
fractions to preserve their physical positions on curved edges. Normal pattern instancing then
handles mirrored pieces. An anisotropic half-pattern requires a fold parallel or perpendicular to
the grain; an oblique fold requires a full explicit piece. Degenerate grain, non-finite dimensions,
incompatible simulation scales and stale source locks refuse with structured errors.

`treat` binds the after state; `compensate` keeps the before state. Repeating compensation on the
same piece refuses even if unrelated metadata changed. Seam-allowance and tool widths remain
authored manufacturing choices. The report retains independent seam-walk results before and after
the transformation, so differential shrinkage is visible rather than silently repaired. Existing
material-pair contact observations retain their exact locks and refuse when stale; the request can
explicitly replace `interfaces` with properly qualified pairs.

The output freezes all effective piece assignments and retains source, request and output JSON,
their hashes, transition reports and treatment history. This is the ordinary rest geometry for a
subsequent simulation; no hidden solver multiplier or special treatment material is installed.
Mass comes from transformed retained area and the measured post-state areal density. Thermal
consumers use the corresponding post-state resistance or law. Neither mass conservation nor
unchanged mechanics is inferred from a change in dimensions.

## Compare and optimize the design after treatment

Treatment is also an ordinary design operation:

```json
{"op":"treatment","request":"REPLACE WITH THE COMPLETE ftx-pattern-treatment/1 OBJECT"}
```

Use it in `designs[].operations`, `design_grid.after`, or `design_adaptive.after`. Each operation
uses the same standalone authority, then the normal flat geometry, seam, outline and selected-body
fit gates inspect the resulting dimensions. A grid or adaptive search can therefore vary the
pre-treatment pattern and admit candidates against **post-treatment fit targets**. Put any required
seam corrections in the explicit operation sequence. A mismatched final seam or failed fit stays a
refused design and cannot enter downstream manufacturing or scenario comparison.

The existing source adapter emits a complete runnable study, without placeholders:

```sh
cargo run -p ftx-pattern --example laundered_denim_treatment -- 1 pattern > panel.json
cargo run -p ftx-pattern --example laundered_denim_treatment -- 1 study > treatment-study.json
cargo run -p ftx-cli --bin ftx -- --json materials study panel.json treatment-study.json
```

It compares the unchanged panel, washed panel, cutting compensation, and compensation followed by
washing. Retained mass uses each candidate's actual area and correct before/after areal density.
The measured example does not request unsupported cutting stock, mechanics or seam qualification.
The companion `ftx-materials study` accepts the same two files. Existing scenario studies use these
same final ordinary patterns to rebuild their drape and thermal consumers whenever the selected
material supplies the necessary observations.

Completed stages retain `design.treatments[]`, each containing `operation_index` and the full
canonical treatment `report`. Its exact `source_pattern_json`, `pattern_json`, and
`frozen_request_json` replay every intermediate step. A later refused operation can retain earlier
completed stages for inspection without admitting the final design. Final manufacturing replay
still uses `design.altered_pattern_json` plus the row's `frozen_changes`.

Material variants are bound after the design sequence. The final effective material of each treated
piece must match the last treatment's promised state: AFTER for treatment, BEFORE for compensation.
Replacing an unrelated lining remains allowed; silently attaching an untreated material to shrunken
geometry refuses before consumers run. A zero-dimensional transition can change mass or other
material properties without claiming that the geometry changed. Fixed-geometry
`ftx-material-study/1` requests still reject design operations. All screened and adaptive attempts
reserve geometry and retained-document work, including repeated embedded material records.

## Browser workflow

In `web/materials.html`, open **Laundering and dimensional treatment**, choose a material or pattern
treatment request and run it. Material-only requests need no pattern. Pattern requests use the
current frozen binding when one exists, otherwise the current source pattern. The page shows
directional remaining lengths, area ratio, unavailable post-state properties and seam results.

Download the exact replay inputs and derived pattern. **Use derived pattern** adopts its retained
JSON as the current source; freeze/bind it to recompute the ordinary consumers. Replacing a request,
source pattern, assignment or material-pair evidence withdraws obsolete results, including late
worker replies. The worker uses the shared Rust exports `material_treatment` and
`material_treatment_pattern`; an older bundle without them asks for a rebuild instead of inventing
a browser implementation.

Manufacturing and scenario studies also display treatment stages for ordinary alternatives and
screened/adaptive trials. Download links use the exact retained stage strings, including completed
stages of a later-refused design. The final geometry/fit/binding result determines admission;
merely displaying a stage never marks that candidate selected or physically validated.

## Evidence and remaining physical scope

The built-in five denim pairs document construction, directional dimensional change, areal mass
and steady thermal resistance. [The source notes](laundered-denim-materials.md) explain the study,
protocol, reported deviations and missing properties. A source's unqualified gauge thickness stays
`reported_thickness`; it cannot satisfy contact or sewing-stack clearance. These records remain
unready for a fully specified mechanical drape, transient storage, stock-width purchasing and
qualified machine sewing where their required measurements are absent.

The software tests cover anisotropy, curves and physical notches, mirrors/folds, compensation
round trips, differential seams, source substitution, native and portable replay, output-file
preservation, and propagation into ordinary mass and heat consumers. Free coupons do not establish
the effects of seam restraint, bonding, inter-layer shear, process-dependent contact or handling
during a wash. Matched treated mechanical specimens and held-out sewn garments remain the next
physical acceptance work under `ftx-care1`, `ftx-tm2` and `ftx-tm8`.
