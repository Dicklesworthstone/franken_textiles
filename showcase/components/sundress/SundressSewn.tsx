"use client";

import dynamic from "next/dynamic";
import { useCallback, useEffect, useRef, useState } from "react";
import { PlaybackMark } from "../InlineFilm";
import type { DrapeInfo, DrapeLook, DrapeView } from "../atelier3d/SolverDrape";
import drape from "../../public/exhibits/sundress/dress-summary.json";

const Replay = dynamic(() => import("../atelier3d/SolverDrape"), { ssr: false });

// A plain-weave cotton in the fitting room's green, self-lined. The colour is for display; the drape's own
// fabric constants are in the note below.
const looks: Record<string, DrapeLook> = {
  cotton: { fabric: { colour: "#0f5f4d", lining: "#0c4a3c", sheen: "#5fb59b", weave: "plain", yarnPitchMm: 0.7, thickness: 0.0012 }, edge: "hem" },
};
const cameras = ["Full length", "Bodice", "Back"] as const;
const views: Record<(typeof cameras)[number], DrapeView> = {
  "Full length": { target: 0.9, span: 0.95, yaw: -0.32 },
  Bodice: { target: 1.22, span: 0.34, yaw: -0.25 },
  Back: { target: 0.9, span: 0.95, yaw: Math.PI - 0.32 },
};

// The replay keeps a thinned set of the solver's own frames; name each by the phase it came from.
function stage(info: DrapeInfo, index: number) {
  const f = info.sourceFrames[index], { fold, zip, sew } = info.phases;
  if (index === info.frames - 1) return "Settled";
  if (f === 0) return "Flat pattern";
  if (f < fold) return "Wrapping";
  if (f < fold + zip) return "Joining seams";
  if (f < fold + zip + sew) return "Sewing";
  return "Gravity";
}

/** The sundress pattern as the cloth solver sewed and hung it on the reference form: `public/exhibits/sundress/dress.*`. */
export default function SundressSewn() {
  const [view, setView] = useState<{ preset: (typeof cameras)[number] }>({ preset: "Full length" });
  const [info, setInfo] = useState<DrapeInfo | null>(null), [frame, setFrame] = useState(drape.frames - 1);
  const [playing, play] = useState(false), [failed, setFailed] = useState(false);
  const onReady = useCallback((next: DrapeInfo) => setInfo(next), []);
  const onFailure = useCallback(() => { setFailed(true); play(false); }, []);
  const began = useRef(0), last = drape.frames - 1;
  useEffect(() => {
    if (!playing) return;
    let raf = 0;
    const tick = (now: number) => {
      const next = Math.min(last, Math.floor((now - began.current) / 150));
      setFrame(next);
      if (next < last) raf = requestAnimationFrame(tick); else play(false);
    };
    const hidden = () => { if (document.hidden) play(false); };
    document.addEventListener("visibilitychange", hidden);
    raf = requestAnimationFrame(tick);
    return () => { cancelAnimationFrame(raf); document.removeEventListener("visibilitychange", hidden); };
  }, [playing, last]);
  function replay() {
    if (playing) { play(false); return; }
    const from = frame >= last ? 0 : frame;
    setFrame(from); began.current = performance.now() - from * 150; play(true);
  }
  const flags = drape.flags as string[], gating = drape.gating as string[];
  return <section className="sd-sewn" id="sewn" aria-labelledby="sewn-title">
    <div className="sd-sewn-board">
      <div className="sd-sewn-stage">{failed ? <p className="sd-sewn-failed">This view needs WebGL. The solver summary below still downloads.</p> :
        <Replay source="/exhibits/sundress/dress" looks={looks} look="cotton" views={views} view={view} frame={frame} className="sd-sewn-scene"
          captureHook="ftxDressFrameDrape" onReady={onReady} onFailure={onFailure}
          label="The garden sundress sewn and hung by the cloth solver on the reference form. Drag or use arrow keys to turn, and plus or minus to zoom." />}
        {!failed && <div className="sd-stage-caption"><i /> Fitted Anny body <span>·</span> solver drape</div>}</div>
      {!failed && <><div className="sd-viewbar"><div>{cameras.map(preset =>
        <button key={preset} aria-pressed={view.preset === preset} onClick={() => setView({ preset })}>{preset}</button>)}</div>
        <p>Drag to turn · scroll to zoom</p></div>
        <div className="sd-replay">
          <button type="button" onClick={replay} disabled={!info} aria-label={playing ? "Pause the sewing replay" : "Play the sewing replay"}><PlaybackMark playing={playing} /></button>
          <input type="range" min="0" max={last} value={frame} disabled={!info} aria-label="Sewing replay frame" aria-valuetext={info ? stage(info, frame) : "Loading"}
            onChange={event => { play(false); setFrame(Number(event.target.value)); }} />
          <span>{info ? stage(info, frame) : "Loading the drape…"}</span>
        </div></>}
    </div>
    <div className="sd-sewn-text"><p className="kicker">Sewn by the solver</p><h2 id="sewn-title">The same pattern, as cloth.</h2>
      <p>The dress above is drawn round the figure so that it can follow the sliders. This one is not drawn. The four pattern pieces were wrapped round the reference form, sewn along their eight seams and left to hang, and the replay shows the cloth solver’s own positions from the flat cut to the settled dress.</p>
      <dl className="sd-readings">
        <div><dt>Solver points</dt><dd>{drape.vertices.toLocaleString("en")}</dd></div>
        <div><dt>Gravity steps</dt><dd>{drape.phases.drape}</dd></div>
        <div><dt>Edge strain <abbr title="95th percentile">P95</abbr></dt><dd>{drape.edgeStrainPercent.p95.toFixed(1)} %</dd></div>
      </dl>
      <details open><summary>What the solver did, and what is drawn</summary>
        <p>Reference form: bust 90 cm, waist 72 cm, arms {drape.armDeg}° from the side. {drape.phases.sew} sewing steps and {drape.phases.drape} gravity steps, the last of them run until the cloth was still. {drape.settled && !gating.length ? "The run is numerically settled and carries no gating flag." : "The run carries the flags listed below."} The waist is gathered {(1.3).toFixed(1)} to 1 by the pattern’s own seam, so its folds come from the solve.</p>
        <p>Two limits show in the side view. Cloth collides with the form and not yet with itself, so where gathers press together they can pass through each other. And on a {drape.spacingMm ?? 20} mm mesh the gathered waist cannot break into fine folds, so the skirt stands away from the waist, front and back, more than cotton would. Drawn for display, and not simulated: the colour, the weave, the 1.2 mm thickness and the smoothing between solver points. The fabric constants are the solver’s defaults ({(drape.fabric.areal_kg_m2 * 1000).toFixed(0)} g/m², bending {drape.fabric.bending_Nm} N·m) and have no measured source. The form is drawn 2.5 mm inside the surface the solver used. Nothing here shows that the dress fits a person. That needs a sewn sample.</p>
        {gating.map(flag => <p key={flag} className="sd-flag">Solver flag · {flag}</p>)}
        {flags.filter(flag => !gating.includes(flag)).map(flag => <p key={flag} className="sd-flag">Solver note · {flag}</p>)}
        {flags.some(flag => flag.includes("rides up")) && <p>That note is worded for a sleeved jacket. What it measured here is that the hems settled that far above where the flat pattern places them: the waist seam sits above this form’s waist.</p>}
        <p><a href="/exhibits/sundress/dress-summary.json" download>Download the solver summary ↓</a></p>
      </details>
    </div>
  </section>;
}
