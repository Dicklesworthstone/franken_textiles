"use client";

import dynamic from "next/dynamic";
import { useCallback, useEffect, useRef, useState } from "react";
import { repo, studies, type Study, type View } from "./studies";

const Studio = dynamic(() => import("../components/Studio"), { ssr: false });
type Exhibit = { study: Study; film: boolean };
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
  const { study } = item;
  const dialog = useRef<HTMLDialogElement>(null);
  const [film, setFilm] = useState(item.film);
  const [frame, setFrame] = useState(study.mode === "construction" ? 0 : 1);
  const [mode, setMode] = useState<View>(study.mode);
  const [wire, setWire] = useState(study.id === "construction");
  const [seams, setSeams] = useState<string[]>([]);
  const [seam, setSeam] = useState("");
  const [resetKey, reset] = useState(0);
  const [playing, play] = useState(false);
  const [available, setAvailable] = useState(false);
  const [failed, setFailed] = useState(false);
  const ready = useCallback(() => setAvailable(true), []);
  const error = useCallback(() => { setFailed(true); play(false); }, []);
  const live = !film && !failed;
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
    <header className="player-head"><div><span className="mono">{live ? "Interactive study" : "Studio film"} / Fig. {study.figure}</span>
      <h2 id="player-title">{study.title}</h2></div>
      <button className="close" onClick={close} aria-label="Close study">×</button></header>
    {live ? <div className="stage"><Studio garment={study.garment} mode={mode}
      frame={mode === "strain" ? 1 : frame} wire={wire} resetKey={resetKey} onReady={ready} onError={error}
      seam={mode === "construction" ? seam : ""} onSeams={setSeams}/>
      {!available && <span className="loading mono" role="status">Loading geometry…</span>}
      {available && <span className="orbit-note mono">Drag to orbit · Scroll to zoom</span>}
    </div> : <video className="movie" key={study.id} controls playsInline autoPlay={!failed}
      poster={`/media/${study.id}.jpg`} src={`/media/${study.id}.mp4`} aria-label={`${study.title} turntable film`}/>}
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
      </div>}
      {mode === "strain" ? <div className="strain-key"><span/>Endpoint edge strain · −3% compression to +3% tension</div> :
        <div className="transport"><button disabled={!available} aria-label={playing ? "Pause replay" : "Play replay"} onClick={() => {
          if (playing) play(false);
          else { const start = frame >= 1 ? 0 : frame; setFrame(start); animationStart.current = performance.now() - start * 9500; play(true); }
        }}>{playing ? "Ⅱ" : "▶"}</button>
          <input aria-label="Fold replay position" type="range" min="0" max="1" step="0.001" value={frame}
            onChange={e => { play(false); setFrame(Number(e.target.value)); }}/>
          <span className="mono phase">{phase}</span></div>}
    </div>}
    <footer className="player-foot">{live ? <p>Native Rust fold replay on a synthetic body.</p> :
      <a download={`frankentextiles-${study.id}.mp4`} href={`/media/${study.id}.mp4`}>Download MP4 ↓</a>}
      <button onClick={changePlayer}>{live ? "Watch film" : "Open 3D study"} <Arrow/></button></footer>
  </dialog>;
}

