// Local browser controller for web/knit.html (FTX-K18).
const statusEl = document.getElementById("knitStatus");
const errorEl = document.getElementById("error");
const runSwatchBtn = document.getElementById("runSwatch");
const runPanelBtn = document.getElementById("runPanel");
const runPulloverBtn = document.getElementById("runPullover");
const runExportBtn = document.getElementById("runExport");
const cancelExportBtn = document.getElementById("cancelExport");
const runDyeBtn = document.getElementById("runDye");

let worker = null;
let engineReady = false;
let reqId = 0;
const latestRequestByKind = { swatch: 0, panel: 0, pullover: 0, export: 0, dye: 0 };
const targetByRequest = new Map();
const downloadUrlsByContainer = new WeakMap();
const MAX_EXPORT_INPUT_BYTES = 4 * 1024 * 1024;
const MAX_DRAFT_INPUT_BYTES = 24 * 1024 * 1024;

const PANEL_MACHINE_DEFAULTS = {
  "kniterate-7e": { needle_pitch_mm: 25.4 / 7.0, carrier_id: "1", cam_setting: 7.0, yarn_tex: 200.0 },
  "brother-kh930-ayab": { needle_pitch_mm: 4.5, carrier_id: "A", cam_setting: 7.0, yarn_tex: 150.0 },
  "brother-kh270-ayab": { needle_pitch_mm: 9.0, carrier_id: "A", cam_setting: 6.0, yarn_tex: 400.0 },
  "generic-vbed-12e": { needle_pitch_mm: 25.4 / 12.0, carrier_id: "1", cam_setting: 45.0, yarn_tex: 80.0 },
  "generic-slider-12e": { needle_pitch_mm: 25.4 / 12.0, carrier_id: "1", cam_setting: 45.0, yarn_tex: 80.0 },
  "hand-knit": { needle_pitch_mm: 4.0, carrier_id: "1", cam_setting: 7.0, yarn_tex: 180.0 },
  "shima-seiki-12e-handoff": { needle_pitch_mm: 25.4 / 12.0, carrier_id: "1", cam_setting: 45.0, yarn_tex: 80.0 },
  "stoll-12e-handoff": { needle_pitch_mm: 25.4 / 12.0, carrier_id: "1", cam_setting: 11.0, yarn_tex: 80.0 },
};

function showError(msg) {
  errorEl.textContent = msg;
  errorEl.hidden = false;
}

function clearError() {
  errorEl.textContent = "";
  errorEl.hidden = true;
}

function clearDownloads(container) {
  const urls = downloadUrlsByContainer.get(container);
  if (urls) {
    for (const url of urls) URL.revokeObjectURL(url);
  }
  downloadUrlsByContainer.set(container, []);
  container.textContent = "";
}

function makeDownload(container, filename, data, mime = "text/plain") {
  const url = URL.createObjectURL(new Blob([data], { type: mime }));
  const urls = downloadUrlsByContainer.get(container) || [];
  urls.push(url);
  downloadUrlsByContainer.set(container, urls);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.textContent = `Download ${filename}`;
  container.append(a);
}

function decodeBase64Bytes(b64) {
  const binary = atob(String(b64));
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

function clearPulloverResult() {
  clearDownloads(document.getElementById("pulloverDownloads"));
  document.getElementById("pulloverOut").hidden = true;
  document.getElementById("pulloverSummary").textContent = "";
  document.getElementById("pulloverPre").textContent = "";
}

function invalidatePullover() {
  latestRequestByKind.pullover = ++reqId;
  clearPulloverResult();
}

function clearExportResult() {
  cancelExportBtn.disabled = true;
  clearDownloads(document.getElementById("exportDownloads"));
  document.getElementById("exportOut").hidden = true;
  document.getElementById("exportSummary").textContent = "";
  document.getElementById("exportPre").textContent = "";
}

function invalidateExport() {
  latestRequestByKind.export = ++reqId;
  clearExportResult();
}

function selectExportMode() {
  const kind = document.getElementById("exportKind").value || "saved";
  document.getElementById("exportSavedInputs").hidden = kind !== "saved";
  document.getElementById("exportShapeInputs").hidden = kind !== "shape";
  document.getElementById("exportMeasuredInputs").hidden = kind !== "measured";
  document.getElementById("exportMachine").disabled = kind === "measured";
}

function buildPresetPixels(preset, w, h) {
  const pixels = [];
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      if (preset === "horizon") {
        pixels.push(y < Math.floor(h / 2) ? [34, 118, 188] : [224, 166, 48]);
      } else if (preset === "checker") {
        const hi = ((x < w / 2) ^ (y < h / 2)) !== 0;
        pixels.push(hi ? [188, 48, 62] : [238, 224, 186]);
      } else {
        const t = h > 1 ? y / (h - 1) : 0;
        pixels.push([
          Math.round(45 + 195 * t),
          Math.round(62 + 120 * (1 - Math.abs(2 * t - 1))),
          Math.round(168 * (1 - t) + 38 * t),
        ]);
      }
    }
  }
  return pixels;
}

function drawGridCanvas(canvas, cols, rows, rgbList) {
  const ctx = canvas.getContext("2d");
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  const cw = canvas.width / cols;
  const ch = canvas.height / rows;
  for (let r = 0; r < rows; r++) {
    for (let c = 0; c < cols; c++) {
      const rgb = rgbList[r * cols + c] || [220, 220, 220];
      ctx.fillStyle = `rgb(${rgb[0]},${rgb[1]},${rgb[2]})`;
      ctx.fillRect(Math.floor(c * cw), Math.floor(r * ch), Math.ceil(cw), Math.ceil(ch));
    }
  }
}

