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

## Limits and remaining acceptance

This is static, fixed-reference-area, independent-ply **prescribed-pressure planning**. It does not solve
presser-foot contact, predict local force distributions, thickness recovery or speed/dwell effects,
lateral bulging, needle penetration, frictional feeding, thread tension, pucker or seam strength. The
original uncompressed observation must itself be appropriate for the intended entry condition. A
compression table alone does not qualify a sewing process; process evidence remains independent.

Work is bounded across the whole job: at most 128 physical plies per pressure stack and 65,536 supplied
scalar compression observations across all operations, in addition to existing sewing/geometry budgets.
Failures publish no partial program or SVG. `hardware_execution_authorized` and
`physical_seam_quality_validated` remain **false**, including for source-matched recipes.

The `ftx-sp2` / plan §25 sewing gate remains open for real matched stack/process coupons, foot-pressure
measurement, dynamic and interacting-ply validation, machine registration, workholding and interlocks.
Synthetic library/command tests establish arithmetic, material propagation and refusal behavior—not
empirical material, seam or machine qualification.
