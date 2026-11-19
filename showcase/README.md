# The Pattern Room

A Next.js sewing studio that runs FrankenTextiles on the visitor's device.

- **Wear the sundress.** At `/wardrobe/sundress/`, drag either hand or use the arm controls.
  Adjust stature, body mass, bust and waist on an adult figure sampled from the native Anny fit.
  The browser interpolates fitted surfaces and arm poses. The dress keeps its original cut.
  The four-panel pattern, SVG and tiled A4 PDF come from the native pipeline.
- **Dress a hussar.** At `/wardrobe/hussar/`, compare the Swedish Blue and Yellow regiments of
  1761, remove the pelisse, and inspect the chest cord. The reconstructed garment and its native
  two-pass DST braid sampler share an original motif. The page cites the Swedish military-history
  museums’ description of both uniforms.
- **Test the cloth.** `/experiments/strip/` compares warp, weft and bias cuts of the same synthetic
  material. `/experiments/cylinder/` shows a specimen twisting and compressing between two holders.
  Geometry, grip forces, torque, residuals and protocol discrepancies come from downloadable Rust
  experiment reports. The web camera and holder models are illustrative.
- **Draft.** Choose a bolero, trousers or a fez, change the chart size and ease, and pin a piece
  to compare its outline. Download the resulting SVG or tiled A4 PDF.
- **Cut.** Send that exact draft to the cutting table. Change the fabric width and garment count,
  inspect piece and grain placement, and download the full-scale SVG marker.
- **Stitch.** Follow a Tajima DST needle path, pause at the braid placement stop, switch panels,
  and download the machine file being replayed.
- **Drape.** Orbit two native synthetic garment runs in WebGL. Follow a single panel from its
  flat outline to its place in the sewn garment, or inspect a seam and its endpoint strain.
- **Trace.** Reveal the Rust tracer's review of a synthetic bolero sheet, then open the same PNG
  in the workbench to inspect its corners and name the recovered pieces. The sample has no scale calibration.

The chart-based drafts and recorded drapes are separate examples. The homepage has nine
downloadable H.264 films. Gallery previews play muted while visible, pause offscreen and respect
reduced motion. The cover is an AI-generated illustration. The films show the actual exhibits.

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

## Share a section

Each section heading links to its own page. The wardrobe, material experiments and local
workbenches also have individual cards. Open Graph previews are 1200 × 630 and X previews
are 1200 × 600. Next.js builds the artwork from local exhibit renders, actual studio drawings
and the studio fonts. Each format has its own versioned URL.

For a preview deployment, set `FTX_SITE_ORIGIN` to its public HTTPS origin when running the
build. This gives crawlers the correct absolute image URLs. To check the published pages and
download both card formats, run:

```sh
python3 scripts/check-share-images.py https://franken-textiles.com
```

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

For the material exhibits, build the `ftx-materials` binary and run the following from this directory.
The preparation script reads the repository’s cylinder and clamped-strip fixtures, runs the solver,
and saves its inputs and outputs. Both material films use the same state selection as the web views.

```sh
python3 scripts/prepare-material-experiments.py /path/to/new-reports
CHROMIUM_EXECUTABLE=/path/to/chrome node scripts/render-material-experiments.mjs http://localhost:3000 /path/to/new-films
FTX_STUDIO_URL=http://localhost:3000 CHROMIUM_EXECUTABLE=/path/to/chrome node --test scripts/exhibition.test.mjs
```

To reproduce the fixed sundress pattern, build the native `ftx` binary and choose a new output
directory. The optional `--record-drape --model /path/to/converted-model` records the native
solver outcome on a synthetic adult body. The wardrobe films capture the two live WebGL exhibits.
The last command edits the published films in `public/media/` into the homepage loop.

```sh
python3 scripts/prepare-sundress.py /path/to/new-dress
CHROMIUM_EXECUTABLE=/path/to/chrome node scripts/render-sundress.mjs http://localhost:3000 /path/to/new-sundress-film
CHROMIUM_EXECUTABLE=/path/to/chrome node scripts/render-hussar.mjs http://localhost:3000 /path/to/new-hussar-film
sh scripts/render-wardrobe-reel.sh /path/to/new-reel
```

The [ranked exhibit ideas](../.beads/showcase-ideas.jsonl) include acceptance checks and dependencies.
The next garment studies are a wrap skirt with adjustable overlap, a period waistcoat with working
pocket details, and a second checked sundress draft that corrects insufficient ease.