const BLOCKED_SVG_TAGS = new Set([
  "parsererror",
  "script",
  "foreignobject",
  "iframe",
  "object",
  "embed",
  "animate",
  "set",
]);

function renderSvg(container, svgText) {
  if (typeof DOMParser === "function") {
    const parsed = new DOMParser().parseFromString(String(svgText || ""), "image/svg+xml");
    const root = parsed.documentElement;
    if (!root || String(root.localName || root.nodeName || "").toLowerCase() !== "svg") {
      throw new Error("Refused malformed or unsafe SVG chart payload.");
    }
    const nodes = [root, ...Array.from(parsed.querySelectorAll("*"))];
    for (const el of nodes) {
      const tag = String(el.localName || el.nodeName || "").toLowerCase();
      if (BLOCKED_SVG_TAGS.has(tag)) {
        throw new Error("Refused malformed or unsafe SVG chart payload.");
      }
      for (const attr of Array.from(el.attributes || [])) {
        const name = String(attr.name || "").toLowerCase();
        const val = String(attr.value || "").trim().toLowerCase();
        if (
          name.startsWith("on") ||
          ((name === "href" || name === "xlink:href") &&
            (val.startsWith("javascript:") || val.startsWith("data:")))
        ) {
          throw new Error("Refused malformed or unsafe SVG chart payload.");
        }
      }
    }
    container.replaceChildren(document.importNode(root, true));
    return;
  }
  container.textContent = String(svgText || "");
}

function parseFiniteInput(id, label) {
  const raw = document.getElementById(id).value.trim();
  if (!raw) throw new Error(`${label} is required.`);
  const value = Number(raw);
  if (!Number.isFinite(value)) throw new Error(`${label} must be a finite number.`);
  return value;
}

function parseIntRange(id, label, min, max) {
  const value = parseFiniteInput(id, label);
  if (!Number.isInteger(value) || value < min || value > max) {
    throw new Error(`${label} must be an integer between ${min} and ${max}.`);
  }
  return value;
}

function renderSwatch(res) {
  const out = document.getElementById("swatchOut");
  const dl = document.getElementById("swatchDownloads");
  const svgBox = document.getElementById("swatchSvgBox");
  const pre = document.getElementById("swatchPre");
  // Retire prior downloads even when an outdated engine's response is refused.
  clearDownloads(dl);
  out.hidden = true;
  if (res.machine_executable !== false || res.cert?.hardware_execution_authorized !== false) {
    throw new Error("This engine returned an unqualified machine export. Rebuild and reload the current WASM bundle.");
  }
  if (res.kc_text && (typeof res.kc_text !== "string" || !res.kc_text.startsWith("HOME\nRACK:0\n") ||
      res.kcode_verification?.schema !== "ftx-kniterate-readback/1" ||
      res.kcode_verification.hardware_execution_authorized !== false ||
      res.kcode_verification.program_sha256 !== res.knitout_sha256 ||
      res.kcode_verification.kc_sha256 !== res.kc_sha256 ||
      res.kcode_verification.takedown?.kc_sha256 !== res.kc_sha256 ||
      res.kcode_verification.takedown.physical_takedown_calibrated !== false ||
      res.kcode_verification.takedown.machine_sha256 !== res.cert.machine_sha256 ||
      !/^[a-f0-9]{64}$/.test(res.kc_sha256 || ""))) {
    throw new Error("The KCode file is missing its source-bound read-back verification. Rebuild and reload the current WASM bundle.");
  }
  if (typeof res.pattern_json !== "string") {
    throw new Error("This engine omitted the saved pattern. Rebuild and reload the current WASM bundle.");
  }
  renderSvg(svgBox, res.chart_svg);
  makeDownload(dl, "swatch.knit.json", res.pattern_json, "application/json");
  makeDownload(dl, "swatch.k", res.knitout_text, "text/plain");
  if (res.kc_text) {
    makeDownload(dl, "swatch.kc", res.kc_text, "text/plain");
    makeDownload(dl, "kcode_verification.json", JSON.stringify(res.kcode_verification, null, 2), "application/json");
  }
  if (res.backend_export_refusal) {
    makeDownload(dl, "backend_export_refusal.json", JSON.stringify(res.backend_export_refusal, null, 2), "application/json");
  }
  if (res.ayab_png_base64) {
    makeDownload(dl, "swatch_ayab.png", decodeBase64Bytes(res.ayab_png_base64), "image/png");
  }
  if (res.ayab_checklist) {
    makeDownload(dl, "ayab_session_checklist.json", JSON.stringify(res.ayab_checklist, null, 2), "application/json");
  }
  if (res.vendor_handoff_manifest) {
    makeDownload(dl, "vendor_handoff_manifest.json", JSON.stringify(res.vendor_handoff_manifest, null, 2), "application/json");
  }
  makeDownload(dl, "swatch_chart.svg", res.chart_svg, "image/svg+xml");
  makeDownload(dl, "swatch_cert.json", JSON.stringify(res.cert, null, 2), "application/json");
  makeDownload(dl, "swatch_qualification_template.json", JSON.stringify(res.qualification_template, null, 2), "application/json");

  const conversionNotice = res.backend_export_refusal
    ? `Machine conversion required [${res.backend_export_refusal.code}]: ${res.backend_export_refusal.message}\n${(res.backend_export_refusal.repairs || []).join("\n")}\n\n`
    : res.kc_text ? "KCode pass and needle read-back verified; physical machine qualification remains required.\n\n" : "";
  pre.textContent =
    conversionNotice +
    `=== Certificate (hardware_execution_authorized=${res.cert.hardware_execution_authorized}) ===\n` +
    JSON.stringify(res.cert, null, 2) +
    "\n\n=== Knitout v2 (first 40 lines) ===\n" +
    res.knitout_text.split("\n").slice(0, 40).join("\n");
  out.hidden = false;
}

