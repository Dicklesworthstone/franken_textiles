"use client";

import dynamic from "next/dynamic";
import { useCallback, useEffect, useRef, useState } from "react";
import { studies, type Study, type View } from "../app/studies";
import LabBench from "../components/LabBench";
import TraceReveal from "../components/TraceReveal";
import { experiments, type Rig } from "../components/materialExperiments";
import InlineFilm, { PlaybackMark, filmFile } from "../components/InlineFilm";
import GarmentPatterns from "./GarmentPatterns";
import DrapeRoom from "./DrapeRoom";
import ConstructionStudy,{constructionFilms} from "./construction/ConstructionStudy";
import {ExhibitionCover} from "./site/ExhibitionCover";
import Home from "./site/Home";
import SiteHeader from "./site/SiteHeader";
import SiteFooter from "./site/SiteFooter";
import Reveal from "./site/Reveal";
import { workbench } from "./site/links";
import type { PanelOutline } from "../components/garmentMesh";

const Studio = dynamic(() => import("../components/Studio"), { ssr: false });
type Exhibit = { study: Study; film: boolean } | { tour: true } | { rig: Rig } | { reel: true } | { poseRoom: true } | { garment: (typeof wardrobe)[number] };
type CaptureFn = (progress: number, yaw: number) => Promise<void>;
declare global { interface Window { ftxFrame?: CaptureFn } }

const wardrobe = [
  {id:"sundress", title:"The garden sundress", detail:"Dress study", action:"Pose the figure and change its measurements"},
  {id:"hussar", title:"The hussar’s cabinet", detail:"Sweden · 1761 & 1765", action:"Inspect the braid, cuffs and layers"},
] as const;

const films = [
  ...constructionFilms,
  { id:"drape-room", title:"Raising the sleeve", detail:"Body & cloth study" },
  { id:"skirt-atelier", title:"Two hems in motion", detail:"Adult skirt drape" },
  { id:"trouser-posture", title:"Bending the knee", detail:"Trouser posture" },
  { id:"trouser-elastic", title:"Holding the waist", detail:"Elastic casings" },
  { id:"trouser-section", title:"Inside the folds", detail:"Cross-section" },
  { id:"studio-tour", title:"From pattern to stitch", detail:"Studio tour" },
  { id:"wardrobe-reel", title:"Inside the wardrobe", detail:"Exhibition reel" },
  ...wardrobe.map(s=>({ id:"wardrobe-"+s.id, title:s.title, detail:s.detail })),
  ...(Object.keys(experiments) as Rig[]).map(rig=>({ id:"rig-"+rig, title:experiments[rig].title, detail:"Material study" })),
  // A garment study's own film is the same drape as its construction film above, in the older look, so
  // only the seam study is listed, with its card's film.
  ...studies.filter(s=>s.group==="Construction").map(s=>({ id:s.card, title:s.title, detail:"Construction study" })),
];

const filmGroups = ["All", "Construction", "Studies", "Wardrobe", "Materials"] as const;
type FilmGroup = (typeof filmGroups)[number];
function groupOf(id: string): Exclude<FilmGroup, "All"> {
  if (id.startsWith("construction-")) return "Construction";
  if (id.startsWith("wardrobe-")) return "Wardrobe";
  if (id.startsWith("rig-")) return "Materials";
  return "Studies";
}

function Arrow({ diagonal = false }: { diagonal?: boolean }) {
  return <span aria-hidden="true" className={diagonal ? "arrow arrow-diagonal" : "arrow"}/>;
}

// The still on a lab exhibit's card: that exhibit's own stage at its defaults, captured by
// scripts/render-exhibit-cards.mjs. The card's heading names the exhibit, so the picture carries no text.
function CalloutStill({ name }: { name: string }) {
  return <span className="callout-still"><img src={`/media/cards/${name}.webp`} alt="" width={840} height={560} loading="lazy" decoding="async"/></span>;
}

