// One list of destinations for the header, the mobile menu, the footer and the sitemap.
export { repo } from "../../app/studies";
export const siteName = "FrankenTextiles";
export const siteOrigin = process.env.FTX_SITE_ORIGIN ?? "https://franken-textiles.com";

export interface SiteLink { href: string; label: string; note: string }

export const exhibits: SiteLink[] = [
  { href: "/#drape-room", label: "Drape room", note: "Sew five flat pieces onto a fitted body" },
  { href: "/#wardrobe", label: "Wardrobe", note: "A sundress and two hussar uniforms of 1761" },
  { href: "/#studio", label: "Studios", note: "Draft a pattern, lay it on the roll, follow the needle" },
  { href: "/#materials", label: "Materials", note: "Pull and twist cloth between two grips" },
  { href: "/#studies", label: "Drape gallery", note: "Follow panels from the board to the sewn garment" },
  { href: "/#tracing", label: "Tracing", note: "Recover cut lines from a drawn pattern sheet" },
  { href: "/#films", label: "Films", note: "Nine short films of the exhibits" },
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

// Header shows the first four exhibits; the rest live in the menu and the footer.
export const primary = exhibits.filter(item => ["Wardrobe", "Studios", "Materials", "Films"].includes(item.label));
