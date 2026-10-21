# Source-qualified presser-foot stack planning

`ftx-materials sew-plan`, `sew-stitches` and `sew-svg` can evaluate the actual needle stack at a
**prescribed pressure** using the same `ftx_pattern::material_compression::condition` authority as
material conditioning. This is `ftx-sewing-plan/7`, not a separate fabric-compression model or a
machine-control interface. The original pattern, material selections and stitch coordinates stay fixed.

## Runnable synthetic example

```sh
cargo run -p ftx-cli --bin ftx-materials -- sew-plan \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  tests/fixtures/materials/synthetic-sewing-pressure-options.json

cargo run -p ftx-cli --bin ftx-materials -- sew-stitches \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  tests/fixtures/materials/synthetic-sewing-pressure-options.json

cargo run -p ftx-cli --bin ftx-materials -- sew-svg \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  tests/fixtures/materials/synthetic-sewing-pressure-options.json \
  side pressure-review.svg
```

Use a new SVG path; existing files are never overwritten. Everything in these fixtures is **invented
for software verification**, including the source qualification. None of the numerical settings is a
recommendation for a material, machine, needle or person.

The example retains two original 0.4-mm plies: its **uncompressed stack is 0.8 mm**. At the declared
500 Pa, the synthetic observations give each ply 0.2 mm, bounded by [0.18, 0.22] mm. The output therefore
reports **0.4 mm nominal loaded thickness** and **0.44 mm checked loaded thickness**. The latter uses
both reported upper bounds and passes the separate 0.5-mm loaded limit. There are still 40 stitch
intervals and four seconds of continuous feed; no pressure-settling or handling time is invented.
The preview displays both loaded values, pressure and the original stack, retaining acquisition
identities in escaped XML metadata.

## Input contract and one material authority

Add `machine.presser_foot` with `pressure_pa: [minimum, maximum]` in Pa and positive
`max_loaded_stack_mm`. The original `machine.max_stack_mm` remains the **uncompressed entry limit**.
It is checked first and cannot be bypassed by claiming the material will compress later.

For each affected pass, supply `presser_foot` with:

- `model: "independent_ply_equal_pressure/1"`, explicitly selecting independent-ply additivity;
- positive `pressure_pa`, exact `conditioning` and exact `loading_branch`;
- `materials`, a map from each original material's resolved SHA-256 to its complete existing
  `ftx-material-compression/1` request.

The map needs exactly the distinct materials in that operation's actual stack, including interlining
or a repeatedly folded source sheet. Extra or missing entries refuse. Each compression request's
starting material must resolve to its map key under the **original selection's evidence policy**.
Sources must therefore already belong to that source-complete original record. Updating the record
or its sources requires re-resolving the pattern and reviewing the corresponding evidence; do not
rewrite a stale hash merely to suppress a refusal.

Every profile must select the operation's exact pressure, conditioning and loading branch. The existing
compression authority checks source metadata, units, positive thickness, nonincreasing loft, supported
pressure domain and explicit interpolation permission. A material family, apparent softness or yarn
identity never supplies a missing pressure response. Acquisition permission cannot grant permission to
consume an estimated loaded thickness when the original sheet selection disallows estimates.

Complete flat-fold assemblies use their **actual ordered material preimages** at the needle, including
repeated regions. Legacy ply-count jobs use declared A plies followed by B plies; that bookkeeping order
is not proof of a physical folded assembly. Every ply receives the same prescribed pressure in this
explicit approximation. There is no inferred foot force, contact area, pressure field or interply law.

## Outputs and process qualification

`seams[i].stack_mm`, `sides`, the original assembly and the plan's material inventory remain unchanged.
`seams[i].presser_foot` separately contains central and checked loaded stack totals, a row for every ply,
its original and conditioned material hashes, loaded thickness, bound-use flag and cumulative loaded
z bounds. Compression receipts are retained once per distinct original material, including complete
observations and interpolation brackets. Derived loaded records are **not** rebound to the pattern.
Their discarded constitutive/thermal properties are not used to invent other pressure-dependent effects.

When a ply has source bounds, checking uses its upper bound; otherwise it uses the central observation
and leaves that absence explicit. Summing the upper bounds does not give them a new confidence level.
The z bounds start at zero for stack review, not at a calibrated bed or needle coordinate.

Existing source-qualified recipes need an additional `presser_foot` envelope with `pressure_pa`,
`loaded_stack_mm`, `conditioning` and `loading_branch`. Both the original **uncompressed** recipe range
and this **loaded** range must pass. A recipe without a foot-pressure envelope does not qualify a
pressure-bearing operation, nor does a pressure-only recipe silently qualify an operation without that
state. `require_qualification` and estimate permissions retain their existing meanings.