export default function Home() {
  const [filter, setFilter] = useState("All studies");
  const [item, setItem] = useState<Exhibit | null>(null);
  const [capture, setCapture] = useState<Study>();
  useEffect(() => {
    const id = new URLSearchParams(location.search).get("capture");
    if (id) setCapture(studies.find(study => study.id === id));
  }, []);
  if (capture) return <Capture study={capture}/>;
  return <>
    <a className="skip" href="#studies">Skip to studies</a>
    <header className="mast"><div className="wrap nav">
      <a className="wordmark" href="#" aria-label="FrankenTextiles home"><Mark/>Franken<span>Textiles</span></a>
      <nav aria-label="Main navigation"><a href="#studies">Studies</a><a href="#films">Films</a>
      <a className="worklink" href="/workbench/index.html">Open workbench <Arrow diagonal/></a></nav>
    </div></header>
    <main>
      <section className="hero" aria-labelledby="hero-title"><div className="wrap hero-grid">
        <div className="hero-text"><div className="eyebrow"><i/> Computational textile studio</div>
          <h1 id="hero-title">The drape<br/><em>studies.</em></h1>
          <p>From flat pattern to folded cloth. Explore the geometry, seams, and movement of garments sewn by the FrankenTextiles solver.</p>
          <div className="actions"><a className="primary" href="#studies">Explore studies <Arrow/></a>
            <button className="watch" onClick={() => setItem({ study: studies[0], film: true })}><span className="play-ring">▶</span> Watch the film</button></div>
        </div>
        <figure className="hero-plate"><div className="plate-grid"/><div className="hero-picture" role="img" aria-label="Copper bolero with an open front and a cutaway revealing the triangular mesh on one sleeve"/>
          <div aria-hidden="true" className="crop tl"/><div aria-hidden="true" className="crop tr"/>
          <div aria-hidden="true" className="crop bl"/><div aria-hidden="true" className="crop br"/>
          <span className="plate-id mono">FTX — 001</span>
          <button className="orbit-link mono" aria-label="Open the Zouave bolero 3D study" onClick={() => setItem({ study: studies[0], film: false })}><i/> View in 3D <Arrow diagonal/></button>
          <figcaption><span className="mono">Fig. 01</span><em>Zouave bolero</em></figcaption>
        </figure>
      </div></section>
      <section id="studies" className="paper"><div className="wrap collection">
        <div className="section-top"><div><div className="eyebrow dark"><i/> Interactive WebGL</div><h2>Study collection<span>.</span></h2></div>
          <div className="filters" aria-label="Filter studies">{["All studies","Garments","Construction"].map(k =>
            <button key={k} aria-pressed={filter === k} onClick={() => setFilter(k)}>{k}</button>)}</div></div>
        <div className="cards">{studies.filter(s => filter === "All studies" || s.group === filter).map(s =>
          <button key={s.id} className="card" onClick={() => setItem({study:s,film:false})}>
            <div className="thumb"><img src={`/media/${s.id}.jpg`} alt="" loading="lazy"/><span className="thumb-id mono">Fig. {s.figure}</span><span className="launch"><Arrow diagonal/></span></div>
            <div className="card-text"><span className="mono">{s.label}</span><h3>{s.title}</h3><p>{s.description}</p></div>
          </button>)}</div>
      </div></section>
      <section id="films" className="films"><div className="wrap film-grid">
        <div><div className="eyebrow"><i/> Rendered with FFmpeg</div><h2>Screening room<span>.</span></h2>
          <p>Short films of the recorded folds and a closer look at the sewn mesh.</p></div>
        <div className="film-list">{studies.map(s => <button key={s.id} onClick={() => setItem({study:s,film:true})}>
          <img src={`/media/${s.id}.jpg`} alt="" loading="lazy"/><span><small className="mono">Film {s.figure}</small><strong>{s.title}</strong></span><span className="play-ring">▶</span></button>)}</div>
      </div></section>
      <section className="paper"><div className="wrap bench"><div><div className="eyebrow dark"><i/> Pattern workbench</div><h2>Try your own pattern<span>.</span></h2>
        <p>Draft, check seams, and export printable pieces locally in your browser. The studies use synthetic bodies. Physical fit takes a muslin or a dressed photo.</p></div>
        <a className="primary ink" href="/workbench/index.html">Open workbench <Arrow diagonal/></a>
      </div></section>
    </main>
    <footer className="base"><div className="wrap"><a className="wordmark" href="#"><Mark/>Franken<span>Textiles</span></a><a href={repo}>Source on GitHub <Arrow diagonal/></a></div></footer>
    {item && <Player item={item} close={() => setItem(null)}/>}
  </>;
}
