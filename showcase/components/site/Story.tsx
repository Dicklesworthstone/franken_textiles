import type { ElementType } from "react";
import { repo, workbench } from "./links";
import "./site.css";

// The numbers and file names on this page come from the repository README and from running
// `ftx export all` on the 92 cm chart bolero. Change the copy only together with its source.

const steps = [
  {
    title: "Measure",
    text: "Start with what you know about the wearer: a few tape measurements, a photograph with a sheet of paper for scale, or only an age and a height. Each value keeps its uncertainty and where it came from.",
    gives: "Measurements with a source",
    link: [workbench("photo"), "Measure from a photo"],
  },
  {
    title: "Fit a body",
    text: "A body model that covers newborn to adult is fitted to those measurements, read the way a tape reads them. Anything it fills in without a measurement is labelled as the model’s estimate.",
    gives: "A fitted body and what it could not match",
    link: ["/wardrobe/sundress/", "Change the figure"],
  },
  {
    title: "Draft or trace",
    text: "Draft a garment to the measurements, alter a pattern you already have, or trace a drawn or scanned sheet. Every seam is walked and every fit point is checked on the flat pattern.",
    gives: "A pattern whose seams match",
    link: ["/#studio", "Draft a bolero"],
  },
  {
    title: "Sew and drape",
    text: "A cloth solver sews the pieces together around the fitted body, with contact and friction. It reports whether the seams close, where the cloth runs tight, and whether an elastic holds.",
    gives: "A draped garment and its flags",
    link: ["/#drape-room", "Watch it sew"],
  },
  {
    title: "Cut and stitch",
    text: "Print tiled pages at home, send one large sheet to a copy shop, or hand a cutting service its DXF. Plotters, cutting machines and embroidery machines each get their own file, and every file is checked before it is written.",
    gives: "Files for people and machines",
    link: ["/#studio", "Lay out the roll"],
  },
] as const;

export function Pipeline({ heading: Heading = "h2", more = true }: { heading?: ElementType; more?: boolean }) {
  return <section className="ft-pipeline" id="how" aria-labelledby="how-title"><div className="wrap">
    <div className="ft-section-top">
      <div><p className="kicker">From a tape measure to cut cloth</p>
        <Heading id="how-title" className="section-title">How a pattern gets made<span>.</span></Heading></div>
      <p>Five steps, and each one shows its work.{more && <> <a href="/how-it-works/">Read how it works <span aria-hidden="true" className="arrow"/></a></>}</p>
    </div>
    <ol className="ft-steps">
      {steps.map((step, index) => <li key={step.title} data-reveal>
        <span className="ft-step-no mono">{String(index + 1).padStart(2, "0")}</span>
        <h3>{step.title}</h3>
        <p>{step.text}</p>
        <p className="ft-step-out"><span className="mono">Gives</span>{step.gives}</p>
        <a className="ft-step-link" href={step.link[0]}>{step.link[1]} <span aria-hidden="true" className="arrow"/></a>
      </li>)}
      <li className="ft-step-last" data-reveal>
        <span className="ft-step-no mono">Then</span>
        <h3>Make a muslin</h3>
        <p>Nothing computed can show that a garment fits a person. Sew a test garment in cheap cloth before you cut the good fabric.</p>
      </li>
    </ol>
  </div></section>;
}

const facts = [
  ["21", "tape measurements the body model can be fitted to"],
  ["51", "acceptance checks on the exported files"],
  ["Bit-identical", "results on the command line and in the browser"],
  ["Nothing", "is uploaded: every calculation runs on your device"],
] as const;

export function Manifesto() {
  return <section className="ft-manifesto" aria-labelledby="what-title"><div className="wrap">
    <p className="kicker" id="what-title">What FrankenTextiles is</p>
    <p className="ft-statement" data-reveal>Sewing patterns that <em>demonstrably sew together</em> and fit a specific body,
      with the files people and machines need to make them.</p>
    <div className="ft-manifesto-body">
      <p data-reveal>An image model will happily draw a convincing pattern sheet. One that was audited had side seams 8% apart
        and a sleeve cap too short to set into its armhole. A pattern has to be checked as geometry, and then on a body,
        before anyone cuts cloth. FrankenTextiles does both, in Rust, on your own device.</p>
      <dl className="ft-facts">{facts.map(([value, label]) => <div key={value} data-reveal><dt>{value}</dt><dd>{label}</dd></div>)}</dl>
    </div>
  </div></section>;
}

