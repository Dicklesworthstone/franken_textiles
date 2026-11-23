"use client";

import dynamic from "next/dynamic";
import { useCallback, useState } from "react";
import InlineFilm, { filmFile } from "../InlineFilm";
import CodeBlock from "../site/CodeBlock";
import { repo } from "../site/links";
import HussarReferences from "./HussarReferences";
import HussarWardrobe from "./HussarWardrobe";
import ConstructionStudy, { ConstructionMotion } from "../construction/ConstructionStudy";
import SiteHeader from "../site/SiteHeader";
import SiteFooter from "../site/SiteFooter";
import Crumbs from "../site/Crumbs";
import type { Framing, Regiment } from "./HussarDrape";
import sampler from "../../public/wardrobe/hussar/braid-sampler.json";
import drape from "../../public/exhibits/hussar/dolman-summary.json";
import "./hussar.css";

const Scene = dynamic(() => import("./HussarDrape"), { ssr: false });
const svgPath = sampler.pieces[0].decor[0].pts.map(([x, y], i) => `${i ? "L" : "M"}${x} ${36 - y}`).join(" ");
const source = "https://sfhm.se/wp-content/uploads/2025/04/SFHM_skriftserie_Svenska_armens_uniformer_del_1_kavalleriet.pdf#page=21";
const cameras = ["Jacket", "Chest", "Reverse", "Figure"] as const satisfies readonly Framing[];
const excluded = drape.scope.excluded_instances?.length ?? 0;
const gating: readonly string[] = drape.gating;
const advisories = drape.flags.filter(flag => !gating.includes(flag));

