# Planar tool and fixture clearance during sewing

The optional `passes[].access` policy checks whether the declared circular tool envelope can traverse
an operation's complete needle chords without approaching fixed fixtures too closely. It uses the
ordinary sewing plan's actual registered/folded assembly and source material qualification. A plan
with this capability has schema `ftx-sewing-plan/14`; omission preserves the existing behavior.

This closes a geometric planning gap: a needle centerline can clear a clamp while the surrounding
presser foot or tool body intersects it. The policy supplies that envelope explicitly; needle labels,
material thickness, compression reference area and fixture counts do not determine its radius.

## Authoring a policy

Supply `access` on an operation that already has an explicit `assembly`. Its fields are:

| Field | Meaning |
|---|---|
| `model` | Exactly `planar_disk_sweep/1` |
| `machine_sha256` | Full typed machine/configuration hash |
| `qualifications` | Allowed process records as `{id, sha256}`; the actual admitted record must match |
| `tool_radius_mm` | Positive circular envelope radius about the needle |
| `minimum_clearance_mm` | Nonnegative required separation beyond that radius |
| `fixtures` | Unique `{id, outline_mm}` filled simple polygons in the assembly frame |
| `source`, `method`, `conditions` | Acquisition and applicability of the geometry policy |
| `evidence`, `allow_estimates` | Independent evidence class and explicit estimate admission |

Coordinates are millimetres in `operation_local_mm`, the same frame used by `FlatAssembly` placements
and folds. The polygon closing edge is implicit; either winding and concave simple polygons are
supported. Holes, repeated closing vertices, zero edges, self-intersections and degenerate polygons
refuse. Every fixture polygon is checked, including geometry outside the source cloth.

Use the existing Rust functions
`ftx_export::sewing::production::workholding::machine_sha256` and `qualification_sha256` to construct
content locks from the typed options. A reused ID cannot hide changes to machine limits, the ordered
material stack, process envelope, compression policy or source evidence. Multiple exact qualification
references can admit explicitly supported material/recipe alternatives. Independent geometry estimate
consent does not relax the ordinary material or seam qualification.

For example, with a canonical needle path at x=20 mm, a 2 mm tool radius and a clamp whose nearest
edge is x=25 mm, the computed tool clearance is 3 mm. A requested margin of 3 mm is admitted exactly;
a larger margin refuses. Moving that edge to x=22 mm causes tool contact and refuses even when the
requested margin is zero. These are synthetic geometric examples, not measurements of a machine.

The complete [access options](../examples/sewing-access-options.json) use the existing
[synthetic sewing pattern](../tests/fixtures/materials/synthetic-sewing-pattern.json). They combine
three production copies, two finite holding jigs and two registered sewing operations whose declared
tool clearance is 3 mm. Run both ordinary consumers from the repository root:

```bash
cargo run -p ftx-cli --bin ftx -- --json materials sew-plan \
  tests/fixtures/materials/synthetic-sewing-pattern.json examples/sewing-access-options.json
cargo run -p ftx-cli --bin ftx -- --json materials sew-stitches \
  tests/fixtures/materials/synthetic-sewing-pattern.json examples/sewing-access-options.json
```

The same files can be loaded into the materials browser's pattern and sewing-options inputs. Direct
sewing, manufacturing studies and scenario studies display per-operation radius, required and minimum
gap, checked chord counts and limiting witnesses, with exact source policy and assembly details. A
missing retained receipt stays unknown. Changing source/options withdraws stale output and downloads.

## Complete-path admission and replay

The check computes the minimum distance between each complete canonical needle segment and every
fixture edge, and checks filled-polygon containment and intersections. It therefore detects a clamp
between two otherwise clear stitch stations. It does not approximate a continuous chord by endpoint
samples. The full seam traversal covers every canonical chord; reverse-feed and start/end backtacks
revisit the same sweep, preserving the same clearance result.

For each admitted seam, `access` retains:

- The actual admitted machine and process-qualification locks.
- Tool radius, requested margin, minimum centerline distance and minimum tool clearance.
- The number of unique chords and fixtures checked.
- The limiting fixture, canonical chord indices, fixture-edge indices and nearest point on each.
- `physical_access_validated: false`.

Equal minima retain the first witness in canonical chord, authored fixture and polygon-edge order.
The complete source policy remains in `seam.pass.access` and the plan's ordinary options. Rebuild
with those options and the frozen pattern to replay; a saved or edited receipt is never trusted as
input. Altered pattern geometry produces a fresh assembly path and clearance calculation. A material
change must first pass the ordinary process qualification and then the policy's exact content locks.

The same check runs in `sew-plan`, `sew-stitches`, qualified recipe selection and manufacturing/material
studies because they use `SewPlan::build`. Production copies reuse the admitted operation geometry;
actual expanded stitch output still follows the selected schedule. Refusal cannot be bypassed by
choosing a different client or requesting only the stitch program.

## Bounds and physical scope

An operation supports 1–64 fixtures, at most 1024 polygon vertices in total and 1–256 allowed
qualification references. Coordinates, radius and margin are bounded by one million millimetres.
All operations share a 50000000-unit geometry budget, charged for polygon validation and complete
chord/edge work before evaluation. Oversized requests refuse instead of checking a prefix.

This is a static planar exclusion model. A polygon represents the entire filled forbidden region
regardless of height; no vertical bypass is inferred. The circular footprint has no orientation
degree of freedom. The calculation supplies no contact-pressure distribution, grip force, cloth
support, tool acceleration, fixture loading/unloading, approach or retraction motion, inter-operation
trajectory, calibrated machine coordinates or collision-free folding sequence. It checks the emitted
piecewise-linear needle chords, not an independently inferred continuous machine interpolation.

Fixture inventory remains the separate production `workholding` authority. These geometric policies
are local to their declared operations and reused for each copy; neither policy infers that physical
fixtures remain in the same pose between operations or that interchangeable units share measured
geometry. `hardware_execution_authorized` and physical seam-quality claims remain false.

Synthetic regressions cover analytic distance/witnesses, tool-only collisions, intersections between
stations, containment, exact margins, concavity and winding, folded/translated assemblies, altered
geometry, reverse/backtack and production reuse, exact source invalidation, independent estimate
admission, bounded aggregate work and omitted-policy compatibility. Empirical fixture, tool and
process measurements and real machine qualification remain under `ftx-pb2.2` and `ftx-sp2`.
