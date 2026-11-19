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
export const initial = { height: 168, bust:samples[0].bust, waist:samples[0].waist, weight:52, left: 14, right: 19 };
export type Form = typeof initial;
export type Arm = "left" | "right";
export type Readings = Pick<Form,"height"|"weight"|"bust"|"waist"> & {hem:number};
export const round=(n:number)=>Math.round(n*10)/10;

// Upper seam-line profiles from the native cubic pattern. The shoulder, neckline and armhole
// coordinates are shared by the PDF and the wrapped illustration.
const tops=["bodice_front","bodice_back"].map(id=>{
  const v=vertices(id),piece=pattern.pieces.find(p=>p.id===id)!;
  const edges:readonly {id:string;from:string;to:string;curve?:{c:number[][]}}[]=piece.edges;
  const poly:number[][]=[];
  for(const edge of edges.filter(e=>["arm_R","strap_outer_R","shoulder_R","strap_inner_R","neck_R"].includes(e.id))){
    const a=v[edge.from],d=v[edge.to],dx=d[0]-a[0],dy=d[1]-a[1];
    if(poly.length===0)poly.push(a);
    if(edge.curve){
      const [b,c]=edge.curve.c.map(([t,h])=>[a[0]+dx*t-dy*h,a[1]+dy*t+dx*h]);
      for(let i=1;i<=42;i++){
        const t=i/42,s=1-t;
        poly.push([0,1].map(k=>s**3*a[k]+3*s*s*t*b[k]+3*s*t*t*c[k]+t**3*d[k]));
      }
    }else poly.push(d);
  }
  return {poly,halfWidth:v.ur[0]};
});
export function cutPosition(a:number){
  const isFront=Math.cos(a)>=0,edge=tops[isFront?0:1],x=Math.asin(Math.min(1,Math.abs(Math.sin(a))))/(Math.PI/2)*edge.halfWidth;
  return {x:cm(x)*Math.sign(Math.sin(a)),isFront};
}
export function neckline(a:number){
  const {x,isFront}=cutPosition(a),xx=Math.abs(x)*10,{poly}=tops[isFront?0:1];
  let height=235;
  for(let i=1;i<poly.length;i++){
    const p=poly[i-1],q=poly[i];if(xx<Math.min(p[0],q[0])-.0001||xx>Math.max(p[0],q[0])+.0001)continue;
    const t=Math.abs(q[0]-p[0])<1e-6?0:(xx-p[0])/(q[0]-p[0]);
    height=Math.max(height,p[1]+(q[1]-p[1])*t);
  }
  return cm(height);
}

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
