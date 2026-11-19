import type { Metadata } from "next";
import SitePage from "../../components/site/SitePage";
import { Claims, Pipeline } from "../../components/site/Story";
import { repo } from "../../components/site/links";

export const metadata: Metadata = {
  title: "How it works · FrankenTextiles",
  description: "Follow a visual fitting from a long bolero sleeve to a revised sewing pattern. Inspect the simulated garment, find the flat edge, and compare two cuts on the same body.",
  alternates: { canonical: "/how-it-works/" },
};

// Figures and statuses below are taken from the repository README (2026-11-18). Update them together.
const gates = [
  ["G1 · Body", "The Rust body model stays within 0.05 mm of the model it was ported from, and fitting recovers synthetic tape targets within 3 mm.", "Passed 12 October 2026", "passed"],
  ["G2 · Drape", "The known-answer tests pass, and real garments settle on fitted bodies within budget.", "Passed historically · qualification open", "open"],
  ["G3 · Port", "Patterns, validation, drafting and exports pass their fixtures, and a full round runs through the command-line tool.", "Passed 13 October 2026", "passed"],
  ["G4 · Browser", "The browser build matches the reference on its 25 cases and drapes garments bit-identically to the command line.", "Passed historically · qualification open", "open"],
  ["G5 · Gauntlet", "A multi-round audit of parity, physics and the whole surface.", "Required before version 1.0", "open"],
] as const;

const crates = [
  ["ftx-core", "Units, deterministic maths, curves, polygons, schemas and structured refusals."],
  ["ftx-pattern", "The pattern model, validation, alteration, drafting and the sourced material catalogue."],
  ["ftx-body", "The Anny body model, tape-style measuring, fitting and poses."],
  ["ftx-measure", "The measurement catalogue, priors, fusion, readiness, photo scale and photogrammetry."],
  ["ftx-drape", "The garment scene, the contact cloth solver, heat and moisture transport, metrics and flags."],
  ["ftx-export", "SVG, PDF, DXF, HPGL, cutter files, GarmentCode, DST, cutting and sewing planners, acceptance checks."],
  ["ftx-trace", "Tracing pattern sheets, printable calibration sheets and image decoders."],
  ["ftx-cli", "The ftx and ftx-materials command-line tools."],
  ["ftx-wasm", "Browser bindings for the workbenches."],
  ["ftx-conformance", "Reference, known-answer, property and determinism tests."],
] as const;

const faq = [
  ["Does a passing result mean the garment fits?", "No. It means the pattern is consistent, meets its fit targets on the flat, and sews together on the fitted body without a warning flag. Make a muslin."],
  ["Why a simulation at all, if only a muslin proves fit?", "It catches the failures that waste cloth: seams of different lengths, a sleeve cap that cannot be set in, trousers that slide down because the elastic is too long. Those are cheaper to find before cutting."],
  ["Why the Anny body model?", "It covers newborn to elderly, and its code and assets are licensed for any use (Apache 2.0 and CC0). The better-known alternatives are restricted to non-commercial use or cover infants only."],
  ["Why does it run on the processor and not the graphics card?", "A garment here is a few thousand points. Getting contact and friction right, and getting the same answer every time, matters more than raw speed. A processor-based solver also runs unchanged in a browser."],
  ["Why Rust?", "It gives a solver that returns the same result on every machine, a single program to install, and a browser build with nothing else to download. The earlier Python version remains the reference each ported part is checked against."],
  ["What does it cost?", "Nothing. The source is public under the MIT licence with an added rider; read the licence file for the terms."],
] as const;

