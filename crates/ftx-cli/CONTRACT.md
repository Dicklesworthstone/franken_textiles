# ftx-cli contract

`ftx inspect-pixel ROUND_DIR --view VIEW --pixel X,Y --json` casts the saved still's ray through
the body and garment endpoint. Pixels are zero-based from the original PNG's upper left. A cloth
hit names the physical instance, authored piece, nearest sampled boundary in posed 3D, its distance
in mm and the authored edge endpoints in flat millimetres. A body or background hit has no pattern
edge. The round retains its original gating flags. Older completed rounds with native stills work
from their existing OBJ files and mesh map.

After a solver refusal, pick from `sim/refused/still_VIEW.png`. If the initial placement was
rejected, pick from the view listed in the placement report at `sim/refused/start/still_VIEW.png`.
The original `refusal` accompanies the hit. The diagnostic map identifies `refused_pose.phase` and
`accepted_steps`. A rejected placement has phase `placement` and zero accepted steps. Archived
views without a source map support visual comparison and body or background picks. A garment
edge requires the map from that same stage.

**Initial drafting from a selected body.** `ftx draft PROGRAM --model PATH` fits
the supplied tape evidence; `--body-fit REPORT [--model PATH]` reuses the selected
accepted shape without refitting. Both supply dimensions and placement through
the shared drape/pattern authority, retaining style/material selection in `--out`
and `--ws` modes. Omitting both flags retains ordinary analytic drafting.
Standalone output requires a new directory; workspace output reserves a new round
only after source and drafting checks. Both add `draft_body.json`,
`validation_body.json` and flat `validation.json` on the selected body. Accepted
imports also preserve exact `body_fit.json` bytes; tape fits invent no such report.
Original measurement evidence and readiness are unchanged. Bolero hand passage and
trouser foot passage require separate supplied evidence; unavailable anatomy and
invalid model/report/demographic associations refuse without tape or chart fallback.
See `docs/reuse-body-fit.md` for commands, the trouser rise basis and source limits.

**Later flat checks.** `ftx validate` and `ftx round` explicitly select a model body
with `--model PATH` and/or `--body-fit REPORT`. They reuse the shared drafting body's
reference measurement pose for dimensions and landmarks, including separately supplied hand/foot
passage evidence when the fit contract requires it. A pattern carrying `meta.draft_body`
requires an explicit body selection on later checks; it cannot silently revert to
analytic measurements. Historical drafting identity stays intact, while the current
body, model hash and residuals are recorded separately as `validation_body`.
Rounds save `validation_body.json`, retain accepted report bytes as `body_fit.json`
when supplied, and expose both in their review without changing measurement readiness.
For ordinary patterns, existing `round --sim|--stills --model|--body-fit` keeps its
simulation-only body selection. Add `--flat-body` to select that body for flat checks
too; this flag can explicitly use the verified default model when no path is supplied.
An accepted flat check and simulation consume the same retained report snapshot.
Flat source/model admission happens before reserving a round; simulation rechecks
the model hash against that selected flat body and records a refusal if the file
changed. Model predictions establish neither new measured evidence nor physical
garment fit.

**Purpose.** The `ftx` binary (plan §14): the reference's `fab` commands, so the skill's scripts can call
FrankenTextiles subsystem by subsystem as each reaches parity. It provides `ftx draft`, `ftx validate`,
`ftx drape`, `ftx qualify`, `ftx body`, `ftx alter`, `ftx measure`, `ftx export`, `ftx check`, `ftx trace`,
`ftx annotate`, `ftx scale-fit`, `ftx audit`, `ftx render`, `ftx viewer`, `ftx report`, `ftx sim-points`,
`ftx outfit`, `ftx materials` and immutable `ftx round` workspaces with their local review.

**Garment pose and continuation.** `ftx drape` and `ftx round --sim|--stills` accept
`--legs standing|seated|squat`, `--leg-fraction 0..1`, `--skinning lbs|dqs`, and
`--max-settle-steps 0..256`. Defaults retain the standing pose, full leg fraction, upstream Anny
linear blend skinning (`lbs`, omitted from default JSON), and fixed sewing/gravity schedule.
Opt-in `--skinning dqs` switches posing to deterministic Dual-Quaternion Skinning (also accepted by
`ftx body`, `ftx qualify`, and `ftx place`), recording `"skinning": "dqs"` in the report when non-default.
Leg posture is independent of `--arm-deg`; fitting and nappy calibration retain their reference pose.
Extra settling continues the same sewn cloth at the original time step until the shared numerical
quiet window or the declared cap. Reports and animations retain all accepted steps and actual
duration, including both starts when requested. Invalid settings refuse before file I/O. History
laws that require a declared duration refuse automatic settling through the shared solver guard.
The fixed chest-band qualification schedule remains unchanged.

`--static-equilibrium` optionally requests a separate frictionless conservative comparison after
the ordinary dynamic run, also forwarded by `round --sim|--stills`. `--static-tol-mm` (default
0.001) is a distance tolerance; `--static-max-newton` (default 300) is bounded to 1..=1000.
Explicit controls require the opt-in flag. Successful results write `static_equilibrium.obj` and
an independent report; refusals preserve the dynamic output and publish no trial mesh. Ordinary
trajectory, time, metrics, gates and exit status keep their dynamic basis. See
`docs/fold-inspection.md` for material-history and no-friction limits.

