"use client";
import dynamic from "next/dynamic";
import { useCallback, useState } from "react";
import { axes, dress, hemLine, initial, samples, type Arm } from "./form";
import type { View } from "./SundressScene";
import InlineFilm from "../InlineFilm";
import "./sundress.css";

const Scene=dynamic(()=>import("./SundressScene"),{ssr:false});
function signed(n:number) {return (n>0?"+":"")+n;}

function EaseSection({name,girth}:{name:"bust"|"waist";girth:number}){
  const [gx,gz]=axes(dress[name]),[bx,bz]=axes(girth);
  const gap=dress[name]-girth;
  return <figure><svg viewBox="-22 -15 44 30" role="img" aria-label={name+" section, garment "+dress[name]+" centimetres, form "+girth+" centimetres."}>
    <ellipse rx={bx} ry={bz} fill={gap<0?"#c46847":"#c19169"} fillOpacity=".62" stroke={gap<0?"#923f27":"#9b6945"} strokeWidth=".35"/>
    <ellipse rx={gx} ry={gz} fill="none" stroke="#17675c" strokeWidth=".6" strokeDasharray="1.4 .65"/>
  </svg><figcaption className={gap<0?"sd-short":""}>{name==="bust"?"Bust":"Waist"} · {Math.abs(gap)} cm {gap<0?"short":"ease"}</figcaption></figure>;
}

export default function SundressLab(){
  const [form,setForm]=useState(initial),[view,setView]=useState<View>("Full length"),[failed,setFailed]=useState(false);
  const arm=useCallback((side:Arm,angle:number)=>setForm(p=>({...p,[side]:angle})),[]);
  const fail=useCallback(()=>setFailed(true),[]);
  const measurements=[["Stature","height",154,184],["Bust girth","bust",78,108],["Waist girth","waist",60,94]] as const;
  return <main className="sd-page">
    <header className="sd-nav wrap"><a className="wordmark" href="/">Franken<span>Textiles</span></a><a href="/#wardrobe">← Wardrobe</a></header>
    <div className="wrap sd-wrap"><div className="sd-head"><div><p className="kicker">The fitting room</p><h1>The garden sundress<em>.</em></h1></div>
      <p>A narrow-strapped bodice and a gathered skirt, cut in a single size. Change the form beneath the dress to see how much room the pattern leaves.</p></div>
      <div className="sd-desk"><section className="sd-board" aria-label="Interactive sundress">
        <div className="sd-stage">{failed?<InlineFilm id="wardrobe-sundress" title="Recorded geometric sundress study" controls/>:
          <Scene form={form} view={view} onArm={arm} onError={fail}/>}
          {!failed && <div className="sd-stage-caption"><i/> Geometric preview <span>·</span> fixed cut</div>}</div>
        <div className="sd-viewbar"><div>{(["Full length","Bodice"] as const).map(v=><button key={v} aria-pressed={view===v} onClick={()=>setView(v)} disabled={failed}>{v}</button>)}</div>
          <p>{failed?"WebGL unavailable · recorded view":"Drag a hand to pose · drag the ground to orbit"}</p></div>
      </section><section className="sd-controls" aria-label="Wearer and pose">
        <div className="sd-section"><h2>Wearer</h2><button className="sd-reset" onClick={()=>{setForm(initial);setView("Full length");}}>Reset</button></div>
        <div className="sd-poses" aria-label="Reference form comparisons">{samples.map(({name,bust,waist})=>
          <button key={name} aria-pressed={form.bust===bust&&form.waist===waist} onClick={()=>setForm(p=>({...p,bust,waist}))}>{name}</button>)}</div>
        {measurements.map(([label,key,min,max])=><label key={key}>{label}<output>{form[key]} cm</output>
          <input type="range" min={min} max={max} step={key==="height"?1:.5} value={form[key]} aria-label={label} aria-valuetext={form[key]+" centimetres"}
            onChange={e=>{const value=Number(e.currentTarget.value);setForm(p=>({...p,[key]:value}));}}/></label>)}
        <div className="sd-lines"><div className="sd-section"><h2>Pattern ease</h2><span>cm</span></div>
          <div className="sd-table"><span>Section</span><span>Dress</span><span>Difference</span>
          {(["bust","waist"] as const).map(k=><div key={k}><strong>{k}</strong><span>{dress[k]}</span><b className={dress[k]<form[k]?"sd-negative":""}>{signed(dress[k]-form[k])}</b></div>)}</div>
          <p>{dress.bust<form.bust||dress.waist<form.waist?"A negative difference marks a smaller garment circumference.":
            dress.bust-form.bust<dress.bustBand[0]||dress.waist-form.waist<dress.waistBand[0]?"Room falls below the flat pattern’s minimum ease target.":
            dress.bust-form.bust>dress.bustBand[2]||dress.waist-form.waist>dress.waistBand[2]?"Room exceeds this pattern’s ease band.":"Both girths are within this pattern’s ease band."}</p>
          <details open><summary>Compare cross-sections</summary><div className="sd-cross"><EaseSection name="bust" girth={form.bust}/><EaseSection name="waist" girth={form.waist}/></div>
            <p>The solid areas are the form. Dashed lines show the fixed garment girths at the same scale.</p></details>
        </div>
        <div className="sd-section"><h2>Arm pose</h2></div><div className="sd-poses">{([["At rest",14,19],["Reach",83,29],["Raised",115,106]] as const).map(([name,left,right])=>
          <button key={name} aria-pressed={form.left===left&&form.right===right} onClick={()=>setForm(p=>({...p,left,right}))}>{name}</button>)}</div>
        <details><summary>Adjust each arm</summary>{(["left","right"] as const).map(side=><label key={side}>{side} arm<output>{form[side]}°</output><input type="range" min="4" max="125" value={form[side]} aria-label={side+" arm angle"} onChange={e=>arm(side,Number(e.target.value))}/></label>)}</details>
      </section></div>
      <div className="sd-notes"><div><h2>One dress, different proportions</h2>
        <p>The shoulder straps support the same {dress.shoulderToWaist} cm bodice drop and {dress.skirtLength} cm skirt in every pose. At this stature the nominal hem sits <strong>{hemLine(form.height).toFixed(1)} cm</strong> above the floor.</p>
        <p>The interactive figure is a synthetic geometric mannequin. Ease is calculated from the flat dress dimensions and the selected body girths. The folded skirt is an illustration. Physical fit requires a sewn sample.</p>
        <details><summary>Native solver evidence</summary><p>All eight seam checks passed for the reference pattern. The native dynamic drape stopped during sewing, before the garment settled. The report includes the body-fit residuals and solver refusal.</p>
          <a href="/sundress/drape-result.json" download>Download native drape result ↓</a></details></div>
        <div className="sd-links"><figure className="exhibit-film"><InlineFilm id="wardrobe-sundress" title="Garden sundress fitting-room film" controls/><figcaption>The sundress in motion</figcaption></figure>
          <a href="/sundress/master.svg" download>Download full-scale pattern SVG ↓</a><a href="/sundress/pattern-A4.pdf" download>Download tiled A4 pattern ↓</a>
          <a href="/sundress/pattern.json" download>Download FrankenTextiles pattern ↓</a><a href="/sundress/fit-comparison.json" download>Three native flat checks ↓</a><a href="/media/wardrobe-sundress.mp4" download>Download fitting-room film ↓</a></div>
      </div></div>
  </main>;
}
