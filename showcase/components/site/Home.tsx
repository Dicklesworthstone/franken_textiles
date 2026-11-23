import { repo, workbench } from "./links";
import "./home.css";

// The home page below its opening film. It says what the tool does in the order a maker meets it, shows a
// handful of finished garments, and points to the rest of the site. The full catalogue of studies lives on
// the section pages (/sections/…) and in the header's index.

const steps = [
  {
    image: "/media/home/pattern.webp",
    alt: "The pattern pieces of a sundress drawn with cut lines, sewing lines and grain arrows.",
    title: "Draft it to your measurements",
    text: "Type in a few tape measurements or start from a size chart. You get pattern pieces with seam allowances, grain lines and notches, and every seam is checked against the one it will be sewn to.",
    href: workbench("index"),
    link: "Open the drafting workbench",
  },
  {
    image: "/media/construction-sundress-join.jpg",
    alt: "The cut pieces of the sundress rolled toward each other, with lines joining the edges that will be sewn.",
    title: "See it sewn before you cut",
    text: "The pieces are sewn together around a body with your measurements. You see where the cloth pulls, gapes or drags while the pattern is still free to change.",
    href: "/sections/wardrobe/",
    link: "Watch garments come together",
  },
  {
    image: "/media/cards/tiling.webp",
    alt: "Pattern pieces laid across tiled A4 pages with alignment marks at the page joins.",
    title: "Print it and cut",
    text: "Print tiled pages at home, one large sheet at a copy shop, or send a file to a projector or cutting machine. The pages carry alignment marks and a scale bar to check before you cut.",
    href: "/studies/tiling/",
    link: "Assemble the printed pages",
  },
] as const;

const garments = [
  { image: "sundress", name: "Garden sundress", note: "Four pieces, gathered at the waist", href: "/wardrobe/sundress/", pdf: "/sundress/pattern-A4.pdf" },
  { image: "blue", name: "Hussar jacket, 1761", note: "A braided Swedish cavalry dolman", href: "/wardrobe/hussar/", pdf: "/patterns/hussar-blue.pdf" },
  { image: "flare", name: "Flared skirt", note: "Six gores; compare it with a column cut", href: "/studies/skirt/", pdf: "/atelier/flare.pdf" },
  { image: "ensemble", name: "Bolero and trousers", note: "Two garments sewn and draped together", href: "/sections/studies/?outfit=joint#drape-room", pdf: "/patterns/bolero.pdf" },
  { image: "trousers", name: "Gathered trousers", note: "Elastic waist and cuffs, tried standing and in a squat", href: "/studies/trousers/", pdf: "/patterns/trousers.pdf" },
  { image: "bolero", name: "Zouave bolero", note: "A cropped jacket in five pieces", href: "/sections/studies/#drape-room", pdf: "/patterns/bolero.pdf" },
] as const;

const loop = [
  ["Look", "Every simulated try is drawn as six pictures from fixed viewpoints, with a second set made for reading folds and hems."],
  ["Point", "Pick any pixel on a picture. The tool answers with the pattern piece and the edge that pixel belongs to."],
  ["Change", "Shorten, widen or redraft that edge, sew it again, and compare the two tries from the same camera."],
] as const;

const tools = [
  { page: "index", image: "/media/home/pattern.webp", name: "Pattern drafting", text: "Draft to measurements, alter the pieces, fold them up in 3D and print." },
  { page: "trace", image: "/workbench/atelier-assets/trace-review.webp", name: "Pattern tracing", text: "Turn a drawn or scanned pattern sheet into checked pieces." },
  { page: "materials", image: "/workbench/atelier-assets/material-rig.webp", name: "Fabric studies", text: "See how different cloths behave on the same pattern." },
  { page: "photo", image: "/workbench/atelier-assets/photo-example-sleeve.jpg", name: "Photo measurements", text: "Measure from a photograph with a sheet of paper in the frame." },
] as const;

const more = [
  ["/sections/wardrobe/", "Wardrobe", "A sundress, two hussar uniforms and an 18th-century waistcoat"],
  ["/sections/studies/", "Drape room", "Raise the arms, change the size, watch a jacket being sewn"],
  ["/sections/studio/", "Studios", "Draft a pattern, lay it on the roll, follow an embroidery needle"],
  ["/sections/materials/", "Materials", "Pull, twist, wash and heat cloth and see what changes"],
  ["/sections/tracing/", "Tracing", "Recover cut lines from a drawn pattern sheet"],
  ["/sections/films/", "Films", "All the garment and material films in one place"],
  ["/how-it-works/", "How it works", "The steps from a tape measure to cut cloth, and what each result means"],
  ["/get-started/", "Get started", "Open a workbench or build the command-line tool"],
] as const;

function Arrow({ diagonal = false }: { diagonal?: boolean }) {
  return <span aria-hidden="true" className={diagonal ? "arrow arrow-diagonal" : "arrow"}/>;
}

