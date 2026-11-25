// FrankenTextiles in a worker: drafting and exports take a second or two, a fold-up half a minute or more,
// and neither may freeze the page. It answers the page's messages (init, fields, run, body, fold) as the
// Pyodide worker running webapp.py answered the first three.
let ftx = null;
let body = null; // the parsed body model, kept for every fold-up
let docs = null; // the last round's pattern, measurements and fit spec (JSON text)
let bodyRound = null; // body checking also works when the garment has no fold-up placement
let alterationRound = null; // exact successfully checked source; an operation never edits it in place
let bodyRequest, foldRequest;

const pad = (n) => String(n).padStart(2, "0");
function clock() {
  // the wearer's local date (tape files, staleness) and minute (export stamps), as webapp.py reads them
  const d = new Date();
  const today = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  return { today, created: `${today}T${pad(d.getHours())}:${pad(d.getMinutes())}` };
}

// the drape's progress lines, called from inside the module while it works
self.ftxProgress = (line) => postMessage({ type: "foldProgress", request: foldRequest, text: line });

const text = (bytes) => (bytes ? new TextDecoder().decode(bytes) : null);
// Python truthiness, as webapp.py's has_fold_up reads a placement: {} and [] are false
const truthy = (v) => !(v === null || v === undefined || v === false || v === 0 || v === ""
  || (Array.isArray(v) && v.length === 0) || (typeof v === "object" && !Array.isArray(v) && Object.keys(v).length === 0));
function checkedArmPose(value) {
  const keys = value && typeof value === "object" && !Array.isArray(value) ? Object.keys(value) : null;
  if (!keys || keys.length !== 2 || !Object.hasOwn(value, "left_abduction_deg") || !Object.hasOwn(value, "right_abduction_deg") ||
      [value.left_abduction_deg, value.right_abduction_deg].some(angle => !Number.isFinite(angle) || angle < 0 || angle > 180)) {
    throw Error("Independent arm pose needs only finite left_abduction_deg and right_abduction_deg from 0 to 180 degrees.");
  }
  return { left_abduction_deg: value.left_abduction_deg, right_abduction_deg: value.right_abduction_deg };
}
function checkedThumbPose(value) {
  const keys = value && typeof value === "object" && !Array.isArray(value) ? Object.keys(value) : null;
  if (!keys || keys.length !== 2 || !Object.hasOwn(value, "left_rotation_vector_rad") || !Object.hasOwn(value, "right_rotation_vector_rad")) {
    throw new Error("Thumb pose must be an object with only left_rotation_vector_rad and right_rotation_vector_rad.");
  }
  const out = {};
  for (const key of ["left_rotation_vector_rad", "right_rotation_vector_rad"]) {
    const vec = value[key];
    if (!Array.isArray(vec) || vec.length !== 3 || vec.some(v => typeof v !== "number" || !Number.isFinite(v))) {
      throw new Error("Each thumb rotation vector must contain 3 finite radian numbers.");
    }
    const scale = Math.max(Math.abs(vec[0]), Math.abs(vec[1]), Math.abs(vec[2]));
    const angle = scale === 0 ? 0 : scale * Math.sqrt((vec[0] / scale) ** 2 + (vec[1] / scale) ** 2 + (vec[2] / scale) ** 2);
    if (angle > Math.PI) {
      throw new Error(`Thumb rotation magnitude ${angle} exceeds π radians.`);
    }
    out[key] = [vec[0], vec[1], vec[2]];
  }
  return out;
}
let dead = false; // set when the module traps: its memory and handles can no longer be trusted

