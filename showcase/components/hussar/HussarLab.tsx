"use client";

import dynamic from "next/dynamic";
import { useCallback, useState } from "react";
import InlineFilm from "../InlineFilm";
import HussarReferences from "./HussarReferences";
import { type Layer, type Framing, type RegimentView } from "./uniform";
import sampler from "../../public/wardrobe/hussar/braid-sampler.json";
import "./hussar.css";

const Scene=dynamic(()=>import("./HussarScene"),{ssr:false});
const svgPath=sampler.pieces[0].decor[0].pts.map(([x,y],i)=>`${i?"L":"M"}${x} ${36-y}`).join(" ");
const source="https://sfhm.se/wp-content/uploads/2025/04/SFHM_skriftserie_Svenska_armens_uniformer_del_1_kavalleriet.pdf#page=21";
function choice<T extends string>(name:string,choices:readonly T[],value:T,set:(v:T)=>void) {
  return <fieldset><legend>{name}</legend><div className="hu-choice">{choices.map(item=><button type="button" key={item} aria-pressed={value===item} onClick={()=>set(item)}>{item}</button>)}</div></fieldset>;
}
function WardrobeFilm({compact=false}:{compact?:boolean}) {
  return (
    <div className={compact ? "hu-fallback" : "hu-film"}>
      <InlineFilm id="wardrobe-hussar" title="Rendered tour of the two hussar uniforms" controls/>
    </div>
  );
}
export default function HussarLab(){
  const [regiment, setRegiment] = useState<RegimentView>("Blue");
  const [layer, setLayer] = useState<Layer>("Dressed");
  const [framing, setFraming] = useState<{ preset: Framing }>({preset:"Bust"});
  const [failed,setFailed]=useState(false);const onFailure=useCallback(()=>setFailed(true),[]);const blue=regiment==="Blue";
  return <main className="hu-page"><header className="hu-mast wrap"><a className="wordmark" href="/">Franken<span>Textiles</span></a><a href="/#wardrobe">← Wardrobe</a></header>
    <div className="wrap hu-wrap"><div className="hu-heading"><div><p className="kicker">Swedish uniforms / pattern of 1761</p><h1>The hussar’s cabinet<em>.</em></h1></div>
      <p>Dress the figure in Putbus’s blue or Wrangel’s black and chamois. <a className="hu-compare-link" href="#regimental-plates">Compare with the museum plate ↓</a></p></div>
    <div className="hu-desk"><div className="hu-figure">
      <div className="hu-stage">{failed?<WardrobeFilm compact/>:<Scene regiment={regiment} layer={layer} framing={framing} onFailure={onFailure}/>}
      {!failed&&<><span className="hu-plate">Swedish Pomerania · Uniform pattern of 1761</span><span className="hu-tag">{regiment==="Compare"?"Blue, 1761 at left · Yellow, 1765 at right":blue?"Putbus’s Blue Hussars · 1761 dress":"Wrangel’s Yellow Hussars · 1765 dress"}</span></>}</div>
      <div className="hu-cam"><span>{failed?"Recorded wardrobe tour":"Drag to turn · Scroll to zoom"}</span>
        {!failed && choice("Camera", ["Bust","Uniform","Chest","Reverse"] as const, framing.preset, preset => setFraming({preset}))}
      </div>
    </div><aside className="hu-controls"><p className="kicker">Dressing room</p><h2>{failed ? "Garment notes." : "Dress the figure."}</h2>
      {choice("Regiment",["Blue","Yellow","Compare"] as const,regiment,setRegiment)}
      {choice("Garment layers",["Dressed","Dolman","Pelisse"] as const,layer,setLayer)}
      <div className="hu-palette" aria-hidden="true">{(blue?["#243b53","#d9b15b","#252724"]:["#222725","#ceb072","#dddccb"]).map(c=><i key={c} style={{background:c}}/>)}</div>
      <h3>{regiment==="Compare"?"Two regimental wardrobes":layer==="Pelisse"?"The pelisse":layer==="Dolman"?"The dolman":blue?"Putbus’s regiment":"Wrangel’s regiment"}</h3>
      <p>{regiment==="Compare"?"Putbus’s yellow lace and blue wool appear at left. Wrangel’s white lace, yellow cuffs and chamois pelisse appear at right. Switch between jacket layers to compare the same component on each figure.":
        layer==="Pelisse"?`The short outer jacket uses ${blue?"dark blue broadcloth and a black lamb-fur lining":"chamois-colored broadcloth, a pale lining and black fur edging"}. Turn it to examine its open fronts and empty sleeves.`:
        layer==="Dolman"?`The fitted jacket carries ${blue?"yellow lace on dark blue broadcloth":"white lace and yellow cuffs on black broadcloth"}. Choose Chest to follow the loops between its buttons.`:
        blue?"Dark blue broadcloth and yellow lace belong to the Blue regiment. A black felt mirliton and a braided barrel sash complete the silhouette.":"The Yellow regiment wore a black dolman with yellow cuffs and white lace, a chamois pelisse and a fur cap with a yellow bag."}</p>
      <div className="hu-pattern-links">{(regiment==="Compare"?["Blue","Yellow"]:[regiment]).map(name=>
        <a key={name} href={`/patterns/hussar-${name.toLowerCase()}.pdf`} download>Download {name} ensemble pattern PDF ↓</a>)}</div>
      <details className="hu-pattern-notes"><summary>Size, pieces and printing</summary>
        <p>Chest 100 cm, waist 86 cm, head 58 cm. Each reconstructed cut has 49 templates covering the dolman, pelisse, breeches, charivades, cap, gauntlets, stock, sash and crossbelt. Supply boots, buttons, plume and hardware separately.</p>
        <p>Print at actual size on A4. The Blue set has 81 pages and the Yellow set has 79. Check both the 100 mm and 4 inch squares before joining the sheets. Test the cut in a toile.</p>
      </details>
    </aside></div>
    <HussarReferences/>
    <section className="hu-reel"><div><p className="kicker">Wardrobe film</p><h2>Around the figure.</h2><p>Watch the camera move from the braided chest to the full silhouette and its second regimental colorway.</p><a href="/media/wardrobe-hussar.mp4" download>Download the film ↓</a></div>{!failed && <WardrobeFilm/>}</section>
    <section className="hu-sample"><div className="hu-braid"><svg viewBox="0 0 100 36" role="img" aria-label="The jacket’s looped chest cord laid flat"><path d={svgPath} fill="none" stroke="#81612f" strokeWidth="2.5" strokeLinejoin="round"/><path d={svgPath} fill="none" stroke="#ebca7e" strokeWidth="1.25" strokeLinejoin="round"/></svg><span>100 × 36 mm sampler board</span></div>
      <div><p className="kicker">From cord to machine</p><h2>Take a braid sample.</h2><p>The 3D uniform is an illustrated reconstruction. Its chest braid starts with this original flat loop. The native FrankenTextiles exporter wrote the two-pass Tajima file, including the stop to lay the cord.</p>
        <div className="hu-files"><a href="/wardrobe/hussar/braid-sampler.dst" download>Download DST ↓</a><a href="/wardrobe/hussar/braid-sampler.json" download>Pattern JSON ↓</a></div></div></section>
    <details className="hu-history"><summary>Historical references and model scope</summary>
      <p>The Swedish military-history museums describe the uniform pattern of 1761 on page 39. Schützercrantz’s plate on page 40 depicts the Blue trooper in 1761 and the Yellow trooper in 1765. The face, neck and hands use a synthetic adult male fitted with FrankenTextiles’ native Anny body model. The figure and garments use generated surface textures. The folds, grooming and garment dimensions are interpretive. The native embroidery export applies to the flat sampler.</p>
      <p><a href={source}>Swedish cavalry uniforms, pp. 38–39 ↗</a> · <a href="https://historicalsoldiers.com/">Historical Soldiers visual reference ↗</a></p></details>
    </div></main>;
}
