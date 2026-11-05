# Local browser material workbench

`web/materials.html`, linked from the main drafting page, connects material selection to the existing
native engines. Build `web/pkg` with `sh tools/build_web.sh`, serve `web/` with
`python3 -m http.server -d web 8000`, and open `http://localhost:8000/materials.html`.
The build needs the repository's pinned Rust toolchain, wasm target and wasm-bindgen CLI; the page
has no Python or native-library runtime dependency. It does not install or operate a machine.

## Select once, reuse the existing consumers

After drafting or checking a round in Pattern Workbench, **Choose materials** opens this
workbench with the round's exact pattern, measurements and fit-specification files. Inspect
material sources, **Freeze selections and bind**, then explicitly validate/regenerate exports
or run another supported consumer. When selected, the exact local body model, original accepted
image-fit report and ordinary drape arm angle accompany those files. The receiving tab requires
a fresh wearer association before using an accepted shape; arrival never runs a consumer.
Measurement readiness is recomputed by the ordinary export consumer and does not become approval
to cut. A round without body sources still uses the original three-file route.
The handoff also preserves the ordinary arm angle without an accepted report. Old packets without
an explicit drape angle retain 35°, or their accepted-source angle when present. Conflicting angles refuse.

The tracing workshop's **Choose materials** action also opens this page with its exact reviewed
pattern and source image hash after the trace scale is fitted. Unscaled traces use Pattern Workbench
for scale review. It transfers no wearer, fit or body evidence. Assign materials and
explicitly bind before running a supported consumer; the trace retains its scale/audit limitations
and `REVIEW_ONLY` scope. **Clear received tracing pattern** or a manual pattern file replaces that
source and withdraws dependent results. A source edit before receipt cancels the pending transfer.

The tabs exchange immutable local files through the same origin, window and a one-use token;
no files enter browser storage or a server. Editing the source round, selected model, accepted
report, wearer association or pose before the receiving tab requests its files cancels the pending
transfer. A received round remains that snapshot. Choosing another pattern cancels the handoff;
individual file choices replace their imported counterparts and defeat delayed packets. Received
body sources have local download links for inspection and replay. **Clear received round** withdraws
its imported pattern/body sources, download URLs and dependent outputs. If popups are blocked,
download the ordinary round ZIP and select its files and original body sources manually.

Search the unified built-in catalog, select an exact record, and inspect its original observations,
normalized units, conditioning, sources and independent capability refusals. Alternatively, load a
portable `Selection` JSON containing a local characterization record. The page does not fetch supplier
pages or add a second database. Estimates remain disabled unless the selection explicitly admits them.
A source-locked record cannot be modified while keeping its old digest.

**Prepare a bounded membrane approximation from published fabric stiffness** turns an inspected
Wang et al. (2011) source origin into the ordinary bounded material law through
`material_linearize_origin`, the same core as `ftx materials linearize-origin`. Inspect a selection
with `allow_estimates:true`, explicitly enter all six limits for `[E11, E22, 2E12]`, and supply an
approximation reference, access date and rationale. The controls start empty. Each interval must span
zero; the core checks the physical domain, source/specimen/basis consistency and tangent stability.
The browser performs no stiffness calculation and chooses no validity interval. Confirm the local
estimate separately; this confirmation resets when the selected source changes. Admission is part of
the frozen material identity, so the page never flips `allow_estimates` under an existing digest.

The result displays the authored domain, source identity, dated rationale, tangent and model scope,
and inspects the exact returned `selection_json`. Assign that selection to an original pattern piece
or add it to the ordinary material comparison. Downloads retain the exact source report, portable
derived selection and request containing the original frozen source. Changing the domain or rationale
withdraws the prepared result and restores the original source in the editor, so preparing again does
not accidentally approximate an already-derived law. Cancellation and source edits defeat late worker
results. An already assigned or shortlisted material remains its explicit immutable snapshot.

This is an estimated constant tangent on the operator's declared Green-strain box, not the paper's
complete nonlinear model or an empirically measured validity range. Existing incomplete material
capabilities remain incomplete: no bending, contact friction, thickness or thermal data is supplied.
The source with an unstable origin block still refuses in the shared core. See
[wang2011-materials.md](wang2011-materials.md) for the model and native workflow.

Load an original `fabric-pattern/1` pattern and assign an inspected selection to its default or an exact
source-piece ID. A source-piece assignment affects that piece's mirrored/cut instances. Freeze and bind
creates a new in-memory pattern through `ftx_pattern::material_binding::Changes`; it never edits the
uploaded file. It freezes all effective selections, including untouched explicit pieces. Changing the
default preserves an already assigned lining or cuff. Grain, vertices, curves, seams, simulation roles
and cut quantities retain their original values. The optional **Replacement material-pair friction
evidence** file replaces the complete interface list in the same operation. It uses exact final
material hashes and independently sourced observations; the Rust binding validates those identities.
Omitting the file preserves the original list, uploading `[]` explicitly clears it, and **Keep original
interfaces** discards the pending replacement. See [piece-materials.md](piece-materials.md) for the
pair schema and the equivalent native `bind-changes` command.

Changing a pair file withdraws the current bound pattern and dependent outputs. Invalid or unfinished
reads disable binding until corrected or explicitly cleared; delayed reads cannot attach to a newer
pattern. Estimate admission belongs to each pair and remains separate from sheet-material consent.
Binding downloads include `original-pattern.json` and the exact `material-changes.json` request for
replay, alongside `pattern.json` and `material-bindings.json`.

`material-bindings.json` contains original/bound pattern hashes, the selected identity per source piece
and the production drape resolver's result with and without self-contact. These are material-input
checks, not evidence that meshing, contact or a complete simulation will succeed. Partial materials
can still be selected for a supported manufacturing or thermal consumer. No missing coefficient is
filled from a fiber name, construction label or a brand.

After binding, **Check or alter this pattern** opens Pattern Workbench with the exact current
bound pattern and its reported hash. Select measurement evidence and a fit contract there, then
explicitly **Draft and check** to use its ordinary alteration controls. No wearer, accepted body,
readiness or cutting approval is transferred. Source/assignment edits, re-binding or cancellation
before receipt cancel the handoff; an already received pattern is an immutable snapshot. A manual
pattern file or **Clear material handoff** replaces it. Blocked popups can use `pattern.json` instead.

The bridge retains a `pattern_json` string and original Rust drape/report JSON. JavaScript parses
copies for display but does **not** reserialize source-bound numeric data for subsequent consumers or
downloads. Otherwise values such as `1.0` and `-0.0` change representation and invalidate the existing
material/geometry receipts. Nested coverage requests preserve validated JSON fragments verbatim;
they do not replace the hash checks or manufacture new receipts.
The existing drafting worker and its `sim_report.json` download use the same lossless rule.

Section 3 accepts **Additional measurements** alongside a primary measurement document. Confirm
that all files describe the same wearer before running a consumer. Up to eight files (16 MiB total)
enter the existing Rust validation/fusion path, preserving exact original source text and photo
uncertainty. Exports, drapes and body studies using section-3 measurements consume the same fused
records; drapes download `measurements.json`, and studies retain it in `body-input.json`. Separate
body-request mode uses that request's own evidence. Evidence edits withdraw affected outputs and
reset association; **Clear additional measurements** explicitly returns to primary-only evidence.
Refused supplements remain selected until corrected or cleared. PHOTO_ESTIMATED and MODEL_FIT
remain distinct from direct measurements, and neither added evidence nor a computed body proves fit.

**Measure a photo for this wearer** opens the local photo tool while keeping this session's pattern,
material choices, primary measurements and model. After measuring, choose **Return records to
Material Workbench**, then review the original photo-record download and confirm the same wearer
here. Returned records share the eight-file/16 MiB supplemental budget and ordinary fusion path.
Changing the primary measurements, model or pattern cancels a pending return. Closing the photo
tab or returning refused evidence blocks its use until cleared or replaced; no old photo is reused.
**Clear additional measurements** removes the returned snapshot and any manually selected supplements.

**Check body against measurement targets** runs the existing body-check engine without a garment,
material binding or fit contract. Select the model and measurements, or associate an accepted
image-fit report explicitly. Supplemental files use the ordinary fusion path. The table retains
unrounded model/target differences, supplied uncertainty, fitting scales and unavailable sections;
accepted shapes are compared without refitting. Downloads preserve the exact report, measurement
document and optional original accepted report. Body/evidence/association edits withdraw results.
The comparison uses the model's measurement pose; drape angle and contact controls remain separate.
MODEL_FIT predictions and a successful check do not change readiness or establish physical fit.

The garment operations consume this same bound pattern:

| Operation | Additional local inputs | Existing authority and output |
|---|---|---|
| Validate/export | Measurements and fit-specification JSON | `ftx_wasm::page::run`; current verdict basis, measurement readiness and checked export ZIP. No implicit drape/fit claim. |
| Mechanical drape | Measurements, fit specification, converted `.ftxbody` model, optional accepted image-fit report | Existing fold engines fit the supplied model or reuse the exact accepted shape at the declared arm angle. Nondefault tape-fitted angles use `fold_up_configured`. Current report and fold-up viewer, or an explicit refusal. |
| Cutting | `CutOptions` JSON | `ftx_export::cutting::CutPlan`; all material-specific roll requirements and a bounded first-roll SVG preview. Not a machine-coordinate program or proof of optimal nesting. |
| Complete mixed cutting | `ftx-cutting-job/1` or `/2` request | Existing `material_cut_job`; complete roll/finite-stock inventory, route-specific knife programs or independent refusals, purchasing outcomes and exact frozen replay inputs. |
| Purchasing | Cut options, dated `QuoteBook`, comparable `Market` scope | Existing marker procurement evaluates gross stock including layout waste and order terms. Missing offers remain unknown, not zero-cost purchases. No checkout or live refresh runs in the page. |
| Finite-stock identities / pricing | Finite-stock request, plus `ftx-finite-stock-pricing/1` for prices | Shared `stock_identities` / `stock_price`; exact offer-binding identities, dated whole-item prices and exclusions, unknown required totals and original replay inputs. The separate price file does not re-optimize stock allocation. |
| Sewing | Complete `SewOptions` recipes and machine envelope | Existing `SewProgram`; every declared stitch interval and process refusals. Execution remains unauthorized. |
| Independent sheet heat | A fresh workbench drape and authored uniform conditions or original `SurfaceConditions` JSON | Existing `thermal_steady_surface`; material-following fields, optional declared exterior exchange and source-locked replay. |
| Independent sheet vapor | A fresh workbench drape and authored uniform conditions or original `ftx-vapor-surface-conditions/1` JSON | Existing `thermal_vapor_surface`; current sheet areas and source-qualified Ret, independent of dry heat readiness or available water. |
| Layered body coverage | A fresh workbench drape, body/instance-layer map and coverage conditions | Existing `thermal_steady_coverage`; recomputed overlaps, covered/bare accounting and exposed-face exchange. No guessed layer map or environmental data. |

