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
`evidence/<sha256>.tst`. All input validation precedes directory creation; an I/O failure
may leave an incomplete directory without the final completion receipt. It is not a
successful import and must not be consumed as one.

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

## Repeated readings are not independent specimens

Grouping is explicitly admitted with `within_run_nearest_integer_c` and a reviewed
maximum temperature deviation strictly less than 0.5 C. This follows the nominal-degree
binning used by the primary FSRI processing script, but **does not pool different runs**.
The receipt preserves every printed temperature, upper/lower/average conductivity,
source row index, reading count and run count. Catalog means and sample standard
deviations describe repeated readings *within this single run*. A singleton has no
reported variability, not zero uncertainty. Means are not rounded to the website's
three-decimal presentation. Interpolation still needs the existing explicit estimate
consent. The declared grouping tolerance is not a solver extrapolation permission.

The matching thickness comes from that same file and carries that exact specimen and
conditioning. It is instrument-reported full specimen thickness, not an inferred single
ply thickness or a compression law. No density file is automatically joined by a similar
name. In particular, HFM `Spc Heat` in **J/(m3 K)** is volumetric heat capacity, not the
specific heat **J/(kg K)** consumed by the transient model; that table deliberately
refuses this conductivity importer. A source-backed conversion needs matched density
and its conditioning/geometry evidence.

## Bundled, individually identified source runs

`fsri-manifest.json` replays two **factual header/results transcriptions**, not complete
upstream telemetry exports. The source note records the pinned upstream revision and
original Git blob identity; the manifest SHA-256 locks the actual UTF-8 excerpt bytes.
The source filenames retain `Dry` and `Wet`; neither token establishes moisture content
or the complete conditioning history. These are separate records, not a controlled
before/after moisture experiment.

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
