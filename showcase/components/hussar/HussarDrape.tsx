"use client";

import SolverDrape, { type DrapeInfo, type DrapeLook, type DrapeView } from "../atelier3d/SolverDrape";

export type { DrapeInfo };
export type Regiment = "Blue" | "Yellow";
export type Framing = "Jacket" | "Chest" | "Reverse" | "Figure";

// Fulled wool broadcloth is a plain weave with the yarns closed up, lined in unbleached linen.
// Colours follow the 1761 plate; the fabric constants of the drape are in the method note.
const looks: Record<Regiment, DrapeLook> = {
  Blue: { fabric: { colour: "#111a31", lining: "#b7aa90", sheen: "#46587f", weave: "plain", yarnPitchMm: 0.9, thickness: 0.0022 }, cord: "#cfa648", button: "#a98a43" },
  Yellow: { fabric: { colour: "#161717", lining: "#b7aa90", sheen: "#55595d", weave: "plain", yarnPitchMm: 0.9, thickness: 0.0022 }, cord: "#dedacb", button: "#b9b8b2" },
};
const views: Record<Framing, DrapeView> = {
  Jacket: { target: 1.3, span: 0.5, yaw: -0.3 },
  Chest: { target: 1.33, span: 0.25, yaw: -0.22 },
  Reverse: { target: 1.3, span: 0.5, yaw: Math.PI - 0.3 },
  Figure: { target: 0.9, span: 0.96, yaw: -0.3 },
};
// The jacket ends at the waist, so the form is shown as a jacket mount that ends at the hip.
const HIP_LINE = 0.94;

type Props = { regiment: Regiment; framing: { preset: Framing }; frame: number; onReady: (info: DrapeInfo) => void; onFailure: () => void };

/** The 1761 dolman as the cloth solver draped it: `public/exhibits/hussar/dolman.*`. */
export default function HussarDrape({ regiment, framing, frame, onReady, onFailure }: Props) {
  return <SolverDrape source="/exhibits/hussar/dolman" looks={looks} look={regiment} views={views} view={framing} frame={frame}
    mountAt={HIP_LINE} className="hu-scene" captureHook="ftxHussarFrame" onReady={onReady} onFailure={onFailure}
    label="The 1761 dolman draped by the cloth solver on a fitted adult form. Drag or use arrow keys to turn, and plus or minus to zoom." />;
}