**Commands.**
- `ftx measure fit-start-review INPUT --model MODEL --out NEW_DIR [--silhouettes MANIFEST] [--json]`:
  inspect every declared shared shape and frame pose through the ordinary native model before
  fitting. The shared canonical preparation checks the same observations, identities, strict bounds
  and distinct starts as the fitter. Private source-image overlays, actual target projections,
  sampled contour comparisons and full initial OBJ meshes retain each declared start and frame.
  Behind-camera points, missing contours and large residuals remain explicit diagnostics;
  malformed input still refuses. Distorted cameras show projected vertices without invented
  straight lens-warped edges. `INITIALIZATION_REVIEW` and exact original/source/model bindings
  establish no optimized body, observability, fit acceptance, collision skin or READY status.
  The local editor authors shared-shape and per-frame pose starts with explicit named bounds and
  units. Empty/one-start inputs are draft-only; all existing starts retain strict validation, and
  the ordinary fitter still requires two to four distinct starts. Edits do not recompute the
  displayed original geometry. The review writes and embeds immutable `prepared-input.json` in
  original frame order. After applying two to four complete distinct starts, the editor can
  download `prepared-input-edited.json` directly for browser fitting, changing only `fit.starts`.
  Exact native frame identities, checked cameras, imported mask samples, observations, model
  configuration, metric evidence and FIT/CHECK roles remain intact. Pending edits, unresolved
  imports, cancellation, reset and page departure withdraw stale prepared and annotation links;
  restored drafts require Apply before complete downloads. Unsupported lossless timestamp
  handling withholds only the prepared download; annotations and draft editing remain usable.
  Browser export does not reread source files, update this static review, run optimization or accept a body.
  Load its exact edited prepared input and model in Photo Workbench to preview a selected declared
  start/frame through the shared portable model. Native and portable initialization views derive
  the complete mesh and ordered targets from one requested pose without evaluating tape functionals.
  Changed source evidence requires a fresh native review.
- `ftx measure fit-starts ANNOTATIONS --model MODEL --out NEW_DIR [--json]`: apply a complete
  `ftx-body-starts/1` annotation through the same native fitter admission. Exact source bytes,
  model and capture/mask evidence bind the values; original-order frame ordinals preserve full
  native timestamps without a browser round trip. Only starts and rebased capture paths change,
  plus explicit inline materialization of separately imported review-mask slices. All observations,
  FIT/CHECK roles, model targets and parameter bounds remain intact. `input.json` is an ordinary
  capture/prepared-fit input; `starts.json` records private create-only publication. Regenerate a
  start review, or preview the edited prepared snapshot in Photo Workbench, to see changed geometry.
  No pose estimate, optimization or fit acceptance.
- `ftx measure capture-init-review MANIFEST --model MODEL --calibration CAL --reference CHARUCO_DECL
  --reference-id ID --out NEW_DIR [--json]` and `ftx measure capture-init CHOICES --model MODEL
  --out NEW_DIR [--json]`: construct the first ordinary capture input from actual selected frames,
  one checked calibration and an existing stationary ChArUco reference declaration. Native setup
  records the exact source/model identities and available model controls. Explicit fixed/free
  shape, root and joint choices determine the configuration; no anatomical targets or starts are
  invented. Materialization resolves native frame ordinals, rechecks selection and source identities,
  and runs the existing camera acquisition and draft validator before private create-only publication.
  Empty targets/observations/starts keep the result a preparation draft. Independent optional metric
  observations retain their exact uncertainty, evidence and FIT/CHECK roles. Subsequent target, photo
  and initialization editors complete that ordinary input; ordinary fitting retains its original gates.
  The private offline setup page shows exact copied images, native text labels and explicit model
  choices. Browser payloads contain bounded frame ordinals instead of native timestamp numbers.
  Blank new controls, visible fixed-root/default assumptions and explicit FIT/CHECK tape roles
  retain operator authority. Apply prepares a choices download; pending/invalid edits, cancel and
  reset revoke stale links. Native source bindings and camera/model admission remain authoritative.
- `ftx measure body-target-review INPUT --model MODEL --out NEW_DIR [--json]` and
  `ftx measure body-targets ANNOTATIONS --model MODEL --out NEW_DIR [--json]`: author named
  correspondences on the exact array-verified converted model in a private offline mesh picker,
  then materialize the ordinary capture/prepared-fit input through native target validation.
  The unfitted rest preview uses declared fixed shape and midpoints of free shape bounds;
  surface selections preserve original triangle/barycentric identities, vertices preserve indices,
  and rig endpoints use exact bone labels. Empty target maps can be reviewed without placeholder
  anatomy. Application requires all observed names and rejects duplicates/invalid locations.
  Browser annotations carry only source/model bindings and the target map; native reconstruction
  preserves every other input value, including full-width timestamps, and rebases capture paths.
  New private directories, exact original bytes and report-last publication retain existing rules.
  No automatic anatomical mapping, photo observation, fit, READY or physical-accuracy approval.
  The local picker saves nonempty applied partial maps as `body-targets-draft.json` and reopens
  bounded strict UTF-8 annotations bound to the exact source/model. Imports validate all target
  definitions before replacing work; later actions cancel pending reads. Loaded maps require
  explicit review/application before the completed download, which still requires every observed
  name. Native complete-map validation and all other input values remain unchanged.
- `ftx measure video-mask-review MANIFEST.json --out NEW_DIR [--json]` and
  `ftx measure video-masks POLYGONS.json --out NEW_DIR [--json]`: author person/sheet sharpness
  regions in a private offline editor, then materialize opaque binary PNGs using the existing native
  rasterizer. Manifest bytes, image bytes, decoded pixels and dimensions bind every annotation;
  native code restores all original acquisition metadata, including full-width timestamps, without
  a JavaScript numeric round trip. Unreviewed regions refuse; explicitly invisible regions become
  zero masks and retain their frames for ordinary selector diagnostics. Components union after
  subtracting their own holes; integer bounds/topology and aggregate work/output limits refuse
  before publication. Original images are copied exactly, every frame is retained, and selection
  remains the existing `video-select` authority. No inferred anatomy, silhouette or stillness.
- `ftx measure video-decode REQUEST.json --out NEW_DIR [--json]`: explicitly invoke locally installed
  FFmpeg/ffprobe on a private snapshot of one local clip. The caller supplies stream/lens identity
  and a quarter-turn rotation; the existing selector receives original PTS-derived microseconds,
  checked decoded pixels and a `ftx-video-frames/1` manifest. Independently probed and decoded
  timestamps/counts/dimensions must agree. No masks are invented: success is
  `DECODED_REQUIRES_MASKS` until the operator supplies person/sheet masks or explicitly replaces
  their paths with rectangle sampling regions in the manifest.
  New private output only, local container/protocol allowlists, explicit source/frame/pixel/byte/time
  limits, and no partial success on decoder failure. See `docs/video-frame-selection.md` and the
  opt-in real external `video_decode_workflow` test. No stillness, calibration, anatomy or fit claim.
