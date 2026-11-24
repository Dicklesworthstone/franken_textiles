"use client";

import dynamic from "next/dynamic";
import {useEffect,useMemo,useRef,useState} from "react";
import InlineFilm,{filmFile} from "../InlineFilm";
import SiteHeader from "../site/SiteHeader";
import SiteFooter from "../site/SiteFooter";
import Crumbs from "../site/Crumbs";
import ConstructionStudy from "../construction/ConstructionStudy";
import type {Model} from "./Scene";
import {loadSurface,loadTopology,loadSewing,section,type Layout,type Cut} from "./surface";
import type {SewingPhases} from "../construction/replay";

const Scene=dynamic(()=>import("./Scene"),{ssr:false});
type Run={id:string;label:string;surface?:string;posture:string;fraction:number;stiffness:number;seatGap?:number;meshKey?:string;clothVertices?:number;spacingMm?:number;frames?:number[];phases?:SewingPhases;
  waistbandShift?:number;strain?:{p95:number};flags?:string[];refusal?:{code:string;message:string;repairs:string[]}};
type Study=Layout&{cases:Run[]};
const stations=["Posture","Elastic","Cross-section"] as const;
type Station=typeof stations[number];

function Drawing({body,cloth,height}:{body:Cut|null;cloth:Cut|null;height:number}){
  const cuts=[body,cloth].filter((c):c is Cut=>!!c);
  const cy=cuts.length?(Math.min(...cuts.map(c=>c.back))+Math.max(...cuts.map(c=>c.front)))/2:75;
  const top=cy-139;
  return <div className="ts-diagram"><div className="ts-figure-label"><span>Horizontal section</span><b>{height} mm</b></div>
    <svg role="img" aria-label={"Body and trouser contours viewed from above at "+height+" millimetres above the sole"} viewBox={`-217 ${top} 434 278`}>
      <defs><pattern id="ts-squared" width="20" height="20" patternUnits="userSpaceOnUse"><path d="M20 0H0V20" fill="none" stroke="#cabfa9" strokeWidth=".65"/></pattern></defs>
      <rect x="-220" y={top} width="440" height="278" fill="url(#ts-squared)"/>
      <path d={`M0 ${top+6}v266`} stroke="#a8a18d" strokeDasharray="3 4" fill="none"/>
      {body&&<path data-contour="body" d={body.path} fill="none" stroke="#545a50" strokeWidth="2.2" strokeLinecap="round"/>}
      {cloth&&<path data-contour="cloth" d={cloth.path} fill="none" stroke="#bd542c" strokeWidth="2.7" strokeLinecap="round"/>}
      <path d={`M-193 ${top+245}v5h50v-5`} fill="none" stroke="#4d4b40" strokeWidth="1.2"/>
      <text x="-193" y={top+266}>50 mm</text><text x="190" y={top+266} textAnchor="end">FRONT ↓</text>
    </svg><div className="ts-legend"><span>Body form</span><span>Cloth</span></div></div>;
}

