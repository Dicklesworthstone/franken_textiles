// Drives web/ in headless Chromium: a round per program (chart body), the tape form, a refusal, and with
// --model a fold-up per garment, compared byte for byte with `ftx drape` on the same round's documents.
//
//   tools/build_web.sh && cargo build --release -p ftx-cli
//   NODE_PATH=$(npm root -g) node tools/check_web.cjs [--model PATH.ftxbody] [--ftx target/release/ftx]
//   node tools/check_web.cjs --viewer /path/to/fold_viewer.html   # offline file:// rendering and controls
//   node tools/check_web.cjs --model PATH.ftxbody --timings 20   # browser timing lane; no native comparison
//   node tools/check_web.cjs --calibration-sheets NATIVE_DIR  # six local downloads, without page JavaScript
//   node tools/check_web.cjs --generated-calibration-sheets NATIVE_DIR  # actual WASM generation/replay
//   node tools/check_web.cjs --web-dir STAGED_WEB   # retained build with the same page/worker assets
//
// Needs Playwright with its Chromium (npm i -g playwright && npx playwright install chromium).
const { chromium } = require(process.env.FTX_PLAYWRIGHT_MODULE || "playwright");
const { spawn, spawnSync, execFileSync } = require("child_process");
const crypto = require("crypto");
const fs = require("fs");
const os = require("os");
const path = require("path");
const { pathToFileURL } = require("url");
const { performance } = require("perf_hooks");

const arg = (name) => { const i = process.argv.indexOf(name); return i > 0 ? process.argv[i + 1] : null; };
const root = path.resolve(__dirname, "..");
const webDir = path.resolve(arg("--web-dir") || path.join(root, "web"));
const model = arg("--model");
const acceptedFit = arg("--accepted-fit"), acceptedMeasurements = arg("--accepted-measurements");
const standalone = arg("--viewer");
const calibrationSheets = arg("--calibration-sheets");
const generatedSheets = arg("--generated-calibration-sheets");
const evidenceReplay = arg("--measurement-evidence");
const timingArg = arg("--timings");
const timingRuns = Number(timingArg || 0);
if ((acceptedFit || acceptedMeasurements) && (!model || !acceptedFit || !acceptedMeasurements || standalone || timingArg)) {
  throw Error("accepted-fit checks need --model, --accepted-fit and --accepted-measurements; run separately from timing/viewer checks");
}
if (process.argv.includes("--timings") && (!model || standalone || !/^[1-9][0-9]*$/.test(timingArg || "") || timingRuns > 100)) {
  throw Error("--timings needs --model and an integer run count from 1 to 100; it is separate from --viewer");
}
if (process.argv.includes("--calibration-sheets") && (!calibrationSheets || model || standalone || timingArg)) {
  throw Error("--calibration-sheets needs a native capture directory and runs separately from model/viewer/timing checks");
}
if (process.argv.includes("--generated-calibration-sheets") && (!generatedSheets || calibrationSheets || model || standalone || timingArg)) {
  throw Error("--generated-calibration-sheets needs its native capture directory and runs separately");
}
const userAgent = "OpenAI File Downloader, XaiImageApiFetch/1.0";
const ftx = arg("--ftx") || path.join(root, "target/release/ftx");
const sha = (b) => crypto.createHash("sha256").update(b).digest("hex");
let failed = 0;
const check = (ok, what) => { console.log(`${ok ? "ok  " : "FAIL"} ${what}`); if (!ok) failed++; };