function renderPanel(res) {
  const out = document.getElementById("panelOut");
  const dl = document.getElementById("panelDownloads");
  clearDownloads(dl);
  out.hidden = true;
  if (typeof res.pattern_json !== "string") {
    throw new Error("This engine omitted the saved pattern. Rebuild and reload the current WASM bundle.");
  }
  renderSvg(document.getElementById("panelSvgBox"), res.chart_svg);
  if (dl) {
    makeDownload(dl, "panel.knit.json", res.pattern_json, "application/json");
    makeDownload(dl, "panel_chart.svg", res.chart_svg, "image/svg+xml");
    makeDownload(dl, "panel_instructions.txt", res.instructions_text, "text/plain");
    makeDownload(dl, "panel_metadata.json", JSON.stringify(res.metadata, null, 2), "application/json");
  }
  const construction = res.metadata?.neckline_construction === "held_center_tuck_anchored_shoulders"
    ? "a panel with tuck-anchored shoulder short rows and held center neckline loops; manual finishing required"
    : res.metadata?.neckline_construction === "center_loops_held_separate_shoulder_carriers"
      ? "a panel with held center neckline loops and separate shoulder carriers; manual finishing required"
      : "a straight-top panel";
  document.getElementById("panelSummary").textContent =
    `Drafted ${res.stitch_count} stitches for ${construction} · doc SHA-256 ${res.doc_sha256.slice(0, 12)}… · SVG checks=${res.svg_checks_passed} · fit_claimed=${res.fit_claimed}`;
  document.getElementById("panelPre").textContent =
    JSON.stringify(res.metadata, null, 2) + "\n\n" + res.instructions_text;
  out.hidden = false;
}

function renderPullover(res) {
  clearPulloverResult();
  for (const flag of ["machine_executable", "hardware_execution_authorized", "physical_knitting_validated", "fit_claimed"]) {
    if (res[flag] !== false) {
      throw new Error("This engine returned an unqualified pullover claim. Rebuild and reload the current WASM bundle.");
    }
  }
  if (typeof res.archive_base64 !== "string" || !res.archive_base64 || !Array.isArray(res.files)) {
    throw new Error("The engine returned an incomplete four-piece pullover archive.");
  }
  const archive = decodeBase64Bytes(res.archive_base64);
  if (archive.length < 22) throw new Error("The engine returned an incomplete pullover ZIP archive.");
  const pieceCount = res.pieces && typeof res.pieces === "object" ? Object.keys(res.pieces).length : 0;
  document.getElementById("pulloverSummary").textContent =
    `${res.machine} · ${pieceCount} pieces · ${res.seam_count ?? 0} checked seams · ${res.files.length} files · ${res.assembly || "four separate pieces joined manually"} · hardware execution authorized: false · physical knitting validated: false · fit claimed: false`;
  const refusals = res.backend_export_refusals && Object.keys(res.backend_export_refusals).length > 0
    ? `=== Backend Conversion Notices ===\n${JSON.stringify(res.backend_export_refusals, null, 2)}\n\n`
    : "";
  document.getElementById("pulloverPre").textContent =
    refusals +
    `ZIP SHA-256: ${res.archive_sha256}\n\n=== Openings & Residuals (mm) ===\n` +
    JSON.stringify({ openings: res.openings, residuals_mm: res.residuals_mm }, null, 2) +
    "\n\n=== Files (uncompressed bytes and SHA-256) ===\n" +
    res.files.map((file) => `${file.name}\n  ${file.bytes} bytes · ${file.sha256}`).join("\n") +
    "\n\n=== Piece Certificates ===\n" +
    JSON.stringify(res.pieces, null, 2);
  makeDownload(document.getElementById("pulloverDownloads"), "knit-pullover.zip", archive, "application/zip");
  document.getElementById("pulloverOut").hidden = false;
}

function renderExport(res) {
  clearExportResult();
  for (const flag of ["machine_executable", "hardware_execution_authorized", "physical_knitting_validated", "fit_claimed"]) {
    if (res[flag] !== false) {
      throw new Error("This engine returned an unqualified export claim. Rebuild and reload the current WASM bundle.");
    }
  }
  if (typeof res.archive_base64 !== "string" || !res.archive_base64 || !Array.isArray(res.files)) {
    throw new Error("The engine returned an incomplete saved-pattern export.");
  }
  const kcodeFile = res.files.find((file) => file.name === "program.kc");
  if (kcodeFile && (res.kcode_verification?.schema !== "ftx-kniterate-readback/1" ||
      res.kcode_verification.hardware_execution_authorized !== false ||
      res.kcode_verification.program_sha256 !== res.cert?.program_sha256 ||
      res.kcode_verification.kc_sha256 !== kcodeFile.sha256 ||
      res.kcode_verification.takedown?.kc_sha256 !== kcodeFile.sha256 ||
      res.kcode_verification.takedown.physical_takedown_calibrated !== false ||
      res.kcode_verification.takedown.machine_sha256 !== res.cert?.machine_sha256 ||
      !res.files.some((file) => file.name === "kcode_verification.json"))) {
    throw new Error("The KCode archive lacks a matching source-bound read-back verification.");
  }
  const archive = decodeBase64Bytes(res.archive_base64);
  if (archive.length < 22) throw new Error("The engine returned an incomplete ZIP archive.");
  const conversionNotice = res.backend_export_refusal
    ? `Machine conversion required [${res.backend_export_refusal.code}]: ${res.backend_export_refusal.message}\n${(res.backend_export_refusal.repairs || []).join("\n")}\n\n`
    : "";
  document.getElementById("exportSummary").textContent =
    `${res.machine} · ${res.files.length} files · pattern SHA-256 ${res.pattern_sha256} · KCode read-back verified: ${Boolean(kcodeFile)} · hardware execution authorized: false · physical knitting validated: false · fit claimed: false`;
  document.getElementById("exportPre").textContent = conversionNotice +
    (res.shape ? "=== Shape geometry and gauge residuals ===\n" + JSON.stringify(res.shape, null, 2) + "\n\n" : "") +
    (res.drafting ? "=== Body dimensions and garment ease ===\n" + JSON.stringify(res.drafting, null, 2) + "\n\n" : "") +
    `ZIP SHA-256: ${res.archive_sha256}\n\n=== Files (uncompressed bytes and SHA-256) ===\n` +
    res.files.map((file) => `${file.name}\n  ${file.bytes} bytes · ${file.sha256}`).join("\n") +
    "\n\n=== Virtual machine report ===\n" + JSON.stringify(res.vm_report, null, 2) +
    "\n\n=== Certificate ===\n" + JSON.stringify(res.cert, null, 2);
  makeDownload(document.getElementById("exportDownloads"), "knit-export.zip", archive, "application/zip");
  document.getElementById("exportOut").hidden = false;
}

