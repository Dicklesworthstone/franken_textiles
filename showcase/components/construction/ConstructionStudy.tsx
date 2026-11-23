"use client";

import {useEffect, useId, useRef, useState} from "react";
import {filmFile, filmSource, PlaybackMark, watchFilm} from "../InlineFilm";
import "./construction.css";

const studies = {
  sundress: {name:"Garden sundress", cut:"Four pieces", text:"The front and back bodices meet at the shoulders. The longer skirt edges gather into their shorter waist seams.", pdf:"/sundress/pattern-A4.pdf", href:"/wardrobe/sundress/#sewn"},
  eased: {name:"Lightly eased sundress", cut:"Four pieces · revised waist", text:"Each skirt panel loses 97.5 mm at the waist while keeping its original hem. Follow the narrower cut onto the same bodice and wearer.", pdf:"/exhibits/sundress/light-A4.pdf", href:"/wardrobe/sundress/?garment=eased#sewn"},
  column: {name:"Column skirt", cut:"Six gores · 68 cm form", text:"Six long curved panels close around the high hip. Their nearly straight lower edges give the column its narrow sweep.",pdf:"/atelier/column.pdf", href:"/studies/skirt/?cut=column&waist=680#skirt-atelier"},
  flare: {name:"Flared skirt", cut:"Six gores · 68 cm form", text:"Each gore widens from the waist to the hem. Joined edge to edge, the six pieces give the hem 168 cm of sweep.",pdf:"/atelier/flare.pdf", href:"/studies/skirt/?cut=flare&waist=680#skirt-atelier"},
  blue: {name:"Blue Hussar dolman", cut:"1761 · jacket shell", text:"The paired fronts carry the cord from the cutting table onto the chest. The back, side pieces and two-part sleeves close around the form.",pdf:"/patterns/hussar-blue.pdf", href:"/wardrobe/hussar/#native-dolman"},
  yellow: {name:"Yellow Hussar dolman", cut:"1765 · jacket shell", text:"Pale cord follows the same jacket front into the sewn shape. This color study uses the Blue dolman’s recorded drape. The Yellow uniform has its own full pattern.",pdf:"/patterns/hussar-yellow.pdf", href:"/wardrobe/hussar/#native-dolman"},
  "bolero-full-hand": {name:"355 mm Zouave bolero", cut:"355 mm sleeve · 201.7 mm cuff", text:"All 49 physical frames passed the full-hand contact check. Turn on the fitted form in the 3D replay to inspect the clearance.", pdf:"/fittings/bolero-full-hand-A4.pdf", href:"/fittings/bolero-full-hand-replay"},
  bolero: {name:"Zouave bolero", cut:"Five pieces · 52 cm chest", text:"Two fronts meet the back at the shoulders. Each sleeve rolls into a tube before its cap joins the armhole.",pdf:"/patterns/bolero.pdf", href:"/sections/studies/#drape-room"},
  trousers: {name:"Gathered trousers", cut:"Seven pieces · standing form", text:"The legs close along their inseams. Shorter cuffs and an elastic waistband gather the fullness at both ends.",pdf:"/patterns/trousers.pdf", href:"/studies/trousers/#trouser-fit"},
  ensemble: {name:"Bolero and trousers", cut:"Twelve pieces · coupled drape", text:"Follow both cuts into one outfit. The final sewing and gravity steps include contact between the jacket and trousers.",pdf:"/patterns/bolero.pdf", href:"/sections/studies/?outfit=joint#drape-room"},
  fez: {name:"Felt fez", cut:"Crown and tip · geometric assembly", text:"Close the two ends of the curved crown and its upper edge becomes a circle. The round tip fits that opening. The fold follows the drafted cone dimensions.",pdf:"/patterns/fez.pdf", href:"/sections/draft/"},
} as const;
export type Garment = keyof typeof studies;
const all:readonly Garment[] = ["flare","column","sundress","eased","blue","yellow","bolero","trousers","ensemble","fez"];
const homepage:readonly Garment[] = ["bolero-full-hand", "sundress", "eased", "flare", "column", "bolero", "trousers", "ensemble", "blue", "yellow", "fez"];
export const constructionFilms=homepage.map(id=>({id:"construction-"+id,title:studies[id].name+" · from cut to cloth",detail:studies[id].cut}));
const chapters = [
  {name:"Flat pattern", image:"cut", frame:0},
  {name:"Match the edges", image:"join", frame:53},
  {name:"Sewn article", image:"sewn", frame:90},
] as const;

