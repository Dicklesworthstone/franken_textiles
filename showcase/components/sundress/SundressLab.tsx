"use client";
import dynamic from "next/dynamic";
import { useCallback, useEffect, useState } from "react";
import { axes, dress, initial, samples, round, type Arm, type Readings } from "./form";
import type { View } from "./SundressScene";
import SundressSewn, {cuts, frontSkirt, type DressCut} from "./SundressSewn";
import ConstructionStudy from "../construction/ConstructionStudy";
import InlineFilm from "../InlineFilm";
import SiteHeader from "../site/SiteHeader";
import SiteFooter from "../site/SiteFooter";
import Crumbs from "../site/Crumbs";
import "./sundress.css";

const Scene=dynamic(()=>import("./SundressScene"),{ssr:false});
function signed(n:number) {const v=round(n);return (v>0?"+":"")+v.toFixed(1);}

function EaseSection({name,girth}:{name:"bust"|"waist";girth:number}){
  const [gx,gz]=axes(dress[name]),[bx,bz]=axes(girth);
  const gap=dress[name]-girth;
  return <figure><svg viewBox="-22 -15 44 30" role="img" aria-label={name+" section, garment "+dress[name]+" centimetres, form "+girth+" centimetres."}>
    <ellipse rx={bx} ry={bz} fill={gap<0?"#c46847":"#c19169"} fillOpacity=".62" stroke={gap<0?"#923f27":"#9b6945"} strokeWidth=".35"/>
    <ellipse rx={gx} ry={gz} fill="none" stroke="#17675c" strokeWidth=".6" strokeDasharray="1.4 .65"/>
  </svg><figcaption className={gap<0?"sd-short":""}>{name==="bust"?"Bust":"Waist"} · {Math.abs(gap).toFixed(1)} cm {gap<0?"short":"ease"}</figcaption></figure>;
}

