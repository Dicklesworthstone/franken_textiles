import * as T from "three";
import { axes, dress, neckline } from "./form";
import { sprigPrint } from "../atelier3d/weave";

const tau = Math.PI*2;
type Sample = (angle: number, row: number) => number[];
const material = (color: string, roughness = .8) => new T.MeshStandardMaterial({ color, roughness, side: T.DoubleSide });

function skin(rows: number, slices: number, point: Sample) {
  const geometry = new T.BufferGeometry();
  const pos: number[] = [], uv: number[] = [], index: number[] = [];
  for (let v=0; v<rows; v++) for (let u=0; u<=slices; u++) {
    pos.push(...point(u*tau/slices,v));
    uv.push(u/slices, v/(rows-1));
    if (u<slices && v<rows-1) {
      const a=v*(slices+1)+u,b=a+1,c=b+slices+1,d=c-1;
      index.push(a,b,d,b,c,d);
    }
  }
  geometry.setAttribute("position",new T.Float32BufferAttribute(pos,3));
  geometry.setAttribute("uv",new T.Float32BufferAttribute(uv,2));
  geometry.setIndex(index); geometry.computeVertexNormals();
  return geometry;
}
function put(parent:T.Object3D, geometry:T.BufferGeometry, mat:T.Material, x=0,y=0,z=0) {
  const mesh=new T.Mesh(geometry,mat);
  mesh.position.set(x,y,z);mesh.castShadow=true;mesh.receiveShadow=true;parent.add(mesh);
  return mesh;
}
function oval(parent:T.Object3D,mat:T.Material,x:number,y:number,z:number,rx:number,ry:number,rz:number) {
  const mesh=put(parent,new T.SphereGeometry(1,36,24),mat,x,y,z);mesh.scale.set(rx,ry,rz);return mesh;
}
function line(parent:T.Object3D,points:number[][],width:number,mat:T.Material,closed=false) {
  const curve=new T.CatmullRomCurve3(points.map(p=>new T.Vector3(p[0],p[1],p[2])),closed);
  return put(parent,new T.TubeGeometry(curve,Math.max(24,points.length),width,7,closed),mat);
}
function bodiceRadius(y:number){
  if(y<9)return dress.waist+(84-dress.waist)*y/9;
  if(y<20)return 84+(dress.bust-84)*(y-9)/11;
  if(y<23)return dress.bust;
  return dress.bust-(y-23)*.7;
}
function bodicePoint(a:number,t:number) {
  const y=t*neckline(a);
  const [rx,rz]=axes(bodiceRadius(y));return [rx*Math.sin(a),y,rz*Math.cos(a)];
}
function skirtSection(a:number,t:number){
  const [rx,rz]=axes(dress.waist);
  const fold=(.3+.7*t)*(.8*Math.cos(18*a+.45*t)+.32*Math.cos(11*a-.8*t));
  return [(rx+18*t+4*t*t+(2.6*t+.28)*fold)*Math.sin(a),(rz+17*t+3*t*t+(2.6*t+.28)*fold)*Math.cos(a)];
}
const skirtScales=new Map<number,number>();
function skirtPoint(a:number,t:number){
  let scale=skirtScales.get(t);
  if(scale===undefined) {
    const circumference=dress.waist+(dress.gatheredEdge-dress.waist)*(1-Math.exp(-t/.115))+(dress.hem-dress.gatheredEdge)*t;
    let perimeter=0,previous=skirtSection(0,t);
    for(let i=1;i<=144;i++){const p=skirtSection(tau*i/144,t);perimeter+=Math.hypot(p[0]-previous[0],p[1]-previous[1]);previous=p;}
    scale=circumference/perimeter;skirtScales.set(t,scale);
  }
  const [x,z]=skirtSection(a,t);return [x*scale,-dress.skirtLength*t+.65*t**2*Math.sin(11*a+.5),z*scale];
}
export function buildDress() {
  // The same print the solver-draped dress wears.
  const g=new T.Group(),base=sprigPrint(), bodiceMap=base.clone(),skirtMap=base.clone();
  bodiceMap.repeat.set(2.35,1.0);skirtMap.repeat.set(3.6,1.85);
  const cloth=(map:T.Texture)=>new T.MeshStandardMaterial({map,roughness:.94,side:T.DoubleSide});
  const bodice=cloth(bodiceMap),skirt=cloth(skirtMap),piping=material("#c8a671"),band=material("#145351");
  base.dispose();
  const top=put(g,skin(40,256,(a,i)=>bodicePoint(a-Math.PI/2,i/39)),bodice);
  top.userData.sideSeam=256;
  const uv=top.geometry.getAttribute("uv"),coordinates=top.geometry.getAttribute("position");
  for(let i=0;i<uv.count;i++)uv.setY(i,coordinates.getY(i)/30);
  put(g,skin(47,144,(a,i)=>skirtPoint(a-Math.PI/2,1-i/46)),skirt).userData.sideSeam=144;
  const angles=Array.from({length:256},(_,i)=>i*tau/256);
  line(g,angles.map(a=>bodicePoint(a,1)),.15,piping,true).userData.binding=true;
  line(g,angles.map(a=>skirtPoint(a,.965)),.19,piping,true).userData.hem=true;
  line(g,angles.map(a=>skirtPoint(a,.981)),.11,piping,true).userData.hem=true;
  const waist=skin(3,96,(u,i)=>{const a=u-Math.PI/2,[rx,rz]=axes(dress.waist+.5+i*.8);return [(rx+.25)*Math.sin(a),i*.95,(rz+.25)*Math.cos(a)];});put(g,waist,band).userData.sideSeam=96;
  for(let i=0;i<6;i++){
    const y=3+i*4.3,[,rz]=axes(bodiceRadius(y));
    oval(g,piping,0,y,rz+.15,.46,.45,.22);
  }
  return g;
}
