"use client";

import {useEffect, useId, useRef, useState} from "react";
import {filmFile, PlaybackMark} from "../InlineFilm";
import "./construction.css";

const studies = {
  sundress: {name:"Garden sundress", cut:"Four pieces", text:"The front and back bodices meet at the shoulders. The longer skirt edges gather into their shorter waist seams.", pdf:"/sundress/pattern-A4.pdf", href:"/wardrobe/sundress/#sewn"},
  column: {name:"Column skirt", cut:"Six gores · 68 cm form", text:"Six long curved panels close around the high hip. Their nearly straight lower edges give the column its narrow sweep.",pdf:"/atelier/column.pdf", href:"/studies/skirt/?cut=column&waist=680#skirt-atelier"},
  flare: {name:"Flared skirt", cut:"Six gores · 68 cm form", text:"Each gore widens from the waist to the hem. Joined edge to edge, the six pieces give the hem 168 cm of sweep.",pdf:"/atelier/flare.pdf", href:"/studies/skirt/?cut=flare&waist=680#skirt-atelier"},
  blue: {name:"Blue Hussar dolman", cut:"1761 · jacket shell", text:"The paired fronts carry the cord from the cutting table onto the chest. The back, side pieces and two-part sleeves close around the form.",pdf:"/patterns/hussar-blue.pdf", href:"/wardrobe/hussar/#native-dolman"},
  yellow: {name:"Yellow Hussar dolman", cut:"1765 · jacket shell", text:"Pale cord follows the same jacket front into the sewn shape. This color study uses the Blue dolman’s recorded drape. The Yellow uniform has its own full pattern.",pdf:"/patterns/hussar-yellow.pdf", href:"/wardrobe/hussar/#native-dolman"},
  bolero: {name:"Zouave bolero", cut:"Five pieces · 52 cm chest", text:"Two fronts meet the back at the shoulders. Each sleeve rolls into a tube before its cap joins the armhole.",pdf:"/patterns/bolero.pdf", href:"/sections/studies/#drape-room"},
  trousers: {name:"Gathered trousers", cut:"Seven pieces · standing form", text:"The legs close along their inseams. Shorter cuffs and an elastic waistband gather the fullness at both ends.",pdf:"/patterns/trousers.pdf", href:"/studies/trousers/#trouser-fit"},
  ensemble: {name:"Bolero and trousers", cut:"Twelve pieces · coupled drape", text:"Follow both cuts into one outfit. The final sewing and gravity steps include contact between the jacket and trousers.",pdf:"/patterns/bolero.pdf", href:"/sections/studies/#drape-room"},
  fez: {name:"Felt fez", cut:"Crown and tip · geometric assembly", text:"Close the two ends of the curved crown and its upper edge becomes a circle. The round tip fits that opening. The fold follows the drafted cone dimensions.",pdf:"/patterns/fez.pdf", href:"/sections/draft/"},
} as const;
export type Garment = keyof typeof studies;
const all:readonly Garment[] = ["flare","column","sundress","blue","yellow","bolero","trousers","ensemble","fez"];
export const constructionFilms=all.map(id=>({id:"construction-"+id,title:studies[id].name+" · from cut to cloth",detail:studies[id].cut}));
const chapters = [
  {name:"Flat pattern", image:"cut", frame:0},
  {name:"Join the pieces", image:"join", frame:53},
  {name:"Sewn article", image:"sewn", frame:90},
] as const;

function Motion({garment, hero}:{garment:Garment;hero:boolean}) {
  const data=studies[garment], base="construction-"+garment;
  const imageFile=(index:number)=>filmFile(index===2?base:base+"-"+chapters[index].image,"jpg");
  const video=useRef<HTMLVideoElement>(null);
  const manual=useRef<boolean|undefined>(undefined), wantTime=useRef<number|null>(null);
  const [playing,setPlaying]=useState(false),[chapter,setChapter]=useState(0),[still,setStill]=useState<number|null>(0);
  useEffect(()=>{
    const el=video.current!, reduced=matchMedia("(prefers-reduced-motion: reduce)");
    const save=(navigator as Navigator & {connection?:{saveData?:boolean}}).connection?.saveData;
    let visible=false;
    function sync(){
      if(!visible||document.hidden||!(manual.current??(!reduced.matches&&!save))){el.pause();return;}
      if(!el.src)el.src=filmFile(base);
      void el.play().catch(()=>{});
    }
    const observer=new IntersectionObserver(([entry])=>{visible=entry.intersectionRatio>=.32;sync();},{threshold:[0,.32]});
    observer.observe(el);document.addEventListener("visibilitychange",sync);reduced.addEventListener("change",sync);
    return ()=>{observer.disconnect();document.removeEventListener("visibilitychange",sync);reduced.removeEventListener("change",sync);el.pause();};
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
    if(!el.src)el.src=filmFile(base);
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
        <img src={imageFile(i)} alt="" loading={hero?"eager":"lazy"}/>
        <span>{c.name}</span></button>)}</div>
  </div>;
}

export default function ConstructionStudy({choices=all,hero=false}:{choices?:readonly Garment[];hero?:boolean}) {
  const [selected,select]=useState<Garment>(choices[0]),id=useId(),study=studies[selected];
  useEffect(()=>{
    const key=new URLSearchParams(location.search).get("garment") as Garment|null;
    if(key&&choices.includes(key))select(key);
  },[choices]);
  return <section id={hero?"construction":undefined} className={"construction-study"+(hero?" construction-hero":"")} aria-labelledby={id}>
    <div className="construction-copy">
      <p className="construction-eyebrow">{hero?"FrankenTextiles / Construction studies":"Construction study"}</p>
      {hero?<h1 id={id}>From cut<br/><em>to cloth.</em></h1>:<h2 id={id}>From cut <em>to cloth.</em></h2>}
      {choices.length>1?<label className="construction-select">Choose a garment<select value={selected} onChange={e=>select(e.target.value as Garment)}>
        {choices.map(k=><option key={k} value={k}>{studies[k].name}</option>)}</select></label>:<h3>{study.name}</h3>}
      <p className="construction-spec">{study.cut}</p><p className="construction-description">{study.text}</p>
      <div className="construction-links">{hero&&<a href={study.href}>Open the study ↗</a>}
        <a href={study.pdf} download>{selected==="ensemble"?"Jacket pattern":"Full-size pattern"} ↓</a>
        {selected==="ensemble"&&<a href="/patterns/trousers.pdf" download>Trouser pattern ↓</a>}</div>
    </div>
    <Motion key={selected} garment={selected} hero={hero}/>
  </section>;
}
