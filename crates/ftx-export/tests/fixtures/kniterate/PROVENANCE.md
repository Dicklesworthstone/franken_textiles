# Independent Kniterate KCode fixtures

These files are exact upstream bytes, retrieved 2026-11-27 through GitHub. They
were not produced or updated by the FrankenTextiles emitter.

Source: <https://github.com/textiles-lab/knitout-backend-kniterate/tree/b19ae3d25fe7b977513362731ed1ff9220dacf69>.
License: MIT, Copyright (c) 2020 Carnegie Mellon Textiles Lab; see `LICENSE`.

| Upstream path | Git blob SHA-1 |
| --- | --- |
| tests/position-keep.k | d38fc80e8ef2cbd34b8fe7e11226823bf9a29a72 |
| tests/position-keep.kc | 305cdad21e526041c9f08df3246f2f00722a382a |
| tests/xfer-style.k | 2595cd5eda3c35a193189411868d0ad99af5b4bf |
| tests/xfer-style.kc | 44c1f7047a5339136fa79fb41b94d6a037500641 |
| tests/split.k | 70ac22569eec37cdb137454b2fce978a0b731147 |
| tests/split.kc | 90e4fc9605f320badd1af1024a9013c050a071d0 |
| LICENSE | 0ce377f6fccfc220fa702ee8f822493aae876ad4 |

The source converter `knitout-to-kcode.js` at this revision has blob
`723b6940b5699e6da3ed077c891f092ddeaa32d1`. Its pass rasters, 15-cell margins,
253 raster positions (252 physical needles plus one unused sentinel), hexadecimal
cam arrays, carrier parking positions, transfer directions and carriage markers
are the format reference. FTX preserves its already-certified zero-based physical
needle addresses; upstream's `Position: Keep` interprets input needles as one-based.

The `position-keep` source independently specifies 7 tucks, 14 knits and 7 drops.
The `xfer-style` source specifies 8 tucks, 72 knits, 68 transfers and 8 drops. Their
checked-in machine files contain respectively 5 and 47 physical passes. These
counts and selected addresses are checked without invoking the candidate emitter.

## Deliberately retained upstream defect

The pinned commit fixes negative-direction split operations to use `Tr-Rl` /
`Rl-Tr`, but its old checked-in `tests/split.kc` still contains `<< Tr-Rr`.
The reader must reject this original fixture. It must not silently repair it,
regenerate it from FTX, or weaken its receiving-cam direction check to accept it.

## Executable pauses and explicit calibrated dials

Kniterate support explicitly documents an uncommented word, including `Pause`,
as an executable stop before a row:
<https://support.kniterate.com/hc/en-us/community/posts/35186942005277-Pause-function-at-line>
(Ariadna Pons, 2026-04-07). FTX emits `Pause` and puts operator instructions on
separate `//` comment lines; comments alone never satisfy the read-back pause check.

Upstream ignores generic Knitout `stitch L T` values. FTX's AST carries calibrated
dimensionless dials in tenths in that operation; this backend converts only exact
whole dials 0..15 to their hexadecimal cam rows and refuses fractional settings.
It never substitutes the upstream default dial or rounds an unsupported setting.

## Actual pass take-down budget

The independent reader also replays every physical pass's roller control and the
oldest loops on each occupied hook. Tucking cannot renew a held loop's age;
transfers and splits carry that age to the receiving hook. Positive roller controls
on empty passes count too. The budget is 100 nominal upstream roller units per
profile holding course; these are neither measured millimetres nor yarn tension.
An over-budget conversion refuses instead of borrowing the source VM's shorter
logical-pass count. The report stays bound to both the exact KCode and the machine
profile, and `physical_takedown_calibrated` remains false.

The Kniterate compiler makes zero-extra-advance segments explicit in Knitout with
`x-add-roller-advance -100`: a multi-pass bind-off or knit/tuck change inside one
logical pass must not silently multiply its take-down. Final manual loop releases
also carry explicit controls. The KCode scheduler does not infer a larger holding
allowance or silently rewrite caller-supplied roller instructions.

The software verifier does not establish firmware execution, waste-yarn setup,
take-down calibration, washed gauge, local yarn-lot behavior or wearer fit. Those
remain separate qualification requirements. Hardware authorization stays false.
