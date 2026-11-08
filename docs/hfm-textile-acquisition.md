# Source-locked HFM textile acquisition

`import_hfm_textiles.py` imports **individual Wintherm conductivity runs** into the
existing `ftx-textile-catalog/1` schema. It is not another materials authority. The
catalog contains ordinary `Material` records with source-bearing thickness and
`thermal_response.conductivity`, ready for the existing selection/normalization path.
No mass, specific heat, friction, composition, pressure or ply count is invented.

```sh
python3 scripts/import_hfm_textiles.py \
  tests/fixtures/materials/hfm/synthetic-manifest.json --out /tmp/ftx-hfm-example
python3 -m unittest discover -s scripts -p 'test_import_hfm_textiles.py' -v
```

The `synthetic-manifest.json` example is entirely **synthetic**, including its instrument identity.
Use a new output directory: existing results are never replaced. A successful import
writes `catalog.json`, `acquisition.json` and byte-for-byte copies under
`evidence/<sha256>.tst`. It also writes one self-contained selection to
`selections/<source-sha256>.json` for each run. The CLI's JSON output lists each
material ID and its selection path; filenames use source hashes, not specimen IDs.
All input validation precedes directory creation; an I/O failure
may leave an incomplete directory without the final completion receipt. It is not a
successful import and must not be consumed as one.

After a successful import, choose one generated selection under **Or load a
source-bearing selection JSON** in the Material Workbench, then **Inspect sources & capabilities**. The
selection carries the complete imported record, its original source references,
conditioning and source-byte hash; it does not depend on a built-in catalog ID.
Inspect the same file natively, or bind it to a pattern:

```sh
ftx materials inspect OUT/selections/SOURCE-SHA256.json --json
ftx-materials inspect OUT/selections/SOURCE-SHA256.json
ftx-materials bind pattern.json OUT/selections/SOURCE-SHA256.json NEW-PATTERN.json
```

Both inspection commands share the browser's report and read estimate consent from the file.
They do not need a pattern or create output files. Use the actual selection path from the importer's
output and a new destination when binding a pattern. Rust
still validates and normalizes the record and produces its ordinary frozen digest.
The importer does not compute that digest or grant estimate consent:
`allow_estimates` remains false. Interpolation requires the existing explicit
estimate policy, and missing mass, specific heat, mechanics and unloaded clearance
remain unavailable. The catalog and acquisition receipt keep their existing formats.

## Manifest and acquisition boundary

The executable fixture documents every required field. Each run declares an exact
instrument `Sample Name`, a reviewed conditioning description, material form, source
citation/date, explicit retention note, manifest-relative file path and SHA-256 of the
**actual retained bytes**. Keep upstream revision and original-versus-transcribed scope
in the source note. Hash a UTF-8 transcription as a transcription; its hash is not the
hash of the original UTF-16 instrument export. The importer does no network access and
does not imply that a public download grants redistribution rights. Source pages,
photographs and copyrighted narrative are not included by this example.

UTF-16 must carry its endian BOM; UTF-8 is also accepted. Duplicate headers, unknown
columns, non-SI/ambiguous tables, inconsistent printed averages, missing setpoints,
nonpositive or nonfinite values and mismatched source identity refuse. Instrument warnings,
errors, aborted runs, unreached setpoints and manually supplied thickness also refuse;
they are not silently removed from an otherwise useful results table. Files must stay
within the manifest directory, including after symlink resolution. A batch is limited
to 128 runs and 32 MiB; an individual source to 8 MiB and 4096 table rows. Repeated
paths, source bytes, material IDs or Sample Names cannot masquerade as more replicates.

Upper/lower/average consistency is checked exactly at the source's printed precision,
independently of the caller's Decimal precision. Additional decimal places do not
establish instrument accuracy or physical uncertainty; exported observations remain f64.

## Repeated readings are not independent specimens

Grouping is explicitly admitted with `within_run_nearest_integer_c` and a reviewed
maximum temperature deviation strictly less than 0.5 C. This follows the nominal-degree
binning used by the primary FSRI processing script, but **does not pool different runs**.
The deviation limit is inclusive and compares decimal representations of the retained
f64 readings and tolerance, without an extra allowance. A zero limit admits only retained
integer temperatures; half-degree ties exceed every permitted limit.
The receipt preserves every printed temperature, upper/lower/average conductivity,
source row index, reading count and run count. Catalog means and sample standard
deviations describe repeated readings *within this single run*. A singleton has no
reported variability, not zero uncertainty. Means are not rounded to the website's
three-decimal presentation. Interpolation still needs the existing explicit estimate
consent. The declared grouping tolerance is not a solver extrapolation permission.

