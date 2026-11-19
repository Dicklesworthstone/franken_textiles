const patterns = {
  bolero: ["Zouave bolero", "/patterns/bolero.pdf"],
  trousers: ["Gathered trousers", "/patterns/trousers.pdf"],
  fez: ["Fez", "/patterns/fez.pdf"],
  dress: ["Garden sundress", "/sundress/pattern-A4.pdf"],
  blue: ["Blue Hussars", "/patterns/hussar-blue.pdf"],
  yellow: ["Yellow Hussars", "/patterns/hussar-yellow.pdf"],
} as const;

const exhibits: Record<string, (keyof typeof patterns)[]> = {
  bolero: ["bolero"], construction: ["bolero"], trousers: ["trousers"], fez: ["fez"],
  sundress: ["dress"], hussar: ["blue", "yellow"],
  "wardrobe-sundress": ["dress"], "wardrobe-hussar": ["blue", "yellow"],
  "studio-tour": ["bolero"], "wardrobe-reel": ["dress", "blue", "yellow", "bolero"],
};

export default function GarmentPatterns({exhibit}: {exhibit: string}) {
  const list = exhibits[exhibit];
  if (!list) return null;
  return <div className="garment-patterns">
    {list.map(key => <a key={key} href={patterns[key][1]} download>
      {patterns[key][0]} · full-size A4 PDF <span aria-hidden="true">↓</span></a>)}
  </div>;
}
