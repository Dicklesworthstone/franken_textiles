"use client";

import { useState } from "react";
import CodeBlock from "./CodeBlock";
import { repo } from "./links";

const cases = [
  {
    id: "sleeve",
    kicker: "Case 01 · Sleeve length & hand contact",
    tab: "Shortening a sleeve",
    summary: "Shorten 385 → 335 mm, catch a hidden hand crossing",
    headline: "Trace a cuff pixel straight to its flat edge.",
    lead: "When a simulated sleeve covers the fingers, a vision-capable agent picks a pixel on the rendered PNG, queries ftx inspect-pixel to identify sleeve.L / hem, shortens the draft by 50 mm, and compares both rounds under a shared camera on the same body.",
    image: "/fittings/sleeve-contact-review.png",
    width: 2000,
    height: 777,
    alt: "Two bolero cuffs from the same camera beside their overlaid flat sleeve outlines. Both archived wrist-collider trajectories failed the full-body contact audit at frame 87.",
    caption: "Shared-camera comparison of round 000 (385 mm, flat NOT_CONVERGED) and round 001 (335 mm, flat CONVERGED), aligned beside the flat sleeve.L overlay.",
    inset: {
      src: "/fittings/sleeve-knuckle-review.png",
      width: 166,
      height: 268,
      alt: "The revised 335 mm sleeve ending just above the hand in the knuckle-collider run.",
      note: "Re-running the 335 mm cut with opt-in --hands knuckle passed the flat check and all 49 physical frames. The 385 mm cut over the hand still refuses honestly — nothing is masked.",
    },
    metrics: [
      ["Pixel query", "597, 415 → sleeve.L / hem"],
      ["Flat edit", "385 mm → 335 mm (−50 mm)"],
      ["Audit result", "335 mm clears 49/49 frames (--hands knuckle)"],
    ],
    commandTitle: "Inspect a pixel, revise, and audit contact",
    command: `# map a cuff pixel on the rendered PNG back to pattern.json
ftx inspect-pixel work/rounds/000 \\
  --view front --pixel 597,415 --json
> hit.instance = "sleeve.L"  hit.nearest_edge = "hem"

# redraft shorter and re-run with knuckle contact
ftx round work --pattern shorter/pattern.json \\
  --stills --self-contact-mm 2 --hands knuckle
ftx viewer work/rounds/001/sim --check-contact --json`,
    links: [
      { href: "/fittings/bolero-revision-A4.pdf", label: "Download revised A4 PDF ↓", download: true },
      { href: "/how-it-works/#visual-fitting", label: "Read the full walkthrough →", download: false },
      { href: `${repo}/blob/main/.agents/skills/visual-pattern-fitting/SKILL.md`, label: "Visual fitting skill ↗", download: false },
    ],
  },
  {
    id: "raised",
    kicker: "Case 02 · Refused start at 90° abduction",
    tab: "Clearing a raised arm",
    summary: "Diagnose intersecting_start before solving, shift wrap axes",
    headline: "Refuse a crossing start with a picture of where.",
    lead: "Raising both arms to 90° on a 54 cm chest caused six pairs of cloth triangles to cross before sewing began. Instead of silently exploding or launching cloth, FrankenTextiles refused with diagnostic stills marking the crossing in yellow — letting the cut lines stay untouched while shifting the wrap axes 30 mm.",
    image: "/fittings/bolero-raised-start.png",
    width: 2000,
    height: 683,
    alt: "Two back views of the raised-arm bolero: round 000 refused at start with a yellow cloth-crossing mark at the right shoulder, round 001 settled after shifting starting placement axes by 30 mm, beside coinciding flat back outlines.",
    caption: "Round 000 refused at placement (intersecting_start, yellow crossing mark at right armhole) vs round 001 settled after a 30 mm wrap-axis shift with identical cutting lines.",
    metrics: [
      ["Refusal code", "intersecting_start (6 crossing pairs)"],
      ["Placement fix", "+30 mm along wrap axes (0 mm cut change)"],
      ["Settled report", "28 mm ride-up · 1.6% mean chest strain"],
    ],
    commandTitle: "Preview placements before running the solver",
    command: `# render wrap & pre-sewn start stills in seconds, no solver
ftx place bolero.json --measurements chest-540.json \\
  --arm-deg 90 --out start-check

# inspect the yellow crossing pixel on the refused start still
ftx inspect-pixel work/rounds/000 \\
  --view back --pixel 642,218 --json`,
    links: [
      { href: "/sections/studies/?chest=54&arm=90&surface=cloth#drape-room", label: "Turn the raised-arm bolero in 3D ↗", download: false },
      { href: "/exhibits/poses/bolero-start.json", label: "Download revised placements ↓", download: true },
      { href: "/how-it-works/#raised-arm-start", label: "Read the start diagnosis →", download: false },
    ],
  },
] as const;

