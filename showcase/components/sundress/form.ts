import pattern from "../../public/sundress/pattern.json";
import native from "../../public/sundress/fit-comparison.json";

// Convert the checked native millimetre pattern and validation report into scene centimetres.
const cm = (mm:number) => mm/10;
type Report = typeof native.runs[number];
function point(run:Report,id:"bust"|"waist"){
  const found=run.points.find(p=>p.id===id);
  if(!found)throw Error("Sundress validation is missing the "+id+" section.");
  return found;
}
function vertices(id:string):Record<string,number[]> {
  const p=pattern.pieces.find(p=>p.id===id);
  if(!p)throw Error("Sundress pattern is missing "+id);
  return Object.fromEntries(Object.entries(p.vertices).filter((entry):entry is [string,number[]]=>Array.isArray(entry[1])));
}
const front=vertices("bodice_front"),skirt=vertices("skirt_front"),back=vertices("skirt_back");
const width=(v:Record<string,number[]>,l:string,r:string)=>Math.abs(v[r][0]-v[l][0]);
const bustCheck=point(native.runs[0],"bust"),waistCheck=point(native.runs[0],"waist");
export const dress = { bust:cm(bustCheck.finished_mm),waist:cm(waistCheck.finished_mm),
  bustBand:bustCheck.target_mm.map(cm),waistBand:waistCheck.target_mm.map(cm),
  gatheredEdge:cm(width(skirt,"wl","wr")+width(back,"wl","wr")),hem:cm(width(skirt,"hl","hr")+width(back,"hl","hr")),
  shoulderToWaist:cm(front.rt[1]-front.wr[1]),skirtLength:cm(skirt.wr[1]-skirt.hr[1]) };
export const samples=native.runs.map(run=>({name:run.form,bust:cm(point(run,"bust").body_mm),waist:cm(point(run,"waist").body_mm)}));
export const initial = { height: 168, bust:samples[0].bust, waist:samples[0].waist, left: 14, right: 19 };
export type Form = typeof initial;
export type Arm = "left" | "right";
export const shoulderLine = (height: number) => height * .85;
export const hemLine = (height: number) => shoulderLine(height) - dress.shoulderToWaist - dress.skirtLength;

// Scale a polygonal ellipse to its specified girth. The same section is used for the model and dress.
const unitSections = new Map<number, number>();
export function axes(girth: number, ratio = .71) {
  let perimeter = unitSections.get(ratio);
  if (perimeter === undefined) {
    perimeter=0;
    for (let i = 0; i < 96; i++) {
      const t = i*2*Math.PI/96, u = (i+1)*2*Math.PI/96;
      perimeter += Math.hypot(Math.sin(t)-Math.sin(u), ratio*(Math.cos(t)-Math.cos(u)));
    }
    unitSections.set(ratio,perimeter);
  }
  return [girth/perimeter, ratio*girth/perimeter] as const;
}
