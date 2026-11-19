"use client";
import dynamic from "next/dynamic";
import {useCallback,useEffect,useRef,useState} from "react";
import study from "../../public/atelier/study.json";
import InlineFilm from "../InlineFilm";
import type {Surface} from "./Scene";
import "./atelier.css";

const Scene=dynamic(()=>import("./Scene"),{ssr:false});
type Cut="column"|"flare";
const runs:Record<string,(typeof study.runs)[keyof typeof study.runs]>=study.runs;
const waists=Array.from(new Set(Object.values(runs).map(r=>r.waist))).sort((a,b)=>a-b);
const surfaces:Surface[]=["Print","Panels","Strain","Hems"];
function cutName(cut:Cut){return cut==="flare"?"Flared":"Column";}
function SkirtIcon({cut}:{cut:Cut}){
  const outline=cut==="flare"?"M9 3Q8 11 3 21Q13 25 23 21Q18 11 17 3Z":"M9 3Q6 12 6 21Q13 24 20 21Q20 12 17 3Z";
  return <svg viewBox="0 0 26 27" width="24" height="25" fill="none" stroke="currentColor" aria-hidden="true"><path d={outline}/><path d="M10 5h6m-5 1-2 15m6-15 2 15" strokeWidth=".7"/></svg>;
}
export default function Atelier({home=false}:{home?:boolean}){
  const [cut,setCut]=useState<Cut>("flare"),[waist,setWaist]=useState(680),[surface,setSurface]=useState<Surface>("Print");
  const [step,setStep]=useState(100),[playing,setPlaying]=useState(false);
  const [dressForm,setDressForm]=useState(false);
  const [loaded,setLoaded]=useState(""),[failed,setFailed]=useState("");
  const hero=useRef<HTMLElement>(null),key=cut+"-"+waist,run=runs[key],shown=loaded===key&&failed!==key;
  const ready=useCallback((id:string)=>setLoaded(id),[]),fail=useCallback((id:string)=>{setFailed(id);setPlaying(false);},[]);
  useEffect(()=>{
    const q=new URLSearchParams(location.search),c=q.get("cut"),w=Number(q.get("waist"));
    if((c==="column"||c==="flare")&&runs[c+"-"+w]){setCut(c);setWaist(w);}
  },[]);
  useEffect(()=>{
    if(!playing)return;
    const start=performance.now();let frame=0;
    function tick(time:number){const pct=Math.min(100,Math.round((time-start)/38));setStep(pct);
      if(pct<100)frame=requestAnimationFrame(tick);else setPlaying(false);}
    const observer=new IntersectionObserver(([entry])=>{if(!entry.isIntersecting)setPlaying(false);});observer.observe(hero.current!);
    const hide=()=>{if(document.hidden)setPlaying(false);};document.addEventListener("visibilitychange",hide);
    frame=requestAnimationFrame(tick);return ()=>{cancelAnimationFrame(frame);observer.disconnect();document.removeEventListener("visibilitychange",hide);};
  },[playing]);
  function select(c:Cut,w:number){
    if(c===cut&&w===waist)return;
    setPlaying(false);setStep(100);setFailed("");setLoaded("");setCut(c);setWaist(w);
    if(surface==="Hems"&&!runs[(c==="flare"?"column":"flare")+"-"+w])setSurface("Print");
  }
  const altCut:Cut=cut==="flare"?"column":"flare";
  const qualified=run.flags.length===0,hem=study.cuts[cut]/10;
  const heights=Object.values(run.hemHeights),spread=Math.max(...heights)-Math.min(...heights);
  const ease=(780-run.tape.waist.body_mm)/10;
  return <section id="skirt-atelier" ref={hero} className="sa" aria-labelledby="skirt-title">
    <div className="sa-room">
      <div className="sa-inside">
        <div className="sa-intro">
          <p className="sa-label"><i/> The skirt atelier</p>
          <h1 id="skirt-title">Cut on<br/><em>the curve.</em></h1>
          <p className="sa-deck">Six panels, one waist, two very different hems. Change the cut and the figure to see where the cloth falls.</p>
          <fieldset className="sa-cut"><legend>Silhouette</legend><div>{(["column","flare"] as Cut[]).map(c=><button key={c} aria-pressed={cut===c} disabled={!runs[c+"-"+waist]} onClick={()=>select(c,waist)}><SkirtIcon cut={c}/>{cutName(c)}</button>)}</div></fieldset>
          <fieldset className="sa-waists"><legend>Form waist <span>/ cm</span></legend><div>{waists.filter(w=>runs[cut+"-"+w]).map(w=><button key={w} aria-label={w/10+" centimetre form waist"} aria-pressed={waist===w} onClick={()=>select(cut,w)}>{w/10}</button>)}</div></fieldset>
          <p className="sa-fit">{(run.tape.hip.body_mm/10).toFixed(1)} cm fitted hip · {ease>=0?"+":""}{ease.toFixed(1)} cm flat waist ease</p>
        </div>
        <figure className="sa-display" aria-label="Fitted dress form">
          {!shown&&<img className="sa-poster" alt="Reference view of the flared skirt on the 68 cm form" src="/atelier/form.webp" fetchPriority="high"/>}
          {failed!==key&&<div className={"sa-scene"+(shown?" visible":"")}><Scene cut={cut} waist={waist} alternateHem={runs[altCut+"-"+waist]?.hemOutline} dressForm={dressForm} surface={surface} step={step} ready={ready} fail={fail}/></div>}
          <button className="sa-form-toggle" disabled={!shown} aria-pressed={dressForm} onClick={()=>{setLoaded("");setDressForm(!dressForm);}}>{dressForm?"See on the body":"See on a dress form"}</button>
          <figcaption>{failed===key?<a href="/media/skirt-atelier.mp4">68 cm flared still · watch film ↗</a>:shown?"Drag to turn · arrow keys to rotate":"Reference view · loading the form"}</figcaption>
          {shown&&<span className="sa-measure"><i/><small>{surface==="Hems"?"Dashed hem":"Sewing line"}</small><strong>{surface==="Hems"?(study.cuts[altCut]/10):hem} cm</strong><em>{surface==="Hems"?cutName(altCut):cutName(cut)} sweep</em></span>}
        </figure>
      </div>
    </div>
    <div className="sa-desk"><div className="sa-desk-inner">
      <div className="sa-reading"><span>Flat hem</span><strong>{hem}<small> cm</small></strong></div>
      <div className="sa-reading"><span>Hem height spread</span><strong>{spread.toFixed(1)}<small> mm</small></strong></div>
      <div className="sa-surface"><span>Inspect the cloth</span><div>{surfaces.map(s=><button key={s} aria-pressed={s===surface} disabled={!shown||(s==="Hems"&&!runs[altCut+"-"+waist])} onClick={()=>{setSurface(s);setPlaying(false);if(s==="Strain"||s==="Hems")setStep(100);}}>{s==="Hems"?"Compare hems":s}</button>)}</div></div>
      <button className="sa-replay" disabled={!shown} onClick={()=>{if(playing){setPlaying(false);return;}setSurface(surface==="Panels"?"Panels":"Print");setStep(0);setPlaying(true);}}><span className={"playback-mark"+(playing?" is-playing":"")}/>{playing?"Pause replay":"Replay drape"}</button>
      <a className="sa-pdf" href={"/atelier/"+cut+".pdf"} download>Print this cut <span aria-hidden="true">↓</span><small>1:1 · A4 PDF</small></a>
    </div></div>
    <div className="sa-method"><div>
      <details><summary>Measurements &amp; solver notes</summary>
        <p>The six panels have a combined 78 cm waist and a 59 cm centre length. The {cutName(cut).toLowerCase()} hem measures {hem} cm on the sewing line. The native Anny waist is {(run.tape.waist.body_mm/10).toFixed(1)} cm against a {waist/10} cm target.</p>
        <p>Each selection loads its own native run with {run.sew} sewing steps and {run.drape} gravity steps, including cloth–body and cloth–cloth contact. The garment detail shows the actual fitted pelvis and legs. The optional linen bust is cut from the same adult mesh. Skin color and fabric print are illustrative finishes.</p>
        <p>The hem height spread is the difference between the highest and lowest of the six native panel hem readings at the endpoint. The 95th percentile of edge strain is {run.strain.p95}%. The strain view uses the endpoint colors. Red indicates extension, blue compression and white little change.</p>
        <label className="sa-scrub">Sewing &amp; gravity replay <input type="range" min="0" max="100" value={step} disabled={!shown} onChange={e=>{setPlaying(false);setSurface(surface==="Panels"?surface:"Print");setStep(+e.target.value);}}/></label>
        {run.flags.map(f=><p key={f}>{f}</p>)}
        <p>Both cuts at 74 cm and the flared cut at 80 cm stopped before completing the solve. The available form settings have recorded cloth trajectories.</p>
        <p>The PDF includes all six pieces with 10 mm seam allowances and print calibration squares. Print at 100% and check the scale before cutting. A sewn sample is needed to establish fit.</p>
        <a href={"/atelier/"+cut+".json"} download>Pattern source ↓</a> · <a href="/atelier/study.json" download>Runs, residuals &amp; refused cases ↓</a> · <a href={"/studies/skirt/?cut="+cut+"&waist="+waist}>Link to this cut and form ↗</a>
      </details>
      <span className="sa-status" role="status">{surface==="Strain"?"Endpoint edge strain · P95 "+run.strain.p95+"%":surface==="Hems"?"Dashed line = "+cutName(altCut)+" hem":!runs[altCut+"-"+waist]?"Flared solve refused for this form":playing?"Native sewing and gravity replay":qualified?"Settled endpoint":"Provisional drape endpoint"}</span>
      {home&&<a className="sa-film-link" href="/studies/skirt/#skirt-film">Watch the studio film ↗</a>}
    </div></div>
    {!home&&<div className="sa-film" id="skirt-film"><div><p className="sa-label">Studio film</p><h2>Two hems<br/><em>in motion.</em></h2><p>Follow the same 68 cm form from the column cut to the flare. The stitched panels change shape as each skirt takes its own weight.</p><a href={"/atelier/"+altCut+".pdf"} download>Print the {cutName(altCut).toLowerCase()} cut ↓</a></div><figure><InlineFilm id="skirt-atelier" title="Two six-gore skirts on one fitted form" controls/></figure></div>}
  </section>;
}