export default function HowItWorks() {
  return <SitePage>
    <section className="ft-hero"><div className="wrap ft-hero-grid">
      <div><p className="kicker">How it works</p>
        <h1>From a tape measure to <em>cut cloth</em>.</h1></div>
      <div><p className="ft-lead">A sewist sets the brief. A vision-capable agent inspects the dressed body, edits the flat
        pattern and tries another cut. FrankenTextiles sews each version in simulation and keeps the views, seam checks
        and files that let you decide what to make.</p>
        <div className="ft-hero-actions"><a className="primary" href="/get-started/">Get started <span aria-hidden="true" className="arrow"/></a>
          <a className="ft-text-link" href="/#drape-room">See a garment sewn <span aria-hidden="true" className="arrow"/></a></div></div>
    </div></section>
    <div className="ft-tape" aria-hidden="true"/>

    <section id="visual-fitting" className="ft-block"><div className="wrap ft-block-grid">
      <div className="ft-block-head"><p className="kicker">A visual fitting</p><h2>Let the hands show.</h2>
        <p>Two bolero cuts, one reference body and one change to the flat pattern.</p></div>
      <div className="ft-prose">
        <a href="/fittings/sleeve-revision.png"><img src="/fittings/sleeve-revision.png" width="2000" height="683"
          style={{width:"100%",height:"auto"}} alt="The original cuffs cover the hands. The revised cuffs end near the wrists. The flat sleeve outlines share the same cap and differ at the hem."/></a>
        <p>Both drapes settled. In the first front view, most of each hand was still inside a sleeve.
          A pixel at the left cuff traced back to the <code>hem</code> edge of the <code>sleeve</code> pattern piece.
          Shortening the draft from 385 to 335 mm exposed the hands while keeping the sleeve cap and body cut.</p>
        <p>The revised sleeve is 15 mm longer than the reference arm. Both runs used the same pose, material and
          body measurements. A wearing trial would establish how the cuff sits on a person.</p>
        <div className="ft-hero-actions"><a className="ft-text-link" href="/fittings/bolero-revision-A4.pdf" download>
          Print the revised A4 pattern <span aria-hidden="true" className="arrow"/></a>
          <a className="ft-text-link" href={repo+"/blob/main/.agents/skills/visual-pattern-fitting/SKILL.md"}>
          Reproduce the fitting <span aria-hidden="true" className="arrow"/></a></div>
      </div>
    </div></section>

    <section className="ft-block"><div className="wrap ft-block-grid">
      <div className="ft-block-head"><p className="kicker">Why it exists</p><h2>A drawing is not a pattern.</h2></div>
      <div className="ft-prose">
        <p>An image model will happily draw a convincing pattern sheet for a child’s costume. The pieces look right, the labels look right,
          and the dimensions are printed neatly beside each edge. Then someone measures it.</p>
        <div className="ft-audit">
          <div data-reveal><strong>8%</strong><span>difference in length between two side seams that have to be sewn to each other</span></div>
          <div data-reveal><strong>57%</strong><span>largest contradiction between dimensions printed on the same sheet</span></div>
          <div data-reveal><strong>Too short</strong><span>a sleeve cap shorter than its armhole, which cannot be set in</span></div>
        </div>
        <p>A pattern has to be checked as geometry, and then on a body, before anyone cuts cloth. An earlier Python tool already did the
          first part. Its body was a stack of ellipses, though, and its cloth simulation could not say whether an elastic waistband holds.
          FrankenTextiles replaces both and rebuilds the rest in Rust, tested against that earlier tool case by case.</p>
      </div>
    </div></section>

    <Pipeline more={false}/>

    <section className="ft-block"><div className="wrap ft-block-grid">
      <div className="ft-block-head"><p className="kicker">The body</p><h2>A body, not a dress form.</h2>
        <p>The usual wearer is a small child, and a child is not a scaled-down adult.</p></div>
      <div className="ft-prose">
        <p>The body comes from <a href="https://github.com/naver/anny">Anny</a>, a parametric model from NAVER LABS Europe that covers newborn to
          elderly, with height for age calibrated to World Health Organization data. FrankenTextiles ports its forward model to Rust and matches
          the original on sixteen reference bodies.</p>
        <figure className="ft-figure" data-reveal><img src="/about/anny-bodies.jpg" width="1000" height="627" loading="lazy"
          alt="Four unclothed grey mannequin figures standing side by side with arms held out: a toddler, a child, a tall man and a woman."/>
          <figcaption>Four bodies from the model at average build: a girl of two and a half (94 cm), a boy of six (132 cm), a man of thirty (190 cm)
            and a woman of thirty (176 cm). All are synthetic.</figcaption></figure>
        <p>It is fitted from age, sex and height alone, from weight, or from any of <strong>21 tape measurements</strong>: chest, waist, hip, thigh,
          arm length, back length and more. A girth is read the way a tape reads it, bridging hollows instead of following them.</p>
        <p>The fit reports what it could not match. Every value it reads off the fitted body without a measurement behind it is
          labelled <code>MODEL_FIT</code>, never “measured”. That label matters: a body fitted to height alone came out 62 to 75 mm too small
          around the chest, waist and hip of the 92 cm reference toddler.</p>
      </div>
    </div></section>

    <section className="ft-block"><div className="wrap ft-block-grid">
      <div className="ft-block-head"><p className="kicker">The drape</p><h2>Sewn together, on that body.</h2>
        <p>The pieces leave the pattern board, wrap around the figure, join at their seams and settle under gravity.</p></div>
      <div className="ft-prose">
        <p>The cloth has its real thickness, stretches differently along and across its grain, and touches the body and itself without passing
          through. Seams are joined exactly. That lets the simulation answer the questions a sewist would ask.</p>
        <div className="ft-questions">
          <div data-reveal><h3>Does the elastic hold?</h3><p>A waistband is judged by the elastic’s grip on the body against the garment’s weight.
            On the trousers, an elastic no longer than the waist holds and one 20% longer does not, and the report says so.</p></div>
          <div data-reveal><h3>Is it tight?</h3><p>The earlier simulation had to shrink the body, or every girth gained 4.4 cm. This one does not,
            so clearance and strain can be read directly.</p></div>
          <div data-reveal><h3>Has it settled?</h3><p>Every solver step must converge, and the cloth must stay nearly still for three steps in a
            row. If it does not, the result carries a flag.</p></div>
        </div>
        <figure className="ft-figure" data-reveal><img src="/about/bolero-on-anny-92cm.jpg" width="1260" height="513" loading="lazy"
          alt="A blue cropped jacket with long sleeves on a grey toddler mannequin, shown from the front, the side and the back."/>
          <img src="/about/trousers-on-anny-92cm.jpg" width="1260" height="513" loading="lazy" style={{marginTop:12}}
          alt="Full blue trousers gathered at the waist and below the knee on the same mannequin, shown from the front, the side and the back."/>
          <figcaption>The drafted bolero and trousers on a body fitted to fifteen tape measurements of a synthetic reference wearer aged two and a half.
            Neither raises a fit flag, and the trousers’ waistband and cuffs end within 7 mm of where they were drafted to sit. On a body fitted to
            height alone, the same trousers slid 63 mm down.</figcaption></figure>
        <p>Where the correct answer is known from physics, the solver is tested against it: a ring of cloth on a cylinder holds below a
          calculated load and slides above it, and a hanging strip bends within 1.5% of the exact curve. The same garment gives the same
          result, bit for bit, on the command line and in a browser.</p>
      </div>
    </div></section>

    <Claims/>

    <section className="ft-block" id="status"><div className="wrap ft-block-grid">
      <div className="ft-block-head"><p className="kicker">Honest status</p><h2>Where it stands.</h2>
        <p>In development. The plan names gates that can stop the body port or the cloth solver early. As of 18 November 2026:</p></div>
      <div>
        <dl className="ft-rows">{gates.map(([name, text, status, state]) => <div key={name}>
          <dt>{name}</dt><dd>{text}<br/><span className="ft-status" data-state={state}>{status}</span></dd></div>)}</dl>
        <div className="ft-prose" style={{marginTop:24}}>
          <p>Two checks still fail: the strict fit check against chart sizes, and one fitted drape that no longer matches its recorded result
            bit for bit. Full qualification, including physical fit and performance, remains open.</p>
          <p>One timing has been measured properly. In a browser on an Apple M5 Ultra, over 20 runs each, the bolero drape took 16.7 s at the
            median and the trousers 46.0 s. That is one machine and one workload, and is not a comparison with other software.</p>
          <p><a href={`${repo}/blob/main/FEATURE_PARITY.md`}>Read the full parity status</a> for what is present, partial and missing.</p>
        </div>
      </div>
    </div></section>

    <section className="ft-block"><div className="wrap ft-block-grid">
      <div className="ft-block-head"><p className="kicker">Inside</p><h2>Ten Rust crates.</h2>
        <p>No unsafe code, and no Python, C or C++ at run time. The browser workbenches run the same crates.</p></div>
      <dl className="ft-rows">{crates.map(([name, text]) => <div key={name}><dt><code>{name}</code></dt><dd>{text}</dd></div>)}</dl>
    </div></section>

    <section className="ft-block"><div className="wrap ft-block-grid">
      <div className="ft-block-head"><p className="kicker">Scope and privacy</p><h2>What it is not.</h2></div>
      <div className="ft-prose">
        <ul>
          <li>Not a drawing program with a curve editor.</li>
          <li>Not a fashion renderer. The figures on this site are for inspection.</li>
          <li>Not a body scanner.</li>
          <li>Not a general cloth simulation library.</li>
          <li>Not a replacement for a muslin.</li>
        </ul>
        <p>Because the usual wearer is a young child, no real person’s measurements, photographs, videos or name appear anywhere in the project.
          Examples use synthetic bodies or a reference wearer built from published population data. Everything runs on your own device, and
          the workbenches upload nothing.</p>
      </div>
    </div></section>

    <section className="ft-block"><div className="wrap ft-block-grid">
      <div className="ft-block-head"><p className="kicker">Questions</p><h2>Asked often.</h2></div>
      <div className="ft-faq">{faq.map(([question, answer]) => <details key={question}><summary>{question}</summary><p>{answer}</p></details>)}</div>
    </div></section>

    <section className="ft-block"><div className="wrap ft-next">
      <a href="/get-started/"><span className="mono">Next</span><strong>Get started</strong>
        <span>Open a workbench in your browser, or build the command-line tool and draft a first pattern.</span></a>
      <a href="/#drape-room"><span className="mono">Or look around</span><strong>The exhibition</strong>
        <span>Sew a bolero onto a fitted body, pose a sundress, and pull cloth on the bias.</span></a>
    </div></section>
  </SitePage>;
}