- `ftx measure video-select`: each person/sheet mask is a binary PNG path or explicit
  `{"rect_px":[left,top,width,height]}` sharpness region in stored oriented pixels. Rectangles
  reuse ordinary binary-mask scoring, refuse invalid integer bounds without clipping, and
  retain their declaration and raster hash without inventing a file hash. Original file-mask
  records stay unchanged. Small/empty-interior regions remain unusable. These sampling regions
  infer no segmentation, stillness or fitting silhouette. Capture replay binds subsequent edits.
- `ftx measure capture-review CAPTURE.json --out NEW_DIR [--silhouettes MANIFEST] [--json]`: reuse decoded source selection
  and checked same-lens sheet cameras without a model or body fit. The private create-only review
  shows original images and every supplied named FIT/CHECK keypoint, sigma and model target. Original
  bytes and source/camera evidence remain in `review.json` (`PREPARED`); its replay `capture.json`
  rebases local paths and binds `reviewed_sources_sha256`. The ordinary fitter reruns the checks and
  refuses changed evidence before loading a model. Original sources remain required. No anatomy,
  segmentation, observability, READY or physical-fit approval follows. See `docs/multi-frame-body-fit.md`
  and the real-binary `body_fit_workflow` regressions. This is an extension, not Python parity.
  The local page corrects existing coordinates by image click or numeric entry, keeping existing
  names, sigmas and partitions fixed. It can append observations for unused declared model targets
  after explicit frame, FIT/CHECK role and sigma selection, including in initially empty frames.
  Target definitions and other input values stay fixed; duplicate names, invalid values and the
  128-observation frame budget refuse. Partial drafts do not bypass fitter admission/acceptance.
  A current edit downloads as `capture-edited.json` and `prepared-input-edited.json`. The prepared
  snapshot joins reviewed annotations to the original native cameras by exact frame identity;
  model configuration, tape observations, starts and imported mask samples remain unchanged.
  It feeds browser fitting without native re-preparation, but cannot recheck later source-file
  changes. The path-bound capture retains that rechecking route. Pending/invalid/cancel/reset
  revoke both URLs, and reset restores the complete original point lists.
  Explicit selected-observation removal preserves every remaining correspondence and makes the
  removed target placeable again. Pending point/silhouette edits must be resolved first; reset
  restores the original names/roles/sigmas too. Insufficient FIT evidence retains native refusals;
  missing CHECK evidence retains the explicit no-held-out advisory rather than a new refusal.
  Original files remain unchanged and script-disabled original review remains available.
  Where JSON numeric-source and raw-JSON capabilities exist, opaque full-width integers remain
  exact through local edits and serialization. Otherwise the existing unsafe-number refusal
  applies before editing; no timestamp or other source identity is rounded to enable a download.
  Each frame's read-only stationary-sheet review shows the exact admitted correspondences,
  FIT/CHECK roles, metric positions, camera projections and residuals on the same stored pixels.
  Lens/reference identities and calibration/reference hashes remain visible. No camera refit,
  sheet edit/detection, physical reference or calibration accuracy approval occurs in the page.
  Optional capture `silhouettes` paths or the review flag reuse the strict mask reader without a
  body model. Inline/imported slices pass existing rectification and interval checks, then appear
  as observed FIT/CHECK foreground/background lines with original sigma/evidence.
  Replay rereads source masks and binds manifest bytes, samples and decoded hashes; changed sources
  refuse. No-manifest digests are unchanged. Neither a manifest nor inline constraints can be
  overwritten by another declaration. No segmentation, full-mask or occlusion approval follows.
  The local editor authors inline sampled silhouettes on checked rectified frames: explicit
  axis/position, foreground intervals or background, FIT/CHECK role, sigma and clothing/occlusion
  evidence. Existing slices retain their axis/position/role/sigma during endpoint corrections.
  Imported mask frames remain read-only, distorted cameras refuse authoring, and invalid or
  pending edits withdraw downloads. The ordinary inline schema and fitter retain validation,
  held-out isolation and acceptance authority; no inferred contour or body-fit approval.
  New unbound reviews may declare explicit `frame` identities instead of full `source` records,
  omitting calibration hashes. Existing selection/camera checks resolve those exact timestamps;
  replay contains full sources/hashes and preserves authored targets, observations and starts.
  Direct fitting, bound declarations, ambiguous frame/source pairs and incomplete full records
  refuse. No frame, pose or anatomy is inferred. Original input and bytes remain exact.
  A frame reference may declare `ftx-charuco-sheet-reference/1` with an original sheet sidecar,
  predeclared CHECK corner IDs and the stationary upright board's metric bottom-left. The existing
  native detector acquires every corner from the exact selected pixels and checked lens; all declared
  CHECK corners and at least four FIT corners are required. Existing sheet-pose gates and overlays
  consume those observations unchanged. Source bindings retain the complete acquisition and refuse
  changed sidecars, partitions, placement, images or calibration on replay. File-based references
  retain their behavior. The source declaration does not verify stationarity, print scale or anatomy.
