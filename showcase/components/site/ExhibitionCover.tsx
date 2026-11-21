import "./exhibition-cover.css";
import ConstructionStudy from "../construction/ConstructionStudy";

const features=[
  {img:"skirt-editorial",meta:"6 gores · 65–80 cm",name:"The skirt atelier",action:"Compare column and flared sweeps on fitted forms",href:"/studies/skirt/",pdf:"/atelier/flare.pdf"},
  {img:"voile",meta:"4 pieces · Anny fit",name:"The garden sundress",action:"Pose the fitted body and test bodice ease",href:"/wardrobe/sundress/",pdf:"/sundress/pattern-A4.pdf"},
  {img:"cord",meta:"49 templates · 1761",name:"The hussar’s jacket",action:"Inspect reconstructed shells and two-pass braid",href:"/wardrobe/hussar/",pdf:"/patterns/hussar-blue.pdf"},
  {img:"cut",meta:"Live WASM · roll marker",name:"The cutting table",action:"Draft to size and nest pieces on a fabric roll",href:"/sections/cut/"},
];
export function ExhibitionCover(){
  return <ConstructionStudy hero/>;
}
export function ExhibitionFeatures(){return <section className="ec-gallery" aria-label="Explore the collection">
  <div className="wrap">
    <div className="ec-heading"><h2>On the worktable</h2><span>Interactive studies &amp; downloadable patterns</span></div>
    <div className="ec-features">{features.map(f=><article key={f.img}>
      <a href={f.href}><div className="ec-detail"><img src={"/atelier/"+f.img+".webp"} loading="lazy" alt="" width="508" height="340"/></div>
        <span className="ec-meta mono">{f.meta}</span>
        <div className="ec-caption"><h3>{f.name}</h3><span aria-hidden="true">↗</span></div><p>{f.action}</p></a>
      {f.pdf&&<a className="ec-pattern" href={f.pdf} download>Full-size A4 pattern ↓</a>}
    </article>)}</div>
  </div></section>;}