export default function SundressLab(){
  const [cut,setCut]=useState<DressCut>("dress");
  const chosen = cuts[cut];
  useEffect(()=>{if(new URLSearchParams(location.search).get("garment")==="eased")setCut("light");},[]);
  const [form,setForm]=useState(initial),[view,setView]=useState<View>("Full length"),[failed,setFailed]=useState(false);
  const [body,setBody]=useState<Readings|null>(null);
  const arm=useCallback((side:Arm,angle:number)=>setForm(p=>({...p,[side]:angle})),[]);
  const fail=useCallback(()=>setFailed(true),[]);
  const measurements=[["Stature","height",154,184,"cm"],["Body mass","weight",45,74,"kg"],["Bust girth","bust",78,108,"cm"],["Waist girth","waist",60,94,"cm"]] as const;
  return <><SiteHeader/><main id="main" className="sd-page">
    <Crumbs trail={[["Exhibition","/"],["Wardrobe","/sections/wardrobe/"],["The garden sundress"]]}/>
    <div className="wrap sd-wrap"><div className="sd-head"><div><p className="kicker">The fitting room</p><h1>The garden sundress<em>.</em></h1></div>
      <div className="sd-intro"><p>Follow a four-piece dress from the cutting table to a fitted Anny form. Compare two skirt cuts, then change the wearer’s proportions to explore the bodice ease.</p>
        <a href={chosen.pdf} download>Full-size sewing pattern PDF ↓</a><small>A4 · {chosen.pages} pages · print at 100%</small><a href="#muslin">Compare a sewn sample</a> · <a href="/wardrobe/sundress/compare/">Compare two native drapes →</a></div></div>
      <ConstructionStudy choices={["sundress","eased"]} value={cut==="light"?"eased":"sundress"} onSelect={garment=>setCut(garment==="eased"?"light":"dress")}/>
      <SundressSewn cut={cut} setCut={setCut}/>
      <div className="sd-desk"><section className="sd-board" aria-label="Interactive sundress">
        <div className="sd-stage">{failed?<InlineFilm id="wardrobe-sundress" title="Recorded geometric sundress study" controls/>:
          <Scene form={form} view={view} onArm={arm} onError={fail} onReadings={setBody}/>}
          {!failed && <div className="sd-stage-caption"><i/> Fitted Anny body <span>·</span> geometric garment</div>}</div>
        <div className="sd-viewbar"><div>{(["Full length","Bodice","Side seam"] as const).map(v=><button key={v} aria-pressed={view===v} onClick={()=>{
          setView(v);if(v==="Side seam")setForm(f=>({...f,right:Math.max(75,f.right)}));
        }} disabled={failed}>{v}</button>)}</div>
          <p>{failed?"WebGL unavailable · recorded view":"Drag a hand to pose · drag the ground to turn · scroll to zoom · right-drag to move"}</p></div>
      </section><section className="sd-controls" aria-label="Wearer and pose">
        <div className="sd-section"><h2>Wearer</h2><button className="sd-reset" onClick={()=>{setForm(initial);setView("Full length");}}>Reset</button></div>
        <div className="sd-poses" aria-label="Reference form comparisons">{samples.map(({name,bust,waist})=>
          <button key={name} disabled={failed} aria-pressed={form.bust===bust&&form.waist===waist} onClick={()=>setForm(p=>({...p,bust,waist}))}>{name}</button>)}</div>
        {measurements.map(([label,key,min,max,unit])=><label key={key}>{label} target<output>{form[key]} {unit}</output>
          <input type="range" min={min} max={max} step={key==="height"||key==="weight"?1:.5} value={form[key]} disabled={failed} aria-label={label+" target"} aria-valuetext={form[key]+" "+unit}
            onChange={e=>{const value=Number(e.currentTarget.value);setForm(p=>({...p,[key]:value}));}}/></label>)}
        {/* The model's readouts hold their place until it reports, so the panel does not grow on load. The
            placeholders are as long as the text that replaces them, so narrow columns wrap the same way. */}
        {!failed&&<p className="sd-actual" data-model-mass={body?.weight.toFixed(1)}>Model stature {body?body.height.toFixed(1):"–––.–"} cm · mass {body?body.weight.toFixed(1):"––.–"} kg</p>}
        <div className="sd-lines"><div className="sd-section"><h2>Pattern ease</h2><span>cm</span></div>
          <div className="sd-table"><span>Section</span><span>Dress</span><span>Model</span><span>Ease</span>
          {(["bust","waist"] as const).map(k=><div key={k}><strong>{k}</strong><span>{dress[k]}</span><span data-fitted={k}>{body?body[k].toFixed(1):"—"}</span><b className={body&&dress[k]<body[k]?"sd-negative":""}>{body?signed(dress[k]-body[k]):"—"}</b></div>)}</div>
          {!failed&&<><p>{!body?"Reading the bust and waist from the fitted model.":dress.bust<body.bust||dress.waist<body.waist?"The model girth exceeds the available cloth at the negative sections.":
            dress.bust-body.bust<dress.bustBand[0]||dress.waist-body.waist<dress.waistBand[0]?"Room falls below the flat pattern’s minimum ease target.":
            dress.bust-body.bust>dress.bustBand[2]||dress.waist-body.waist>dress.waistBand[2]?"Room exceeds this pattern’s ease band.":"Both girths are within this pattern’s ease band."}</p>
          <details><summary>Compare cross-sections</summary>{body&&<><div className="sd-cross"><EaseSection name="bust" girth={body.bust}/><EaseSection name="waist" girth={body.waist}/></div>
            <p>Solid ellipses represent the model tapes. Dashed lines represent the fixed garment girths at the same scale.</p></>}</details>
          <p><a href="/wardrobe/sundress/redraft/">Redraft bodice &amp; waist for this ease →</a></p></>}
        </div>
        <div className="sd-section"><h2>Arm pose</h2></div><div className="sd-poses">{([["At rest",14,19],["Reach",83,29],["Raised",115,106]] as const).map(([name,left,right])=>
          <button key={name} disabled={failed} aria-pressed={form.left===left&&form.right===right} onClick={()=>setForm(p=>({...p,left,right}))}>{name}</button>)}</div>
        <details><summary>Adjust each arm</summary>{(["left","right"] as const).map(side=><label key={side}>{side} arm<output>{form[side]}°</output><input type="range" min="4" max="125" value={form[side]} disabled={failed} aria-label={side+" arm angle"} onChange={e=>arm(side,Number(e.target.value))}/></label>)}</details>
      </section></div>
      <div className="sd-notes"><div><h2>One dress, different proportions</h2>
        <p>The flat pattern measures {dress.shoulderToWaist} cm from shoulder to waist and {dress.skirtLength} cm from waist to hem. {!failed&&<>In this preview, the nominal hem sits <strong>{body?body.hem.toFixed(1):"—"} cm</strong> above the floor.</>}</p>
        <p>The synthetic adult comes from the native Anny body model. The controls interpolate fitted shapes and five arm poses. Model readings interpolate the native tape and mass results. Ease compares those readings with the fixed flat pattern.</p>
        <p>The adjustable figure shows the original gathered cut and the flat-pattern ease of its bodice. Its bodice opens where cloth runs short and its skirt folds are illustrative. <a href="#sewn">Compare the two native cloth solves</a> on the reference form to inspect the sewn shapes. Physical fit requires a sewn sample.</p>
        <details><summary>Cut and sew the dress</summary><p>{chosen.name}. Print the cover at actual size and check the 10 cm and 4 inch squares in both directions. Assemble the {chosen.pages-1} tiles by their matching diamonds. Cut one front and one back of both bodice and skirt on the marked grain. The solid cut lines include a 10 mm seam allowance.</p>
          <p>Join the shoulder seams, bind the neckline and armholes, and sew the side seams with an opening for your chosen closure. {cut==="light"?"Ease":"Gather"} each {frontSkirt(chosen.pattern).waist} mm skirt waist to its matching 390 mm bodice waist. Join the waist, fit the closure and finish the hem.</p></details>
        <details><summary>Native solver evidence</summary><p>All eight seam checks passed for the reference pattern. In the gathered version the skirt drops as its pleats close. The solver tracks the waist corners through sewing and gravity. Their motion, the remaining fit advisories and the waist cross-section are shown beside the replay.</p>
          <a href="/fitted-bodies/sundress.json" download>Download native body targets and outcomes ↓</a><br/><a href={`/exhibits/sundress/${cut}-summary.json`} download>Download the settled drape’s summary ↓</a><br/><a href="/sundress/drape-result.json" download>Download the earlier gathered attempt ↓</a></details></div>
        <div className="sd-links"><figure className="exhibit-film"><InlineFilm id="wardrobe-sundress" title="Garden sundress fitting-room film" controls/><figcaption>The sundress in motion</figcaption></figure>
          <a href={chosen.svg} download>Download full-scale pattern SVG ↓</a>
          <a href={`/exhibits/sundress/${cut}-pattern.json`} download>Download FrankenTextiles pattern ↓</a><a href="/sundress/fit-comparison.json" download>Three native flat checks ↓</a><a href="/media/wardrobe-sundress.mp4" download>Download fitting-room film ↓</a></div>
      </div></div>
  </main><SiteFooter/></>;
}