const loop = [
  ["01", "Brief & draft", "Start from a garment brief, measured draft, traced scan, or GarmentCode JSON."],
  ["02", "Sew & render", "ftx round fits the Anny body, solves contact drape, and writes multi-angle stills."],
  ["03", "Pixel → pattern", "ftx inspect-pixel traces any rendered pixel straight to its piece and named edge."],
  ["04", "Alter & audit", "Edit via ftx alter or style params, compare under a shared camera, audit every frame."],
] as const;

export default function VisualFitting() {
  const [selected, setSelected] = useState<(typeof cases)[number]["id"]>("sleeve");
  const current = cases.find(c => c.id === selected) ?? cases[0];
  return <section className="ft-fitting" id="visual-fitting" aria-labelledby="fitting-title"><div className="wrap">
    <div className="ft-section-top">
      <div><p className="kicker">Sewist &amp; vision-agent loop</p>
        <h2 id="fitting-title" className="section-title">Look at the drape, edit the flat cut<span>.</span></h2></div>
      <p>Every round keeps its inputs, stills, checks and refusal diagnostics in its own directory, so a sewist and an AI agent can compare cuts without overwriting history.</p>
    </div>

    <ol className="ft-loop" aria-label="Visual fitting loop">
      {loop.map(([num, title, desc]) => <li key={num} data-reveal>
        <span className="mono">{num}</span><strong>{title}</strong><p>{desc}</p>
      </li>)}
    </ol>

    <div className="ft-case-tabs" role="tablist" aria-label="Visual fitting case studies">
      {cases.map(item => <button key={item.id} type="button" role="tab" id={`fit-tab-${item.id}`}
        aria-selected={current.id === item.id} aria-controls="fit-case-panel"
        onClick={() => setSelected(item.id)}>
        <span className="mono">{item.kicker}</span>
        <strong>{item.tab}</strong>
        <small>{item.summary}</small>
      </button>)}
    </div>

    <div id="fit-case-panel" role="tabpanel" aria-labelledby={`fit-tab-${current.id}`} className="ft-case-board">
      <div className="ft-case-visual">
        <a className="ft-case-frame" href={current.image}>
          <img src={current.image} width={current.width} height={current.height} loading="lazy" alt={current.alt}/>
        </a>
        <p className="ft-case-cap">{current.caption}</p>
        {"inset" in current && current.inset && <figure className="ft-case-inset">
          <img src={current.inset.src} width={current.inset.width} height={current.inset.height} loading="lazy" alt={current.inset.alt}/>
          <figcaption><span className="mono">Follow-up · --hands knuckle</span>{current.inset.note}</figcaption>
        </figure>}
      </div>

      <div className="ft-case-copy">
        <p className="mono ft-case-eyebrow">{current.kicker}</p>
        <h3>{current.headline}</h3>
        <p className="ft-case-lead">{current.lead}</p>
        <dl className="ft-case-metrics">
          {current.metrics.map(([k, v]) => <div key={k}><dt className="mono">{k}</dt><dd>{v}</dd></div>)}
        </dl>
        <CodeBlock title={current.commandTitle} code={current.command}/>
        <div className="ft-case-links">
          {current.links.map(link => <a key={link.label} href={link.href} download={link.download || undefined}>{link.label}</a>)}
        </div>
      </div>
    </div>
  </div></section>;
}