function renderDye(res, targetPixels, targetW, targetH) {
  const out = document.getElementById("dyeOut");
  const [courses, wales] = res.grid;
  const receipt = res.uncertainty_receipt;
  const receiptSuffix = receipt
    ? ` · color_fidelity_claimed=${Boolean(receipt.four_claims?.color_fidelity_claimed)}` +
      (Array.isArray(receipt.gating_flags) && receipt.gating_flags.length
        ? ` · flags=${receipt.gating_flags.join(", ")}`
        : "")
    : "";
  document.getElementById("dyeMetrics").textContent =
    `CIEDE2000 p50=${res.delta_e00_p50.toFixed(2)}, p95=${res.delta_e00_p95.toFixed(2)} · gamut floor=${res.gamut_floor_delta_e00.toFixed(2)} · ` +
    `coherent horizon: circular=${res.coherent_horizon_circular_stitches} st / flatbed=${res.coherent_horizon_flatbed_stitches} st · ` +
    `iterations=${res.iterations}${receiptSuffix}`;

  drawGridCanvas(document.getElementById("dyeTargetCanvas"), targetW, targetH, targetPixels);
  // Flip courses so course 0 is at the bottom of the knitted fabric canvas
  const flipped = [];
  for (let c = courses - 1; c >= 0; c--) {
    for (let w = 0; w < wales; w++) {
      flipped.push(res.predicted_stitch_srgb[c * wales + w]);
    }
  }
  drawGridCanvas(document.getElementById("dyePredCanvas"), wales, courses, flipped);

  const tbody = document.querySelector("#dyeParetoTable tbody");
  tbody.textContent = "";
  for (const row of res.pareto_regimes || []) {
    const tr = document.createElement("tr");
    const feasibleText = row.feasible
      ? "Yes"
      : row.refusal_reason
        ? `No (${row.refusal_reason})`
        : "No";
    const p95Text = Number.isFinite(row.expected_delta_e00_p95)
      ? row.expected_delta_e00_p95.toFixed(2)
      : "—";
    const cells = [
      String(row.regime ?? ""),
      feasibleText,
      p95Text,
      String(row.complexity_rank ?? "—"),
      String(row.rationale ?? ""),
    ];
    for (const text of cells) {
      const td = document.createElement("td");
      td.textContent = text;
      tr.append(td);
    }
    tbody.append(tr);
  }

  const dl = document.getElementById("dyeDownloads");
  clearDownloads(dl);
  makeDownload(dl, "dye_program.json", JSON.stringify(res.program, null, 2), "application/json");
  if (res.uncertainty_receipt) {
    makeDownload(dl, "dye_uncertainty_receipt.json", JSON.stringify(res.uncertainty_receipt, null, 2), "application/json");
  }
  if (res.schedule_csv) makeDownload(dl, "dye_schedule.csv", res.schedule_csv, "text/csv");
  if (res.controller_gcode) makeDownload(dl, "dye_controller.gcode", res.controller_gcode, "text/plain");
  if (res.cert) makeDownload(dl, "dye_cert.json", JSON.stringify(res.cert, null, 2), "application/json");

  document.getElementById("dyePre").textContent = res.controller_gcode
    ? `=== Schedule CSV ===\n${res.schedule_csv}\n\n=== G-Code ===\n${res.controller_gcode}`
    : res.controller_export_refusal
      ? `Controller export refused [${res.controller_export_refusal.code || "refused"}]: ${res.controller_export_refusal.message || "hazard check failed"}`
      : "Wearable age classes require an Exp-4 fastness qualification receipt before emitting controller G-code; JSON dye program is ready for review.";
  out.hidden = false;
}

function failEngine(message) {
  engineReady = false;
  invalidatePullover();
  invalidateExport();
  statusEl.textContent = "Engine unavailable.";
  runSwatchBtn.disabled = true;
  runPanelBtn.disabled = true;
  runPulloverBtn.disabled = true;
  runExportBtn.disabled = true;
  runDyeBtn.disabled = true;
  showError(message);
}

function isStaleReply(msg) {
  return Boolean(
    msg.request &&
    msg.kind &&
    latestRequestByKind[msg.kind] &&
    msg.request !== latestRequestByKind[msg.kind]
  );
}

