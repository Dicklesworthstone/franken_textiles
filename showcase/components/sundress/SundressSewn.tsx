"use client";

import dynamic from "next/dynamic";
import { useState } from "react";
import type { DrapeLook, DrapeView } from "../atelier3d/SolverDrape";
import gathered from "../../public/exhibits/sundress/dress-summary.json";
import gatheredSection from "../../public/exhibits/sundress/dress-section.json";
import gatheredPattern from "../../public/exhibits/sundress/dress-pattern.json";
import eased from "../../public/exhibits/sundress/light-summary.json";
import easedSection from "../../public/exhibits/sundress/light-section.json";
import easedPattern from "../../public/exhibits/sundress/light-pattern.json";
import TorsoSlice from "../construction/TorsoSlice";

const Making = dynamic(() => import("../atelier3d/SolverDrape"), { ssr: false });

// A plain-weave cotton in the fitting room's sprig print, self-lined. The print runs with each piece's grain, so
// it shows how the cloth turns and gathers. Colour and print are for display; the drape's own fabric constants
// are in the note below.
const looks: Record<string, DrapeLook> = {
  cotton: { fabric: { colour: "#0e6766", lining: "#0b5352", sheen: "#7fc4b4", weave: "plain", yarnPitchMm: 0.7, thickness: 0.0012, nap: 0.3, wrinkleMm: 14, print: "sprig" }, edge: "hem" },
};
const cameras = ["Full length", "Bodice", "Waist profile", "Back"] as const;
const views: Record<(typeof cameras)[number], DrapeView> = {
  "Full length": { target: 0.9, span: 0.95, yaw: -0.32 },
  Bodice: { target: 1.22, span: 0.34, yaw: -0.25 },
  "Waist profile": { target: 1.02, span: 0.32, yaw: -Math.PI / 2 },
  Back: { target: 0.9, span: 0.95, yaw: Math.PI - 0.32 },
};
const pieces: Record<string, string> = { bodice_front: "Bodice front", bodice_back: "Bodice back", skirt_front: "Skirt front", skirt_back: "Skirt back" };

const cuts = {
  dress: { name: "Gathered waist", report: gathered, section: gatheredSection, pattern: gatheredPattern, pdf: "/sundress/pattern-A4.pdf" },
  light: { name: "Lightly eased waist", report: eased, section: easedSection, pattern: easedPattern, pdf: "/exhibits/sundress/light-A4.pdf" },
};
const cutIds = ["dress", "light"] as const;
function frontSkirt(pattern: Pick<typeof gatheredPattern, "pieces">) {
  const skirt = pattern.pieces.find(piece => piece.id === "skirt_front")!;
  return {
    outline: skirt.edges.map(edge => skirt.vertices[edge.from as keyof typeof skirt.vertices]!.join(",")).join(" "),
    waist: skirt.vertices.wr[0] - skirt.vertices.wl[0],
  };
}

type WaistSeam = { ratio?: number; gather_relief_mm?: number; gather_compression?: number };

