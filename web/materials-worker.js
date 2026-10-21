// The only network request loads the local application bundle. User evidence is never uploaded.
// Every operation carries explicit inputs; no old pattern/drape survives a failed replacement.
import init, * as ftx from "./pkg/ftx_wasm.js";
let failure = null, active = null;
const ready = init().then(() => postMessage({ type: "ready" })).catch(e => {
  failure = e; postMessage({ type: "loadError", message: String(e.message || e) });
});
self.ftxProgress = text => postMessage({ type: "progress", request: active, text });
const json = text => JSON.parse(text);
const file = (name, value, mime = "application/json") => ({ name, text: typeof value === "string" ? value : JSON.stringify(value, null, 2), mime });
// Display only: never recalculate a price, rank an offer, or alter source-bearing JSON.
// The self-contained review has no scripts, external assets, source links or network requests.
function stockReview(plan) {
  const parts = [], encoder = new TextEncoder(); let bytes = 0;
  const escape = value => {
    const text = String(value ?? "Unknown");
    if (text.length > 32768) throw Error("A review field exceeds 32,768 characters; inspect the original JSON.");
    return text.replace(/[&<>"']/g, c => ({"&":"&amp;", "<":"&lt;", ">":"&gt;", '"':"&quot;", "'":"&#39;"})[c]);
  };
  const add = html => {
    bytes += encoder.encode(html).byteLength;
    if (bytes > 4 * 1024 * 1024) throw Error("Stock review exceeds 4 MiB; original JSON products remain available.");
    parts.push(html);
  };
  const number = value => Number.isFinite(value) ? String(value) : "Unknown";
  const row = values => add(`<tr>${values.map(value => `<td>${escape(value)}</td>`).join("")}</tr>`);
  const table = (caption, headings) => {
    add(`<table><caption>${escape(caption)}</caption><thead><tr>${headings.map(h => `<th scope="col">${escape(h)}</th>`).join("")}</tr></thead><tbody>`);
  };
  const end = () => add("</tbody></table>");
  if (plan.schema !== "ftx-finite-stock-plan/1" || !Array.isArray(plan.markers)) throw Error("Stock review requires a complete native plan.");
  const optimization = plan.optimization, pricing = plan.purchasing, market = plan.request?.pricing?.market;
  const objective = optimization?.objective;
  const label = objective === "minimize_purchase_cost" ? "Lower quoted purchase cost" :
    objective === "minimize_stock_count" ? "Use fewer stock items" :
    objective === "minimize_gross_area" ? "Use less whole-stock area" : "First complete cutting layout";
  if (pricing && (!Array.isArray(pricing.stocks) || pricing.stocks.length !== plan.markers.length || market?.currency !== pricing.currency ||
    pricing.stocks.some((s, i) => s.identity?.stock_index !== i || s.identity.stock_id !== plan.markers[i].id ||
      s.identity.used !== Boolean(plan.markers[i].placements?.length)))) throw Error("Stock review lost inventory or currency correspondence.");
  const money = amount => {
    if (!Number.isFinite(amount) || !pricing) return "Not evaluated / unknown";
    const display = Number(amount.toPrecision(12));
    return `${display === amount ? "" : "≈"}${display} ${pricing.currency}`;
  };
  add(`<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'">
<title>Stock and purchasing review · FrankenTextiles</title><style>
body{font:16px/1.5 system-ui,sans-serif;color:#172c36;margin:2rem auto;padding:0 1rem;max-width:1100px}h1{line-height:1.2}
table{border-collapse:collapse;width:100%;margin:1.5rem 0;font-size:.9rem}caption{text-align:left;font-weight:700;margin:.5rem 0}th,td{padding:.5rem;text-align:left;border:1px solid #cbd9dc;vertical-align:top;overflow-wrap:anywhere}
th{background:#f3f6f5}code{overflow-wrap:anywhere}details{margin:1rem 0}.notice{padding:1rem;border-left:4px solid #145b68;background:#f3f6f5}
@media print{body{max-width:none;margin:0}thead{display:table-header-group}tr{break-inside:avoid}}</style></head><body>
<h1>${escape(label)}</h1><p class="notice">Complete cutting layout. This review displays the native result; it does not recalculate costs or certify physical cutting.</p>`);
  if (optimization) {
    const status = optimization.budget_exhausted ? "Search budget reached. The best complete layout found is retained; a cheaper or smaller layout may remain." :
      optimization.lattice_optimal && optimization.termination === "lattice_exhausted" ?
        "No better layout remains on the declared placement grid and allowed orientations. This is not a continuous-space global optimum." :
        "Search completion is not established. Inspect the original optimization record.";
    add(`<p>${escape(status)}</p>`);
    table("First complete layout versus selected layout", ["Result", "Stock items", "Whole-stock area (m²)", "Quoted item total"]);
    for (const [name, use] of [["First complete", optimization.baseline], ["Selected", optimization.selected]]) {
      row([name, number(use?.stock_count), number(use?.gross_area_m2), money(use?.purchase_cost)]);
    }
    end();
    const stop = optimization.termination === "lattice_exhausted" ? "Grid search completed" :
      optimization.termination === "work_limit" ? "Geometry work limit reached" :
      optimization.termination === "backtrack_limit" ? "Placement reconsideration limit reached" : "Unknown; inspect the original report";
    add(`<p>${escape(stop)}. Candidate checks: ${escape(number(plan.candidates))}. Geometry work units: ${escape(number(plan.work_used))}.</p>`);
  } else add("<p>This is a feasible first-complete result, not a minimum-cost or minimum-waste claim.</p>");
  if (pricing) {
    add(`<h2>Purchasing basis</h2><p>Selected whole-item total: <strong>${escape(money(pricing.least_observed_independent_order_total))}</strong>.
Comparison covers admitted whole-item offers only. Unknown or excluded offers are not proved more expensive.</p>`);
    table("Declared market — not a live availability check", ["As of", "Region", "Quality / condition", "Cost terms", "Maximum observation age (days)"]);
    row([market.as_of_at ?? market.as_of, market.region, market.quality, market.terms, number(market.max_age_days)]); end();
    add(`<p>${pricing.complete_observed_quantity_coverage === true ? "Every selected offer has sufficient observed quantity." :
      "At least one selected offer lacks confirmed quantity coverage; availability is not established."} Stock is not reserved and no purchase is executed.</p>`);
  } else add("<p>Purchase costs were not evaluated. Unknown prices must not be interpreted as free stock.</p>");
  table("Physical stock — each selected item is charged once", ["Stock item", "Allocation", "Cut pieces", "Whole area (m²)", "Eligible whole-item cost", "Chosen offer", "Observed", "Quantity evidence"]);
  for (const [i, marker] of plan.markers.entries()) {
    const price = pricing?.stocks[i], selected = Boolean(marker.placements?.length);
    const offer = price?.offers?.find(o => o.quote?.offer_id === price.least_observed_offer_id);
    row([marker.id, selected ? "Selected" : pricing && !offer ? "Excluded from priced search" : "Not selected",
      number(marker.placements?.length), number(marker.gross_area_m2), money(price?.least_observed_order_cost),
      price?.least_observed_offer_id ?? "None", offer?.quote?.observed_at ?? offer?.quote?.observed_on ?? "Unknown",
      offer ? offer.quantity_sufficiency_observed === true ? "Observed sufficient" : "Unknown" : "Not evaluated"]);
  }
  end();
  if (pricing) {
    for (const stock of pricing.stocks) {
      add(`<h2>${escape(stock.identity.stock_id)} — offer evidence</h2>`);
      if (stock.binding_refusal) add(`<p>${escape(stock.binding_refusal.message)}</p>`);
      table("Admitted offers and exclusions retained by the native evaluator", ["Offer", "Result", "Whole-order cost", "Observed", "Source / diagnostic"]);
      const observation = q => q?.observed_at ?? q?.observed_on ?? "Unknown";
      for (const offer of stock.offers ?? []) row([offer.quote?.offer_id,
        offer.quote?.offer_id === stock.least_observed_offer_id ? "Lowest admitted offer for this item" : "Admitted alternative",
        money(offer.order_cost), observation(offer.quote), offer.quote?.source]);
      for (const excluded of stock.source_exclusions ?? []) row([excluded.quote?.offer_id,
        `Excluded: ${String(excluded.reason ?? "unknown reason").replaceAll("_", " ")}`, "Not admitted", observation(excluded.quote), excluded.quote?.source]);
      for (const excluded of stock.stock_exclusions ?? []) row([excluded.quote?.offer_id,
        "Exact-item qualification failed", "Not admitted", observation(excluded.quote), excluded.reason]);
      for (const failed of stock.source_refusals ?? []) row([failed.offer_id, "Source evaluation refused", "Not admitted", "Unknown", failed.refusal?.message]);
      end();
    }
    add("<p>Consumed regions change a stock item's identity. Requalify its remaining-item price before reusing a residual request; the original whole-hide price is not transferred automatically.</p>");
  }
  add(`<h2>Replay and limits</h2><p>Keep the original plan, residual request and purchasing JSON downloads. This HTML is a display, not an input or accounting settlement record. A cost prefixed with ≈ is rounded for display to 12 significant digits; original JSON retains full precision. No taxes, shipping, discounts, exchange rates or offcut values are inferred.</p>
<p>Pattern identity: <code>${escape(plan.pattern_sha256)}</code><br>Stock request identity: <code>${escape(plan.request_sha256)}</code></p>
<p>Source-qualified geometry and offer eligibility are not measured garment fit, machine qualification or hardware authorization.</p></body></html>`);
  return parts.join("");
}
function budget(msg) {
  let bytes = 0;
  for (const value of Object.values(msg)) {
    if (typeof value === "string") bytes += new TextEncoder().encode(value).byteLength;
  }
  if (bytes > 64 * 1024 * 1024) throw Error("Combined JSON inputs exceed 64 MiB.");
  if (msg.bytes?.byteLength > 256 * 1024 * 1024) throw Error("Body model exceeds 256 MiB.");
}

self.onmessage = async ({ data: msg }) => {
  await ready;
  if (failure) return postMessage({ type: "error", request: msg.request, message: String(failure.message || failure), fatal: true });
  active = msg.request;
  try {
    budget(msg);
    let result, files = [], viewer = null, picture = null, raw, reportText = null;
    switch (msg.type) {
      case "catalog": result = json(ftx.material_catalog(msg.query)); break;
      case "inspect": result = json(ftx.material_inspect(msg.selection)); break;
      case "pressure": {
        if (json(msg.compressionRequest).schema === "ftx-compression-stack/1") {
          if (typeof msg.compressionLayer !== "string" || !msg.compressionLayer.trim()) throw Error("Name the stack layer to inspect and assign; no layer is selected automatically.");
          if (typeof ftx.material_compression_stack !== "function") throw Error("Rebuild web/pkg: missing compression-stack engine.");
          raw = ftx.material_compression_stack(msg.compressionRequest); result = json(raw);
          const chosen = result.layers.find(layer => layer.id === msg.compressionLayer);
          if (!chosen) throw Error("The requested stack layer ID is not in this result.");
          if (result.layers.some(layer => typeof layer.conditioned?.selection_json !== "string")) throw Error("Rebuild web/pkg: missing frozen stack selections.");
          // Keep the exact Rust selection bytes. Layer IDs are display text, never download paths.
          result.chosen_layer_id = chosen.id;
          result.selection = chosen.conditioned.selection;
          result.selection_json = chosen.conditioned.selection_json;
          result.inspection = json(ftx.material_inspect(result.selection_json));
          files = [file("compression-stack-report.json", raw), file("compression-request.json", msg.compressionRequest),
            ...result.layers.map((layer, i) => file(`compression-layer-${i + 1}-selection.json`, layer.conditioned.selection_json))];
          break;
        }
        if (msg.compressionLayer) throw Error("A layer ID is only valid for a compression-stack request.");
        raw = ftx.material_pressure_condition(msg.compressionRequest); result = json(raw);
        if (typeof result.selection_json !== "string") throw Error("Rebuild web/pkg: missing pressure-conditioned selection bridge.");
        result.inspection = json(ftx.material_inspect(result.selection_json));
        files = [file("pressure-conditioned-material.json", raw),
          file("pressure-conditioned-selection.json", result.selection_json),
          file("compression-request.json", msg.compressionRequest)];
        break;
      }
      case "calibrate": {
        const identify = ({ nonlinear: ftx.material_calibrate_nonlinear,
          relaxation: ftx.material_calibrate_relaxation,
          membrane_plasticity: ftx.material_calibrate_membrane_plasticity })[msg.calibrationMode];
        if (!["nonlinear", "relaxation", "membrane_plasticity"].includes(msg.calibrationMode)) throw Error("Choose the response to identify.");
        if (typeof identify !== "function") throw Error("Rebuild web/pkg: missing material calibration engine.");
        if (typeof msg.allowEstimates !== "boolean") throw Error("Declare admission of fitted estimates explicitly.");
        raw = identify(msg.calibrationRequest, msg.allowEstimates); result = json(raw);
        if (result.calibration?.schema === "ftx-membrane-plasticity-calibration-report/1" && result.calibration.status !== "ACCEPTED") {
          if (result.selection !== null || result.selection_json !== null || result.material !== null) {
            throw Error("Unresolved permanent-stretch identification cannot publish a selection.");
          }
          files = [file("material-calibration.json", raw), file("calibration-input.json", msg.calibrationRequest)];
          break;
        }
        if (result.comparison?.schema === "ftx-relaxation-model-comparison-report/1" && result.comparison.status !== "ACCEPTED") {
          if (result.selection_json !== null || result.calibration !== null) throw Error("Unresolved model comparison cannot publish a selection.");
          files = [file("material-calibration.json", raw), file("calibration-input.json", msg.calibrationRequest)];
          break;
        }
        if (typeof result.selection_json !== "string" || !result.material?.capabilities) {
          throw Error("Rebuild web/pkg: missing calibrated selection and capability bridge.");
        }
        files = [file("material-calibration.json", raw), file("calibrated-selection.json", result.selection_json),
          file("calibration-input.json", msg.calibrationRequest)];
        break;
      }
      case "screen":
        raw = ftx.material_screen(msg.screenRequest); result = json(raw);
        files = [file("material-screen.json", raw), file("screen-request.json", msg.screenRequest)];
        break;
      case "study":
        raw = ftx.material_study(msg.pattern, msg.studyRequest); result = json(raw);
        files = [file("material-study.json", raw), file("original-pattern.json", msg.pattern),
          file("study-request.json", msg.studyRequest)];
        break;
      case "scenario": {
        // A new handle for every invocation: no previous alternative's body or cloth state.
        const model = new ftx.WebBody(new Uint8Array(msg.bytes));
        try {
          raw = ftx.material_scenario_study(model, msg.pattern, msg.studyRequest, msg.scenarioRequest, msg.measurements);
          result = json(raw);
        } finally { model.free(); }
        // Save original JSON text, not the display objects: number spellings participate in locks.
        // The large local model stays with its owner; the report identifies its exact source hash.
        files = [file("material-scenario-study.json", raw), file("original-pattern.json", msg.pattern),
          file("study-request.json", msg.studyRequest), file("scenario-request.json", msg.scenarioRequest),
          file("body-input.json", msg.measurements)];
        break;
      }
      case "bind":
        raw = ftx.material_bind(msg.pattern, msg.changes); result = json(raw);
        if (typeof result.pattern_json !== "string") throw Error("Rebuild web/pkg: missing lossless material pattern bridge.");
        files = [file("pattern.json", result.pattern_json), file("material-bindings.json", raw),
          file("material-changes.json", msg.changes), file("original-pattern.json", msg.pattern)];
        break;
      case "fold": {
        const model = new ftx.WebBody(new Uint8Array(msg.bytes));
        try {
          const fold = ftx.fold_up(model, msg.pattern, msg.measurements, msg.fit, msg.selfContactMm);
          try { reportText = fold.report(); result = json(reportText); viewer = fold.viewer(); }
          finally { fold.free(); }
        } finally { model.free(); }
        files = [file("sim_report.json", reportText), file("fold_viewer.html", viewer, "text/html")];
        break;
      }
      case "heat":
        raw = ftx.thermal_steady_surface(msg.report, msg.conditions); result = json(raw);
        files = [file("thermal-surface.json", raw)]; break;
      case "coverage":
        // Validate each complete JSON fragment, but preserve its number representation. A JS
        // parse/stringify round trip would invalidate the Rust material/geometry receipt hashes.
        for (const fragment of [msg.report, msg.map, msg.conditions]) json(fragment);
        raw = ftx.thermal_steady_coverage(`{"schema":"ftx-steady-coverage-request/1","report":${msg.report},"map":${msg.map},"conditions":${msg.conditions}}`);
        result = json(raw); files = [file("thermal-coverage.json", raw)]; break;
      case "cut":
        raw = ftx.cut_plan(msg.pattern, msg.options); result = json(raw);
        files = [file("cut-plan.json", raw)];
        // One bounded on-demand preview rather than quadratically regenerating every roll.
        if (result.rolls.length) {
          picture = ftx.cut_svg(msg.pattern, msg.options, 0);
          files.push(file("roll-0.svg", picture, "image/svg+xml"));
        }
        break;
      case "stock": {
        // Older bundles keep their existing single-product contract. Unknown optimizer options
        // still fail their native request parser; the worker never removes or rewrites a policy.
        if (ftx.stock_job === undefined) {
          raw = ftx.stock_plan(msg.pattern, msg.stockRequest); result = json(raw);
          const residual = ftx.stock_residual(msg.pattern, msg.stockRequest); json(residual);
          files = [file("finite-stock-plan.json", raw), file("residual-stock-request.json", residual)];
          const i = result.markers.findIndex(m => m.placements.length);
          if (i >= 0) {
            picture = ftx.stock_svg(msg.pattern, msg.stockRequest, i);
            files.push(file(`stock-${i}.svg`, picture, "image/svg+xml"));
          }
          break;
        }
        if (typeof ftx.stock_job !== "function") throw Error("Rebuild web/pkg: invalid single-run finite-stock engine.");
        const job = json(ftx.stock_job(msg.pattern, msg.stockRequest));
        if (job.schema !== "ftx-stock-job/1" || typeof job.plan_json !== "string" || typeof job.residual_json !== "string") {
          throw Error("Rebuild web/pkg: incomplete finite-stock job response.");
        }
        raw = job.plan_json; result = json(raw); json(job.residual_json);
        const i = job.stock_index;
        if (!Number.isInteger(i) || i < 0 || !result.markers?.[i]?.placements?.length) {
          throw Error("Finite-stock preview does not identify an allocated item.");
        }
        files = [file("finite-stock-plan.json", raw), file("residual-stock-request.json", job.residual_json)];
        if (typeof job.preview_svg === "string" && job.preview_refusal == null) {
          picture = job.preview_svg; files.push(file(`stock-${i}.svg`, picture, "image/svg+xml"));
        } else if (job.preview_svg == null && job.preview_refusal?.message) {
          files.push(file("stock-preview-refusal.json", job.preview_refusal));
        } else {
          throw Error("Finite-stock preview needs either its SVG or its explicit refusal.");
        }
        if (result.optimization) files.push(file("stock-optimization.json", result.optimization));
        if (result.purchasing != null) {
          if (typeof job.purchasing_json !== "string") throw Error("Rebuild web/pkg: missing source-preserving stock purchasing receipt.");
          json(job.purchasing_json);
          files.push(file("stock-purchasing.json", job.purchasing_json));
        } else if (job.purchasing_json != null) {
          throw Error("Unexpected purchasing receipt without a priced stock plan.");
        }
        break;
      }
      case "price":
        raw = ftx.cut_price(msg.pattern, msg.options, msg.quotes, msg.market); result = json(raw);
        files = [file("cut-price.json", raw)]; break;
      case "sew":
        raw = ftx.sew_stitches(msg.pattern, msg.options); result = json(raw);
        files = [file("sewing-program.json", raw)]; break;
      case "exports": {
        for (const fragment of [msg.pattern, msg.fit, msg.measurements]) json(fragment);
        const round = ftx.run(`{"pattern":${msg.pattern},"fit_spec":${msg.fit},"body":{"mode":"file","measurements":${msg.measurements}}}`, msg.today, msg.created);
        try {
          result = json(round.page());
          if (result.errors || result.status === "BLOCKED") throw Error(JSON.stringify(result.errors || result));
          files = [{ name: round.zip_name(), bytes: round.zip(), mime: "application/zip" }];
        } finally { round.free(); }
        break;
      }
      default: throw Error("Unknown material workbench operation.");
    }
    if (msg.type === "stock") {
      try { files.push(file("stock-review.html", stockReview(result), "text/html")); }
      catch (e) { files.push(file("stock-review-refusal.json", { message: String(e.message || e) })); }
    }
    postMessage({ type: "result", request: msg.request, op: msg.type, result, files, viewer, picture, reportText },
      files.filter(f => f.bytes).map(f => f.bytes.buffer));
  } catch (e) {
    const fatal = e instanceof WebAssembly.RuntimeError;
    if (fatal) failure = e;
    postMessage({ type: "error", request: msg.request, op: msg.type, fatal, message: String(e.message || e) });
  } finally { active = null; }
};
