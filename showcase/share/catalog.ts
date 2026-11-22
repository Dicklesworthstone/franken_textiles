export const sections = ["wardrobe","studio","draft","cut","stitch","repair","materials","studies","tracing","films","tools"] as const;
export type Section = (typeof sections)[number];
export const isSection = (value: string): value is Section => sections.some(section => section === value);

export interface Plate {
  title: [string, string];
  caption: string;
  category: string;
  specimen: string;
  colors: [paper: string, ink: string, accent: string];
  images: string[];
  layout?: "cover" | "pair" | "contact";
}
const paper = ["#f0ede0","#253c36","#a74b2e"] as Plate["colors"];
const midnight = ["#172930","#f5ead5","#ecaa78"] as Plate["colors"];
const oxblood = ["#43262e","#faebd9","#efb896"] as Plate["colors"];

export const plates = {
  home: {title:["From cut","to cloth."],caption:"Follow the pieces from the cutting table to the sewn garment. Print the same cut at full size.",category:"The pattern room",specimen:"Six gores on a fitted 68 cm form",colors:["#f3ecde","#342a24","#974531"],images:["flare-sewn","flare-cut","flare-join"],layout:"contact"},
  skirtStudy: {title:["The skirt","atelier."],caption:"See where the bound waist sits on the body, then print the cut you see.",category:"A six-panel study",specimen:"65–80 cm waists · two hem sweeps",colors:["#e8e2cf","#2a392e","#97553e"],images:["skirt-column","skirt-flare"],layout:"pair"},
  wrapSkirt: {title:["A wrap skirt","with ties."],caption:"Compare two tie settings on standing and seated Anny slices. The pattern stays fixed and is available as a full-scale PDF.",category:"Wrap geometry",specimen:"Pattern pieces · standing and seated Anny slices",colors:["#e8e2cf","#2a392e","#97553e"],images:["wrap-skirt"]},
  trouserStudy: {title:["Room to","move."],caption:"Bend the knees, change the elastic and take a section through seven sewn panels.",category:"The trouser study",specimen:"A fitted Anny form · elastic casings",colors:["#edf0df","#263c35","#976039"],images:["trouser-form"]},
  tiling: {title:["Assemble an","A4 pattern."],caption:"Inspect a real tiled A4 export on a virtual floor: alignment diamonds, 5 mm bleed overlap, 100 mm scale bar and downloadable PDF.",category:"Home printing",specimen:"A4 tiled PDF · 190 × 277 mm frames",colors:paper,images:["draft"]},
  sewingOrder: {title:["Play the","sewing order."],caption:"Step through all ten accepted operations of the Zouave bolero on flat pieces and the 3D jacket, with prerequisites, endpoints and stitch files.",category:"Sewing planner",specimen:"10 operations · 734 directed stitches",colors:paper,images:["construction"]},
  wardrobe: {title:["The","wardrobe."],caption:"Explore a garden sundress and two Swedish regimental uniforms in three dimensions.",category:"Garment collection",specimen:"Sundress & hussar dolman",colors:oxblood,images:["dress","hussar"],layout:"pair"},
  sundress: {title:["The garden","sundress."],caption:"Change the skirt waist and follow four pieces onto the same fitted form.",category:"The fitting room",specimen:"Two cuts · one bodice",colors:["#dce5d3","#183e37","#93532e"],images:["eased-dress","eased-cut","eased-join"],layout:"contact"},
  sundressRedraft: {title:["Redraft the","sundress."],caption:"After a flat bust or waist ease warning, request a second checked pattern, pin both silhouettes at 1:1 scale, and compare every recalculated seam.",category:"The fitting room",specimen:"Native alteration · pinned silhouettes",colors:["#dce5d3","#183e37","#93532e"],images:["eased-cut"]},
  hussar: {title:["The hussar’s","cabinet."],caption:"Follow the cord from the flat jacket front onto the sewn dolman.",category:"Historical wardrobe",specimen:"Swedish cavalry · 1761 and 1765",colors:["#1c3036","#f4ecda","#e1b55f"],images:["hussar","blue-cut","blue-join"],layout:"contact"},
  hussarEmbroidery: {title:["Trace the","embroidery."],caption:"Register real Tajima DST placement and soutache couching stitches directly onto the solved 3D hussar dolman chest.",category:"Historical wardrobe",specimen:"Tajima DST · 2,172 registered chest stitches",colors:["#1c3036","#f4ecda","#e1b55f"],images:["hussar"]},
  studio: {title:["The","worktable."],caption:"Draft the pieces, lay them out on the roll, and follow the embroidery needle.",category:"Hands-on studios",specimen:"Drafting · cutting · embroidery",colors:paper,images:["draft","cut","stitch"],layout:"contact"},
  draft: {title:["Draft the","pattern."],caption:"Change the size and ease of a measured draft, then compare the cut outlines.",category:"Pattern drafting",specimen:"Zouave bolero · full-scale draft",colors:paper,images:["draft"]},
  cut: {title:["On the","cutting table."],caption:"Arrange real pattern pieces on a roll and download the full-scale cutting marker.",category:"Cutting studio",specimen:"Roll layout · grain alignment",colors:["#dce4cc","#293d35","#82502f"],images:["cut"]},
  stitch: {title:["Follow the","needle."],caption:"Replay a Tajima stitch path through the stop where the maker lays the braid.",category:"Machine embroidery",specimen:"Tajima DST · cord placement",colors:["#202b47","#fff0d7","#ffc17b"],images:["stitch"]},
  repair: {title:["Repair a","seam."],caption:"Walk paired front and back sewing lines, catch a flat mismatch, and true the seam in local Rust WebAssembly.",category:"Pattern alteration",specimen:"Zouave bolero · true-seam repair",colors:paper,images:["draft"]},
  materials: {title:["Cloth under","load."],caption:"Explore how a woven specimen responds when the grips pull, twist and compress it.",category:"Material experiments",specimen:"Tensile grips & torsion rig",colors:midnight,images:["strip","cylinder"],layout:"pair"},
  cylinder: {title:["The torsion","rig."],caption:"Twist and compress a cloth cylinder while inspecting the native force and torque readings.",category:"Material experiment",specimen:"Cloth cylinder · prescribed load cycle",colors:["#3c322d","#f8efdf","#f3b986"],images:["cylinder"]},
  strip: {title:["Cut on","the bias."],caption:"Pull warp, weft and bias specimens of the same cloth and compare the measured grip reactions in the simulation.",category:"Material experiment",specimen:"Warp · weft · bias",colors:["#dce5dc","#203c3f","#93552e"],images:["strip"]},
  thermal: {title:["Thermal","sandwich."],caption:"Stack measured neoprene and leather sheets at explicit interface resistance and compute steady 1D heat flux in Rust WebAssembly.",category:"Material experiment",specimen:"Measured sheets · Fourier conduction",colors:midnight,images:["material-detail"]},
  wash: {title:["Wash, shrink","& recut."],caption:"Apply peer-reviewed ISO 6330 denim wash shrinkage along each piece's grainline or compute an oversized pre-wash compensation cut.",category:"Material experiment",specimen:"Kanat & Ozdil 2026 · warp/weft shrinkage",colors:paper,images:["draft"]},
  friction: {title:["The friction","sled."],caption:"Follow the recorded sled path from sticking to sliding. Compare two synthetic surfaces and rerun calibration in Rust WebAssembly.",category:"Material experiment",specimen:"Recorded sled path · force curves at breakaway",colors:midnight,images:["friction-sled"]},
  studies: {title:["The drape","gallery."],caption:"Follow five flat panels onto a fitted body and compare the forces as its arms rise.",category:"Cloth in three dimensions",specimen:"Zouave bolero · body contact and edge strain",colors:["#213d38","#f6e9cf","#e2b988"],images:["pose","load"],layout:"pair"},
  tracing: {title:["Read","the lines."],caption:"Reveal the piece outlines, grain lines and corners recovered from a practice pattern sheet.",category:"Raster tracing",specimen:"Bolero sheet · native contour review",colors:paper,images:["trace"]},
  films: {title:["The screening","room."],caption:"Watch the pieces become garments, then explore the material rigs and workshops.",category:"Exhibition films",specimen:"Frames from the exhibition",colors:["#191d1d","#f2e8da","#e4a578"],images:["hussar","pose","trouser-form"],layout:"contact"},
  tools: {title:["The local","workbenches."],caption:"Draft, bind cloth, trace a sheet and measure a sleeve with the Rust engine in your browser.",category:"Browser workbenches",specimen:"Pattern pieces & a Photo Workbench sample",colors:paper,images:["draft","sleeve"],layout:"pair"},
  patternTool: {title:["The pattern","workbench."],caption:"Open a pattern, validate its pieces and seams, and inspect the manufacturing files.",category:"Browser workbench",specimen:"Native pattern document · millimetres",colors:midnight,images:["construction"]},
  materialTool: {title:["The material","workbench."],caption:"Inspect material records and run fabric experiments in your browser.",category:"Browser workbench",specimen:"Fabric specimen · clamped strip",colors:["#2a423f","#f3eddb","#c5d8a5"],images:["material-detail"]},
  traceTool: {title:["Trace a","pattern."],caption:"Review recovered pieces and corners from a raster pattern sheet on your own device.",category:"Browser workbench",specimen:"Source sheet & recovered edge geometry",colors:["#e6dfce","#343932","#924634"],images:["source","trace"],layout:"pair"},
  photoTool: {title:["Photo","measurements."],caption:"Measure two chalk marks across a sleeve using an A4 sheet to recover scale.",category:"Browser workbench",specimen:"Indigo sleeve · A4 reference",colors:["#dce6df","#253b35","#944c35"],images:["sleeve"]},
  howItWorks: {title:["From tape","to cloth."],caption:"Follow a bolero from its flat cut to a raised-arm drape. See how moving its starting panels 30 mm cleared an intersecting sleeve.",category:"How it works",specimen:"Raised-arm bolero · two fitting rounds",colors:paper,images:["bolero-settled","bolero-refused","bolero-back-cut"],layout:"contact"},
  getStarted: {title:["Get","started."],caption:"Open a workbench in your browser, or build the command-line tool and draft a first pattern.",category:"Get started",specimen:"Four workbenches · one command-line tool",colors:midnight,images:["cut"]},
} satisfies Record<string, Plate>;
export type PlateId = keyof typeof plates;