export type DressCut = (typeof cutIds)[number];
export default function SundressSewn({cut,setCut}:{cut:DressCut;setCut:(cut:DressCut)=>void}) {
  const [view, setView] = useState<{ preset: (typeof cameras)[number] }>({ preset: "Full length" });
  const {report: drape, section, pdf} = cuts[cut];
  const source = "/exhibits/sundress/" + cut;
  const waistSeam: WaistSeam = cuts[cut].pattern.seams.find(seam => seam.id === "gather_front")!;
  const ratio = waistSeam.ratio;
  // A gathered waist may declare a band of pleats for the solver; the eased cut has none.
  const band = waistSeam.gather_relief_mm && waistSeam.gather_compression
    ? { depth: waistSeam.gather_relief_mm, percent: Number((waistSeam.gather_compression * 100).toPrecision(3)) } : null;
  const flags = drape.flags as string[], gating = drape.gating as string[];
  return <section className="sd-sewn" id="sewn" aria-labelledby="sewn-title">
    <div className="sd-cut-picker">
      <div><p className="kicker">Pattern revision</p><h2 id="sewn-title">One bodice, two skirt cuts.</h2>
        <p>Both skirts have a 168 cm hem. Shortening each waist edge by 97.5 mm brings the cloth closer at centre front, from {gatheredSection.frontGapMm} to {easedSection.frontGapMm} mm clear of the same wearer.</p></div>
      <div className="sd-cut-options" role="group" aria-label="Skirt waist cut">{cutIds.map(id => {
        const {outline, waist} = frontSkirt(cuts[id].pattern);
        return <button type="button" key={id} aria-pressed={cut === id} onClick={() => setCut(id)}>
          <svg viewBox="-445 -20 890 645" aria-hidden="true"><g transform="scale(1 -1)"><polygon points={outline}/>
            <path d={`M${-waist/2} 0h${waist}`}/></g></svg>
          <span><strong>{cuts[id].name}</strong><small>{waist} → 390 mm</small></span></button>;
      })}</div>
    </div>
    <div className="sd-sewn-board">
      <Making key={cut} source={source} looks={looks} look="cotton" views={views} view={view} className="sd-making"
        captureHook="ftxDressFrameDrape" pieceName={panel => pieces[panel] ?? panel}
        label="The garden sundress from flat pattern to sewn dress, made by the cloth solver on the reference form. Drag or use arrow keys to turn, and plus or minus to zoom.">
        <div className="sd-stage-caption"><i /> Fitted Anny body <span>·</span> solver drape</div></Making>
      <div className="sd-viewbar"><div>{cameras.map(preset =>
        <button key={preset} aria-pressed={view.preset === preset} onClick={() => setView({ preset })}>{preset}</button>)}</div>
        <p>Drag to turn · scroll to zoom</p></div>
    </div>
    <div className="sd-sewn-text"><p className="kicker">From flat pattern to sewn dress</p><h3>{cuts[cut].name}</h3>
      <p>Follow the shoulders, side seams and waist as the solver joins the four pieces. Use <b>Waist profile</b> to inspect the folds from the side.</p>
      <a href={pdf} download>Full-size A4 pattern for this cut ↓</a>
      <dl className="sd-readings">
        <div><dt>Solver points</dt><dd>{drape.vertices.toLocaleString("en")}</dd></div>
        <div><dt>Gravity steps</dt><dd>{drape.phases.drape}</dd></div>
        <div><dt>Edge strain <abbr title="95th percentile">P95</abbr></dt><dd>{drape.edgeStrainPercent.p95.toFixed(1)} %</dd></div>
      </dl>
      <figure className="sd-slice"><TorsoSlice {...section} otherCut={cut==="light"?gatheredSection.cloth:easedSection.cloth}/><figcaption><strong>Through the wearer’s waist</strong>
        <span>Both cuts are sectioned at {(section.heightMm/10).toFixed(1)} cm above the sole. This cloth (red) clears the torso (brown) by <b>{section.frontGapMm} mm</b> at centre front and <b>{section.backGapMm} mm</b> at centre back.</span>
        <a href={source+"-section.json"} download>Section coordinates ↓</a></figcaption></figure>
      <details open><summary>What the solver did, and what is drawn</summary>
        <p>The reference form has a 90 cm bust and a 72 cm waist, with arms {drape.armDeg}° from the side. The solver took {drape.phases.sew} sewing steps and {drape.phases.drape} gravity steps. {drape.settled && !gating.length ? "The run settled with no gating flags." : "The run carries the flags listed below."} Each skirt waist is {ratio} times the length of the bodice edge it joins.</p>
        <p>The skirt starts 270 mm from the body axis. The contact audit finds zero crossings there and the 1 mm cloth-to-cloth barrier stays active through sewing and settling. {band
          ? <>The gathers are modelled as a band of pleats. For {band.depth} mm below the waist seam the solver lets the cloth shorten almost freely, at {band.percent} % of its stiffness in tension, which stands for pleats finer than its {drape.spacingMm} mm mesh. The band is authored, not measured. Without it this skirt left the waist as a shelf, 92.8 mm clear at centre front. The fine pleats drawn under the seam mark where the solver’s cloth is shorter than its pattern. They are not folds the solver resolved. In profile a small ledge remains at the back.</>
          : <>Look at the waist in profile. Its broad folds stand away from the form on the {drape.spacingMm} mm mesh.</>} Finer gathers and real-world fit still need comparison with a sewn sample.</p>
        <p>The viewer draws weave, piece colors and seam guides on the recorded cloth surface. The fabric uses synthetic defaults of {(drape.fabric.areal_kg_m2 * 1000).toFixed(0)} g/m² and {drape.fabric.bending_Nm} N·m bending stiffness. The interactive form is drawn 2.5 mm inside the solver surface.</p>
        {band && <p>A deeper pleat band let the skirt cross the fingers during sewing. This version clears the complete hands and body in all {1 + drape.phases.sew + drape.phases.drape} recorded physical states.</p>}
        {gating.map(flag => <p key={flag} className="sd-flag">Solver flag · {flag}</p>)}
        {flags.filter(flag => !gating.includes(flag)).map(flag => <p key={flag} className="sd-flag">Solver note · {flag}</p>)}
        {flags.some(flag => flag.includes("slid")) && <p>The slide flags compare how high each skirt panel’s corners sit after sewing and after gravity, and are worded for a garment slipping off the body. What they measured here is the gathered top of the skirt coming down to hang from the waist once its pleats could close. The waist seam itself stays sewn to the bodice.</p>}
        {flags.some(flag => flag.includes("rides up")) && <p>That note is worded for a sleeved jacket. What it measured here is that the hems settled that far above where the flat pattern places them: the waist seam sits above this form’s waist.</p>}
        <p><a href={source+"-pattern.json"} download>Exact pattern and starting placements ↓</a> · <a href={source+"-summary.json"} download>Solver summary ↓</a></p>
      </details>
    </div>
  </section>;
}
