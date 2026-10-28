# Source-qualified zero-storage bridges in moving coverage

Moving garment coverage can include an explicitly characterized seam or closure's **instantaneous
excess heat exchange** while retaining the same material temperature and enthalpy solution. Author
the optional policy at `scenario.motion.transient_coverage.thermal_bridges`. Native scenario studies,
the portable selected-body adapter and the browser use the same producer and comparison admission.

The feature does not infer thermal behavior from seam labels, sewing recipes or a generic textile
family. It requires both the existing exact unbridged reference observation and a separate source's
qualification of the zero-storage approximation. No built-in material is silently upgraded to have
measured transient seam behavior.

## Physical and numerical meaning

For each current, owned portion of a declared centerline, the model computes

```text
P_excess = psi * L_current * (T_inside - T_outside)
E_excess = P_excess * actual_accepted_sample_duration
E_inside_total = E_inside_baseline + E_excess
E_outside_total = E_outside_baseline + E_excess
```

`psi` is the independently qualified nonnegative **excess** linear conductance in W/(m K), after
subtracting the identified ordinary textile stack under the same reservoir films. It is not a full
additional seam footprint. Negative temperature differences produce signed inward exchange; equal
reservoir temperatures produce zero bridge exchange. Signed contributions are retained, including
reversals over a history.

The existing local-normal projection owns the portions and their inside-to-outside textile stacks.
Each physical edge portion is counted once. Named paths follow retained mesh edges; a closed path
repeats only its initial vertex. Every edge must belong to the named retained instance, and every
contributing portion must remain covered by the declared body domain. Unrelated bare body remains
valid. Overlapping centerlines, uncovered seam portions, collapsed edges and incompatible shared
boundary qualifications refuse the bridge consumer.

As in ordinary moving heat, geometry is held at the accepted mechanical sample's right endpoint.
The bridge uses that endpoint's actual path length for the full accepted interval. Thermal
subdivision does not invent intermediate cloth/body motion or qualify a longer mechanical interval.
This integration is exact for the explicitly held geometry and constant interval reservoirs; it
does not establish continuous-motion accuracy.

The bridge stores no heat and changes neither the baseline temperature evolution nor the physical
stored-change, internal-source or remap ledger. Actual initial/remapped cells, every solved thermal
substep and material-source preload temperatures all participate in qualification. A hypothetical
steady face temperature cannot replace a hotter retained transient material state. With positive
conductances, the prescribed reservoirs and these cell extrema also bound reconstructed faces.

## Request and separate temporal evidence

The wrapper has these fields:

| Field | Meaning |
| --- | --- |
| `schema` | `ftx-motion-thermal-bridges/1` |
| `model` | `qualified_zero_storage_centerline_exchange/1` |
| `reference` | Complete existing `ftx-steady-thermal-bridges/1` request: exact mesh paths and alternative reference observations. |
| `qualifications` | One or more independently sourced temporal qualifications; exactly one must match each contributing portion. |

Each temporal qualification contains:

| Field | Meaning |
| --- | --- |
| `id` | Unique temporal observation identity. |
| `path_id` | Exact physical path identity in `reference.paths`. |
| `steady_qualification_sha256` | Canonical digest of the complete referenced steady qualification, including source and applicability domains. |
| `time_range_s` | Finite nonnegative elapsed-motion range. Both the previous and current sample time must be inside it. |
| `sample_interval_range_s` | Positive range for the actual accepted mechanical interval duration. |
| `temperature_range_c` | Additional qualification range for every live covered cell, preload and prescribed reservoir temperature. |
| `zero_storage` | Explicit `true`; this implementation does not accept a storage-bearing bridge. |
| `evidence`, `allow_estimates` | Evidence class and independent consent for an estimated temporal approximation. Material and steady-observation estimate policies remain separate. |
| `source`, `sources` | Source key and complete source records (`reference`, `accessed_on`, `note`). |
| `method`, `specimen_id`, `conditioning`, `qualification` | Source's method, specimen, conditioning and rationale/limits for instantaneous zero-storage exchange over the declared domains. |