function choice<T extends string>(name: string, choices: readonly T[], value: T, set: (v: T) => void) {
  return <fieldset><legend>{name}</legend><div className="hu-choice">{choices.map(item =>
    <button type="button" key={item} aria-pressed={value === item} onClick={() => set(item)}>{item}</button>)}</div></fieldset>;
}
function WardrobeFilm() {
  return <div className="hu-film">
    <InlineFilm id="wardrobe-hussar" title="Regimental dress and the embroidered jacket fronts" controls />
  </div>;
}
export default function HussarLab() {
  const [regiment, setRegiment] = useState<Regiment>("Blue");
  const [framing, setFraming] = useState<{ preset: Framing }>({ preset: "Jacket" });
  const [failed, setFailed] = useState(false);
  const onFailure = useCallback(() => setFailed(true), []);
  const blue = regiment === "Blue";
  return <><SiteHeader /><main id="main" className="hu-page"><Crumbs trail={[["Exhibition", "/"], ["Wardrobe", "/sections/wardrobe/"], ["The hussar’s cabinet"]]} />
    <div className="wrap hu-wrap"><div className="hu-heading"><div><p className="kicker">Swedish uniforms / pattern of 1761</p><h1>The hussar’s cabinet<em>.</em></h1></div>
      <p>Turn two Swedish hussars in regimental dress, inspect the jacket cuts, then replay the dolman sewn on its fitted form. <a className="hu-compare-link" href="#regimental-plates">Read the museum plate ↓</a></p></div>
    <ConstructionStudy choices={["blue","yellow"]}/>
    <div className="hu-desk" id="native-dolman"><div className="hu-figure">
      {failed ? <div className="hu-fallback"><ConstructionMotion key={regiment} garment={blue ? "blue" : "yellow"}/></div> :
        <Scene regiment={regiment} framing={framing} onFailure={onFailure}>
          <span className="hu-tag">{blue ? "Putbus’s Blue Hussars · 1761" : "Wrangel’s Yellow Hussars · 1765"}</span></Scene>}
      <div className="hu-cam">{failed ? <><span role="status">Recorded construction · 3D unavailable</span>
        <div className="hu-choice"><button type="button" onClick={()=>setFailed(false)}>Retry 3D</button></div></> : <><span>Drag to turn · Scroll to zoom</span>
        {choice("Camera", cameras, framing.preset, preset => setFraming({ preset }))}</>}
      </div>
    </div><aside className="hu-controls"><p className="kicker">Regimental dress</p><h2>{failed ? "Garment notes." : "Inspect the cut."}</h2>
      {choice("Regiment", ["Blue", "Yellow"] as const, regiment, setRegiment)}
      <div className="hu-palette" aria-hidden="true">{(blue ? ["#243b53", "#d9b15b", "#b7aa90"] : ["#222725", "#dedacb", "#b7aa90"]).map(c => <i key={c} style={{ background: c }} />)}</div>
      <h3>The dolman</h3>
      <p>{blue ? "Putbus’s regiment wore yellow cord on dark blue broadcloth." : "Wrangel’s regiment wore white cord on black broadcloth."} The same cut serves both. Twelve rows of cord cross each front, and each row is a line in the pattern that the draped cloth carries.</p>
      <dl className="hu-readings">
        <div><dt>Panels sewn</dt><dd>{drape.instances.length}</dd></div>
        <div><dt>Solver points</dt><dd>{drape.vertices.toLocaleString("en")}</dd></div>
        <div><dt>Edge strain <abbr title="95th percentile">P95</abbr></dt><dd>{drape.edgeStrainPercent.p95.toFixed(1)} %</dd></div>
        <div><dt>Body offset</dt><dd>{drape.bodyOffsetMm.achieved.toFixed(3)} <small>/ {drape.bodyOffsetMm.target} mm</small></dd></div>
      </dl>
      <div className="hu-pattern-links">{(["Blue", "Yellow"] as const).map(name =>
        <a key={name} href={`/patterns/hussar-${name.toLowerCase()}.pdf`} download>Download {name} ensemble pattern PDF ↓</a>)}</div>
      <details className="hu-pattern-notes"><summary>Size, pieces and printing</summary>
        <p>Chest 100 cm, waist 86 cm, head 58 cm. Each reconstructed cut has 49 templates covering the dolman, pelisse, breeches, charivades, cap, gauntlets, stock, sash and crossbelt. Supply boots, buttons, plume and hardware separately.</p>
        <p>Print at actual size on A4. Check both the 100 mm and 4 inch squares before joining the sheets. Test the cut in a toile.</p>
      </details>
    </aside></div>
    <details className="hu-method" open><summary>What the solver did, and what is drawn <span>{drape.gating.length ? "Reported with flags" : "Settled"}</span></summary>
      <p>The jacket on the form is the output of the FrankenTextiles cloth solver, replayed. {drape.instances.length} panels of the published pattern were wrapped round a synthetic adult form (stature 178 cm, chest 100 cm, waist 86 cm, arms {drape.armDeg}° from the side), sewn along their seams over {drape.phases.sew} steps and left to hang under gravity for {drape.phases.drape}. The replay shows the solver’s own positions.</p>
      <p>The front fastens with loops and buttons. The solver closes it as a seam and checks the jacket against both the body and its own panels throughout sewing and gravity. The shell uses a 1 mm cloth contact setting. The cuffs, facings, breeches, cap and pelisse appear in the pattern only{excluded ? ` (${excluded} excluded jacket pieces)` : ""}.</p>
      <p>Each panel bends on its own section of the cutting table. The view then cuts to the solver’s checked starting position on the form. The cord and buttons follow their marks on the pattern. Weave, lighting and seam threads help you read the surface. The jacket mount shows the posed body above the hip and a stand below. Its surface sits 2.5 mm inside the body used for the solve.</p>
      <p>The cut was revised by draping it and looking. Trial drapes of the first draft pulled tight across the shoulder blades and distorted the sleeve-cap seams. Three allowances followed: 14 mm let out on each side of the side-back seams at the armscye, 10 mm on each front and side-back panel at the underarm, and 14 mm on each half of the upper sleeve, which lowers its cap. Between the first allowance alone and all three, the 95th-percentile edge strain fell from 9.5% to 6.3% and the seams the solver called distorted from six to two. Those trials had the arms 20° from the side and no cloth-to-cloth contact, so their figures are not the ones of the run shown here, which are given above and below. A steeper shoulder was also tried and made the drape worse, so it was dropped. The PDF is the revised cut.</p>
      <p>The fabric constants are placeholders for a lined wool ({(drape.fabric.areal_kg_m2 * 1000).toFixed(0)} g/m², bending {drape.fabric.bending_Nm} N·m) and have no measured source. Edge strain over the whole jacket runs from {drape.edgeStrainPercent.min}% to +{drape.edgeStrainPercent.max}%. Nothing here shows that the jacket fits a person. That needs a toile.</p>
      {gating.map(flag => <p key={flag} className="hu-warning">Solver flag · {flag}</p>)}
      <p>The body offset sets the minimum distance between the cloth and the body’s collision surface. The solver increases it during sewing and gravity while preserving clearance at the closest contact. The largest remaining force component on a free vertex in the final solve was {drape.finalResidualN.toLocaleString("en", { maximumSignificantDigits: 3 })} N.</p>
      <p>On this same form, the <a href={`${repo}/blob/5d785218/showcase/public/exhibits/hussar/dolman-summary.json`}>earlier jacket run</a> reached 0.843 mm of body offset after 180 gravity steps. Tightening the Newton correction tolerance from 0.001 to 0.00001 m/s brought it to 1.441 mm at the same step. Another 23 steps brought it to the full 1.5 mm.</p>
      <details className="hu-flags"><summary>{advisories.length} advisory notes from the solver</summary>
        {advisories.map(flag => <p key={flag}>{flag}</p>)}</details>
      <details className="hu-flags"><summary>Replay this jacket with FrankenTextiles</summary>
        <p>Save the <a href="/exhibits/hussar/dolman-pattern.json" download>jacket pattern and starting placements</a> and <a href="/exhibits/hussar/dolman-measures.json" download>synthetic body measurements</a> together. With the converted Anny 0.6.1 model installed, run these commands in that directory.</p>
        <CodeBlock title="Sew the dolman and check the recorded surfaces" code={`ftx drape dolman-pattern.json --measurements dolman-measures.json ${drape.solverArguments.join(" ")} --out dolman\nftx viewer dolman --check-contact`}/>
        {"solverRevision" in drape && typeof drape.solverRevision === "string" &&
          <p>Native solver used for this recording · <a href={`${repo}/tree/${drape.solverRevision}`}><code>{drape.solverRevision.slice(0, 8)}</code></a></p>}
        <p>Recorded <code>fold.pc2</code> SHA-256 · <code style={{ overflowWrap: "anywhere" }}>{drape.sourcePc2Sha256}</code></p>
      </details>
      <p><a href="/exhibits/hussar/dolman-summary.json" download>Download the solver summary and flags ↓</a></p>
    </details>
    <HussarWardrobe/>
    <HussarReferences />
    <section className="hu-reel"><div><p className="kicker">Wardrobe film</p><h2>Around the figure.</h2><p>See the two regimental uniforms and follow the braid across the dolman and pelisse fronts from the cutting patterns.</p><a href={filmFile("wardrobe-hussar")} download>Download the film ↓</a></div><WardrobeFilm /></section>
    <section className="hu-sample"><div className="hu-braid"><svg viewBox="0 0 100 36" role="img" aria-label="The jacket’s looped chest cord laid flat"><path d={svgPath} fill="none" stroke="#81612f" strokeWidth="2.5" strokeLinejoin="round" /><path d={svgPath} fill="none" stroke="#ebca7e" strokeWidth="1.25" strokeLinejoin="round" /></svg><span>100 × 36 mm sampler board</span></div>
      <div><p className="kicker">From cord to machine</p><h2>Take a braid sample.</h2><p>Every row of cord on the jacket repeats this flat loop. The FrankenTextiles exporter wrote the two-pass Tajima file for it, including the stop to lay the cord.</p>
        <div className="hu-files"><a href="/wardrobe/hussar/embroidery/">Trace stitches on the 3D dolman →</a><a href="/wardrobe/hussar/braid-sampler.dst" download>Download DST ↓</a><a href="/wardrobe/hussar/braid-sampler.json" download>Pattern JSON ↓</a></div></div></section>
    <details className="hu-history"><summary>Historical references and scope</summary>
      <p>The Swedish military-history museums describe the uniform pattern of 1761 on page 39. Schützercrantz’s plate on page 40 depicts the Blue trooper in 1761 and the Yellow trooper in 1765. The cut is an original reconstruction from those plates, not a copy of a surviving garment, and its dimensions belong to a synthetic adult fitted with FrankenTextiles’ Anny body model.</p>
      <p><a href={source}>Swedish cavalry uniforms, pp. 38–39 ↗</a> · <a href="https://historicalsoldiers.com/">Historical Soldiers visual reference ↗</a></p></details>
    </div></main><SiteFooter /></>;
}
