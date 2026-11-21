import type { ElementType } from "react";
import CodeBlock from "./CodeBlock";
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
  ["1e-14 m", "Anny 0.6.1 forward model vs upstream PyTorch (newborn to elderly, ~0.3–0.4 ms/body)"],
  ["21", "tape measurements read as convex-hull slices, with MODEL_FIT provenance on estimates"],
  ["0 mm inset", "IPC barrier cloth contact on real thickness, with Coulomb friction & 2 mm self-contact"],
  ["51", "acceptance checks across PDF, SVG, DXF, HPGL, cutter, GarmentCode & Tajima DST"],
  ["4.3 MiB gz", "pure Rust WebAssembly (#![forbid(unsafe_code)]), bit-identical to the native CLI"],
  ["449", "source-bearing textile records whose missing measurements stay absent, never guessed"],
] as const;

const contrasts = [
  ["Body geometry", "Stacked-ellipse dress forms whose armpit sat 45 mm too high and whose crotch needed a special case", "Parametric Anny 0.6.1 body (newborn → adult, Apache-2.0 / CC0) fitted by Levenberg–Marquardt to tape hulls, with opt-in Dual-Quaternion Skinning"],
  ["Cloth contact", "Blender cloth needing a collider inset (+4.4 cm girth inflation), start-state jitter, and no elastic-grip answer", "Incremental Potential Contact on real cloth thickness, exact welded seams, Coulomb friction, transactional Newton refusal & opt-in --time-subdivision"],
  ["Pattern geometry", "AI-drawn sheets with 8% side-seam mismatches, short sleeve caps, and 57% contradictory printed dimensions", "Pure-Rust raster tracer, round-0 auditor, flat seam walker, and five deterministic pattern alterations checked before anyone cuts cloth"],
  ["Runtime & privacy", "Python, OpenCV, Blender, and a 19 MB Pyodide runtime — or cloud upload workflows", "One native Rust binary and a 4.3 MiB gzipped WASM module running locally on your device with zero network uploads"],
] as const;

export function Manifesto() {
  return <section className="ft-manifesto" aria-labelledby="what-title"><div className="wrap">
    <p className="kicker" id="what-title">What FrankenTextiles is</p>
    <p className="ft-statement" data-reveal>Sewing patterns that <em>demonstrably sew together</em> and fit a specific body,
      with the files people and machines need to make them.</p>
    <div className="ft-manifesto-body">
      <div className="ft-manifesto-copy" data-reveal>
        <p>FrankenTextiles is the mechanical core of a fitting room for a sewist and a vision-capable AI agent. Start with a
          garment brief, a measured draft, a scanned pattern sheet, or evaluated GarmentCode panels.</p>
        <p>Every seam is walked on the flat pattern, then sewn around a real fitted body in Rust — reporting residuals,
          refusing unconverged steps instead of clamping silently, and writing every print and machine file with automated checks.</p>
      </div>
      <dl className="ft-facts">{facts.map(([value, label]) => <div key={value} data-reveal><dt>{value}</dt><dd>{label}</dd></div>)}</dl>
    </div>
    <div className="ft-contrast" data-reveal>
      <div className="ft-contrast-head">
        <span className="mono">Why rebuild the mechanical core</span>
        <span className="mono">Legacy pipeline</span>
        <span className="mono">FrankenTextiles (Rust &amp; WASM)</span>
      </div>
      {contrasts.map(([area, legacy, ftx]) => <div key={area} className="ft-contrast-row">
        <strong>{area}</strong>
        <p className="ft-contrast-old">{legacy}</p>
        <p className="ft-contrast-new">{ftx}</p>
      </div>)}
    </div>
  </div></section>;
}