- `ftx draft zouave_bolero|zouave_trousers|fez --measurements M --out DIR [--style S.json] [--json]`: the
  reference's `drafting.py` on `ftx_pattern::draft`. Writes `DIR/pattern.json` and `DIR/fit_spec.json`
  (replacing them, as the reference does; Python's number formatting, keys sorted) and prints
  `{pattern, fit_spec, structure_errors, solved}`; exit 0, or 2 with structure errors (the reference exits
  1). An unknown program or style key, a fez style out of range and a missing measurement are refusals
  (exit 3) and write nothing.
- `ftx validate PATTERN --measurements M [--fit F] [--json]`: the flat validation of `ftx-pattern`, the
  reference's text table, exit 0 CONVERGED, 2 NOT_CONVERGED, 3 BLOCKED.
- `ftx round WS [--pattern P] [--measurements M] [--sim|--stills] [--arm-deg DEG] [--skinning lbs|dqs]
  [--compression F] [--elastic-hold-steps N] [--res WxH] [--export] [--json]`: a new immutable round with separate construction, physics,
  manufacturing and measurement bases. `--stills` implies simulation and adds six native PNG
  views to the portable review (default 960x540), without Blender. No video is produced; this
  deliberately does not claim the reference's `--render` movie behavior. `--res` needs `--stills`.
  Rendering is recorded separately as `rendering.status`: PASS, FAILED, SKIPPED or NOT_REQUESTED.
  A renderer failure retains the construction/physics verdict and its diagnostics, but returns
  exit 2 (4 for a budget stop), with no images advertised; incomplete or previous still sets are
  never reused. Failed, blocked, unscaled and inapplicable simulations do not render. New images
  use exclusive creation; the review permits only this round's six known paths and no symlinks.
  When `--sim` runs on a workspace, `WS/qualification/qualify.json` is checked (`check_qualification`):
  a missing, failed or stale qualification keeps tightness flags advisory. The native engine requires matching
  `qualification_basis` identities for the posed collision body (`body_sha256`) and either the resolved uniform
  material (`material_sha256`) or all active simulated piece materials (`piece_material_sha256s`), including
  body-contact parameters. Older reports require rerunning `ftx qualify`. Even when collider bands pass
  (`collider_tightness_qualified: true`), wearer-blocking tightness promotion (`tightness_qualified: true`)
  requires `|posed_chest_girth_mm - input_chest_mm| <= 5.0` mm and `|residual_mm| <= 5.0` mm on measured
  girth targets at any `tightness_gating` landmark; larger residuals or `MODEL_FIT` priors demote those
  tightness flags to `advisory` with an explicit note. After `gate_tightness` updates `sim_report.json`,
  `rounds/NNN/sim/fold_viewer.html` is updated so its embedded `gating` and `advisory` arrays match
  `sim_report.json` and `review.html`.
  Tests: `round_workflow`, `round_review`, and `round::stills` synthetic triangle scenes; these
  tests do not certify an Anny garment's fit or solve at every requested arm angle.
- `ftx qualify [WS | --ws WS | --measurements M --out DIR] [--pattern P] [--fabric F] [--model PATH]
  [--body-fit PATH] [--arm-deg DEG] [--legs standing|seated|squat] [--leg-fraction F] [--skinning lbs|dqs] [--json]`:
  runs the three known-answer chest bands (`-20`, `0`,
  `+20` mm ease) at zero gravity (70 sew steps, no drape steps) for the uniform garment fabric or for
  every distinct active simulated `piece.fabric` selection. Band widths use the torso girth
  of the posed collision mesh at the chest placement landmark. The command reports that girth,
  the input chest target, `chest_girth_residual_mm`, `collider_pass`, and `wearer_target_aligned`
  in `qualify.json` (`fabric-qualify/1`, spec 04 §7.1) alongside `pass`, `rows`, optional
  `piece_materials`, `fabric_key`, `qualification_basis` and `measurements_sha256`.
  Each band saves its `pattern.json`, the exact `fit_spec.json` used by the solve, and `sim/` outputs,
  so the band can be replayed through `ftx drape` with those saved inputs. It prints each
  band's result and `QUALIFIED` / `NOT QUALIFIED: ...`. Exit 0 when all three bands pass, 2 otherwise.
- `ftx report WS [--json]`: regenerates `WS/report.html` from existing `WS/rounds.jsonl` without creating
  a new round directory.
- `ftx viewer SIMDIR [--out HTML] [--pattern P] [--json]`: rebuilds `fold_viewer.html` from an existing
  simulation directory (`fold.pc2`, `mesh_map.json`, `body.obj`, `sim_report.json`) without re-running
  the cloth solver.
- `ftx viewer SIMDIR --check-contact [--view VIEW] [--json]` checks all recorded physical frames
  against the full displayed body and cloth. When given a native still view, it first checks
  that the saved garment coordinates round to the final PC2 frame. Strain views also require the
  same positions and faces as the plain garment. A successful JSON check reports `endpoint_view`
  when this endpoint check ran.
- `ftx drape PATTERN --measurements M --out DIR [--fit F] [--sew N] [--drape N] [--elastic-ea N] [--elastic-hold-steps N] [--model PATH]
  [--arm-deg DEG] [--skinning lbs|dqs] [--self-contact-mm MM] [--self-mu MU] [--compression F] [--two-start-sew N] [--res WxH] [--json]` (`--elastic-ea`: the stiffness EA in newtons of a drafted
  casing's elastic, default 15; 0 simulates the casing at its finished size as the reference does).
  `--elastic-hold-steps N` reports casing displacement during the last N accepted gravity steps;
  it adds no steps and changes no physics. Zero requests no window; N above `--drape` refuses.
  Duration, endpoint speed and quietness remain separate from ideal-ring capacity and physical fit.
  It fits an Anny body of the wearer's age (`wearer.age_months`) and sex (`wearer.sex`; unknown = sexes
  averaged), fitted on the drape pose to whichever of the 21 tape measurements of
  `ftx_body::TAPE_FIELDS` the file has fused values for (the stature from `wearer.stature_mm` first) and
  to `wearer.weight_kg` or `wearer.bmi` when given (`ftx_body::fit_tape`; `Pose::drape`, hands and feet
  left out of contact), then
  `ftx_drape::run::drape`, writing `sim_report.json` (the metrics and flags plus run fields: pattern path
  as given, measurements SHA-256, wall time, and `body` with the fit's target and body value per
  measurement, its free parameters, every available tape reading and the body's mass; with `wearer.weight_kg` or
  `wearer.bmi` the mass is fitted too), `fold.pc2`, `mesh_map.json`,
  `fold_viewer.html`, and the last frame as OBJ for renders elsewhere (metres, z up): `garment.obj` with
  per-vertex seam colours and `garment_strain.obj` with strain colours (`v x y z r g b`, spec 04 §9.4),
  and `body.obj` (the full posed skin). Exit 0 when no gating flag, 2 with gating flags. Without `--fit`, no girth landmarks
  are read. Ordinary drapes enable cloth–cloth contact on the welded solver mesh. `--self-contact-mm`
  sets the sum of the layers' half-thicknesses (2 mm by default, with the barrier still active at zero).
  `--self-mu` sets its independent friction coefficient (default 0.4). `--compression F` (0 < F ≤ 1,
  default 1, the reference's cloth, as stiff in compression as in tension) sets how strongly cloth resists
  being shorter than its pattern along warp and weft. Below 1 it is a different cloth model: a mesh too
  coarse to form a fold lets the cloth shorten there, which stands for folds finer than the mesh without
  resolving them. The factor is authored, has no measured source, and is recorded as `compression_factor`
  in `sim_report.json` whenever it is not 1. When an ordinary drape (not `--two-start-sew`, not an outfit;
  `ftx round --sim` runs this same command) refuses once its start has been admitted, it writes the cloth
  state the refusal is about under `DIR/refused/`: `body.obj`, `garment.obj` coloured by piece,
  `garment_strain.obj` and the six stills, as a `ftx place` stage does, and names that directory in the
  refusal's first repair. The state is the last accepted one when sewing, draping or settling refuses (the
  admitted start if the first sewing step does), or the recorded frame that crosses the displayed wearer when
  the full-body check refuses. It is not a drape or a fit. A refusal of the start itself
  (`intersecting_start`: a posed body the surface audit does not admit, cloth inside or too near the
  body, cloth layers crossing or touching) has no solver state, so the command draws the two `ftx place`
  stages from the pattern and body the solver was given (an outfit merged, a `--fabric` or `--elastic-ea`
  override applied; both members of `--two-start-sew` share this start) and leaves them under
  `DIR/refused/` (`start/`, `wrap/`, `placement.json`). The first repair names them, quotes the body
  audit when the pose was not admitted, and gives the number of cloth vertices inside the body (magenta)
  and of crossing triangle pairs (yellow), largest piece pair first. Those marks test penetration and
  crossing only: layers that merely touch (refused when cloth contact is on), or cloth inside the body's
  offset surface with no vertex inside the body, are refused with both counts at zero. Any other early
  refusal leaves `DIR` empty. Every refusal picture is `--res WxH` pixels (default 960x540, the limits of
  `ftx render`); `ftx round --stills --res` hands the drape its own size, so a pixel names the same place
  in a refused round's pictures as in a completed round's stills, and `refused_pose.width`/`height` in the
  map beside them say that size. When the refused state is the start itself (no sewing step accepted),
  the second repair says what the run knows about that start, because no solver budget changes it: that
  it is an earlier relaxation checkpoint, with both iteration counts, when the fully relaxed start was not
  admitted for cloth contact (the progress line says the same); and each instance at least 2 % of whose
  vertices have a mean edge strain of 15 % or more, with the pattern edge nearest that stretch. The
  threshold is measured on one garment (the comment on `TIGHT_START_PCT` gives the numbers), not a
  validated limit. `ftx place` applies the same measure to the start it draws, prints such instances as
  `TIGHT START` and records `stretched_past_ramp_pct` and `stretched_nearest_edge` in each instance's
  `start_edge_strain`. It draws the fully relaxed start, so it does not flag a garment whose fully relaxed
  start is loose but is not admitted for cloth contact: there only the drape's refusal names the stretched
  checkpoint.
  Numerical options are
  validated before file IO or body fitting. Starts with crossing/touching cloth layers are refused,
  with a repair message. Both `sim_report.json` and the CLI result contain a
  `self_contact` object with enabled/audit status, parameters and nearby final gap when enabled.
  This audit covers sewing/draping solver states, not the kinematic fold/zip animation or fit accuracy.
  `--arm-deg` accepts a finite 0–180 degrees from vertical (default 35), including an explicit zero
  rather than the reference round's ignored-zero quirk (spec 04 §16). Body fitting keeps the standard
  tape reference pose; only the collision/display pose and garment placement change. Reports retain
  the actual angle. A requested pose can still fail contact/feasibility gates; it is not a fit claim.
- `ftx outfit --sim DRAPE_DIR --sim DRAPE_DIR ... --measurements M --out DIR [--json]`:
  composes frozen final garments from compatible saved drapes for a proportions review
  (`docs/outfit.md`). It does not resolve contact between garments or simulate their layers together.
- `ftx sim-points SIMDIR --pattern P --measurements M [--json]`: inspects drafted vs settled z-levels of
  non-sleeve corners across the tail of `fold.pc2` (`docs/fold-inspection.md`).
- `ftx materials <subcommand> ... [--json]` and `ftx-materials <subcommand> ...`: share the same
  source-qualified material, thermal, calibration, strip/cylinder, cutting, finite-stock and sewing
  subcommand set (`list`, `catalog [SEARCH]`, `show ID`, `freeze ID`, `compare FIRST_ID SECOND_ID`,
  `inspect`, `resolve`, `bind`, `bind-changes`, `price`, `screen`, `study`, `study-on-body`,
  `scenario-study`, `pressure-condition`, `compression-stack`, `treatment`, `treatment-pattern`,
  `calibrate*`, `linearize-origin`, `strip-*`, `cylinder-experiment`, `thermal*`, `cut-*`, `stock-*`,
  `sew-*`). Only `show` retains each binary's documented top-level inspection envelope (`ftx-material-detail/1`
  under `ftx materials show` vs `snapshot` + `selection` under `ftx-materials show`); every other shared
  subcommand produces identical domain JSON (`docs/material-catalog.md`, `docs/material-calibration.md`,
  `docs/material-thermal.md`).
