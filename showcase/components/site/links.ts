// One list of destinations for the header, the mobile menu, the footer and the sitemap.
export { repo } from "../../app/studies";
export const siteName = "FrankenTextiles";
export const siteOrigin = process.env.FTX_SITE_ORIGIN ?? "https://franken-textiles.com";

export interface SiteLink { href: string; label: string; note: string }

export const exhibits: SiteLink[] = [
  { href: "/studies/skirt/", label: "Skirt atelier", note: "Compare two hems on fitted adult forms" },
  { href: "/#drape-room", label: "Drape room", note: "Sew five flat pieces onto a fitted body" },
  { href: "/studies/trousers/", label: "Trouser study", note: "Bend knees, test elastic grip, and slice through sewn folds" },
  { href: "/#visual-fitting", label: "Visual fitting", note: "Trace rendered pixels back to flat pattern edges" },
  { href: "/#wardrobe", label: "Wardrobe", note: "A sundress and Swedish hussars of 1761 and 1765" },
  { href: "/#studio", label: "Studios", note: "Draft a pattern, lay it on the roll, follow the needle, or repair a seam" },
  { href: "/#materials", label: "Materials", note: "Pull and twist cloth between two grips" },
  { href: "/experiments/thermal/", label: "Thermal sandwich", note: "Stack measured neoprene & leather sheets with live steady conduction" },
  { href: "/experiments/wash/", label: "Wash & shrink", note: "Apply measured warp/weft denim shrinkage or compute a pre-wash recut" },
  { href: "/#studies", label: "Drape gallery", note: "Follow panels from the board to the sewn garment" },
  { href: "/#tracing", label: "Tracing", note: "Recover cut lines from a drawn pattern sheet" },
  { href: "/#films", label: "Films", note: "Watch the garments, material rigs and workshops" },
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

// Header shows the five core destinations; the full exhibition index lives in the menu and the footer.
export const primary = exhibits.filter(item => ["Drape room", "Wardrobe", "Studios", "Materials", "Films"].includes(item.label));