const pillars = [
  {
    tag: "449 records · zero invented numbers",
    title: "Source-bearing textiles & shrinkage",
    body: "Browse measured linen, muslin, habotai silk, 33 Sperl 2022 production knits, 10 Wang 2011 specimens, 10 bovine leather groups, and 5 laundered denim pairs. Missing properties stay explicitly absent. Apply measured warp/weft wash shrinkage to compensate cutting patterns in the grain frame.",
    href: workbench("materials"),
    cta: "Open Material Workbench",
  },
  {
    tag: "Thermal · moisture · viscoelastic",
    title: "Heat, vapor & constitutive solvers",
    body: "Run 1D transient exterior convection and gray-diffuse radiation on layer stacks or saved drapes, inspect vapor resistance and compression thermal histories, lower Fractional-Zener curves via exact-exponential Prony recursion, and calibrate strip or cylinder fixtures with exact adjoint gradients.",
    href: "/get-started/#materials-cli",
    cta: "Try the thermal CLI",
  },
  {
    tag: "27 ChArUco families · local only",
    title: "Camera calibration & photogrammetry",
    body: "Print calibrated Letter or A4 ChArUco sheets, detect corners in pure Rust without OpenCV, recover plain-sheet photo scale with EXIF orientation and bootstrap σ, propagate Brown lens covariance, and run multi-frame articulated body fits with held-out CHECK gates.",
    href: workbench("photo"),
    cta: "Open Photo Workbench",
  },
  {
    tag: "GarmentCode · nesting · stitch cells",
    title: "Import, cut-room & sewing planners",
    body: "Import evaluated GarmentCode panels with explicit bounded circular-arc tolerances and REVIEW_REQUIRED reports. Plan finite-stock roll markers with planar knife/fixture clearance and drill cycles, allocate split supplier thread orders, and emit ordered multi-cell sewing schedules.",
    href: "/get-started/#command-line",
    cta: "Explore CLI workflows",
  },
] as const;

export function PlatformCapabilities({ heading: Heading = "h2" }: { heading?: ElementType }) {
  return <section className="ft-platform" id="capabilities" aria-labelledby="capabilities-title"><div className="wrap">
    <div className="ft-section-top">
      <div><p className="kicker">Beyond the fitting room</p>
        <Heading id="capabilities-title" className="section-title">Textiles, cameras &amp; production<span>.</span></Heading></div>
      <p>The same deterministic Rust engine extends from a single garment round into sourced fabric physics, calibrated photogrammetry, and cutting-room planning — without inventing missing evidence.</p>
    </div>
    <div className="ft-pillar-grid">
      {pillars.map(p => <article key={p.title} className="ft-pillar" data-reveal>
        <span className="mono ft-pillar-tag">{p.tag}</span>
        <h3>{p.title}</h3>
        <p>{p.body}</p>
        <a className="ft-step-link" href={p.href}>{p.cta} <span aria-hidden="true" className="arrow"/></a>
      </article>)}
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
  ["JSON", "GarmentCode", "Export panels and stitches, or import evaluated GarmentCode JSON with circular-arc bounds.", "Bidirectional exchange"],
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

const quickstartCode = `# reference measurements for a synthetic 92 cm wearer
ftx measure reference --stature-mm 920 --age-months 30 --out m.json

# draft the bolero, validate flat seams, then write every file
ftx draft zouave_bolero --measurements m.json --out bolero
ftx validate bolero/pattern.json \\
  --measurements m.json --fit bolero/fit_spec.json
> STATUS: CONVERGED
ftx export all bolero/pattern.json --out bolero/files
> PASS: 183/183 checks`;

export function StartHere({ heading: Heading = "h2" }: { heading?: ElementType }) {
  return <section className="ft-start" aria-labelledby="start-title"><div className="wrap ft-start-grid">
    <div><p className="kicker">Start here</p>
      <Heading id="start-title" className="section-title">Open a workbench, or build the tool<span>.</span></Heading>
      <p>The workbenches run compiled Rust in your browser with nothing installed and open with a working example. The command-line tools (<code>ftx</code> and <code>ftx-materials</code>) run the exact same crates locally and write every manufacturing file.</p>
      <div className="ft-start-actions">
        <a className="primary" href="/#workbenches">Open a workbench <span aria-hidden="true" className="arrow arrow-diagonal"/></a>
        <a className="ft-text-link" href="/get-started/">Build the command-line tool <span aria-hidden="true" className="arrow"/></a>
        <a className="ft-text-link" href={repo}>Source on GitHub <span aria-hidden="true" className="arrow arrow-diagonal"/></a>
      </div></div>
    <CodeBlock title="A first pattern, for a synthetic 92 cm wearer" code={quickstartCode}/>
  </div></section>;
}
