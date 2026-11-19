"use client";

import dynamic from "next/dynamic";
import { useCallback, useEffect, useRef, useState } from "react";
import { repo, studies, type Study, type View } from "./studies";
import LabBench from "../components/LabBench";
import TraceReveal from "../components/TraceReveal";

const Studio = dynamic(() => import("../components/Studio"), { ssr: false });
type Exhibit = { study: Study; film: boolean } | { tour: true };
type CaptureFn = (progress: number, yaw: number) => Promise<void>;
declare global { interface Window { ftxFrame?: CaptureFn } }

function Mark() {
  return <span className="mark" aria-hidden="true"><i/><i/><i/><i/></span>;
}

function Arrow({ diagonal = false }: { diagonal?: boolean }) {
  return <span aria-hidden="true" className="arrow">{diagonal ? "↗" : "→"}</span>;
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
  const mediaId = study?.id ?? "studio-tour";
  const title = study?.title ?? "A tour of the pattern room";
  const dialog = useRef<HTMLDialogElement>(null);
  const [film, setFilm] = useState("film" in item ? item.film : true);
  const [frame, setFrame] = useState(study?.mode === "construction" ? 0 : 1);
  const [mode, setMode] = useState<View>(study?.mode ?? "cloth");
  const [wire, setWire] = useState(study?.id === "construction");
  const [seams, setSeams] = useState<string[]>([]);
  const [seam, setSeam] = useState("");
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
      seam={mode === "construction" ? seam : ""} spread={spread} onSeams={setSeams} onStrain={setStrain}/>
      {!available && <span className="loading mono" role="status">Loading geometry…</span>}
      {available && <span className="orbit-note mono">Drag to orbit · Scroll to zoom</span>}
    </div> : <video className="movie" key={mediaId} controls playsInline autoPlay={!failed}
      poster={`/media/${mediaId}.jpg`} src={`/media/${mediaId}.mp4`} aria-label={title}/>}
    {failed && <p role="status" className="fallback">WebGL is unavailable on this device. The film shows the same garment.</p>}
    {live && <div className="controls">
      <div className="control-row"><div className="seg" aria-label="Garment view">
        {(["cloth", "construction", "strain"] as const).map((v, i) => <button key={v} aria-pressed={mode === v}
          onClick={() => { setMode(v); if (v === "strain") play(false); }}>{["Cloth","Panels","Strain"][i]}</button>)}
      </div><div className="tools"><label><input type="checkbox" checked={wire} onChange={e => setWire(e.target.checked)}/> Mesh</label>
        <button onClick={() => reset(k => k + 1)}>Reset view</button></div></div>
      {mode === "construction" && seams.length > 0 && <div className="seam-control">
        <label>Inspect seam <select value={seam} onChange={e => setSeam(e.target.value)}>
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
        }}>{playing ? "Ⅱ" : "▶"}</button>
          <input aria-label="Fold replay position" type="range" min="0" max="1" step="0.001" value={frame}
            onChange={e => { play(false); setFrame(Number(e.target.value)); }}/>
          <span className="mono phase">{phase}</span></div></>}
    </div>}
    <footer className="player-foot">{live ? <p>Native Rust fold replay on a synthetic body.</p> :
      <a download={`frankentextiles-${mediaId}.mp4`} href={`/media/${mediaId}.mp4`}>Download MP4 ↓</a>}
      {study ? <button onClick={changePlayer}>{live ? "Watch film" : "Open 3D study"} <Arrow/></button> :
        <a href="#studio" onClick={close}>Try the studios <Arrow/></a>}</footer>
  </dialog>;
}

