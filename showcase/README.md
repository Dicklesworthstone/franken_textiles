# The Pattern Room

A Next.js sewing studio that runs FrankenTextiles on the visitor's device.

- **Change the skirt cut.** At `/studies/skirt/`, compare column and flared skirts graded to the selected adult waist. Inspect the bound edge up close, then compare the body and garment sections on the same horizontal plane. Each published size has its own native trajectory, flat source and full-size A4 pattern.
- **Change the wearer and raise the arms.** The drape room on the homepage and `/sections/studies/` shows
  the same bolero on synthetic Anny forms fitted to 48, 52 and 54 cm chest targets. Choose
  a 35°, 75° or 90° arm pose, turn the figure, or replay the flat pieces joining. At the
  52 cm target, the mean chest gap is 23.8 mm at 35° and 8.0 mm at 75°.
  The color views show native panel identities, endpoint strain, or the magnitude
  of the barrier reaction at each welded cloth vertex in newtons. Choose **Coupled outfit** for 12 sewing and 36 gravity steps with the bolero and trousers in one contact solve at 52 cm and 35°. Both
  full-size A4 patterns are available beside the controls.
- **Wear the sundress.** At `/wardrobe/sundress/`, drag either hand or use the arm controls.
  Adjust stature, body mass, bust and waist on an adult figure sampled from the native Anny fit.
  The browser interpolates fitted surfaces and arm poses. The dress keeps its original cut.
  The four-panel pattern, SVG and tiled A4 PDF come from the native pipeline.
- **Dress a hussar.** At `/wardrobe/hussar/`, compare the Swedish Blue and Yellow regiments of
  1761, turn the isolated pelisse, and inspect the dolman's chest cord. The reconstructed garment and its native
  two-pass DST braid sampler share an original motif. The page cites the Swedish military-history
  museums’ description of both uniforms.