- `ftx materials inspect SELECTION.json [--json]` and `ftx-materials inspect SELECTION.json`:
  inspect an ID-based or self-contained selection through the same source/capability report as the
  browser. Inputs use the existing 64 MiB material limit. The file owns estimate consent and digest
  locks; missing capabilities remain separate refusals. No pattern binding or output file is required.
- `ftx materials sew-cell-svg PATTERN OPTIONS CELL OPERATION NEW.svg [--json]` and
  `ftx-materials sew-cell-svg PATTERN OPTIONS CELL OPERATION NEW.svg` rebuild the full sewing job
  and render the explicitly selected cell's actual machine/recipe stations. Frozen machine and
  qualification identities, assigned global copies and actual feed remain in the SVG metadata.
  Configured-cell jobs require this explicit cell; ordinary `sew-svg` refuses their ambiguous
  reference geometry. SVG and receipt outputs retain create-new semantics. See `docs/sewing-production.md`.
- `ftx materials thermal-vapor REQUEST.json [--json]` and companion
  `ftx-materials thermal-vapor REQUEST.json` expose the shared `ftx-vapor-stack/1` consumer.
  Canonical measured Ret and explicit area, vapor pressures, films and latent heat determine
  conditional steady heat/water transfer; exact frozen replay and source refusals are retained.
  This is independent of dry thermal storage and mechanical readiness; see `docs/vapor-resistance.md`.
