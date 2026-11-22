// Pure geometry, packed-drape decompression, and comparison metrics for
// comparing two independently accepted native drapes of one fixed sundress (ftx-9zm).
//
// Invariants:
// - Both accepted drapes share the exact same fixed dress-pattern.json, 20 mm spacing,
//   3,458 cloth vertices, and solverArguments.
// - Both sides render in a common world frame (metres, sole anchored at Y = 0.000 m).
// - Swapping left & right sides preserves each side's report, flags, SHA-256, and world scale.
// - Placing the same state on both sides produces exact 0.0 mm / 0.00% deltas.
// - A refused solve (`accepted === false`) has no completed garment mesh and cannot render as one.

export const COMMON_WORLD_SCALE_M_PER_UNIT = 1.0;

export function parsePackedDrapeBuffer(meta, rawBuffer) {
  const view = new DataView(rawBuffer);
  const nBody = view.getUint32(0, true);
  const nBodyIdx = view.getUint32(4, true);
  const nCloth = view.getUint32(8, true);
  const nFrames = view.getUint32(12, true);

  let off = 16;
  const bodyRaw = new Float32Array(rawBuffer, off, nBody * 3);
  off += nBody * 3 * 4;
  const bodyIndices = new Uint32Array(rawBuffer, off, nBodyIdx);
  off += nBodyIdx * 4;
  const flatUv = new Float32Array(rawBuffer, off, nCloth * 2);
  off += nCloth * 2 * 4;
  // Take final settled physical frame (frame index nFrames - 1)
  const finalFrameOffset = off + (nFrames - 1) * nCloth * 3 * 4;
  const clothRaw = new Float32Array(rawBuffer, finalFrameOffset, nCloth * 3);

  // Find sole Z (minimum Z in native z-up Anny world frame) so every form stands on exact Y = 0.000 m floor!
  let minZ = Infinity;
  for (let i = 2; i < bodyRaw.length; i += 3) {
    if (bodyRaw[i] < minZ) minZ = bodyRaw[i];
  }

  // Convert native (X right, Y back, Z up; facing -Y) -> Three.js (X right, Y up from sole=0, Z front=-Y)
  const bodyPos = new Float32Array(nBody * 3);
  for (let i = 0; i < nBody; i++) {
    bodyPos[i * 3] = bodyRaw[i * 3];
    bodyPos[i * 3 + 1] = bodyRaw[i * 3 + 2] - minZ;
    bodyPos[i * 3 + 2] = -bodyRaw[i * 3 + 1];
  }

  const clothPos = new Float32Array(nCloth * 3);
  for (let i = 0; i < nCloth; i++) {
    clothPos[i * 3] = clothRaw[i * 3];
    clothPos[i * 3 + 1] = clothRaw[i * 3 + 2] - minZ;
    clothPos[i * 3 + 2] = -clothRaw[i * 3 + 1];
  }

  return {
    nBody,
    nCloth,
    nFrames,
    soleZRawM: minZ,
    bodyPos,
    bodyIndices,
    clothPos,
    clothIndices: new Uint32Array(meta.triangles),
    flatUv,
    panels: meta.panels,
  };
}

export function buildCompareMetrics(leftState, rightState) {
  if (!leftState || !rightState) return null;
  const leftAccepted = Boolean(leftState.accepted);
  const rightAccepted = Boolean(rightState.accepted);
  const bothAccepted = leftAccepted && rightAccepted;
  const sameState = leftState.id === rightState.id;

  const diff = (a, b, digits = 1) => Number((b - a).toFixed(digits));

  return {
    worldScale: COMMON_WORLD_SCALE_M_PER_UNIT,
    leftId: leftState.id,
    rightId: rightState.id,
    leftAccepted,
    rightAccepted,
    bothAccepted,
    sameState,
    canRenderLeftGarment: leftAccepted && Boolean(leftState.sourceBase),
    canRenderRightGarment: rightAccepted && Boolean(rightState.sourceBase),
    deltas: bothAccepted
      ? {
          hemFrontMm: sameState ? 0 : diff(leftState.world.hemAboveSoleMm.front, rightState.world.hemAboveSoleMm.front, 1),
          hemBackMm: sameState ? 0 : diff(leftState.world.hemAboveSoleMm.back, rightState.world.hemAboveSoleMm.back, 1),
          hemMeanMm: sameState ? 0 : diff(leftState.world.hemAboveSoleMm.mean, rightState.world.hemAboveSoleMm.mean, 1),
          frontGapMm: sameState ? 0 : diff(leftState.section.frontGapMm, rightState.section.frontGapMm, 1),
          backGapMm: sameState ? 0 : diff(leftState.section.backGapMm, rightState.section.backGapMm, 1),
          strainP95Pct: sameState ? 0 : diff(leftState.summary.edgeStrainPercent.p95, rightState.summary.edgeStrainPercent.p95, 2),
        }
      : null,
  };
}
