import type { Metadata } from "next";
import SitePage from "../../components/site/SitePage";
import BoleroReplay from "../../components/BoleroReplay";
import CodeBlock from "../../components/site/CodeBlock";
import { repo, workbenches } from "../../components/site/links";

export const metadata: Metadata = {
  title: "Get started · FrankenTextiles",
  description: "Open a FrankenTextiles workbench in your browser with nothing to install, or build the ftx command-line tool and draft, check and export a first sewing pattern.",
  alternates: { canonical: "/get-started/" },
};

const thumbs = ["drafting.webp", "trace-review.webp", "material-rig.webp", "photo-example-sleeve.jpg"];

const build = `
git clone ${repo}
cd franken_textiles
cargo build --release -p ftx-cli
export PATH="$PWD/target/release:$PATH"
`;
const draft = `
# reference measurements for a synthetic 92 cm wearer, from population data
ftx measure reference --stature-mm 920 --age-months 30 --out m.json

# draft the bolero to those measurements
ftx draft zouave_bolero --measurements m.json --out bolero
`;
const check = `
ftx validate bolero/pattern.json --measurements m.json --fit bolero/fit_spec.json
> STATUS: CONVERGED
`;
const exportAll = `
ftx export all bolero/pattern.json --out bolero/files
> PASS: 183/183 checks
`;
const own = `
ftx measure init --label demo --stature 92cm --age-months 30 --out mine.json
ftx measure add mine.json --name chest --value 52cm
`;
const materialsDemo = `
# browse the 449 built-in source-bearing textile records
ftx materials list --json

# run the self-contained synthetic transient exterior heat example
ftx materials thermal-transient \\
  tests/fixtures/materials/synthetic-transient-exterior.json
`;
const commands = [
  ["ftx draft", "Draft a bolero, trousers or a fez to measurements or a fitted Anny body."],
  ["ftx validate", "Walk every seam and check every fit point on the flat pattern."],
  ["ftx alter", "Slash and spread, move a vertex, true a seam, ease a sleeve cap, or transfer a curve."],
  ["ftx import garmentcode", "Convert evaluated GarmentCode panels (with explicit circular-arc tolerances) to native pattern.json."],
  ["ftx trace", "Trace a PNG, JPEG or WebP pattern sheet in pure Rust; pair with ftx annotate, scale-fit and audit."],
  ["ftx body", "Fit the Anny 0.6.1 body model to tape measurements or multi-frame reports (supports --skinning lbs|dqs)."],
  ["ftx place", "Preview wrapped and pre-sewn starting placements as stills in seconds, without running the solver."],
  ["ftx drape", "Sew a pattern or multi-garment outfit with IPC contact, friction, --time-subdivision and --hands knuckle."],
  ["ftx render", "Draw a finished drape's six stills. --shading legible adds a second set for reading folds and hems: a drawing aid that describes no fabric."],
  ["ftx inspect-pixel", "Map any rendered PNG pixel deterministically back to its physical panel and nearest flat pattern edge."],
  ["ftx viewer", "Build an offline 3D fold-up HTML page or audit every recorded frame with --check-contact."],
  ["ftx qualify", "Run three chest qualification bands (−20 / 0 / +20 mm) around a fitted torso collider."],
  ["ftx export", "Write tiled PDFs, copy-shop & projector PDFs, SVG, DXF, HPGL, cutter, GarmentCode and DST + 51 check rules."],
  ["ftx round", "Run the measurement gate, flat checks, drape/stills and exports in one immutable numbered directory."],
  ["ftx measure", "Manage measurements, generate ChArUco sheets, calibrate lenses, scale photos and fit bodies from frames."],
  ["ftx materials / ftx-materials", "Inspect 449 sourced fabrics, apply wash shrinkage, run thermal/strip/cylinder physics and plan production."],
] as const;