Obtain `steady_qualification_sha256` from the existing steady bridge contribution's
`qualification_sha256`, or hash the canonical serialized `Qualification` value. Canonical values
include normalized defaults such as an absent quantity bound becoming `bounds: null`; hashing an
incomplete hand-authored object without that normalization does not identify the same observation.
The temporal law cannot be reused after editing its steady observation and retaining an old digest.

The existing reference qualification still checks the exact source-surface topology/material hash,
ordered material identities, interfaces, reservoir films, temperature range, total current path
length and every layer's current-cloth/body area ratio. The temporal wrapper adds requirements;
it does not weaken any reference domain. Material changes therefore require a matching source-bound
reference alternative and its own temporal evidence. No physics code changes are required to use
another properly qualified material alternative.

There are at most 128 paths, 2,048 path vertices, 64 reference alternatives per path and 512 temporal
qualifications. Existing bounded centerline geometry and temporal matching work count toward
`transient_coverage.max_geometry_operations` and the shared scenario-session reservation. Missing
qualifications, out-of-domain time/temperature/length, denied estimates, ambiguity, nonrepresentable
signed energy or late budget exhaustion refuse the entire thermal history. Unrelated mechanical,
manufacturing and thermal consumers remain independently available.

## Reports and decisions

Ordinary `inside_energy_j` and `outside_energy_j` now include the qualified bridge excess whenever
this policy was requested. The nested `thermal_bridges` summary retains:

| Field | Meaning |
| --- | --- |
| `baseline_inside_energy_j`, `baseline_outside_energy_j` | Original covered, bare and explicitly exposed-sheet boundary energies. |
| `bridge_energy_j` | Signed cumulative bridge excess, added once to each boundary side. |
| `total_inside_energy_j`, `total_outside_energy_j` | Totals used by the ordinary thermal result and incoming-energy objective. |
| `bridge_stored_change_j`, `excess_balance_residual_j` | Exactly zero for this law. |
| `surface_model_sha256`, `storage_modeled`, `temperature_feedback` | Source identity and explicit false flags for storage and temperature feedback. |

Each `trace[].thermal_bridges.interval` retains the actual `[previous,current]` time, current geometry
identities, region temperature/area domains, per-body bridge powers, exact path portions and selected
steady/temporal qualification digests. Its surrounding trace entry also retains cumulative baseline,
bridge and total energies. `final_field` preserves baseline body powers alongside bridge powers and
total body powers; its material region temperatures remain the baseline solution.

The existing `transient_coverage_inside_energy_j` objective automatically reads the total incoming
energy. `transient_coverage_net_boundary_energy_j` and temperature objectives retain their existing
storage meanings. Comparison admission checks the complete accepted sample sequence, actual edge
lengths and complete edge partitions, consumed sources and domains, signed arithmetic and cumulative
accounting. Storage reconciliation uses the retained baseline sides so large equal bridge additions
cannot create a fictitious heat balance through floating-point cancellation. Different consumed
source protocols require the existing explicit mixed-protocol consent.

## Limits and evidence

This law currently requires the characterized covered reservoir films; covered exterior convection
or radiation replacement is refused. Independently requested exposed-sheet exchange can coexist
because it acts on different physical portions. Independently qualified material dissipation heat
can coexist; its actual preload extrema still have to satisfy bridge domains.

No seam heat capacity, storage/recovery dynamics, relative transport, inferred microgeometry,
compression law, thermoregulation or empirical garment warmth is supplied. The numerical fixtures
are explicitly synthetic. They establish conservation, signed exchange, current-geometry/material
propagation and native/portable consistency; they do not establish real textile performance.

Omitting `thermal_bridges` preserves the ordinary coverage result and its serialized shape.