function startWorker() {
  const activeWorker = new Worker("knit-worker.js", { type: "module" });
  worker = activeWorker;
  activeWorker.onerror = (event) => {
    if (worker !== activeWorker) return;
    failEngine(String(event?.message || "Knitting & dyeing worker failed to load."));
  };
  activeWorker.onmessageerror = () => {
    if (worker !== activeWorker) return;
    failEngine("Knitting & dyeing worker returned an unreadable message.");
  };
  activeWorker.onmessage = ({ data: msg }) => {
    if (worker !== activeWorker) return;
    if (msg.type === "ready") {
      engineReady = true;
      statusEl.textContent = "Local WASM knitting & dyeing engine ready.";
      runSwatchBtn.disabled = false;
      runPanelBtn.disabled = false;
      runPulloverBtn.disabled = false;
      runExportBtn.disabled = false;
      runDyeBtn.disabled = false;
    } else if (msg.type === "loadError") {
      failEngine(msg.message);
    } else if (msg.type === "refusal") {
      targetByRequest.delete(msg.request);
      if (isStaleReply(msg)) return;
      if (msg.kind === "export") clearExportResult();
      if (msg.kind === "pullover") clearPulloverResult();
      statusEl.textContent = `Refused (${msg.code}).`;
      showError(`[${msg.code}] ${msg.message}`);
    } else if (msg.type === "error") {
      targetByRequest.delete(msg.request);
      if (isStaleReply(msg)) return;
      if (msg.kind === "export") clearExportResult();
      if (msg.kind === "pullover") clearPulloverResult();
      statusEl.textContent = "Error.";
      showError(msg.message);
    } else if (msg.type === "result") {
      const target = targetByRequest.get(msg.request);
      targetByRequest.delete(msg.request);
      if (isStaleReply(msg)) return;
      clearError();
      statusEl.textContent = `Completed ${msg.kind}.`;
      try {
        if (msg.kind === "swatch") renderSwatch(msg.result);
        else if (msg.kind === "panel") renderPanel(msg.result);
        else if (msg.kind === "pullover") renderPullover(msg.result);
        else if (msg.kind === "export") renderExport(msg.result);
        else if (msg.kind === "dye" && target) renderDye(msg.result, target.pixels, target.w, target.h);
      } catch (err) {
        if (msg.kind === "export") clearExportResult();
        if (msg.kind === "pullover") clearPulloverResult();
        statusEl.textContent = "Render refused.";
        showError(String(err?.message || err));
      }
    }
  };
}

runSwatchBtn.addEventListener("click", () => {
  clearError();
  let needles;
  let coursesPerZone;
  let cams;
  try {
    needles = parseIntRange("swatchNeedles", "Needles", 4, 80);
    coursesPerZone = parseIntRange("swatchCourses", "Courses / Cam Zone", 1, 200);
    const rawCams = document.getElementById("swatchCams").value.split(",");
    cams = rawCams.map((token) => {
      const trimmed = token.trim();
      if (!trimmed) throw new Error("Cam settings must be a comma-separated list of finite numbers.");
      const num = Number(trimmed);
      if (!Number.isFinite(num)) throw new Error(`Invalid cam setting: "${trimmed}".`);
      return num;
    });
  } catch (err) {
    statusEl.textContent = "Invalid input.";
    showError(String(err?.message || err));
    return;
  }
  const materialSha256 = document.getElementById("swatchMaterialSha256").value.trim();
  const request = ++reqId;
  latestRequestByKind.swatch = request;
  statusEl.textContent = "Generating verified calibration swatch kit…";
  worker.postMessage({
    type: "swatch",
    request,
    payload: {
      machine: document.getElementById("swatchMachine").value,
      yarn_id: document.getElementById("swatchYarn").value.trim() || "merino-200tex",
      ...(materialSha256 ? { material_sha256: materialSha256 } : {}),
      needles,
      courses_per_zone: coursesPerZone,
      cam_settings: cams,
    },
  });
});

