// Drives web/ in headless Chromium: a round per program (chart body), the tape form, a refusal, and with
// --model a fold-up per garment, compared byte for byte with `ftx drape` on the same round's documents.
//
//   tools/build_web.sh && cargo build --release -p ftx-cli
//   NODE_PATH=$(npm root -g) node tools/check_web.cjs [--model PATH.ftxbody] [--ftx target/release/ftx]
//   node tools/check_web.cjs --viewer /path/to/fold_viewer.html   # offline file:// rendering and controls
//   node tools/check_web.cjs --model PATH.ftxbody --timings 20   # browser timing lane; no native comparison
//   node tools/check_web.cjs --calibration-sheets NATIVE_DIR  # six local downloads, without page JavaScript
//
// Needs Playwright with its Chromium (npm i -g playwright && npx playwright install chromium).
const { chromium } = require(process.env.FTX_PLAYWRIGHT_MODULE || "playwright");
const { spawn, execFileSync } = require("child_process");
const crypto = require("crypto");
const fs = require("fs");
const os = require("os");
const path = require("path");
const { pathToFileURL } = require("url");
const { performance } = require("perf_hooks");

const arg = (name) => { const i = process.argv.indexOf(name); return i > 0 ? process.argv[i + 1] : null; };
const root = path.resolve(__dirname, "..");
const model = arg("--model");
const standalone = arg("--viewer");
const calibrationSheets = arg("--calibration-sheets");
const timingArg = arg("--timings");
const timingRuns = Number(timingArg || 0);
if (process.argv.includes("--timings") && (!model || standalone || !/^[1-9][0-9]*$/.test(timingArg || "") || timingRuns > 100)) {
  throw Error("--timings needs --model and an integer run count from 1 to 100; it is separate from --viewer");
}
if (process.argv.includes("--calibration-sheets") && (!calibrationSheets || model || standalone || timingArg)) {
  throw Error("--calibration-sheets needs a native capture directory and runs separately from model/viewer/timing checks");
}
const userAgent = "OpenAI File Downloader, XaiImageApiFetch/1.0";
const ftx = arg("--ftx") || path.join(root, "target/release/ftx");
const sha = (b) => crypto.createHash("sha256").update(b).digest("hex");
let failed = 0;
const check = (ok, what) => { console.log(`${ok ? "ok  " : "FAIL"} ${what}`); if (!ok) failed++; };

