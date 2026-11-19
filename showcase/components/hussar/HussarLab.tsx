"use client";

import dynamic from "next/dynamic";
import { useCallback, useEffect, useRef, useState } from "react";
import InlineFilm, { PlaybackMark, filmFile } from "../InlineFilm";
import HussarReferences from "./HussarReferences";
import HussarWardrobe from "./HussarWardrobe";
import SiteHeader from "../site/SiteHeader";
import SiteFooter from "../site/SiteFooter";
import Crumbs from "../site/Crumbs";
import type { DrapeInfo, Framing, Regiment } from "./HussarDrape";
import sampler from "../../public/wardrobe/hussar/braid-sampler.json";
import drape from "../../public/exhibits/hussar/dolman-summary.json";
import "./hussar.css";

const Scene = dynamic(() => import("./HussarDrape"), { ssr: false });
const svgPath = sampler.pieces[0].decor[0].pts.map(([x, y], i) => `${i ? "L" : "M"}${x} ${36 - y}`).join(" ");
const source = "https://sfhm.se/wp-content/uploads/2025/04/SFHM_skriftserie_Svenska_armens_uniformer_del_1_kavalleriet.pdf#page=21";
const cameras = ["Jacket", "Chest", "Reverse", "Figure"] as const satisfies readonly Framing[];
const excluded = drape.scope.excluded_instances?.length ?? 0;

function choice<T extends string>(name: string, choices: readonly T[], value: T, set: (v: T) => void) {
  return <fieldset><legend>{name}</legend><div className="hu-choice">{choices.map(item =>
    <button type="button" key={item} aria-pressed={value === item} onClick={() => set(item)}>{item}</button>)}</div></fieldset>;
}
function WardrobeFilm({ compact = false }: { compact?: boolean }) {
  return <div className={compact ? "hu-fallback" : "hu-film"}>
    <InlineFilm id="wardrobe-hussar" title="Regimental dress and the embroidered jacket fronts" controls />
  </div>;
}
// The replay keeps a thinned set of the solver's own frames; name each by the phase it came from.
function stage(info: DrapeInfo, index: number) {
  const f = info.sourceFrames[index], { fold, zip, sew } = info.phases;
  if (index === info.frames - 1) return "Draped";
  if (f === 0) return "Flat pattern";
  if (f < fold) return "Wrapping";
  if (f < fold + zip) return "Joining seams";
  if (f < fold + zip + sew) return "Sewing";
  return "Gravity";
}

