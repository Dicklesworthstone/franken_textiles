# AGENTS.md - FrankenTextiles

> Guidelines for AI coding agents working in this sewing-pattern, body-model, cloth-draping and
> manufacturing-file codebase.

---

## RULE 0 - THE FUNDAMENTAL OVERRIDE PREROGATIVE

If I tell you to do something, even if it goes against what follows below, YOU MUST LISTEN TO ME. I AM IN
CHARGE, NOT YOU.

---

## RULE 0.1 - VALUE DELIVERY OVER PROCESS: NO PROCESS PORN

FrankenTextiles is early. Every hour spent on meta-infrastructure is an hour not spent on the Phase 0 work
that tells us whether the body model and the drape solver are worth having.

**Process is never the product unless the user explicitly asks for process work.** Beads, plans, Agent Mail,
audits, manifests, provenance, logging, CI, harnesses, dashboards and status reports exist only to support
delivery.

### The value test

Before doing non-product work, answer all three:
1. What concrete capability, correctness defect or immediate blocker does it address?
2. Is it the smallest direct action that addresses it?
3. Will its likely value exceed its cost right now?

If any answer is unclear, **do not do the work**. Return to implementing the requested functionality.

### Hard scope limits

- A review request authorizes reading the relevant code, reproducing concrete defects, the smallest sound
  fixes and focused regressions. It does not authorize redesigning adjacent infrastructure.
- Never turn a small repair into hundreds of lines of harness, schema, logging or planning without explicit
  approval.
- Do not build a validator for a validator or a harness for a harness.
- The oracle fixture tooling is a few capture scripts and a comparison crate. If you find yourself building
  infrastructure *around* the fixtures rather than filling them in, stop.
- Do not chase a failure already present on `HEAD` unless it blocks the requested deliverable.
- Do not spawn an agent swarm for a narrow task.

### Implementation must dominate

Spend the dominant share of effort on working product code and direct tests of it. Prefer an existing test
seam over a new framework. Run the narrowest relevant checks while iterating.

### Mandatory checkpoint and stop rule

At the first sign of scope expansion, state the outcome, the files and size of the expansion, why it is
necessary and the smaller alternatives. If that cannot establish immediate value in a few sentences, ask
first. When work has drifted into process porn, stop, freeze the tree, disclose the state and wait.

---

## RULE NUMBER 1: NO FILE DELETION

**YOU ARE NEVER ALLOWED TO DELETE A FILE WITHOUT EXPRESS PERMISSION.** Even a new file that you yourself
created, such as a test file. You must always ask and receive clear, written permission before deleting a
file or folder of any kind.

---

## Irreversible Git & Filesystem Actions - DO NOT EVER BREAK GLASS

1. **Absolutely forbidden commands:** `git reset --hard`, `git clean -fd`, `rm -rf`, or any command that can
   delete or overwrite code/data, unless the user explicitly provides the exact command and states, in the
   same message, that they understand and want the irreversible consequences.
2. **No guessing:** if there is any uncertainty about what a command might delete or overwrite, stop and ask.
3. **Safer alternatives first:** `git status`, `git diff`, backups, hand-written patches.
4. **Mandatory explicit plan:** even after authorization, restate the command verbatim, list what will be
   affected, and wait for confirmation.
5. **Document the confirmation** in your final response: the authorizing text, the command, the time.

---

## Git Branch: ONLY Use `main`, NEVER `master`

All work happens on `main`. Never create, switch to or push feature branches. Never reference `master`.

## RULE 2: NO GIT BRANCHES. NO GIT WORKTREES. EVER.

Forbidden: `git branch <other>`, `git checkout -b`, `git switch -c`, `git worktree add`, pushing non-main refs,
pull requests from branches, scratch clones of this repository used to isolate work, and any tool that creates
branches or worktrees as a side effect. Instead: commit to `main` when the work is ready, keep unfinished work
in the tree, coordinate through Agent Mail reservations and Beads IDs, and never revert or stash another
agent's changes.

---

## Project Truth Sources

- `COMPREHENSIVE_PLAN_FOR_THE_DESIGN_OF_FRANKENTEXTILES.md` - the technical constitution (architecture,
  claims, gates, phases, crates, budgets).
