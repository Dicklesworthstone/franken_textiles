import { dirname } from "node:path";
import { fileURLToPath } from "node:url";

export default {
  output: "export",
  trailingSlash: true,
  devIndicators: false,
  turbopack: { root: dirname(fileURLToPath(import.meta.url)) },
};
