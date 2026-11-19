import "./plates.css";

const catalogue = "https://sfhm.se/wp-content/uploads/2025/04/SFHM_skriftserie_Svenska_armens_uniformer_del_1_kavalleriet.pdf";

export default function HussarReferences() {
  return <section className="regimental-plates" id="regimental-plates" aria-labelledby="plate-heading">
    <div className="plate-text"><p className="kicker">The museum plate</p>
      <h2 id="plate-heading">Reading the uniform.</h2>
      <p>The Blue Hussar wears yellow cord on dark blue wool. On the Yellow Hussar, white cord crosses a black dolman beneath a chamois-colored pelisse. Follow the scalloped edge just below the buckskin breeches on both figures.</p>
      <div className="period-figures">
        {[["blue","Blue Hussar","1761"],["yellow","Yellow Hussar","1765"]].map(([id,name,date]) =>
          <figure key={id}><a href={catalogue + "#page=22"}><img src={`/wardrobe/hussar/${id}-plate.jpg`}
            alt={name + " in A. Schützercrantz’s uniform plate"} height="260" loading="lazy"/></a>
            <figcaption>{name}<small>{date}</small></figcaption></figure>)}
      </div>
      <p className="plate-credit">A. Schützercrantz, nineteenth-century illustrations reproduced in <i>Svenska arméns uniformer, del 1: Kavalleriet</i>, p. 40.</p>
      <a href={catalogue + "#page=22"}>Examine the museum plate ↗</a>
    </div>
    <figure className="regimental-portrait"><a href="/wardrobe/hussar/regimental-portrait.webp">
      <img src="/wardrobe/hussar/regimental-portrait.webp" width="1536" height="1024" loading="lazy"
        alt="Interpretive portrait of two hussars in fitted braided jackets, long cloth leggings and fur-edged pelisses"/></a>
      <figcaption><strong>Material study</strong><span>AI-generated illustration based on the two plates. The faces, lighting and textile surfaces are imagined.</span></figcaption>
    </figure>
  </section>;
}
