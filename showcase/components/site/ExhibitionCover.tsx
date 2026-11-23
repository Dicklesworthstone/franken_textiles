import "./exhibition-cover.css";
import ConstructionStudy from "../construction/ConstructionStudy";

const features=[
  {img:"/media/construction-sundress.jpg",name:"Fit a sundress",action:"Change the form and compare two skirt cuts.",href:"/wardrobe/sundress/",pdf:"/sundress/pattern-A4.pdf",print:"Original sundress A4"},
  {img:"/media/skirt-atelier.jpg",name:"Compare two skirt cuts",action:"See how panel width changes the sweep on the same form.",href:"/studies/skirt/",pdf:"/atelier/flare.pdf",print:"Flared skirt A4"},
  {img:"/media/cards/inside-out.webp",name:"Turn a waistcoat inside out",action:"Reveal the lining, facings, and pocket bags in 3D.",href:"/wardrobe/waistcoat/inside-out/",pdf:"/exhibits/waistcoat/waistcoat-A4.pdf",print:"Waistcoat A4"},
  {img:"/media/cards/tiling.webp",name:"Assemble a pattern at home",action:"Arrange the actual A4 tiles and check the printed scale.",href:"/studies/tiling/"},
];
export function ExhibitionCover(){
  return <ConstructionStudy hero/>;
}
export function ExhibitionFeatures(){return <section className="ec-gallery" aria-label="Explore the collection">
  <div className="wrap">
    <div className="ec-heading"><h2>Try a garment study</h2><a href="/sections/wardrobe/">Browse the wardrobe</a></div>
    <div className="ec-features">{features.map(f=><article key={f.img}>
      <a href={f.href}><div className="ec-detail"><img src={f.img} loading="lazy" decoding="async" alt="" width="508" height="340"/></div>
        <div className="ec-caption"><h3>{f.name}</h3><span className="arrow" aria-hidden="true"/></div><p>{f.action}</p></a>
      {f.pdf&&<a className="ec-pattern" href={f.pdf} download>{f.print}</a>}
    </article>)}</div>
  </div></section>;}