The GarmentCode review offers **Also select the supplied fit specification** when annotations
include one. Check it before **Use imported pattern** to select the exact Rust-generated contract
without downloading and uploading it again. The active contract and its download appear beside
the ordinary fit-file input; choosing a file there replaces it. Source, annotation or arc-policy
edits withdraw the pattern and contract still owned by that import. An explicitly adopted study
or treatment pattern retains its selected contract as a frozen input; a new pattern file clears
the imported snapshot. Pattern-only adoption preserves manual fit-file choices. Measurement
readiness and physical fit remain independent.

For a complete mixed cutting job, load its request under **Complete roll / finite-stock job JSON**
and select **Plan complete mixed cutting job** after binding the current materials. Every source
piece must have an explicit route; the page does not infer roll versus hide from material names.
The result keeps valid allocation when a knife qualification or purchasing offer is unavailable.
Download `cut-job-pattern.json` and `cut-job-request.json` to replay the frozen job with native
`cut-job`; the original bound pattern, original request and complete report remain separate
downloads. Request or source edits, cancellation and refused replacements withdraw stale outputs.
These review results do not reserve stock, place orders or authorize a machine.

After a finite-stock allocation, **Use planned remaining stock** selects the exact Rust-generated
residual request for the next allocation, identity check, price check or guided finite-stock study.
Choose it only when treating that layout as consumed in the next planned job. Its consumed regions,
zones, material identities, search policy and price locks remain intact; Rust rechecks them on the
next run. A consumed item's old price binding may therefore be unavailable until requalified.
The selected request has its own download. Choosing a stock file replaces it, and **Clear selected
stock inventory** removes the current choice. Source or binding edits withdraw stale adoption
buttons; no result automatically consumes stock. Each allocation retains its exact original bound
pattern and stock request downloads alongside the plan and residual request. This records a planning
decision, without confirming physical cuts or valuing offcuts.

Drape arm angle applies to both tape-fitted and accepted shapes, from 0–180° with a 35° default.
Blank or invalid angles refuse. Nondefault ordinary angles download `fold-settings.json` for
native replay. Tape fitting and body checks retain their measurement pose. Body-dependent studies
keep their explicit source policy below; selecting a preview angle does not change measurement-source studies.

To keep a previously accepted image-fitted shape, open **Reuse an accepted image-fit shape for
this drape**, choose its original report, explicitly associate it with the current wearer and
select the ordinary drape arm angle. The existing Rust body authority verifies the exact model,
accepted status and measurement consistency. It does not reuse an image-frame pose or refit the
shape from tape. The original report is downloadable alongside the simulation and viewer.
An invalid replacement stays selected and refuses; only **Clear accepted fit and use tape fitting**
returns to the tape-fitting path. Changing the pattern, model or measurements clears the wearer
association. Report or pose changes withdraw drape, heat, vapor and coverage results, including
pending work. The independent measurement/export readiness gate is unchanged. Numerical acceptance
and a completed drape do not establish physical garment fit.

### Discover suitable materials and use their exact sources

In section 4, **Discover suitable materials** opens ordinary catalog filters. Choose a nominal or
source-reported-range basis, search by identity, fiber or construction, and optionally restrict the
material form. The mechanics, steady-conduction and transient-thermal checkboxes run the existing
native compilers under each material's evidence policy. A record with many observations still fails
a requested compiler when its required data is missing. Unrequested consumers do not exclude it.

Add property rules with inclusive minimum/maximum values, an independent minimize/maximize objective,
or both. The displayed units are canonical: for example, 0.3 kg/m² is 300 g/m² and 0.02 m is 20 mm.
Choose another exact property key/unit for observations outside the common presets. Optional method
and condition restrictions retain exact source strings. **Add property filter / objective** freezes
that rule; subsequent draft edits do not change it. Remove a rule to replace it, or clear the list.
The native limits remain 64 constraints and 16 objectives. Search and compiler requirements also
work without property rules. Estimates and cross-protocol comparisons require separate explicit choices.

For a source-qualified insulation shortlist, search `thinsulate`, require steady conduction, set
areal mass at most 0.3 kg/m² and thickness at most 0.02 m, minimize areal mass and maximize thermal
resistance in m² K/W. These are the policies in `tests/fixtures/materials/screen-insulation.json`;
they compare reported observations and do not establish a garment temperature rating.

Each successfully resolved result offers **Inspect … source** and an exact frozen-selection download,
including excluded results whose evidence may still be useful for another purpose. Eligible results
also offer **Add … to comparisons** when an original pattern is loaded. The shortlist retains up to
seven saved alternatives plus its unchanged baseline, rejects duplicate individual frozen identities, and feeds the
existing piece/default binding, manufacturing, thermal, motion and design-study paths. No source is
reconstructed by copying values from the displayed table. The native `selection_json` string keeps
negative zero, source numbers, evidence consent and digest intact through adoption and replay.

Changing a discovery policy withdraws its old results and pending reads; already chosen shortlist
entries remain independent frozen snapshots. Stale or busy result buttons cannot replace a source.
Selecting a screening file switches to the original-file route, which retains its exact bytes.
Scalar eligibility, compiler readiness and a computed frontier remain separate from measured textile
behavior and physical garment qualification.

### Compare complete garment material configurations

A guided alternative can retain several material assignments together: for example, one shell,
a different lining and their exact material-pair contact evidence. No study JSON file is needed.

1. Load the original pattern in section 2. Inspect each material in section 1, then assign it to the
   pattern default or its exact source pieces. Changing the default preserves explicit piece assignments.
2. Retain the original interface evidence, supply a complete replacement list, or stage a calibrated pair
   through the existing contact controls. A replacement list `[]` deliberately removes all original pairs.
3. Optionally name the alternative and choose **Save current assignments to comparison**.
   This saves the current default and piece substitutions and the current interface choice as one
   immutable configuration. A pending or failed interface-file read must finish or be cleared first.
4. Edit the draft assignments for another configuration and save it under another name. Run the ordinary
   guided manufacturing, single-scenario, named-scenario or linked-design comparison.

The shortlist shares one limit of seven alternatives across individual materials and configurations.
The original pattern always supplies the unchanged baseline. Current preview edits affect a saved
alternative only when explicitly captured; clearing the preview or inspecting another source leaves
previous alternatives intact. Remove an alternative and save the revised assignments to replace it.
Replacing or adopting the original pattern clears its shortlist, preventing assignments from being
reused with unrelated source-piece identities.

Individual material choices continue to use the shared default/all-pieces/selected-pieces target.
A configuration supplies its own assignments, so that target is disabled and ignored when the list
contains only configurations. Mixed shortlists apply the shared target only to individual materials.

Each configuration's expandable detail shows the retained assignments, estimate policies and interface
evidence. The generated `variants[].changes` contains the original source fragments without converting
their numeric representations. Omitted interface replacement preserves the original table exactly;
the browser never maps old friction evidence onto new material hashes. The native binding and each
consumer perform their existing admission checks. Independent outcomes remain available when another
consumer lacks suitable evidence. Saving an alternative itself does not qualify a material or garment.

### Continue from an evaluated study candidate

Each available final binding in a manufacturing, single-scenario or named-matrix study offers
**Use … pattern**. This makes that candidate's exact `pattern_json` the new workbench source,
including its evaluated alterations, treatment effects and frozen material/interface selections.
The original numeric text is retained for the next binding and consumers. No candidate is chosen
automatically, and a refused binding has no adoption action. Independent consumer refusals remain
visible for review before choosing an otherwise available pattern.

Adoption clears pending material edits, combination choices, old bindings, previews and study results.
Choose **Freeze selections and bind**, then explicitly run the desired consumers against the current
body, fit specification and process inputs. Received measurement/fit/model snapshots remain selected;
accepted-body association requires fresh confirmation. Study alterations now start from the adopted
candidate, so review the study request before running it again. A selected pattern carries no old
measurement-readiness, scenario-admission or physical-fit verdict.

Joint outfits retain their original source request, composition correspondence and required
self-contact. **Use derived pattern** after a dimensional treatment uses this same source-adoption
path: garment assignment groups and original outfit correspondence remain available for the next
binding and drape. The original file chooser is cleared so that selecting that file again can
explicitly restore it. Current measurements, fit specification and model remain selected, while
accepted-body association requires fresh confirmation. No prior drape or fit verdict follows the
treated pattern. Cleared, replaced or invalidated study/treatment results cannot later replace the
current pattern through a stale action. Adoption is unavailable while a worker operation is pending.

The treatment result's **Use derived pattern** action follows the same adoption rules. It keeps
the joint outfit's garment assignment groups and exact treated pattern, retains the selected
measurement, fit and body inputs, and clears the accepted-body association for fresh confirmation
before recomputation.

### Search linked alterations against the selected body

The guided study's **Fit-driven linked alterations** option searches up to three ordered variables
through the existing design engine. Load the original garment fit contract in section 3, select the
local model and the desired study body source, then choose the source pieces or vertices to alter.
Alteration targets are independent of the material-assignment target. An accepted shape still requires
the existing wearer association; a refused or missing source never falls back to another body.