- `docs/spec/01…05-*.md` - the behaviour specifications extracted from the reference implementation
  (01–04) and from the pinned Anny body model (05). **When
  porting, implement from the spec, not from the legacy code.** If the spec is wrong or silent, fix the spec
  first (with the evidence), then the code.
- `FEATURE_PARITY.md` - what is present, partial, missing or excluded, with the fixture IDs that prove it.

When this file and the plan disagree on a technical rule, the plan wins; flag the discrepancy.

---

## FrankenTextiles - This Project

FrankenTextiles makes sewing patterns that demonstrably sew together and fit a specific body, and writes the
files people and machines need. It is the mechanical core of the `fabric-pattern-creation-and-alteration`
agent skill, rebuilt in Rust:
- patterns: model, validation, alteration and measured drafting programs;
- bodies: the Anny parametric body model (newborn to adult) fitted to tape measurements, photos and video;
- draping: a contact-correct cloth solver (incremental potential, barrier contact, Coulomb friction);
- measurement: fusion, readiness, photo scale and photogrammetry of a moving child;
- exports: SVG, tiled PDFs, copy-shop and projector PDFs, DXF-ASTM/AAMA, HPGL, cutter files, GarmentCode,
  Tajima DST embroidery;
- tracing AI or scanned pattern sheets; a CLI (`ftx`) and a wasm build for the browser.

### What this project is NOT

- Not a CAD application with an interactive curve editor.
- Not a fashion renderer.
- Not a body-scanning product.
- Not a replacement for a muslin.
- Not a general cloth-simulation library.

---

## THE FOUR CLAIMS - NEVER CONFLATE THEM

| Claim | Meaning | Evidence |
|---|---|---|
| **Parity** | A ported subsystem reproduces the reference (Python skill or upstream Anny) | Oracle fixtures; differential tests |
| **Physics** | The drape solver behaves like cloth where the answer is known | Known-answer tests (plan §11.9) |
| **Fit** | A garment fits a real person | A muslin or a dressed photo. Nothing computed establishes it |
| **Performance** | A workload runs in a stated time on stated hardware | A benchmark with fingerprint, inputs and spread |

Never report them as one number. A drape with no gating flags is not a fit claim. A body fitted to
measurements fills the unmeasured from its prior: those values are `MODEL_FIT`, never "measured".

---

## Honesty Rules Specific To This Project

1. **Never regenerate an oracle fixture from the candidate.** Fixtures come from the pinned reference. A
   mismatch is a defect until proven otherwise.
2. **A reference quirk is a decision, not an accident.** Reproduce it under a named flag or fix it with a
   regression test and a parity exception in `FEATURE_PARITY.md`. Never silently.
3. **No speed claim before a benchmark.** No "faster than Blender" without one.
4. **No fabric number without a source.** Fabric parameters and friction stay `UNCERTAIN` until sourced.
5. **Report residuals, never hide them.** A body fit, a photo measurement or a drape reports what it could
   not match.
6. **A failure is a failed cell, not an excluded cell.** Partial never rounds up.
7. **Refuse, never clamp silently** (plan §6).

---

## Privacy - The Usual Wearer Is A Young Child

- **Never commit a real person's measurements, photos, videos, scans or name.** Fixtures are synthetic or the
  public-prior reference wearer. Not in tests, not in docs, not in examples, not in commit messages.
- Everything runs locally; the browser page uploads nothing.
- Converted body-model data and user workspaces stay out of the repository (`.gitignore`).

---

## Oracle Pin Discipline

- Python reference: the `fabric-pattern-creation-and-alteration` skill's scripts at revision `88e03965`.
- Body model: `anny==0.6.1` (pin and wheel hash in `tools/body-model/PIN.md`).
- Lifted frankensim code: a provenance header on every lifted file naming the source path and commit.
- Moving a pin is a deliberate event with new fixtures and the old results kept.

---

## Toolchain: Rust, Cargo, Wasm

- Dated nightly pinned in `rust-toolchain.toml`; Rust 2024 edition; one Cargo workspace.
- Targets: native (CLI) and `wasm32-unknown-unknown` (browser).
- Determinism (plan §6): fixed-order assembly, sorted candidates, deterministic elementary functions where
  native/wasm results must agree, no fused multiply-add in those hot loops, no hash-map iteration that
  affects output.

