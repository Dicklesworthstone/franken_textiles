# The Pattern Room

A Next.js sewing studio that runs FrankenTextiles on the visitor's device.

- **Draft.** Choose a bolero, trousers or a fez, change the chart size and ease, and pin a piece
  to compare its outline. Download the resulting SVG or tiled A4 PDF.
- **Cut.** Send that exact draft to the cutting table. Change the fabric width and garment count,
  inspect piece and grain placement, and download the full-scale SVG marker.
- **Stitch.** Follow a Tajima DST needle path, pause at the braid placement stop, switch panels,
  and download the machine file being replayed.
- **Drape.** Orbit two native synthetic garment runs in WebGL. Scrub from flat pattern to settled
  garment, separate panels to see the construction, select a seam or inspect endpoint strain.
- **Trace.** Reveal the Rust tracer's review of a synthetic bolero sheet, then open the same PNG
  in the workbench to inspect its corners and name the recovered pieces. The sample has no scale calibration.

The chart-based drafts and recorded drapes are separate examples. The studio tour and three
garment films are available as downloadable H.264 videos. The cover is an AI-generated illustration.

## Run locally

From this directory, with Bun, the repository's Rust toolchain, its `wasm32-unknown-unknown` target,
and the `wasm-bindgen` CLI version pinned in [the web build](../tools/build_web.sh):

```sh
bun install --frozen-lockfile
bun run workbench
bun dev
```

Open the local URL printed by Next.js. The checked-in geometry and films can be viewed with
`bun dev` alone after dependency installation. Drafting, cutting and tracing use the staged Rust module.
The workbench staging step builds that module,
copies the browser tools, and omits debug names to fit Cloudflare's per-file asset limit.
It uses `gzip` to package a smaller download of the same module for the drafting and cutting studios.
For a prebuilt release from this checkout, set `FTX_WASM_ARTIFACT` to its absolute
`ftx_wasm.wasm` path.

## Publish on Cloudflare

```sh
bun run build
bunx --no-install wrangler deploy
```

`wrangler.jsonc` serves the static Next.js export from `out/` as the
`frankentextiles-studio` Worker and binds its custom domain to `franken-textiles.com`.
Authenticate Wrangler to the Cloudflare account that owns the zone before publishing.
The site serves its own fonts, videos, geometry and workbench assets.

## Rebuild exhibition media

Each garment comes from a 12 mm mesh with a 2 mm cloth self-contact thickness on a synthetic
92 cm body. The data files include the native PC2 digest, panel ranges, paired seam indices,
solver summary, and original frame numbers. The viewer interpolates 49 retained native samples
across each 136-frame fold. The strain view uses the native endpoint colors. Physical fit
requires evidence from a real garment.

Run the extractor against a synthetic native drape directory containing `fold.pc2`,
`mesh_map.json`, `garment_strain.obj` and `sim_report.json`. It checks the exported mesh against the
last PC2 frame and preserves existing, different outputs.

```sh
python3 scripts/prepare-exhibits.py bolero /path/to/synthetic-bolero /path/to/new-exhibits
python3 scripts/prepare-exhibits.py trousers /path/to/synthetic-trousers /path/to/new-exhibits
```

To render the films, start the site, install FFmpeg with libx264, and supply a Chromium executable
or run `bunx --no-install playwright-core install chromium`. The renderer drives the same WebGL
studio at fixed frame positions and passes its images to FFmpeg. The output directory receives
three 720p films, three gallery posters and the hero still.

```sh
CHROMIUM_EXECUTABLE=/path/to/chrome bun run films http://localhost:3000 /path/to/new-media
```

The 20-second studio tour records the drafting and cutting handoff, a native garment replay,
and the embroidery timeline. Stage the workbench before recording, then run:

```sh
CHROMIUM_EXECUTABLE=/path/to/chrome node scripts/render-studio-tour.mjs http://localhost:3000 /path/to/new-tour
```