- `ftx materials thermal-vapor-coverage REQUEST.json [--json]` and the companion command
  replay the shared geometry-derived layered vapor network. Frozen sources, declared body/cloth
  geometry and vapor boundaries determine covered-region potential flows; uncovered fractions
  remain explicit and no sweat supply or coupled evaporative cooling is inferred.
- `ftx materials thermal-vapor-coverage-history HISTORY.json [--json]` and the companion command
  replay the complete bounded moving-coverage potential through the shared history evaluator.
  Fixed source/body identities and explicit vapor boundaries follow every accepted geometry sample;
  right-endpoint potential integrals retain exact replay and refuse any failed sample. This does
  not evolve moisture storage or inject latent cooling. See `docs/vapor-coverage.md`.
- `ftx materials cut-job PATTERN.json REQUEST.json [--json]` and the companion command
  route every physical cut instance/copy exactly once to roll or finite stock through the existing
  planners. Complete geometry and independent purchasing outcomes retain frozen pattern/request
  replay. Missing stock refuses the complete job; missing offers do not erase an allocated job.
- `ftx materials strip-experiment REQUEST.json [--json]` and
  `calibrate-strip ACQUISITION.json [--allow-estimates] [--json]` expose the shared finite-strip
  forward and inverse experiment. The companion `ftx-materials` accepts the same commands.
  Actual constrained equilibria supply grip reactions and midpoint chord widths; the inverse
  retains independent FIT/CHECK, mesh-refinement, source and strain-domain gates. Output estimate
  admission is separate from acquisition admission. Rejected acceptance gates preserve diagnostics
  without a selection; exhausted work and failed physical solves return structured refusals.
  Accepted coefficients remain partial materials when bending/contact or other properties are absent.
  Exact frozen selections feed ordinary material binding. See `docs/clamped-strip-calibration.md`.
- `ftx body --measurements M --out DIR [--pose drape|rest|arms-up|seated|squat] [--skinning lbs|dqs] [--model PATH] [--json]`:
  reserves a fresh output directory before model loading or fitting; existing directories, files and
  symlinks refuse without changing previous evidence. `body.obj` and `body.json` use the existing
  private create-only writer (mode 0600 on Unix), also refusing a file created during publication.
  Writes the same fitted body (fitted on the drape pose, then posed as asked) as `body.obj` (the closed skin)
  and `body.json` (`ftx-body/1`: the fit as `drape` reports it, plus the measurements' SHA-256, the pose,
  `pose_geometry` reporting rest-vs-posed skin and `clothing_collider` volume change, LBS volume-loss
  advisories and exact self-intersection audits, the phenotype and every local change in use). Exit 0; an unknown pose is refused. Both reports carry
  `model_fit`: available readings with no target or only a chart-derived target from record fusion.
  The latter also record `fit_target_provenance: CHART_PRIOR`. Explicit fused-only targets stay supplied.
  Each is a `fabric-measurements/1` record with provenance `MODEL_FIT` and the population spread as σ (the chart prior's at the wearer's
  stature and age, else the catalogue's 12 mm), so it can be added to the file as a model value, never
  as a measurement (`tests/body_model_fit.rs`, which needs the model). Unrequested missing sections
  do not block fitting: `readings_mm` and `model_fit` omit them, and `tape_evaluation` reports partial
  coverage with its exact unavailable fields. A missing requested reading still refuses by name.
  The synthetic articulated-prism workflow in `tests/accepted_body_workflow.rs` covers native and
  portable fitting, body publication and material-study handoff without external model assets.
  Explicit `wearer.nappies: true` adds the shared calibrated nappy occupancy to exported/draped
  geometry, including accepted-body reuse. The required fused `seat_over_nappy` target must be
  available and reachable. Bare body parameters, mass, source identity and fitting residuals stay
  separate from the `nappy_occupancy` report and computed `readings_mm.seat_over_nappy`. Calibration
  uses the standard measurement pose once; a seated/squat/export pose never refits that proxy.

Record-bearing standalone `draft`, tape-fitting `body`, and `drape` inputs are re-fused through the same
authority as workspace rounds before fitting, model loading or output creation. A cached `fused` value cannot
override source records. Explicit fused-only and flat inputs retain their supplied targets. Source
hashes identify the original file bytes, and the input is never rewritten. This CLI freshness exception
does not change fitting numerics or promote model estimates to measurements. Direct invalid-record
controls and full-model fresh/poisoned-cache comparisons live in the existing workflow tests.
- `ftx render DRAPE_DIR [--res WxH] [--json]` (default 960x540, each side 16 to 8192, at most 16 million
  pixels per view): the six stills of an
  `ftx drape` run (`ftx_drape::render`) from its `body.obj`, `garment.obj` and `garment_strain.obj`, written
  into DRAPE_DIR as `still_{front,side,back,three_quarter,strain_front,strain_back}.png`, printing
  `{"stills": {view: path}}` as the reference's `blender_sew.py --stills-from` does; no Blender. A missing
  or unreadable OBJ, empty mesh or non-finite geometry is refused (`io`, `invalid_document`).
- `ftx render BASELINE_DIR --compare REVISED_DIR --view VIEW --out NEW_DIR [--res WxH] [--json]`
  renders one native view of each scene at the baseline camera with the same raster dimensions.
  It keeps the original scene files and writes `before.png` and `after.png` in the new directory.
  The report indicates whether the posed bodies have equal vertices and faces. Use the
  original view for pixel inspection and to check any geometry outside the baseline framing.
  Strain comparisons use the saved strain colours and require the plain and strain meshes to
  have the same geometry.
- `ftx place PATTERN --measurements M --out NEW_DIR [--model PATH] [--body-fit REPORT] [--spacing-mm MM]
  [--arm-deg DEG] [--legs standing|seated|squat] [--leg-fraction F] [--skinning lbs|dqs] [--res WxH] [--json]`: where a pattern
  starts on the body, for a person or an agent authoring placements and judging a cut by eye. It runs the
  drape's own scope admission, mesh, isometric wrap, pre-sew and relaxation (spec 04 §2–4) and **stops
  before contact admission and dynamics**, so it answers in seconds and also shows a start `ftx drape`
  would refuse. It writes `wrap/` (each piece on its cylinder) and `start/` (pre-sewn and relaxed), each
  with `body.obj`, `garment.obj`, `garment_strain.obj` and the six `still_*.png` of `ftx render`, plus
  `placement.json`. Cloth is coloured by source piece (a mirrored copy is the lighter tint of its pair);
  a vertex inside the collision body is magenta. The strain views show seam travel in `wrap/` (green 0 to
  red 150 mm) and edge strain in `start/` on a ±15 % ramp, because a kinematic start saturates the ±3 %
  ramp of a finished drape. `placement.json` gives, per instance, the resolved radius and anchor, its
  angular and height span in the part frame of the right-hand copy, vertices inside the body at both
  stages, and the start's edge strain (percentiles and a least-squares mean along pattern x, pattern y
  and in shear); per seam, how far its pairs travel from the wrap; the torso, arm and leg frames with
  every landmark height and section radius the wrap uses; `body_surface`, the result of the drape's
  own self-intersection audit of the posed body, reported and not enforced here; and `cloth_contact`,
  every pair of start-state cloth triangles that cross (one vertex per sewing group, so a sewn seam is
  adjacency), counted per pair of pieces and painted yellow in `start/`. That is the first thing
  `ftx drape --self-contact-mm` refuses on; its thickness clearance is not tested here. Lengths are millimetres; OBJ files are metres.
  **No-claim boundary:** nothing here is a drape, a fit or an admission, and start-state strain is a
  property of the kinematic start, not of the garment at rest. Unit tests cover the strain fit and the
  piece colouring; the command itself has been exercised on the 1761 dolman, not against an oracle.