The matching thickness comes from that same file and carries that exact specimen and
conditioning. It is instrument-reported full specimen thickness, not an inferred single
ply thickness or a compression law. No density file is automatically joined by a similar
name. `conditions.gauge_scope` marks this contact-plate observation for the shared
unloaded-clearance guard: it cannot supply free-sheet sewing or cutter clearance.
Its thermal observation remains available. Existing frozen selections stay immutable;
re-importing carries the corrected qualification into a new selection.
In particular, HFM `Spc Heat` in **J/(m3 K)** is volumetric heat capacity, not the
specific heat **J/(kg K)** consumed by the transient model; that table deliberately
refuses this conductivity importer. A source-backed conversion needs matched density
and its conditioning/geometry evidence.

## Bundled, individually identified source runs

`fsri-manifest.json` replays two **factual header/results transcriptions**, not complete
upstream telemetry exports. The source note records the pinned upstream revision and
original Git blob identity; the manifest SHA-256 locks the actual UTF-8 excerpt bytes.
The source filenames retain `Dry` and `Wet`. The pinned
[Technical Reference Guide, HFM sample preparation](https://github.com/ulfsri/fsri_materials_database/blob/a432697e4b7d40f642b5cc8f384506439ebb32df/04_Documentation/Technical_Reference_Guide/FSRI_MaP_Tech_Guide.tex#L289-L295)
describes unconditioned as-received tests, followed by oven drying at 105 C for at least
48 hours until mass stabilizes. The records now carry this protocol and its source
identity. Mapping the `Wet` group to that as-received stage is an explicit protocol
interpretation, not a measured water loading. Neither group establishes moisture at
each test temperature, contact pressure, ply count or lot identity. These are separate
records, not a controlled before/after moisture experiment.

| Built-in ID | Instrument thickness | Mean conductivity at nominal 15 / 65 C (W/m K) |
| --- | --- | --- |
| `fsri/hfm/cotton-sheet/dry/220330-r1` | 0.30 mm | 0.022633333333333335 / 0.026846666666666668 |
| `fsri/hfm/cotton-sheet/wet/220318-r1` | 0.43 mm | 0.02562666666666667 / 0.030783333333333333 |

Each mean retains three readings from **one run**, with within-run sample SD. All twelve
original printed rows and actual temperatures survive in the acquisition receipt. The
bundled catalog is regenerated from these retained facts and compared exactly in the
Python regression, with independent numerical known answers. No source narratives or
photos are bundled and no general redistribution license is asserted.

```sh
python3 scripts/import_hfm_textiles.py \
  tests/fixtures/materials/hfm/fsri-manifest.json --out /tmp/ftx-fsri-runs
```

The dry R2 file reports `Warning: At least one setpoint was not reached!`. Its warning
and result table are retained in a separate **rejection fixture**, never bundled as an
admitted material. No parsing success can override the instrument warning. The existing
pooled FSRI website records are not rewritten to match these individual runs.

## Primary-source basis and remaining work

FSRI repository version `a432697e4b7d40f642b5cc8f384506439ebb32df`:

- https://github.com/ulfsri/fsri_materials_database/tree/a432697e4b7d40f642b5cc8f384506439ebb32df/01_Data/Cotton_Sheet/HFM
- https://github.com/ulfsri/fsri_materials_database/blob/a432697e4b7d40f642b5cc8f384506439ebb32df/02_Scripts/plot_HFM_data_html.py
- https://materials.fsri.org/materialdetail/cotton-sheet

Attribution: McKinnon, Weinschenk, Dow, DiDomizio and Madrzykowski, *Materials and
Products Database*, version 1.0.0, FSRI, UL Research Institutes (2023). Source inspection
was on 2026-10-15. The library's existing pooled website records remain separate.

This is a software acquisition slice of `ftx-tme`, not closure of its empirical gate.
Matched mass/area, ply count, pressure, lot linkage, thermal history and independent
held-out validation remain necessary for a qualified garment model. Drape readiness,
transient readiness, temperature ratings, flammability, cut protection and robot
execution are not established by successfully importing a conductivity table.

The 2026-11-01 source review also checked the pinned dry cotton-sheet R1 heat-capacity
export and density CSV. The conductivity run reports 0.30 mm, while the heat-capacity
run reports 0.27 mm and volumetric `Spc Heat` in J/(m3 K). The upstream processing script
divides pooled volumetric observations by a conditioning-group mean density; that does
not establish the matched specimen/geometry conversion required here. The guide's
nominal 200 mm square also permits smaller specimens with surrounding filler, so it
cannot supply an unreported actual area. Specific heat, areal mass and transient
readiness remain unavailable in these records. The reviewed guide has SHA-256
`9df19964f73ea1a6bcdaa930da954def5d422bf41b6127eb5806219e0cc42726`;
raw instrument excerpts, their hashes and all conductivity readings are unchanged.