runPanelBtn.addEventListener("click", () => {
  clearError();
  const request = ++reqId;
  latestRequestByKind.panel = request;
  clearDownloads(document.getElementById("panelDownloads"));
  document.getElementById("panelOut").hidden = true;
  let hem;
  let chest;
  let length;
  let rib;
  let neckWidth;
  let neckDepth;
  let shoulderPairs;
  try {
    hem = parseFiniteInput("panelHem", "Hem Width (mm)");
    chest = parseFiniteInput("panelChest", "Chest Width (mm)");
    length = parseFiniteInput("panelLength", "Length (mm)");
    if (hem <= 0 || chest <= 0 || length <= 0) {
      throw new Error("Panel widths and length must be positive numbers.");
    }
    rib = parseIntRange("panelRib", "Rib Hem Courses", 0, 200);
    neckWidth = parseFiniteInput("panelNeckWidth", "Neck Opening Width (mm)");
    neckDepth = parseFiniteInput("panelNeckDepth", "Neck Drop (mm)");
    shoulderPairs = parseIntRange("panelShoulderPairs", "Short-Row Pairs per Shoulder", 0, 30);
    if (neckWidth < 0 || neckWidth >= chest || neckDepth < 0 || neckDepth >= length) {
      throw new Error("Neck opening width and drop must be nonnegative and smaller than the panel chest width and length.");
    }
    if ((neckWidth > 0) !== (neckDepth > 0)) {
      throw new Error("Set both neckline dimensions to zero for a straight top, or supply both a positive width and drop.");
    }
    if (shoulderPairs > 0 && neckWidth === 0) {
      throw new Error("Shoulder short rows require a positive neck opening width and drop.");
    }
  } catch (err) {
    statusEl.textContent = "Invalid input.";
    showError(String(err?.message || err));
    return;
  }
  const machine = document.getElementById("panelMachine").value;
  const defaults = PANEL_MACHINE_DEFAULTS[machine] || PANEL_MACHINE_DEFAULTS["kniterate-7e"];
  if (rib > 0 && (machine === "brother-kh930-ayab" || machine === "brother-kh270-ayab")) {
    statusEl.textContent = "Unsupported construction.";
    showError("This single-bed Brother profile cannot form the requested rib. Set Rib Hem Courses to zero or choose a double-bed profile.");
    return;
  }
  const armholeDepth = Math.min(95, Math.max(10, Math.round(length * 0.31)));
  const armholeInset = Math.min(36, Math.max(0, Math.round(chest * 0.13)));
  statusEl.textContent = "Drafting measured shaped panel…";
  worker.postMessage({
    type: "panel",
    request,
    payload: {
      machine,
      shoulder_short_row_pairs: shoulderPairs,
      panel: {
        name: "workbench_panel",
        wearer_age_class: document.getElementById("panelAgeClass").value,
        hem_width_mm: hem,
        chest_width_mm: chest,
        length_mm: length,
        armhole_depth_mm: armholeDepth,
        armhole_inset_total_mm: armholeInset,
        neck_width_mm: neckWidth,
        neck_depth_mm: neckDepth,
        rib_hem_courses: rib,
        cam_setting: defaults.cam_setting,
        yarn_id: "merino-200tex",
        carrier_id: defaults.carrier_id,
        has_hood_or_neck_drawstring: false,
      },
      gauge: {
        wales_per_100mm: 22.0,
        courses_per_100mm: 30.0,
        needle_pitch_mm: defaults.needle_pitch_mm,
        yarn_tex: defaults.yarn_tex,
        relaxed_after_wash: false,
      },
    },
  });
});

runPulloverBtn.addEventListener("click", async () => {
  clearError();
  invalidatePullover();
  const request = latestRequestByKind.pullover;
  try {
    if (!engineReady) throw new Error("Wait for the local engine to finish loading.");
    const machine = document.getElementById("pulloverMachine").value;
    const defaults = PANEL_MACHINE_DEFAULTS[machine] || PANEL_MACHINE_DEFAULTS["hand-knit"];
    const chest = parseFiniteInput("pulloverChest", "Chest Girth (mm)");
    const hem = parseFiniteInput("pulloverHem", "Hem Girth (mm)");
    const bodyLength = parseFiniteInput("pulloverBodyLength", "Body Length (mm)");
    const armhole = parseFiniteInput("pulloverArmhole", "Armhole Depth (mm)");
    const sleeveLength = parseFiniteInput("pulloverSleeveLength", "Sleeve Length (mm)");
    const cuff = parseFiniteInput("pulloverCuff", "Cuff Girth (mm)");
    const neckWidth = parseFiniteInput("pulloverNeckWidth", "Neck Width (mm)");
    const seamTol = parseFiniteInput("pulloverSeamTolerance", "Max Seam Mismatch (mm)");
    if (chest <= 0 || hem <= 0 || bodyLength <= 0 || armhole <= 0 || sleeveLength <= 0 || cuff <= 0 || neckWidth <= 0) {
      throw new Error("Pullover dimensions must be positive numbers.");
    }
    if (seamTol < 0) {
      throw new Error("Max Seam Mismatch (mm) cannot be negative.");
    }
    const bodyRib = parseIntRange("pulloverBodyRib", "Body Rib Courses", 0, 40);
    const cuffRib = parseIntRange("pulloverCuffRib", "Cuff Rib Courses", 0, 40);
    const qualFile = document.getElementById("pulloverQualification").files[0];
    if (qualFile && (!Number.isSafeInteger(qualFile.size) || qualFile.size < 0 || qualFile.size > MAX_EXPORT_INPUT_BYTES)) {
      throw new Error("Qualification file exceeds the 4 MiB budget.");
    }
    const materialSha256 = document.getElementById("pulloverMaterialSha256").value.trim();
    const created = document.getElementById("pulloverCreated").value;
    if (materialSha256 && !/^[0-9a-fA-F]{64}$/.test(materialSha256)) {
      throw new Error("Source Material SHA-256 must contain exactly 64 hexadecimal characters.");
    }
    const qualificationJson = qualFile ? await qualFile.text() : undefined;
    if (request !== latestRequestByKind.pullover || !engineReady) return;
    // Read-only inspection aligns drafted gauge/cam/lot to the supplied qualification while
    // forwarding the exact raw `qualificationJson` text untouched across the WASM boundary.
    let parsedQual = null;
    if (qualificationJson) {
      try {
        const candidate = JSON.parse(qualificationJson);
        if (candidate && typeof candidate === "object") parsedQual = candidate;
      } catch {
        parsedQual = null;
      }
    }
    const qualWales = Number.isFinite(parsedQual?.wales_per_100mm) && parsedQual.wales_per_100mm > 0
      ? parsedQual.wales_per_100mm
      : 20.0;
    const qualCourses = Number.isFinite(parsedQual?.courses_per_100mm) && parsedQual.courses_per_100mm > 0
      ? parsedQual.courses_per_100mm
      : 25.0;
    const qualThickness = Number.isFinite(parsedQual?.thickness_mm) && parsedQual.thickness_mm > 0
      ? parsedQual.thickness_mm
      : 2.0;
    const qualCam = Number.isFinite(parsedQual?.cam_stitch_setting)
      ? parsedQual.cam_stitch_setting
      : defaults.cam_setting;
    const qualYarnId = typeof parsedQual?.yarn_lot_id === "string" && parsedQual.yarn_lot_id.trim()
      ? parsedQual.yarn_lot_id.trim()
      : "merino-200tex";
    const relaxedAfterWash = qualFile
      ? parsedQual?.was_washed_and_blocked === true
      : machine === "hand-knit";
    const payload = {
      machine,
      spec: {
        name: "workbench_seamed_pullover",
        wearer_age_class: document.getElementById("pulloverAgeClass").value,
        chest_girth_mm: chest,
        hem_girth_mm: hem,
        body_length_mm: bodyLength,
        armhole_depth_mm: armhole,
        sleeve_length_mm: sleeveLength,
        cuff_girth_mm: cuff,
        neck_width_mm: neckWidth,
        front_neck_depth_mm: 0.0,
        back_neck_depth_mm: 0.0,
        body_rib_courses: bodyRib,
        cuff_rib_courses: cuffRib,
        cam_setting: qualCam,
        yarn_id: qualYarnId,
        carrier_id: defaults.carrier_id,
        max_seam_length_mismatch_mm: seamTol,
        has_hood_or_neck_drawstring: false,
      },
      gauge: {
        wales_per_100mm: qualWales,
        courses_per_100mm: qualCourses,
        needle_pitch_mm: defaults.needle_pitch_mm,
        yarn_tex: defaults.yarn_tex,
        thickness_mm: qualThickness,
        relaxed_after_wash: relaxedAfterWash,
      },
      ...(qualFile ? { qualification_json: qualificationJson } : {}),
      ...(materialSha256 ? { material_sha256: materialSha256 } : {}),
      ...(created ? { created } : {}),
    };
    if (new TextEncoder().encode(JSON.stringify(payload)).byteLength > MAX_EXPORT_INPUT_BYTES) {
      throw new Error("The encoded pullover request exceeds the 4 MiB budget.");
    }
    statusEl.textContent = "Drafting, seam-checking, and packaging four-piece pullover ZIP…";
    worker.postMessage({ type: "pullover", request, payload });
  } catch (err) {
    if (request !== latestRequestByKind.pullover) return;
    clearPulloverResult();
    statusEl.textContent = "Pullover refused.";
    showError(String(err?.message || err));
  }
});