async function measurementEvidence(page, settle, work, nativeDir) {
  const assert = require("node:assert/strict");
  const request = JSON.parse(fs.readFileSync(path.join(nativeDir, "request.json")));
  const source = fs.readFileSync(path.join(nativeDir, "photo-measurements.json"));
  await page.check('input[name=program][value=zouave_bolero]');
  await page.check('input[name=bodyMode][value=tape]');
  await page.fill("#stature", String(request.body.stature_cm));
  await page.fill("#age", String(request.body.age_months));
  await page.locator('#tapeRows input[data-name="chest"]').waitFor();
  for (const input of await page.locator("#tapeRows input").all()) await input.fill("");
  for (const [name, value] of Object.entries(request.body.tape)) {
    await page.fill(`#tapeRows input[data-name="${name}"]`, String(value));
  }
  await page.locator("summary").filter({hasText:"Add photo or other measurement evidence"}).click();
  const upload = async (name, buffer) => page.setInputFiles("#supplementalFiles", {name, mimeType:"application/json", buffer});
  const absent = async () => assert.equal(await page.locator("#results").evaluate(e=>e.classList.contains("hidden")), true);
  await upload("photo-measurements.json", source);
  assert.match(await settle("#go",120000), /same wearer/);await absent();
  await page.check("#supplementalConsent");assert.equal(await settle("#go",120000), "");
  const saveRound=async dir=>{
    fs.mkdirSync(dir);
    // JSON round documents are in the actual ZIP download, not individual export links.
    const waiting=page.waitForEvent("download");await page.click("#resFiles a[download$='.zip']");
    const download=await waiting;assert.equal(await download.failure(),null);
    fs.copyFileSync(await download.path(),path.join(dir,"round.zip"));
    execFileSync("unzip",["-q","round.zip"],{cwd:dir});
  };
  const dir=path.join(work,"measurement-evidence");await saveRound(dir);
  for(const name of ["measurements.json","pattern.json","readiness.json","fit_spec.json"]) {
    assert.deepEqual(JSON.parse(fs.readFileSync(path.join(dir,name))),JSON.parse(fs.readFileSync(path.join(nativeDir,name))),name);
  }
  const measured=JSON.parse(fs.readFileSync(path.join(dir,"measurements.json")));
  assert.equal(measured.measurement_imports[0],source.toString("utf8"));
  assert.equal(measured.fused.arm_length.primary_provenance,"PHOTO_ESTIMATED");assert.equal(measured.taken_on,null);
  assert.ok(measured.records.some(r=>r.provenance==="MEASURED_TAPE"));
  await page.click("#clearSupplemental");assert.equal(await settle("#go",120000),"");
  const baselineDir=path.join(work,"measurement-baseline");await saveRound(baselineDir);
  const baseline=JSON.parse(fs.readFileSync(path.join(baselineDir,"pattern.json")));
  const withPhoto=JSON.parse(fs.readFileSync(path.join(dir,"pattern.json")));
  assert.notDeepEqual(baseline.pieces.find(p=>p.id==="sleeve").vertices,
    withPhoto.pieces.find(p=>p.id==="sleeve").vertices,"photo arm length must change the actual sleeve vertices");
  await upload("photo-measurements.json",source);await page.check("#supplementalConsent");
  assert.equal(await settle("#go",120000),"");
  await page.evaluate(()=>{window.__holdEvidence=true;});await page.click("#go");
  await page.waitForFunction(()=>!!window.__heldEvidence,null,{timeout:120000});
  await upload("same-bytes-new-source.json",source);await absent();
  await page.evaluate(()=>{const h=window.__heldEvidence;delete window.__heldEvidence;
    h.worker.dispatchEvent(new MessageEvent("message",{data:h.data}));});
  await absent();assert.equal(await page.isChecked("#supplementalConsent"),false);
  await page.check("#supplementalConsent");assert.equal(await settle("#go",120000),"");
  await page.fill('#tapeRows input[data-name="chest"]',"55");await absent();
  assert.equal(await page.isChecked("#supplementalConsent"),false);
  assert.match(await settle("#go",120000),/same wearer/);
  await page.fill('#tapeRows input[data-name="chest"]',String(request.body.tape.chest));
  const invalid=JSON.parse(source);invalid.records[0].sigma_mm=-1;
  for(const [name,buffer,pattern] of [["invalid.json",Buffer.from(JSON.stringify(invalid)),/sigma/],
    ["invalid-utf8.json",Buffer.from([0xc3,0x28]),/encoded data|encoding/i]]) {
    await upload(name,buffer);await page.check("#supplementalConsent");
    assert.match(await settle("#go",120000),pattern);await absent();
  }
  await upload("photo-replacement.json",source);await page.check("#supplementalConsent");
  assert.equal(await settle("#go",120000),"");
  // A saved combined file retains source text but must not append those same records again.
  await page.click("#clearSupplemental");await absent();
  await page.check('input[name=bodyMode][value=file]');
  await page.setInputFiles("#measFile",path.join(dir,"measurements.json"));
  assert.equal(await settle("#go",120000),"");
  const replayDir=path.join(work,"measurement-replay");await saveRound(replayDir);
  const replay=JSON.parse(fs.readFileSync(path.join(replayDir,"measurements.json")));
  assert.deepEqual(replay,measured);
  const imageHash=JSON.parse(source).photo.image_input.sha256;
  assert.equal(await page.evaluate(hash=>Object.values(localStorage).some(s=>s.includes(hash)),imageHash),false);
  await page.check('input[name=bodyMode][value=tape]');
  for (const input of await page.locator("#tapeRows input").all()) await input.fill("");
  await page.check('input[name=bodyMode][value=file]');
  console.log("PASS measurement evidence: actual EXIF-photo records plus tape, full native measurements/pattern/readiness/fit equality, exact imported text, real ZIP download, source edit/refusal controls and saved-file replay. Artifacts:",dir);
}

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

