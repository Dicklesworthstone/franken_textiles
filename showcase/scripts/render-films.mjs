// With the showcase running locally: node scripts/render-films.mjs [url] [output-directory] [study]
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { captureOptions, enableFilmResolution, filmArgs, fps, frameCapture } from "./film-capture.mjs";

const base = new URL(process.argv[2] || "http://localhost:3000");
const target = path.resolve(process.argv[3] || fileURLToPath(new URL("../public/media/", import.meta.url)));
const studies = [
  { id: "bolero", title: "Zouave bolero", angle: 0.32 },
  { id: "trousers", title: "Zouave trousers", angle: -0.35 },
  { id: "construction", title: "Panel construction", angle: 0.22 },
].filter(study => !process.argv[4] || study.id === process.argv[4]);
if (studies.length === 0) throw new Error("Choose bolero, trousers or construction.");

const duration = 8;
const movie = { width: 1280, height: 720 };
const frames = fps * duration;
const startFrame = Number(process.env.FTX_FILM_START_FRAME ?? 0);
if (!Number.isInteger(startFrame) || startFrame < 0 || startFrame >= frames) {
  throw new Error(`FTX_FILM_START_FRAME must be an integer from 0 through ${frames - 1}.`);
}
const ease = t => t * t * (3 - 2 * t);
const unit = t => Math.max(0, Math.min(1, t));

async function draw(page, progress, yaw) {
  await page.evaluate(async ([position, angle]) => {
    await window.ftxFrame(position, angle);
  }, [progress, yaw]);
}

async function still(page, name) {
  const bytes = await page.screenshot({ type: "jpeg", quality: 95 });
  await writeFile(path.join(target, name + ".jpg"), bytes, { flag: "wx" });
}

// A matching pair of exposures shows the native panel mesh through the right sleeve.
async function hero(page) {
  const size = { width: 1080, height: 1080 };
  await page.setViewportSize(size);
  const detail = await page.context().newPage();
  try {
    const construction = new URL(base);
    construction.searchParams.set("capture", "construction");
    await detail.setViewportSize(size);
    await detail.goto(construction.href);
    await detail.waitForFunction(() => typeof window.ftxFrame === "function", undefined, { timeout: 90000 });
    const exposures = [];
    for (const view of [page, detail]) {
      await view.bringToFront();
      await view.addStyleTag({ content: "nextjs-portal {display:none!important}" });
      await draw(view, 1, -0.43);
      exposures.push("data:image/png;base64," + (await view.screenshot()).toString("base64"));
    }
    await detail.evaluate(async sources => {
      const layers = await Promise.all(sources.map(async src => {
        const image = new Image();
        image.src = src;
        await image.decode();
        image.style.cssText = "width:100%;height:100%;position:absolute;inset:0;transform:scale(1.15) rotate(-8deg)";
        return image;
      }));
      layers[1].style.maskImage = "linear-gradient(112deg, transparent 59%, black 67%)";
      const mount = document.createElement("div");
      mount.style.cssText = "position:fixed;inset:0;overflow:hidden;z-index:999;background:#232723";
      mount.append(...layers);
      document.body.append(mount);
    }, exposures);
    await still(detail, "hero");
  } finally {
    await detail.close();
    await page.setViewportSize(movie);
  }
}

// Hold the laid-out pattern briefly, retain the native frame order, then orbit the sewn result.
async function encode(page, study) {
  const screenshot = await frameCapture(page);
  await page.evaluate(async ({ id, title }) => {
    const garment = id === "trousers" ? id : "bolero";
    const response = await fetch("/exhibits/" + garment + ".json");
    if (!response.ok) throw new Error("Missing fold timing for " + garment);
    const { phases, sourceFrames } = await response.json();
    const slate = document.createElement("aside");
    slate.style.cssText = "position:fixed;inset:auto 4.4% 5.4%;display:flex;align-items:baseline;justify-content:space-between;border-top:1px solid #d7d8bf55;padding-top:10px;pointer-events:none;color:#fbf4e3;z-index:10";
    const name = document.createElement("b");
    name.style.cssText = "font:normal 25px Georgia,serif;letter-spacing:-0.7px";
    name.textContent = title;
    const phase = document.createElement("span");
    phase.style.cssText = "font:11px ui-monospace,monospace;letter-spacing:1.8px;text-transform:uppercase";
    slate.append(name, phase);
    document.body.append(slate);
    window.ftxFilmLabel = (progress) => {
      const frame = Math.round(progress * (sourceFrames - 1));
      phase.textContent = frame === 0 ? "Pattern board"
        : frame < phases.fold ? "Wrapping panels"
        : frame < phases.fold + phases.zip ? "Match the edges"
        : frame < phases.fold + phases.zip + phases.sew ? "Sewing"
        : frame < sourceFrames - 1 ? "Gravity drape"
        : "Drape endpoint";
    };
  }, study);
  const output = path.join(target, study.id + ".mp4");
  const ffmpeg = spawn("ffmpeg", filmArgs(output), { stdio: ["pipe", "ignore", "inherit"] });
  const finished = once(ffmpeg, "close");
  ffmpeg.stdin.on("error", () => {}); // The exit status reports a rejected input or output.
  try {
    for (let index = startFrame; index < frames; index++) {
      const time = index / (frames - 1);
      const progress = ease(unit((time - 0.06) / 0.66));
      const orbit = 1.14 * ease(unit((time - 0.64) / 0.36));
      await draw(page, progress, study.angle + orbit);
      await page.evaluate(progress => window.ftxFilmLabel(progress), progress);
      const bytes = await screenshot();
      await new Promise((resolve, reject) => {
        ffmpeg.stdin.write(bytes, error => error ? reject(error) : resolve());
      });
    }
  } finally {
    ffmpeg.stdin.end();
    const [code] = await finished;
    if (code !== 0) throw new Error("FFmpeg failed for " + study.id + " (exit " + code + ").");
  }
}

await mkdir(target, { recursive: true });
const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM_EXECUTABLE || process.env.FTX_CHROMIUM_EXECUTABLE || chromium.executablePath(),
  args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"],
});
try {
  const context = await browser.newContext(captureOptions(movie));
  await enableFilmResolution(context);
  const page = await context.newPage();
  page.setDefaultTimeout(120000);
  async function openStudy(study) {
    const url = new URL(base);
    url.searchParams.set("capture", study.id);
    await page.goto(url.href);
    await page.waitForFunction(() => typeof window.ftxFrame === "function", undefined, { timeout: 90000 });
    await page.addStyleTag({ content: "nextjs-portal {display:none!important}" });
  }
  for (const study of startFrame === 0 ? studies : []) {
    await openStudy(study);
    if (study.id === "bolero") {
      await hero(page);
    }
    await draw(page, study.id === "construction" ? 0.29 : 1, study.angle);
    await still(page, study.id);
  }
  for (const study of studies) {
    await openStudy(study);
    await encode(page, study);
    console.log(`${study.title}: rendered frames ${startFrame}–${frames - 1} at ${fps} fps.`);
  }
} finally {
  await browser.close();
}
