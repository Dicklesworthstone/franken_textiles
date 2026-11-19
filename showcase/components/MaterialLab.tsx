"use client";

import dynamic from "next/dynamic";
import { useCallback, useEffect, useState } from "react";
import { experiments, loadExperiment, type Rig, type Experiment } from "./materialExperiments";
import InlineFilm, { PlaybackMark } from "./InlineFilm";
import SiteHeader from "./site/SiteHeader";
import SiteFooter from "./site/SiteFooter";
import Crumbs from "./site/Crumbs";
import "./material-lab.css";

const Scene = dynamic(() => import("./MaterialScene"), { ssr: false });
const views = ["Perspective","Front","Top"] as const;
const inks = ["#2f626a","#9a572b","#66723d"];
const fmt = (value: number, places: number) => (Math.abs(value)<.5*10**-places ? 0 : value).toFixed(places);

function LoadChart({ data, step }: { data: Experiment; step: number }) {
  const all=data.readings.flatMap(row=>row.forces);
  const hi=Math.max(...all), lo=Math.min(0,...all), span=hi-lo || 1;
  const x=(k:number)=>9+222*(data.readings[k].coordinate-data.readings[0].coordinate)/(data.readings.at(-1)!.coordinate-data.readings[0].coordinate);
  const y=(v:number)=>53-46*(v-lo)/span;
  return <figure className="rig-chart"><figcaption>Grip force <span>N</span></figcaption>
    <svg viewBox="0 0 240 62" role="img" aria-label="Grip force across recorded states. Exact selected values appear below.">
      <path d={`M9 ${y(0)} H231`} stroke="#bbb8a8" strokeDasharray="2 3"/>
      <path d={`M${x(step)} 3 V57`} stroke="#8c8978" strokeDasharray="2 3"/>
      {data.specimens.map((s,i)=><g key={s.name} stroke={inks[i]}>
        <polyline points={data.readings.map((r,k)=>`${x(k)},${y(r.forces[i])}`).join(" ")} fill="none" strokeWidth="1.8"/>
        <circle cx={x(step)} cy={y(data.readings[step].forces[i])} r="3" fill={inks[i]}/>
      </g>)}
    </svg><div><span>0</span><span>{data.kind==="cylinder"?"Time (s)":"Extension (%)"}</span><span>{data.readings.at(-1)!.coordinate}</span></div>
  </figure>;
}