function Capture({ study }: { study: Study }) {
  const [pose, setPose] = useState({ frame: 1, yaw: 0.32 });
  const [ready, setReady] = useState(false);
  const done = useRef<(() => void) | null>(null);
  useEffect(() => {
    if (!ready) return;
    window.ftxFrame = (frame, yaw) => new Promise<void>((resolve) => {
      done.current = resolve;
      setPose({ frame, yaw });
    });
    return () => { delete window.ftxFrame; };
  }, [ready]);
  useEffect(() => {
    if (!done.current) return;
    let id = requestAnimationFrame(() => { id = requestAnimationFrame(() => {
      done.current?.(); done.current = null;
    }); });
    return () => cancelAnimationFrame(id);
  }, [pose]);
  return <main className="capture"><Studio garment={study.garment} mode={study.mode}
    wire={study.id === "construction"} {...pose} onReady={() => setReady(true)}/></main>;
}

function Player({ item, close }: { item: Exhibit; close: () => void }) {
  const study = "study" in item ? item.study : undefined;
  const rig = "rig" in item ? item.rig : undefined;
  const garment = "garment" in item ? item.garment : undefined;
  const mediaId = study?.id ?? (garment ? "wardrobe-"+garment.id : rig ? "rig-" + rig : "reel" in item ? "wardrobe-reel" : "poseRoom" in item ? "drape-room" : "studio-tour");
  const title = study?.title ?? garment?.title ?? (rig ? experiments[rig].title : "reel" in item ? "Inside the wardrobe" : "poseRoom" in item ? "Raising the sleeve" : "A tour of the pattern room");
  const dialog = useRef<HTMLDialogElement>(null);
  const [film, setFilm] = useState("film" in item ? item.film : true);
  const [frame, setFrame] = useState(0);
  const [mode, setMode] = useState<View>(study?.mode ?? "cloth");
  const [wire, setWire] = useState(study?.id === "construction");
  const [seams, setSeams] = useState<string[]>([]);
  const [seam, setSeam] = useState("");
  const [panels, setPanels] = useState<PanelOutline[]>([]);
  const [panel, setPanel] = useState("");
  const outline = mode === "construction" && panels.find(p=>p.name===panel);
  const [spread, setSpread] = useState(0);
  const [strain, setStrain] = useState<{ min: number; max: number; p95: number }>();
  const [resetKey, reset] = useState(0);
  const [playing, play] = useState(false);
  const [available, setAvailable] = useState(false);
  const [failed, setFailed] = useState(false);
  const ready = useCallback(() => setAvailable(true), []);
  const error = useCallback(() => { setFailed(true); play(false); }, []);
  const live = study !== undefined && !film && !failed;
  const animationStart = useRef(0);
  useEffect(() => {
    const el = dialog.current!;
    const active = document.activeElement;
    el.showModal();
    const old = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      el.close();
      document.body.style.overflow = old;
      if (active instanceof HTMLElement) active.focus({ preventScroll: true });
    };
  }, []);
  useEffect(() => {
    if (!playing) return;
    let id = 0;
    const step = (now: number) => {
      const next = Math.min(1, (now - animationStart.current) / 9500);
      setFrame(next);
      if (next === 1) play(false);
      else id = requestAnimationFrame(step);
    };
    id = requestAnimationFrame(step);
    return () => cancelAnimationFrame(id);
  }, [playing]);
  const sample = Math.round(frame * 135);
  const phase = sample === 0 ? "Pattern board" : sample < 70 ? "Wrapping" : sample < 88 ? "Joining panels"
    : sample < 100 ? "Sewing" : sample === 135 ? "Settled" : "Draping";
  function changePlayer() {
    play(false);
    setAvailable(false);
    setFailed(false);
    setFilm(live);
  }
  return <dialog ref={dialog} className="player" aria-labelledby="player-title"
    onCancel={e => { e.preventDefault(); close(); }}
    onClick={e => { if (e.target === e.currentTarget) {
      const box = e.currentTarget.getBoundingClientRect();
      if (e.clientX < box.left || e.clientX > box.right || e.clientY < box.top || e.clientY > box.bottom) close();
    } }}>
    <header className="player-head"><div><span className="mono">{live ? "Interactive study" : "Studio film"}{study && ` / Fig. ${study.figure}`}</span>
      <h2 id="player-title">{title}</h2></div>
      <button className="close" onClick={close} aria-label="Close study">×</button></header>
    {live ? <div className="stage"><Studio garment={study.garment} mode={mode}
      frame={mode === "strain" ? 1 : frame} wire={wire} resetKey={resetKey} onReady={ready} onError={error}
      seam={mode === "construction" ? seam : ""} spread={spread} onSeams={setSeams} onStrain={setStrain} panel={panel} onPanels={setPanels}/>
      {outline && <div className="flat-inset"><svg viewBox={outline.box} role="img" aria-label={outline.name+" flat panel outline"}>
        <path d={outline.path} fill="none" stroke={outline.color} strokeWidth="2" vectorEffect="non-scaling-stroke"/></svg><span>Flat panel · {outline.name}</span></div>}
      {!available && <span className="loading mono" role="status">Loading geometry…</span>}
      {available && <span className="orbit-note mono">Drag to turn · scroll to zoom · right-drag to move</span>}
    </div> : <div className="movie"><InlineFilm key={mediaId} id={mediaId} title={title} controls requested={!failed}/></div>}
    {failed && <p role="status" className="fallback">WebGL is unavailable on this device. The film shows the same garment.</p>}
    {live && <div className="controls">
      <div className="control-row"><div className="seg" aria-label="Garment view">
        {(["cloth", "construction", "strain"] as const).map((v, i) => <button key={v} aria-pressed={mode === v}
          onClick={() => { setMode(v); if (v === "strain") play(false); }}>{["Cloth","Panels","Strain"][i]}</button>)}
      </div><div className="tools"><label><input type="checkbox" checked={wire} onChange={e => setWire(e.target.checked)}/> Mesh</label>
        <button onClick={() => reset(k => k + 1)}>Reset view</button></div></div>
      {mode === "construction" && seams.length > 0 && <div className="seam-control">
        <label>Follow panel <select value={panel} onChange={e=>{setPanel(e.target.value);setSeam("");}}>
          <option value="">All panels</option>{panels.map(p=><option key={p.name} value={p.name}>{p.name.replaceAll("_"," ")}</option>)}
        </select></label>
        <label>Inspect seam <select value={seam} onChange={e => {setSeam(e.target.value);setPanel("");}}>
          <option value="">All seams</option>{seams.map(name => <option key={name} value={name}>{name.replaceAll("_", " ")}</option>)}
        </select></label>{seam && <span><i/> Side A <i/> Side B</span>}
        <label className="exploder">Exploded view <input aria-label="Exploded view" type="range" min="0" max="1" step=".02" value={spread}
          onChange={event => setSpread(Number(event.target.value))}/></label>
        {spread > 0 && <small>Panels spaced apart for inspection.</small>}
      </div>}
      {mode === "strain" ? <div className="strain-key"><span/>Color scale −3% to +3%.
        {strain && <strong>Endpoint range {strain.min}% to +{strain.max}%</strong>}</div> :
        <><div className="phase-jumps" aria-label="Replay landmarks">{([[0,"Flat pattern"],[69,"Wrapped"],[99,"Sewn"],[135,"Settled"]] as const).map(([tick,title]) =>
          <button key={tick} aria-pressed={sample === tick} onClick={() => { play(false); setFrame(tick / 135); }}>{title}</button>)}</div>
        <div className="transport"><button disabled={!available} aria-label={playing ? "Pause replay" : "Play replay"} onClick={() => {
          if (playing) play(false);
          else { const start = frame >= 1 ? 0 : frame; setFrame(start); animationStart.current = performance.now() - start * 9500; play(true); }
        }}><PlaybackMark playing={playing}/></button>
          <input aria-label="Fold replay position" type="range" min="0" max="1" step="0.001" value={frame}
            onChange={e => { play(false); setFrame(Number(e.target.value)); }}/>
          <span className="mono phase">{phase}</span></div></>}
    </div>}
    <footer className="player-foot">{live ? <p>Native Rust fold replay on a synthetic body.</p> :
      <a download={`frankentextiles-${mediaId}.mp4`} href={filmFile(mediaId)}>Download MP4 ↓</a>}
      {study ? <button onClick={changePlayer}>{live ? "Watch film" : "Open 3D study"} <Arrow/></button> :
        garment ? <a href={`/wardrobe/${garment.id}/`}>Open garment study <Arrow/></a> : rig ? <a href={`/experiments/${rig}/`}>Open experiment <Arrow/></a> : "reel" in item ? <a href="/sections/wardrobe/">Explore the wardrobe <Arrow/></a> : "poseRoom" in item ? <a href="/sections/studies/#drape-room">Try the drape room <Arrow/></a> : <a href="/sections/studio/">Try the studios <Arrow/></a>}</footer>
    <GarmentPatterns exhibit={mediaId}/>
  </dialog>;
}

