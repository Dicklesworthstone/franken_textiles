"use client";

import {useEffect,useId,useRef,useState} from "react";
import {Drawing,readPieces,type Piece} from "../DraftLab";

// Windows onto the 1024 x 1536 costume plates. The separate pelisse plate has both coats side by side.
const items=[
  {name:"Breeches",prefix:["breech","fall","knee"],crop:"294 737 420 295",join:"The front fall, its stay and the knee openings complete the paired fronts and backs."},
  {name:"Dolman",prefix:["dolman"],crop:"236 407 550 446",join:"The shaped side backs close the waist. The corded fronts meet down the centre."},
  {name:"Pelisse",prefix:["pelisse"],crop:"",join:"The two sleeve pieces form a tube. Separate fur strips finish the hem and front opening."},
  {name:"Charivades",prefix:["leg_"],crop:"325 868 375 331",join:"The halves join along the leg. A separate placket supports the row of closures."},
  {name:"Cap",prefix:["mirliton","busby"],crop:"352 100 325 254",join:"The wall closes around the tip. The sweatband follows the lower edge."},
  {name:"Gloves",prefix:["glove","web_","thumb"],crop:"260 804 162 218",join:"Finger webs join palm and back. A gusset gives the thumb its opening."},
  {name:"Neck stock",prefix:["stock_"],crop:"392 364 240 119",join:"The face and lining turn together into the narrow band at the neck."},
  {name:"Sash",prefix:["sash_"],crop:"352 712 328 135",join:"The long face is backed and finished with ties at the waist."},
  {name:"Crossbelts",prefix:["crossbelt"],crop:"337 427 343 392",join:"The paired leather straps cross over the two fronts of the dolman."},
] as const;

export default function RegimentalCuts(){
  const [regiment,setRegiment]=useState("blue"),[item,setItem]=useState(0),[selected,select]=useState("");
  const [sheet,setSheet]=useState<{regiment:string;pieces:Piece[]}>(),[failed,setFailed]=useState("");
  const view=useRef<HTMLElement>(null),[near,setNear]=useState(false);
  const clip=useId();
  const base="/patterns/hussar-"+regiment,group=items[item];
  useEffect(()=>{
    const observer=new IntersectionObserver(([entry])=>{if(entry.isIntersecting){setNear(true);observer.disconnect();}},{rootMargin:"400px"});
    observer.observe(view.current!);return()=>observer.disconnect();
  },[]);
  useEffect(()=>{
    if(!near)return;
    const abort=new AbortController();
    fetch(base+".svg",{signal:abort.signal}).then(async response=>{
      if(!response.ok)throw Error("Missing master sheet");
      const pieces=readPieces(await response.text());
      if(!abort.signal.aborted)setSheet({regiment,pieces:pieces.filter((piece,i)=>pieces.findIndex(p=>p.id===piece.id)===i)});
    }).catch(()=>{if(!abort.signal.aborted)setFailed(regiment);});
    return()=>abort.abort();
  },[base,regiment,near]);
  const pieces=sheet?.regiment===regiment?sheet.pieces.filter(p=>group.prefix.some(prefix=>p.id.startsWith(prefix))):[];
  const piece=pieces.find(p=>p.id===selected)??pieces[0],pelisse=group.name==="Pelisse";
  const crop=pelisse?(regiment==="blue"?"0 0 720 900":"720 0 720 900"):group.crop;
  const [x,y,width,height]=crop.split(" ").map(Number);
  return <section ref={view} className="regimental-cuts" aria-labelledby="uniform-cuts" id="uniform-pieces">
    <header><div><p className="kicker">The uniform pattern</p><h2 id="uniform-cuts">Inside the <em>full dress.</em></h2></div>
      <label>Regiment<select value={regiment} onChange={e=>setRegiment(e.target.value)}><option value="blue">Blue Hussars · 1761</option><option value="yellow">Yellow Hussars · 1765</option></select></label></header>
    <div className="regimental-items" role="group" aria-label="Uniform article">{items.map((g,i)=><button key={g.name} aria-pressed={item===i} onClick={()=>{setItem(i);select("");}}>{g.name}</button>)}</div>
    <div className="regimental-pair"><figure><div className="regimental-cut">{piece?<Drawing piece={piece}/>:<p role="status">{failed===regiment?"Master sheet unavailable. The complete pattern is linked below.":"Opening the native master sheet…"}</p>}</div>
      <figcaption><label>Flat piece<select value={piece?.id??""} disabled={!pieces.length} onChange={e=>select(e.target.value)}>{pieces.map(p=><option key={p.id} value={p.id}>{p.name}</option>)}</select></label>
      {piece&&<small>{piece.quantity} · {piece.allowance} mm allowance</small>}</figcaption></figure>
      <figure><svg role="img" aria-label={regiment+" "+group.name+" in the costume reconstruction"} viewBox={crop}>
        <defs><clipPath id={clip}><rect x={x} y={y} width={width} height={height}/></clipPath></defs>
        <image clipPath={"url(#"+clip+")"} href={"/wardrobe/hussar/"+(pelisse?"pelisses":regiment+"-dressed")+".webp"} width={pelisse?1440:1024} height={pelisse?900:1536}/></svg>
        <figcaption><strong>{group.name}</strong><small>Costume reconstruction</small></figcaption></figure></div>
    <div className="regimental-note"><p>{group.join}</p><a href={base+".pdf"} download>Full uniform pattern PDF ↓</a><a href={base+".svg"} download>Full-scale master SVG ↓</a></div>
  </section>;
}