export default function Home() {
  const [filter, setFilter] = useState("All studies");
  const [item, setItem] = useState<Exhibit | null>(null);
  const [capture, setCapture] = useState<Study>();
  useEffect(() => {
    const params = new URLSearchParams(location.search);
    const id = params.get("capture");
    if (id) setCapture(studies.find(study => study.id === id));
    else if (params.get("film") === "studio-tour") setItem({ tour: true });
    else {
      const study = studies.find(s => s.id === (params.get("study") ?? params.get("film")));
      if (study) setItem({ study, film: !params.has("study") });
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
    <a className="skip" href="#studio">Skip to the studios</a>
    <header className="mast"><div className="wrap nav">
      <a className="wordmark" href="#" aria-label="FrankenTextiles home"><Mark/>Franken<span>Textiles</span></a>
      <nav aria-label="Main navigation"><a href="#studio">Studios</a><a href="#studies">Drape gallery</a><a href="#films">Films</a>
      <a className="worklink" href="/workbench/index.html">Open workbench <Arrow diagonal/></a></nav>
    </div></header>
    <main>
      <section className="atelier-hero" aria-labelledby="hero-title">
        <img className="cover-photo" src="/media/atelier-cover.webp" alt="" fetchPriority="high"/>
        <div className="wrap hero-inner"><div className="hero-copy">
          <p className="kicker">Open-source sewing studio</p>
          <h1 id="hero-title">The pattern<br/><em>room.</em></h1>
          <p>Draft the pieces, follow the fold, and see where every stitch lands. A sewing studio you can explore in your browser.</p>
          <div className="actions"><a className="primary" href="#studio">Try the studios <span aria-hidden="true">↓</span></a>
            <button className="watch" onClick={() => setItem({ tour: true })}><span className="play-ring">▶</span> Watch the studio tour</button></div>
        </div></div>
      </section>
      <LabBench/>
      <section id="studies" className="paper"><div className="wrap collection">
        <div className="section-top"><div><p className="kicker">Cloth in three dimensions</p><h2>The drape gallery<span>.</span></h2></div>
          <div className="filters" aria-label="Filter studies">{["All studies","Garments","Construction"].map(k =>
            <button key={k} aria-pressed={filter === k} onClick={() => setFilter(k)}>{k}</button>)}</div></div>
        <div className="cards">{studies.filter(s => filter === "All studies" || s.group === filter).map(s =>
          <button key={s.id} className="card" onClick={() => setItem({study:s,film:false})}>
            <div className="thumb"><img src={`/media/${s.id}.jpg`} alt="" loading="lazy"/><span className="thumb-id mono">Fig. {s.figure}</span><span className="launch"><Arrow diagonal/></span></div>
            <div className="card-text"><span className="mono">{s.detail}</span><h3>{s.title}</h3><p>{s.description}</p></div>
          </button>)}</div>
      </div></section>
      <TraceReveal/>
      <section id="films" className="films"><div className="wrap film-grid">
        <div><p className="kicker">Films</p><h2>Screening room<span>.</span></h2>
          <p>Watch a pattern move from the board to the needle, or look closely at a single garment.</p></div>
        <div className="film-list"><button onClick={() => setItem({ tour: true })}>
          <img src="/media/studio-tour.jpg" alt="" loading="lazy"/><span><small className="mono">Studio tour</small><strong>From pattern to stitch</strong></span><span className="play-ring">▶</span></button>
          {studies.map(s => <button key={s.id} onClick={() => setItem({study:s,film:true})}>
          <img src={`/media/${s.id}.jpg`} alt="" loading="lazy"/><span><small className="mono">Film {s.figure}</small><strong>{s.title}</strong></span><span className="play-ring">▶</span></button>)}</div>
      </div></section>
      <section className="paper"><div className="wrap bench"><div><p className="kicker">Local browser tools</p><h2>Bring your own project<span>.</span></h2>
        <p>Start from a scan, a material sample, or a measured draft. Your files stay in your browser. The drape studies use synthetic bodies. Physical fit takes a muslin or a dressed photo.</p></div>
        <div className="bench-links"><a href="/workbench/index.html">Pattern workbench <Arrow diagonal/></a><a href="/workbench/trace.html">Trace a pattern sheet <Arrow diagonal/></a>
          <a href="/workbench/materials.html">Material workbench <Arrow diagonal/></a><a href="/workbench/photo.html">Photo measurements <Arrow diagonal/></a></div>
      </div></section>
    </main>
    <footer className="base"><div className="wrap"><a className="wordmark" href="#"><Mark/>Franken<span>Textiles</span></a><a href={repo}>Source on GitHub <Arrow diagonal/></a></div></footer>
    {item && <Player item={item} close={closePlayer}/>}
  </>;
}