- `ftx alter OP PATTERN --out OUT [...]` with OP `slash-spread` (`--piece --axis x|y --at --add`), `move`
  (`--piece --vertex [--dx] [--dy]`), `true-seam` (`--seam [--end start|end]`), `cap-ease` (`--piece
  --seams S1,S2 --target`) or `transfer-curve` (`--from SRC.json:INSTANCE/EDGE`, split at the last `:`,
  `--to PIECE/EDGE`): the reference's `alter.py` (spec 01 §10.7) on `ftx_pattern::alter`. Writes OUT even
  with structure errors and prints the op's history sentence; exit 0, or 2 with the structure errors
  listed (the reference exits 1); a reference error (missing piece or vertex, a `.L` side) is a refusal.
  OUT must be a new path: existing files (including the input itself or an alias of it) are never
  overwritten. Serialization completes before file creation; the write is flushed before success.
  This intentional no-clobber exception preserves previous pattern revisions. Impossible seam/cap
  targets are refused before any output is created, not saved as saturated adjustments.

- `ftx export TARGET PATTERN --out DIR [--unfold] [--sa MM] [--mitre-limit N] [--projector-width MM]
  [--layout-width MM] [--keep-blank-pages] [--hpgl-rotate90] [--hpgl-mirror] [--created YYYY-MM-DDTHH:MM]` with
  TARGET `svg`, `pdf`, `letter`, `a4`, `copyshop`, `projector`, `dxf`, `astm`, `aama`, `flat-dxf`, `hpgl`,
  `cutter`, `garmentcode`, `embroidery` or `all`: the reference's `export.py` (spec 03 §1.3) on `ftx-export`.
  Writes the target's files into DIR and updates `<STYLE>_export_manifest.json` there (an existing manifest
  keeps its entries and key order, so separate runs share one), printing each file's path. `all` runs every
  target, then every acceptance check with the pattern, printing `FAIL check [file] detail` lines and
  `PASS|FAIL: n/m checks`; exit 0, or 2 when a check fails (the reference exits 1). The structure errors are
  printed to stderr as warnings. A refused target (a pattern traced in pixels: `unscaled`; a projector page
  over 200 in) writes nothing. Without `--created` or the pattern's `meta.created`, the current UTC minute
  (the reference uses local time). `ftx export check DIR` is `ftx check DIR`.
- `ftx check DIR [--pattern-ref PATTERN]`: every acceptance check (spec 03 §8.1) over an export directory,
  printed as above; exit 0, or 2 when one fails. With the pattern the DXF sew references are recomputed and
  the DSTs' couching checked against the braid lines. A file the manifest lists but DIR lacks is a failed
  `file.missing` item (the reference raises); no manifest is a failed `manifest` item.
- `ftx trace IMAGE --out DIR` with the reference's flags (`--crop`, `--channel`, `--invert`/`--no-invert`,
  `--threshold`, `--bg R,G,B`, `--stroke`, `--close`, `--open`, `--corner-deg`, `--soft-deg`,
  `--simplify-frac`, `--smooth`, `--min-area-frac`, repeated `--erase X,Y,W,H`, `--no-dashes`,
  `--no-review-images`): the reference's `vectorize.py trace` on `ftx-trace`. Accepts PNG, baseline/progressive
  8-bit JPEG and lossless/lossy WebP (`--threshold adaptive` or integer). Writes `DIR/draft.pattern.json`,
  `DIR/mask.png`, `DIR/trace_px.json`, and (unless `--no-review-images` is passed) native `DIR/overlay.png`
  and `DIR/labels/pN.png`; prints the reference's summary and exits 3 when no piece is found.