async function generatedReferences(browser, origin, expectedDir) {
  const assert = require("node:assert/strict");
  const cases = [
    ["default-letter", "letter", 22, 16, "DICT_4X4_50"],
    ["default-a4", "a4", 22, 16, "DICT_4X4_50"],
    ["aruco4-large", "letter", 23, 15, "DICT_4X4_1000"],
    ["aruco5", "letter", 23, 15, "DICT_5X5_50"],
    ["aruco6", "a4", 18, 11, "DICT_6X6_250"],
    ["aruco7", "letter", 23, 15, "DICT_7X7_1000"],
    ["original", "a4", 18, 11, "DICT_ARUCO_ORIGINAL"],
    ["tag16", "letter", 22, 16, "DICT_APRILTAG_16H5"],
    ["tag25", "a4", 18, 11, "DICT_APRILTAG_25h9"],
    ["tag36-10", "letter", 23, 15, "DICT_APRILTAG_36H10"],
    ["tag36-11", "a4", 18, 11, "DICT_APRILTAG_36h11"],
    ["mip36", "letter", 22, 16, "DICT_ARUCO_MIP_36h12"],
  ].map(([id, paper, square, marker, dictionary]) => ({ id, paper, square, marker, dictionary }));
  const context = await browser.newContext({ userAgent, acceptDownloads: true });
  // Hold an actual computed worker result, then deliver it after cancellation/source edits.
  // Numeric data still comes from the unmodified production WASM, never from a test double.
  await context.addInitScript(() => {
    const Original = window.Worker;
    window.Worker = class extends Original {
      constructor(...args) {
        super(...args);
        this.addEventListener("message", e => {
          if (window.__holdSheet && e.data.type === "result") {
            e.stopImmediatePropagation(); window.__holdSheet = false;
            window.__heldSheet = { worker: this, data: e.data };
          }
        });
      }
    };
  });
  const page = await context.newPage(), errors = [], requests = [];
  context.on("request", r => requests.push({ url: r.url(), method: r.method() }));
  page.on("pageerror", e => errors.push(String(e)));
  const panel = page.locator("#sheetDownloads");
  const absent = async () => assert.equal(await panel.locator("a").count(), 0);
  const configure = async c => {
    await page.selectOption("#sheetPaper", c.paper);
    await page.fill("#sheetSquare", String(c.square)); await page.fill("#sheetMarker", String(c.marker));
    await page.fill("#sheetDictionary", c.dictionary); await absent();
  };
  const complete = async () => {
    await page.click("#sheetGenerate");
    await page.waitForFunction(() => !document.getElementById("sheetGenerate").disabled, null, { timeout: 60000 });
  };
  const work = fs.mkdtempSync(path.join(os.tmpdir(), "ftx-generated-references-"));
  try {
    await page.goto(origin); await page.waitForFunction(() => document.querySelectorAll("#sheetDictionaries option").length === 27);
    await page.locator("#calibrationSheets summary").click();
    for (const c of cases) {
      await configure(c); await complete(); assert.equal(await panel.locator("a").count(), 3, await page.textContent("#sheetStatus"));
      for (const ext of ["pdf", "png", "json"]) {
        const name = `calib-sheet-${c.paper}.${ext}`, event = page.waitForEvent("download");
        await panel.locator(`a[download="${name}"]`).click(); const d = await event;
        assert.equal(await d.failure(), null); const bytes = fs.readFileSync(await d.path());
        assert.ok(bytes.equals(fs.readFileSync(path.join(expectedDir, c.id, name))), `${c.id}/${name}: WASM/native bytes differ`);
        fs.writeFileSync(path.join(work, `${c.id}-${name}`), bytes, { flag: "wx" });
        // Chromium throttles more than ten rapid downloads from one page. Pace clicks;
        // keep each real download and byte comparison rather than bypassing the UI.
        await page.waitForTimeout(150);
      }
      console.log(`PASS generated ${c.id}: matching native PNG/PDF/sidecar bytes`);
    }
    for (const negative of [{ dictionary: "DICT_5X5_51" }, { marker: 22 }, { square: 22.5 }, { square: 24 }, { marker: 0 }]) {
      await configure({ paper: "letter", square: 22, marker: 16, dictionary: "DICT_4X4_50", ...negative });
      await complete(); await absent(); assert.match(await page.textContent("#sheetStatus"), /refused/);
      console.log(`PASS generated refusal ${JSON.stringify(negative)}: no stale/partial downloads`);
    }
    for (const change of ["cancel", "sheetPaper", "sheetSquare", "sheetMarker", "sheetDictionary"]) {
      await configure({ paper: "letter", square: 22, marker: 16, dictionary: "DICT_4X4_50" });
      await page.evaluate(() => { window.__heldSheet = null; window.__holdSheet = true; });
      await page.click("#sheetGenerate"); await page.waitForFunction(() => window.__heldSheet !== null, null, { timeout: 60000 });
      if (change === "cancel") await page.click("#sheetCancel");
      else if (change === "sheetPaper") await page.selectOption("#sheetPaper", "a4");
      else await page.fill("#" + change, change === "sheetDictionary" ? "DICT_5X5_50" : "18");
      await absent();
      await page.evaluate(() => { const h = window.__heldSheet; h.worker.dispatchEvent(new MessageEvent("message", { data: h.data })); window.__heldSheet = null; });
      await absent(); assert.equal(await page.isDisabled("#sheetCancel"), true);
      console.log(`PASS generated ${change}: late actual-WASM result cannot publish`);
    }
    assert.deepEqual(errors, []); assert.ok(requests.every(r => r.method === "GET" && r.url.startsWith(origin + "/")));
    console.log(`PASS:12 positive/5 refusal/5 late-result controls, no page errors or uploads/external requests. Digital software checks only; physical print accuracy unproven. Downloads: ${work}`);
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
  const srv = spawn("python3", ["-m", "http.server", String(port), "-d", webDir], { stdio: ["ignore", "ignore", "inherit"] });
  await new Promise((r) => setTimeout(r, 1200));
  const browser = await chromium.launch();
  if (calibrationSheets || generatedSheets) {
    try {
      if (generatedSheets) await generatedReferences(browser, `http://localhost:${port}`, path.resolve(generatedSheets));
      else await printableReferences(browser, `http://localhost:${port}`, path.resolve(calibrationSheets));
    }
    finally { await browser.close(); srv.kill(); }
    return;
  }
  const page = await browser.newPage({ userAgent });
  if (evidenceReplay) await page.route("**/worker.js",async route=>{
    const response=await route.fetch();
    // Freeze the caller's date, not numerical results, to the native page fixture's clock.
    await route.fulfill({response,body:"const NativeDate=Date; globalThis.Date=class extends NativeDate { constructor(...a) { super(...(a.length?a:[2026,9,13,12,0])); } };\n"+await response.text()});
  });
  if (evidenceReplay) await page.addInitScript(()=>{
    const Original=window.Worker;
    window.Worker=class extends Original {
      constructor(...args) {super(...args);this.addEventListener("message",e=>{
        if(window.__holdEvidence && e.data.type==="result") {
          e.stopImmediatePropagation();window.__holdEvidence=false;window.__heldEvidence={worker:this,data:e.data};
        }
      });}
    };
  });
  const errors = [], requests = [];
  page.context().on("request", r => { if (/^https?:/.test(r.url())) requests.push({ url: r.url(), method: r.method() }); });
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
    // Actual file uploads through the page/worker/WASM; saved fold inputs must follow records.
    const oracle = JSON.parse(fs.readFileSync(path.join(root, "tests/fixtures/oracle/web/webapp.json")));
    const source = oracle.cases.find(c => c.id === "file-pattern-spec").input.body.measurements;
    const stale = structuredClone(source);
    stale.fused.chest.value_mm = 1111; stale.fused.waist.value_mm = 1234;
    const measured = structuredClone(source); measured.wearer.reference = false;
    for (const r of measured.records) {
      r.provenance = "MEASURED_TAPE"; r.method = "test: synthetic independent tape control";
      r[Object.hasOwn(r, "sigma_deg") ? "sigma_deg" : "sigma_mm"] = 1;
    }
    await page.check("input[name=program][value=zouave_bolero]");
    await page.check("input[name=bodyMode][value=file]");
    let original, ready;
    for (const name of ["fresh", "stale", "measured", "modeled"]) {
      const input = name === "fresh" ? source : name === "stale" ? stale : name === "measured" ? measured : structuredClone(ready);
      if (name === "modeled") for (const r of input.records) {
        r.provenance = "MODEL_FIT"; r.method = "test: model estimate, not an independent measurement";
      }
      const upload = path.join(work, `upload-${name}.json`), raw = Buffer.from(JSON.stringify(input));
      fs.writeFileSync(upload, raw, { flag: "wx" });
      await page.setInputFiles("#measFile", upload);
      check(await page.locator("#results").evaluate(e => e.classList.contains("hidden")), `${name} upload withdraws the previous round`);
      check(!(await settle("#go", 120000)), `${name} file round executes`);
      const dir = path.join(work, `upload-${name}`); fs.mkdirSync(dir);
      fs.writeFileSync(path.join(dir, "round.zip"), await bytes("a[download$='.zip']"), { flag: "wx" });
      execFileSync("unzip", ["-q", "round.zip"], { cwd: dir });
      const measurements = fs.readFileSync(path.join(dir, "measurements.json"));
      const pattern = fs.readFileSync(path.join(dir, "pattern.json"));
      const readiness = JSON.parse(fs.readFileSync(path.join(dir, "readiness.json")));
      if (name === "fresh") original = { measurements, pattern };
      if (name === "stale") check(measurements.equals(original.measurements) && pattern.equals(original.pattern), "poisoned cache leaves actual saved measurements/pattern bytes unchanged");
      if (name === "measured") {
        ready = JSON.parse(measurements);
        check(readiness.status === "READY" && (await page.textContent("#resStatus")) === "CONVERGED", "synthetic independent tape control has a READY cache and converged flat round");
      }
      if (name === "modeled") {
        check(JSON.parse(measurements).fused.chest.primary_provenance === "MODEL_FIT", "model records remain MODEL_FIT in actual saved fold input");
        check(readiness.status === "NOT_READY" && (await page.textContent("#resStatus")) === "MEASUREMENTS_NOT_READY", "formerly READY cache cannot promote model records through the browser measurement gate");
      }
      check(fs.readFileSync(upload).equals(raw), `${name} source upload remains unchanged`);
    }
    const invalid = structuredClone(source); invalid.records[0].sigma_mm = -1;
    await page.setInputFiles("#measFile", { name: "invalid.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(invalid)) });
    check(/sigma/.test(await settle("#go", 120000)), "valid cache cannot hide an invalid uploaded record");
    check(await page.locator("#results").evaluate(e => e.classList.contains("hidden")), "refused upload exposes no prior round downloads");
    if (evidenceReplay) await measurementEvidence(page, settle, work, path.resolve(evidenceReplay));
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
        const report = JSON.parse(await bytes("a[download='sim_report.json']"));
        check(report.body?.provenance === "MODEL_FIT" && Object.keys(report.body.readings_mm).length === 21,
          `${program} retains all body predictions as MODEL_FIT`);
        check(report.body.model_sha256 === sha(fs.readFileSync(model))
          && report.measurements_sha256 === sha(fs.readFileSync(path.join(dir, "measurements.json"))),
          `${program} diagnostics identify the exact model and saved measurements`);
        const tape = Object.entries(report.body.fit.tape);
        check(tape.length > 0 && tape.every(([name, r]) => r.residual_mm === r.body_mm - r.target_mm
          && r.body_mm === report.body.readings_mm[name]),
          `${program} retains every signed unrounded body-target residual`);
        check((await page.locator("#foldSummary table tr").count()) === tape.length + 1
          && /MODEL_FIT/.test(await page.textContent("#foldSummary")), `${program} shows the actual body residual rows`);
        check((await page.textContent("#resStatus")) === "CONVERGED", `${program} fold diagnostics preserve the separate round verdict`);
        await offlineViewer(browser, viewerFile);
        execFileSync(ftx, ["drape", "pattern.json", "--measurements", "measurements.json", "--fit", "fit_spec.json",
          "--self-contact-mm", "2", "--model", model, "--out", "native"], { cwd: dir, stdio: ["ignore", "pipe", "inherit"] });
        const native = fs.readFileSync(path.join(dir, "native/fold_viewer.html"));
        const nativeBody = JSON.parse(fs.readFileSync(path.join(dir, "native/sim_report.json"))).body;
        check(tape.every(([name, r]) => r.target_mm === nativeBody.fit.tape[name].target_mm
          && Math.round(r.body_mm * 10) / 10 === nativeBody.fit.tape[name].body_mm),
          `${program} fitted targets/readings agree with native at its documented 0.1mm report rounding`);
        check(sha(viewer) === sha(native), `${program} fold-up equals ftx drape's, byte for byte (${sha(native).slice(0, 12)})`);
        await page.locator("#selfContact").uncheck();
        check((await page.textContent("#foldSummary")) === "" && (await page.locator("#foldSummary a").count()) === 0,
          `${program} edits withdraw residuals and downloads`);
        await page.locator("#selfContact").check();
      }
    }
    if (acceptedFit) {
      const assert = require("node:assert/strict"), raw = fs.readFileSync(acceptedFit);
      const input = fs.readFileSync(acceptedMeasurements), saved = JSON.parse(raw);
      await page.check("input[name=program][value=zouave_bolero]");
      await page.check("input[name=bodyMode][value=file]");
      await page.setInputFiles("#measFile", acceptedMeasurements);
      assert.equal(await settle("#go", 120000), "");
      const status = await page.textContent("#resStatus"), dir = path.join(work, "accepted-fit");
      fs.mkdirSync(dir);
      fs.writeFileSync(path.join(dir, "round.zip"), await bytes("a[download$='.zip']"), {flag:"wx"});
      execFileSync("unzip", ["-q", "round.zip"], {cwd:dir});
      await page.locator("details").filter({has:page.locator("#bodyFitFile")}).locator("summary").click();
      await page.setInputFiles("#bodyFitFile", acceptedFit);
      await page.waitForFunction(() => /Fit report read/.test(document.getElementById("status").textContent));
      assert.equal(await page.locator("#foldGo").isDisabled(),true);
      await page.check("#bodyFitConsent");await page.fill("#bodyArmDeg","50");
      assert.equal(await settle("#foldGo",900000),"");
      const report = JSON.parse(await bytes("a[download='sim_report.json']"));
      assert.deepEqual(report.body.source_fit,saved);
      assert.deepEqual(report.body.phenotype,saved.solution.anny_parameters.phenotype);
      assert.equal(report.body.shape_source.refitted,false);
      assert.equal(report.body.shape_source.image_poses_reused,false);
      assert.equal(report.body.shape_source.report_sha256,sha(raw));
      assert.ok((await bytes("a[download='body_fit.json']")).equals(raw));
      assert.equal(await page.textContent("#resStatus"),status);
      assert.match(await page.textContent("#foldSummary"),/without refitting/);
      const viewer = await bytes("a[download='fold_viewer.html']");
      fs.writeFileSync(path.join(dir,"body_fit.json"),raw,{flag:"wx"});
      const nativeRun = spawnSync(ftx,["drape","pattern.json","--measurements","measurements.json","--fit","fit_spec.json",
        "--body-fit","body_fit.json","--arm-deg","50","--self-contact-mm","2","--model",model,"--out","native"],
        {cwd:dir,stdio:["ignore","pipe","inherit"]});
      assert.ok([0,2].includes(nativeRun.status),String(nativeRun.stdout)); // retain any ordinary drape gating flags
      const native = JSON.parse(fs.readFileSync(path.join(dir,"native/sim_report.json")));
      delete native.pattern;delete native.wall_s;
      assert.deepEqual(report,native);
      assert.ok(viewer.equals(fs.readFileSync(path.join(dir,"native/fold_viewer.html"))));
      check(true,"accepted fit: complete native/browser report and viewer equality, exact original report bytes, no refit or readiness promotion");
      const rejected = {...saved,status:"REJECTED"};
      await page.setInputFiles("#bodyFitFile",{name:"rejected.json",mimeType:"application/json",buffer:Buffer.from(JSON.stringify(rejected))});
      await page.waitForFunction(() => /Fit report read/.test(document.getElementById("status").textContent));
      assert.equal(await page.locator("#foldSummary a").count(),0);
      assert.equal(await page.locator("#foldGo").isDisabled(),true);await page.check("#bodyFitConsent");
      assert.match(await settle("#foldGo",120000),/accepted Anny shape/);
      assert.equal(await page.locator("#foldSummary a").count(),0);
      assert.ok(fs.readFileSync(acceptedFit).equals(raw));assert.ok(fs.readFileSync(acceptedMeasurements).equals(input));
      check(true,"rejected replacement refuses without stale downloads or tape fallback; source inputs unchanged");
    }
    check(errors.length === 0, `no console errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
    check(requests.every(r => r.method === "GET" && r.url.startsWith(`http://localhost:${port}/`)), "actual browser rounds make only local GET requests, with no upload or external request");
  } finally {
    await browser.close();
    srv.kill();
    console.log(`work files in ${work}`);
  }
  process.exit(failed ? 1 : 0);
})().catch((e) => { console.error("FAILED", e); process.exit(1); });
