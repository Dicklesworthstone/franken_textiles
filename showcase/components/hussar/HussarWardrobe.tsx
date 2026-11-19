"use client";

import dynamic from "next/dynamic";
import {useCallback,useEffect,useRef,useState} from "react";
import type {Layer,Framing,RegimentView} from "./RegimentalStudy";
import type {Mount,Section,PieceReading} from "./PatternMount";
import RegimentalCuts from "./RegimentalCuts";

const Scene=dynamic(()=>import("./HussarScene"),{ssr:false});

function options<T extends string>(name:string,items:readonly T[],value:T,select:(choice:T)=>void){
  return <fieldset className="costume-choices"><legend>{name}</legend>{items.map(item=>
    <label key={item}><input type="radio" name={"costume-"+name} checked={item===value} value={item} onChange={()=>select(item)}/><span>{item}</span></label>)}</fieldset>;
}

export default function HussarWardrobe(){
  const [regiment,setRegiment]=useState<RegimentView>("Compare");
  const [layer,setLayer]=useState<Layer>("Dressed");
  const [section,setSection]=useState<Section>("Front");
  const [layout,setLayout]=useState<Mount>("Curved");
  const [reading,readPiece]=useState<PieceReading|null>(null);
  const [framing,setFraming]=useState<{preset:Framing}>({preset:"Uniform"});
  const [failed,setFailed]=useState(false), failure=useCallback(()=>setFailed(true),[]);
  const cabinet=useRef<HTMLElement>(null),[near,setNear]=useState(false);
  useEffect(()=>{
    const observer=new IntersectionObserver(([entry])=>{if(entry.isIntersecting){setNear(true);observer.disconnect();}},{rootMargin:"400px"});
    observer.observe(cabinet.current!);return()=>observer.disconnect();
  },[]);
  const names=regiment==="Compare"?["Blue","Yellow"]:[regiment];
  return <><section ref={cabinet} className="costume-cabinet" aria-labelledby="costume-heading" id="dress-study">
    <div className="costume-display"><div className="hu-stage costume-stage" data-layer={layer}>
      {failed||!near?<img loading="lazy" className="costume-fallback" src="/wardrobe/hussar/regimental-portrait.webp" alt="Costume interpretation of Putbus’s Blue Hussar and Wrangel’s Yellow Hussar in full dress"/>:
        <Scene regiment={regiment} layer={layer} framing={framing} section={section} layout={layout} onFailure={failure} onPiece={readPiece}/>}
      <span className="hu-plate">{layer==="Dressed"?"Costume reconstruction":"Pattern study · "+layer}</span>
      <span className="hu-tag">{layer!=="Dressed"?layer+" · "+section:regiment==="Compare"?"Putbus, 1761 · Wrangel, 1765":regiment==="Blue"?"Putbus’s Blue Hussars · 1761":"Wrangel’s Yellow Hussars · 1765"}</span>
    </div>{!failed&&<div className="costume-camera"><span>Drag to turn · Scroll to zoom</span>
      {options("Viewpoint",layer==="Dressed"?["Uniform","Bust","Chest","Side","Reverse"] as const:["Uniform","Chest","Side","Reverse"] as const,framing.preset,preset=>setFraming({preset}))}</div>}</div>
    <aside className="costume-settings"><p className="kicker">{layer==="Dressed"?"Swedish Pomerania":"The cutting table"}</p>
      <h2 id="costume-heading">{layer==="Dressed"?"In full dress.":layer+" pattern."}</h2>
      {!failed&&<>{options("Regiment",["Compare","Blue","Yellow"] as const,regiment,setRegiment)}
        {options("Study",["Dressed","Dolman","Pelisse"] as const,layer,next=>{setLayer(next);setFraming({preset:"Uniform"});})}</>}
      {layer==="Dressed"?<>
        <div className="costume-sources">{names.map(name=><a href="#regimental-plates" key={name}><img src={"/wardrobe/hussar/"+name.toLowerCase()+"-plate.jpg"} alt={name+" trooper in the museum plate"}/><span>{name==="Blue"?"Blue · 1761":"Yellow · 1765"}</span></a>)}</div>
        <p>Schützercrantz shows yellow cord on blue for Putbus and white cord on black for Wrangel. Eight AI-generated views based on those plates supply each 3D figure’s shape and colour. The faces and unseen details are imagined.</p>
      </>:<>{options("Piece",["Front","Upper sleeve","Back","Cuff","Collar"] as const,section,setSection)}
        {options("Mount",["Curved","Flat"] as const,layout,setLayout)}
        {reading&&<dl className="hu-readings"><div><dt>Seam outline</dt><dd>{Math.round(reading.widthMm)} × {Math.round(reading.heightMm)} mm</dd></div>
          <div><dt>Seam allowance</dt><dd>{reading.allowance} mm</dd></div>
          {reading.cords>0&&<div><dt>Cord paths per cut</dt><dd>{reading.cords}</dd></div>}</dl>}
        <p>{section==="Front"?"Each cord follows its placement line in the cutting pattern. The buttons sit on the marked stations. Choose Flat to open one mirrored forepart.":"The panel follows the curves in the downloadable pattern. Choose Flat to inspect its seam outline."} Curved shows the cut on a display support.</p>
      </>}
      <a className="costume-jump" href="#native-dolman">Watch the dolman sew together ↓</a>
      <div className="costume-files">{names.map(name=><a href={"/patterns/hussar-"+name.toLowerCase()+".pdf"} download key={name}>{name} ensemble PDF ↓</a>)}</div>
    </aside>
  </section><RegimentalCuts/></>;
}
