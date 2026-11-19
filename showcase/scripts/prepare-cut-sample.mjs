// Run after staging the workbench: node scripts/prepare-cut-sample.mjs NEW-OUTPUT.json
import { readFile, writeFile } from "node:fs/promises";
import init, * as wasm from "../public/workbench/pkg/ftx_wasm.js";
import { initialCut, planCut } from "../public/labs/cut-worker.mjs";

if (!process.argv[2]) throw Error("Supply a new output filename.");
const module_or_path = await readFile(new URL("../public/workbench/pkg/ftx_wasm_bg.wasm", import.meta.url));
await init({ module_or_path });
await writeFile(process.argv[2], JSON.stringify(planCut(wasm, initialCut)) + "\n", { flag: "wx" });