export default function HussarLab() {
  const [regiment, setRegiment] = useState<Regiment>("Blue");
  const [framing, setFraming] = useState<{ preset: Framing }>({ preset: "Jacket" });
  const [info, setInfo] = useState<DrapeInfo | null>(null), [frame, setFrame] = useState(drape.frames - 1);
  const [playing, play] = useState(false), [failed, setFailed] = useState(false);
  const onFailure = useCallback(() => { setFailed(true); play(false); }, []);
  const onReady = useCallback((next: DrapeInfo) => setInfo(next), []);
  const began = useRef(0), blue = regiment === "Blue", last = drape.frames - 1;
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
  return <><SiteHeader /><main id="main" className="hu-page"><Crumbs trail={[["Exhibition", "/"], ["Wardrobe", "/#wardrobe"], ["The hussar’s cabinet"]]} />
    <div className="wrap hu-wrap"><div className="hu-heading"><div><p className="kicker">Swedish uniforms / pattern of 1761</p><h1>The hussar’s cabinet<em>.</em></h1></div>
      <p>Turn two Swedish hussars in regimental dress, inspect the jacket cuts, then replay the dolman sewn on its fitted form. <a className="hu-compare-link" href="#regimental-plates">Read the museum plate ↓</a></p></div>
    <HussarWardrobe/>
    <div className="hu-desk" id="native-dolman"><div className="hu-figure">
      <div className="hu-stage">{failed ? <WardrobeFilm compact /> : <Scene regiment={regiment} framing={framing} frame={frame} onReady={onReady} onFailure={onFailure} />}
        {!failed && <><span className="hu-plate">Swedish Pomerania · Dolman of 1761 · solver drape</span>
          <span className="hu-tag">{blue ? "Putbus’s Blue Hussars · 1761" : "Wrangel’s Yellow Hussars · 1765"}</span></>}</div>
      <div className="hu-cam"><span>{failed ? "Recorded turn of the jacket" : "Drag to turn · Scroll to zoom"}</span>
        {!failed && choice("Camera", cameras, framing.preset, preset => setFraming({ preset }))}
      </div>
      {!failed && <div className="hu-replay">
        <button type="button" onClick={replay} disabled={!info} aria-label={playing ? "Pause the sewing replay" : "Play the sewing replay"}><PlaybackMark playing={playing} /></button>
        <input type="range" min="0" max={last} value={frame} disabled={!info} aria-label="Sewing replay frame" aria-valuetext={info ? stage(info, frame) : "Loading"}
          onChange={event => { play(false); setFrame(Number(event.target.value)); }} />
        <span>{info ? stage(info, frame) : "Loading the drape…"}</span>
      </div>}
    </div><aside className="hu-controls"><p className="kicker">Regimental dress</p><h2>{failed ? "Garment notes." : "Inspect the cut."}</h2>
      {choice("Regiment", ["Blue", "Yellow"] as const, regiment, setRegiment)}
      <div className="hu-palette" aria-hidden="true">{(blue ? ["#243b53", "#d9b15b", "#b7aa90"] : ["#222725", "#dedacb", "#b7aa90"]).map(c => <i key={c} style={{ background: c }} />)}</div>
      <h3>The dolman</h3>
      <p>{blue ? "Putbus’s regiment wore yellow cord on dark blue broadcloth." : "Wrangel’s regiment wore white cord on black broadcloth."} The same cut serves both. Twelve rows of cord cross each front, and each row is a line in the pattern that the draped cloth carries.</p>
      <dl className="hu-readings">
        <div><dt>Panels sewn</dt><dd>{drape.instances.length}</dd></div>
        <div><dt>Solver points</dt><dd>{drape.vertices.toLocaleString("en")}</dd></div>
        <div><dt>Edge strain <abbr title="95th percentile">P95</abbr></dt><dd>{drape.edgeStrainPercent.p95.toFixed(1)} %</dd></div>
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
      <p>The front fastens with loops and buttons, and is closed here as a seam. The cuffs and front facings are in the pattern and were left out of the drape{excluded ? ` (${excluded} cut pieces)` : ""}, as were the breeches, cap and pelisse. Cloth does not yet collide with cloth in this run, only with the form.</p>
      <p>Drawn for display, and not simulated: the weave, the 2 mm thickness, the smoothing between solver points and the round section of the cord. The cord and the button stations sit on the cloth where their lines lie in the flat pattern. The form has no skin or face. It is shown as a jacket mount: the solver draped the jacket on the whole body, and below the hip the page draws a plate and a pole in its place. The form is drawn 2.5 mm inside the surface the solver used, so that flat cloth facets do not show it through.</p>
      <p>The cut was revised by looking at it. A trial drape of the first draft pulled tight across the shoulder blades and the solver flagged the sleeve-cap seams, so each side-back seam was let out 11 mm at the armscye, tapering to nothing at the waist. The PDF is the revised cut. The trial runs are not published here.</p>
      <p>The fabric constants are placeholders for a lined wool ({(drape.fabric.areal_kg_m2 * 1000).toFixed(0)} g/m², bending {drape.fabric.bending_Nm} N·m) and have no measured source. Edge strain over the whole jacket runs from {drape.edgeStrainPercent.min}% to +{drape.edgeStrainPercent.max}%. Nothing here shows that the jacket fits a person. That needs a toile.</p>
      {drape.gating.map(flag => <p key={flag} className="hu-warning">Solver flag · {flag}</p>)}
      <details className="hu-flags"><summary>{drape.flags.length - drape.gating.length} advisory notes from the solver</summary>
        {drape.flags.filter(flag => !drape.gating.includes(flag)).map(flag => <p key={flag}>{flag}</p>)}</details>
      <p><a href="/exhibits/hussar/dolman-summary.json" download>Download the solver summary and every flag ↓</a></p>
    </details>
    <HussarReferences />
    <section className="hu-reel"><div><p className="kicker">Wardrobe film</p><h2>Around the figure.</h2><p>See the two regimental uniforms and follow the braid across the dolman and pelisse fronts from the cutting patterns.</p><a href={filmFile("wardrobe-hussar")} download>Download the film ↓</a></div>{!failed && <WardrobeFilm />}</section>
    <section className="hu-sample"><div className="hu-braid"><svg viewBox="0 0 100 36" role="img" aria-label="The jacket’s looped chest cord laid flat"><path d={svgPath} fill="none" stroke="#81612f" strokeWidth="2.5" strokeLinejoin="round" /><path d={svgPath} fill="none" stroke="#ebca7e" strokeWidth="1.25" strokeLinejoin="round" /></svg><span>100 × 36 mm sampler board</span></div>
      <div><p className="kicker">From cord to machine</p><h2>Take a braid sample.</h2><p>Every row of cord on the jacket repeats this flat loop. The FrankenTextiles exporter wrote the two-pass Tajima file for it, including the stop to lay the cord.</p>
        <div className="hu-files"><a href="/wardrobe/hussar/braid-sampler.dst" download>Download DST ↓</a><a href="/wardrobe/hussar/braid-sampler.json" download>Pattern JSON ↓</a></div></div></section>
    <details className="hu-history"><summary>Historical references and scope</summary>
      <p>The Swedish military-history museums describe the uniform pattern of 1761 on page 39. Schützercrantz’s plate on page 40 depicts the Blue trooper in 1761 and the Yellow trooper in 1765. The cut is an original reconstruction from those plates, not a copy of a surviving garment, and its dimensions belong to a synthetic adult fitted with FrankenTextiles’ Anny body model.</p>
      <p><a href={source}>Swedish cavalry uniforms, pp. 38–39 ↗</a> · <a href="https://historicalsoldiers.com/">Historical Soldiers visual reference ↗</a></p></details>
    </div></main><SiteFooter /></>;
}