async function printableReferences(browser, origin, expectedDir) {
  const assert = require("node:assert/strict");
  const context = await browser.newContext({ javaScriptEnabled: false, acceptDownloads: true, userAgent });
  const page = await context.newPage(), errors = [], requests = [];
  const work = fs.mkdtempSync(path.join(os.tmpdir(), "ftx-calibration-downloads-"));
  page.on("pageerror", e => errors.push(String(e)));
  page.on("request", r => requests.push({ url: r.url(), method: r.method() }));
  try {
    await page.goto(origin);
    await page.locator("#calibrationSheets summary").click();
    const panel = page.locator("#calibrationSheets");
    assert.match(await panel.textContent(), /100% \/ Actual size/);
    assert.match(await panel.textContent(), /both axes with a ruler/);
    assert.equal(await panel.locator("a[download]").count(), 6);
    for (const [paper, size, rows, originPx] of [["letter", [2550, 3300], 6, [135, 94]], ["a4", [2480, 3508], 7, [100, 94]]]) {
      for (const ext of ["pdf", "png", "json"]) {
        const name = `calib-sheet-${paper}.${ext}`, link = panel.locator(`a[download="${name}"]`);
        assert.equal(await link.getAttribute("href"), `calibration/${name}`);
        const pending = page.waitForEvent("download");
        await link.click();
        const download = await pending;
        assert.equal(download.suggestedFilename(), name);
        assert.equal(await download.failure(), null);
        const actual = fs.readFileSync(await download.path());
        assert.ok(actual.equals(fs.readFileSync(path.join(expectedDir, name))), `${name} differs from native output`);
        if (ext === "png") {
          assert.equal(actual.subarray(0, 8).toString("hex"), "89504e470d0a1a0a");
          assert.deepEqual([actual.readUInt32BE(16), actual.readUInt32BE(20)], size);
          assert.equal(actual.toString("ascii", 37, 41), "pHYs");
          assert.deepEqual([actual.readUInt32BE(41), actual.readUInt32BE(45), actual[49]], [11811, 11811, 1]);
        } else if (ext === "pdf") {
          assert.equal(actual.subarray(0, 8).toString(), "%PDF-1.4");
          assert.ok(actual.includes(Buffer.from("/BitsPerComponent 1 /Interpolate false")));
          assert.ok(actual.includes(Buffer.from(`/MediaBox [0 0 ${(size[0] * 0.24).toFixed(6)} ${(size[1] * 0.24).toFixed(6)}]`)));
        } else {
          const sidecar = JSON.parse(actual);
          assert.equal(sidecar.schema, "fabric-calib-sheet/1"); assert.equal(sidecar.paper, paper);
          assert.equal(sidecar.dpi, 300); assert.deepEqual(sidecar.image_px, size);
          assert.deepEqual([sidecar.board.squares_x, sidecar.board.squares_y, sidecar.board.square_mm, sidecar.board.marker_mm], [8, rows, 22, 16]);
          assert.equal(sidecar.board.dictionary, "DICT_4X4_50"); assert.equal(sidecar.board.legacy_pattern, false);
          assert.deepEqual(sidecar.board.origin_px, originPx); assert.equal(sidecar.pdf_lossless, true);
          assert.deepEqual(sidecar.bars.map(b => b.nominal_mm), [100, 101.6, 100, 101.6]);
          assert.deepEqual(sidecar.test_squares.map(s => s.nominal_mm), [100, 25.4]);
          assert.deepEqual(sidecar.files, { png: `calib-sheet-${paper}.png`, pdf: `calib-sheet-${paper}.pdf` });
          assert.equal(Object.hasOwn(sidecar, "paths"), false);
        }
        fs.writeFileSync(path.join(work, name), actual, { flag: "wx" });
        console.log(`PASS ${name}: actual no-JavaScript browser download equals native bytes (${sha(actual)})`);
      }
    }
    assert.deepEqual(errors, []);
    assert.ok(requests.every(r => r.url.startsWith(origin + "/") && r.method === "GET"));
    await panel.screenshot({ path: path.join(work, "references.png") });
    console.log(`PASS: all six printable references work without page JavaScript; local GET only, no page errors. Digital interoperability only; physical print/measurement accuracy unproven. Artifacts: ${work}`);
  } finally { await context.close(); }
}

async function offlineViewer(browser, file) {
  const html = fs.readFileSync(file, "utf8");
  const imports = JSON.parse(html.split('<script type="importmap">')[1].split('</script>')[0]).imports;
  for (const [name, digest] of [
    ["three", "3e690ac7d180b0aadf0891bea39eec643e29e2d3e75c99b18689518665f69ba6"],
    ["three/addons/controls/OrbitControls.js", "5a44a9e86a2a0fb11933eed69bc2cd33c76a496854c1aed6ed776efa87d7b064"],
  ]) {
    const prefix = "data:text/javascript;base64,";
    if (!imports[name].startsWith(prefix)) throw Error(`${name} is not embedded in the viewer`);
    check(sha(Buffer.from(imports[name].slice(prefix.length), "base64")) === digest, `${name} matches its pinned upstream bytes`);
  }
  check(html.includes(fs.readFileSync(path.join(root, "assets/three-r160/LICENSE"), "utf8")), "the complete renderer license is distributed with the viewer");
  const context = await browser.newContext({ offline: true, userAgent, reducedMotion: "reduce" });
  const page = await context.newPage(), errors = [], external = [];
  const work = fs.mkdtempSync(path.join(os.tmpdir(), "ftx-offline-viewer-"));
  page.on("pageerror", e => errors.push(String(e)));
  page.on("request", r => { if (/^https?:/.test(r.url())) external.push(r.url()); });
  try {
    await page.goto(pathToFileURL(path.resolve(file)).href);
    await page.waitForFunction(() => document.body.dataset.ready, null, { timeout: 20000 });
    check(await page.getAttribute("body", "data-ready") === "1", "offline viewer initializes from file://");
    if (await page.getAttribute("body", "data-ready") !== "1") throw Error(await page.textContent("#error"));
    check(await page.locator("#stage canvas").count() === 1, "the production renderer creates its canvas");
    const frames = await page.evaluate(() => window.__fold.frames);
    check(frames > 1, "the viewer contains multiple retained frames");
    const slider = page.locator("#frame"), canvas = page.locator("#stage canvas");
    await slider.focus(); await slider.press("Home");
    check(/Flat pattern/.test(await page.textContent("#phase")), "Home selects the board frame");
    const board = await canvas.screenshot({ path: path.join(work, "board.png") });
    await slider.press("End");
    check(/Draped/.test(await page.textContent("#phase")), "End selects the draped frame");
    check(await page.textContent("#count") === `${frames} / ${frames}`, "the frame counter reaches the final retained frame");
    const draped = await canvas.screenshot({ path: path.join(work, "draped.png") });
    check(sha(board) !== sha(draped), "different retained geometry changes the rendered pixels");
    await page.check("input[name=colour][value=strain_rgb]");
    await page.uncheck("#showBody");
    await page.click("#play");
    check(await page.textContent("#play") === "Pause", "playback resumes through the normal control");
    const blob = await page.evaluate(html => URL.createObjectURL(new Blob([html], { type: "text/html" })), html);
    await page.goto(blob);
    await page.waitForFunction(() => document.body.dataset.ready, null, { timeout: 20000 });
    check(await page.getAttribute("body", "data-ready") === "1" && await page.locator("#stage canvas").count() === 1,
      "the same HTML also renders as an offline blob preview");
    check(external.length === 0, `no HTTP(S) requests (${external.length})`);
    check(errors.length === 0, `no page errors${errors.length ? ": " + errors.join(" | ") : ""}`);
    console.log(`viewer sha256 ${sha(fs.readFileSync(file))}; screenshots in ${work}`);
  } finally { await context.close(); }
}

