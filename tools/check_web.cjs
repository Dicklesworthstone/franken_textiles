// Drives web/ in headless Chromium: a round per program (chart body), the tape form, a refusal, and with
// --model a fold-up per garment, compared byte for byte with `ftx drape` on the same round's documents.
//
//   tools/build_web.sh && cargo build --release -p ftx-cli
//   NODE_PATH=$(npm root -g) node tools/check_web.cjs [--model PATH.ftxbody] [--ftx target/release/ftx]
//
// Needs Playwright with its Chromium (npm i -g playwright && npx playwright install chromium).
const { chromium } = require("playwright");
const { spawn, execFileSync } = require("child_process");
const crypto = require("crypto");
const fs = require("fs");
const os = require("os");
const path = require("path");

const arg = (name) => { const i = process.argv.indexOf(name); return i > 0 ? process.argv[i + 1] : null; };
const root = path.resolve(__dirname, "..");
const model = arg("--model");
const ftx = arg("--ftx") || path.join(root, "target/release/ftx");
const sha = (b) => crypto.createHash("sha256").update(b).digest("hex");
let failed = 0;
const check = (ok, what) => { console.log(`${ok ? "ok  " : "FAIL"} ${what}`); if (!ok) failed++; };

(async () => {
  const port = 20000 + Math.floor(Math.random() * 20000);
  const srv = spawn("python3", ["-m", "http.server", String(port), "-d", path.join(root, "web")], { stdio: "ignore" });
  await new Promise((r) => setTimeout(r, 1200));
  const browser = await chromium.launch();
  const page = await browser.newPage();
  const errors = [];
  page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
  page.on("pageerror", (e) => errors.push(String(e)));
  const runs = async () => +(await page.evaluate(() => document.body.dataset.runs || 0));
  const settle = async (click, timeout) => {
    const n = await runs();
    await page.click(click);
    await page.waitForFunction((n) => +(document.body.dataset.runs || 0) > n, n, { timeout });
    return page.$eval("#errorBox", (e) => (e.classList.contains("hidden") ? "" : e.textContent.trim()));
  };
  const bytes = async (sel) => Buffer.from(await page.$eval(sel, async (a) => {
    const b = new Uint8Array(await (await fetch(a.href)).arrayBuffer());
    let s = "";
    for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode.apply(null, b.subarray(i, i + 0x8000));
    return btoa(s);
  }), "base64");
  const work = fs.mkdtempSync(path.join(os.tmpdir(), "ftx-check-web-"));
  try {
    await page.goto(`http://localhost:${port}/`);
    await page.waitForFunction(() => /Ready|Could not/.test(document.getElementById("status").textContent), null, { timeout: 60000 });
    check(/Ready/.test(await page.textContent("#status")), "the module loads");
    for (const [program, status] of [["zouave_bolero", "CONVERGED"], ["zouave_trousers", "CONVERGED"], ["fez", "CONVERGED"]]) {
      await page.check(`input[name=program][value=${program}]`);
      const err = await settle("#go", 120000);
      check(!err && (await page.textContent("#resStatus")) === status, `${program} round ${status}`);
    }
    await page.check("input[name=program][value=zouave_trousers]");
    await page.check("input[name=bodyMode][value=tape]");
    await page.waitForFunction(() => document.querySelectorAll("#tapeRows input").length > 0, null, { timeout: 30000 });
    await settle("#go", 120000);
    check((await page.textContent("#resStatus")) === "MEASUREMENTS_NOT_READY", "an empty tape form is NOT_READY");
    await page.check("input[name=bodyMode][value=chart]");
    await page.fill("#stature", "9");
    check(/enter centimetres/.test(await settle("#go", 120000)), "a 9 cm stature is refused");
    await page.fill("#stature", "92");
    if (model) {
      await page.setInputFiles("#bodyFile", model);
      await page.waitForFunction(() => /Body model loaded/.test(document.getElementById("status").textContent), null, { timeout: 120000 });
      for (const program of ["zouave_bolero", "zouave_trousers"]) {
        await page.check(`input[name=program][value=${program}]`);
        await settle("#go", 120000);
        const dir = path.join(work, program);
        fs.mkdirSync(dir);
        fs.writeFileSync(path.join(dir, "round.zip"), await bytes("a[download$='.zip']"));
        execFileSync("unzip", ["-q", "round.zip"], { cwd: dir });
        const t0 = Date.now();
        const err = await settle("#foldGo", 900000);
        const s = ((Date.now() - t0) / 1000).toFixed(1);
        check(!err && /no gating flag/.test(await page.textContent("#foldSummary")), `${program} folds up in the page (${s} s), settled, no gating flag`);
        if (err) continue;
        const viewer = await bytes("a[download='fold_viewer.html']");
        execFileSync(ftx, ["drape", "pattern.json", "--measurements", "measurements.json", "--fit", "fit_spec.json",
          "--self-contact-mm", "2", "--model", model, "--out", "native"], { cwd: dir, stdio: "ignore" });
        const native = fs.readFileSync(path.join(dir, "native/fold_viewer.html"));
        check(sha(viewer) === sha(native), `${program} fold-up equals ftx drape's, byte for byte (${sha(native).slice(0, 12)})`);
      }
    }
    check(errors.length === 0, `no console errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
  } finally {
    await browser.close();
    srv.kill();
    console.log(`work files in ${work}`);
  }
  process.exit(failed ? 1 : 0);
})().catch((e) => { console.error("FAILED", e); process.exit(1); });