### Unsafe Code

`#![forbid(unsafe_code)]` in every crate (workspace lint). The wasm boundary uses `wasm-bindgen`'s safe
API; zero-copy views go through it, never through hand-written pointer casts.

### Dependency Policy

Small, pinned allowlist (plan §16): `serde`/`serde_json` (files only), `spade` (constrained Delaunay),
PNG/JPEG decoders, `wasm-bindgen`/`js-sys` in `ftx-wasm` only. **Forbidden in library crates**: Python at
runtime, PyTorch, BLAS/LAPACK, C/C++ FFI, Tokio, a mandatory Rayon pool. For each new crate record version,
used API, wasm build result, transitive closure and demonstrated benefit; prefer `default-features = false`.
Sibling Franken projects are resources, not dependencies: lift files with provenance, do not depend on the
workspace.

---

## Planned Workspace Crates

Ten crates. No new crate without explicit user approval.

| Crate | Responsibility | Planned lines incl. tests |
|---|---|---:|
| `ftx-core` | Units, deterministic maths, curves, polygons, schemas, refusals, receipts | 8,000 |
| `ftx-pattern` | Pattern model, instances, validation, alteration, drafting | 16,000 |
| `ftx-body` | Anny forward model, tape functionals, fitting, poses, landmarks | 9,000 |
| `ftx-measure` | Catalogue, priors, fusion, readiness, photo scale, photogrammetry | 16,000 |
| `ftx-drape` | Garment scene, contact cloth solver, metrics and flags | 14,000 |
| `ftx-export` | SVG, PDF, DXF, HPGL, cutter, GarmentCode, DST, acceptance checks | 14,000 |
| `ftx-trace` | Tracing pattern sheets | 7,000 |
| `ftx-cli` | The `ftx` binary | 4,000 |
| `ftx-wasm` | Browser bindings | 3,000 |
| `ftx-conformance` | Oracle, known-answer and determinism tests | 9,000 |
| **Ceiling (with 20,000 contingency)** | | **120,000** |

### Dependency Direction

```text
core <- pattern, body, measure
core/pattern/body <- drape
core/pattern <- export, trace
everything <- cli, wasm, conformance
```

`ftx-measure` reaches the body model through a trait, never a cycle. Browser types never leak into numerical
crates. If you need to violate the direction, stop and redesign the API.

---

## Core Invariants

