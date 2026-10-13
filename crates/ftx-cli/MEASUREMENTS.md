# Measurement workflow

`ftx measure` manages `fabric-measurements/1` evidence and reports which measurements a garment
still needs. All lengths entered as tape readings require a unit; stored lengths are mm and
angles are degrees. Uncertainty is **one sigma**, not a confidence interval.

```sh
# Public-prior-only fictional wearer: never reported as an ordinary measured READY.
ftx measure reference --stature-mm 920 --age-months 30 --out reference.json --json
ftx measure ready reference.json --garment zouave --as-of 2026-10-13 --json

# For a wearer, start an evidence file. Fill tape readings from actual observations, not a chart.
ftx measure init --label "wearer nickname" --out measurements.json
ftx measure catalogue --garment bodice --json
ftx measure catalogue
# Example syntax only: replace VALUE_WITH_UNIT with the observed reading, such as 55cm.
ftx measure add measurements.json --name chest --value VALUE_WITH_UNIT
ftx measure fuse measurements.json --out fused.json --json
ftx measure ready fused.json --garment bodice --as-of 2026-10-13 --json
```

The eight commands are `catalogue`, `prior`, `init`, `add`, `validate`, `fuse`, `ready`, and
`reference`. `prior` and `reference` take `--stature-mm` as a numeric millimetre value.
`init --stature` instead takes an explicitly unit-labelled tape reading and records it as
`MEASURED_TAPE`. `init` requires `--label` and `--out`. Optional metadata: `--age-months`,
`--sex`, `--taken-on`, `--nappies`.

`add FILE --name NAME --value VALUE` accepts repeated `--value` options, plus `--sigma`,
`--provenance`, `--method`, `--definition-id`, `--notes`, and `--out`. Without `--out`, it
updates FILE only after all readings validate. The fused cache is cleared; run `fuse` after
adding evidence. Repeated readings with the same provenance and method are correlated:
their uncertainty does not shrink just because more were entered. Tape uncertainty defaults
to the catalogue. Non-tape observations need an explicit sigma unless the catalogue's
provenance defaults apply (garment-derived, model-fit, scan-measured). Angles need a degree sigma.

`fuse FILES...` and `ready FILES...` concatenate evidence from input files without modifying
them, unless an explicit `--out` requests it. `--no-priors` disables automatic prior influence,
not chart sanity bounds or user-supplied prior records. Sources disagreeing above z=3 request
remeasurement instead of silently averaging. Incompatible definitions are reported and excluded.
Built-in chart priors cover **800–1280 mm stature**. Measurements outside that range are retained,
but there are no built-in chart bounds for them.

`ready --garment KIND` supports bodice, set_in_sleeve, trousers, full_trousers_gathered, hat_fez,
and their catalogue aliases. Options: `--passes-hip`, `--closed-cuff`, `--nappies` or
`--no-nappies`, `--elastic-waist`, `--cuff-level knee|calf|ankle`, and `--cuff-opening`.
A prior-only required value blocks ordinary readiness even when its sigma meets the target.
Reference wearers return `READY_REFERENCE` with `for_real_child` instructions. Staleness is
advisory after 42 days. CLI dates default to **UTC today**; use `--as-of`/`--taken-on` for
reproducible or local-date behavior. The library takes an explicit date and has no hidden clock.

`--json` uses the existing `ftx-cli/1` envelope. `--out` writes the raw domain document rather
than that envelope, so it can be passed directly to subsequent commands. Output writes use a
same-directory temporary file and atomic rename; Unix-created files are mode 0600. On an I/O
failure, a temporary file can remain for recovery. Validation and computation finish before
an existing destination is touched. CLI exits are 0 for success/READY/READY_REFERENCE and 3
for refused input, invalid documents or NOT_READY, following the existing ftx convention
rather than the legacy script's error-code split.

This is evidence readiness, not proof that a real garment fits. No photo calibration, photo
measurement or role-driven round gate is implemented by these commands. Native tests, strict
Clippy and Wasm compilation are checked; physical fit still requires a muslin or dressed photo.
