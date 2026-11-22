// Run live Rust/WASM `material_calibrate_friction` verification for the synthetic friction-sled
// exhibit and verify native sled report invariants (ftx-13b).
//
// Invariants:
// - Every synthetic state's kinetic coefficient mu is verified live via `ftx.material_calibrate_friction`.
// - Setting `allowEstimates = false` produces an explicit interface-consent refusal, never silent admission.
// - Missing source-qualified coefficients on real catalog textiles remain strictly unavailable.
import { getStudioEngine } from "./studio-engine.mjs";

export function verifySledHistory(history) {
  if (!history || !Array.isArray(history.samples) || history.samples.length < 5) {
    throw new Error("Sled history is missing recorded samples.");
  }
  const { units, normal_load_n: normalLoad, mu, coulomb_threshold_n: threshold, pull_direction: dir } = history;
  if (units?.force !== "N" || units?.displacement !== "mm" || units?.velocity !== "mm/s" || units?.time !== "s" || units?.mu !== "1") {
    throw new Error(`Unexpected units in sled history ${history.id}`);
  }
  const expectedThreshold = mu * normalLoad;
  if (Math.abs(threshold - expectedThreshold) > 1e-4) {
    throw new Error(`Threshold mismatch in ${history.id}: ${threshold} vs ${expectedThreshold}`);
  }
  const dirNorm = Math.hypot(dir[0], dir[1]);
  if (Math.abs(dirNorm - 1) > 1e-5) {
    throw new Error(`Pull direction must be a unit vector in ${history.id}`);
  }

  let stickCount = 0;
  let slipCount = 0;
  for (const s of history.samples) {
    if (!s.converged) throw new Error(`Unconverged sample at step ${s.step} in ${history.id}`);
    if (!(s.min_gap_mm > 0)) throw new Error(`Non-positive contact gap at step ${s.step} in ${history.id}`);
    const normalErr = Math.abs(s.retained_normal_n - normalLoad);
    if (normalErr > Math.max(0.05, normalLoad * 0.02)) {
      throw new Error(`Normal reaction drift at step ${s.step} in ${history.id}: ${s.retained_normal_n} vs ${normalLoad}`);
    }
    if (s.regime === "stick") stickCount++;
    else if (s.regime === "slip") slipCount++;
    else throw new Error(`Unknown regime ${s.regime}`);
  }
  if (stickCount === 0 || slipCount === 0) {
    throw new Error(`History ${history.id} must span both stick and slip regimes (got stick=${stickCount}, slip=${slipCount})`);
  }

  const last = history.samples[history.samples.length - 1];
  const dispMag = Math.hypot(last.sled_center_mm[0], last.sled_center_mm[1]);
  if (!(dispMag > 1.0)) {
    throw new Error(`Final slip displacement too small in ${history.id}: ${dispMag} mm`);
  }
  const cosAlign = (last.sled_center_mm[0] * dir[0] + last.sled_center_mm[1] * dir[1]) / dispMag;
  if (cosAlign < 0.995) {
    throw new Error(`Motion direction misaligned in ${history.id}: cos=${cosAlign}`);
  }
  if (Math.abs(last.tangential_reaction_along_n - threshold) > threshold * 0.05) {
    throw new Error(
      `Slip tangential reaction ${last.tangential_reaction_along_n} N deviates from threshold ${threshold} N in ${history.id}`,
    );
  }
  return {
    id: history.id,
    stickCount,
    slipCount,
    finalDisplacementMm: Number(last.displacement_mm.toFixed(2)),
    cosAlign: Number(cosAlign.toFixed(6)),
    thresholdN: threshold,
  };
}

export function calibrateFrictionState(ftx, calibrationInput, allowEstimates = true) {
  const raw = typeof calibrationInput === "string" ? calibrationInput : JSON.stringify(calibrationInput);
  const out = JSON.parse(ftx.material_calibrate_friction(raw, Boolean(allowEstimates)));
  return {
    mu: out.calibration?.coefficient?.value ?? null,
    inputSha256: out.calibration?.input_sha256 ?? null,
    fit: out.calibration?.fit ?? null,
    check: out.calibration?.check ?? null,
    fitNormalRangeN: out.calibration?.fit_normal_n_range ?? null,
    fitSpeedRangeMS: out.calibration?.fit_speed_m_s_range ?? null,
    predictions: out.calibration?.predictions ?? [],
    readiness: out.readiness ?? null,
    interfaceJson: out.interface_json ?? null,
    scope: out.scope ?? "",
  };
}

if (typeof DedicatedWorkerGlobalScope !== "undefined" && self instanceof DedicatedWorkerGlobalScope) {
  let latest;
  self.onmessage = async ({ data }) => {
    latest = data.id;
    try {
      const ftx = await getStudioEngine();
      if (latest !== data.id) return;
      const result = calibrateFrictionState(ftx, data.calibrationInput, data.allowEstimates);
      if (latest !== data.id) return;
      self.postMessage({ id: data.id, result });
    } catch (error) {
      if (latest === data.id) {
        self.postMessage({ id: data.id, error: error instanceof Error ? error.message : String(error) });
      }
    }
  };
}