- **Units**: pattern files in millimetres (2D, y up, wearer's right, public side, CCW loops); bodies and
  drapes in metres, kilograms, seconds (3D, z up, wearer facing −y). One conversion boundary each.
- **One authority per number**: lengths and ease from the flat pattern; hang, contact and construction from
  the drape; fit from the muslin.
- **Seams** are exact (welded) in the drape, not stiff springs.
- **The verdict names its basis**: flat status, drape gating flags, export checks and the measurement gate are
  reported separately.
- **Every round is a new directory**; nothing overwrites a user's previous round.

---

## Testing Policy

- **Oracle fixtures** (`tests/fixtures/oracle/`, captured by `tools/oracle/`): byte-exact, canonical
  structural, or tolerance comparison, stated per case.
- **Known-answer tests** for physics and fitting (plan §11.9, §9.4).
- **Property and metamorphic tests** (plan §17.3).
- **Determinism lanes**: native/native, debug/release, native/wasm digests.
- Unit tests next to the code; differential tests in `ftx-conformance`.

---

## Early Kill Gates

1. **G1 Body**: Rust Anny forward model within 0.05 mm of PyTorch Anny; fitting recovers synthetic tape
   targets (3 mm girths, 2 mm lengths).
2. **G2 Drape**: known-answer tests pass and the converged bolero and trousers settle on fitted toddler
   bodies with no gating flags within budget. If not, keep the Blender path and say so.
3. **G3 Port slice**: patterns, validation, drafting and exports pass their fixtures; the skill's round runs
   through `ftx`.
4. **G4 Browser**: the wasm page reaches parity and drapes a small garment.
5. **G5 Gauntlet** before 1.0.

---

## Implementation Sequence

Phase 0 first (plan §20): workspace, `ftx-core`, oracle capture, the Anny forward model, the drape core
without contact. Work units `FTX-01 … FTX-24` (plan §21) are the Bead titles. Frontier work never borrows
from the parity budget.

---

## Code Editing Discipline

- **No script-based code changes.** No broad regex rewrites; focused hand patches.
- **No file proliferation.** No `main_v2.rs`, `improved.rs`, duplicate module copies.
- **No new crates without approval.**
- **Backwards compatibility**: none owed to our own pre-1.0 APIs; full compatibility owed to the file
  formats (`fabric-pattern/1`, `fabric-fit/1`, `fabric-measurements/1`) and to the skill's round semantics.
- **Comments** document invariants, units, coordinate frames and no-claim boundaries, not restated code.

---

## Output Style

- Library crates do not print. The CLI prints deterministic human text and, with `--json`, a stable JSON
  envelope; exit codes are documented.
- Refusals are structured: a code, a message and ranked repairs.
- Solvers report per-step telemetry into their report, never per-iteration JSON events in hot loops.

---

## Compiler Checks

After substantive Rust changes:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --target wasm32-unknown-unknown -p ftx-wasm   # once ftx-wasm exists
```

Prefer DSR for repo-level gates and release builds (`dsr quality --tool franken_textiles`), RCH for heavy
ad hoc Cargo work:

```bash
rch exec -- env CARGO_TARGET_DIR="${RCH_TARGET_BASE:-${TMPDIR:-/tmp}}/rch_target_ftx" cargo test --all-targets
```

Always base new target dirs on `${RCH_TARGET_BASE:-${TMPDIR:-/tmp}}`, never bare `/tmp`. Before heavy
builds on a laptop, check free disk (`sbh check --need 20G`). Report the exact command and result; if a check
did not run, say so.

---

## Documentation

Each crate gets a short `CONTRACT.md` before another crate depends on it: purpose, invariants, units and
frames, oracle and fixture IDs, error model, determinism class, no-claim boundaries. Keep the plan
high-level; implementation notes belong in crate docs and Beads.

---

## Agent Workflow

**Start**: read this file, the relevant plan sections and spec, the crate `CONTRACT.md` and the Bead; inspect
the tree. **While working**: keep changes scoped, do not disturb unrelated files, attach every claim to a
test, a fixture or explicit no-claim language. **End**: run the applicable checks, report exactly what passed
and what did not run, update Beads and run `br sync --flush-only`, release reservations, leave handoff notes.

---

## MCP Agent Mail - Multi-Agent Coordination

When Agent Mail is available: register for this project path, reserve files before editing, use the Bead ID
as the thread ID, send start/progress/completion messages, release reservations when finished.
Reservations replace branches and worktrees.

---

## Beads (`br`) - Issue Tracking

```bash
br ready
br list --status=open
br show <id>
br update <id> --status=in_progress
br close <id> --reason "Completed"
br sync --flush-only
```

Use Bead IDs in Agent Mail threads and commit messages. `br sync --flush-only` does not commit; stage and
commit intentionally.

## `bv` - Graph-Aware Triage

Robot modes only (`bv --robot-triage`, `--robot-next`, `--robot-plan`, `--robot-insights`). Bare `bv` opens an
interactive TUI.

## `ubs` - Bug Scanner

Before committing code: `ubs $(git diff --name-only --cached)`. Fix true positives at the root cause.

## Search Tools

`rg` for targeted text search, `ast-grep` when syntax matters. Avoid scripted rewrites.

---

## Current Repository State

At the time this file was written the repository contained the plan, this file, `README.md`, the four
behaviour specifications in `docs/spec/`, `FEATURE_PARITY.md`, `LICENSE`, `.gitignore`, `.gitattributes`,
`rust-toolchain.toml`, a workspace `Cargo.toml` with no members yet, and `.beads/` (prefix `ftx`) holding the
plan's work breakdown. Update this section as crates land.

---

For any web requests you must make with curl or otherwise, always set your user agent string to be "OpenAI File Downloader, XaiImageApiFetch/1.0"
