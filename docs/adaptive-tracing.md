# Trace pattern sheets with uneven lighting

The native trace accepts `--threshold adaptive` in addition to the default `otsu`.
Use the adaptive mode when one global brightness cutoff confuses a shadow with a
piece or loses lighter strokes elsewhere in the same image:

```sh
ftx trace oriented-sheet.png --channel dark --threshold adaptive --out traced
ftx audit traced/draft.pattern.json --out audit
```

`dark` is for dark ink; `light` is for light ink. The default `auto` channel still
uses Lab distance from the estimated background. `gray`, `r`, `g`, `b` and `sat`
retain the usual Otsu-frame polarity heuristic. `--invert` and `--no-invert`
override it. Inversion happens to the **channel before local averaging**, not to
the resulting binary mask. Crop and erase processing still precede thresholding;
closing, contours, stroke-centre geometry, marks and review images are unchanged.

The adaptive threshold compares each channel value with a local Gaussian mean
rounded to the nearest 8-bit level, and retains values more than 10 levels above
that mean (C = -10). Its square window is `max(15, round(min(width,height)/20) | 1)`
pixels, with nearest-even rounding before making it odd, and replicated borders.
Otsu's cutoff and border foreground share are still recorded for diagnosing polarity; the actual method
is in `meta.trace.threshold.method`. No automatic mode switch is made.

The library uses separable float32 filtering with deterministic kernel construction
and accumulation order, bounded by the existing 40-million-pixel raster ceiling.
It does not call Python or OpenCV at runtime, and adds no dependency. Full CLI
image/review limits still apply. A faint line below the contrast margin may vanish,
and texture may become foreground: inspect the mask and overlay rather than treating
successful tracing as a guarantee of a correct piece inventory.

Tracing never establishes physical scale. Adaptive drafts remain `UNSCALED`, and
true-size exports still refuse them. Annotate the pieces and establish scale from
a justified measurement before exporting, for example:

```sh
ftx scale-fit traced/draft.pattern.json --target p1:width=120mm --out scaled.json
ftx export svg scaled.json --out scaled-export
```

The 120 mm value is an example, not a scale inferred from image appearance.

Verification: all 22 existing OpenCV 5 whole-trace cases now match exactly: drafts,
diagnostics and masks on successful traces, and refusal text on invalid input.
The adaptive case is no longer exempted. A separate
OpenCV 4.13 capture checks 36 kernel masks (impulses, textures, illumination ramps,
narrow images, multiple window sizes and rounding boundaries) without replacing
the pinned reference.
CLI regressions cover a shadowed sheet, explicit polarity, unchanged default Otsu,
unknown methods, and the trace-to-scale-to-export boundary. This is fixture parity,
not universal parity with every OpenCV hardware acceleration path. JPEG/WebP input,
the remaining buffer fallback and browser tracing controls are separate work.
