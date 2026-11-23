"use client";

import dynamic from "next/dynamic";
import { useCallback, useEffect, useState } from "react";
import { experiments, loadExperiment, type Rig, type Experiment } from "./materialExperiments";
import InlineFilm, { PlaybackMark, filmFile } from "./InlineFilm";
import SiteHeader from "./site/SiteHeader";
import SiteFooter from "./site/SiteFooter";
import Crumbs from "./site/Crumbs";
import "./material-lab.css";

const Scene = dynamic(() => import("./MaterialScene"), { ssr: false });
const views = ["Perspective","Fabric detail","Front","Top"] as const;
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
  const [focus,setFocus]=useState(2);
  const [failed,setFailed]=useState(false),[camera,setCamera]=useState<{name:typeof views[number];revision:number}>({name:"Perspective",revision:0});
  const onFailure=useCallback(()=>setFailed(true),[]);
  useEffect(()=>{
    const controller=new AbortController();
    setData(undefined);setProblem(false);setPlaying(false);setFailed(false);
    loadExperiment(kind,controller.signal).then(result=>{
      if(controller.signal.aborted)return;
      setStep(cylinder ? result.readings.findIndex(r=>r.coordinate===2.5) : result.readings.length-1);
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
    <Crumbs trail={[["Exhibition","/"],["Material studies","/sections/materials/"],[meta.title]]}/>
    <div className="wrap rig-wrap">
      <div className="rig-heading"><div><p className="kicker">Material studies / Fig. {meta.figure}</p><h1>{meta.title}<em>.</em></h1></div><p>{meta.subtitle}</p></div>
      <div className="rig-desk">
        <section className="rig-instrument" aria-label="Interactive test rig">
          <div className={`rig-stage ${kind}`}>
            {failed ? <InlineFilm controls id={`rig-${kind}`} title={meta.title}/> :
              data ? <Scene data={data} step={step} mesh={mesh} focus={focus} camera={camera} onFailure={onFailure}/> :
                <img className="rig-poster" src={filmFile("rig-"+kind,"jpg")} alt="Rendered view of the specimen holders"/>}
            {!failed && <><span className="rig-plate mono">{cylinder?"Torsion · 50 mm gauge":"Tensile test · 100 mm gauge"}</span>
              <div className="rig-annot">{cylinder ? r && (camera.name==="Top"?<>Dashed · mounted <b>Solid · current section</b></>:<><b>{fmt(r.gap,1)} mm</b> between grips · {fmt(r.angle,1)}° twist</>) :
                focus===-1?"Warp · Weft · Bias":data&&r&&<><b>{data.specimens[focus].name} cut</b> · {fmt(r.widths[focus],2)} mm at centre</>}</div></>}
          </div>
          {failed ? <p className="rig-viewbar">WebGL is unavailable. The film shows the recorded experiment.</p> :
          <div className="rig-viewbar">{!cylinder&&<div aria-label="Displayed specimen">{["Warp","Weft","Bias","All cuts"].map((name,i)=><button key={name} aria-pressed={focus===(i===3?-1:i)} onClick={()=>setFocus(i===3?-1:i)}>{name}</button>)}</div>}<div aria-label="Camera angle">{views.map(name=><button key={name} aria-pressed={camera.name===name} onClick={()=>setCamera(c=>({name,revision:c.revision+1}))}>{name==="Top"?(cylinder?"Mid-section":"Reverse"):name}</button>)}</div>
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
        "The grips close to 45 mm on a 50 mm tube. Select Mid-section to look into the folds halfway up its height. Both runs use the coupled outfit’s synthetic fabric, with cloth self-contact enabled." :
        "The three strips use the same material as the coupled outfit, cut along the warp, weft or bias. A 16 × 24 mesh gives each strip room to narrow between the fixed-width jaws. Each slider position is a separate planar equilibrium. This test covers tension and in-plane contraction."}</p>
        {r&&data?.comparison&&<p className="rig-refinement">At {fmt(r.coordinate,3)} s, the upper grip reaction is <b>{fmt(r.forces[0],2)} N</b> on the displayed 32 × 8 mesh and <b>{fmt(data.comparison[step],2)} N</b> on a 24 × 8 mesh. The predicted load at peak compression still depends on the mesh. <a href="/materials/cylinder-refinement.json" download>Comparison readings ↓</a></p>}
        {data && r && <details><summary>Solver notes and residuals</summary><p>Maximum free force residual at this state: {r.residual.toExponential(2)} N.</p>
          {data.notes.length>0 && <ul>{data.notes.map(note=><li key={note}>{note}</li>)}</ul>}
          <p>The blue surface uses the Rough Linen material photographed by colormass and processed by Rico Cilliers. Its color and yarn relief are illustrative. <a href="https://polyhaven.com/a/rough_linen">Poly Haven scan ↗</a></p></details>}{!cylinder&&data?.bench&&r&&<section className="rig-measured"><p className="kicker">Measured specimen · SJ1</p><h3>What did the lab measure?</h3>
            <p>At the same {r.coordinate}% extension, Sperl and colleagues recorded these forces on a 100 × 60 mm polyester–spandex jersey. The blue specimen above uses a synthetic material law.</p>
            <div>{data.bench.points[step].map((n,i)=><span key={i}>{data.specimens[i].name}<strong>{fmt(n,2)} N</strong></span>)}</div>
            <a href="/materials/jersey-readings.json" download>Source rows and license ↓</a> · <a href="https://doi.org/10.1145/3528223.3530167">Read the experiment ↗</a></section>}</div>
        <div className="rig-links"><figure className="exhibit-film"><InlineFilm controls id={`rig-${kind}`} title={meta.title+" film"}/><figcaption>{cylinder?"The full loading cycle":"Three cuts under tension"}</figcaption></figure>
          <a href={filmFile("rig-"+kind)} download>Download film ↓</a><a href={`/materials/${kind}.json`} download>Download solver report{cylinder?"":"s"} ↓</a>
          <a href={`/experiments/${cylinder?"strip":"cylinder"}/`}>{experiments[cylinder?"strip":"cylinder"].title} →</a>
          <a href="/sections/studies/?outfit=joint#drape-room">See the same fabric in a jacket and trousers →</a></div></div>
    </div>
  </main><SiteFooter/></>;
}
