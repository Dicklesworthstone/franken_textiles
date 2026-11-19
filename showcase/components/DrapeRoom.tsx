"use client";

import dynamic from "next/dynamic";
import { useCallback, useEffect, useRef, useState } from "react";
import experiment from "../public/exhibits/poses/runs.json";
import { PlaybackMark } from "./InlineFilm";
import type { StudioMode } from "./garmentMesh";
import "./drape-room.css";

const Scene=dynamic(()=>import("./DrapeRoomScene"),{ssr:false});
const chests=[48,52,54] as const, angles=[35,75,90] as const;
const modes: [StudioMode,string][]=[["cloth","Wool"],["construction","Cut panels"],["strain","Strain"],["contact","Contact"]];
const endpointMode=(mode:StudioMode)=>mode==="strain"||mode==="contact";
const phase=(frame:number)=>frame===0?"Flat pattern":frame<70?"Wrapping":frame<88?"Joining panels":frame<100?"Sewing":frame<135?"Gravity drape":"Final frame";
type LowerRun={available:boolean;flags?:string[];reason?:string;waistbandShiftMm?:number};

export default function DrapeRoom({home=false}:{home?:boolean}){
  const [chest,setChest]=useState<typeof chests[number]>(52),[angle,setAngle]=useState<typeof angles[number]>(35);
  const [mode,setMode]=useState<StudioMode>("cloth"),[frame,setFrame]=useState(135);
  const [full,setFull]=useState(false);
  const [playing,play]=useState(false),[reset,resetView]=useState(0);
  const [ready,setReady]=useState(""),[failed,setFailed]=useState("");
  const section=useRef<HTMLElement>(null),start=useRef(0);
  const key=`${chest*10}-${angle}` as keyof typeof experiment.cases;
  const run=experiment.cases[key];
  const lower:LowerRun=run.trousers;
  const flags=[...run.flags,...(lower.flags??[]).map(flag=>"Trousers · "+flag)];
  const shown=ready===key && failed!==key;
  const onReady=useCallback((id:string)=>setReady(id),[]);
  const onError=useCallback((id:string)=>{setFailed(id);play(false);},[]);
  useEffect(()=>{
    const params=new URLSearchParams(location.search);
    const c=chests.find(n=>String(n)===params.get("chest"));
    const a=angles.find(n=>String(n)===params.get("arm"));
    const surface=modes.find(([id])=>id===params.get("surface"));
    if(c)setChest(c);if(a)setAngle(a);if(surface)setMode(surface[0]);
  },[]);
  const Heading=home?"h1":"h2";
  function beginChange(){setReady("");setFailed("");play(false);setFrame(135);}
  useEffect(()=>{
    if(!playing)return;
    let raf=0;
    const tick=(now:number)=>{
      const next=Math.min(135,Math.floor((now-start.current)/60));
      setFrame(next);
      if(next<135)raf=requestAnimationFrame(tick);else play(false);
    };
    const observer=new IntersectionObserver(([entry])=>{if(!entry.isIntersecting)play(false);});
    observer.observe(section.current!);
    const visibility=()=>{if(document.hidden)play(false);};
    document.addEventListener("visibilitychange",visibility);
    raf=requestAnimationFrame(tick);
    return ()=>{cancelAnimationFrame(raf);observer.disconnect();document.removeEventListener("visibilitychange",visibility);};
  },[playing]);
  function replay(){
    if(playing){play(false);return;}
    if(endpointMode(mode))setMode("cloth");
    const initial=frame>=135?0:frame;
    setFrame(initial);start.current=performance.now()-initial*60;play(true);
  }
  return <section className="dr-room" id="drape-room" ref={section} aria-labelledby="room-title">
    <div className="wrap dr-layout">
      <div className="dr-intro"><p className="dr-eyebrow"><i/>The drape room</p>
        <Heading id="room-title">Cloth in<br/><em>motion.</em></Heading>
        <p className="dr-deck">Five flat pieces become a wool bolero. Change the figure’s chest or raise its arms to see where the sewn cloth pulls closer.</p>
      </div>
      <div className="dr-settings">
        <fieldset><legend>Chest target <small>cm</small></legend><div className="dr-options">{chests.map(c=><button key={c} aria-label={c+" centimetre chest"} aria-pressed={chest===c} onClick={()=>{if(c!==chest){beginChange();setChest(c);}}}>{c}</button>)}</div></fieldset>
        <fieldset><legend>Arm angle <small>from the side</small></legend><div className="dr-options">{angles.map(a=><button key={a} aria-label={a+" degree arm pose"} aria-pressed={angle===a} onClick={()=>{if(a!==angle){beginChange();setAngle(a);}}}>{a}°</button>)}</div></fieldset>
        <div className="dr-patterns"><span>Full-size sewing patterns</span><a className="dr-pattern" download href="/patterns/bolero.pdf">Bolero <span aria-hidden="true">↓</span></a>
          <a className="dr-pattern" download href="/patterns/trousers.pdf">Trousers <span aria-hidden="true">↓</span></a></div>
      </div>
      <figure className="dr-display">
        <div className="dr-stage"><div className="dr-stage-head"><span>Zouave bolero</span><button className="dr-length" disabled={!shown||!lower.available} aria-pressed={full} onClick={()=>setFull(v=>!v)}>{full?"Jacket detail":"Full figure"}</button></div>
          {!shown && <picture><source media="(max-width:680px)" srcSet="/media/drape-room-mobile.webp"/><img alt={failed===key?"Recorded reference view of the bolero on its display form":""} className="dr-poster" src="/media/drape-room.webp" fetchPriority="high"/></picture>}
          {failed!==key && <div className={"dr-mount"+(shown?" is-ready":"")}><Scene run={key} strain={run.edgeStrainPercent} frame={frame} mode={mode} reset={reset} full={full} outfit={lower.available} registration={run.headRegistration} onReady={onReady} onError={onError}/></div>}
          <span className="dr-reverse">Reverse</span>
          <div className="dr-stage-foot"><div className="dr-modes" aria-label="Garment surface">{modes.map(([m,name])=><button key={m} aria-pressed={m===mode} disabled={!shown} onClick={()=>{play(false);setMode(m);if(endpointMode(m))setFrame(135);}}>{name}</button>)}</div>
            <button className="dr-reset" disabled={!shown} onClick={()=>resetView(n=>n+1)} aria-label="Reset 3D camera">Reset view</button></div>
          {mode==="strain"&&shown&&<div className="dr-legend"><span>−3%</span><i/><span>+3%</span></div>}
          {mode==="contact"&&shown&&<div className="dr-legend dr-force" aria-label="Body contact force in newtons">
            <span>0</span><span>0.02</span><span>0.1</span><span>1+ N</span></div>}
          {shown&&<button className="dr-flat" aria-label={frame===0?"Return to sewn bolero":"Lay the five cut pieces flat"} onClick={()=>{
            play(false);setMode(frame===0?"cloth":"construction");setFrame(frame===0?135:0);
          }}><img src="/exhibits/poses/flat-cut.svg" alt=""/><span>{frame===0?"See sewn":"Lay flat"} <b aria-hidden="true">↗</b></span></button>}
        </div>
        <figcaption>
          <div className="dr-readings"><div><span>Fitted chest</span><strong>{(run.chest.body_mm/10).toFixed(1)} <small>cm</small></strong></div>
            <div><span>Mean chest gap</span><strong data-gap={key}>{run.chestClearance.toFixed(1)} <small>mm</small></strong></div>
            {mode==="contact"?<div><span>Peak contact force</span><strong>{run.peakBodyForceN.toFixed(2)} <small>N</small></strong></div>:
              <div><span>Edge strain <abbr title="95th percentile">P95</abbr></span><strong>{run.edgeStrainPercent.p95.toFixed(2)} <small>%</small></strong></div>}</div>
          {failed===key?<p className="dr-fallback" role="status">Reference still · 3D unavailable. <a href="/media/drape-room.mp4">Watch the pose comparison</a></p>:
            <div className="dr-timeline"><button onClick={replay} disabled={!shown} aria-label={playing?"Pause sewing replay":"Play sewing replay"}><PlaybackMark playing={playing}/></button>
              <input disabled={!shown} type="range" aria-label="Sewing replay frame" aria-valuetext={phase(frame)+", frame "+frame} min="0" max="135" value={frame}
                onChange={event=>{play(false);setFrame(Number(event.target.value));if(endpointMode(mode))setMode("cloth");}}/>
              <span>{shown?phase(frame):"Loading 3D…"}</span></div>}
        </figcaption>
      </figure>
      <details className="dr-method"><summary>How the cloth was simulated <span>{flags.length?(run.settled?"Contact thickness incomplete":"Cloth still moving"):"Settled"}</span></summary>
        <p>Each combination replays a separate FrankenTextiles run with the same pattern and fabric. The first 88 frames illustrate assembly. The final 48 show native sewing and gravity steps on the fitted synthetic 92 cm form. The readings above describe the endpoint. The wool trim and AI-generated facial coloring are illustrative.</p>
        <p>The mean gap averages the distance at the chest ring. Edge strain is the change in edge length over the whole garment. This run’s endpoint range is {run.edgeStrainPercent.min}% to +{run.edgeStrainPercent.max}%. Contact shows the magnitude of the body’s barrier force at each welded cloth vertex, in newtons. Physical fit requires a sewn sample.</p>
        {lower.available?<p>The trousers are shown at their own drape endpoint on the identical posed body. Each garment was solved separately. Interactions between the two layers were outside these runs.</p>:<p>Trouser replay unavailable for this pose. {lower.reason}</p>}
        {flags.map(flag=><p key={flag} className="dr-warning">{flag}</p>)}
        <div className="dr-sources"><a href="/exhibits/poses/runs.json" download>Download solver summaries and body-fit residuals ↓</a>
          <a href={`?chest=${chest}&arm=${angle}&surface=${mode}#drape-room`}>Link to this setup</a><a href="/sections/films/?film=drape-room">Watch the pose comparison</a></div>
      </details>
    </div>
  </section>;
}