// With no `section` this is the home page: the opening film and components/site/Home.tsx. Each part of the
// catalogue below is a page of its own (/sections/<id>/).
export default function Exhibition({section}: {section?: string}) {
  const show = (id: string) => section === id;
  const Heading = section ? "h1" : "h2";
  const studio = section === "draft" || section === "cut" || section === "stitch" || section === "repair" ? section : undefined;
  const [filter, setFilter] = useState("All studies");
  const [filmGroup, setFilmGroup] = useState<FilmGroup>("All");
  const [screening, setScreening] = useState(films[0]);
  const [filmChosen, setFilmChosen] = useState(false);
  const projection = useRef<HTMLDivElement>(null);
  const [item, setItem] = useState<Exhibit | null>(null);
  const [capture, setCapture] = useState<Study>();
  useEffect(() => {
    const params = new URLSearchParams(location.search);
    const id = params.get("capture");
    if (id) setCapture(studies.find(study => study.id === id));
    else if (params.get("film") === "studio-tour") setItem({ tour: true });
    else if (params.get("film") === "wardrobe-reel") setItem({ reel: true });
    else if (params.get("film") === "drape-room") setItem({ poseRoom: true });
    else if (/^(trouser-|construction-)/.test(params.get("film")??"")) {
      const selection=films.find(f=>f.id===params.get("film"));
      if(selection){setScreening(selection);setFilmChosen(true);projection.current?.scrollIntoView({block:"start"});}
    }
    else if (params.get("film") === "rig-cylinder") setItem({ rig: "cylinder" });
    else if (params.get("film") === "rig-strip") setItem({ rig: "strip" });
    else {
      const study = studies.find(s => s.id === (params.get("study") ?? params.get("film")));
      const garment = wardrobe.find(s => "wardrobe-"+s.id === params.get("film"));
      if (study) setItem({ study, film: !params.has("study") });
      else if (garment) setItem({ garment });
    }
  }, []);
  function closePlayer() {
    setItem(null);
    const url = new URL(location.href);
    if (url.searchParams.has("study") || url.searchParams.has("film")) {
      url.searchParams.delete("study");
      url.searchParams.delete("film");
      history.replaceState(history.state, "", url);
    }
  }
  if (capture) return <Capture study={capture}/>;
  return <>
    <SiteHeader skipTo="#exhibition"/>
    <main id="exhibition">
      {!section && <><ExhibitionCover/><Home/></>}
      {section==="wardrobe"&&<ConstructionStudy choices={["sundress","eased","blue","yellow","column","flare","fez"]}/>}
      {section==="studies"&&<ConstructionStudy choices={["bolero","trousers","ensemble"]}/>}
      {show("studies") && <DrapeRoom/>}
      {show("wardrobe") && <section id="wardrobe" className="paper wardrobe-gallery"><div className="wrap collection">
        <div className="section-top"><div><p className="kicker">Interactive garments</p><Heading className="section-title"><a href="/sections/wardrobe/">The wardrobe<span>.</span></a></Heading></div>
          <p>Take a garment off the rail. Turn it around, change the figure, or look beneath a layer.</p></div>
        <div className="material-pair">{wardrobe.map(s=><article key={s.id} data-reveal>
          <div className="material-cover"><InlineFilm id={"construction-"+(s.id==="hussar"?"blue":"sundress")} title={s.title}/>
            <a className="card-open" aria-label={"Explore "+s.title} href={`/wardrobe/${s.id}/`}><span className="launch"><Arrow diagonal/></span></a></div>
          <a className="material-caption" href={`/wardrobe/${s.id}/`}><div className="mono">{s.detail}</div><h3>{s.title}</h3><span>{s.action} <Arrow/></span></a>
          <GarmentPatterns exhibit={s.id==="sundress"?"sundress-cuts":s.id}/>
        </article>)}</div>
        <div className="material-callouts" data-reveal>
          <a className="thermal-callout" href="/wardrobe/waistcoat/">
            <CalloutStill name="waistcoat"/>
            <div>
              <span className="mono">From a museum piece · 1780s</span>
              <h3>An 18th-century waistcoat</h3>
              <p>A pattern rebuilt from a waistcoat in the Victoria and Albert Museum: silk fronts, linen linings and welt pockets. All 36 seams match, and it prints at full size.</p>
            </div>
            <span className="thermal-callout-cta">See the cut and the pockets <Arrow diagonal/></span>
          </a>
          <a className="thermal-callout" href="/wardrobe/waistcoat/inside-out/">
            <CalloutStill name="inside-out"/>
            <div>
              <span className="mono">Linings and facings in 3D</span>
              <h3>Turn a lined garment inside out</h3>
              <p>Turn the waistcoat’s shell, facing, lining and pocket bags together, then take the shell away to see what sits underneath.</p>
            </div>
            <span className="thermal-callout-cta">Open the layers <Arrow diagonal/></span>
          </a>
          <a className="thermal-callout" href="/wardrobe/sundress/compare/">
            <CalloutStill name="sundress-compare"/>
            <div>
              <span className="mono">One dress, two figures</span>
              <h3>Compare two drapes of one dress</h3>
              <p>Stand the same sundress on two figures side by side, at the same scale, and compare where the hem falls and how the waist sits. A figure it does not fit is shown as refused, not faked.</p>
            </div>
            <span className="thermal-callout-cta">Compare the two <Arrow diagonal/></span>
          </a>
        </div>
      </div></section>}
      {(show("studio") || studio) && <LabBench initialStation={studio} heading={Heading}/>}
      {show("materials") && <section className="material-gallery" id="materials"><div className="wrap">
        <div className="section-top"><div><p className="kicker">Material studies</p><Heading className="section-title"><a href="/sections/materials/">Cloth under load<span>.</span></a></Heading></div>
          <p>Two experiments on a tube and a strip of cloth. Move the grips and see what changes.</p></div>
        <div className="material-pair">{(Object.keys(experiments) as Rig[]).map(kind=><article key={kind} data-reveal>
          <div className="material-cover"><InlineFilm id={"rig-"+kind} title={experiments[kind].title}/>
            <a className="card-open" aria-label={"Explore "+experiments[kind].title} href={`/experiments/${kind}/`}><span className="launch"><Arrow diagonal/></span></a></div>
          <a className="material-caption" href={`/experiments/${kind}/`}><h3>{experiments[kind].title}</h3><span>{experiments[kind].action} <Arrow/></span></a>
        </article>)}</div>
        <div className="material-callouts" data-reveal>
          <a className="thermal-callout" href="/experiments/thermal/">
            <CalloutStill name="thermal"/>
            <div>
              <span className="mono">Heat through layers</span>
              <h3>Thermal sandwich</h3>
              <p>Stack sheets of measured neoprene and leather between a warm side and a cold side, and read the temperature at each layer.</p>
            </div>
            <span className="thermal-callout-cta">Build a stack <Arrow diagonal/></span>
          </a>
          <a className="thermal-callout" href="/experiments/wash/">
            <CalloutStill name="wash"/>
            <div>
              <span className="mono">Shrinkage in the wash</span>
              <h3>Wash, shrink &amp; recut</h3>
              <p>Apply the measured wash shrinkage of a denim to a pattern piece, lengthwise and crosswise, and get the larger outline to cut so that it fits once washed.</p>
            </div>
            <span className="thermal-callout-cta">Compare the outlines <Arrow diagonal/></span>
          </a>
          <a className="thermal-callout" href="/experiments/friction/">
            <CalloutStill name="friction"/>
            <div>
              <span className="mono">Stick, then slip</span>
              <h3>Friction sled</h3>
              <p>Pull a weighted swatch across a surface and watch it hold, then slide. The surfaces are synthetic examples; a fabric with no measured friction value cannot be chosen.</p>
            </div>
            <span className="thermal-callout-cta">Pull the sled <Arrow diagonal/></span>
          </a>
        </div>
      </div></section>}
      {show("studies") && <section id="studies" className="paper"><div className="wrap collection">
        <div className="section-top"><div><p className="kicker">Cloth in three dimensions</p><Heading className="section-title"><a href="/sections/studies/">The drape gallery<span>.</span></a></Heading></div>
          <div className="filters" aria-label="Filter studies">{["All studies","Garments","Construction"].map(k =>
            <button key={k} aria-pressed={filter === k} onClick={() => setFilter(k)}>{k}</button>)}</div></div>
        <div className="cards">{studies.filter(s => filter === "All studies" || s.group === filter).map(s =>
          <article key={s.id} className="card">
            <div className="thumb"><InlineFilm id={s.card} title={s.title}/><span className="thumb-id mono">Fig. {s.figure}</span>
              <button className="card-open" aria-label={"Explore "+s.title} onClick={()=>setItem({study:s,film:false})}><span className="launch"><Arrow diagonal/></span></button></div>
            <button className="card-text" onClick={()=>setItem({study:s,film:false})}><span className="mono">{s.detail}</span><h3>{s.title}</h3><p>{s.description}</p></button>
            <GarmentPatterns exhibit={s.garment}/>
          </article>)}</div>
        <div className="material-callouts" data-reveal>
          <a className="thermal-callout" href="/studies/wrap-skirt/">
            <CalloutStill name="wrap-skirt"/>
            <div>
              <span className="mono">Does it still close when you sit?</span>
              <h3>A wrap skirt with ties</h3>
              <p>Keep one wrap-skirt pattern and try it on three figures, standing and seated, at two tie settings, to see how much the front still overlaps.</p>
            </div>
            <span className="thermal-callout-cta">Check the overlap <Arrow diagonal/></span>
          </a>
          <a className="thermal-callout" href="/studies/tiling/">
            <CalloutStill name="tiling"/>
            <div>
              <span className="mono">Printing at home</span>
              <h3>Assemble the printed pages</h3>
              <p>Lay the real A4 pages of a pattern on a virtual floor and see how the overlaps, alignment marks and scale bar line up before you print.</p>
            </div>
            <span className="thermal-callout-cta">Open the pages <Arrow diagonal/></span>
          </a>
          <a className="thermal-callout" href="/studies/sewing/">
            <CalloutStill name="sewing"/>
            <div>
              <span className="mono">Which seam first?</span>
              <h3>Jacket sewing order</h3>
              <p>Step through the ten sewing steps of the bolero in order, on the flat pieces and on the 3D jacket, with what has to be done before each.</p>
            </div>
            <span className="thermal-callout-cta">Play the steps <Arrow diagonal/></span>
          </a>
        </div>
      </div></section>}
      {show("tracing") && <TraceReveal heading={Heading}/>}
      {show("films") && <section id="films" className="films"><div className="wrap film-grid">
        <div className="screening" ref={projection}><p className="kicker">Films · 4K 60 fps</p><Heading className="section-title"><a href="/sections/films/">Screening room<span>.</span></a></Heading>
          <p className="screening-lead">Every film on the site: garments going from cut pieces to sewn, cloth under test, and the workshop tools at work.</p>
          <InlineFilm key={screening.id} id={screening.id} title={screening.title} controls requested={filmChosen}/>
          <div className="screening-caption"><strong>{screening.title}</strong><a href={filmFile(screening.id)} download>Download MP4 ↓</a></div>
          <GarmentPatterns exhibit={screening.id}/></div>
        <div className="film-rail">
          <div className="film-filters" role="group" aria-label="Filter films">
            {filmGroups.map(group => {
              const matching = group === "All" ? films : films.filter(f => groupOf(f.id) === group);
              return <button key={group} type="button" aria-pressed={filmGroup === group} onClick={() => {
                setFilmGroup(group);
                if (!matching.some(f => f.id === screening.id) && matching[0]) {
                  setFilmChosen(false);
                  setScreening(matching[0]);
                }
              }}>
                {group} <small>{matching.length}</small>
              </button>;
            })}
          </div>
          <div className="film-list">{films.filter(f => filmGroup === "All" || groupOf(f.id) === filmGroup).map(f=><button key={f.id} aria-pressed={f.id===screening.id} onClick={()=>{
            setFilmChosen(true);setScreening(f);
            if(projection.current && projection.current.getBoundingClientRect().bottom<120) projection.current.scrollIntoView({block:"start"});
          }}>
            <img src={`/media/${f.id}.jpg`} alt="" loading="lazy"/><span><small className="mono">{f.detail}</small><strong>{f.title}</strong></span><span className="play-ring"><PlaybackMark/></span></button>)}</div>
        </div>
      </div></section>}
      {show("tools") && <section id="workbenches" className="paper workbench-band"><div className="wrap workbench-library"><div className="section-top"><div><p className="kicker">Run FrankenTextiles in your browser</p><Heading className="section-title"><a href="/sections/tools/">The workbenches<span>.</span></a></Heading></div>
        <p>Four tools that run in your browser. Each opens with a working example, and nothing you load into them is uploaded.</p></div>
        <div className="workbench-shelves">{[
          ["index","Pattern drafting","Draft to measurements, alter the pieces, fold them up in 3D and print.","drafting.webp"],
          ["trace","Pattern tracing","Turn a drawn or scanned pattern sheet into checked pieces.","trace-review.webp"],
          ["materials","Fabric studies","Browse 449 fabrics with sourced measurements and see how they behave on a pattern.","material-rig.webp"],
          ["photo","Photo measurements","Measure from a photograph with a sheet of paper in the frame.","photo-example-sleeve.jpg"],
        ].map(([page,title,description,image])=><a key={page} data-reveal href={workbench(page as "index"|"trace"|"materials"|"photo")}>
          <img src={`/workbench/atelier-assets/${image}`} alt="" loading="lazy"/>
          <div><h3>{title}</h3><p>{description}</p></div><Arrow diagonal/>
        </a>)}</div>
      </div></section>}
    </main>
    <SiteFooter/>
    <Reveal/>
    {item && <Player item={item} close={closePlayer}/>}
  </>;
}