Choose one source-coordinate axis, a cut coordinate shared across the selected pieces, a nonzero
multiplier, and a comma-separated list of strictly increasing values. Every selected piece receives
the listed millimetre value times the multiplier. This is not automatically a total circumference
adjustment. For local shaping, select **Move selected source vertices**, choose exact piece/vertex
pairs, and specify horizontal and vertical movement per listed millimetre. At least one component
must be nonzero. Relative curves follow their endpoints while independent marks stay fixed.
Every trial starts from the original pattern and uses canonical alteration, seam, outline, cut/grain
and flat-fit checks; the browser does not transform geometry or repair seams.

Use **Add another variable** for a coupled adjustment. Each variable has its own name, sites and
1–64 strictly increasing values. **Edit**, **Move … earlier** and **Remove** preserve the other
drafts; order matters because linked operations are applied sequentially. Merely viewing another
variable leaves an active study running. Changing its values, sites or order cancels affected work.
Linked sites and optional seam adjustments share a limit of 16 operations per design. The vertex picker is populated only
when needed and supports up to 2048 original vertices; larger sources can still use complete design
requests, slash-and-spread and independent material workflows.

The original fit contract must contain 1–32 explicit points and fit within 256 KiB of UTF-8 source.
With a blank **Maximum grid points to screen**, every listed combination is requested, through a
maximum of 256. Supply an explicit allowance from 1 to 256 to inspect a bounded prefix of a larger
domain. The complete declared domain remains in the request, including unvisited coordinates;
invalid displacements cannot hide beyond that prefix. The browser does not expand the Cartesian
product. Rust generates, validates and caches each screened geometry once for the downstream materials.
**Maximum retained altered designs** retains the first qualifying designs in grid order.
It does not choose the cheapest designs before evaluating their consumers.
The original geometry and unchanged material each add a baseline, and the complete reserved
geometry/material cross-product must fit within 32 alternatives. Oversized requests refuse rather
than silently trimming the value list or material shortlist.

Both comparison actions use the selected body automatically when linked alterations are enabled.
Fresh scenarios retain the same mandatory flat-fit screening even if their separate drape-fit
reporting is explicitly set to no contract. Existing result tables show screened refusals,
passing-but-unselected designs, actual manufacturing/scenario outcomes and the computed frontier.
Use a candidate's **Use … pattern** action to retain its exact evaluated geometry and material
assignments. Loading or adopting another pattern clears every variable and source-site selection, preventing an
accidental second application. An advanced request retains its own authored alterations.

The existing single-variable synthetic numerical test screens nine values and retains the +30 mm linked front/back
adjustment, producing two 130 mm panels. Complete native, compiled-WASM and actual-worker reports
match, and the selected bound pattern reproduces its retained cutting plan. That is software and
declared-target evidence; physical wearer fit and continuous or global optimality remain separate.

#### Adjust seams inside each candidate

Under **Adjust seams after the linked alterations**, add the canonical operations that each searched
design needs after applying all its variable sites. Both listed-grid and adaptive discovery retain
the same ordered sequence:

- **True a seam by moving its B-side endpoint:** select an original seam and explicitly choose its
  start or end endpoint. The existing operator searches the bounded B-side slide while retaining
  that seam's declared ratio and tolerance.
- **Adjust sleeve-cap height to a declared ease ratio:** select the original cap piece, one or more
  original cap/armhole seams, and a positive target total cap/armhole length ratio. The existing
  operator adjusts cap height; original seam metadata and each half's validation remain intact.

Choose **Add seam adjustment** to retain the operation. Draft field changes do not change an active
sequence. Use **Replace … from current controls**, Move, Remove or Clear to change it explicitly.
Operation order is preserved and can affect the resulting geometry. Exact source piece/seam names
are required; an ambiguous seam name refuses. The picker supports up to 2048 named source seams;
larger sources retain the complete-request workflow and other guided design operations.

The total number of variable sites plus these operations must not exceed 16. Each seam-truing or
cap-ease operation reserves 64 document passes per trial under the existing native work accounting.
Reduce the authored search allowance if its full reservation is too large. A missing root or failed
outline, seam or selected-body fit check remains a refused candidate, with its existing diagnostics.
Only admitted designs continue to material and scenario consumers. No original seam contract is
relaxed to make an alteration pass.

The unchanged baseline receives no adjustments. The resulting `design_grid.after` or
`design_adaptive.after` reaches the existing Rust evaluator unchanged, and all named cases share
that same studied geometry. Accepted sequence edits cancel affected work; loading or adopting a
different pattern clears the sequence. Complete study files retain their own operation policy.
This connects existing geometry operators to design discovery and does not establish physical fit.

The executable synthetic example moves three linked corners by 10 mm, then trues the original
side seam into two 20 × 110 mm panels. Removing that explicit repair leaves the candidate refused.
A second adaptive example adjusts a cap against its original 1.1 seam ratio and admits the selected
geometry to assembly-mass comparison. Complete reports match across the native command, public
WASM API and worker, including selected-body fit, preserved seam metadata and baseline refusals.

#### Discover values and production tradeoffs adaptively

Choose **Discover values within declared bounds** to search between listed grid values. For every
variable, supply a finite lower and upper bound, an initial coordinate inside that interval, and
positive initial and minimum polling steps. The minimum step cannot exceed the initial step;
the initial step cannot exceed the domain span. Each variable retains its grid list separately,
so switching search methods does not convert values into a domain or discard either declaration.

Choose what guides discovery:

- **Satisfying the declared fit bands** uses the original selected-body point-of-measure bands.
  The existing engine compares unrounded out-of-band residuals and retains qualifying designs;
  downstream production goals still compare the retained designs independently.
- **Improving one comparison goal** uses exactly one goal from the ordinary comparison controls.
  Nominate the unchanged material baseline or one exact frozen shortlist choice explicitly.
- **Exploring tradeoffs across comparison goals** uses two or more ordinary goals and one explicit
  material nominee. It retains admitted tradeoffs without combining the goals into a weighted score.
- **Improving the worst named case for one shared goal** uses a loaded named-case matrix whose
  effective policies share exactly one metric, direction and comparison basis.
- **Exploring tradeoffs across all named cases** keeps every case's own comparison goals and basis.
  At least two case/objective coordinates are required; each case may have different goals.

For production guidance, every trial must pass the ordinary geometry and selected-body fit gates,
then the existing comparison's limits, required results and source/protocol admission. Missing or
refused measurements do not become zero scores. The nominated alternative guides discovery; selected
geometries subsequently cross every shortlisted alternative. A configuration nomination retains its
complete default, piece and interface changes, so replacing just a lining or a contact policy also
requires a fresh nomination. Removing or replacing that nominated source clears its selection and
requires an explicit new choice, even if the same name or default material is reused.

Set **Maximum unique discovery trials** from 1 to 256 and the retained-design capacity separately.
Adaptive admission reserves the full retained design/material capacity, including both baselines,
even when fewer discovery trials are allowed. Native aggregate manufacturing and scenario budgets
also apply. The existing engine caches evaluated coordinates and reuses retained production outputs
instead of rerunning them for final comparison. Trial/step limits and later refusals keep valid
incumbents visible, with the unexplored continuous domain still unknown.

For a loaded named-case matrix, single-goal and within-case tradeoff guidance require an explicit
selection from that file's case names. The selected case's original comparison policy takes
precedence over the ordinary comparison controls; an omitted case policy inherits those controls.
Every case must still admit every retained trial. Replacing the scenario file clears the nominated
case, even when the replacement uses the same name. All-case guidance omits that nomination.

The matrix's declared `max_evaluations` must cover
`case_count × (discovery_trials + (retained_designs + 1) × material_choices_including_baseline)`
for consumer-guided discovery, up to 64. Fit-only discovery does not run the consumers during its
probes, so it reserves only final comparisons. Admission never reduces your search or retention
allowance to fit a budget. Manufacturing-only comparison remains independent of a loaded matrix.

Exact original matrix, fit, body, material and process inputs remain in replay downloads. The
matrix's complete source policies retain advanced consumers and reported ranges; Rust performs
their numerical and source admission. Explicit restart points still require a complete study
request. These authoring controls add no numerical optimizer, physical fit certification or global
optimum claim.

### Combine reported-range objectives with nominal constraints

The comparison policy's `basis` governs its objectives. Each constraint can explicitly declare
`basis: "nominal"` or `basis: "reported_bounds"`; omission inherits the policy basis. This lets a
source-bounded mass or vapor objective retain a nominal strain, contact or production limit without
inventing uncertainty endpoints for that limit. Conversely, a nominal objective may require a
bounded constraint. Every declared use of a metric must qualify: a nominal constraint cannot
relax a bounded objective on the same metric.

The workbench shows each constraint's effective basis and inclusive limits. Missing ranges appear
only where an objective or constraint requires them, including named-case and adaptive-trial
readings. The ordinary Rust admission, source protocols and independent consumer refusals remain
authoritative. Nominal constraints make no uncertainty claim.

### Explore adaptive tradeoffs across every named case

In an authored design study, set `design_adaptive.objective` to
`consumer_matrix_pareto` and nominate one `material_key`. Omit `case_key`: every
case in the named matrix guides discovery using its own effective comparison policy.
Cases may use different metrics, directions and nominal or reported-range bases. A case
may minimize the same signed heat metric that another case maximizes.

The **Multi-case Pareto adaptive alteration search** view shows each case's complete
objective vector, basis, inclusive constraints, required consumers and protocol policy.
Trial readings come from the original per-case comparisons; the page creates no aggregate
reading or weighted score. Every case must admit a retained design. The engine's dominance
decision requires no worse outcomes across all cases and a strict improvement in at least one.

Evaluated nondominated trials, retained outputs and candidates omitted by the output capacity
remain separate. A capacity-omitted trial can still guide subsequent search. Consumer refusals,
missing required ranges and case protocol incompatibility remain visible with their original
reasons. The bounds and shared budgets describe the explored work; the result does not establish
a complete continuous frontier or physical garment fit. Existing nominated-case Pareto and
worst-case searches retain their own meanings and views.

