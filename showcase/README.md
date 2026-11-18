# The Drape Studies

A Next.js exhibition of two synthetic FrankenTextiles garment runs. Three.js renders the bolero,
the trousers and a construction view of the bolero. The gallery and its three H.264 films work
on static hosting. The included Pattern Workbench runs the project's Rust WebAssembly module
on the visitor's device.

## Run locally

From this directory, with Bun, the repository's Rust toolchain, its `wasm32-unknown-unknown` target,
and the `wasm-bindgen` CLI version pinned in [the web build](../tools/build_web.sh):

```sh
bun install --frozen-lockfile
bun run workbench
bun dev
```

Open the local URL printed by Next.js. The checked-in geometry and films can be viewed with
`bun dev` alone after dependency installation. The workbench staging step builds the Rust module,
copies the browser tools, and omits debug names to fit Cloudflare's per-file asset limit.
For a prebuilt release from this checkout, set `FTX_WASM_ARTIFACT` to its absolute
`ftx_wasm.wasm` path.

## Publish on Cloudflare

```sh
bun run build
bunx --no-install wrangler deploy
```

`wrangler.jsonc` serves the static Next.js export from `out/` as the
`frankentextiles-studio` Worker. Authenticate Wrangler to the desired account before publishing.
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