- **Test the cloth.** `/experiments/strip/` compares warp, weft and bias cuts of the same synthetic
  material. `/experiments/cylinder/` shows a specimen twisting and compressing between two holders.
  Both fixtures use the synthetic sheet from the coupled outfit. The 16 × 24 planar strip has separate source readings for Sperl et al.’s SJ1 jersey. The 32 × 8 cylinder includes self-contact and compares its grip reaction with a second 24 × 8 run at the same time. The films follow the recorded loading order. The rendered surface uses the CC0 [Rough Linen scan](https://polyhaven.com/a/rough_linen) photographed by colormass and processed by Rico Cilliers. Its 270.7 × 271.3 mm color and normal maps provide appearance only.
- **Draft.** Choose a bolero, trousers or a fez, change the chart size and ease, and pin a piece
  to compare its outline. Download the resulting SVG or tiled A4 PDF.
- **Cut.** Send that exact draft to the cutting table. Change the fabric width and garment count,
  inspect piece and grain placement, and download the full-scale SVG marker.
- **Stitch.** Follow a Tajima DST needle path, pause at the braid placement stop, switch panels,
  and download the machine file being replayed.
- **Drape.** Orbit two native synthetic garment runs in WebGL. Follow a single panel from its
  flat outline to its place in the sewn garment, or inspect a seam and its endpoint strain.
- **Trace.** Reveal the Rust tracer's review of a synthetic bolero sheet, then open the same PNG
  in the workbench to inspect its corners. The starting exercise names three pieces and sets the back hem to 180 mm.
  The print link alongside the exercise opens the native bolero draft.

The chart-based drafts and recorded drapes are separate examples. Gallery previews use
downloadable H.264 films, play muted while visible, pause offscreen and respect
reduced motion. The cover is an AI-generated illustration. The films show the actual exhibits.

## Print the garments

The skirt generator builds new workspaces with `python3 showcase/scripts/prepare-skirt-atelier.py WORK --inputs`.
Use `WORK --solve CUT WAIST_MM` for each cut and size, then `WORK --package OUTPUT`. Packaging checks the
flat waist, joined sewing lines, truing and printed calibration squares. Only settled endpoints with zero gating
flags enter the cut selector. From `showcase/`, `node scripts/render-skirt-atelier.mjs NEW_DIR --posters-only`
captures every published grade. Omit `--posters-only` to record the film. The generated Glen check in
`public/atelier/glen-check.webp` depicts a 16 cm square of oatmeal, chestnut and charcoal yarn with a rust
overcheck. It follows the flat panel coordinates. All three surface finishes share the native synthetic sheet mechanics.

The gallery and film players link to tiled A4 sewing patterns. Print at actual size and measure both squares on the guide, 100 mm and 4 inches. The cutting studio's PDF follows the draft that was transferred into it.

| Garment | Pattern and reference review | Render and print review |
| --- | --- | --- |
| Garden sundress | The native bodice neckline and armhole curves determine the 3D upper edge. The flat pattern has eight checked joins, including both gathers. | Compared front, back, wrist-drag and negative-ease views. The 38-page PDF regenerated byte-for-byte and both calibration squares measured to size. |
| Blue Hussar | Compared the blue dolman, furred pelisse, mirliton, crossbelts and scalloped charivades with Schützercrantz’s 1761 trooper on museum plate 40. | Reviewed clothed, unlayered and rear views and printed tiles. The reconstruction has 49 templates, 187 passing seam walks and 81 A4 pages. |
| Yellow Hussar | Compared the black dolman, yellow cuffs, pale lace, fur cap and chamois pelisse with the 1765 trooper on the same plate. | Inspected cuff clearance, skull plate, belt and matching cap pieces. The reconstruction has 49 templates, 190 passing seam walks and 79 A4 pages. |
| Zouave bolero | Matched the pattern digest to the recorded drape source and checked five panels and ten seams. | Reviewed the replay and all eight A4 pages. The PDF's vector calibration squares have the expected horizontal and vertical dimensions. |
| Gathered trousers | Matched the retained pattern to the native replay, including the waistband, legs and cuffs. | Reviewed the gathered silhouette and all thirteen A4 pages, including calibration geometry. |
| Fez | Checked the 92 cm synthetic chart draft at its 62% crown setting and walked both cap seams. | The five-page native export matches the browser PDF byte-for-byte. Both calibration squares were measured from PDF coordinates. |

The hussar patterns are original reconstructions of textile items. Footwear and metal fittings are supplied separately. A muslin or dressed photograph is still needed to assess fit on a person.

Both sundresses use the same fitted body mesh, a 270 mm front starting radius and a 1 mm cloth contact barrier. The rear starting radii are 240 mm for the gathered cut and 255 mm for the eased cut. On the gathered cut a 120 mm band with a compression factor of 0.002 represents pleats finer than the 20 mm mesh (spec 04 §5.4). Its skirt corners dropped a median 41 mm during gravity while its least-moved corners held their sewn height to within 1 mm. Both runs settled. All 163 recorded physical states of each cut clear the full body and other cloth triangles.

The lightly eased cut shortens each skirt waist edge from 507 to 409.5 mm with the bodice and hem fixed. At the wearer's waist, the centre-front clearance is 90.7 mm on the gathered cut and 31.1 mm on the eased cut. Both studies include the point cache, waist section and full-size PDF. Reproduce the eased version with `prepare-sundress.py --gather-ratio 1.05` and `prepare-sundress-drape.py --lightly-eased`. The adjustable figure illustrates changes in flat bodice ease.

The homepage construction player shows the flat pieces, matching seams and dressed endpoint for each reference cut. The material maps use the same CC0 linen scan as the load experiments. The two dolmans share a solved jacket shell. The fez rolls up from its drafted cone geometry. For the ensemble both garments come from a single joint solve. Each curved panel occupies its own space on the board. The film cuts to the admitted start and plays the recorded sewing and gravity positions.

To reproduce a construction film, use Blender 4.5 with FFmpeg on the path. For the two skirts, bolero, trousers and ensemble, pass the matching native drape directory after the destination. The renderer checks its point-cache digest against the selected exhibit. The sundress and both dolmans use the packaged solver data directly.

```sh
blender -b -P showcase/scripts/render-construction.py -- sundress /path/to/new-dress-film
blender -b -P showcase/scripts/render-construction.py -- ensemble /path/to/new-outfit-film /path/to/joint-drape
```

The original surface data for both figures come from the native Anny model. The face pigment and historical portrait are synthetic illustrations. The full model stays local. Run `scripts/prepare-fitted-bodies.py --help` for the display-sample inputs. To reproduce the historical PDFs, run `python3 scripts/prepare-hussar-patterns.py NEW_DIR`. For the replay PDFs and fez, use `python3 scripts/prepare-study-patterns.py NEW_DIR`.

The four `/workbench/` pages open with native results. Pattern drafts the bolero, Materials lays its five pieces on softened linen, Tracing scales the bolero exercise, and Photo measures a 200 mm span on an illustrated sleeve. Each accepts replacement inputs locally.

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
export CLOUDFLARE_API_TOKEN=...   # or run `bunx --no-install wrangler login` in a terminal with a browser
bun run deploy
```

`bun run deploy` builds the site and publishes two Workers to the Cloudflare account that owns the zone:

- `frankentextiles-studio` (`wrangler.jsonc`) serves the static Next.js export from `out/` at
  `franken-textiles.com`. Its Worker redirects HTTP to HTTPS and handles byte ranges for MP4 seeking.
- `frankentextiles-www` (`www-redirect/`) answers `www.franken-textiles.com` with a permanent redirect
  to the same path on the bare domain.

Afterwards, `sh scripts/check-deploy.sh` confirms the published site: page statuses, the `www` and
HTTPS redirects, byte-range answers for the films, caching and the content security policy.

An API token needs the *Workers Scripts: Edit* account permission, and *Workers Routes: Edit* and
*DNS: Edit* on the zone so Wrangler can attach the two custom domains. Leave `FTX_SITE_ORIGIN` unset
for this build, so canonical links and share-card URLs name `franken-textiles.com`.

`public/_headers` sets long-lived caching for the hashed build output, an hour for films, geometry
and the wasm module, and a content security policy that keeps every request on the site itself.
A workbench page therefore cannot send a pattern, measurements or a photograph anywhere else. Cloudflare
serves the workbench pages without their `.html` extension and redirects the longer form.

The site serves its own fonts, videos, geometry and workbench assets. Besides the exhibition it has
`/how-it-works/`, `/get-started/`, a `robots.txt`, a sitemap and a web manifest. The commands printed on
`/get-started/` were run against this repository; rerun them before changing that page.

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

## How cloth and bodies are drawn

Every scene that shows a solver result uses `components/atelier3d/`, so cloth and bodies look the
same everywhere and no scene invents its own lighting or materials.

- **Cloth.** The solver's vertices are never moved. Around them the cloth is drawn as a shell with a
  face, a reverse and an edge, 2 mm thick, the same thickness the solver holds layers apart at.
  Between solver vertices the surface is refined for display (each triangle split in four, twice,
  following the vertex normals), which rounds a sleeve that would otherwise show flat facets.
  The weave is laid out in flat-pattern coordinates, so it follows each piece's grain. Seams are
  drawn as a ditch with topstitching and unsewn edges as a corded binding or a turned hem. The weave,
  the seam finish and the edge finish are drawing, not simulation.
- **Bodies.** A body is shown as a tailor's form: the fitted body's own shape in a plain matt finish.
  Skin, a face and hair are not part of the body model, so none are drawn.
- **What must not be shown.** A pose with no garment on it, or two garments that were solved
  separately in a view where they overlap. A run the solver refuses is left out of the exhibit and
  its refusal goes in the notes.

To add a scene, call `prepareRenderer`, `lightStudio`, `formMaterial` and `dressCloth`; `CoupledScene.tsx`
is the shortest complete example.

## Rebuild exhibition media

For the homepage experiment, run the following from the repository root with the pinned Anny
model installed, a built `target/release/ftx` and Python with NumPy. Use a fresh scratch path.

```sh
python3 showcase/scripts/prepare-drape-room.py --solve /path/to/new-runs /path/to/new-display-data
```

The generator sews the same five jacket pieces in nine body and arm combinations. All panels
start 25 mm higher along their placement axes. The native checker reviews the admitted starting
position, each sewing step and each gravity step against the complete body and the other cloth.
All nine runs settled without gating flags. The 48 cm, 75° case needed six additional gravity
steps. The other eight settled within the initial 96. Each download includes the exact frame
numbers, endpoint readings and body-fit residuals. The coupled outfit has its own solver run.

The figure uses the native fitted surface. The stand, lighting and gold binding belong to the display.
From `showcase/`, `node scripts/render-drape-room.mjs /path/to/new-film` records the two
52 cm poses and their native contact map into an eight-second H.264 film with FFmpeg.

Each garment comes from a 12 mm mesh with a 2 mm cloth self-contact thickness on a synthetic
92 cm body. The data files include the native PC2 digest, panel ranges, paired seam indices,
solver summary, and original frame numbers. The pose viewer preserves the native float32 body
and cloth coordinates, including the start and end of each phase. The strain view uses the native endpoint colors. Physical fit
requires evidence from a real garment.

Run the extractor against a synthetic native drape directory containing `fold.pc2`,
`mesh_map.json`, `garment_strain.obj` and `sim_report.json`. It checks the exported mesh against the
last PC2 frame and preserves existing, different outputs.

```sh
python3 scripts/prepare-exhibits.py bolero /path/to/synthetic-bolero /path/to/new-exhibits
python3 scripts/prepare-exhibits.py trousers /path/to/synthetic-trousers /path/to/new-exhibits
```

To render the films, start the site, install FFmpeg with libx264, and supply a Chromium executable
or run `bunx --no-install playwright-core install chromium`. The browser records the same
WebGL scenes with a UHD drawing buffer. Each film is encoded at 3840 × 2160 and 60 fps.
The 60 fps camera is sampled independently of the retained solver states.

```sh
CHROMIUM_EXECUTABLE=/path/to/chrome bun run films http://localhost:3000 /path/to/new-media
```

On Linux, `FTX_FILM_OPENGL=1` uses ANGLE's OpenGL backend in a window. For Mesa software rendering
without a desktop, run the same command under `xvfb-run -a` with `LIBGL_ALWAYS_SOFTWARE=1`.

For the construction films, use Blender or the EGL recorder with NumPy, ModernGL 5.13 and
glcontext 3.1. Both use the same source frames and chapter timings. The EGL renderer uses the
actual pattern boundaries, seam pairs and garment meshes with direct lighting.

```sh
python3 scripts/render-native-film.py sundress /path/to/new-media/construction-sundress.mp4
python3 scripts/render-native-film.py bolero /path/to/new-media/construction-bolero.mp4 /path/to/native-solve
node --test scripts/films.test.mjs
```

The trouser films can also be rendered directly from the published native surfaces. Choose `posture`,
`elastic` or `section`. The skirt comparison takes the finished column and flare construction films.

```sh
python3 scripts/render-native-trouser-films.py posture /path/to/new-media/trouser-posture.mp4
sh scripts/render-skirt-comparison.sh /path/to/new-skirt-comparison
```

The workshop uses H.264 with 8-bit 4:2:0 colour for browser playback. Verify the finished files
before replacing `public/media/` assets. Cloudflare accepts files up to 25 MiB per static asset.
Set `FTX_FILM_SOURCE_DIR` when assembling the studio tour or wardrobe reel from a fresh output folder.

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

## Room to move

At `/studies/trousers/`, three benches share one seven-piece pattern and a fitted 92 cm synthetic
form. The posture bench compares standing, shallow bends and a clothed quarter squat (25° hip flexion, 27.5° knee flexion). The bench lists deeper poses refused by the solver. The elastic bench compares casing stiffness in
newtons. Changing 5 N to 60 N reduces the final waistband drop from 16.6 mm to 2.8 mm.
The section bench cuts both meshes at the same height above the sole and draws the intersection
at a common scale. All three benches link to the original thirteen-page sewing PDF.

From the repo root, the following reproduces the inputs, drapes and refusals with
the pinned model installed. The second command records three six-second H.264 films from a
running site into a new directory.

```sh
python3 showcase/scripts/prepare-trouser-study.py --solve /path/to/new-runs /path/to/new-surfaces
FTX_STUDIO_URL=http://localhost:3000 CHROMIUM_EXECUTABLE=/path/to/chrome node showcase/scripts/render-trouser-study.mjs /path/to/new-films
```

### Ranked exhibit ideas

| Rank | Exhibit | What the visitor can learn or make | Issue |
| --- | --- | --- | --- |
| 1 | Trouser cross-sections · shipped | Slide a plane through the folds to see exactly where the cloth leaves room around each leg. | ftx-oew |
| 2 | Elastic waistband · shipped | Compare casing stiffness with the distance the same waistband actually drops. | ftx-1ef |
| 3 | Trouser posture · shipped | Examine shallow bends and see where deep flexion prevents the solver from starting. | ftx-ksp |
| 4 | Wash and recut | Turn the measured warp and weft shrinkage after five washes into a compensated cutting pattern. | ftx-8mf |
| 5 | Heat through layers | Build an exploded stack of measured sheets, change the boundary temperatures and inspect the temperature drop through each layer. | ftx-6ic |
| 6 | Use the remnant | Fit a second garment into the exact polygon left on the cutting table after the first. | ftx-k4g |
| 7 | Repair a seam | Walk mismatched sewing lines, make an alteration and see if they join after the correction. | ftx-9r5 |
| 8 | Redraft the sundress | Change the ease that triggered a flat warning and print the second cut. | ftx-ahm |
| 9 | Assemble a paper pattern | Arrange actual PDF tiles against their registration marks and master outline. | ftx-qiu |
| 10 | Follow the braid in 3D | Pause a registered embroidery path at its placement stop and inspect that point on the dolman. | ftx-f9t |
| 11 | Compare two dress drapes | Orbit two native dress runs at the same scale once qualified garment surfaces are available. | ftx-9zm |
| 12 | Start the swatches sliding | Put a pair of surfaces on a virtual sled and find the transition from sticking to sliding. | ftx-13b |
| 13 | Turn the lining out | Inspect facings and construction order inside a lined jacket. | ftx-z55 |
| 14 | Tie a wrap skirt | Change its overlap and see how that changes coverage on the form. | ftx-kod |
| 15 | Build a period waistcoat | Examine the working pocket and reconstruct its pieces from period references. | ftx-n4x |
