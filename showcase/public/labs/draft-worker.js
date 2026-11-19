// Use the same chart, draft, validation and export round as the full Pattern Workbench.
import { getStudioEngine } from "./studio-engine.mjs";

self.onmessage = async ({ data }) => {
  const { key, program, height, style } = data;
  let round;
  try {
    const { run } = await getStudioEngine();
    const now = new Date().toISOString();
    round = run(JSON.stringify({ program, body: { mode: "chart", stature_cm: height }, style }),
      now.slice(0, 10), now.slice(0, 16));
    const page = JSON.parse(round.page());
    if (!page.preview_svg || page.errors?.length) {
      throw new Error(page.errors?.map(e => typeof e === "string" ? e : e.message).join(" · ") ||
        "The engine could not produce a checked pattern at these settings.");
    }
    const files = {};
    const transfers = [];
    for (const [type, suffix] of [["pdf", "_A4.pdf"], ["svg", "_master_mm.svg"]]) {
      const source = page.files.find(file => file.name.endsWith(suffix));
      if (!source) continue;
      const bytes = round.file(source.name);
      files[type] = { bytes, name: source.name };
      transfers.push(bytes.buffer);
    }
    const seams = page.validation.seams;
    const pattern = new TextDecoder("utf-8", { fatal: true }).decode(round.file("pattern.json"));
    self.postMessage({ key, pattern, preview: page.preview_svg, files, checks: page.exports.checks,
      flat: page.basis.flat, passed: seams.filter(seam => seam.status === "PASS").length,
      total: seams.length, offTarget: page.validation.points
        .filter(point => point.status === "LOW" || point.status === "HIGH").map(point => point.label),
    }, transfers);
  } catch (error) {
    self.postMessage({ key, error: String(error?.message || error) });
  } finally {
    round?.free();
  }
};
