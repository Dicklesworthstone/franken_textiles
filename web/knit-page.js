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
  "kniterate-7e": { needle_pitch_mm: 25.4 / 7.0, carrier_id: "1" },
  "brother-kh930-ayab": { needle_pitch_mm: 4.5, carrier_id: "A" },
  "hand-knit": { needle_pitch_mm: 4.0, carrier_id: "1" },
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

function makeDownload(container, filename, text, mime = "text/plain") {
  const url = URL.createObjectURL(new Blob([text], { type: mime }));
  const urls = downloadUrlsByContainer.get(container) || [];
  urls.push(url);
  downloadUrlsByContainer.set(container, urls);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.textContent = `Download ${filename}`;
  container.append(a);
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

function renderSwatch(res) {
  const out = document.getElementById("swatchOut");
  const dl = document.getElementById("swatchDownloads");
  const svgBox = document.getElementById("swatchSvgBox");
  const pre = document.getElementById("swatchPre");
  clearDownloads(dl);
  makeDownload(dl, "swatch.k", res.knitout_text, "text/plain");
  if (res.kc_text) makeDownload(dl, "swatch.kc", res.kc_text, "text/plain");
  if (res.ayab_checklist) {
    makeDownload(dl, "ayab_session_checklist.json", JSON.stringify(res.ayab_checklist, null, 2), "application/json");
  }
  makeDownload(dl, "swatch_chart.svg", res.chart_svg, "image/svg+xml");
  makeDownload(dl, "swatch_cert.json", JSON.stringify(res.cert, null, 2), "application/json");
  makeDownload(dl, "swatch_qualification_template.json", JSON.stringify(res.qualification_template, null, 2), "application/json");

  svgBox.innerHTML = res.chart_svg;
  pre.textContent =
    `=== Certificate (hardware_execution_authorized=${res.cert.hardware_execution_authorized}) ===\n` +
    JSON.stringify(res.cert, null, 2) +
    "\n\n=== Knitout v2 (first 40 lines) ===\n" +
    res.knitout_text.split("\n").slice(0, 40).join("\n");
  out.hidden = false;
}

function renderPanel(res) {
  const out = document.getElementById("panelOut");
  document.getElementById("panelSummary").textContent =
    `Drafted ${res.stitch_count} stitches (${res.short_row_turns} anchored short-row turns) · doc SHA-256 ${res.doc_sha256.slice(0, 12)}… · SVG checks=${res.svg_checks_passed}`;
  document.getElementById("panelSvgBox").innerHTML = res.chart_svg;
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
    : "Wearable age classes require an Exp-4 fastness qualification receipt before emitting controller G-code; JSON dye program is ready for review.";
  out.hidden = false;
}

function startWorker() {
  worker = new Worker("knit-worker.js", { type: "module" });
  worker.onmessage = ({ data: msg }) => {
    if (msg.type === "ready") {
      statusEl.textContent = "Local WASM knitting & dyeing engine ready.";
      runSwatchBtn.disabled = false;
      runPanelBtn.disabled = false;
      runDyeBtn.disabled = false;
    } else if (msg.type === "loadError") {
      statusEl.textContent = "Engine unavailable.";
      showError(msg.message);
    } else if (msg.type === "refusal") {
      targetByRequest.delete(msg.request);
      statusEl.textContent = `Refused (${msg.code}).`;
      showError(`[${msg.code}] ${msg.message}`);
    } else if (msg.type === "error") {
      targetByRequest.delete(msg.request);
      statusEl.textContent = "Error.";
      showError(msg.message);
    } else if (msg.type === "result") {
      const target = targetByRequest.get(msg.request);
      targetByRequest.delete(msg.request);
      if (msg.request && latestRequestByKind[msg.kind] && msg.request !== latestRequestByKind[msg.kind]) {
        return;
      }
      clearError();
      statusEl.textContent = `Completed ${msg.kind}.`;
      if (msg.kind === "swatch") renderSwatch(msg.result);
      else if (msg.kind === "panel") renderPanel(msg.result);
      else if (msg.kind === "dye" && target) renderDye(msg.result, target.pixels, target.w, target.h);
    }
  };
}

runSwatchBtn.addEventListener("click", () => {
  clearError();
  const cams = document.getElementById("swatchCams").value
    .split(",").map((s) => Number(s.trim())).filter((v) => Number.isFinite(v));
  const request = ++reqId;
  latestRequestByKind.swatch = request;
  statusEl.textContent = "Generating verified calibration swatch kit…";
  worker.postMessage({
    type: "swatch",
    request,
    payload: {
      machine: document.getElementById("swatchMachine").value,
      yarn_id: document.getElementById("swatchYarn").value.trim() || "merino-200tex",
      needles: Number(document.getElementById("swatchNeedles").value),
      courses_per_zone: Number(document.getElementById("swatchCourses").value),
      cam_settings: cams,
    },
  });
});

runPanelBtn.addEventListener("click", () => {
  clearError();
  const machine = document.getElementById("panelMachine").value;
  const defaults = PANEL_MACHINE_DEFAULTS[machine] || PANEL_MACHINE_DEFAULTS["kniterate-7e"];
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
        hem_width_mm: Number(document.getElementById("panelHem").value),
        chest_width_mm: Number(document.getElementById("panelChest").value),
        length_mm: Number(document.getElementById("panelLength").value),
        armhole_depth_mm: 95,
        armhole_inset_total_mm: 36,
        neck_width_mm: 105,
        neck_depth_mm: 24,
        rib_hem_courses: Number(document.getElementById("panelRib").value),
        cam_setting: 7.0,
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
  const courses = Number(document.getElementById("dyeCourses").value);
  const wales = Number(document.getElementById("dyeWales").value);
  const preset = document.getElementById("dyePreset").value;
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
      regime: preset === "horizon" ? "regime1_a_stripes" : "regime1_b_circular_pooling",
      width: wales,
      height: courses,
      srgb_u8: pixels,
      grid: [courses, wales],
      is_circular: document.getElementById("dyeMode").value === "circular",
      drift_eps: Number(document.getElementById("dyeEps").value),
      drift_sigma_eta: Number(document.getElementById("dyeSigma").value),
      wick_mm: Number(document.getElementById("dyeWick").value),
      max_iters: 18,
    },
  });
});

startWorker();