export default function TrouserStudy({study}:{study:Study}){
  const [station,setStation]=useState<Station>("Posture"),[id,setId]=useState("standing");
  const [model,setModel]=useState<Model>(),[error,setError]=useState(""),[gpu,setGpu]=useState(true);
  const [view,setView]=useState(0),[strain,setStrain]=useState(false),[clip,setClip]=useState(false);
  const [height,setHeight]=useState(280);
  const [step,setStep]=useState(100),[playing,setPlaying]=useState(false);
  const bench=useRef<HTMLDivElement>(null);
  const [requested,setRequested]=useState("");
  const run=study.cases.find(r=>r.id===id)!;
  const visible=model?.key===id?model:undefined;
  const cut=station==="Cross-section";
  useEffect(()=>{
    const params=new URLSearchParams(location.search);
    const tab=stations.find(s=>s.toLowerCase()===params.get("bench"));
    const row=study.cases.find(r=>r.id===params.get("run"));
    if(tab)setStation(tab);
    if(row?.refusal)setRequested(row.label+" was refused by the solver. Showing the dressed standing run.");
    else if(row && (tab==="Elastic"?row.posture==="standing":!row.id.startsWith("elastic-"))){setId(row.id);if(row.posture!=="standing")setView(1);}
  },[study]);
  useEffect(()=>{
    const {frames,phases}=run;
    if(!run.surface||run.refusal||!frames||!phases)return;
    const controller=new AbortController();setError("");
    const layout={...study,clothVertices:run.clothVertices??study.clothVertices};
    Promise.all([loadSurface(run.surface,true,layout,controller.signal),loadTopology(layout,controller.signal,run.meshKey),
      loadSewing(run.id,layout.clothVertices,frames.length,controller.signal)]).then(([data,mesh,replay])=>{
      if(controller.signal.aborted)return;
      let floor=Infinity;
      for(let i=1;i<data.body.length;i+=3)floor=Math.min(floor,data.body[i]);
      setModel({...data,key:run.id,mesh,floor,replay,frames,phases});
    }).catch(()=>{if(!controller.signal.aborted)setError("This surface could not be loaded.");});
    return()=>controller.abort();
  },[run,study]);
  useEffect(()=>{
    if(!playing)return;
    const start=performance.now();let raf=0;
    function tick(time:number){const pct=Math.max(0,Math.min(100,Math.round((time-start-550)/72)));setStep(pct);
      if(pct<100)raf=requestAnimationFrame(tick);else setPlaying(false);}
    const observer=new IntersectionObserver(([e])=>{if(!e.isIntersecting)setPlaying(false);});observer.observe(bench.current!);
    function hide(){if(document.hidden)setPlaying(false);}
    document.addEventListener("visibilitychange",hide);raf=requestAnimationFrame(tick);
    return()=>{observer.disconnect();cancelAnimationFrame(raf);document.removeEventListener("visibilitychange",hide);};
  },[playing]);
  const y=(visible?.floor??0)+height/1000;
  const contours=useMemo(()=>!cut||!visible?null:{
    body:section({positions:visible.body,indices:visible.mesh.body},y),
    cloth:visible.cloth.length?section({positions:visible.cloth,indices:visible.mesh.cloth},y):null,
  },[cut,visible,y]);
  function choose(tab:Station){
    setStation(tab);setStrain(false);setClip(false);setPlaying(false);setStep(100);
    if((tab==="Elastic"&&run.posture!=="standing")||(tab!=="Elastic"&&(id.startsWith("elastic-")||run.refusal)))setId("standing");
  }
  const choices=study.cases.filter(r=>!!r.surface&&!r.refusal&&(station==="Elastic"?r.posture==="standing":!r.id.startsWith("elastic-")));
  if(station==="Elastic")choices.sort((a,b)=>a.stiffness-b.stiffness);
  return <><SiteHeader/><main id="main" className="ts-page">
    <Crumbs trail={[["Exhibition","/"],["Drape gallery","/sections/studies/"],["Room to move"]]}/>
    <div className="ts-wrap">
      <header className="ts-heading"><div><p className="ts-kicker">The trouser study</p><h1>Room to <em>move.</em></h1></div>
        <p>One pattern, seven cut pieces. Bend the form, change the elastic, or take a section through the folds.</p></header>
      <ConstructionStudy choices={["trousers"]}/>
      <nav id="trouser-fit" className="ts-tabs" aria-label="Trouser experiments">{stations.map(s=><button key={s} aria-pressed={station===s} onClick={()=>choose(s)}>{s}</button>)}</nav>
      <div ref={bench} className="ts-bench">
        <figure className="ts-figure"><div className="ts-stage">
          <div className="ts-mark"><span>Gathered trousers</span><small>{station==="Elastic"?run.stiffness+" N elastic":run.label}</small></div>
          {visible&&gpu?<Scene model={visible} plane={cut?y:null} clip={cut&&clip} strain={strain&&!cut} view={view} step={step} onError={()=>setGpu(false)}/>:
            <div className="ts-placeholder"><img src="/media/trouser-form.jpg" alt="Reference rendering of the standing trouser study"/><span role="status">{error||(!gpu?"Recorded view · WebGL unavailable":"Loading surface…")}</span></div>}
          <div className="ts-cameras" aria-label="Camera">{["Three-quarter","Profile","Back"].map((name,i)=><button key={name} aria-pressed={view===i} onClick={()=>setView(i)}>{name}</button>)}</div>
        </div><figcaption><span>{cut?"Section plane shown at scale":strain?"Native endpoint strain":"Native garment surface"}</span><span>Drag to turn · scroll to zoom · right-drag to move</span></figcaption></figure>
        <aside className="ts-panel">
          <h2>{cut?"Inside the folds":station==="Elastic"?"At the waistband":"Bend the knees"}</h2>
          <p>{cut?"Move a horizontal plane through the form. Each line traces its intersection with a real mesh.":station==="Elastic"?"Change the axial stiffness of the elastic casings. The waist and both cuffs use the same value.":"Compare the sewn trousers on the same 92 cm figure. In the quarter squat, the hips bend 25° and the knees bend 27.5°."}</p>
          {requested&&<p role="status">{requested}</p>}
          <fieldset><legend>{station==="Elastic"?"Elastic stiffness":"Body posture"}</legend><div className="ts-options">{choices.map(r=>
            <button key={r.id} aria-pressed={id===r.id} onClick={()=>{setRequested("");setId(r.id);setPlaying(false);setStep(100);if(station==="Posture"&&r.posture!=="standing")setView(1);}}>{station==="Elastic"?r.stiffness+" N":r.label}</button>)}</div></fieldset>
          {!cut&&<fieldset disabled={!visible||!gpu}><legend>Sew this pair</legend><div className="ts-options">{[[0,"Flat pattern"],[45,"Join seams"],[100,"Sewn trousers"]].map(([pct,label])=>
            <button key={pct} aria-pressed={step===pct} onClick={()=>{setPlaying(false);setStrain(false);setStep(Number(pct));}}>{label}</button>)}
            <button onClick={()=>{setPlaying(!playing);if(!playing){setStrain(false);setStep(0);}}}>{playing?"Pause sewing":"Replay sewing"}</button></div></fieldset>}
          {cut?<><label className="ts-range">Height above sole <strong>{height} mm</strong><input type="range" min="100" max="490" step="1" value={height} onChange={e=>setHeight(Number(e.target.value))}/></label>
            <Drawing body={contours?.body??null} cloth={contours?.cloth??null} height={height}/>
            <label className="ts-check"><input type="checkbox" checked={clip} onChange={e=>setClip(e.target.checked)}/> Cut away above the plane</label>
            {contours&&<p className="ts-note">{contours.cloth?"Cloth spans "+(contours.cloth.right-contours.cloth.left).toFixed(1)+" mm from left to right at this height.":"The plane clears the trouser cloth at this height."}</p>}</>:
          <><dl className="ts-values"><div><dt>Mean seat gap</dt><dd>{run.seatGap?.toFixed(1)} <small>mm</small></dd></div><div><dt>Waistband shift</dt><dd>{run.waistbandShift?.toFixed(1)} <small>mm</small></dd></div></dl>
            <label className="ts-check"><input type="checkbox" checked={strain} onChange={e=>{setStrain(e.target.checked);setPlaying(false);setStep(100);}}/> Show endpoint edge strain</label>
            {strain&&<p className="ts-note">Blue under compression · red under tension. 95th percentile {run.strain?.p95.toFixed(2)}%.</p>}</>}
          {run.flags?.map(flag=><p key={flag} className="ts-note">{flag}</p>)}
          <a className="ts-download" href="/patterns/trousers.pdf" download>Full-size trouser pattern <span>↓</span></a>
        </aside>
      </div>
      <section className="ts-films"><header><p className="ts-kicker">On film</p><h2>Three views of the <em>cut.</em></h2></header>
        {[["trouser-posture","Bending the knee"],["trouser-elastic","Holding the waist"],["trouser-section","Inside the folds"]].map(([film,title])=><figure key={film}><div><InlineFilm id={film} title={title}/></div><figcaption>{title}<a href={filmFile(film)} download aria-label={"Download "+title}>MP4 ↓</a></figcaption></figure>)}
      </section>
      <details className="ts-method"><summary>Pattern, poses and solver readings</summary><p>The seven panels and seventeen seams come from the downloadable pattern. The drapes use 2 mm self-contact thickness and synthetic fabric parameters. The quarter squat uses 20 mm meshing and the other cases use 12 mm. Each starts at its prescribed posture. Chair and floor contact are outside the calculation.</p>
        <p>The full squat and full seat were refused because the posed body crosses itself. The zero-elastic run had a cloth crossing at placement. Diagnostics are retained in the downloadable results.</p>
        <p>The gap is the solver’s mean seat-ring clearance. Negative waistband shift means the band dropped. The slice uses display geometry rounded to 0.1 mm. Its left-to-right span is a geometric dimension across the section. Physical fit requires a sewn sample.</p>
        <a href="/exhibits/trouser-study/study.json" download>Download the run summaries and synthetic inputs ↓</a></details>
    </div>
  </main><SiteFooter/></>;
}