export default function Home() {
  return <>
    <section className="hm-steps" id="steps" aria-labelledby="steps-title"><div className="wrap">
      <header className="hm-head">
        <h2 id="steps-title">Three steps from a tape measure to cut cloth.</h2>
        <p>Each one runs on your own computer and shows its work, so you can check it before you spend any fabric.</p>
      </header>
      <ol className="hm-step-list">{steps.map((step, index) => <li key={step.title}>
        <a className="hm-step-picture" href={step.href} tabIndex={-1} aria-hidden="true">
          <img src={step.image} alt="" width={1440} height={960} loading="lazy" decoding="async"/></a>
        <span className="hm-step-number">{index + 1}</span>
        <h3>{step.title}</h3>
        <p>{step.text}</p>
        <a className="hm-link" href={step.href}>{step.link} <Arrow/></a>
      </li>)}</ol>
    </div></section>

    <section className="hm-garments" id="garments" aria-labelledby="garments-title"><div className="wrap">
      <header className="hm-head">
        <h2 id="garments-title">Garments made this way.</h2>
        <p>Each began as a flat pattern and was sewn in simulation on a body. Open one to turn it around, change its size, or download its pattern.</p>
      </header>
      <ul className="hm-garment-list">{garments.map(garment => <li key={garment.name}>
        <a className="hm-garment" href={garment.href}>
          <img src={`/media/home/${garment.image}.webp`} alt="" width={960} height={1200} loading="lazy" decoding="async"/>
          <span className="hm-garment-name">{garment.name}</span>
          <span className="hm-garment-note">{garment.note}</span>
        </a>
        <a className="hm-pattern" href={garment.pdf} download>Pattern (PDF)</a>
      </li>)}</ul>
      <p className="hm-after"><a className="hm-link" href="/sections/wardrobe/">See the whole wardrobe <Arrow/></a></p>
    </div></section>

    <section className="hm-loop" id="agent" aria-labelledby="agent-title"><div className="wrap">
      <div className="hm-loop-copy">
        <p className="hm-kicker">For a sewist working with an AI assistant</p>
        <h2 id="agent-title">Look at the drape. Change the flat pattern.</h2>
        <p>FrankenTextiles is built to be driven by an assistant that can see. It draws each try, says which pattern edge any spot in the picture came from, and keeps every round so two cuts can be compared.</p>
        <ol className="hm-loop-steps">{loop.map(([name, text]) => <li key={name}><strong>{name}</strong><span>{text}</span></li>)}</ol>
        <a className="hm-link" href="/how-it-works/#visual-fitting">Read how a sleeve was shortened this way <Arrow/></a>
      </div>
      <div className="hm-loop-figure" role="group" aria-label="One sleeve, two tries, seen from the same camera">
        <figure>
          <img src="/media/home/sleeve-before.webp" width={310} height={500} loading="lazy" decoding="async"
            alt="A jacket sleeve on a simulated arm. The cuff reaches past the knuckles and covers most of the hand."/>
          <figcaption><strong>First try</strong>The cuff covers the hand.</figcaption>
        </figure>
        <figure>
          <img src="/media/home/sleeve-after.webp" width={310} height={500} loading="lazy" decoding="async"
            alt="The same sleeve from the same camera, shorter. It ends at the wrist and the whole hand shows."/>
          <figcaption><strong>Second try</strong>The pattern’s sleeve is 50 mm shorter and ends above the hand.</figcaption>
        </figure>
      </div>
    </div></section>

    <section className="hm-tools" id="workbenches" aria-labelledby="tools-title"><div className="wrap">
      <header className="hm-head">
        <h2 id="tools-title">Try it in your browser.</h2>
        <p>Four tools open with a working example. They run on your device; patterns, measurements and photographs are not uploaded.</p>
      </header>
      <ul className="hm-tool-list">{tools.map(tool => <li key={tool.page}>
        <a href={workbench(tool.page)}>
          <img src={tool.image} alt="" loading="lazy" decoding="async"/>
          <span className="hm-tool-name">{tool.name} <Arrow diagonal/></span>
          <span className="hm-tool-text">{tool.text}</span>
        </a>
      </li>)}</ul>
    </div></section>

    <section className="hm-limits" id="limits" aria-labelledby="limits-title"><div className="wrap">
      <div>
        <h2 id="limits-title">A simulation is not a fitting.</h2>
        <p>It catches the mistakes that waste cloth: seams of different lengths, a sleeve that cannot be set in, a waistband that will not hold. Whether a garment fits a person is settled by sewing a test garment in cheap cloth first.</p>
        <p>The site says which results are checked against a reference, which are physics tests, and which nobody has measured yet.</p>
        <p className="hm-limit-links">
          <a className="hm-link" href="/how-it-works/#claims">What each result does and does not claim <Arrow/></a>
          <a className="hm-link" href={`${repo}/blob/main/FEATURE_PARITY.md`}>What is finished and what is not <Arrow diagonal/></a>
        </p>
      </div>
      <nav className="hm-more" aria-labelledby="more-title">
        <h2 id="more-title">Everything else</h2>
        <ul>{more.map(([href, name, note]) => <li key={href}><a href={href}><strong>{name}</strong><span>{note}</span></a></li>)}</ul>
      </nav>
    </div></section>
  </>;
}