- `ftx annotate DRAFT --names NAMES.json --out PATTERN`, `ftx scale-fit PATTERN --target REF=LEN [--target
  …] --out PATTERN` and `ftx audit PATTERN --out DIR [--image IMG] [--labels L.json] [--tol-mm 3]
  [--median-seam-mm 250] [--label-tol 0.03] [--no-review-images]`: the reference's `vectorize.py
  annotate|scale-fit|audit` (spec 04 §10.1) on `ftx-trace`, writing what the reference writes for the same
  inputs (`ftx-cli/tests/trace_workflow.rs`), plus native `DIR/round0_audit.png` when `--image` is provided.

**Output.** Human text, or with `--json` exactly one JSON envelope on stdout:
`{"schema": "ftx-cli/1", "command", "ok", "exit", "result"}` or, for refused input,
`{..., "ok": false, "exit": 3, "refusal": {code, message, repairs}}` (exit 4 for budget refusals).
Progress lines go to stderr.
Malformed option syntax and unknown commands also use that envelope when `--json` is requested.
Duplicate options (except repeated measurement `--value` readings), a flag in place of an option
value, and extra alteration positionals are refused rather than silently changing the requested
edit. Negative numeric values remain valid.

**Error model.** A refusal is printed with its repairs, exit 3 (4 for `budget_exceeded`): an option the command
does not take (`invalid_document`, so a misspelt `--fit` cannot silently drop the fit spec), unreadable
or malformed files (`io`, `invalid_document`), an output directory that cannot be created (checked
before the drape starts), a wearer without age or stature, a stature the body model cannot reach
at that age (`out_of_range`; other measurements out of reach stay residuals in `body.fit.tape`), a start that crosses the body (`intersecting_start`).

**No claims.** Legacy round parity and rendered movies; Blender pixel parity; a body that matches the
wearer anywhere but the fitted measurements; performance at arbitrary garment sizes or requested poses.

Both native material `show` commands report `sewing_thickness_m` through the shared
unloaded-thickness authority, independently of mechanical and thermal readiness. A ready
`value` is the admitted nominal observation in metres; source bounds remain in the material,
and actual manufacturing clearance uses their upper endpoint. This compatibility/admission
result is not measured machine qualification. Pressure/gauge observations and unapproved
estimates retain their structured refusals without hiding the original material or heat data.

## Measurement evidence commands

`measure catalogue|prior|init|add|validate|fuse|ready|reference|calib-sheet|calibrate-camera|photo-scale|sheet-pose|measure-photo|girth-from-section|triangulate|bundle-adjust|fit-body|still-window|video-select`
delegates calculations to `ftx-measure` (and `ftx-body` for `fit-body`); details and examples are in
`MEASUREMENTS.md` and `docs/*.md`. Files written by `--out` are raw domain JSON (or SVG/PDF for
`calib-sheet`), while stdout `--json` remains the CLI envelope. The CLI supplies UTC today unless an
explicit date is given; the library is clock-free. Repeated `--value` readings are preserved. Invalid
edits never replace the input; successful writes use same-directory atomic rename and mode 0600 on Unix.
Exit 3 includes invalid measurement documents and NOT_READY. The evidence gate distinguishes
READY_REFERENCE from ordinary readiness and makes no fit claim.

`video-select` accepts already-oriented opaque 8-bit PNG or supported complete JPEG frames; JPEG
EXIF orientation is absent or identity. It reuses the strict photo decoder, refusing automatic
rotation/reflection because masks and calibration already share the stored pixel frame. File masks stay
binary grayscale PNGs of the exact frame size. Encoded limits are 128 MiB PNG / 64 MiB JPEG; decoded
frames stay within 40 million pixels. Tests in `video_selection_workflow` cover JPEG/decoded-PNG
selection equality, absent/identity orientation, source preservation and create-only outputs, and
nonidentity/corrupt/mask refusals. JPEG quality is decoded-pixel quality, not stillness, frame decoding,
segmentation or measurement accuracy.

`measure fit-body` also accepts `ftx-anny-capture-input/1`: reviewed selected-source records,
original stationary-sheet points and hash-bound accepted camera calibrations join into the existing
articulated fit. Selection, same-lens calibration and sheet pose are recomputed; source bytes,
decoded buffers, frame/epoch/orientation and held-out identities are retained. Conflicting or changed
inputs refuse before fitting/publication. JSON sources are capped at 16 MiB and complete capture
evidence at 4 MiB. `--mesh-out` adds hash-checked original images to the existing private, create-only
review bundle, under its 256 MiB limit; image changes during fitting refuse publication. Its offline
page allows only local images and inline styles, no scripts or external resources. Original prepared-coordinate
fits keep their input and output semantics. Capture consistency does not authenticate anatomy,
decoding, stationarity, calibration accuracy or fit, and never promotes measurement readiness.
See `docs/multi-frame-body-fit.md` for the capture/decode boundary and input contract.

Capture and initialization reviews, including `fit-starts` materialization, retain an optional
complete `capture_images` binding in `prepared-input.json`. Native and portable fitting validate
the same exact frame/hash/dimension/already-applied-orientation records and preserve them in the
saved report. Browser frame review can then inspect an explicitly selected original image without
reopening a native file path or refitting. Coordinate-only inputs without image bindings retain
their behavior; image matching does not authenticate the capture or upgrade measurement evidence.

Imported-shape drapes retain the exact accepted report bytes as `body_fit.json` through
the existing create-only output writer. Simulated rounds keep it in their own `sim/`
directory and link it from the construction review. That review displays the saved
MODEL_FIT body's signed model-minus-target residuals at their stored precision, keeping
missing predictions unavailable. Current targets retain their original evidence basis;
no tolerance, refit, uncertainty estimate or readiness promotion is introduced. Failed
simulations expose no success/source links. Ordinary tape-fitting output is unchanged.

**Timed strips, cylindrical holders, and draped vapor.** Both native material entry points accept
`strip-history REQUEST.json`, `cylinder-experiment REQUEST.json`,
`calibrate-cylinder REQUEST.json [--allow-estimates]`, and
`thermal-vapor-surface SIM_REPORT.json CONDITIONS.json`.
They delegate to the production strip/cylinder forward and inverse solvers and canonical vapor surface consumers, enforcing the same
64 MiB input limit, source policies, physical/numerical refusals and exact frozen replay as the library.
Strip and cylinder reactions exclude imposed-node/hardware inertia, cylinder calibration preserves separate
observation vs output estimate gates without fabricating unloaded `thickness` from `reported_thickness`, and
sheet vapor totals remain conditional independent-sheet potentials. See `docs/fabric101-materials.md` and the strip and vapor documents for examples and scope.
