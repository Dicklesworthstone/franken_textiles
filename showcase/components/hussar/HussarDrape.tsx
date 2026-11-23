"use client";

import type { ReactNode } from "react";
import SolverDrape, { type DrapeLook, type DrapeView } from "../atelier3d/SolverDrape";
import report from "../../public/exhibits/hussar/dolman-summary.json";

export type Regiment = "Blue" | "Yellow";
export type Framing = "Jacket" | "Chest" | "Reverse" | "Figure";

// Fulled wool broadcloth is a plain weave with the yarns closed up and a raised nap, lined in unbleached linen.
// Colours follow the 1761 plate; the fabric constants of the drape are in the method note.
const looks: Record<Regiment, DrapeLook> = {
  Blue: { fabric: { colour: "#111a31", lining: "#b7aa90", sheen: "#46587f", weave: "plain", yarnPitchMm: 0.9, thickness: 0.0022, nap: 0.8, wrinkleMm: 16 }, cord: "#cfa648", button: "#a98a43" },
  Yellow: { fabric: { colour: "#161717", lining: "#b7aa90", sheen: "#55595d", weave: "plain", yarnPitchMm: 0.9, thickness: 0.0022, nap: 0.8, wrinkleMm: 16 }, cord: "#dedacb", button: "#b9b8b2" },
};
const views: Record<Framing, DrapeView> = {
  Jacket: { target: 1.3, span: 0.5, yaw: -0.3, fitWidth: true },
  Chest: { target: 1.33, span: 0.25, yaw: -0.22 },
  Reverse: { target: 1.3, span: 0.5, yaw: Math.PI - 0.3, fitWidth: true },
  Figure: { target: 0.9, span: 0.96, yaw: -0.3, fitWidth: true },
};
// The jacket ends at the waist, so the form is shown as a jacket mount that ends at the hip.
const HIP_LINE = 0.94;
const pieces: Record<string, string> = { back: "Back", front: "Front", side: "Side back", topsleeve: "Upper sleeve", undersleeve: "Under sleeve", collar: "Collar" };
const pieceName = (panel: string) => pieces[panel.replace(/^dolman_/, "").replace(/\.[RLC]$/, "")] ?? panel;

type Props = { regiment: Regiment; framing: { preset: Framing }; onFailure?: () => void; children?: ReactNode };

/** The 1761 dolman from flat pattern to sewn jacket, as the cloth solver made it: `public/exhibits/hussar/dolman.*`. */
export default function HussarDrape({ regiment, framing, onFailure, children }: Props) {
  return <SolverDrape source="/exhibits/hussar/dolman" recording={report} looks={looks} look={regiment} views={views} view={framing}
    mountAt={HIP_LINE} className="hu-making" captureHook="ftxHussarFrame" pieceName={pieceName} onFailure={onFailure}
    label="The 1761 dolman from flat pattern to sewn jacket, draped by the cloth solver on a fitted adult form. Drag or use the arrow keys to turn it, right-drag or Shift with the arrow keys to move it, scroll, pinch or use plus and minus to zoom. Zero returns to the film's own view.">{children}</SolverDrape>;
}
