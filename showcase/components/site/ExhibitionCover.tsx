import "./exhibition-cover.css";
import ConstructionStudy from "../construction/ConstructionStudy";

const features=[
  {img:"voile",name:"The garden sundress",action:"Pose the fitted body",href:"/wardrobe/sundress/",pdf:"/sundress/pattern-A4.pdf"},
  {img:"cord",name:"The hussar’s jacket",action:"Inspect the braid and cut",href:"/wardrobe/hussar/",pdf:"/patterns/hussar-blue.pdf"},
  {img:"cut",name:"The cutting table",action:"Lay out a real pattern",href:"/sections/cut/"},
];
export function ExhibitionCover(){
  return <ConstructionStudy hero/>;
}
export function ExhibitionFeatures(){return <section className="ec-gallery" aria-label="Explore the collection">
  <div className="ec-heading"><h2>On the worktable</h2><span>Illustrated studies</span></div>
  <div className="ec-features">{features.map(f=><article key={f.img}>
    <a href={f.href}><div className="ec-detail"><img src={"/atelier/"+f.img+".webp"} loading="lazy" alt="" width="508" height="1024"/></div>
      <div className="ec-caption"><h3>{f.name}</h3><span aria-hidden="true">↗</span></div><p>{f.action}</p></a>
    {f.pdf&&<a className="ec-pattern" href={f.pdf} download>Full-size pattern ↓</a>}
  </article>)}</div></section>;}
