// Evaluate fixed wrap-skirt front overlap, tie-placement bounds against flat pattern lengths,
// and neutral (rest) vs seated synthetic Anny coverage limits (ftx-kod).
//
// Invariants:
// - The wrap skirt flat pattern stays strictly fixed (1020 mm waist sweep, 1540 mm hem sweep,
//   780 mm each tie, 601.3 mm wrap edge) while the wearer form, posture, and tie settings change.
// - Every wrap-edge length, waistband division, and tie tail length is checked directly against
//   the native flat pattern dimensions — never clamped silently.

export const MIN_SAFE_OVERLAP_MM = 140;
export const MIN_BOW_TAIL_MM = 220;

export const TIE_PRESETS = [
  { id: "firm", label: "Firm waist tie (+20 mm ease)", easeMm: 20, slotShiftMm: 0 },
  { id: "relaxed", label: "Relaxed waist tie (+65 mm ease)", easeMm: 65, slotShiftMm: 0 },
  { id: "shifted", label: "Shifted side tie (+35 mm ease, +45 mm slot)", easeMm: 35, slotShiftMm: 45 },
];

export function evaluateWrapSetting(exhibit, { formId, posture = "rest", easeMm = 20, slotShiftMm = 0 }) {
  const form = exhibit.forms.find(f => f.id === formId);
  if (!form) throw new Error(`Unknown synthetic adult form: ${formId}`);
  const poseData = posture === "seated" ? form.seated : form.rest;
  if (!poseData) throw new Error(`Missing ${posture} pose slice for ${formId}`);

  const flat = exhibit.flat_geometry;
  const totalWaistMm = flat.total_waist_sweep_mm; // 1020 mm
  const hipSweepMm = flat.hip_sweep_180mm_below_waist_mm; // 1176 mm
  const lapSweepMm = flat.lap_sweep_310mm_below_waist_mm; // 1289 mm
  const tieLenMm = flat.tie_each_length_mm; // 780 mm
  const baseSlotMm = flat.slot_station_from_left_end_mm; // 690 mm (at right side seam)

  // Check requested tie pass-through placement against the flat 1020 mm waistband length.
  const requestedSlotMm = baseSlotMm + Number(slotShiftMm);
  const slotMinMm = flat.front_each_waist_mm + 20; // must stay past left side seam
  const slotMaxMm = totalWaistMm - 60; // must leave at least 60 mm before right wrap tip
  const slotInBounds = requestedSlotMm >= slotMinMm && requestedSlotMm <= slotMaxMm;

  // Waist wrap geometry: effective wrapped waist circumference = body waist + chosen tie ease.
  const bodyWaistMm = poseData.waist_tape_mm;
  const wrappedWaistMm = bodyWaistMm + Number(easeMm);
  const waistOverlapMm = Number((totalWaistMm - wrappedWaistMm).toFixed(1));
  const overlapHalfMm = Number((waistOverlapMm / 2).toFixed(1));

  // Neutral vs Seated horizontal Anny slice perimeter comparison at hip (-180 mm) and lap (-310 mm).
  const hipSlicePerimMm = poseData.sections.hip.perimeter_mm;
  const lapSlicePerimMm = poseData.sections.lap.perimeter_mm;
  // When seated, legs project forward so lap perimeter grows dramatically while fixed skirt sweep stays 1289 mm.
  const hipOverlapMm = Number((hipSweepMm - hipSlicePerimMm - Number(easeMm) * 0.5).toFixed(1));
  const lapOverlapMm = Number((lapSweepMm - lapSlicePerimMm).toFixed(1));

  // Tie reach check against flat 780 mm tie straps:
  // Outer tie runs from right wrap tip around the front/left side to knot at left side (~half waist minus overlap margin).
  // Inner tie passes through the right side slot and runs around the back to meet the outer tie at the left side.
  const halfWaistMm = wrappedWaistMm / 2;
  const innerTieWrapTravelMm = Math.max(0, halfWaistMm + Number(slotShiftMm));
  const outerTieWrapTravelMm = Math.max(0, halfWaistMm - Math.max(0, overlapHalfMm));
  const innerTailMm = Number((tieLenMm - innerTieWrapTravelMm).toFixed(1));
  const outerTailMm = Number((tieLenMm - outerTieWrapTravelMm).toFixed(1));
  const minTieTailMm = Math.min(innerTailMm, outerTailMm);

  const limits = [];
  if (!slotInBounds) {
    limits.push(
      `Requested tie slot (${requestedSlotMm.toFixed(0)} mm) falls outside the flat waistband safe zone (${slotMinMm}–${slotMaxMm} mm).`,
    );
  }
  if (waistOverlapMm < 0) {
    limits.push(`Fixed 1020 mm waistband cannot close around ${wrappedWaistMm.toFixed(0)} mm wrapped waist (${waistOverlapMm} mm short).`);
  } else if (waistOverlapMm < MIN_SAFE_OVERLAP_MM) {
    limits.push(`Front waist overlap (${waistOverlapMm} mm) is below the ${MIN_SAFE_OVERLAP_MM} mm coverage margin.`);
  }
  if (hipOverlapMm < 0) {
    limits.push(
      `Horizontal ${posture} hip/thigh slice (${hipSlicePerimMm} mm) exceeds the fixed ${hipSweepMm} mm skirt hip sweep by ${Math.abs(hipOverlapMm)} mm — front wrap parts when sitting.`,
    );
  } else if (hipOverlapMm < MIN_SAFE_OVERLAP_MM) {
    limits.push(
      `Hip overlap in ${posture} pose (${hipOverlapMm} mm against ${hipSlicePerimMm} mm slice) falls below ${MIN_SAFE_OVERLAP_MM} mm.`,
    );
  }
  if (lapOverlapMm < 0) {
    limits.push(
      `Lower-thigh/shin slice (${lapSlicePerimMm} mm) exceeds the fixed ${lapSweepMm} mm skirt sweep by ${Math.abs(lapOverlapMm)} mm.`,
    );
  } else if (lapOverlapMm < MIN_SAFE_OVERLAP_MM) {
    limits.push(
      `Lower-thigh overlap (${lapOverlapMm} mm against ${lapSlicePerimMm} mm slice) is below ${MIN_SAFE_OVERLAP_MM} mm.`,
    );
  }
  if (minTieTailMm < 0) {
    limits.push(`Flat 780 mm tie strap is ${Math.abs(minTieTailMm)} mm too short to reach the side knot at this setting.`);
  } else if (minTieTailMm < MIN_BOW_TAIL_MM) {
    limits.push(`Remaining tie tail (${minTieTailMm} mm) is under ${MIN_BOW_TAIL_MM} mm (enough for a knot, short for a full bow).`);
  }

  let status = "SECURE";
  if (!slotInBounds || waistOverlapMm < 0 || hipOverlapMm < 0 || lapOverlapMm < 0 || minTieTailMm < 0) {
    status = "LIMIT_EXCEEDED";
  } else if (limits.length > 0) {
    status = "MARGINAL";
  }

  return {
    formId: form.id,
    formLabel: form.label,
    posture,
    easeMm: Number(easeMm),
    slotShiftMm: Number(slotShiftMm),
    requestedSlotMm,
    slotInBounds,
    bodyWaistMm,
    wrappedWaistMm: Number(wrappedWaistMm.toFixed(1)),
    waistOverlapMm,
    overlapHalfMm,
    hipSlicePerimMm,
    hipOverlapMm,
    lapSlicePerimMm,
    lapOverlapMm,
    innerTailMm,
    outerTailMm,
    minTieTailMm,
    status,
    limits,
    hullHip: poseData.sections.hip.hull,
    hullLap: poseData.sections.lap.hull,
    hullWaist: poseData.sections.waist.hull,
  };
}