for (const id of [
  "pulloverMachine",
  "pulloverAgeClass",
  "pulloverChest",
  "pulloverHem",
  "pulloverBodyLength",
  "pulloverArmhole",
  "pulloverSleeveLength",
  "pulloverCuff",
  "pulloverNeckWidth",
  "pulloverBodyRib",
  "pulloverCuffRib",
  "pulloverSeamTolerance",
  "pulloverQualification",
  "pulloverMaterialSha256",
  "pulloverCreated",
]) {
  const input = document.getElementById(id);
  const changed = () => {
    invalidatePullover();
    clearError();
    statusEl.textContent = engineReady
      ? "Pullover inputs changed. Run again to package current downloads."
      : "Waiting for the local engine…";
  };
  input.addEventListener("input", changed);
  input.addEventListener("change", changed);
}

runExportBtn.addEventListener("click", async () => {
  clearError();
  invalidateExport();
  const request = latestRequestByKind.export;
  try {
    if (!engineReady) throw new Error("Wait for the local engine to finish loading.");
    const kind = document.getElementById("exportKind").value || "saved";
    if (!["saved", "shape", "measured"].includes(kind)) throw new Error("Choose a supported starting point.");
    const requiredFile = (id, label) => {
      const file = document.getElementById(id).files[0];
      if (!file) throw new Error(`Choose ${label}.`);
      return file;
    };
    const qualification = document.getElementById("exportQualification").files[0];
    const drapeReceipt = document.getElementById("exportDrapeReceipt").files[0];
    const sources = kind === "saved"
      ? { pattern_json: requiredFile("exportPattern", "a saved pattern JSON file") }
      : kind === "shape"
        ? {
          shell_json: requiredFile("exportShell", "a 3D shell JSON file"),
          config: requiredFile("exportShapeConfig", "shaping choices JSON"),
          gauge: requiredFile("exportShapeGauge", "relaxed gauge JSON"),
        }
        : {
          input_json: requiredFile("exportBodyDesign", "a garment design JSON file"),
          measurements_json: requiredFile("exportMeasurements", "a measurements JSON file"),
        };
    const bodyFit = kind === "measured" ? document.getElementById("exportBodyFit").files[0] : null;
    const model = kind === "measured" ? document.getElementById("exportBodyModel").files[0] : null;
    if (Boolean(bodyFit) !== Boolean(model)) {
      throw new Error("An accepted body report requires its matching body model, and vice versa.");
    }
    if (model && (!Number.isSafeInteger(model.size) || model.size <= 0 || model.size > 512 * 1024 * 1024)) {
      throw new Error("The body model must be a nonempty file no larger than 512 MiB.");
    }
    if (bodyFit) sources.body_fit_json = bodyFit;
    if (qualification) sources.qualification_json = qualification;
    if (drapeReceipt) sources.drape_receipt_json = drapeReceipt;
    const limit = kind === "saved" ? MAX_EXPORT_INPUT_BYTES : MAX_DRAFT_INPUT_BYTES;
    const files = Object.values(sources);
    if (files.some((file) => !Number.isSafeInteger(file.size) || file.size < 0) ||
        files.reduce((total, file) => total + file.size, 0) > limit) {
      throw new Error(`Source and evidence files exceed the combined ${limit / (1024 * 1024)} MiB budget.`);
    }
    const machine = document.getElementById("exportMachine").value;
    const materialSha256 = document.getElementById("exportMaterialSha256").value.trim();
    const created = document.getElementById("exportCreated").value;
    if (materialSha256 && !/^[0-9a-fA-F]{64}$/.test(materialSha256)) {
      throw new Error("Source Material SHA-256 must contain exactly 64 hexadecimal characters.");
    }
    statusEl.textContent = "Reading local pattern and evidence files…";
    cancelExportBtn.disabled = false;
    // Original shell, measurement, accepted-body and sealed-evidence texts stay unchanged.
    const texts = Object.fromEntries(await Promise.all(
      Object.entries(sources).map(async ([name, file]) => [name, await file.text()]),
    ));
    if (request !== latestRequestByKind.export || !engineReady) return;
    const payload = {
      ...(kind === "measured" ? {} : { machine }),
      ...texts,
      ...(materialSha256 ? { material_sha256: materialSha256 } : {}),
      ...(created ? { created } : {}),
    };
    if (kind === "shape") {
      payload.config = JSON.parse(texts.config);
      payload.gauge = JSON.parse(texts.gauge);
    }
    // JSON string escaping also counts against the WASM request's UTF-8 byte budget.
    if (new TextEncoder().encode(JSON.stringify(payload)).byteLength > limit) {
      throw new Error(`The encoded source and evidence request exceeds the ${limit / (1024 * 1024)} MiB budget.`);
    }
    const modelBytes = model ? new Uint8Array(await model.arrayBuffer()) : null;
    if (request !== latestRequestByKind.export || !engineReady) return;
    statusEl.textContent = "Verifying the saved pattern and building local export artifacts…";
    worker.postMessage({
      type: kind === "saved" ? "export" : kind, request, payload,
      ...(modelBytes ? { model_bytes: modelBytes } : {}),
    }, modelBytes ? [modelBytes.buffer] : []);
  } catch (err) {
    if (request !== latestRequestByKind.export) return;
    clearExportResult();
    statusEl.textContent = "Export refused.";
    showError(String(err?.message || err));
  }
});