export function ConstructionMotion({garment, hero=false}:{garment:Garment;hero?:boolean}) {
  const data=studies[garment], base="construction-"+garment;
  const imageFile=(index:number)=>filmFile(index===2?base:base+"-"+chapters[index].image,"jpg");
  const video=useRef<HTMLVideoElement>(null);
  const manual=useRef<boolean|undefined>(undefined), wantTime=useRef<number|null>(hero ? 3.875 : null);
  // The 4K film or its 1080p copy, once the device has said which it can decode.
  const source=useRef<string|undefined>(undefined);
  const firstChapter = hero ? 2 : 0;
  const [playing,setPlaying]=useState(false),[chapter,setChapter]=useState(firstChapter),[still,setStill]=useState<number|null>(firstChapter);
  useEffect(()=>{
    const el=video.current!, reduced=matchMedia("(prefers-reduced-motion: reduce)");
    const save=(navigator as Navigator & {connection?:{saveData?:boolean}}).connection?.saveData;
    let visible=false, live=true;
    source.current=undefined;
    void filmSource(base).then(path=>{if(live){source.current=path;sync();}});
    const unwatch=watchFilm(el,base);
    function sync(){
      if(!visible||document.hidden||!(manual.current??(!reduced.matches&&!save))){el.pause();return;}
      if(!source.current)return;
      if(!el.src)el.src=source.current;
      void el.play().catch(()=>{});
    }
    const observer=new IntersectionObserver(([entry])=>{visible=entry.intersectionRatio>=.32;sync();},{threshold:[0,.32]});
    observer.observe(el);document.addEventListener("visibilitychange",sync);reduced.addEventListener("change",sync);
    return ()=>{live=false;unwatch();observer.disconnect();document.removeEventListener("visibilitychange",sync);reduced.removeEventListener("change",sync);el.pause();};
  },[base]);
  function pick(n:number){
    const el=video.current!;manual.current=false;el.pause();setChapter(n);setStill(n);
    wantTime.current=chapters[n].frame/16;
    if(el.readyState>=1){el.currentTime=wantTime.current;wantTime.current=null;}
  }
  function play(){
    const el=video.current!;
    if(!el.paused){manual.current=false;el.pause();return;}
    manual.current=true;
    if(chapter===2){wantTime.current=0;if(el.readyState>=1){el.currentTime=0;wantTime.current=null;}}
    // Before the source is known, `manual` is enough: the effect starts the film as soon as it is.
    if(!source.current)return;
    if(!el.src)el.src=source.current;
    void el.play().catch(()=>{});
  }
  return <div className="construction-motion" data-garment={garment}>
    <div className="construction-image">
      <video ref={video} muted playsInline loop preload="none" aria-label={data.name+" from pattern to assembly"}
        onLoadedMetadata={()=>{if(wantTime.current!==null){video.current!.currentTime=wantTime.current;wantTime.current=null;}}}
        onPlaying={()=>{setStill(null);setPlaying(true);}} onPause={()=>setPlaying(false)}
        onError={()=>{setPlaying(false);setStill(chapter);}}
        onTimeUpdate={()=>{if(!video.current?.paused){const f=video.current!.currentTime*16;setChapter(f<26?0:f<77?1:2);}}}/>
      {still!==null&&<img className="construction-still" src={imageFile(still)}
        fetchPriority={hero?"high":"auto"} alt={data.name+" · "+chapters[still].name}/>}
      <button type="button" className="construction-transport" onClick={play} aria-label={(playing?"Pause":"Play")+" construction film"}>
        <PlaybackMark playing={playing}/>{playing?"Pause":"Play construction"}</button>
    </div>
    <div className="construction-chapters" role="group" aria-label="Construction chapters">{chapters.map((c,i)=>
      <button key={c.image} type="button" aria-pressed={chapter===i} onClick={()=>pick(i)}>
        {!hero&&<img src={imageFile(i)} alt="" loading="lazy"/>}
        <span>{c.name}</span></button>)}</div>
  </div>;
}

export default function ConstructionStudy({hero=false,choices=hero?homepage:all,value,onSelect}:{choices?:readonly Garment[];hero?:boolean;value?:Garment;onSelect?:(garment:Garment)=>void}) {
  const [local,select]=useState<Garment>(choices[0]),id=useId();
  const selected=value??local,study=studies[selected];
  useEffect(()=>{
    if(value!==undefined)return;
    const key=new URLSearchParams(location.search).get("garment") as Garment|null;
    if(key&&choices.includes(key))select(key);
  },[choices,value]);
  const selector = <label className="construction-select"><span>Garment</span>
    <select value={selected} onChange={e=>(onSelect??select)(e.target.value as Garment)}>
      {choices.map(k=><option key={k} value={k}>{studies[k].name}</option>)}
    </select>
  </label>;
  const patterns = <div className="construction-links">
    <a href={study.pdf} download>{selected==="ensemble"?"Jacket pattern":"Full-scale A4 pattern"}</a>
    {selected==="ensemble"&&<a href="/patterns/trousers.pdf" download>Trouser pattern</a>}
  </div>;
  if (hero) {
    const fullHand = selected === "bolero-full-hand";
    return <section id="construction" className="construction-study construction-hero" aria-labelledby={id}>
      <div className="construction-copy">
        <h1 id={id}>Sewing patterns <em>in motion</em></h1>
        <p className="construction-pitch">Draft to measurements, see how the pieces sew and drape, and print the same cut at full size.</p>
        <div className="construction-cta">
          <a className="construction-primary" href={study.href}>{fullHand ? "Turn the bolero in 3D" : "Open garment study"} <span aria-hidden="true" className="arrow"/></a>
          <a className="construction-secondary" href={fullHand ? "/workbench/?sample=full-hand-bolero" : "/#workbenches"}>{fullHand ? "Edit the draft" : "Open a workbench"}</a>
        </div>
      </div>
      <div className="construction-exhibit">
        <div className="construction-exhibit-bar">
          <span>Construction study</span>
          {selector}
        </div>
        <ConstructionMotion key={selected} garment={selected} hero/>
        <div className="construction-exhibit-note">
          <div>
            <p className="construction-spec">{study.cut}</p>
            <p className="construction-description">{study.text}</p>
          </div>
          {patterns}
        </div>
      </div>
    </section>;
  }
  return <section className="construction-study" aria-labelledby={id}>
    <div className="construction-copy">
      <p className="construction-eyebrow">Construction study</p>
      <h2 id={id}>From cut <em>to cloth.</em></h2>
      <div>
        {choices.length>1?selector:<h3>{study.name}</h3>}
        <p className="construction-spec">{study.cut}</p><p className="construction-description">{study.text}
          {selected!=="fez"&&" Lines on the board mark matching edges. The film cuts to the checked seam placement, then follows the cloth under gravity."}</p>
        {patterns}
      </div>
    </div>
    <ConstructionMotion key={selected} garment={selected}/>
  </section>;
}
