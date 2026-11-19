// Save the same synthetic 92 cm chart rounds used by the cutting studio.
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import init, * as wasm from "../public/workbench/pkg/ftx_wasm.js";

const dest = process.argv[2];
if (!dest) throw Error("Supply a destination directory for new PDF files.");
await init({ module_or_path: await readFile(new URL("../public/workbench/pkg/ftx_wasm_bg.wasm", import.meta.url)) });
for (const program of ["zouave_bolero", "zouave_trousers", "fez"]) {
  const round = wasm.run(JSON.stringify({ body: {mode: "chart", stature_cm: 92}, program, style: {} }),
    "2026-10-12", "2026-10-12T00:00:00Z");
  try {
    const page = JSON.parse(round.page());
    const pdf = page.files.find(file => file.name.endsWith("_A4.pdf"));
    if (!pdf || !page.exports.checks.startsWith("PASS")) throw Error(JSON.stringify(page.exports));
    await writeFile(join(dest, "chart-" + program + ".pdf"), round.file(pdf.name), {flag: "wx"});
  } finally { round.free(); }
}
