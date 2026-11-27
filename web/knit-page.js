// Local browser controller for web/knit.html (FTX-K18).
const statusEl = document.getElementById("knitStatus");
const errorEl = document.getElementById("error");
const runSwatchBtn = document.getElementById("runSwatch");
const runPanelBtn = document.getElementById("runPanel");
const runDyeBtn = document.getElementById("runDye");

let worker = null;
let reqId = 0;
const latestRequestByKind = { swatch: 0, panel: 0, dye: 0 };
const targetByRequest = new Map();
const downloadUrlsByContainer = new WeakMap();

const PANEL_MACHINE_DEFAULTS = {
  "kniterate-7e": { needle_pitch_mm: 25.4 / 7.0, carrier_id: "1", cam_setting: 7.0 },
  "brother-kh930-ayab": { needle_pitch_mm: 4.5, carrier_id: "A", cam_setting: 7.0 },
  "brother-kh270-ayab": { needle_pitch_mm: 9.0, carrier_id: "A", cam_setting: 6.0 },
  "hand-knit": { needle_pitch_mm: 4.0, carrier_id: "1", cam_setting: 7.0 },
  "shima-seiki-12e-handoff": { needle_pitch_mm: 25.4 / 12.0, carrier_id: "1", cam_setting: 45.0 },
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
  if (res.machine_executable !== false || res.kc_text) {
    throw new Error("This engine returned an unqualified machine export. Rebuild and reload the current WASM bundle; Kniterate requires conversion of the verified Knitout file.");
  }
  renderSvg(svgBox, res.chart_svg);
  makeDownload(dl, "swatch.k", res.knitout_text, "text/plain");
  if (res.backend_export_refusal) {
    makeDownload(dl, "backend_export_refusal.json", JSON.stringify(res.backend_export_refusal, null, 2), "application/json");
  }
  if (res.ayab_png_base64) {
    makeDownload(dl, "swatch_ayab.png", decodeBase64Bytes(res.ayab_png_base64), "image/png");
  }
  if (res.ayab_checklist) {
    makeDownload(dl, "ayab_session_checklist.json", JSON.stringify(res.ayab_checklist, null, 2), "application/json");
  }
  makeDownload(dl, "swatch_chart.svg", res.chart_svg, "image/svg+xml");
  makeDownload(dl, "swatch_cert.json", JSON.stringify(res.cert, null, 2), "application/json");
  makeDownload(dl, "swatch_qualification_template.json", JSON.stringify(res.qualification_template, null, 2), "application/json");

  const conversionNotice = res.backend_export_refusal
    ? `Machine conversion required [${res.backend_export_refusal.code}]: ${res.backend_export_refusal.message}\n${(res.backend_export_refusal.repairs || []).join("\n")}\n\n`
    : "";
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
  renderSvg(document.getElementById("panelSvgBox"), res.chart_svg);
  if (dl) {
    clearDownloads(dl);
    makeDownload(dl, "panel_chart.svg", res.chart_svg, "image/svg+xml");
    makeDownload(dl, "panel_instructions.txt", res.instructions_text, "text/plain");
    makeDownload(dl, "panel_metadata.json", JSON.stringify(res.metadata, null, 2), "application/json");
  }
  document.getElementById("panelSummary").textContent =
    `Drafted ${res.stitch_count} stitches for a straight-top panel · doc SHA-256 ${res.doc_sha256.slice(0, 12)}… · SVG checks=${res.svg_checks_passed} · fit_claimed=${res.fit_claimed}`;
  document.getElementById("panelPre").textContent =
    JSON.stringify(res.metadata, null, 2) + "\n\n" + res.instructions_text;
  out.hidden = false;
}

function renderDye(res, targetPixels, targetW, targetH) {
  const out = document.getElementById("dyeOut");
  const [courses, wales] = res.grid;
  document.getElementById("dyeMetrics").textContent =
    `CIEDE2000 p50=${res.delta_e00_p50.toFixed(2)}, p95=${res.delta_e00_p95.toFixed(2)} · gamut floor=${res.gamut_floor_delta_e00.toFixed(2)} · ` +
    `coherent horizon: circular=${res.coherent_horizon_circular_stitches} st / flatbed=${res.coherent_horizon_flatbed_stitches} st · ` +
    `iterations=${res.iterations}`;

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
  statusEl.textContent = "Engine unavailable.";
  runSwatchBtn.disabled = true;
  runPanelBtn.disabled = true;
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
  worker = new Worker("knit-worker.js", { type: "module" });
  worker.onerror = (event) => {
    failEngine(String(event?.message || "Knitting & dyeing worker failed to load."));
  };
  worker.onmessageerror = () => {
    failEngine("Knitting & dyeing worker returned an unreadable message.");
  };
  worker.onmessage = ({ data: msg }) => {
    if (msg.type === "ready") {
      statusEl.textContent = "Local WASM knitting & dyeing engine ready.";
      runSwatchBtn.disabled = false;
      runPanelBtn.disabled = false;
      runDyeBtn.disabled = false;
    } else if (msg.type === "loadError") {
      failEngine(msg.message);
    } else if (msg.type === "refusal") {
      targetByRequest.delete(msg.request);
      if (isStaleReply(msg)) return;
      statusEl.textContent = `Refused (${msg.code}).`;
      showError(`[${msg.code}] ${msg.message}`);
    } else if (msg.type === "error") {
      targetByRequest.delete(msg.request);
      if (isStaleReply(msg)) return;
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
        else if (msg.kind === "dye" && target) renderDye(msg.result, target.pixels, target.w, target.h);
      } catch (err) {
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
  let hem;
  let chest;
  let length;
  let rib;
  try {
    hem = parseFiniteInput("panelHem", "Hem Width (mm)");
    chest = parseFiniteInput("panelChest", "Chest Width (mm)");
    length = parseFiniteInput("panelLength", "Length (mm)");
    if (hem <= 0 || chest <= 0 || length <= 0) {
      throw new Error("Panel widths and length must be positive numbers.");
    }
    rib = parseIntRange("panelRib", "Rib Hem Courses", 0, 200);
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
  const request = ++reqId;
  latestRequestByKind.panel = request;
  statusEl.textContent = "Drafting measured shaped panel…";
  worker.postMessage({
    type: "panel",
    request,
    payload: {
      machine,
      panel: {
        name: "workbench_panel",
        wearer_age_class: document.getElementById("panelAgeClass").value,
        hem_width_mm: hem,
        chest_width_mm: chest,
        length_mm: length,
        armhole_depth_mm: armholeDepth,
        armhole_inset_total_mm: armholeInset,
        neck_width_mm: 0,
        neck_depth_mm: 0,
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
        yarn_tex: 200.0,
        relaxed_after_wash: false,
      },
    },
  });
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
    ? (preset === "horizon"
      ? "regime1_a_stripes"
      : isCircular
        ? "regime1_b_circular_pooling"
        : "regime2_fiducial_synced")
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
    for (const id of ["swatchDownloads", "panelDownloads", "dyeDownloads"]) {
      const el = document.getElementById(id);
      if (el) clearDownloads(el);
    }
  });
}

startWorker();