(async () => {
  if (standalone) {
    const browser = await chromium.launch();
    try { await offlineViewer(browser, standalone); } finally { await browser.close(); }
    process.exit(failed ? 1 : 0);
  }
  const port = 20000 + Math.floor(Math.random() * 20000);
  const srv = spawn("python3", ["-m", "http.server", String(port), "-d", path.join(root, "web")], { stdio: ["ignore", "ignore", "inherit"] });
  await new Promise((r) => setTimeout(r, 1200));
  const browser = await chromium.launch();
  if (calibrationSheets) {
    try { await printableReferences(browser, `http://localhost:${port}`, path.resolve(calibrationSheets)); }
    finally { await browser.close(); srv.kill(); }
    return;
  }
  const page = await browser.newPage({ userAgent });
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
  const writeTimingFingerprint = () => {
    if (!timingRuns) return;
    const fingerprint = {
      lane: "browser_timing_only", requested_runs_per_garment: timingRuns, warmups_per_garment: 3,
      garments: ["zouave_bolero", "zouave_trousers"],
      duration_clock: "Node performance.now (monotonic milliseconds)",
      host: { cpu: os.cpus()[0].model, logical_cpus: os.cpus().length, ram_bytes: os.totalmem(),
        platform: os.platform(), arch: os.arch(), kernel: os.release(), node: process.version },
      browser: browser.version(), repository_head: execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim(),
      declared_build_profile: process.env.FTX_WASM_PROFILE || "unspecified; bind the artifact hash to its build receipt",
      wasm_sha256: sha(fs.readFileSync(path.join(root, "web/pkg/ftx_wasm_bg.wasm"))),
      glue_sha256: sha(fs.readFileSync(path.join(root, "web/pkg/ftx_wasm.js"))),
      model_sha256: sha(fs.readFileSync(model)),
      sources: Object.fromEntries(["web/index.html", "web/worker.js", "tools/check_web.cjs"].map(n => [n, sha(fs.readFileSync(path.join(root, n)))])),
      conditions: "Headless Chromium on a shared host; no affinity, clock/governor changes or cache flush. Model retained in worker; each garment drafted once. Fold click to completed result, including tape fit and UI delivery; viewer checks outside timing. Sequential bolero then trousers.",
      started_utc: new Date().toISOString(), system_load: os.loadavg(),
    };
    fs.writeFileSync(path.join(work, "fingerprint.json"), JSON.stringify(fingerprint, null, 2), { flag: "wx" });
  };
  const timedFolds = async (dir, program) => {
    const rows = [];
    const inputs = Object.fromEntries(["pattern.json", "measurements.json", "fit_spec.json"].map(n => [n, sha(fs.readFileSync(path.join(dir, n)))]));
    let reference = null, viewerFile = null, verifiedOffline = false;
    try {
      await page.check("#selfContact");
      for (let i = 0; i < timingRuns + 3; i++) {
        const warmup = i < 3, name = `${warmup ? "warmup" : "sample"}-${warmup ? i + 1 : i - 2}`;
        const row = { name, warmup, started_utc: new Date().toISOString(), system_load_start: os.loadavg() };
        const t0 = performance.now();
        try {
          const err = await settle("#foldGo", 900000);
          row.elapsed_ms = performance.now() - t0;
          if (err) throw Error(err);
          const raw = await bytes("a[download='sim_report.json']"), report = JSON.parse(raw);
          fs.writeFileSync(path.join(dir, `${name}.report.json`), raw, { flag: "wx" });
          row.report_sha256 = sha(raw);
          row.numerical_settled = report.numerical?.settled === true;
          row.all_steps_converged = report.numerical?.sew?.unconverged === 0 && report.numerical?.drape?.unconverged === 0;
          row.gating = report.gating;
          row.self_contact = report.self_contact;
          row.vertices = report.vertices; row.faces = report.faces;
          reference ||= row.report_sha256;
          row.valid = row.numerical_settled && row.all_steps_converged && Array.isArray(row.gating) && row.gating.length === 0
            && row.self_contact?.enabled === true && row.self_contact.thickness_mm === 2
            && row.report_sha256 === reference && row.elapsed_ms < 300000;
          if (!row.valid) throw Error("numerical, contact, digest or declared 300-second browser budget failed");
          if (i === 3) {
            viewerFile = path.join(dir, "browser_fold_viewer.html");
            fs.writeFileSync(viewerFile, await bytes("a[download='fold_viewer.html']"), { flag: "wx" });
          }
        } catch (e) {
          row.elapsed_ms ??= performance.now() - t0;
          row.valid = false; row.error = String(e);
          throw e;
        } finally {
          row.system_load_end = os.loadavg(); rows.push(row);
          fs.appendFileSync(path.join(dir, "timings.jsonl"), JSON.stringify(row) + "\n");
          console.log(`${program} ${name}: ${row.elapsed_ms} ms, ${row.valid ? "valid" : row.error}`);
        }
      }
      const failuresBefore = failed;
      await offlineViewer(browser, viewerFile);
      verifiedOffline = failed === failuresBefore;
    } finally {
      const samples = rows.filter(r => !r.warmup), complete = verifiedOffline && rows.length === timingRuns + 3 && rows.every(r => r.valid);
      const times = samples.map(r => r.elapsed_ms).sort((a, b) => a - b);
      const quantile = q => times[Math.ceil(q * times.length) - 1];
      const result = { program, inputs, warmups_requested: 3, runs_requested: timingRuns,
        attempts: rows.length, retained_runs: samples.length, complete, offline_viewer_passed: verifiedOffline, report_sha256: reference,
        latency_ms: complete ? { min: times[0], p50: quantile(0.5), p95: quantile(0.95), max: times.at(-1) } : null,
        statistics: "Empirical nearest-rank quantiles; no tail confidence or hardware extrapolation. Failed/incomplete runs have no latency summary." };
      fs.writeFileSync(path.join(dir, "timing-summary.json"), JSON.stringify(result, null, 2), { flag: "wx" });
      console.log(JSON.stringify(result));
    }
  };
  try {
    writeTimingFingerprint();
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
        if (timingRuns) { await timedFolds(dir, program); continue; }
        const t0 = Date.now();
        const err = await settle("#foldGo", 900000);
        const s = ((Date.now() - t0) / 1000).toFixed(1);
        check(!err && /no gating flag/.test(await page.textContent("#foldSummary")), `${program} folds up in the page (${s} s), settled, no gating flag`);
        if (err) continue;
        const viewer = await bytes("a[download='fold_viewer.html']");
        const viewerFile = path.join(dir, "browser_fold_viewer.html");
        fs.writeFileSync(viewerFile, viewer, { flag: "wx" });
        await offlineViewer(browser, viewerFile);
        execFileSync(ftx, ["drape", "pattern.json", "--measurements", "measurements.json", "--fit", "fit_spec.json",
          "--self-contact-mm", "2", "--model", model, "--out", "native"], { cwd: dir, stdio: ["ignore", "pipe", "inherit"] });
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