### Compare thread-order costs across fixed sewing routes

For one bound pattern, enable **Choose from listed sewing recipes and routes** in the
direct sewing controls and load a complete `ftx-sewing-selection` request in the sewing
file input. **Choose recipes & evaluate stitch paths** calls the shared bounded selector,
then generates the selected job's actual stitch program. Downloads retain the original
request and pattern, complete selection report, exact selected options and canonical program.
No selected candidate means no options/program download and no fallback to the original
recipe. Changing the request, selection mode, bound pattern or material withdraws the old
results; cancellation discards pending output. These plans do not authorize hardware execution.

A study's `sewing_selection` may use `ftx-sewing-selection/6` with
`objective: "minimize_thread_observed_order_cost"` to compare its authored fixed routes
and recipe choices. Each route supplies the actual qualified cell recipe, thread demand and
complete source-qualified purchasing result. Eligible-cell assignment searches remain outside
this cost objective.

The workbench shows the chosen route and recipes, selected complete cost and currency, proof
status and unresolved trials. **Complete thread order costs by recipe and route** also exposes
every trial's cost availability and independent deadline admission. Missing consumption or prices
remain unresolved; they never become zero-cost orders.

An optional `completion_deadline_milliseconds` constrains complete schedules before cost
selection. A cheaper route that misses the deadline, or whose deadline status remains unresolved,
retains its cost for review without becoming the chosen route. Pricing a complete fixed-route
order does not require proof of the shortest makespan. The separate deadline table retains the
completion time, lower bound and timing proof, and downloaded reports preserve the exact inputs
and source evidence.

### Inspect the actual sewing operation and production cell

After a successful **Evaluate sewing & stitch paths** or recipe-selection result, choose
a **Sewing operation** and, for parallel or routed production, an explicit **Production cell**.
**Preview selected sewing operation** calls the canonical native SVG renderer with the exact
retained pattern and evaluated options. A selected recipe uses its `selected_options_json`,
including the selected route's machines and qualifications; the search template is not replayed.

Separate passes over one seam keep their own operation IDs. Configured cells offer only their
own source operations, and routed cells use their retained specialist plans. The summary shows
the selected garment copy IDs for that operation. An admitted idle cell or unused eligible
operation may be inspected, with its empty copy list stated explicitly.

The SVG distinguishes exterior cuts, original seam lines, offset stitch paths, needle stations,
and the source-qualified geometry supported by that operation. Its two flat source frames are
laid out independently; their display translations do not establish machine registration.
The exact SVG, original pattern, evaluated options and chosen operation/cell are downloadable.
No geometry is recreated in JavaScript.

A preview-only refusal preserves the completed sewing result and its downloads. Changing a
selector cancels a pending preview and withdraws its image; changing recipes, selection mode,
material bindings or source pattern also withdraws the retained preview inputs. Cancellation
and late worker responses cannot restore obsolete images. A search without an admitted
incumbent provides no preview. Inspection does not certify physical seam quality or authorize
hardware execution.

### Reuse the selected body in manufacturing and scenario studies

Section 4 has an explicit **Body source for body-dependent studies** choice. It applies to fresh
single-scenario and named-matrix comparisons and automatically to guided linked alterations.
Advanced manufacturing studies with an explicit `design_fit` contract use it when **Constrain an
advanced manufacturing study to the selected body** is checked. Every mode uses the section-3 model.
Fixed-pattern manufacturing without that constraint needs no body input.

- **Fit section-3 measurements at 35°** uses the current measurements through the existing tape-fit authority.
- **Fit section-3 measurements at preview pose** uses those measurements with the preview's arm angle,
  leg posture and fraction, preserving them after supplemental measurement fusion. It performs ordinary
  tape fitting and does not consume a selected accepted-fit report or its consent. Study solver settings
  and authored motion remain explicit in the study request. Pose edits withdraw dependent results and pending work.
- **Reuse the accepted shape and pose selected above** uses the original accepted report, current ordinary
  arm angle and explicit wearer association already selected for the preview. Pattern-to-material handoffs
  support this choice directly. Optional current measurements are compared with the accepted shape without
  refitting; an absent measurement file is allowed. The page embeds the original report and measurement
  text in the existing `ftx-scenario-body/1` request without reserializing source-bound numbers.
- **Use the separate body-source request below** reads only that request's measurements, accepted report
  and pose. Its body source is independent of the preview report, angle and section-3 measurement file.

An absent or invalid selected source refuses; the page never substitutes another mode or tape-fits a
refused accepted shape. Clearing an accepted report or a separate request requires explicitly choosing a
different study source before using one. Changing a consumed model, measurement, report, pose, wearer
association or source choice withdraws affected results and cancels pending work, including delayed file
reads. Inputs that the selected mode does not consume leave its studies intact. The original model stays
local, and each body-dependent result downloads its exact `body-input.json` alongside the original study
and scenario inputs for native replay. The Rust body/fit authority and all ordinary qualification gates
remain authoritative; selecting an accepted numerical shape does not establish physical garment fit.

### Declare uniform sheet heat and vapor conditions

In section 3, each sheet consumer offers **Retain original JSON** or **Author uniform conditions**.
Original files remain the default. The uniform forms start with every physical value blank and
require explicit input; switching modes preserves the inactive file and form without consuming them.

For heat, enter inside/outside reservoir temperatures in Celsius and both film resistances in
m² K/W. Temperatures must be finite and at least −273.15 °C; films must be finite and nonnegative.
Zero film resistance prescribes the corresponding material face temperature. Equal or reversed
reservoirs are valid, and the native result preserves signed heat flow. The boundary applies to
every retained sheet using its own qualified thermal transport and current draped area; no heat
capacity or transient policy is required.

For vapor, enter inside/outside partial pressures in Pa, both film resistances in m² Pa/W and
latent heat in J/kg. Inside pressure must be at least outside pressure and both must be nonnegative.
Equal pressures, including zero, are valid. Both vapor films and latent heat must be strictly
positive. The form does not convert relative humidity or infer latent heat, sweat, liquid inventory
or evaporation. Native Ret evidence and protocol admission remain independent of dry heat.

The forms generate one uniform boundary without binding a previous preview's model hash. Use
original JSON for nonuniform model-bound conditions, qualified exterior exchange, exact Ret source
filters or explicit vapor protocol consent. Original text is retained unchanged. The native result
owns material qualification, physical domains, numerical residuals and any supported reported bounds.

After a fresh drape, the ordinary sheet heat and vapor actions consume these selected conditions.
Guided heat scenarios use the same section-3 selection. Guided vapor scenarios can either retain
their separate original case file or explicitly **Use the selected section-3 sheet vapor conditions**.
Saving a named case freezes the selected requests. Later form/source changes affect the active
draft, while saved cases change only through explicit replacement. Active changes cancel obsolete
reads and results; inactive files/forms and unconsumed section-3 sources leave independent work intact.

### Configure fresh garment/body comparisons without a scenario file

Under **Fresh body, drape and heat studies**, choose **Configure one garment/body scenario here**.
It uses the body source selected above and the original pattern, with a new production drape for
the baseline and every inspected material choice. Select the original section-3 fit contract, or
explicitly choose **No fit contract — numerical comparison only**. A missing or malformed selected
contract refuses; it never becomes an empty contract automatically.

Set mesh spacing, sewing and gravity steps, time step, optional additional settling, and mesh and
aggregate vertex-frame limits. The initial phases need at least six steps, with at most 256 steps
including settling. The ordinary cloth–cloth contact option is explicit; joint outfits require it.
These are numerical settings, not measured fabric thickness, friction or a runtime guarantee.
Enable independent sheet heat to retain the exact section-3 heat conditions and evaluate each
alternative's current geometry through the existing thermal engine.

Guided material comparisons can combine the following goals and inclusive limits with
manufacturing objectives, or run without manufacturing consumers. For a scenario-only comparison,
clear unused manufacturing goals and limits before disabling their inputs.

| Quantity | Meaning and admission |
|---|---|
| Simulated cloth mass (kg) | Mass of the simulated pieces; it excludes unsimulated parts. Use the separate complete-inventory assembly-mass consumer for total garment mass. |
| Maximum absolute panel-edge strain (%) | The existing edge-based simulation metric. It is not principal strain or a physical safe-fit threshold. |
| Peak absolute panel-edge strain through movement (%) | Includes the initial drape and every accepted motion sample. Requires the complete requested trajectory; failed or incomplete motion cannot qualify. This remains an edge metric, not principal strain or a safe-fit threshold. |
| Signed independent sheet power (W) | Declare minimize or maximize explicitly. Nominal values or complete source-reported ranges for constant thermal laws are available, with signed minimum/maximum limits. Negative power stays negative; minimizing signed power does not minimize its magnitude. This is not layered coverage or whole-wearer comfort. |

Maximums remain active even when their objective checkbox is clear. Explicitly required drape,
sheet-heat and motion consumers can exclude alternatives without adding a goal. An unsettled or gating-flagged
drape cannot qualify these comparisons. Missing mechanics, thermal evidence or work budget retains
the native refusal and any independent manufacturing result. A reported-range assembly-mass
or sheet-heat objective may use an explicit nominal limit basis for the cloth-mass and strain guards;
unsupported result ranges are not invented. Source-range heat limits require the entire reported
interval inside the declared limits; missing ranges retain nominal readings but exclude the range
decision. Use **Recompute every alternative in the requested scenarios**; the
manufacturing-only action refuses scenario goals rather than silently dropping them.

The generated `study-request.json` and `scenario-request.json` retain the original fit, heat and
material source text, alongside the exact pattern and body input downloads. Active source, goal,
mode and numerical-setting edits withdraw stale results and cancel pending reads or worker work.
Unconsumed section-3 files do not replace an advanced scenario's own sources. Original motion
requests retain their transient heat and coupled-transport policies inside guided single or named
cases. Select **Load a complete scenario or named matrix (advanced)** for other existing scenario
policies; its exact request remains authoritative.

#### Compare layered heat and potential vapor transfer