The planner can compose pressure with reverse/backtacks, offsets, explicit assemblies and setup-change
ordering. Expanded programs retain the complete version-7 plan rather than downgrading it to a legacy
schema. Their geometric program version still reflects actual centerline/offset/assembly capabilities.
Jobs without pressure fields retain the previous serialized formats and calculations.

The existing file-free browser functions `sew_plan(patternJson, optionsJson)`,
`sew_stitches(patternJson, optionsJson)` and `sew_svg(patternJson, optionsJson, operation)` use the same
implementation after rebuilding the bindings. No separate JavaScript pressure calculation is needed.

## Clearance-controlled closure: infer the required pressure and force

An operation may instead prescribe a **final gap** between rigid parallel faces and a common reference
area. `ftx-sewing-plan/8` then uses the existing `material_compression::stack::close` inverse on every
actual physical ply, including repeated folded regions. Pressure is an output, never a guessed input.
The original pattern, material inventory, unloaded-entry test and generated stitches remain unchanged.

```sh
cargo run -p ftx-cli --bin ftx-materials -- sew-plan \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  tests/fixtures/materials/synthetic-sewing-clearance-options.json

cargo run -p ftx-cli --bin ftx-materials -- sew-svg \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  tests/fixtures/materials/synthetic-sewing-clearance-options.json \
  side clearance-review.svg
```

This separate **synthetic** fixture starts two 0.4-mm plies at a known 0 Pa and requests a 0.6-mm final
gap over 200 mm². Its independently declared linear branches give **250 Pa**, **0.05 N** final normal
force and **0.000005 J** signed external loading work. The central loaded stack is 0.6 mm; the checked
stack remains 0.66 mm from the supplied upper bounds. A material-only change in compressibility can
require more pressure and force for the same gap, changing machine admission without changing stitches.
None of these numbers is a measured textile result or recommended machine setting.

Select `model: "independent_ply_clearance/1"`, omit the operation's `pressure_pa`, and supply:

```json
"clearance": {
  "gap_mm": 0.6,
  "reference_area_mm2": 200.0,
  "direction": "loading",
  "allow_estimates": true
}
```

The per-material compression requests now use `pressure_pa` for the **known initial pressure**; it must
be identical across all plies. Acquisition `conditioning` and `loading_branch` still must match the
operation. `direction` restricts the traverse of those supplied branches; it does not select or invent
an unloading curve from its name. A characterized unloading traverse can give negative external work.
The shared inverse refuses nonunique pressure, unsupported gaps, extrapolation and invalid evidence.
Supplying both a prescribed operation pressure and clearance is an error.

The machine must additionally declare a positive ordered `presser_foot.normal_force_n` interval.
Both the inferred final pressure and final force must lie inside the machine's separate envelopes.
By default these are nominal-value checks. The opt-in reported-bound policy below instead checks
the complete available pressure/force enclosure against both machine and process envelopes.
This force limit applies to clearance mode; prescribed-pressure mode has no reference area and does not
claim a computed force. The final **checked** loaded thickness must still pass `max_loaded_stack_mm`;
neither a small central gap nor estimated compression bypasses unloaded entry or reported upper bounds.

A required process qualification needs a matching `presser_foot.clearance` object with `gap_mm`,
`reference_area_mm2` and `normal_force_n` intervals and the exact `direction`. Its ordinary pressure,
loaded thickness, conditioning, loading branch and uncompressed stack limits also apply. A pressure-only
recipe does not qualify clearance control, nor does a clearance qualification silently qualify a
pressure-controlled operation. Model-estimate permission is independent of all material and process
evidence policies.

`seams[i].presser_foot.closure` retains the full shared inverse receipt, including input gap/area,
initial/final pressures, pressure bracket, normal force, signed external branch work, numerical closure
residual and every physical ply's conditioning result. In this mode `presser_foot.materials` is empty:
the per-ply results live in `closure.layers`, and `presser_foot.layers` links each original material hash
to its final conditioned hash. Repeated plies are distinct closure participants. SVG metadata carries
the inverse/input and per-ply acquisition hashes; the visible review distinguishes inferred pressure
from prescribed pressure and explicitly does not identify external work as heat.

The inverse supports at most **32 actual plies**, and repeated plies count repeatedly toward the shared
65,536-observation job limit. The same ordinary commands and file-free browser adapters use this path;
they return no partial output on a force, source, geometry or qualification refusal. There is no
separate browser calculation or new hardware command.

**Central observations determine the nominal inverse.** Complete thickness bounds additionally give
the conditional pressure/force enclosure described below; work remains nominal. The checked thickness
is still evaluated at nominal pressure and may exceed the requested central gap; it is retained for
the loaded-stack envelope, not claimed as a second compatible physical configuration. Final force/pressure ranges are final-state checks,
not limits for every point of the traverse. The area is supplied, not inferred from a foot shape or
needle path. These fixed-area, independent-ply, quasi-static assumptions do not model local pressure
fields, lateral expansion, buckling, dwell, rate-dependent recovery or branch switching. Signed
external work is neither a stored-energy nor a dissipation/heat split. No contact solution, uncertainty
qualification, seam-quality validation or machine authorization follows from successful closure.

