import { dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";

const media = new URL("./public/media/", import.meta.url);
const revisions = Object.fromEntries(
  readdirSync(media)
    .filter((name) => /\.(mp4|jpg)$/.test(name))
    .map((name) => {
      const bytes = readFileSync(new URL(name, media));
      return [name, createHash("sha256").update(bytes).digest("hex").slice(0, 12)];
    }),
);

export default {
  output: "export",
  trailingSlash: true,
  devIndicators: false,
  env: { NEXT_PUBLIC_FILM_REVISIONS: JSON.stringify(revisions) },
  turbopack: { root: dirname(fileURLToPath(import.meta.url)) },
};