The **Layered heat and potential vapor transfer** controls keep three independent consumers:

- **Body/layer heat** can be authored in the form or retained as the original `scenario.coverage`
  fragment: `body_triangle_ids`, `instance_layers`, `settings`, `conditions` and optional qualified
  `thermal_bridges`.
- **Independent-sheet vapor** takes original `ftx-vapor-surface-conditions/1` conditions or the
  explicitly selected section-3 uniform/form or original-file conditions.
- **Covered-body vapor** takes the original `scenario.vapor_coverage` fragment with
  `body_triangle_ids`, `instance_layers`, `settings` and `conditions`.

For authored heat, choose **Declare body regions, garment layers and uniform heat conditions here**.
Enter up to 4,096 unique collision-body triangle IDs, using individual IDs or inclusive ranges such
as `0-15, 22, 31-33`. These are indices in the selected body's collision mesh, not inferred anatomical
regions. Saved cases share the selected body; changing that mesh retains the numeric declaration,
so review its intended physical region before rerunning.

Save each exact retained physical-instance name with its physical layer, then declare every needed
directed interface with its resistance and a measurement source or explicit assumption. Zero
resistance must be entered explicitly. Save/replace/remove acts on individual rows; typing a row
only prepares a draft. Renaming a layer does not silently repair an older interface. Declare the
search distance, normal cosine, separation tolerance and depth resolution, followed by uniform
inside/outside reservoir temperatures and film resistances. The native projection still determines
actual overlap and layer order and rejects ambiguous or unsupported geometry.

Named cases freeze the complete saved topology and conditions. Active control edits withdraw stale
work; inactive original files and unsaved rows do not change an authored request. Pattern replacement
clears this garment's topology. The original-policy mode preserves advanced input bytes for
nonuniform boundaries, source-qualified convection/radiation, stagnant-gap interfaces and thermal
bridges. This authoring increment is tracked by `ftx-ms2.21`; it does not add a thermal law or prove
physical garment coverage.

Coverage fragments have no separate schema field. Supply scenario projection policies, not a
previously computed coverage map: every material/design alternative projects its declared layers
onto its own fresh body and cloth geometry. The source files preserve exact JSON text, including
protocols, thermal bridges and source numbers. Rust admits their physical domains and material
evidence; a refused optional consumer does not erase independent drape or manufacturing results.
Malformed typed JSON can still reject the complete request before consumers run; this independence
applies to admitted requests whose source or physical domain fails an individual consumer.

The ordinary comparison exposes these quantities, each with an explicitly chosen minimizing or
maximizing direction and optional inclusive lower/upper limits:

| Quantity | Unit | Required input |
|---|---|---|
| Signed body/layer heat | W | Body/layer heat policy |
| Independent-sheet vapor heat potential | W | Sheet vapor conditions |
| Independent-sheet vapor mass potential | kg/s | Sheet vapor conditions |
| Covered-body vapor heat potential | W | Covered-body vapor policy |
| Covered-body vapor mass potential | kg/s | Covered-body vapor policy |
| Covered-body vapor energy potential through motion | J | Covered-body vapor plus explicit motion history in the same case |
| Covered-body vapor mass potential through motion | kg | Covered-body vapor plus explicit motion history in the same case |

Limits remain active without an objective. Body/layer heat retains its sign; vapor potentials and
their guided limits are nonnegative. All seven metrics support nominal and source-reported-bound
decisions through the existing comparison and limit bases. Complete admitted intervals must
satisfy bounded limits; missing ranges keep the nominal output but exclude the bounded decision.
Nonlinear thermal laws, source-dependent gaps, exterior exchange and thermal bridges can retain
nominal heat while preventing a source-bound comparison. No geometry or boundary uncertainty is
invented. These goals and guards also enter existing grid and adaptive design discovery.

Body/layer heat may include declared bare-body paths. Covered vapor comparisons require complete
coverage of the declared body domain; a partial covered subtotal is inspectable but ineligible.
Sheet and covered-body totals describe different domains and must not be added. Vapor quantities
are transfer potentials under the supplied pressures, films and material evidence; they do not
establish available sweat, actual evaporation, cooling feedback or physiological comfort.

For moving totals, choose **Retain an original motion request** and supply
`motion.vapor_coverage_history` with schema `ftx-motion-vapor-coverage/1`, explicit `right_endpoint`
or `composite_trapezoid` integration and `max_geometry_operations`. The allowance is an integer
from 1 to 20000000; each admitted history reserves it against the engine's shared 40000000-operation
session limit. Enable covered-body vapor in the same case. Saved form endpoints alone request no
transport history. Complete-history comparisons require coverage and admitted source data at the
initial state and every accepted sample. A history refusal preserves independent static outcomes.

Each input and comparison policy can be frozen in a named case. Active-source changes cancel an
obsolete read or run; selecting an inactive source leaves it untouched. Draft changes leave saved
cases intact. Loading or adopting another pattern disables these garment-specific policies.

#### Compare finite body-water outcomes

The **Finite body water through motion** group compares modeled evaporation, remaining liquid,
unmet vapor demand, prescribed supplied water and prescribed-body latent energy. Each has an
explicit minimizing or maximizing direction and optional inclusive lower/upper limits in kg or J.
Limits and **Require a complete finite body-water history** apply independently of goal checkboxes.
These criteria also enter existing linked grid and adaptive design comparisons.

Choose **Retain an original motion request** containing
`vapor_coverage_history.wet_film` with schema `ftx-body-wet-film/1`. The film retains its source and
basis, the exact selected initial body-mesh identity, and nonnegative `initial_liquid_mass_kg` and
`supply_rate_kg_s` arrays in the coverage policy's declared body-triangle order. Enable covered-body
vapor in that same case. A missing or null film is not a dry-film assumption. Saved endpoints alone
do not request a film, and separate matrix cases cannot supply one another's dependencies.

All five quantities support nominal or conditional final source-reported bounds. Complete coverage
is required initially and throughout the admitted trajectory. Each triangle conserves its own
initial and supplied water; different vapor potentials can yield the same evaporation after both
materials exhaust the available liquid. Bounded comparisons replay the source Ret endpoints with
water, supply, geometry, boundaries and integration fixed. Remaining-liquid bounds reverse the
demand endpoint order. Declared supply has a singleton range even without Ret bounds, but still
needs complete nominal history admission. Missing bounds retain nominal readings without admitting
the other bounded decisions. No per-sample water ranges or general water-state uncertainty are inferred.

