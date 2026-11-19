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
const surfaces:Surface[]=["Check","Linen","Print","Panels","Strain","Hems"];
type Section={body:number[][];cloth:number[][];heightMm:number;edgeAboveWaistMm:number;frontGapMm:number;backGapMm:number};
function WaistSection({section}:{section:Section}){
  const lines=[...section.body,...section.cloth];
  const xs=lines.flatMap(([x,,u])=>[x,u]),ys=lines.flatMap(([,y,,v])=>[y,v]);
  const x=Math.min(...xs)-8,y=Math.min(...ys)-8,w=Math.max(...xs)-x+8,h=Math.max(...ys)-y+8;
  const path=(segments:number[][])=>segments.map(([a,b,c,d])=>`M${a} ${b}L${c} ${d}`).join("");
  const offset=section.edgeAboveWaistMm;
  return <div className="sa-section"><div><h2>At the waist</h2>
    <p>A horizontal slice through the worn skirt and torso, {(section.heightMm/10).toFixed(1)} cm above the sole. The median height of the bound edge is {offset===0?"level with":`${Math.abs(offset)} mm ${offset<0?"below":"above"}`} the form’s natural waist.</p>
    <div className="sa-key"><span>Body</span><span>Cloth</span></div>
    <p className="sa-gaps">Centre-front gap {section.frontGapMm} mm · Centre-back gap {section.backGapMm} mm</p></div>
    <svg viewBox={`${x} ${y} ${w} ${h}`} role="img" aria-label="Plan view of the fitted torso and the skirt cut at the same height">
      <path d={path(section.body)} fill="none" stroke="#735042" strokeWidth="1.8"/><path d={path(section.cloth)} fill="none" stroke="#a72942" strokeWidth="1.6"/>
    </svg></div>;
}
function cutName(cut:Cut){return cut==="flare"?"Flared":"Column";}
function SkirtIcon({cut}:{cut:Cut}){
  const outline=cut==="flare"?"M9 3Q8 11 3 21Q13 25 23 21Q18 11 17 3Z":"M9 3Q6 12 6 21Q13 24 20 21Q20 12 17 3Z";
  return <svg viewBox="0 0 26 27" width="24" height="25" fill="none" stroke="currentColor" aria-hidden="true"><path d={outline}/><path d="M10 5h6m-5 1-2 15m6-15 2 15" strokeWidth=".7"/></svg>;
}
export default function Atelier({home=false}:{home?:boolean}){
  const [cut,setCut]=useState<Cut>("flare"),[waist,setWaist]=useState(680),[surface,setSurface]=useState<Surface>("Check");
  const [step,setStep]=useState(100),[playing,setPlaying]=useState(false);
  const [dressForm,setDressForm]=useState(false),[detail,setDetail]=useState(false);
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
  const ease=(run.waistCutMm-run.tape.waist.body_mm)/10;
  return <section id="skirt-atelier" ref={hero} className="sa" aria-labelledby="skirt-title">
    <div className="sa-room">
      <div className="sa-inside">
        <div className="sa-intro">
          <p className="sa-label"><i/> The skirt atelier</p>
          <h1 id="skirt-title">Cut on<br/><em>the curve.</em></h1>
          <p className="sa-deck">Six curved panels follow the rise from waist to hip. Choose a size, then change the sweep below it.</p>
          <fieldset className="sa-cut"><legend>Silhouette</legend><div>{(["column","flare"] as Cut[]).map(c=><button key={c} aria-pressed={cut===c} disabled={!runs[c+"-"+waist]} onClick={()=>select(c,waist)}><SkirtIcon cut={c}/>{cutName(c)}</button>)}</div></fieldset>
          <fieldset className="sa-waists"><legend>Form waist <span>/ cm</span></legend><div>{waists.filter(w=>runs[cut+"-"+w]).map(w=><button key={w} aria-label={w/10+" centimetre form waist"} aria-pressed={waist===w} onClick={()=>select(cut,w)}>{w/10}</button>)}</div></fieldset>
          <p className="sa-fit">{(run.tape.hip.body_mm/10).toFixed(1)} cm fitted hip · {ease>=0?"+":""}{ease.toFixed(1)} cm flat waist ease</p>
        </div>
        <figure className="sa-display" aria-label="Fitted dress form">
          {!shown&&<img className="sa-poster" alt={cutName(cut)+" skirt on the "+waist/10+" cm form"} src={"/atelier/"+(key==="flare-680"?"form":key)+".webp"} fetchPriority="high"/>}
          {failed!==key&&<div className={"sa-scene"+(shown?" visible":"")}><Scene cut={cut} waist={waist} patternStem={run.patternStem} alternateHem={runs[altCut+"-"+waist]?.hemOutline} dressForm={dressForm} detail={detail} surface={surface} step={step} ready={ready} fail={fail}/></div>}
          <div className="sa-view-controls"><button disabled={!shown} aria-pressed={detail} onClick={()=>setDetail(!detail)}>{detail?"Whole skirt":"Waist detail"}</button>
          <button disabled={!shown} aria-pressed={dressForm} onClick={()=>{setLoaded("");setDressForm(!dressForm);}}>{dressForm?"Human body":"Dress form"}</button></div>
          <figcaption>{failed===key?<a href="/media/skirt-atelier.mp4">Watch the 68 cm study film ↗</a>:shown?"Drag to turn · arrow keys to rotate":"Loading the 3D view"}</figcaption>
        </figure>
      </div>
    </div>
    <div className="sa-desk"><div className="sa-desk-inner">
      <div className="sa-reading"><span>Flat hem</span><strong>{hem}<small> cm</small></strong></div>
      <div className="sa-reading"><span>Hem height spread</span><strong>{spread.toFixed(1)}<small> mm</small></strong></div>
      <div className="sa-surface"><span>Inspect the cloth</span><div>{surfaces.map(s=><button key={s} aria-pressed={s===surface} disabled={!shown||(s==="Hems"&&!runs[altCut+"-"+waist])} onClick={()=>{setSurface(s);setPlaying(false);if(s==="Strain"||s==="Hems")setStep(100);}}>{s==="Hems"?"Compare hems":s}</button>)}</div></div>
      <button className="sa-replay" disabled={!shown} onClick={()=>{if(playing){setPlaying(false);return;}if(surface==="Strain"||surface==="Hems")setSurface("Check");setStep(0);setPlaying(true);}}><span className={"playback-mark"+(playing?" is-playing":"")}/>{playing?"Pause replay":"Replay drape"}</button>
      <a className="sa-pdf" href={"/atelier/"+run.patternStem+".pdf"} download>Print this cut <span aria-hidden="true">↓</span><small>{waist/10} cm waist · A4 PDF</small></a>
    </div></div>
    <WaistSection section={run.section}/>
    <div className="sa-method"><div>
      <details><summary>Measurements &amp; solver notes</summary>
        <p>The six panels have a combined {(run.waistCutMm/10).toFixed(1)} cm waist and a 59 cm centre length. The {cutName(cut).toLowerCase()} hem measures {hem} cm on the sewing line. The native Anny waist is {(run.tape.waist.body_mm/10).toFixed(1)} cm against a {waist/10} cm target.</p>
        <p>Each size has its own flat cut, fitted Anny body and native run with {run.sew} sewing steps and {run.drape} gravity steps. Body contact and cloth self-contact are active. All cuts use the solver’s synthetic sheet. The check, linen and print controls change its appearance. The optional tailor’s bust comes from the same body surface.</p>
        <p>The hem height spread is the difference between the highest and lowest of the six native panel hem readings at the endpoint. The 95th percentile of edge strain is {run.strain.p95}%. The strain view uses the endpoint colors. Red indicates extension, blue compression and white little change.</p>
        <label className="sa-scrub">Sewing &amp; gravity replay <input type="range" min="0" max="100" value={step} disabled={!shown} onChange={e=>{setPlaying(false);if(surface==="Strain"||surface==="Hems")setSurface("Check");setStep(+e.target.value);}}/></label>
        {run.flags.map(f=><p key={f}>{f}</p>)}
        <p>The cut selector shows settled drapes that passed the native checks. Refusals and provisional endpoints are recorded with their solver messages in the run data.</p>
        <p>The PDF includes all six pieces with 10 mm seam allowances and print calibration squares. Print at 100% and check the scale before cutting. A sewn sample is needed to establish fit.</p>
        <a href={"/atelier/"+run.patternStem+".json"} download>Pattern source ↓</a> · <a href="/atelier/study.json" download>Runs, residuals &amp; refused cases ↓</a> · <a href={"/studies/skirt/?cut="+cut+"&waist="+waist}>Link to this cut and form ↗</a>
      </details>
      <span className="sa-status" role="status">{surface==="Strain"?"Endpoint edge strain · P95 "+run.strain.p95+"%":surface==="Hems"?"Dashed line = "+cutName(altCut)+" hem":playing?"Native sewing and gravity replay":qualified?"Settled endpoint":"Provisional drape endpoint"}</span>
      {home&&<a className="sa-film-link" href="/studies/skirt/#skirt-film">Watch the studio film ↗</a>}
    </div></div>
    {!home&&<div className="sa-film" id="skirt-film"><div><p className="sa-label">Studio film</p><h2>Two hems<br/><em>in motion.</em></h2><p>Follow the same 68 cm form from the column cut to the flare. The stitched panels change shape as each skirt takes its own weight.</p><a href={"/atelier/"+altCut+".pdf"} download>Print the 68 cm {cutName(altCut).toLowerCase()} cut ↓</a></div><figure><InlineFilm id="skirt-atelier" title="Two six-gore skirts on one fitted form" controls/></figure></div>}
  </section>;
}