export default function GetStarted() {
  return <SitePage>
    <section className="ft-hero"><div className="wrap ft-hero-grid">
      <div><p className="kicker">Get started</p>
        <h1>Open a workbench, or <em>build the tool</em>.</h1></div>
      <div><p className="ft-lead">The workbenches run in your browser and need nothing installed. The command-line tool runs the same Rust code
        on your own machine and writes every file a sewist, a copy shop or a cutting service asks for.</p>
        <div className="ft-hero-actions"><a className="primary" href="#workbenches">In your browser <span aria-hidden="true" className="arrow"/></a>
          <a className="ft-text-link" href="#command-line">On the command line <span aria-hidden="true" className="arrow"/></a></div></div>
    </div></section>
    <div className="ft-tape" aria-hidden="true"/>

    <section className="ft-block" id="workbenches"><div className="wrap ft-block-grid">
      <div className="ft-block-head"><p className="kicker">Nothing to install</p><h2>Four workbenches.</h2>
        <p>Each opens with a working example. Open your own files when you are ready. They stay on your device.</p></div>
      <ul className="ft-shelf">{workbenches.map((item, index) => <li key={item.href} data-reveal><a href={item.href}>
        <img src={`/workbench/atelier-assets/${thumbs[index]}`} alt="" width="112" height="84" loading="lazy"/>
        <span><strong>{item.label}</strong><small>{item.note}</small></span></a></li>)}</ul>
    </div></section>

    <section className="ft-block" id="command-line"><div className="wrap ft-block-grid">
      <div className="ft-block-head"><p className="kicker">Command line</p><h2>A first pattern in four steps.</h2>
        <p>You need Git and Rust. The repository pins its own Rust version, and <code>rustup</code> fetches it on the first build.</p></div>
      <div className="ft-stack">
        <div><p className="ft-step-label"><b>01</b><span>Get the source and build the tool.</span></p><CodeBlock title="Build" code={build}/></div>
        <div><p className="ft-step-label"><b>02</b><span>Draft a bolero from synthetic reference measurements.</span></p>
          <CodeBlock title="Draft" code={draft}/></div>
        <div><p className="ft-step-label"><b>03</b><span>Check it flat. Every seam is walked and each fit point is compared with the body.</span></p>
          <CodeBlock title="Check" code={check}/></div>
        <div><p className="ft-step-label"><b>04</b><span>Write every file: tiled PDFs, copy-shop and projector sheets, SVG, DXF, HPGL, cutter and embroidery files.</span></p>
          <CodeBlock title="Export" code={exportAll}/></div>
      </div>
    </div></section>

    <section className="ft-block"><div className="wrap ft-block-grid">
      <div className="ft-block-head"><p className="kicker">Your own wearer</p><h2>Start from a tape measure.</h2></div>
      <div className="ft-stack">
        <div className="ft-prose"><p>Begin a measurements file with a nickname, a height and an age, then add each measurement you take.
          Use a nickname, and keep the file out of anything you publish.</p></div>
        <CodeBlock title="Measure" code={own}/>
        <div className="ft-prose"><p>With only a height, the flat check answers <code>STATUS: BLOCKED</code> and lists each fit point it could not
          measure. It does not guess. Add the measurements it names and run it again.</p></div>
      </div>
    </div></section>

    <section className="ft-block"><div className="wrap ft-block-grid">
      <div className="ft-block-head"><p className="kicker">In three dimensions</p><h2>Sew it in simulation.</h2></div>
      <div className="ft-stack">
        <div className="ft-prose"><p>Replay the gallery’s five-piece bolero on its 52 cm chest form. This example uses the
          gallery cut and its checked starting placements. Convert the pinned body model once using
          {" "}<a href={repo + "/tree/main/tools/body-model"}>the body-model instructions</a>. If it lives outside the default
          location, point <code>FTX_ANNY_MODEL</code> at the converted file.</p></div>
        <BoleroReplay chest={52} angle={35} repository/>
        <div className="ft-prose"><p>The output folder holds the report and <code>fold_viewer.html</code>, a single page that replays the
          fold in three dimensions. A result with no flags is a prediction. <a href="/how-it-works/#claims">It is not a fit</a>.</p></div>
      </div>
    </div></section>

    <section className="ft-block" id="materials-cli"><div className="wrap ft-block-grid">
      <div className="ft-block-head"><p className="kicker">Materials &amp; heat</p><h2>Inspect fabrics and run thermal studies.</h2>
        <p>Every record retains its original observations and names its mechanical or thermal gaps instead of inventing numbers.</p></div>
      <div className="ft-stack">
        <div className="ft-prose"><p>Browse the 449 built-in records (linen, muslin, habotai silk, 33 Sperl 2022 knits, 10 Wang 2011 specimens, bovine leather groups and laundered denim pairs), or run the self-contained synthetic transient exterior heat example (its numbers demonstrate the solver, not a real fabric).</p></div>
        <CodeBlock title="Sourced textile catalog & transient heat" code={materialsDemo}/>
      </div>
    </div></section>

    <section className="ft-block"><div className="wrap ft-block-grid">
      <div className="ft-block-head"><p className="kicker">Reference</p><h2>The commands.</h2>
        <p>Each prints plain text, or a stable JSON envelope with <code>--json</code>. Run <code>ftx --help</code> or <code>ftx-materials --help</code> for every option.</p></div>
      <dl className="ft-rows">{commands.map(([name, text]) => <div key={name}><dt><code>{name}</code></dt><dd>{text}</dd></div>)}</dl>
    </div></section>

    <section className="ft-block"><div className="wrap ft-next">
      <a href="/how-it-works/"><span className="mono">Read</span><strong>How it works</strong>
        <span>The body model, the cloth solver, and what each result does and does not claim.</span></a>
      <a href={repo}><span className="mono">Source</span><strong>GitHub</strong>
        <span>The code, the design plan, the parity status and the licence.</span></a>
    </div></section>
  </SitePage>;
}