Named cases freeze the exact original film and comparison policy. Active motion-source or criterion
edits cancel stale work; source-domain refusal preserves independent static heat, mechanics and
manufacturing results. Malformed typed requests can still fail native deserialization. This model
describes body-owned liquid under prescribed reservoirs, not textile-owned liquid, physiological
sweating, actual wearer cooling or textile-temperature feedback. See the
[native finite-water contract](material-scenario-studies.md#finite-body-water-through-complete-motion).

#### Compare textile-owned water and latent heat

The **Textile water and latent heat through motion** group uses the existing conservative
independent-sheet heat/water history. Retain an original motion file containing
`transient_surface.surface_water`; the same single or named case must request motion, that transient
surface, and its actual water policy. Body-film, steady-heat and standalone vapor policies cannot
substitute for it. The ordinary dry transient-surface workflow remains independent.

| Guided quantity | Unit | Native metric |
|---|---|---|
| Evaporated textile water | kg | `textile_water_evaporated_mass_kg` |
| Remaining textile liquid | kg | `textile_water_remaining_mass_kg` |
| Supplied textile liquid | kg | `textile_water_supplied_mass_kg` |
| Condensed textile water | kg | `textile_water_condensed_mass_kg` |
| Drained textile water | kg | `textile_water_drained_mass_kg` |
| Evaporation latent-energy removal | J | `textile_water_latent_energy_j` |
| Condensation latent-energy release | J | `textile_water_condensation_latent_energy_j` |

Choose each objective's direction explicitly and optional nonnegative inclusive lower/upper limits.
Limits stay active without their objective. All seven criteria use **nominal values only**; choose
an explicit nominal limit basis when other objectives use reported bounds. Each selected quantity
requires the complete admitted water history through the existing `transient_surface` consumer.
The ordinary **Require a complete independent-sheet heat history** control remains usable without
selecting a water quantity. No separate `textile_water` consumer or uncertainty range is invented.

Native qualification checks each material, instance, source, phase and operating domain. Supply,
condensation and drainage quantities are unavailable when their supporting policy is absent; an
admitted phase can legitimately report zero transfer. Evaporation removes latent energy and
condensation releases it, while drainage exports sensible energy without a latent term. Retained
liquid and temperature share the producer's complete mass and energy balance. Failed or partial
histories cannot qualify a water decision or become zero-valued readings.

The shared scenario can retain a single `surface_water` qualification,
`surface_water_alternatives` for independently qualified thermal assignments, or
`coupled_water_alternatives` for complete water/mechanical qualification pairs. Each collection
matches every retained instance to its exact selected material hash, enabling material comparisons
under the same initial loading and operating conditions. Paired sources also preserve the mechanical
transfer model and incoming/outgoing phase velocities, and bind the complete actual mechanical
material report before liquid changes inertia.

Each alternative retains its own source qualification and loading domain. A missing or ambiguous
uncoupled match refuses the wet result; a failed paired qualification refuses coupled motion.
Heat results display the selected qualification, and paired results display both water and
mechanical sources plus their shared pair digest. Original source hashes and the complete shared
scenario remain unchanged. These forms also support **same-material design searches**, recomputing
geometry, reference-area water inventory and current-area transport for every admitted design.
The native paired comparison also validates its complete coupled history and thermal/vapor source
protocols when only a motion, strain or contact objective is requested.

Saved cases retain the entire original motion source and comparison policy; active source/criterion
edits cancel stale work. Water affects mechanical mass or momentum only through an explicitly
requested, independently admitted mechanical-coupling policy. Without that coupling, a late thermal
refusal preserves independent motion; a failed coupled solve follows the native whole-motion rollback.
This free-liquid model does not infer sorption, wicking, body-to-textile transfer, wet constitutive
properties, wearer cooling or empirical wet-garment performance. See
[textile-surface-water.md](textile-surface-water.md) for the existing source and phase contracts.

#### Compare heat storage and temperatures through motion

Enable **Track dry independent-sheet heat through this movement** in the endpoint editor, or choose
**Retain an original motion request** with an explicitly authored `transient_surface` or
`transient_coverage` policy. The two heat-history groups expose the existing native results as
objectives and inclusive lower/upper limits. Each domain has six independent quantities:

| Quantity | Unit | Interpretation |
|---|---|---|
| Inside-side energy | J | Signed integrated exchange at the inside boundary |
| Net boundary energy | J | Signed total physical boundary exchange |
| Minimum sampled temperature | °C | Minimum over the initial state and retained mechanical samples |
| Maximum sampled temperature | °C | Maximum over the initial state and retained mechanical samples |
| Final minimum temperature | °C | Minimum at the final accepted mechanical sample |
| Final maximum temperature | °C | Maximum at the final accepted mechanical sample |

Select every objective's direction explicitly. Negative energy and temperature limits remain signed;
minimizing signed energy does not minimize its magnitude. Limits apply without their objective, and
each history can also be required independently. These quantities support **nominal values only**.
When combining them with source-reported mass or steady-heat objectives, choose an explicit nominal
limit basis. No transient uncertainty interval is inferred.

The same case must request motion and its corresponding transient policy. A transient sheet does
not need an unrelated steady-sheet request, and transient coverage carries its own declared layer
topology without requiring steady coverage. The original motion text retains initial temperatures,
boundary schedules, source numbers and numerical allowances exactly. Saved cases freeze that text.
The endpoint form emits a dry sheet history only when explicitly enabled with a complete saved
schedule. These criteria also enter existing linked
grid and adaptive design comparisons under each case's own admission policy.

For a guided dry schedule, enter the default initial textile temperature after the initial drape and the
inside/outside reservoir temperatures and film resistances for each endpoint, then **Add motion
keyframe** or **Replace keyframe … from current controls**. The first boundary applies from time
zero to the first endpoint; later boundaries apply after the preceding endpoint through the saved
endpoint. Conditions are constant over each interval. They are not interpolated temperatures,
inferred weather, skin conditions or an equilibrium result. Negative Celsius values are allowed
down to absolute zero. Zero film resistance explicitly prescribes the corresponding face temperature.

Open **Different initial temperatures for individual garment instances** when, for example, a cold
outer garment starts separately from a warm inner sheet. Enter an exact retained physical instance
name and its temperature, then **Add or replace initial instance temperature**. Each saved override
initializes that sheet uniformly through its thickness. Two instances can start at different
temperatures while sharing the same material. Unlisted instances use the explicit default; the
default remains required even when every retained instance has an override. Nothing is inferred
from a material name, source-piece shortcut, current reservoir, body temperature or equilibrium.

Override name/temperature fields are drafts until saved. Remove an individual override or choose
**Use default for every instance** to remove the map. Saved overrides survive heat off/on, remain
separate from each immutable named case and clear when the original pattern changes. Editing a saved
override cancels obsolete single-case work or a pending case save; later draft edits cannot alter a
captured request. The engine verifies the actual retained instance and its material's temperature
domain. Unknown or unretained names refuse the heat consumer without erasing independent results.

The native request retains the optional `initial_instance_temperatures_c` map, its explicit scalar
fallback and the complete original policy. A successful mapped result shows the resolved temperature
of every retained instance and its source-bound triangle ownership. Initial extrema use the actual
retained temperatures, excluding an unused fallback. This bounded extension supports dry independent
sheets, including the original-file exterior policies. Original motion files can also set this map
on `motion.transient_coverage`: covered layer cells and exposed remainders retain each source
instance's initial temperature as the garment moves. The page displays that consumer's own resolved
temperatures, source model and initial frame. Saved cases and replay files retain the complete
original motion policy.
The guided sheet controls above author only the sheet policy. Water, mechanical liquid coupling and
dissipation heating require a separate supported contract and refuse when combined with these maps.

Every saved endpoint owns its four boundary values. Editing the current endpoint or film fields
prepares a draft; only adding or replacing an endpoint changes the saved schedule. Removing an
endpoint also removes its boundary, and the next endpoint retains its own conditions over its new
interval. If heat is enabled after poses have already been saved, explicitly replace each endpoint
that lacks conditions; no earlier endpoint is silently filled from the current fields. Disabling
heat preserves the saved conditions for reactivation. Default initial temperature, cell count and thermal
work limits apply to the active draft immediately; each saved named case retains its own snapshot.

Use 1–64 cells through each sheet, a maximum thermal step from 1e-9 to 1 second and a history
allowance of 1–10000000 temperature-cell advances. The native engine qualifies every retained
sheet's source-bound heat storage and transport, enforces its temperature domain and reserves each
history allowance against its case's shared thermal budget. Larger allocations can leave fewer
optional histories available across material alternatives; the authoring form does not reject the
whole comparison when an optional heat consumer may exhaust its budget. Heat refusals preserve
independent manufacturing and mechanical outcomes. This form authors dry independent sheets;
coverage topology, water, exterior exchange and dissipation heating use the original-motion route.

Native comparison requires a complete, source-bound history. Missing heat capacity, failed motion
or refused thermal work cannot become zero energy or a qualifying temperature. Independent available
manufacturing and static results remain inspectable. Sampled extrema are not continuous-time bounds,
and computed cloth temperature does not establish wearer comfort or a garment temperature rating.

#### Track material-direction strain

Enable **Track warp, weft and shear on the actual cloth triangles** to use all retained physical
instances or exact instance names from the drape report. The six initial and six complete-motion
extrema expose signed warp E11, weft E22 and engineering shear 2E12 in dimensionless Green strain.
They are distinct from the panel-edge percentage metric. Raise selected minima or lower selected
maxima, and enter independently justified lower or upper limits. These metrics support nominal
decisions; use explicit nominal guards alongside source-reported heat or mass objectives.

The declared triangle-sample allowance covers the initial state and every accepted motion sample.
Each alternative reserves its allowance against the shared 64000000-sample budget, including across
named cases. Missing instance scopes and exhausted budgets refuse the affected consumer while
independent results remain available. Required initial and complete-motion strain consumers can
exclude an alternative without adding an objective. No failure or comfort threshold is supplied.

#### Constrain body contact loads

Enable **Track normal body forces on retained cloth** to select the whole retained garment or exact
physical instance names from the drape report. Declare both vertex-sample and candidate-pair work
allowances. Each case reserves these maxima across its alternatives, with separate limits of
64000000 vertices and pairs. Scope selection does not reduce the complete geometry needed for
contact evaluation. Missing instances or exhausted work refuse the affected load result while
independent drape, strain, heat and motion outcomes remain visible.

Four ordinary comparison criteria minimize or limit the initial resultant magnitude, initial
maximum vertex force and their peaks across complete accepted motion. All use newtons and nominal
values. Motion criteria require motion and contact-load tracking in the same case; initial or motion
results can also be required without choosing a force objective. Use explicit nominal limits with
source-reported heat or mass objectives. Saved named cases retain their exact scope and budgets,
and the same criteria can guide existing grid and adaptive design discovery.

Initial and accepted motion samples share each evaluation's allocation. Native partial results stay
available: an initial reading can survive an exhausted motion-load request, and later alternatives
can lose only their optional force results. The authoring controls do not reject a whole study just
because its probes and alternatives would exhaust those optional per-case reservations.

These are discrete normal body forces. Opposing forces can cancel in the resultant, shared welded
vertices keep their complete force, and nodal maxima depend on the mesh and contact policy. No
contact area, pressure, tangential friction, physiological comfort or safe-fit threshold is inferred.

#### Continue the same sewn garment through movement

Enable **Continue the sewn garment through a prescribed body movement**. Declare an endpoint's
cumulative time after the initial drape, absolute arm angle and standing, seated or squat leg pose,
then choose **Add motion keyframe**. Seated and squat endpoints also require an explicit fraction
of that model pose. Returning to standing is an explicit endpoint choice; poses are not incremental.
An unchanged pose at a later time is a hold. No movement is supplied automatically.

Alternatively, choose **Retain an original motion request** and select `ftx-drape-motion/1` JSON.
The complete original text, including transient thermal/water requests and source numbers, is
retained for native admission and replay. This mode does not need saved form endpoints. Its case
motion-attempt allowance remains explicit; the form's inactive trajectory limits do not change it.
Switching sources retains both drafts and withdraws obsolete work.

The list supports explicit replacement from the endpoint controls and removal. When dry sheet heat
is enabled, each endpoint also saves its own reservoir and film conditions described above. Editing
those controls prepares another endpoint without changing the saved path. Disabling motion retains the
draft. Removing, replacing or adopting a pattern clears its saved endpoints, cases and strain scope. A single scenario uses the added endpoints; a named case
freezes its own path and work limits when saved. Later draft edits do not change that saved case.

The existing engine accepts 1–16 endpoints, increasing by at least 1e-9 seconds and ending within
60 seconds, arm angles from 0 to 180 degrees and leg fractions from 0 to 1. Set the maximum implicit
substep, attempted-substep allowance, collision-pair allowance and retained-vertex allowance
explicitly. The trajectory needs enough attempted steps even for its nominal subdivision; actual
collision/solver refinement can use more. The case-wide motion allowance is shared across material
alternatives and discovery probes. Active named cases share at most 32768 motion attempts; this
does not replace their separate vertex-frame and case-evaluation budgets.

Use **Minimize peak absolute panel-edge strain through movement**, its inclusive maximum, or
**Require a complete numerically admissible movement** in the ordinary comparison. Every case
inheriting such a policy needs motion. Per-case policies can instead compare a static case and a
moving case under independent goals. The existing four adaptive guidance modes consume the same
motion objective and guards. These computed strain comparisons are nominal; no propagated strain
range is invented.

The production solver continues cloth positions, velocities and material history on the same
selected body shape. Its fixed-root body path follows checked vertex chords between endpoints,
without inferred chair/floor support or anatomical movement qualification. Use short explicit pose
increments. A failed path retains the independent static result and publishes no partial trajectory.
Existing motion inspection shows the accepted body/cloth samples at their recorded times. Optional
sheet heat gives independent steady endpoint snapshots, not transient temperature evolution.

#### Save several named cases from the ordinary controls

Choose **Save and compare several named cases here** to prepare 1–8 scenarios on the same selected
body and initial pose. Configure a case using the fit, drape, heat, vapor, strain, contact-load and movement controls,
give it a unique name, and choose **Save current settings as a named case**. Each saved case freezes
the exact original fit, heat and vapor text, declared strain/contact scopes and numerical options, and saved motion endpoints
or original motion file. You can change those
source files and settings to prepare the next case without changing any saved case.

For each case, choose one comparison policy:

- Inherit the shared goals and limits as they stand when you run the comparison.
- Save the current guided goals, limits, bases and required consumers with the case. This lets one
  case keep a minimizing heat goal and another keep a maximizing goal without authoring JSON.
- Retain an original case comparison-policy file, preserving its source text and full engine policy.

The saved-case list shows which policy each case uses. **Replace … from current controls** explicitly
updates that case's source snapshot while preserving its name. Remove and reorder actions change
the matrix order; these changes withdraw obsolete reports and any nominated adaptive case must be
chosen again. A refused or cancelled save leaves existing cases intact. Draft-only changes do not
invalidate a finished matrix; the selected body, material/design choices and inherited shared
comparison remain live dependencies.

Set **Total case evaluations allowed** to cover the complete comparison, including adaptive probes
when requested, with a maximum of 64. The cases' vertex-frame allowances must total at most
64000000. The existing single-case default consumes that whole allowance; explicitly choose, for
example, 32000000 each for two cases. Nothing divides or truncates budgets automatically. All
cases share the current selected body, and each alternative still receives a fresh scenario state.

The scenario action runs only the saved cases in this mode. Its `scenario-request.json` contains
the complete replayable matrix and the exact retained source fragments. Switching between saved,
single-scenario and original-file modes preserves their separate requests. Loading or adopting a
different pattern clears saved cases so another garment cannot silently inherit them. The guided
forms author initial drape, prescribed movement, uniform sheet heat/vapor conditions and dry
sheet-heat schedules. Original source files retain layered, exterior and coupled policies, and
advanced motion requests carry the remaining explicit vapor and transient histories unchanged.

The focused compiled-engine proof uses a small synthetic sewing-only fixture with zero gravity
steps. Five complete native/direct-WASM/worker comparisons cover signed heat direction, explicit
maximums, aggregate budget exhaustion, scenario-only goals and independent mass after a mechanics
refusal. This is numerical software evidence, not gravity-settled garment or physical-fit evidence.
The runnable `tools/check_materials_web.cjs --guided-scenario-proof DIR` mode adds real-browser
replay coverage; the session's Chromium attempts crashed before reaching its new assertions, so
that browser proof remains unverified.

Material and source data requirements are unchanged. See `material-sewing-plans.md`,
`material-platform-progress.md`, `steady-conduction.md`, `steady-thermal-coverage.md`,
`exterior-thermal-exchange.md`, and `textile-price-refresh.md` for their input semantics and limitations.
The UI deliberately asks for the existing JSON contracts rather than inventing convenient physical
constants. The synthetic sewing pattern/options under `tests/fixtures/materials/` illustrate process
inputs, not a real qualified textile or recommended machine configuration.

Sewing recipes may opt into `ordering` with `objective: "minimize_setup_changes"` and an explicit
`max_search_nodes` work limit. The existing Rust planner counts needle and thread changes separately
and minimizes their sum while honoring declared predecessors. An optional `initial_setup` names the
initial needle and thread; without it, the first installation is excluded. The workbench displays
baseline and selected counts, the selected operation sequence, and whether optimality was established
or the search stopped at its budget with a complete feasible schedule. The downloaded stitch program
retains the same result and source identities. This count does not infer handling time, undeclared
assembly access, machine calibration or permission to execute hardware. Editing the recipe withdraws
the old program and optimization result together.

An operation may also select `source_range` on its original side-A seam. The planner derives side B
through the existing notch/offset correspondence and checks complete coverage of both source sides.
This allows separate constant-layer recipes for a plain section and a folded or reinforced section
of one join. The workbench displays both source ranges, full original lengths and selected traveled
lengths even without an ordering policy. Reverse feed, backtacks, qualification and clearance keep
their ordinary checks. See the runnable split-seam example in
[material-sewing-plans.md](material-sewing-plans.md#partial-seam-operations-and-local-construction-changes).

Purchasing also accepts `ftx-textile-quotes/2` histories with exact UTC `observed_at` instants and an
explicit market `as_of_at`. Retain the matching `observed_on` and `as_of` dates. The result displays
the Rust evaluator's latest eligible and excluded observations for each roll, their exact times,
source expiry, rounded order quantities and merchandise costs. The source detail retains the
selected order and stock evidence. An unavailable, expired or insufficient latest observation
cannot revive an older offer. Version-1 books remain date-only and display that limitation;
the browser never supplies a clock value, chooses a different offer or recomputes a price.
Changing the quote book or market withdraws the old result. Input JSON and downloaded reports
retain their original numeric text. See [source refresh](textile-price-refresh.md) for creating
version-2 snapshots with `--intraday` and the supported timestamp precision.

Optional material `optical_faces` assignments survive inspection, freezing, piece binding and the
retained drape receipt. In the existing heat-condition JSON, set
`radiation.automatic_material_face: true` and omit explicit face/property strings. Independent
sheets use their canonical positive outside face; layered coverage selects the actual outer side
from cloth/body normals. The result retains the chosen observation and material identity.
No face editor or missing optical data is supplied by the page. See
[material optical faces](material-optical-faces.md) for the source, specimen, spectral and
temperature qualifications and a runnable synthetic stack. The same stack request is available
through the exported `thermal_steady` WASM API; it is not an additional workbench upload mode.

## Potential evaporative transfer

Section 1 accepts a standalone `ftx-vapor-stack/1` request through **Calculate potential vapor
transfer**. It needs no bound pattern or mechanical/heat-storage capability. The request owns its
ordered material selections, common area, boundary vapor pressures, positive film resistances,
latent-heat conversion and source-admission policy. The worker calls the same `thermal_vapor`
WASM export as the native consumer; JavaScript performs no transport calculation.

The result displays total resistance, potential latent heat/water flux and flow, film pressure
drops, every layer's face pressures and its original measurement method/conditions/source.
Source conditions are retained observations, not an inferred operating domain. Mixed protocols
and estimated observations require the existing explicit permissions. Potential transfer does not
determine available sweat, actual wearer cooling, condensation, comfort or a dry/wet heat history.

Downloads preserve the exact Rust `vapor-report.json`, original `vapor-request.json` and
`vapor-replay-request.json` with complete frozen material selections. Running this calculation
preserves the independently inspected material; it creates no replacement material selection.
Replacing or refusing a request withdraws its previous results, and delayed reads or stale worker
replies cannot publish them. See [vapor-resistance.md](vapor-resistance.md) for the schema, equations
and a runnable measured-leather example with explicitly illustrative boundary assumptions.

## Identify material behavior from local observations

Choose the matching response, load the source-bearing JSON and run **Identify material behavior**.
Section 1 supports these existing acquisition contracts:

| Response | Request | Prepared output |
|---|---|---|
| Linear orthotropic stretch and shear | `ftx-swatch-calibration/1` | Source-locked material selection |
| Directional bending and preferred curvature | `ftx-bending-calibration/1` | Source-locked material selection |
| Kinetic friction between two materials | `ftx-friction-calibration/1`, `target.kind: "pair"` | Exact material-pair interface; both material snapshots remain unchanged |
| Same-material kinetic friction | `ftx-friction-calibration/1`, `target.kind: "self_contact"` | Material selection with the identified `friction_self` |
| Kinetic friction against a declared body counterface | `ftx-friction-calibration/1`, `target.kind: "body"` | Material selection with the identified `friction_skin` |
| Nonlinear stretch, shear and optional area response | `ftx-nonlinear-swatch-calibration/1` | Source-locked material selection |
| Relaxation, creep and recovery | `ftx-relaxation-calibration/1` | Source-locked material selection when identification is accepted |
| Permanent stretch and hardening | `ftx-membrane-plasticity-calibration/1` | Source-locked material selection when identification is accepted |
| Clamped strip forces and transverse widths | `ftx-strip-calibration/1` | Source-locked material selection when identification is accepted |

The shared Rust calibrators own the models, fitting, evidence admission, identifiability and residual
gates. The page displays their results and retains the supplied measurements unchanged.

Linear swatches supply observed deformation gradients and nominal forces per reference width. The
result shows the orthotropic membrane stiffness matrix in N/m, its FIT-only Green-strain domain,
excitation diagnostic and separate FIT/CHECK residuals. CHECK observations cannot enlarge that
domain. Directional bending supplies measured curvatures and moment resultants in the material
frame. Its result shows the bending tensor in N·m and preferred curvature in inverse metres;
`fit_preferred_curvature: true` identifies unloaded curvature when the observations support it.
Panel bending leaves independent seam bending and unrelated material properties unchanged.

Friction requests supply corrected steady sliding forces, positive normal contact loads, nonzero
slip velocities, acquisition sources and independent FIT/CHECK acquisitions. The selected browser
mode must match `target.kind`. Body and self-contact fits enter ordinary material assignment;
replacing an existing contact property requires `replace_existing: true` in the request. The body
counterface remains explicit: a surrogate measurement does not establish human-skin behavior.
Results show the effective isotropic kinetic coefficient and observed FIT load/speed ranges. Those
ranges do not enforce a runtime operating domain or establish static-friction behavior.

A **pair** fit preserves the independently inspected material selection, including during pending
or refused fitting. It prepares a separate source-bound interface with its own estimate admission.
The result reports interface readiness separately from each sheet's mechanical readiness; even an
admitted interface does not establish complete material mechanics or whole-garment readiness.
To apply the pair:

1. Assign the exact source material snapshots to the garment's default or relevant source pieces.
2. In section 2, choose **Stage this calibrated contact pair**. Staging uses the current pending
   interface table when supplied, including an explicitly empty table; otherwise it uses the
   original pattern's table. It preserves the other pairs and validates every pair against the
   final assigned material hashes.
3. If that exact pair already has evidence, explicitly select **Replace existing evidence for this
   exact material pair** before staging. Replacement consent resets after successful staging.
4. Use **Freeze selections & bind** to apply the staged pair with the current material assignments,
   or **Save current assignments to comparison** to retain the complete configuration as a guided alternative.

Successful staging withdraws obsolete bound outputs. A refusal or stale response cannot replace the
previous pending interface table. Staging retains valid evidence independently of consumer admission:
an unadmitted fitted estimate remains unavailable to the mechanical contact consumer. Pair downloads
include `material-calibration.json`, `calibration-input.json`, `calibrated-interface.json` and
`friction-binding.json`, with the exact Rust strings retained for reuse. Body and self-contact fits
also provide `friction-binding.json` alongside their ordinary material-selection download.

Timed requests preserve their independent long-time equilibrium. Directional recovery accepts either
fixed `orthotropic.times_seconds` or one to three nonoverlapping `orthotropic.time_bounds_seconds`
windows for identifying the times. Omit or leave empty the other field; supplying both is refused.
Proportional ratio/time search uses its separate top-level time bounds.

For permanent stretch, supply complete pristine-start loading/unloading/reversal histories in raw
Green strain and second-Piola sheet resultants. Missing force channels remain null, while every
recorded strain node still advances the existing permanent-state law. The material supplies matched
elastic coefficients and source-bearing total/accumulated strain limits; explicit search ranges
bound identification of all three yields and all three hardening values. Those ranges do not define
confidence intervals or replace the independently supplied applicability limits.

The result displays the selected FIT parameters, original search bounds, independent partition
errors, held-out status, actual work and excitation diagnostics. Accepted, ambiguous, rejected and
budget-exhausted outcomes remain distinct. A failed CHECK retains the same FIT choice rather than
promoting another candidate. Nonaccepted outcomes clear the previous selection and offer only the
complete report and original request for download; accepted results additionally publish the normal
source-locked selection for existing binding and drape consumers. Complete histories, permanent and
accumulated strain, stored energy, dissipation and force residuals remain inspectable. See
[material-membrane-plasticity.md](material-membrane-plasticity.md) and its deliberately synthetic
calibration fixture; no source identity or observed force is inferred from a fiber or brand label.

The **Admit the fitted material or interface estimates** checkbox is off initially. It sets the
returned material selection's estimate policy or the fitted pair interface's own policy. Pair
admission never changes either sheet's existing estimate consent. Estimated input observations,
estimated equilibrium and replacement of an existing
response require their separate explicit declarations in the uploaded request. A successful fit can
therefore remain unavailable to simulation; unrelated missing mass, bending and contact evidence
also remain visible through the ordinary material capability report.

Results separate fit and held-out component counts, scaled RMS/peak errors, their requested budgets,
and the applicable Green-strain domain. An absent held-out partition says **not supplied**. Nonlinear
domain endpoints come from fitting observations; timed applicability is source-declared. Complete
observations, predictions, residuals and fitted recovery branches remain inspectable. These quantities
are not confidence intervals or independent garment validation.

For directional recovery, the table reports each time and its published warp, weft, normal
coupling and shear tensor entries in N/m. Fixed candidate times remain supplied inputs, not
automatically optimized. In this fixed mode, inactive candidates remain visible with their explicit
zero coefficients; an entirely inactive spectrum retains the independently supplied equilibrium response.
Searched times each require a retained observable recovery branch; individual directional coefficients
may still be zero. When time windows are
declared instead, the page shows the identified time beside both bounds, its scaled force/log-time
sensitivity with fitted tensors held fixed, and its distance to the closest window boundary as a
fraction of the log-time window.
A fraction of zero is an endpoint and 0.5 is the center. The local optimizer table reports converged
and attempted starts, iterations in the selected solve, total objective evaluations, projected tensor
iterations across all solves, maximum scaled gradient and joint stiffness/time excitation pivot.
The gradient describes the objective slope after fitting the tensors; the pivot describes the
column-normalized FIT information for tensor coefficients and log-times. These diagnostics accompany the held-out errors;
they do not establish a global optimum, a unique spectrum or statistical confidence.

The full report separately preserves
unconstrained, physically constrained and explicitly thresholded values. Fit and held-out errors refer
to the final published law, including threshold decisions. Small errors do not establish a unique
spectrum or extend its source-qualified strain applicability.

`tests/fixtures/materials/synthetic-orthotropic-relaxation-search.json` illustrates bounded time
identification with independent directional responses. It is an artificial software fixture, not a
measured textile characterization.

For material-producing fits, the generated selection is ready for the same default or source-piece assignment and binding steps.
Its exact Rust `selection_json` is retained through assignment and download, preserving numeric source
identities. Download `material-calibration.json`, `calibration-input.json` and `calibrated-selection.json`
for replay. Changing the response, uploaded observations or fitted-estimate consent withdraws the
current characterization and its prepared material selection or interface. A pair fit leaves the
independently inspected material intact. An explicitly assigned frozen material or successfully staged
interface remains an independent pending choice. Loading a pattern preserves completed characterization downloads.
Cancellation, newer selection edits and failed replacement fits cannot republish a previous result.

## State, privacy and failure behavior

The original pattern is retained separately from pending assignments and the bound result. Assigning
another material immediately withdraws the bound output and all dependent drape/heat/process/cost
outputs. A change to body or measurements withdraws drape and heat; changing only thermal conditions
withdraws that heat result but preserves mechanics. Changing stock options withdraws cutting and cost;
quote/market changes withdraw cost only. Results are explicitly current-input results, not an unlabeled
comparison with an old run. Previously downloaded files remain the user's own saved versions.

Each worker request includes the necessary current inputs. Epoch and request identities discard late
responses, and revision checks discard stale asynchronous file reads. Cancel terminates the worker,
including synchronous Rust calculations, and starts a new one. Failed replacements do not republish a
previous success. Temporary WASM body/fold/export objects are freed on both successful and failed
operations; a WASM trap requires a fresh engine. Withdrawn download URLs are revoked. Inputs remain
in this tab: no localStorage, sessionStorage, indexedDB, server upload, device connection or background
refresh. The existing fold viewer uses a Three.js CDN, explicitly disclosed on the page; its sandbox
cannot access the parent workbench. Closing the tab drops this unsaved session.

JSON files are decoded as strict UTF-8. Invalid encoding, a leading BOM or malformed JSON refuses
before dispatch; source text is never repaired or reserialized by the reader. Valid Unicode,
whitespace, integer spelling and signed zero remain exact. Both declared file size and actual bytes
are checked against the input limit. Accepted body-fit reports retain their separate 16 MiB limit.
Combined JSON requests are limited to 64 MiB and each body-model input to 256 MiB, followed by the
existing native geometry, candidate, solver, stitch and allocation limits. Ordinary incomplete data
produces the specific native refusal rather than a generic success. A numerical result is not empirical
textile validation, a real-person fit claim, a warmth/safety rating or a hardware approval.

## Checks and delivery scope

`cargo test -p ftx-wasm --test materials --test manufacturing` exercises the actual shared Rust
binding, downstream material resolver, cutting, SVG, sewing and procurement APIs. Seven new regressions
cover source freezing, geometry and explicit-piece preservation, width-driven stock changes,
thickness-driven sewing refusal, separate partial-record capabilities, missing pair evidence and
combined input budgets and lossless floating-point JSON identity. Existing manufacturing tests remain independent.

`node --experimental-vm-modules --test tools/check_trace_frontend.cjs` includes ten new tests of the
actual material page and worker with **explicit DOM/WASM doubles**. They test lifecycle and input
routing, not physical results or browser execution: stale worker messages/file reads, catalog identity
changes, cancel, dependent-output withdrawal, native-handle cleanup, initialization/trap failures,
blocked exports and page-exit download cleanup.

`node --experimental-vm-modules tools/check_materials_wasm.cjs --native target/debug/ftx-materials`
executes the **actual compiled browser WASM and actual worker** under Node, without a mocked numerical
engine. It compares binding (including negative-zero coordinates), stock, SVG, sewing, procurement,
and source-locked thermal surface/coverage results against the native executable. This is runtime
WASM/native agreement on those cases, not browser navigation, fitted-body or empirical validation.

`tools/check_materials_web.cjs` is the separate real Chromium/WASM lane:

```sh
sh tools/build_web.sh
cargo build -p ftx-cli
NODE_PATH=$(npm root -g) node tools/check_materials_web.cjs --native target/debug/ftx-materials
```

It needs the Playwright Node package and Chromium (or `CHROMIUM_EXECUTABLE` pointing to an installed
Chromium). It compares browser binding, cutting, preview, sewing and procurement with the real native
binary and tests changed-material refusal/output invalidation. Source-locked synthetic thermal reports
exercise the actual worker/WASM surface and coverage APIs against native results. They are explicitly
not freshly fitted body drapes. The existing `tools/check_web.cjs --model PATH.ftxbody` lane remains
necessary for successful model-backed browser draping; no model asset or private wearer record is
bundled by this feature.

This delivers the local operator integration slice of plan §25.9, including source-bearing mechanical
calibration, named scenario comparisons, transient heat and motion histories, and potential vapor and
finite body water histories through the shared native engines. Each result retains its own evidence,
physical scope and refusals. It does not close the full empirical linen/leather/knit garment gate,
acquire new measured records, qualify live supplier coverage, or provide geometric body/layer-map
painting or robot execution. Declared numeric coverage policies are editable through the controls
above; geometric region picking and physical qualification remain separate capabilities.
