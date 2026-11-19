export const sections = ["wardrobe","studio","draft","cut","stitch","materials","studies","tracing","films","tools"] as const;
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
  home: {title:["The shape","of cloth."],caption:"From a flat pattern to the folds a garment makes under gravity.",category:"The pattern room",specimen:"Parametric forms · cloth physics · printable patterns",colors:["#f3ecde","#342a24","#974531"],images:["skirt-editorial"]},
  skirtStudy: {title:["The skirt","atelier."],caption:"Compare the column and the flare on a fitted adult form, then print the cut you see.",category:"A six-panel study",specimen:"78 cm waist · two hem sweeps",colors:["#e8e2cf","#2a392e","#97553e"],images:["skirt-cloth"]},
  trouserStudy: {title:["Room to","move."],caption:"Bend the knees, change the elastic and take a section through seven sewn panels.",category:"The trouser study",specimen:"A fitted Anny form · elastic casings",colors:["#edf0df","#263c35","#976039"],images:["trouser-form"]},
  wardrobe: {title:["The","wardrobe."],caption:"Explore a garden sundress and two Swedish regimental uniforms in three dimensions.",category:"Garment collection",specimen:"Sundress & hussar dolman",colors:oxblood,images:["dress","hussar"],layout:"pair"},
  sundress: {title:["The garden","sundress."],caption:"Pose the adult form and see how much ease a fixed gathered dress leaves.",category:"The fitting room",specimen:"Floral bodice and gathered skirt",colors:["#dce5d3","#183e37","#93532e"],images:["dress"]},
  hussar: {title:["The hussar’s","cabinet."],caption:"Examine the braided dolman and fur-trimmed pelisse of two Swedish regiments.",category:"Historical wardrobe",specimen:"Swedish cavalry uniforms · 1761",colors:["#1c3036","#f4ecda","#e1b55f"],images:["hussar"]},
  studio: {title:["The","worktable."],caption:"Draft the pieces, lay them out on the roll, and follow the embroidery needle.",category:"Hands-on studios",specimen:"Drafting · cutting · embroidery",colors:paper,images:["draft","cut","stitch"],layout:"contact"},
  draft: {title:["Draft the","pattern."],caption:"Change the size and ease of a measured draft, then compare the cut outlines.",category:"Pattern drafting",specimen:"Zouave bolero · full-scale draft",colors:paper,images:["draft"]},
  cut: {title:["On the","cutting table."],caption:"Arrange real pattern pieces on a roll and download the full-scale cutting marker.",category:"Cutting studio",specimen:"Roll layout · grain alignment",colors:["#dce4cc","#293d35","#82502f"],images:["cut"]},
  stitch: {title:["Follow the","needle."],caption:"Replay a Tajima stitch path through the stop where the maker lays the braid.",category:"Machine embroidery",specimen:"Tajima DST · cord placement",colors:["#202b47","#fff0d7","#ffc17b"],images:["stitch"]},
  materials: {title:["Cloth under","load."],caption:"Explore how a woven specimen responds when the grips pull, twist and compress it.",category:"Material experiments",specimen:"Tensile grips & torsion rig",colors:midnight,images:["strip","cylinder"],layout:"pair"},
  cylinder: {title:["The torsion","rig."],caption:"Twist and compress a cloth cylinder while inspecting the native force and torque readings.",category:"Material experiment",specimen:"Cloth cylinder · prescribed load cycle",colors:["#3c322d","#f8efdf","#f3b986"],images:["cylinder"]},
  strip: {title:["Cut on","the bias."],caption:"Pull warp, weft and bias specimens of the same cloth and compare the measured grip reactions in the simulation.",category:"Material experiment",specimen:"Warp · weft · bias",colors:["#dce5dc","#203c3f","#93552e"],images:["strip"]},
  studies: {title:["The drape","gallery."],caption:"Follow five flat panels onto a fitted body and compare the forces as its arms rise.",category:"Cloth in three dimensions",specimen:"Zouave bolero · body contact and edge strain",colors:["#213d38","#f6e9cf","#e2b988"],images:["pose","load"],layout:"pair"},
  tracing: {title:["Read","the lines."],caption:"Reveal the piece outlines, grain lines and corners recovered from a practice pattern sheet.",category:"Raster tracing",specimen:"Bolero sheet · native contour review",colors:paper,images:["trace"]},
  films: {title:["The screening","room."],caption:"Watch the garments, material rigs and workshops in fourteen short films.",category:"Exhibition films",specimen:"Frames from the exhibition",colors:["#191d1d","#f2e8da","#e4a578"],images:["hussar","pose","trouser-form"],layout:"contact"},
  tools: {title:["The local","workbenches."],caption:"Draft, bind cloth, trace a sheet and measure a sleeve with the Rust engine in your browser.",category:"Browser workbenches",specimen:"Pattern pieces & a Photo Workbench sample",colors:paper,images:["draft","sleeve"],layout:"pair"},
  patternTool: {title:["The pattern","workbench."],caption:"Open a pattern, validate its pieces and seams, and inspect the manufacturing files.",category:"Browser workbench",specimen:"Native pattern document · millimetres",colors:midnight,images:["construction"]},
  materialTool: {title:["The material","workbench."],caption:"Inspect material records and run fabric experiments in your browser.",category:"Browser workbench",specimen:"Fabric specimen · clamped strip",colors:["#2a423f","#f3eddb","#c5d8a5"],images:["material-detail"]},
  traceTool: {title:["Trace a","pattern."],caption:"Review recovered pieces and corners from a raster pattern sheet on your own device.",category:"Browser workbench",specimen:"Source sheet & recovered edge geometry",colors:["#e6dfce","#343932","#924634"],images:["source","trace"],layout:"pair"},
  photoTool: {title:["Photo","measurements."],caption:"Measure two chalk marks across a sleeve using an A4 sheet to recover scale.",category:"Browser workbench",specimen:"Indigo sleeve · A4 reference",colors:["#dce6df","#253b35","#944c35"],images:["sleeve"]},
  howItWorks: {title:["From tape","to cloth."],caption:"How measurements become a checked sewing pattern, a simulated garment and the files to make it.",category:"How it works",specimen:"Measure · fit · draft · drape · cut",colors:paper,images:["draft","pose"],layout:"pair"},
  getStarted: {title:["Get","started."],caption:"Open a workbench in your browser, or build the command-line tool and draft a first pattern.",category:"Get started",specimen:"Four workbenches · one command-line tool",colors:midnight,images:["cut"]},
} satisfies Record<string, Plate>;
export type PlateId = keyof typeof plates;
