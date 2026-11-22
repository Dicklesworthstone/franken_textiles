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
import {ExhibitionCover,ExhibitionFeatures} from "./site/ExhibitionCover";
import SiteHeader from "./site/SiteHeader";
import SiteFooter from "./site/SiteFooter";
import Reveal from "./site/Reveal";
import { Claims, Deliverables, Manifesto, Pipeline, PlatformCapabilities, StartHere } from "./site/Story";
import VisualFitting from "./site/VisualFitting";
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
  ...studies.map(s=>({ id:s.id, title:s.title, detail:"Drape study" })),
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
      {available && <span className="orbit-note mono">Drag to orbit · Scroll to zoom</span>}
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

export default function Exhibition({section}: {section?: string}) {
  const show = (id: string) => !section || section === id;
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
      {!section && <><ExhibitionCover/><ExhibitionFeatures/></>}
      {section==="wardrobe"&&<ConstructionStudy choices={["sundress","eased","blue","yellow","column","flare","fez"]}/>}
      {section==="studies"&&<ConstructionStudy choices={["bolero","trousers","ensemble"]}/>}
      {show("studies") && <DrapeRoom/>}
      {!section && <section className="dr-index" aria-label="Explore more of the studio"><div className="wrap">
        <a href="/studies/trousers/?bench=posture&amp;run=squat-25"><img src="/media/cards/strip-trouser-posture.webp" alt="" loading="lazy" decoding="async"/><div><small>Body &amp; cloth</small><strong>Bend the knees in trousers</strong></div><Arrow diagonal/></a>
        <a href="/experiments/strip/"><img src="/media/cards/strip-rig.webp" alt="" loading="lazy" decoding="async"/><div><small>Textile mechanics</small><strong>Pull cloth on the bias</strong></div><Arrow diagonal/></a>
        <div className="dr-reel"><figure className="hero-screen"><div><InlineFilm id="wardrobe-reel" title="Inside the wardrobe"/></div></figure>
          <button onClick={()=>setItem({reel:true})}><span><small>Studio film</small><strong>Inside the wardrobe</strong></span><Arrow diagonal/></button></div>
      </div></section>}
      {!section && <><Manifesto/><VisualFitting/><Pipeline/></>}
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
              <span className="mono">V&amp;A 256-1880 · 1780s reconstruction</span>
              <h3>1780s waistcoat &amp; welt pockets</h3>
              <p>Separate silk fronts, plain silk facings, linen linings, and slit welt pockets validated CONVERGED across 36 native seams with 1:1 mm SVG &amp; A4 exports.</p>
            </div>
            <span className="thermal-callout-cta">Inspect cut &amp; welt steps <Arrow diagonal/></span>
          </a>
          <a className="thermal-callout" href="/wardrobe/waistcoat/inside-out/">
            <CalloutStill name="inside-out"/>
            <div>
              <span className="mono">3D layers · Invariant seam indices</span>
              <h3>Turn a lined garment inside out</h3>
              <p>Rotate shell, silk facing, linen lining, and pocket bags together, toggle the shell off to reveal the lining, and audit missing collar linings honestly.</p>
            </div>
            <span className="thermal-callout-cta">Open 3D layer view <Arrow diagonal/></span>
          </a>
          <a className="thermal-callout" href="/wardrobe/sundress/compare/">
            <CalloutStill name="sundress-compare"/>
            <div>
              <span className="mono">Synchronized camera · 1:1 m world scale</span>
              <h3>Compare two native dress drapes</h3>
              <p>Place independently accepted native drapes of one fixed sundress side by side to measure hem drop and waist clearance — or inspect an explicit solver refusal.</p>
            </div>
            <span className="thermal-callout-cta">Compare native drapes <Arrow diagonal/></span>
          </a>
        </div>
      </div></section>}
      {(show("studio") || studio) && <LabBench initialStation={studio} heading={Heading}/>}
      {show("materials") && <section className="material-gallery" id="materials"><div className="wrap">
        <div className="section-top"><div><p className="kicker">Material studies</p><Heading className="section-title"><a href="/sections/materials/">Cloth under load<span>.</span></a></Heading></div>
          <p>Two experiments from the Rust solver. Move the grips and see what changes in the cloth.</p></div>
        <div className="material-pair">{(Object.keys(experiments) as Rig[]).map(kind=><article key={kind} data-reveal>
          <div className="material-cover"><InlineFilm id={"rig-"+kind} title={experiments[kind].title}/>
            <a className="card-open" aria-label={"Explore "+experiments[kind].title} href={`/experiments/${kind}/`}><span className="launch"><Arrow diagonal/></span></a></div>
          <a className="material-caption" href={`/experiments/${kind}/`}><h3>{experiments[kind].title}</h3><span>{experiments[kind].action} <Arrow/></span></a>
        </article>)}</div>
        <div className="material-callouts" data-reveal>
          <a className="thermal-callout" href="/experiments/thermal/">
            <CalloutStill name="thermal"/>
            <div>
              <span className="mono">Live WASM · Measured neoprene &amp; leather</span>
              <h3>Thermal sandwich</h3>
              <p>Stack measured catalog sheets at an explicit contact resistance, adjust reservoir temperatures, inspect layer-by-layer °C drops, and test live solver refusals.</p>
            </div>
            <span className="thermal-callout-cta">Build a layer stack <Arrow diagonal/></span>
          </a>
          <a className="thermal-callout" href="/experiments/wash/">
            <CalloutStill name="wash"/>
            <div>
              <span className="mono">Live WASM · Kanat &amp; Ozdil 2026 denim</span>
              <h3>Wash, shrink &amp; recut</h3>
              <p>Apply measured ISO 6330 warp/weft wash shrinkage along the grainline, compare shrunk vs pre-wash compensated outlines, and test unsupported-material refusals.</p>
            </div>
            <span className="thermal-callout-cta">Compare wash shrinkage <Arrow diagonal/></span>
          </a>
          <a className="thermal-callout" href="/experiments/friction/">
            <CalloutStill name="friction"/>
            <div>
              <span className="mono">Native contact · Live WASM calibration</span>
              <h3>Friction sled</h3>
              <p>Step across recorded IPC normal reactions and the Coulomb stick-to-slip plateau between synthetic surface states while uncalibrated catalog textiles stay unavailable.</p>
            </div>
            <span className="thermal-callout-cta">Inspect stick-to-slip <Arrow diagonal/></span>
          </a>
        </div>
      </div></section>}
      {show("studies") && <section id="studies" className="paper"><div className="wrap collection">
        <div className="section-top"><div><p className="kicker">Cloth in three dimensions</p><Heading className="section-title"><a href="/sections/studies/">The drape gallery<span>.</span></a></Heading></div>
          <div className="filters" aria-label="Filter studies">{["All studies","Garments","Construction"].map(k =>
            <button key={k} aria-pressed={filter === k} onClick={() => setFilter(k)}>{k}</button>)}</div></div>
        <div className="cards">{studies.filter(s => filter === "All studies" || s.group === filter).map(s =>
          <article key={s.id} className="card">
            <div className="thumb"><InlineFilm id={s.id} title={s.title}/><span className="thumb-id mono">Fig. {s.figure}</span>
              <button className="card-open" aria-label={"Explore "+s.title} onClick={()=>setItem({study:s,film:false})}><span className="launch"><Arrow diagonal/></span></button></div>
            <button className="card-text" onClick={()=>setItem({study:s,film:false})}><span className="mono">{s.detail}</span><h3>{s.title}</h3><p>{s.description}</p></button>
            <GarmentPatterns exhibit={s.garment}/>
          </article>)}</div>
        <div className="material-callouts" data-reveal>
          <a className="thermal-callout" href="/studies/wrap-skirt/">
            <CalloutStill name="wrap-skirt"/>
            <div>
              <span className="mono">Fixed pattern · Neutral vs seated slices</span>
              <h3>Adjustable-overlap wrap skirt</h3>
              <p>Keep one trued wrap-skirt cut fixed across three synthetic adult forms, compare two waist-tie settings, and inspect seated Anny thigh slices.</p>
            </div>
            <span className="thermal-callout-cta">Check wrap overlap <Arrow diagonal/></span>
          </a>
          <a className="thermal-callout" href="/studies/tiling/">
            <CalloutStill name="tiling"/>
            <div>
              <span className="mono">Home printing · A4 tiled PDF</span>
              <h3>Assemble an A4 pattern floor</h3>
              <p>Inspect real tiled A4 pages, 5 mm bleed overlap, alignment diamonds, and the 100 mm scale bar before printing at full size.</p>
            </div>
            <span className="thermal-callout-cta">Open tile floor <Arrow diagonal/></span>
          </a>
          <a className="thermal-callout" href="/studies/sewing/">
            <CalloutStill name="sewing"/>
            <div>
              <span className="mono">Native planner · 10 operations</span>
              <h3>Jacket sewing order</h3>
              <p>Step through all ten accepted operations of the Zouave bolero across flat pieces and the 3D jacket with prerequisites and stitch files.</p>
            </div>
            <span className="thermal-callout-cta">Play sewing order <Arrow diagonal/></span>
          </a>
        </div>
      </div></section>}
      {show("tracing") && <TraceReveal heading={Heading}/>}
      {!section && <PlatformCapabilities/>}
      {show("films") && <section id="films" className="films"><div className="wrap film-grid">
        <div className="screening" ref={projection}><p className="kicker">Films · 4K 60 fps</p><Heading className="section-title"><a href="/sections/films/">Screening room<span>.</span></a></Heading>
          <p className="screening-lead">Source-rendered captures from the native cloth solver, historical wardrobe studies, and live WASM workbenches.</p>
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
      {!section && <Deliverables/>}
      {show("tools") && <section id="workbenches" className="paper workbench-band"><div className="wrap workbench-library"><div className="section-top"><div><p className="kicker">Run FrankenTextiles in your browser</p><Heading className="section-title"><a href="/sections/tools/">The workbenches<span>.</span></a></Heading></div>
        <p>Four local WebAssembly tools (4.4 MiB gzipped). Each opens with a working example and runs bit-identically to the native CLI with zero uploads.</p></div>
        <div className="workbench-shelves">{[
          ["index","Pattern drafting","Draft to measurements, run all five alterations, fold in 3D, and print every export.","drafting.webp"],
          ["trace","Pattern tracing","Trace PNG, JPEG or WebP sheets in pure Rust, annotate seams, and run round-0 audits.","trace-review.webp"],
          ["materials","Material studies","Inspect 449 sourced fabrics, import GarmentCode, test heat/strips, and plan cutting rolls.","material-rig.webp"],
          ["photo","Photo measurements","Calibrate lenses with ChArUco, measure EXIF photos with bootstrap σ, and fit bodies.","photo-example-sleeve.jpg"],
        ].map(([page,title,description,image])=><a key={page} data-reveal href={workbench(page as "index"|"trace"|"materials"|"photo")}>
          <img src={`/workbench/atelier-assets/${image}`} alt="" loading="lazy"/>
          <div><h3>{title}</h3><p>{description}</p></div><Arrow diagonal/>
        </a>)}</div>
      </div></section>}
      {!section && <><Claims/><StartHere/></>}
    </main>
    <SiteFooter/>
    <Reveal/>
    {item && <Player item={item} close={closePlayer}/>}
  </>;
}
