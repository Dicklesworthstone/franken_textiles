# Decoded video frame selection

`ftx measure video-select` implements the decoded-frame selection stage of FTX-20 / plan
§10.2. It scores **actual pixels** separately inside the supplied person and reference-sheet
masks, then chooses one eligible frame per populated decoded-time window. It does not use
whole-image sharpness: a sharp background or sheet cannot compensate for a blurred person.

```sh
ftx measure video-select capture/frames.json --out capture/selection.json
ftx measure video-select capture/frames.json --json
```

`--out` is optional and create-only. It cannot overwrite a frame, mask, manifest or earlier
report, including an existing symlink or hard-link alias. Output files are private (0600 on
Unix). No media or report is uploaded and no external process is executed.

## Decoded input manifest

```json
{
  "schema": "ftx-video-frames/1",
  "decode_receipt": "Local decoder log: presentation timestamps retained, display rotation applied to pixels and both masks, lens epoch recorded.",
  "options": {
    "window_us": 150000,
    "min_interior_pixels": 64,
    "min_person_variance": 10.0,
    "min_sheet_variance": 10.0,
    "max_focal_change_ratio": 0.01
  },
  "frames": [
    {
      "metadata": {
        "frame": { "stream": "camera-1", "timestamp_us": 133333 },
        "rotation_cw_deg": 90,
        "camera_epoch": "lens-1-calibration-a",
        "focal_px": [1500.0, 1500.0]
      },
      "image": "frames/frame-133333.png",
      "person_mask": "masks/person-133333.png",
      "sheet_mask": "masks/sheet-133333.png"
    }
  ]
}
```

Paths resolve relative to the manifest's canonical directory, not the shell's current
directory. Absolute paths also work. A report retains the source manifest path, canonical
input ordering, complete acquisition metadata, decode receipt and exact policy defaults.
The receipt is supplied evidence, **not a verification that decoding or orientation was correct**.

Decode video externally, preserving decoded presentation timestamps rather than estimating
times from frame ordinals or a nominal frame rate. Apply display rotation to images and masks
before calling this command. `rotation_cw_deg` records the already-applied clockwise rotation
(0, 90, 180 or 270); it does not request another rotation. Each `(stream, timestamp_us)` must
be unique. Timestamps are nonnegative integer microseconds on a declared stream clock; this
command does not align clocks between cameras.

Frames must be opaque 8-bit grayscale or RGB PNGs. Masks must be 8-bit grayscale PNGs containing
only 0 (outside) and 255 (inside), with exactly the oriented image's width and height. No
resizing, mask inference or thresholding is performed. Transparency, embedded EXIF and animated
PNGs are refused: remove these metadata/encodings after correctly decoding and orienting the
pixels. The manifest is bounded to 16 MiB and 20,000 frames; each PNG to 128 MiB and 40 million
pixels. Images are processed sequentially, not all held in memory. PNG inflation is bounded
by the declared scanline sizes, including interlaced images.

## Selection and diagnostics

The score is population variance of the four-neighbour Laplacian of 8-bit grayscale pixels.
Only samples whose **entire 3 × 3 neighbourhood lies inside the mask and image** contribute.
Thus the mask boundary or unrelated background cannot supply a false sharpness score. Empty
or thin masks retain an absent score, rather than manufacturing a zero-uncertainty result.
The integer accumulation is bounded and deterministic on native and WebAssembly targets.

Both regions must meet their own minimum interior count and variance threshold. Among eligible
candidates the command maximizes the **weaker normalized region score**; ties choose the earlier
decoded timestamp. Thresholds are configurable starting policies, not empirical accuracy
certificates. Noise, exposure, resolution and texture affect Laplacian variance. A blank sheet
or textureless person region may provide no usable sharpness evidence even when in focus;
the command reports that limitation rather than silently selecting a frame. Use a reference
with interior visual features when the plain sheet cannot provide enough evidence.

Windows must be 100,000–250,000 microseconds. They are anchored at the first observed frame of
each acquisition segment; empty time gaps do not invent frames or windows. A new segment starts
on a stream change, caller-declared lens/calibration epoch, resolution or rotation change,
calibrated focal-length availability change, or excessive focal drift. Drift is measured from
the segment anchor, so gradual zoom cannot evade the threshold. `focal_px` is optional and must
be calibrated in the oriented image's pixels. Missing calibration yields an advisory: an
undeclared zoom or lens switch cannot be detected there. No focal length is guessed from EXIF.

Every candidate remains in the report, with mask/interior counts, separate variances, rejection
reasons and selection identity. A populated window with no eligible candidate keeps
`selected: null`; there is no fallback to a bad frame. Exit 0 means every populated window
selected a frame (`SELECTED`); exit 2 means some windows failed (`PARTIAL` or
`NO_USABLE_FRAMES`). Exit 3 is an invalid document, file, policy or output-write refusal.
`--json` uses the usual CLI envelope; `--out` contains its `result` document directly.

## Boundary with reconstruction

**Sharpness is not stillness.** These windows must not be promoted to the reviewed still-window
or synchronized-capture evidence required by `ftx measure triangulate`. A moving person can
be perfectly sharp in every image. Frame selection does not estimate motion, metric scale,
camera calibration, pose, silhouette correctness or body-fit uncertainty. The mask and epoch
labels remain caller-supplied. Automatic decoding, masks and motion classification remain
separate work; no real-wearer fit claim follows from a selected frame.

Rust callers can use `ftx_measure::video::{score_frame, select_frames}` directly with
already-oriented grayscale/mask buffers. These APIs have no I/O or image-decoder dependency.
The CLI reuses the existing trace PNG and grayscale implementations, without adding a crate.