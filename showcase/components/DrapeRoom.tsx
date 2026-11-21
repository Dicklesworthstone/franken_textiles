"use client";

import dynamic from "next/dynamic";
import { useCallback, useEffect, useRef, useState } from "react";
import experiment from "../public/exhibits/poses/runs.json";
import joint from "../public/exhibits/poses/ensemble.json";
import { PlaybackMark } from "./InlineFilm";
import BoleroReplay from "./BoleroReplay";
import type { StudioMode } from "./garmentMesh";
import "./drape-room.css";

const Scene=dynamic(()=>import("./DrapeRoomScene"),{ssr:false});
const CoupledScene=dynamic(()=>import("./CoupledScene"),{ssr:false});
const chests=[48,52,54] as const, angles=[35,75,90] as const;
const modes: [StudioMode,string][]=[["cloth","Wool"],["construction","Cut panels"],["strain","Strain"],["contact","Contact"]];
const endpointMode=(mode:StudioMode)=>mode==="strain"||mode==="contact";
const phase=(frame:number,firstBodyFrame:number,gravityStart:number,last:number)=>frame===0?"Flat pattern":frame<firstBodyFrame?"Shape the panels":frame<gravityStart?"Sewing":frame<last?"Gravity drape":"Final frame";

export default function DrapeRoom({home=false}:{home?:boolean}){
  const [chest,setChest]=useState<typeof chests[number]>(52),[angle,setAngle]=useState<typeof angles[number]>(35);
  const [mode,setMode]=useState<StudioMode>("cloth"),[selectedFrame,setFrame]=useState<number|null>(null);
  const [full,setFull]=useState(false);
  const [playing,play]=useState(false),[reset,resetView]=useState(0);
  const [ready,setReady]=useState(""),[failed,setFailed]=useState("");
  const section=useRef<HTMLElement>(null),start=useRef(0);
  const key=`${chest*10}-${angle}` as keyof typeof experiment.cases;
  const run=experiment.cases[key];
  const flags=full?joint.gating:run.flags;
  const phases=full?joint.phases:run.phases,end=full?joint.frames-1:run.sourceFrames-1;
  const frame=selectedFrame??end;
  const firstBody=phases.fold+phases.zip-1;
  const stageLabel=phase(frame,firstBody,firstBody+1+phases.sew,end)+" · "+(frame+1)+"/"+(end+1);
  const shown=ready===key && failed!==key;
  const onReady=useCallback((id:string)=>setReady(id),[]);
  const onError=useCallback((id:string)=>{setFailed(id);play(false);},[]);
  useEffect(()=>{
    const params=new URLSearchParams(location.search);
    if(params.get("outfit")==="joint"){setFull(true);return;}
    const c=chests.find(n=>String(n)===params.get("chest"));
    const a=angles.find(n=>String(n)===params.get("arm"));
    const surface=modes.find(([id])=>id===params.get("surface"));
    if(c)setChest(c);if(a)setAngle(a);if(surface)setMode(surface[0]);
  },[]);
  const Heading=home?"h1":"h2";
  function beginChange(){setFull(false);setReady("");setFailed("");play(false);setFrame(null);}
  function switchGarment(){beginChange();setFull(!full);setChest(52);setAngle(35);setMode("cloth");}
  useEffect(()=>{
    if(!playing)return;
    let raf=0;
    const tick=(now:number)=>{
      const next=Math.min(end,Math.floor((now-start.current)/60));
      setFrame(next);
      if(next<end)raf=requestAnimationFrame(tick);else play(false);
    };
    const observer=new IntersectionObserver(([entry])=>{if(!entry.isIntersecting)play(false);});
    observer.observe(section.current!);
    const visibility=()=>{if(document.hidden)play(false);};
    document.addEventListener("visibilitychange",visibility);
    raf=requestAnimationFrame(tick);
    return ()=>{cancelAnimationFrame(raf);observer.disconnect();document.removeEventListener("visibilitychange",visibility);};
  },[playing,end]);
  function replay(){
    if(playing){play(false);return;}
    if(endpointMode(mode))setMode("cloth");
    const initial=frame>=end?0:frame;
    setFrame(initial);start.current=performance.now()-initial*60;play(true);
  }
  return <section className="dr-room" id="drape-room" ref={section} aria-labelledby="room-title">
    <div className="wrap dr-layout">
      <div className="dr-intro"><p className="dr-eyebrow"><i/>The drape room</p>
        <Heading id="room-title">Cloth in<br/><em>motion.</em></Heading>
        <p className="dr-deck">Raise the arms to see where the bolero pulls closer. Choose the coupled outfit to watch the jacket hem and trouser waistband settle against each other.</p>
      </div>
      <div className="dr-settings">
        <fieldset><legend>Chest target <small>cm</small></legend><div className="dr-options">{chests.map(c=><button key={c} aria-label={c+" centimetre chest"} aria-pressed={chest===c} onClick={()=>{if(c!==chest){beginChange();setChest(c);}}}>{c}</button>)}</div></fieldset>
        <fieldset><legend>Arm angle <small>from the side</small></legend><div className="dr-options">{angles.map(a=><button key={a} aria-label={a+" degree arm pose"} aria-pressed={angle===a} onClick={()=>{if(a!==angle){beginChange();setAngle(a);}}}>{a}°</button>)}</div></fieldset>
        <div className="dr-patterns"><span>Full-size sewing patterns</span><a className="dr-pattern" download href="/patterns/bolero.pdf">Bolero <span aria-hidden="true">↓</span></a>
          <a className="dr-pattern" download href="/patterns/trousers.pdf">Trousers <span aria-hidden="true">↓</span></a></div>
        <a className="dr-next-study" href="/studies/trousers/">Explore the trouser cross-sections <span aria-hidden="true">↗</span></a>
        <fieldset><legend>{full?"Sew both garments":"Sew the bolero"}</legend><div className="dr-options">{[[0,"Cut"],[firstBody,"Join seams"],[end,"Dressed"]].map(([at,label])=>
          <button key={at} aria-pressed={frame===at} disabled={!shown} onClick={()=>{play(false);setFrame(Number(at));setMode(full||at===end?"cloth":"construction");}}>{label}</button>)}</div></fieldset>
      </div>
      <figure className="dr-display">
        <div className="dr-stage"><div className="dr-stage-head"><span>{full?"Bolero and trousers":"Zouave bolero"}</span><button className="dr-length" aria-pressed={full} onClick={switchGarment}>{full?"Jacket only":"Coupled outfit"}</button></div>
          {!shown && (full?<img alt="Recorded endpoint of the coupled jacket and trousers" className="dr-poster" src="/media/coupled-outfit.webp"/>:<picture><source media="(max-width:680px)" srcSet="/media/drape-room-mobile.webp"/><img alt={failed===key?"Recorded reference view of the bolero on its display form":""} className="dr-poster" src="/media/drape-room.webp" fetchPriority="high"/></picture>)}
          {failed!==key && <div className={"dr-mount"+(shown?" is-ready":"")}>{full?<CoupledScene frame={frame} onReady={onReady} onError={onError}/>:<Scene run={key} timeline={run} layoutIntegrity={experiment.layoutIntegrity} strain={run.edgeStrainPercent} frame={frame} mode={mode} reset={reset} onReady={onReady} onError={onError}/>}</div>}
          {!full&&<div className="dr-stage-foot"><div className="dr-modes" aria-label="Garment surface">{modes.map(([m,name])=><button key={m} aria-pressed={m===mode} disabled={!shown} onClick={()=>{play(false);setMode(m);if(endpointMode(m))setFrame(end);}}>{name}</button>)}</div>
            <button className="dr-reset" disabled={!shown} onClick={()=>resetView(n=>n+1)} aria-label="Reset 3D camera">Reset view</button></div>}
          {mode==="strain"&&shown&&<div className="dr-legend"><span>−3%</span><i/><span>+3%</span></div>}
          {mode==="contact"&&shown&&<div className="dr-legend dr-force" aria-label="Body contact force in newtons">
            <span>0</span><span>0.02</span><span>0.1</span><span>1+ N</span></div>}
          {shown&&!full&&<button className="dr-flat" aria-label={frame===0?"Return to sewn bolero":"Lay the five cut pieces flat"} onClick={()=>{
            play(false);setMode(frame===0?"cloth":"construction");setFrame(frame===0?end:0);
          }}><img src="/exhibits/poses/flat-cut.svg" alt=""/><span>{frame===0?"See sewn":"Lay flat"} <b aria-hidden="true">↗</b></span></button>}
        </div>
        <figcaption>
          <div className="dr-readings"><div><span>Fitted chest</span><strong>{((full?joint.chestMm:run.chest.body_mm)/10).toFixed(1)} <small>cm</small></strong></div>
            <div><span>{full?"Waistband shift":"Mean chest gap"}</span><strong data-gap={key}>{(full?joint.waistbandShift:run.chestClearance).toFixed(1)} <small>mm</small></strong></div>
            {mode==="contact"?<div><span>Peak contact force</span><strong>{run.peakBodyForceN.toFixed(2)} <small>N</small></strong></div>:
              <div><span>Edge strain <abbr title="95th percentile">P95</abbr></span><strong>{(full?joint.strain:run.edgeStrainPercent).p95.toFixed(2)} <small>%</small></strong></div>}</div>
          {failed===key?<p className="dr-fallback" role="status">Reference still · 3D unavailable. {full?<a href="/exhibits/poses/ensemble.json">Read the joint solver result</a>:<a href="/media/drape-room.mp4">Watch the pose comparison</a>}</p>:
            <div className="dr-timeline"><button onClick={replay} disabled={!shown} aria-label={playing?"Pause sewing replay":"Play sewing replay"}><PlaybackMark playing={playing}/></button>
              <input disabled={!shown} type="range" aria-label="Sewing replay frame" aria-valuetext={stageLabel} min="0" max={end} value={frame}
                onChange={event=>{play(false);setFrame(Number(event.target.value));if(endpointMode(mode))setMode("cloth");}}/>
              <span>{shown?stageLabel:"Loading 3D…"}</span></div>}
        </figcaption>
      </figure>
      <details className="dr-method"><summary>How the cloth was simulated <span>{!(full?joint.settled:run.settled)?"Cloth still moving":flags.length?"Review solver flags":"Settled"}</span></summary>
        <p>The display form sits 2.5 mm inside the recorded body so its surface stays behind the jacket at the shoulders. The cloth positions and clearance readings come from the original solve.</p>
        {full?<><p>Both garments share one native cloth solve on a synthetic 92 cm body. Twelve panels bend separately on the board. The replay cuts to their checked placement on the form. The next 12 sewing steps and 36 gravity steps include contact with the body and between garments. Both cuts use the same synthetic material properties. The colors distinguish the garments.</p>
          {joint.advisories.map(note=><p key={note}>Starting placement advisory · {note}.</p>)}
          <p><a href="/exhibits/poses/ensemble-request.json" download>Download the two-pattern solver input</a> · <a href="/exhibits/poses/ensemble.json" download>Download endpoint readings</a></p></>:<><p>Each combination uses the same flat cut and fabric on a synthetic 92 cm form. The panels bend separately on the board, then move to the checked starting position on the wearer. The archive check tests that position, {phases.sew} sewing steps and {phases.drape} gravity steps against the full body and the other cloth panels. The readings describe the endpoint.</p>
        <p>The mean gap averages the distance at the chest ring. Edge strain is the change in edge length over the whole garment. This run’s endpoint range is {run.edgeStrainPercent.min}% to +{run.edgeStrainPercent.max}%. Contact shows the magnitude of the body’s barrier force at each welded cloth vertex, in newtons. Physical fit requires a sewn sample.</p>
        <BoleroReplay chest={chest} angle={angle}/>
        </>}
        {flags.map(flag=><p key={flag} className="dr-warning">{flag}</p>)}
        <div className="dr-sources"><a href="/exhibits/poses/runs.json" download>Download solver summaries and body-fit residuals ↓</a>
          <a href={full?"?outfit=joint#drape-room":`?chest=${chest}&arm=${angle}&surface=${mode}#drape-room`}>Link to this setup</a><a href="/studies/trousers/?bench=posture&run=squat-25">See the clothed quarter squat</a></div>
      </details>
    </div>
  </section>;
}
