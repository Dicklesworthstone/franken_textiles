import { dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";
import { gunzipSync } from "node:zlib";

const media = new URL("./public/media/", import.meta.url);
const revisions = Object.fromEntries(
  readdirSync(media)
    .filter((name) => /\.(mp4|jpg)$/.test(name))
    .map((name) => {
      const bytes = readFileSync(new URL(name, media));
      return [name, createHash("sha256").update(bytes).digest("hex").slice(0, 12)];
    }),
);

// These studies fetch coordinates separately from the report shown on the page.
// Bind both to the same static build, including hosts that decode a precompressed body in transit.
const geometryAssets = {};
const sri = bytes => "sha256-" + createHash("sha256").update(bytes).digest("base64");
for (const [folder, files, allowDecoded] of [
  ["atelier", /(?:\.gz|-mesh\.json)$/, true],
  ["exhibits", /^(?:bolero|trousers)\.(?:json|bin)$/, false],
  ["exhibits/trouser-study", /(?:\.gz|-topology\.json)$/, false],
  ["exhibits/sundress", /^dress\.(?:json|gz)$/, false],
  ["exhibits/sundress-compare", /^(?:compare\.json|slender\.(?:json|gz))$/, false],
  ["fitted-bodies", /\.(?:gz|json)$/, true],
]) {
  const dir = new URL("./public/" + folder + "/", import.meta.url);
  for (const name of readdirSync(dir).filter(name => files.test(name))) {
    const bytes = readFileSync(new URL(name, dir));
    const representations = allowDecoded && name.endsWith(".gz") ? [bytes, gunzipSync(bytes)] : [bytes];
    geometryAssets["/" + folder + "/" + name] = representations.map(sri).join(" ");
  }
}

export default {
  output: "export",
  trailingSlash: true,
  devIndicators: false,
  env: { NEXT_PUBLIC_FILM_REVISIONS: JSON.stringify(revisions), NEXT_PUBLIC_GEOMETRY_INTEGRITY: JSON.stringify(geometryAssets) },
  turbopack: { root: dirname(fileURLToPath(import.meta.url)) },
};