export default function MaterialLab({ kind }: {kind: Rig}) {
  const cylinder=kind==="cylinder", meta=experiments[kind];
  const [data,setData]=useState<Experiment>(),[problem,setProblem]=useState(false),[retry,setRetry]=useState(0);
  const [step,setStep]=useState(0),[mesh,setMesh]=useState(false),[playing,setPlaying]=useState(false);
  const [failed,setFailed]=useState(false),[camera,setCamera]=useState<{name:typeof views[number];revision:number}>({name:"Perspective",revision:0});
  const onFailure=useCallback(()=>setFailed(true),[]);
  useEffect(()=>{
    const controller=new AbortController();
    setData(undefined);setProblem(false);setPlaying(false);setFailed(false);
    loadExperiment(kind,controller.signal).then(result=>{
      if(controller.signal.aborted)return;
      setStep(cylinder ? result.readings.findIndex(r=>r.coordinate===1.5) : result.readings.length-1);
      setData(result);
    }).catch(()=>{if(!controller.signal.aborted)setProblem(true);});
    return ()=>controller.abort();
  },[kind,cylinder,retry]);
  useEffect(()=>{
    if(!playing||!data)return;
    if(step>=data.readings.length-1){setPlaying(false);return;}
    const id=setTimeout(()=>setStep(step+1),cylinder?165:460);
    return ()=>clearTimeout(id);
  },[playing,step,data,cylinder]);
  const r=data?.readings[step];
  function seek(i:number){setPlaying(false);setStep(i);}
  const stages=cylinder ? [[0,"Mounted"],[1.5,"Twisted"],[2.5,"Compressed"],[3.5,"Final pull"]] as const :
    [[0,"Relaxed"],[4,"4% pull"],[8,"8% pull"]] as const;
  return <><SiteHeader/><main id="main" className="rig-page">
    <Crumbs trail={[["Exhibition","/"],["Material studies","/#materials"],[meta.title]]}/>
    <div className="wrap rig-wrap">
      <div className="rig-heading"><div><p className="kicker">Material studies / Fig. {meta.figure}</p><h1>{meta.title}<em>.</em></h1></div><p>{meta.subtitle}</p></div>
      <div className="rig-desk">
        <section className="rig-instrument" aria-label="Interactive test rig">
          <div className={`rig-stage ${kind}`}>
            {failed ? <InlineFilm controls id={`rig-${kind}`} title={meta.title}/> :
              data ? <Scene data={data} step={step} mesh={mesh} camera={camera} onFailure={onFailure}/> :
                <img className="rig-poster" src={`/media/rig-${kind}.jpg`} alt="Rendered view of the specimen holders"/>}
            {!failed && <><span className="rig-plate mono">{cylinder?"Cylindrical holder":"Tensile grips"}</span>
              <div className="rig-annot">{cylinder ? r && <><b>{fmt(r.angle,1)}°</b> holder rotation</> :
                <><i/> Warp <i/> Weft <i/> Bias</>}</div></>}
          </div>
          {failed ? <p className="rig-viewbar">WebGL is unavailable. The film shows the recorded experiment.</p> :
          <div className="rig-viewbar"><div aria-label="Camera angle">{views.map(name=><button key={name} onClick={()=>setCamera(c=>({name,revision:c.revision+1}))}>{name}</button>)}</div>
            <label><input type="checkbox" checked={mesh} onChange={e=>setMesh(e.target.checked)}/> Solver mesh</label></div>}
        </section>
        <section className="rig-console" aria-label="Experiment controls">
          <h2>{cylinder?"Load cycle":"Grain comparison"}</h2>
          {problem ? <p role="alert">The specimen report could not load. <button onClick={()=>setRetry(n=>n+1)}>Retry</button></p> :
           !data || !r ? <p role="status">Loading the recorded experiment…</p> : <>
            <label className="rig-slider">{cylinder?"Recorded time":"Grip extension"} <output>{fmt(r.coordinate,cylinder?3:0)} {cylinder?"s":"%"}</output>
              <input aria-label={cylinder?"Recorded time":"Grip extension"} type="range" min="0" max={data.readings.length-1} value={step}
                aria-valuetext={r.coordinate+(cylinder?" seconds":" percent")} onChange={e=>seek(Number(e.target.value))}/></label>
            <div className="rig-presets">{stages.map(([target,name])=><button key={name} aria-pressed={r.coordinate===target}
              onClick={()=>seek(data.readings.findIndex(row=>row.coordinate===target))}>{name}</button>)}</div>
            <LoadChart data={data} step={step}/>
            {cylinder ? <dl className="rig-values"><div><dt>Upper grip force</dt><dd>{fmt(r.forces[0],2)} N</dd></div>
              <div><dt>Upper grip torque</dt><dd>{fmt(r.torque*1000,1)} mN·m</dd></div><div><dt>Grip spacing</dt><dd>{fmt(r.gap,2)} mm</dd></div></dl> :
              <table className="rig-values"><thead><tr><th>Cut</th><th>Force</th><th>Mid-width</th></tr></thead>
                <tbody>{data.specimens.map((s,i)=><tr key={s.name}><th><i style={{background:inks[i]}}/>{s.name}</th>
                  <td>{fmt(r.forces[i],3)} N</td><td>{fmt(r.widths[i],2)} mm</td></tr>)}</tbody></table>}
            <button className="rig-play" onClick={()=>{if(step===data.readings.length-1)setStep(0);setPlaying(!playing);}}><PlaybackMark playing={playing}/> {playing?"Pause":"Play the cycle"}</button>
            {cylinder && <p className="rig-threshold">{r.slip ? `Grip-slip threshold exceeded (${data.limit} N).` : `Within the ${data.limit} N grip-slip threshold.`}</p>}
          </>}
        </section>
      </div>
      <div className="rig-notes"><div><h2>About the specimen</h2><p>{cylinder ?
        "The Rust cloth solver advances a synthetic tube between two rigid holders. Rotation, compression and prescribed slip follow the input schedule. The force is the upper holder’s reaction before tare." :
        "All three strips use the same synthetic material and ideal grips. Only the material direction changes. Each point is an independent planar equilibrium solved by Rust."}</p>
        {data && r && <details><summary>Solver notes and residuals</summary><p>Maximum free force residual at this state: {r.residual.toExponential(2)} N.</p>
          {data.notes.length>0 && <ul>{data.notes.map(note=><li key={note}>{note}</li>)}</ul>}
          <p>The reports include the exact model inputs, evidence and assumptions.</p></details>}</div>
        <div className="rig-links"><figure className="exhibit-film"><InlineFilm controls id={`rig-${kind}`} title={meta.title+" film"}/><figcaption>{cylinder?"The full loading cycle":"Three cuts under tension"}</figcaption></figure>
          <a href={`/media/rig-${kind}.mp4`} download>Download film ↓</a><a href={`/materials/${kind}.json`} download>Download solver report{cylinder?"":"s"} ↓</a>
          <a href={`/experiments/${cylinder?"strip":"cylinder"}/`}>{experiments[cylinder?"strip":"cylinder"].title} →</a></div></div>
    </div>
  </main><SiteFooter/></>;
}
