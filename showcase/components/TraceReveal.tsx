"use client";

import { useState } from "react";
import "./trace-reveal.css";

export default function TraceReveal({heading:Heading="h2"}: {heading?:"h1"|"h2"}) {
  const [reveal, setReveal] = useState(53);
  return <section className="trace-reveal" id="tracing" aria-labelledby="trace-title"><div className="wrap trace-layout">
    <div className="trace-about">
      <p className="kicker">Raster tracing</p>
      <Heading className="section-title" id="trace-title"><a href="/sections/tracing/">Read the <em>lines.</em></a></Heading>
      <p>Slide across this synthetic bolero sheet to see the piece outlines, corners, and grain directions recovered by the Rust tracer.</p>
      <dl><div><dt>Pieces found</dt><dd>3</dd></div><div><dt>Scale</dt><dd>Unscaled</dd></div></dl>
      <a href="/workbench/trace.html?sample=bolero" className="trace-open">Trace this sheet <span aria-hidden="true">↗</span></a>
      <a className="trace-source" href="/labs/trace-bolero.png" download="bolero-practice-sheet.png">Download source PNG ↓</a>
      <a className="trace-source" href="/patterns/bolero.pdf" download>Full-size bolero sewing pattern (A4 PDF) ↓</a>
    </div>
    <figure>
      <div className="trace-compare">
        <img src="/labs/trace-bolero.png" alt="Back, right front, and sleeve drawn on a gridded paper sheet" loading="lazy" width="1216" height="594"/>
        <img className="trace-layer" src="/labs/trace-overlay.webp" alt="Native tracing review with color-coded edges and corner labels" loading="lazy"
          width="1216" height="594" style={{ clipPath: `inset(0 ${100-reveal}% 0 0)` }}/>
        <div className="trace-divider" aria-hidden="true" style={{left:`${reveal}%`}}><span>↔</span></div>
        <input className="trace-slider" type="range" min="0" max="100" value={reveal} step="1"
          onChange={e=>setReveal(Number(e.target.value))} aria-label="Reveal traced contours" aria-valuetext={`${reveal}% traced view`}/>
      </div>
      <figcaption><button onClick={()=>setReveal(100)}>Rust trace</button><span>Drag the divider</span><button onClick={()=>setReveal(0)}>Source sheet</button></figcaption>
    </figure>
  </div></section>;
}
