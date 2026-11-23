const patterns = {
  column: ["Column skirt", "/atelier/column.pdf"],
  flare: ["Flared skirt", "/atelier/flare.pdf"],
  bolero: ["Zouave bolero", "/patterns/bolero.pdf"],
  "bolero-full-hand": ["355 mm Zouave bolero", "/fittings/bolero-full-hand-A4.pdf"],
  trousers: ["Gathered trousers", "/patterns/trousers.pdf"],
  fez: ["Fez", "/patterns/fez.pdf"],
  dress: ["Garden sundress", "/sundress/pattern-A4.pdf"],
  eased: ["Lightly eased sundress", "/exhibits/sundress/light-A4.pdf"],
  blue: ["Blue Hussars", "/patterns/hussar-blue.pdf"],
  yellow: ["Yellow Hussars", "/patterns/hussar-yellow.pdf"],
} as const;

const exhibits: Record<string, (keyof typeof patterns)[]> = {
  "skirt-atelier":["column","flare"],
  "drape-room": ["bolero","trousers"],
  "trouser-posture":["trousers"], "trouser-elastic":["trousers"], "trouser-section":["trousers"],
  bolero: ["bolero"], construction: ["bolero"], trousers: ["trousers"], fez: ["fez"],
  sundress: ["dress"], "sundress-cuts": ["dress","eased"], hussar: ["blue", "yellow"],
  "wardrobe-sundress": ["dress"], "wardrobe-hussar": ["blue", "yellow"],
  "studio-tour": ["bolero"], "wardrobe-reel": ["dress", "blue", "yellow", "flare", "bolero"],
};

export default function GarmentPatterns({exhibit}: {exhibit: string}) {
  const cut=exhibit.replace(/^construction-/,"");
  const list = cut==="ensemble" ? ["bolero","trousers"] as const :
    exhibits[cut] ?? (Object.hasOwn(patterns,cut)?[cut as keyof typeof patterns]:undefined);
  if (!list) return null;
  return <div className="garment-patterns">
    {list.map(key => <a key={key} href={patterns[key][1]} download>
      {patterns[key][0]} · full-size A4 PDF <span aria-hidden="true">↓</span></a>)}
  </div>;
}