### Require the reported pressure/force range to fit

Add `"pressure_force_basis": "reported_bounds"` inside an operation's `presser_foot.clearance`.
The omitted/default value is `"nominal"`, preserving existing central-value admission. The stricter
policy changes **only pressure and force limit checks**; it does not change stitches, model coefficients,
source selections, central work or loaded-thickness arithmetic. Prescribed-pressure mode has no
inverse or area, so it cannot select this clearance-only policy.

The shared [stack inverse](compression-stacks.md#reported-thickness-bounds-through-the-inverse)
constructs monotone thickness envelopes using all source knots and all physical plies, including
repeated folds. Every acquired thickness needs reported bounds. Both ends of the resulting pressure
range must fit `machine.presser_foot.pressure_pa`, and both force endpoints must fit
`machine.presser_foot.normal_force_n`. A required process qualification must independently cover both
pressure endpoints and both `presser_foot.clearance.normal_force_n` endpoints. Existing arithmetic
tolerance is unchanged; no extra machine or process margin is introduced.

Missing bounds, ranges requiring extrapolation or travel against the declared direction, and
unrepresentable interval arithmetic **refuse the operation**. There is no nominal fallback and no
automatic widening of a process qualification. The receipt records `pressure_force_basis` plus
`closure.reported_bounds`; the SVG shows the checked ranges and their conditional meaning. Recipe
selection retains the same policy in every trial and in the replayable selected options. It can
choose an independently qualified recipe covering the range, but cannot bypass a machine envelope.

For the synthetic two-ply clearance fixture, the nominal **250 Pa / 0.05 N** result has conditional
bounds of **166.6666667–318.1818182 Pa / 0.0333333333–0.0636363636 N**. Its existing synthetic process
window of 200–300 Pa / 0.04–0.06 N therefore passes nominally but **fails** with reported-bound
admission. These bounds are neither measurements nor confidence intervals. Shared source plies do
not become independent samples that shrink the range.

The following exercises a **separate, invented wider qualification**, not an instruction to change
actual source evidence or machine limits. Run from the repository root:

```sh
bounds_demo=$(mktemp -d)
python3 - "$bounds_demo/options.json" <<'PY'
import json
import sys
from pathlib import Path
o = json.loads(Path("tests/fixtures/materials/synthetic-sewing-clearance-options.json").read_text())
o["passes"][0]["presser_foot"]["clearance"]["pressure_force_basis"] = "reported_bounds"
q = o["qualifications"][0]
q["id"] = "synthetic-wider-range-for-software-check"
q["method"] = "Invented wider process window, not measured qualification"
q["presser_foot"]["pressure_pa"] = [100, 400]
q["presser_foot"]["clearance"]["normal_force_n"] = [0.02, 0.08]
with Path(sys.argv[1]).open("x") as output:
    json.dump(o, output, indent=2)
PY
cargo run --locked -p ftx-cli --bin ftx-materials -- sew-plan \
  tests/fixtures/materials/synthetic-sewing-pattern.json "$bounds_demo/options.json"
cargo run --locked -p ftx-cli --bin ftx-materials -- sew-svg \
  tests/fixtures/materials/synthetic-sewing-pattern.json "$bounds_demo/options.json" \
  side "$bounds_demo/review.svg"
```

The enclosure assumes exact gap, area, pressure knots and preload plus the qualified monotone
piecewise-linear law. It does not propagate those inputs' uncertainty, systematic calibration error,
work/heat uncertainty, or spatial contact pressure. Passing it is not hardware authorization or a
physical seam-quality result. It adds a conditional range check, not a full uncertainty model.

## Limits and remaining acceptance

This is static, fixed-reference-area, independent-ply **prescribed-pressure or clearance planning**. It does not solve
presser-foot contact, predict local force distributions, thickness recovery or speed/dwell effects,
lateral bulging, needle penetration, frictional feeding, thread tension, pucker or seam strength. The
original uncompressed observation must itself be appropriate for the intended entry condition. A
compression table alone does not qualify a sewing process; process evidence remains independent.

Work is bounded across the whole job: at most 128 physical plies per prescribed-pressure stack (32 for
clearance closure) and 65,536 supplied scalar observations, in addition to existing sewing/geometry budgets.
Failures publish no partial program or SVG. `hardware_execution_authorized` and
`physical_seam_quality_validated` remain **false**, including for source-matched recipes.

The `ftx-sp2` / plan §25 sewing gate remains open for real matched stack/process coupons, foot-pressure
measurement, dynamic and interacting-ply validation, machine registration, workholding and interlocks.
Synthetic library/command tests establish arithmetic, material propagation and refusal behavior—not
empirical material, seam or machine qualification.