onmessage = async (ev) => {
  const msg = ev.data;
  if (dead) {
    return postMessage({ type: "error", op: msg.type, request: msg.request, fatal: true, message: "the module stopped after an internal error: reload the page" });
  }
  try {
    if (msg.type === "init") {
      postMessage({ type: "progress", text: "Loading FrankenTextiles…" });
      // Load bindings and module from the same numerical-engine revision.
      const revision = "?v=2026-11-24.5";
      ftx = await import(msg.pkg + "ftx_wasm.js" + revision);
      await ftx.default({ module_or_path: msg.pkg + "ftx_wasm_bg.wasm" + revision });
      if ((typeof ftx.initSync === "function" || "wasm_interface_stamp" in ftx) &&
          (typeof ftx.wasm_interface_stamp !== "function" || ftx.wasm_interface_stamp() !== "ftx-wasm-interface/2026-11-24.5")) {
        throw new Error("Stale web/pkg bundle: rebuild web/pkg via ./tools/build_web.sh");
      }
      const info = JSON.parse(ftx.program_info());
      return postMessage({ type: "ready", info, sheetDictionaries: JSON.parse(ftx.calibration_dictionaries()),
        dxf: "every export built in", version: "FrankenTextiles, WebAssembly" });
    }
    if (msg.type === "fields") {
      const rows = JSON.parse(ftx.form_fields(JSON.stringify(msg.args), clock().today));
      return postMessage({ type: "fields", request: msg.request, rows, program: msg.args.program });
    }
    if (msg.type === "run") {
      docs = null; // a refused replacement must never leave the previous garment available
      bodyRound = null;
      alterationRound = null;
      const { today, created } = clock();
      const requestText = typeof msg.req === "string" ? msg.req : JSON.stringify(msg.req);
      let round;
      if (msg.draftOnBody === true) {
        if (!body || msg.body !== bodyRequest) throw Error("Load the current body model before using it for pattern dimensions or flat checks.");
        if (msg.bodyFit != null && (typeof msg.bodyFit !== "string" || !msg.bodyFit.length || msg.bodyFit.length > 16 * 1024 * 1024)) {
          throw Error("An accepted body-fit report must be nonempty JSON text of at most 16 MiB.");
        }
        if (msg.bodyFit != null && msg.bodyFitConsent !== true) throw Error("Associate the accepted fit with this wearer before using it.");
        if (typeof ftx.run_on_selected_body !== "function") throw Error("Rebuild web/pkg via ./tools/build_web.sh to draft or check patterns on a fitted body.");
        round = ftx.run_on_selected_body(body, requestText, today, created, msg.bodyFit ?? undefined);
      } else {
        round = ftx.run(requestText, today, created);
      }
      try {
        const res = JSON.parse(round.page());
        const blobs = {};
        if (res.zip) blobs.zip = round.zip();
        for (const f of res.files || []) blobs[f.name] = round.file(f.name);
        const pattern = text(round.file("pattern.json"));
        const data = pattern && !res.errors ? JSON.parse(pattern) : null;
        // a fold-up needs a pattern that validates, is to scale and has a piece with a fold-up placement
        const foldable = !!data && res.status !== "BLOCKED" && (data.meta || {}).scale_status !== "UNSCALED"
          && (data.pieces || []).some((p) => truthy(p.placement) && p.sim !== false);
        const measurements = text(round.file("measurements.json"));
        const fit = text(round.file("fit_spec.json"));
        if (foldable) {
          docs = { request: msg.request, pattern, measurements, fit };
        }
        if (!res.errors && measurements) bodyRound = { request: msg.request, measurements };
        // These native saved texts outlive the round handle without a JavaScript numeric round-trip.
        const roundSources = res.zip && !res.errors && pattern && measurements && fit
          ? { pattern, measurements, fit } : null;
        let alterationSources = null, alterationUnavailable = null;
        if (data && fit) {
          try {
            if (typeof ftx.alteration_targets !== "function") throw Error("Rebuild the browser bundle to use direct alterations.");
            const targets = JSON.parse(ftx.alteration_targets(pattern));
            alterationRound = { request: msg.request, pattern, fit, targets };
            alterationSources = { pattern, fit, targets };
          } catch (e) {
            if (e instanceof WebAssembly.RuntimeError) throw e;
            alterationUnavailable = String(e.message || e);
          }
        }
        return postMessage({ type: "result", request: msg.request, res, blobs, foldable,
          bodyCheckable: !!bodyRound, roundSources, alterationSources, alterationUnavailable }, Object.values(blobs).map((b) => b.buffer));
      } finally { round.free(); }
    }
    if (msg.type === "alterSource") {
      if (typeof msg.source !== "string") throw Error("Choose an original source pattern JSON file.");
      const targets = JSON.parse(ftx.alteration_targets(msg.source));
      return postMessage({ type: "alterSourceReady", request: msg.request, targets });
    }
    if (msg.type === "alter") {
      if (!alterationRound || msg.round !== alterationRound.request) throw Error("The alteration source changed; run a current round first.");
      if (typeof msg.operation !== "string") throw Error("The alteration must be an explicit operation JSON.");
      docs = null; bodyRound = null;
      const current = alterationRound;
      const result = JSON.parse(ftx.alter_pattern(current.pattern, msg.operation, msg.source));
      if (typeof result.pattern_json !== "string" || typeof result.why !== "string" || !result.targets) {
        throw Error("Incomplete alteration result; the source pattern was retained.");
      }
      return postMessage({ type: "altered", request: msg.request, round: current.request,
        pattern: result.pattern_json, fit: current.fit, why: result.why, targets: result.targets });
    }
    if (msg.type === "clearBody") {
      if (body) body.free();
      body = null; bodyRequest = undefined;
      return;
    }
    if (msg.type === "body") {
      if (body) body.free();
      body = null; bodyRequest = msg.request;
      body = new ftx.WebBody(new Uint8Array(msg.bytes));
      return postMessage({ type: "bodyReady", request: msg.request });
    }
    if (msg.type === "inspectBody") {
      if (!body || !bodyRound) throw new Error("load the body model and run a round first");
      if (msg.round !== bodyRound.request || msg.body !== bodyRequest) throw new Error("the round or body changed: check the current sources");
      if (msg.bodyFit != null && (typeof msg.bodyFit !== "string" || msg.bodyFit.length > 16 * 1024 * 1024)) {
        throw new Error("accepted body-fit report must be JSON text of at most 16 MiB");
      }
      if (msg.bodyFit != null && msg.bodyFitConsent !== true) throw new Error("associate the accepted fit with this wearer before checking");
      const reportText = ftx.inspect_body(body, bodyRound.measurements, msg.bodyFit ?? undefined);
      return postMessage({ type: "bodyChecked", request: msg.request, reportText,
        report: JSON.parse(reportText), bodyFitText: msg.bodyFit ?? null });
    }
    if (msg.type === "fold") {
      if (!body || !docs) throw new Error("load the body model and run a round first");
      if (msg.round !== docs.request || msg.body !== bodyRequest) throw new Error("the garment or body changed: run the current round and load its body first");
      foldRequest = msg.request;
      if (msg.bodyFit != null && (typeof msg.bodyFit !== "string" || msg.bodyFit.length > 16 * 1024 * 1024)) {
        throw new Error("accepted body-fit report must be JSON text of at most 16 MiB");
      }
      const imported = msg.bodyFit != null;
      if (imported && msg.bodyFitConsent !== true) throw new Error("associate the accepted fit with this wearer before folding");
      const armDeg = !imported && msg.armDeg === undefined ? 35 : msg.armDeg;
      if (!Number.isFinite(armDeg) || armDeg < 0 || armDeg > 180) {
        throw new Error("Supply a finite drape arm angle from 0 to 180 degrees.");
      }
      const skinning = msg.skinning ?? "lbs";
      if (!["lbs", "dqs"].includes(skinning)) {
        throw new Error("Choose lbs or dqs skinning.");
      }
      const handCollider = msg.handCollider ?? "wrist";
      if (!["wrist", "palm", "knuckle", "full"].includes(handCollider)) {
        throw new Error("Choose wrist, palm, knuckle or full hand collider.");
      }
      const settings = { self_contact_mm: msg.selfContactMm ?? 2, two_start: !!msg.twoStart,
        arm_abduction_deg: armDeg, legs: msg.legs ?? "standing",
        leg_fraction: msg.legFraction ?? 1, max_settle_steps: msg.maxSettleSteps ?? 0,
        ...(skinning !== "lbs" ? { skinning } : {}),
        ...(handCollider !== "wrist" ? { hand_collider: handCollider } : {}),
        ...(msg.thumbPose !== undefined ? { thumb_pose: checkedThumbPose(msg.thumbPose) } : {}),
        ...(msg.armPose !== undefined ? { arm_pose: checkedArmPose(msg.armPose) } : {}) };
      if (!["standing", "seated", "squat"].includes(settings.legs) ||
          !Number.isFinite(settings.leg_fraction) || settings.leg_fraction < 0 || settings.leg_fraction > 1) {
        throw new Error("Choose standing, seated or squat legs and a finite leg fraction from 0 to 1.");
      }
      if (!Number.isSafeInteger(settings.max_settle_steps) || settings.max_settle_steps < 0 || settings.max_settle_steps > 256) {
        throw new Error("Maximum extra settling steps must be a whole number from 0 to 256.");
      }
      if (msg.timeSubdivision !== undefined) {
        const value = msg.timeSubdivision;
        if (!value || typeof value !== "object" || Array.isArray(value) ||
            Object.keys(value).length !== 2 || Object.keys(value).some(key => !["max_halvings", "max_trials"].includes(key)) ||
            !Number.isSafeInteger(value.max_halvings) || value.max_halvings < 0 || value.max_halvings > 16 ||
            !Number.isSafeInteger(value.max_trials) || value.max_trials < 1 || value.max_trials > 65535) {
          throw Error("Time subdivision needs only max_halvings (whole 0–16) and max_trials (whole 1–65535).");
        }
        settings.time_subdivision = { max_halvings: value.max_halvings, max_trials: value.max_trials };
      }
      if (msg.newtonStrategy !== undefined) {
        const value = msg.newtonStrategy;
        if (!value || typeof value !== "object" || Array.isArray(value)) {
          throw Error("Newton strategy must be a valid tagged solver object.");
        }
        const keys = Object.keys(value);
        if (value.kind === "projected_newton" && keys.length === 1) {
          // Legacy default; omit from FoldSettings JSON.
        } else if (value.kind === "adaptive_ilu" && keys.length === 3 &&
            keys.every(key => ["kind", "pcg_iters", "max_shifts"].includes(key)) &&
            Number.isSafeInteger(value.pcg_iters) && value.pcg_iters >= 1 && value.pcg_iters <= 256 &&
            Number.isSafeInteger(value.max_shifts) && value.max_shifts >= 0 && value.max_shifts <= 16) {
          settings.newton_strategy = { kind: "adaptive_ilu", pcg_iters: value.pcg_iters, max_shifts: value.max_shifts };
        } else if (value.kind === "trust_region_steihaug" && keys.length === 5 &&
            keys.every(key => ["kind", "initial_radius", "max_radius", "min_radius", "cg_iters"].includes(key)) &&
            Number.isFinite(value.min_radius) && Number.isFinite(value.initial_radius) && Number.isFinite(value.max_radius) &&
            value.min_radius > 0 && value.initial_radius >= value.min_radius && value.max_radius >= value.initial_radius &&
            Number.isSafeInteger(value.cg_iters) && value.cg_iters >= 0 && value.cg_iters <= 4096) {
          settings.newton_strategy = { kind: "trust_region_steihaug", initial_radius: value.initial_radius,
            max_radius: value.max_radius, min_radius: value.min_radius, cg_iters: value.cg_iters };
        } else {
          throw Error("Newton strategy must be projected_newton, adaptive_ilu, or trust_region_steihaug with valid bounds.");
        }
      }
      if (msg.integrator !== undefined) {
        const value = msg.integrator;
        if (!value || typeof value !== "object" || Array.isArray(value)) {
          throw Error("Time integrator must be a valid tagged object.");
        }
        const keys = Object.keys(value);
        if (value.kind === "implicit_euler" && keys.length === 1) {
          // Legacy default; omit from FoldSettings JSON.
        } else if (value.kind === "generalized_alpha" && keys.length === 2 &&
            keys.every(key => ["kind", "rho_inf"].includes(key)) &&
            Number.isFinite(value.rho_inf) && value.rho_inf >= 0 && value.rho_inf <= 1) {
          settings.integrator = { kind: "generalized_alpha", rho_inf: value.rho_inf };
        } else {
          throw Error("Time integrator must be implicit_euler or generalized_alpha with finite rho_inf in [0, 1].");
        }
      }
      if (msg.schedule !== undefined) {
        const value = msg.schedule;
        const allowed = ["step_s", "sew_steps", "drape_steps", "newton_tol_m_s"];
        if (!value || typeof value !== "object" || Array.isArray(value) ||
            !Object.keys(value).length || Object.keys(value).some(key => !allowed.includes(key) || value[key] === undefined)) {
          throw Error("Custom integration schedule must specify only step_s, sew_steps, drape_steps, or newton_tol_m_s.");
        }
        if (value.step_s !== undefined) {
          if (typeof value.step_s !== "number" || !Number.isFinite(value.step_s) || value.step_s <= 0) {
            throw Error("Custom time step step_s must be finite and positive.");
          }
          settings.step_s = value.step_s;
        }
        if (value.sew_steps !== undefined) {
          if (!Number.isSafeInteger(value.sew_steps) || value.sew_steps < 0 || value.sew_steps > 512) {
            throw Error("Custom sewing step count must be a whole number from 0 to 512.");
          }
          settings.sew_steps = value.sew_steps;
        }
        if (value.drape_steps !== undefined) {
          if (!Number.isSafeInteger(value.drape_steps) || value.drape_steps < 0 || value.drape_steps > 512) {
            throw Error("Custom gravity step count must be a whole number from 0 to 512.");
          }
          settings.drape_steps = value.drape_steps;
        }
        if (value.newton_tol_m_s !== undefined) {
          if (typeof value.newton_tol_m_s !== "number" || !Number.isFinite(value.newton_tol_m_s) || value.newton_tol_m_s <= 0) {
            throw Error("Custom Newton tolerance newton_tol_m_s must be finite and positive.");
          }
          settings.newton_tol_m_s = value.newton_tol_m_s;
        }
        const effSew = settings.sew_steps ?? 12;
        const effDrape = settings.drape_steps ?? 36;
        if (effSew + effDrape < 6) {
          throw Error("Custom schedule needs at least 6 combined sewing and gravity steps.");
        }
        const longestSew = settings.two_start ? Math.max(effSew, 18) : effSew;
        if (longestSew + effDrape + settings.max_settle_steps > 512) {
          throw Error("Configured fold exceeds the 512-step browser work budget per sewing path (sew + drape + settle).");
        }
      }
      if (msg.staticEquilibrium !== undefined) {
        const value = msg.staticEquilibrium;
        if (!value || typeof value !== "object" || Array.isArray(value) ||
            Object.keys(value).length !== 2 ||
            Object.keys(value).some(key => !["correction_tolerance_m", "max_newton_iterations"].includes(key)) ||
            typeof value.correction_tolerance_m !== "number" || !Number.isFinite(value.correction_tolerance_m) ||
            value.correction_tolerance_m <= 0 || !Number.isFinite(value.correction_tolerance_m * 1e3) ||
            !Number.isSafeInteger(value.max_newton_iterations) || value.max_newton_iterations < 1 || value.max_newton_iterations > 1000) {
          throw Error("Static equilibrium needs finite positive correction_tolerance_m and max_newton_iterations (whole 1–1000).");
        }
        settings.static_equilibrium = {
          correction_tolerance_m: value.correction_tolerance_m,
          max_newton_iterations: value.max_newton_iterations,
        };
      }
      if (msg.bendingModel !== undefined) {
        if (msg.bendingModel === "quadratic") {
          // Legacy default; omit from FoldSettings JSON.
        } else if (msg.bendingModel === "discrete_shells") {
          settings.bending_model = "discrete_shells";
        } else {
          throw Error("Bending model must be quadratic or discrete_shells.");
        }
      }
      if (msg.compressionModel !== undefined) {
        if (msg.compressionModel === "axis_aligned") {
          // Legacy default; omit from FoldSettings JSON.
        } else if (msg.compressionModel === "principal") {
          settings.compression_model = "principal";
        } else {
          throw Error("Compression model must be axis_aligned or principal.");
        }
      }
      if (msg.damping !== undefined) {
        const value = msg.damping;
        const allowed = ["air_drag", "rayleigh_membrane_s", "rayleigh_bending_s"];
        if (!value || typeof value !== "object" || Array.isArray(value) ||
            Object.keys(value).some(key => !allowed.includes(key))) {
          throw Error("Damping options must contain only air_drag, rayleigh_membrane_s, or rayleigh_bending_s.");
        }
        const out = {};
        if (value.air_drag !== undefined) {
          const d = value.air_drag;
          const dReq = ["rho_air_kg_m3", "c_normal", "c_tangent", "eps_v_m_s"];
          const dKeys = [...dReq, "air_velocity_m_s"];
          if (!d || typeof d !== "object" || Array.isArray(d) ||
              Object.keys(d).some(k => !dKeys.includes(k)) ||
              dReq.some(k => !Object.hasOwn(d, k)) ||
              typeof d.rho_air_kg_m3 !== "number" || !Number.isFinite(d.rho_air_kg_m3) || d.rho_air_kg_m3 < 0 ||
              typeof d.c_normal !== "number" || !Number.isFinite(d.c_normal) || d.c_normal < 0 ||
              typeof d.c_tangent !== "number" || !Number.isFinite(d.c_tangent) || d.c_tangent < 0 ||
              typeof d.eps_v_m_s !== "number" || !Number.isFinite(d.eps_v_m_s) || d.eps_v_m_s <= 0) {
            throw Error("Air drag requires finite nonnegative rho_air_kg_m3, c_normal, c_tangent and positive eps_v_m_s.");
          }
          const dragOut = { rho_air_kg_m3: d.rho_air_kg_m3, c_normal: d.c_normal, c_tangent: d.c_tangent, eps_v_m_s: d.eps_v_m_s };
          if (Object.hasOwn(d, "air_velocity_m_s")) {
            const w = d.air_velocity_m_s;
            if (!Array.isArray(w) || w.length !== 3 || w.some(c => typeof c !== "number" || !Number.isFinite(c))) {
              throw Error("Air drag air_velocity_m_s must be a 3-element array of finite numbers in m/s.");
            }
            const w2 = w[0] * w[0] + w[1] * w[1] + w[2] * w[2];
            const w3 = w2 * Math.sqrt(w2);
            if (!Number.isFinite(w2) || !Number.isFinite(w3) || !Number.isFinite(d.rho_air_kg_m3 * (d.c_normal + d.c_tangent) * w3)) {
              throw Error("Air drag air_velocity_m_s overflows aerodynamic drag arithmetic.");
            }
            if (w[0] !== 0 || w[1] !== 0 || w[2] !== 0) {
              dragOut.air_velocity_m_s = [w[0] === 0 ? 0 : w[0], w[1] === 0 ? 0 : w[1], w[2] === 0 ? 0 : w[2]];
            }
          }
          out.air_drag = dragOut;
        }
        for (const key of ["rayleigh_membrane_s", "rayleigh_bending_s"]) {
          if (value[key] !== undefined) {
            if (typeof value[key] !== "number" || !Number.isFinite(value[key]) || value[key] < 0) {
              throw Error(`${key} must be a finite nonnegative duration in seconds.`);
            }
            if (value[key] > 0) out[key] = value[key];
          }
        }
        if (Object.keys(out).length > 0) settings.damping = out;
      }
      if (msg.adaptiveBarrier !== undefined) {
        const b = msg.adaptiveBarrier;
        const bKeys = ["target_gap_fraction", "min_gap_fraction", "growth_factor", "membrane_floor_scale", "kappa_min", "kappa_max"];
        if (!b || typeof b !== "object" || Array.isArray(b) ||
            Object.keys(b).length !== 6 || Object.keys(b).some(k => !bKeys.includes(k)) ||
            bKeys.some(k => typeof b[k] !== "number" || !Number.isFinite(b[k])) ||
            !(b.target_gap_fraction > 0 && b.target_gap_fraction < 1) ||
            !(b.min_gap_fraction > 0 && b.min_gap_fraction <= b.target_gap_fraction) ||
            !(b.growth_factor >= 1 && b.growth_factor <= 16) ||
            !(b.membrane_floor_scale >= 0 && b.membrane_floor_scale <= 1) ||
            b.kappa_min <= 0 || b.kappa_max < b.kappa_min) {
          throw Error("Adaptive barrier policy parameters are out of range.");
        }
        settings.adaptive_barrier = {
          target_gap_fraction: b.target_gap_fraction, min_gap_fraction: b.min_gap_fraction,
          growth_factor: b.growth_factor, membrane_floor_scale: b.membrane_floor_scale,
          kappa_min: b.kappa_min, kappa_max: b.kappa_max,
        };
      }
      for (const [msgKey, wireKey] of [["stickSlip", "stick_slip"], ["selfStickSlip", "self_stick_slip"]]) {
        if (msg[msgKey] !== undefined) {
          const s = msg[msgKey];
          const sKeys = ["mu_s", "mu_k", "transition_v_m_s"];
          if (!s || typeof s !== "object" || Array.isArray(s) ||
              Object.keys(s).length !== 3 || Object.keys(s).some(k => !sKeys.includes(k)) ||
              sKeys.some(k => typeof s[k] !== "number" || !Number.isFinite(s[k])) ||
              s.mu_k < 0 || s.mu_s < s.mu_k || s.transition_v_m_s <= 0) {
            throw Error("Stick-slip friction requires 0 <= mu_k <= mu_s and positive transition_v_m_s.");
          }
          if (wireKey === "self_stick_slip" && !(settings.self_contact_mm > 0)) {
            throw Error("Cloth-cloth self stick-slip friction requires self-contact to be enabled.");
          }
          settings[wireKey] = { mu_s: s.mu_s, mu_k: s.mu_k, transition_v_m_s: s.transition_v_m_s };
        }
      }
      if (msg.mesh !== undefined) {
        const m = msg.mesh;
        const mKeys = ["min_spacing_mm", "cvt_iterations", "max_turning_angle_rad", "gather_refinement"];
        if (!m || typeof m !== "object" || Array.isArray(m) ||
            Object.keys(m).some(k => !mKeys.includes(k))) {
          throw Error("Mesh options contain an unknown field.");
        }
        const out = {};
        if (m.min_spacing_mm !== undefined) {
          if (typeof m.min_spacing_mm !== "number" || !Number.isFinite(m.min_spacing_mm) || m.min_spacing_mm <= 0 || m.min_spacing_mm > 12 || 12 / m.min_spacing_mm > 8) {
            throw Error("Adaptive mesh min_spacing_mm must be in [1.5, 12] mm.");
          }
          out.min_spacing_mm = m.min_spacing_mm;
        }
        if (m.cvt_iterations !== undefined) {
          if (!Number.isSafeInteger(m.cvt_iterations) || m.cvt_iterations < 0 || m.cvt_iterations > 32) {
            throw Error("Mesh cvt_iterations must be a whole number in 0–32.");
          }
          if (m.cvt_iterations > 0) out.cvt_iterations = m.cvt_iterations;
        }
        if (m.max_turning_angle_rad !== undefined) {
          if (typeof m.max_turning_angle_rad !== "number" || !Number.isFinite(m.max_turning_angle_rad) || m.max_turning_angle_rad < 0.05 || m.max_turning_angle_rad > Math.PI) {
            throw Error("Mesh max_turning_angle_rad must be in [0.05, π].");
          }
          if (out.min_spacing_mm === undefined && m.max_turning_angle_rad !== 0.12) {
            throw Error("Mesh max_turning_angle_rad requires min_spacing_mm.");
          }
          if (m.max_turning_angle_rad !== 0.12) out.max_turning_angle_rad = m.max_turning_angle_rad;
        }
        if (m.gather_refinement !== undefined) {
          if (typeof m.gather_refinement !== "number" || !Number.isFinite(m.gather_refinement) || m.gather_refinement < 1 || m.gather_refinement > 8) {
            throw Error("Mesh gather_refinement must be in [1, 8].");
          }
          if (out.min_spacing_mm === undefined && m.gather_refinement !== 2) {
            throw Error("Mesh gather_refinement requires min_spacing_mm.");
          }
          if (m.gather_refinement !== 2) out.gather_refinement = m.gather_refinement;
        }
        if (Object.keys(out).length > 0) settings.mesh = out;
      }
      if (msg.construction !== undefined) {
        const c = msg.construction;
        const cKeys = ["hem_width_mm", "hem_mass_multiplier", "hem_bending_multiplier", "seam_width_mm", "seam_mass_multiplier", "seam_bending_multiplier"];
        if (!c || typeof c !== "object" || Array.isArray(c) ||
            Object.keys(c).length !== 6 || Object.keys(c).some(k => !cKeys.includes(k)) ||
            cKeys.some(k => typeof c[k] !== "number" || !Number.isFinite(c[k])) ||
            c.hem_width_mm < 0 || c.hem_width_mm > 200 || c.seam_width_mm < 0 || c.seam_width_mm > 100 ||
            c.hem_mass_multiplier < 1 || c.hem_mass_multiplier > 10 || c.seam_mass_multiplier < 1 || c.seam_mass_multiplier > 10 ||
            c.hem_bending_multiplier < 1 || c.hem_bending_multiplier > 32 || c.seam_bending_multiplier < 1 || c.seam_bending_multiplier > 32 ||
            ((c.hem_mass_multiplier > 1 || c.hem_bending_multiplier > 1) && c.hem_width_mm <= 0) ||
            (c.hem_width_mm > 0 && c.hem_mass_multiplier === 1 && c.hem_bending_multiplier === 1) ||
            (c.seam_mass_multiplier > 1 && c.seam_width_mm <= 0) ||
            (c.seam_width_mm > 0 && c.seam_mass_multiplier === 1 && c.seam_bending_multiplier === 1)) {
          throw Error("Construction mechanics widths or multipliers are out of range.");
        }
        if (c.hem_mass_multiplier > 1 || c.hem_bending_multiplier > 1 || c.seam_mass_multiplier > 1 || c.seam_bending_multiplier > 1) {
          settings.construction = {
            hem_width_mm: c.hem_width_mm, hem_mass_multiplier: c.hem_mass_multiplier, hem_bending_multiplier: c.hem_bending_multiplier,
            seam_width_mm: c.seam_width_mm, seam_mass_multiplier: c.seam_mass_multiplier, seam_bending_multiplier: c.seam_bending_multiplier,
          };
        }
      }
      if (msg.startSelfRepulsion !== undefined) {
        const r = msg.startSelfRepulsion;
        const rKeys = ["min_separation_m", "stiffness", "period", "max_candidate_checks"];
        if (!r || typeof r !== "object" || Array.isArray(r) ||
            Object.keys(r).some(k => !rKeys.includes(k)) ||
            typeof r.min_separation_m !== "number" || !Number.isFinite(r.min_separation_m) || r.min_separation_m <= 0 || r.min_separation_m > 0.1 ||
            typeof r.stiffness !== "number" || !Number.isFinite(r.stiffness) || r.stiffness <= 0 || r.stiffness > 1 ||
            !Number.isSafeInteger(r.period) || r.period < 1 || r.period > 25) {
          throw Error("Start self-repulsion requires min_separation_m in (0, 0.1] m, stiffness in (0, 1], and period in 1–25.");
        }
        const out = { min_separation_m: r.min_separation_m, stiffness: r.stiffness, period: r.period };
        if (Object.hasOwn(r, "max_candidate_checks")) {
          if (!Number.isSafeInteger(r.max_candidate_checks) || r.max_candidate_checks < 1 || r.max_candidate_checks > 128000000) {
            throw Error("Start self-repulsion max_candidate_checks must be an integer in 1–128000000.");
          }
          if (r.max_candidate_checks !== 32000000) out.max_candidate_checks = r.max_candidate_checks;
        }
        settings.start_self_repulsion = out;
      }
      const configured = (!imported && armDeg !== 35) ||
        settings.legs !== "standing" || settings.leg_fraction !== 1 || skinning !== "lbs" ||
        handCollider !== "wrist" || settings.thumb_pose !== undefined || settings.arm_pose !== undefined ||
        settings.max_settle_steps !== 0 || settings.time_subdivision !== undefined ||
        settings.newton_strategy !== undefined || settings.integrator !== undefined ||
        settings.step_s !== undefined || settings.sew_steps !== undefined ||
        settings.drape_steps !== undefined || settings.newton_tol_m_s !== undefined ||
        settings.static_equilibrium !== undefined ||
        settings.bending_model !== undefined || settings.compression_model !== undefined ||
        settings.damping !== undefined || settings.adaptive_barrier !== undefined ||
        settings.stick_slip !== undefined || settings.self_stick_slip !== undefined ||
        settings.mesh !== undefined || settings.construction !== undefined ||
        settings.start_self_repulsion !== undefined;
      if (configured && typeof ftx.fold_up_configured !== "function") {
        throw new Error("Rebuild web/pkg: missing configured pose and settling drape engine.");
      }
      const fold = configured
        ? ftx.fold_up_configured(body, docs.pattern, docs.measurements, docs.fit, JSON.stringify(settings), imported ? msg.bodyFit : undefined)
        : imported
        ? ftx.fold_up_with_body_fit(body, docs.pattern, docs.measurements, docs.fit, settings.self_contact_mm, !!msg.twoStart, msg.bodyFit, msg.armDeg)
        : ftx.fold_up(body, docs.pattern, docs.measurements, docs.fit, settings.self_contact_mm, !!msg.twoStart);
      try {
        const reportText = fold.report(); // preserve numerical JSON for source-locked thermal replay
        const report = JSON.parse(reportText);
        const viewer = fold.viewer();
        return postMessage({ type: "folded", request: msg.request, report, reportText, viewer,
          settingsText: configured ? JSON.stringify(settings) : null,
          bodyFitText: imported ? msg.bodyFit : null });
      } finally { fold.free(); }
    }
  } catch (e) {
    const full = String((e && e.message) || e).trim();
    // a trap (a Rust panic: "unreachable") leaves the module's handles borrowed and its memory suspect
    const fatal = e instanceof WebAssembly.RuntimeError;
    if (fatal) dead = true;
    postMessage({
      type: "error", op: msg.type, request: msg.request, fatal,
      message: fatal ? "the module stopped after an internal error: reload the page" : full.split("\n").pop(),
      detail: full,
    });
  }
};
