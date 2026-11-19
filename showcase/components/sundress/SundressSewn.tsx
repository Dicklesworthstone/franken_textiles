"use client";

import dynamic from "next/dynamic";
import { useState } from "react";
import type { DrapeLook, DrapeView } from "../atelier3d/SolverDrape";
import drape from "../../public/exhibits/sundress/dress-summary.json";

const Making = dynamic(() => import("../atelier3d/SolverDrape"), { ssr: false });

// A plain-weave cotton in the fitting room's green, self-lined. The colour is for display; the drape's own
// fabric constants are in the note below.
const looks: Record<string, DrapeLook> = {
  cotton: { fabric: { colour: "#0f5f4d", lining: "#0c4a3c", sheen: "#5fb59b", weave: "plain", yarnPitchMm: 0.7, thickness: 0.0012, nap: 0.3, wrinkleMm: 14 }, edge: "hem" },
};
const cameras = ["Full length", "Bodice", "Back"] as const;
const views: Record<(typeof cameras)[number], DrapeView> = {
  "Full length": { target: 0.9, span: 0.95, yaw: -0.32 },
  Bodice: { target: 1.22, span: 0.34, yaw: -0.25 },
  Back: { target: 0.9, span: 0.95, yaw: Math.PI - 0.32 },
};
const pieces: Record<string, string> = { bodice_front: "Bodice front", bodice_back: "Bodice back", skirt_front: "Skirt front", skirt_back: "Skirt back" };

/** The sundress from flat pattern to sewn dress, as the cloth solver made it on the reference form: `public/exhibits/sundress/dress.*`. */
export default function SundressSewn() {
  const [view, setView] = useState<{ preset: (typeof cameras)[number] }>({ preset: "Full length" });
  const flags = drape.flags as string[], gating = drape.gating as string[];
  return <section className="sd-sewn" id="sewn" aria-labelledby="sewn-title">
    <div className="sd-sewn-board">
      <Making source="/exhibits/sundress/dress" looks={looks} look="cotton" views={views} view={view} className="sd-making"
        captureHook="ftxDressFrameDrape" pieceName={panel => pieces[panel] ?? panel}
        label="The garden sundress from flat pattern to sewn dress, made by the cloth solver on the reference form. Drag or use arrow keys to turn, and plus or minus to zoom.">
        <div className="sd-stage-caption"><i /> Fitted Anny body <span>·</span> solver drape</div></Making>
      <div className="sd-viewbar"><div>{cameras.map(preset =>
        <button key={preset} aria-pressed={view.preset === preset} onClick={() => setView({ preset })}>{preset}</button>)}</div>
        <p>Drag to turn · scroll to zoom</p></div>
    </div>
    <div className="sd-sewn-text"><p className="kicker">From flat pattern to sewn dress</p><h2 id="sewn-title">The same pattern, as cloth.</h2>
      <p>Follow the two bodices and two skirt panels around the reference form. The native solver closes eight seams and lets gravity pull on the hem. Turn the form to see how the same waist gather hangs at the front and back.</p>
      <dl className="sd-readings">
        <div><dt>Solver points</dt><dd>{drape.vertices.toLocaleString("en")}</dd></div>
        <div><dt>Gravity steps</dt><dd>{drape.phases.drape}</dd></div>
        <div><dt>Edge strain <abbr title="95th percentile">P95</abbr></dt><dd>{drape.edgeStrainPercent.p95.toFixed(1)} %</dd></div>
      </dl>
      <details open><summary>What the solver did, and what is drawn</summary>
        <p>Reference form: bust 90 cm, waist 72 cm, arms {drape.armDeg}° from the side. {drape.phases.sew} sewing steps and {drape.phases.drape} gravity steps, the last of them run until the cloth was still. {drape.settled && !gating.length ? "The run is numerically settled and carries no gating flag." : "The run carries the flags listed below."} The waist is gathered {(1.3).toFixed(1)} to 1 by the pattern’s own seam, so its folds come from the solve.</p>
        <p>The skirt starts 270 mm from the body axis. The contact audit finds zero crossings there and the 1 mm cloth-to-cloth barrier stays active through sewing and settling. Look at the waist in profile. Its broad folds stand away from the form on the {drape.spacingMm} mm mesh. Finer gathers and real-world fit still need comparison with a sewn sample.</p>
        <p>The weave, thickness, smoothing, piece colors and connecting threads are drawn for display. The fabric uses synthetic defaults of {(drape.fabric.areal_kg_m2 * 1000).toFixed(0)} g/m² and {drape.fabric.bending_Nm} N·m bending stiffness. The interactive form is drawn 2.5 mm inside the solver surface.</p>
        {gating.map(flag => <p key={flag} className="sd-flag">Solver flag · {flag}</p>)}
        {flags.filter(flag => !gating.includes(flag)).map(flag => <p key={flag} className="sd-flag">Solver note · {flag}</p>)}
        {flags.some(flag => flag.includes("rides up")) && <p>That note is worded for a sleeved jacket. What it measured here is that the hems settled that far above where the flat pattern places them: the waist seam sits above this form’s waist.</p>}
        <p><a href="/exhibits/sundress/dress-summary.json" download>Download the solver summary ↓</a></p>
      </details>
    </div>
  </section>;
}
