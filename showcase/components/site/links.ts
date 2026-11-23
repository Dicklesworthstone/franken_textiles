// One list of destinations for the header, the mobile menu, the footer and the sitemap.
export { repo } from "../../app/studies";
export const siteName = "FrankenTextiles";
export const siteOrigin = process.env.FTX_SITE_ORIGIN ?? "https://franken-textiles.com";

// `thumb` names the entry's still under /media/index/ (made by scripts/render-exhibit-cards.mjs).
export interface SiteLink { href: string; label: string; note: string; thumb?: string }

export const exhibits: SiteLink[] = [
  { href: "/studies/skirt/", thumb: "skirt", label: "Skirt atelier", note: "Compare two hems on fitted adult forms" },
  { href: "/studies/wrap-skirt/", thumb: "wrap-skirt", label: "Wrap skirt overlap", note: "Check front overlap, tie reach, and neutral vs seated Anny slices" },
  { href: "/#drape-room", thumb: "drape-room", label: "Drape room", note: "Sew five flat pieces onto a fitted body" },
  { href: "/studies/trousers/", thumb: "trousers", label: "Trouser study", note: "Bend knees, test elastic grip, and slice through sewn folds" },
  { href: "/#visual-fitting", thumb: "visual-fitting", label: "Visual fitting", note: "Trace rendered pixels back to flat pattern edges" },
  { href: "/#wardrobe", thumb: "wardrobe", label: "Wardrobe", note: "A sundress and Swedish hussars of 1761 and 1765" },
  { href: "/wardrobe/sundress/redraft/", thumb: "redraft", label: "Sundress ease redraft", note: "Redraft the sundress after a bust or waist ease warning and pin both outlines" },
  { href: "/wardrobe/sundress/compare/", thumb: "sundress-compare", label: "Compare two dress drapes", note: "Compare two independently accepted native drapes of one fixed sundress under one camera and scale" },
  { href: "/wardrobe/hussar/embroidery/", thumb: "embroidery", label: "Hussar embroidery trace", note: "Trace registered Tajima DST stitches onto the solved 3D dolman chest" },
  { href: "/wardrobe/waistcoat/", thumb: "waistcoat", label: "1780s waistcoat", note: "Separate silk fronts, linen lining, and welt pockets from V&A 256-1880" },
  { href: "/wardrobe/waistcoat/inside-out/", thumb: "inside-out", label: "Lined garment inside out", note: "Rotate shell, facing, and lining together with invariant native seam indices" },
  { href: "/#studio", thumb: "studios", label: "Studios", note: "Draft a pattern, lay it on the roll, follow the needle, or repair a seam" },
  { href: "/studies/tiling/", thumb: "tiling", label: "A4 tile assembly", note: "Assemble a real tiled A4 PDF on a virtual floor with alignment diamonds" },
  { href: "/studies/sewing/", thumb: "sewing", label: "Jacket sewing order", note: "Play all ten native sewing operations across the flat cut and 3D bolero" },
  { href: "/#materials", thumb: "materials", label: "Materials", note: "Pull and twist cloth between two grips" },
  { href: "/experiments/thermal/", thumb: "thermal", label: "Thermal sandwich", note: "Stack measured neoprene & leather sheets with live steady conduction" },
  { href: "/experiments/wash/", thumb: "wash", label: "Wash & shrink", note: "Apply measured warp/weft denim shrinkage or compute a pre-wash recut" },
  { href: "/experiments/friction/", thumb: "friction", label: "Friction sled", note: "Inspect retained normal & tangential reactions across the stick-to-slip threshold" },
  { href: "/#studies", thumb: "drape-gallery", label: "Drape gallery", note: "Follow panels from the board to the sewn garment" },
  { href: "/#tracing", thumb: "tracing", label: "Tracing", note: "Recover cut lines from a drawn pattern sheet" },
  { href: "/#films", thumb: "films", label: "Films", note: "Watch the garments, material rigs and workshops" },
];

// The workbenches are plain HTML files staged into public/workbench/. Cloudflare serves them without
// the extension and redirects the .html form; `next dev` only knows the file names.
const published = process.env.NODE_ENV === "production";
export const workbench = (page: "index" | "trace" | "materials" | "photo") =>
  published ? (page === "index" ? "/workbench/" : `/workbench/${page}`) : `/workbench/${page}.html`;

export const workbenches: SiteLink[] = [
  { href: workbench("index"), label: "Pattern drafting", note: "Draft to measurements, walk the seams, print at full scale" },
  { href: workbench("trace"), label: "Pattern tracing", note: "Turn a scanned sheet into checked pieces" },
  { href: workbench("materials"), label: "Material studies", note: "Bind cloth to a pattern and compare alternatives" },
  { href: workbench("photo"), label: "Photo measurements", note: "Measure from a photo with a sheet of paper in frame" },
];

export const project: SiteLink[] = [
  { href: "/how-it-works/", label: "How it works", note: "From a tape measure to cut cloth, and what each result does and does not claim" },
  { href: "/get-started/", label: "Get started", note: "Open a workbench or build the command-line tool" },
];

// The named sections of the home page, in reading order: `id` is the section's anchor there. The page rail
// (wide pointer screens) and the menu's "On this page" list (everywhere else) are both drawn from this.
export const homeSections = [
  ["construction", "From cut to cloth"],
  ["drape-room", "Drape room"],
  ["visual-fitting", "Visual fitting"],
  ["how", "How a pattern gets made"],
  ["wardrobe", "Wardrobe"],
  ["studio", "Studios"],
  ["materials", "Materials"],
  ["studies", "Drape gallery"],
  ["tracing", "Tracing"],
  ["capabilities", "Capabilities"],
  ["films", "Films"],
  ["files", "Files"],
  ["workbenches", "Workbenches"],
  ["claims", "Four claims"],
] as const;

// Header shows the five core destinations; the full exhibition index lives in the menu and the footer.
export const primary = exhibits.filter(item => ["Drape room", "Wardrobe", "Studios", "Materials", "Films"].includes(item.label));
