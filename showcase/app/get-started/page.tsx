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
# the program is now at target/release/ftx
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
const commands = [
  ["ftx draft", "Draft a bolero, trousers or a fez to a measurements file."],
  ["ftx validate", "Walk every seam and check every fit point on the flat pattern."],
  ["ftx alter", "Slash and spread, move a point, true a seam, ease a sleeve cap, or transfer a curve."],
  ["ftx trace", "Trace a PNG, JPEG or WebP pattern sheet into pieces for review."],
  ["ftx body", "Fit the body model to tape measurements and write the body on its own."],
  ["ftx drape", "Sew a pattern around the fitted body and report the fit flags."],
  ["ftx export", "Write the PDFs, SVG, DXF, HPGL, cutter and embroidery files, with their checks."],
  ["ftx round", "Run the measurement gate, the flat check, the drape and the exports as one numbered round."],
  ["ftx measure", "Start and extend a measurements file, or recover scale from a photograph."],
  ["ftx materials", "Inspect sourced cloth records and bind them to a pattern."],
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
      <div className="ft-block-head"><p className="kicker">Command line</p><h2>A first pattern in four commands.</h2>
        <p>You need Git and Rust. The repository pins its own Rust version, and <code>rustup</code> fetches it on the first build.</p></div>
      <div className="ft-stack">
        <div><p className="ft-step-label"><b>01</b><span>Get the source and build the tool.</span></p><CodeBlock title="Build" code={build}/></div>
        <div><p className="ft-step-label"><b>02</b><span>Draft a bolero. The examples call the program <code>ftx</code>; use its full path, or add it to your path.</span></p>
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

    <section className="ft-block"><div className="wrap ft-block-grid">
      <div className="ft-block-head"><p className="kicker">Reference</p><h2>The commands.</h2>
        <p>Each prints plain text, or a stable JSON envelope with <code>--json</code>. Run <code>ftx --help</code> for every option.</p></div>
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