const files = [
  ["PDF", "A4 and Letter tiles", "Print at home at 100% and check the two calibration squares.", "Home printer"],
  ["PDF", "A0 and 36 × 48 in sheets", "One large sheet with every piece at full scale.", "Copy shop"],
  ["PDF", "Projector sheets", "Light and dark versions for projecting onto the cloth.", "Projector"],
  ["SVG", "Master drawing", "The whole pattern in millimetres, for editing or archiving.", "Vector editor"],
  ["DXF", "ASTM and AAMA", "The interchange files that cutting services and pattern CAD expect.", "Cutting service"],
  ["PLT", "HPGL plot", "Pen-plotter instructions for the cut and stitch lines.", "Plotter"],
  ["SVG", "One file per piece", "Sized for a desktop cutting machine, with a report of what fits.", "Cutting machine"],
  ["DST", "Tajima embroidery", "The braid path, with a stop where you lay the cord.", "Embroidery machine"],
  ["JSON", "GarmentCode", "Panels and stitches for exchange with GarmentCode tools.", "Other software"],
] as const;

export function Deliverables({ heading: Heading = "h2" }: { heading?: ElementType }) {
  return <section className="ft-files" id="files" aria-labelledby="files-title"><div className="wrap">
    <div className="ft-section-top">
      <div><p className="kicker">What it writes</p>
        <Heading id="files-title" className="section-title">One pattern, every file<span>.</span></Heading></div>
      <p>A single command exports them all. For the 92 cm bolero it writes 25 files and passes 183 of 183 checks.</p>
    </div>
    <ul className="ft-file-grid">{files.map(([kind, name, text, user]) => <li key={name} data-reveal>
      <span className="ft-file-kind mono">{kind}</span>
      <h3>{name}</h3><p>{text}</p><span className="ft-file-user mono">{user}</span>
    </li>)}</ul>
  </div></section>;
}

const claims = [
  ["Parity", "A ported part behaves like the reference it replaces.", "Fixtures captured from the pinned reference, never regenerated from the port."],
  ["Physics", "The solver behaves like cloth where the answer is known.", "Known-answer tests: a ring on a cylinder, a ring on a cone, a cantilever strip."],
  ["Fit", "A garment fits a real person.", "A muslin or a photograph of the garment worn. Nothing computed does."],
  ["Performance", "A workload runs in a stated time.", "A benchmark that names its machine, its inputs and its spread."],
] as const;

export function Claims({ heading: Heading = "h2" }: { heading?: ElementType }) {
  return <section className="ft-claims" id="claims" aria-labelledby="claims-title"><div className="wrap">
    <div className="ft-section-top">
      <div><p className="kicker">What a result means</p>
        <Heading id="claims-title" className="section-title">Four claims, kept apart<span>.</span></Heading></div>
      <p>They are never averaged into one score. A drape with no warning flags is a prediction, not a fit.</p>
    </div>
    <ol className="ft-claim-grid">{claims.map(([name, meaning, evidence]) => <li key={name} data-reveal data-claim={name.toLowerCase()}>
      <h3>{name}</h3><p>{meaning}</p>
      <p className="ft-claim-proof"><span className="mono">Established by</span>{evidence}</p>
    </li>)}</ol>
  </div></section>;
}

export function StartHere({ heading: Heading = "h2" }: { heading?: ElementType }) {
  return <section className="ft-start" aria-labelledby="start-title"><div className="wrap ft-start-grid">
    <div><p className="kicker">Start here</p>
      <Heading id="start-title" className="section-title">Open a workbench, or build the tool<span>.</span></Heading>
      <p>The workbenches need nothing installed and open with a working example. The command-line tool runs the same Rust code and writes every file.</p>
      <div className="ft-start-actions">
        <a className="primary" href="/#workbenches">Open a workbench <span aria-hidden="true" className="arrow arrow-diagonal"/></a>
        <a className="ft-text-link" href="/get-started/">Build the command-line tool <span aria-hidden="true" className="arrow"/></a>
      </div></div>
    <figure className="ft-code" aria-label="Example commands">
      <figcaption className="mono">A first pattern, for a synthetic 92 cm wearer</figcaption>
      <pre><code><span className="c"># reference measurements from population data</span>{"\n"}
ftx measure reference --stature-mm 920 --age-months 30 --out m.json{"\n\n"}
<span className="c"># draft the bolero, then write every file</span>{"\n"}
ftx draft zouave_bolero --measurements m.json --out bolero{"\n"}
ftx export all bolero/pattern.json --out bolero/files{"\n"}
<span className="o">PASS: 183/183 checks</span></code></pre>
      <a className="ft-code-more" href={repo}>Source on GitHub <span aria-hidden="true" className="arrow arrow-diagonal"/></a>
    </figure>
  </div></section>;
}
