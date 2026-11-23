export const studies = [
  {
    id: "bolero", garment: "bolero", group: "Garments", label: "Drape study",
    title: "The Zouave bolero", short: "Zouave bolero", figure: "01",
    description: "Follow five cut panels as the sleeves roll into place and the cropped jacket settles.",
    detail: "5 panels · 10 seams", mode: "cloth", card: "construction-bolero",
  },
  {
    id: "trousers", garment: "trousers", group: "Garments", label: "Drape study",
    title: "The gathered trouser", short: "Gathered trouser", figure: "02",
    description: "Watch the full legs gather between an elastic waistband and a pair of narrow cuffs.",
    detail: "7 panels · 17 seams", mode: "cloth", card: "construction-trousers",
  },
  {
    id: "construction", garment: "bolero", group: "Construction", label: "Construction study",
    title: "Anatomy of a seam", short: "Seam anatomy", figure: "03",
    description: "Trace the bolero’s stitch lines, inspect its triangulation, and reveal the final edge strain.",
    detail: "Panel map · Edge strain", mode: "construction", card: "construction-panels",
  },
] as const;
// `card` is the film on a study's card in the drape gallery: the same recorded drape as the study, in the
// studio look of the construction films. The seam study's film shows each of the bolero's pieces in its own
// colour (scripts/render-native-film.py, garment "panels").

export type Study = (typeof studies)[number];
export type View = "cloth" | "construction" | "strain";
export const repo = "https://github.com/Dicklesworthstone/franken_textiles";