for (const id of [
  "exportKind", "exportMachine", "exportPattern", "exportQualification", "exportDrapeReceipt", "exportMaterialSha256", "exportCreated",
  "exportShell", "exportShapeConfig", "exportShapeGauge", "exportBodyDesign", "exportMeasurements", "exportBodyFit", "exportBodyModel",
]) {
  const input = document.getElementById(id);
  const changed = () => {
    invalidateExport();
    selectExportMode();
    clearError();
    statusEl.textContent = engineReady
      ? "Export inputs changed. Verify again to generate current downloads."
      : "Waiting for the local engine…";
  };
  input.addEventListener("input", changed);
  input.addEventListener("change", changed);
}

cancelExportBtn.addEventListener("click", () => {
  invalidateExport();
  for (const kind of ["swatch", "panel", "dye"]) latestRequestByKind[kind] = ++reqId;
  targetByRequest.clear();
  worker?.terminate();
  worker = null;
  engineReady = false;
  for (const button of [runSwatchBtn, runPanelBtn, runExportBtn, runDyeBtn]) button.disabled = true;
  clearError();
  statusEl.textContent = "Cancelled. Restarting the local engine…";
  startWorker();
});

runDyeBtn.addEventListener("click", () => {
  clearError();
  let courses;
  let wales;
  let driftEps;
  let driftSigma;
  let wickMm;
  try {
    courses = parseIntRange("dyeCourses", "Grid Courses", 2, 40);
    wales = parseIntRange("dyeWales", "Grid Wales", 2, 40);
    driftEps = parseFiniteInput("dyeEps", "Systematic Drift ε");
    driftSigma = parseFiniteInput("dyeSigma", "Brownian σ_η");
    wickMm = parseFiniteInput("dyeWick", "Wicking σ (mm)");
    if (driftSigma < 0 || wickMm < 0) {
      throw new Error("Brownian σ_η and wicking σ (mm) cannot be negative.");
    }
  } catch (err) {
    statusEl.textContent = "Invalid input.";
    showError(String(err?.message || err));
    return;
  }
  const preset = document.getElementById("dyePreset").value;
  const isCircular = document.getElementById("dyeMode").value === "circular";
  const selectedRegime = document.getElementById("dyeRegime")?.value || "auto";
  const regime = selectedRegime === "auto"
    ? ((preset === "horizon" || preset === "sunset")
      ? "regime1_a_stripes"
      : "regime1_b_circular_pooling")
    : selectedRegime;
  const pixels = buildPresetPixels(preset, wales, courses);
  const request = ++reqId;
  latestRequestByKind.dye = request;
  targetByRequest.set(request, { pixels, w: wales, h: courses });
  statusEl.textContent = "Running Kubelka–Munk + Sobol-CVaR Bertsekas L-BFGS optimizer…";
  worker.postMessage({
    type: "dye",
    request,
    payload: {
      machine: document.getElementById("dyeMachine").value,
      wearer_age_class: document.getElementById("dyeAgeClass").value,
      regime,
      width: wales,
      height: courses,
      srgb_u8: pixels,
      grid: [courses, wales],
      is_circular: isCircular,
      drift_eps: driftEps,
      drift_sigma_eta: driftSigma,
      wick_mm: wickMm,
      max_iters: 18,
    },
  });
});

if (typeof window !== "undefined" && typeof window.addEventListener === "function") {
  window.addEventListener("pagehide", () => {
    invalidatePullover();
    invalidateExport();
    for (const kind of ["swatch", "panel", "dye"]) latestRequestByKind[kind] = ++reqId;
    targetByRequest.clear();
    for (const id of ["swatchDownloads", "panelDownloads", "dyeDownloads"]) {
      const el = document.getElementById(id);
      if (el) clearDownloads(el);
    }
  });
}

selectExportMode();
startWorker();
