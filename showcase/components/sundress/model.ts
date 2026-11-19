import * as T from "three";
import { axes, dress, initial, shoulderLine, type Form, type Arm } from "./form";

const tau = Math.PI*2;
type Ring = [y: number, rx: number, rz: number, za?: number];
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
function loft(rings: Ring[], sides=48) {
  return skin(rings.length,sides,(a,i)=>[rings[i][1]*Math.sin(a),rings[i][0],rings[i][2]*Math.cos(a)+(rings[i][3]||0)]);
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
function print() {
  const canvas=document.createElement("canvas"); canvas.width=canvas.height=512;
  const c=canvas.getContext("2d")!;
  c.fillStyle="#0e6766";c.fillRect(0,0,512,512);
  for(let i=0;i<512;i+=3){
    c.fillStyle=i%2?"rgba(244,250,213,.10)":"rgba(22,43,55,.10)";c.fillRect(i,0,1,512);
    c.fillStyle="rgba(243,240,211,.07)";c.fillRect(0,i,512,1);
  }
  function leaf(x:number,y:number,angle:number,length:number,color:string) {
    c.save();c.translate(x,y);c.rotate(angle);c.fillStyle=color;c.beginPath();c.ellipse(0,-length*.5,length*.20,length*.57,0,0,tau);c.fill();c.restore();
  }
  // Tile the same sprig across each margin so the fabric repeats without a hard edge.
  for(const [x,y,s,angle] of [[49,29,.78,.48],[178,174,.85,-.55],[375,94,.95,.18],[245,384,.83,.46],[477,349,.72,-.73],[50,481,.65,-.3]]){
    for(const dx of [-512,0,512])for(const dy of [-512,0,512]){
      c.save();c.translate(x+dx,y+dy);c.rotate(angle);c.scale(s*1.43,s*1.43);
      c.strokeStyle="#cfbc81";c.lineWidth=2;c.beginPath();c.moveTo(0,36);c.quadraticCurveTo(-10,8,0,-21);c.stroke();
      leaf(-3,21,-.88,19,"#a4b785");leaf(-3,8,.75,17,"#9dba97");leaf(-4,1,-.73,15,"#b0c394");
      for(let p=0;p<6;p++)leaf(0,-21,p*tau/6,13,p%2?"#f8c49c":"#fff0c5");
      c.strokeStyle="#b4b08a";c.lineWidth=1.65;c.beginPath();c.moveTo(-4,11);c.quadraticCurveTo(5,1,13,-2);c.stroke();
      for(let p=0;p<5;p++)leaf(14,-3,p*tau/5+.7,6.8,"#e79271");
      c.fillStyle="#cf714c";c.beginPath();c.arc(0,-21,3.6,0,tau);c.fill();c.restore();
    }
  }
  const map=new T.CanvasTexture(canvas);map.colorSpace=T.SRGBColorSpace;map.wrapS=map.wrapT=T.RepeatWrapping;map.anisotropy=4;
  return map;
}
function bodiceRadius(y:number){
  if(y<9)return dress.waist+(84-dress.waist)*y/9;
  if(y<20)return 84+(dress.bust-84)*(y-9)/11;
  if(y<23)return dress.bust;
  return dress.bust-(y-23)*.7;
}
function bodicePoint(a:number,t:number) {
  const y=t*(23.5+4.6*Math.cos(a)**2+1.9*Math.sin(2*a)**2);
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
function buildDress() {
  const g=new T.Group(),base=print(), bodiceMap=base.clone(),skirtMap=base.clone();
  bodiceMap.repeat.set(2.95,1.15);skirtMap.repeat.set(4.6,2.0);
  const cloth=(map:T.Texture)=>new T.MeshStandardMaterial({map,roughness:.94,side:T.DoubleSide});
  const bodice=cloth(bodiceMap),skirt=cloth(skirtMap),piping=material("#c8a671"),band=material("#145351");
  base.dispose();
  put(g,skin(37,96,(a,i)=>bodicePoint(a,i/36)),bodice);
  put(g,skin(47,144,(a,i)=>skirtPoint(a,1-i/46)),skirt);
  const angles=Array.from({length:120},(_,i)=>i*tau/120);
  line(g,angles.map(a=>bodicePoint(a,1)),.2,piping,true);
  line(g,angles.map(a=>skirtPoint(a,.965)),.19,piping,true);
  line(g,angles.map(a=>skirtPoint(a,.981)),.11,piping,true);
  const waist=skin(3,96,(a,i)=>{const [rx,rz]=axes(dress.waist+.5+i*.8);return [(rx+.25)*Math.sin(a),i*.95,(rz+.25)*Math.cos(a)];});put(g,waist,band);
  for(let i=0;i<6;i++){
    const y=3+i*4.3,[,rz]=axes(bodiceRadius(y));
    oval(g,piping,0,y,rz+.15,.46,.45,.22);
  }
  for(const side of [-1,1]){
    const path=new T.CatmullRomCurve3([[10.1,29,9.4],[10.6,33,7.8],[11.4,39.4,4.9],[11.9,41.8,0],[11.8,39.5,-5.3],[10.7,34,-7.3],[10.1,29,-9.4]].map(([x,y,z])=>new T.Vector3(side*x,y,z)));
    const pts:number[]=[],uv:number[]=[],indices:number[]=[];
    for(let i=0;i<=48;i++){
      const p=path.getPoint(i/48);
      for(const w of [-1.14,1.14]){pts.push(p.x+w,p.y,p.z);uv.push(w<0?0:.07,i/48);}
      if(i<48){const a=i*2;indices.push(a,a+1,a+2,a+1,a+3,a+2);}
    }
    const geo=new T.BufferGeometry();geo.setAttribute("position",new T.Float32BufferAttribute(pts,3));geo.setAttribute("uv",new T.Float32BufferAttribute(uv,2));geo.setIndex(indices);geo.computeVertexNormals();put(g,geo,bodice);
  }
  return g;
}

export function makeMannequin(scene:T.Scene) {
  const body=new T.Group(),garment=buildDress();scene.add(body,garment);
  const linen=material("#ad7957"),wood=material("#bb8c68"),joint=material("#8f613f"),hair=material("#51392d"),brass=material("#b39a64");
  const torso=new T.Group();body.add(torso);
  const trunk=put(torso,new T.BufferGeometry(),linen);
  put(torso,loft([[137,8,6],[145,6.3,5.8],[150,4.9,4.7],[154,5.4,5.1]],48),wood);
  const head=put(torso,loft([[148.4,2.2,3.5,3],[149.9,4.2,5.1,2],[152,5.5,6.3,1.1],[155.5,6.6,7.3,.7],[161,6.9,7.1],[164.4,6.2,6.8,-.4],[166.4,4.7,5,-.5],[167.6,1.2,1.3,-.5]]),wood);
  // A dressed shop mannequin with a smooth sculpted face and a rolled chignon.
  oval(head,wood,0,156.6,7.55,1,1.55,1.25);
  const cap=skin(24,64,(a,i)=>{const u=i/23,edge=158.2+4.6*Math.cos(a)-.65*Math.cos(2*a+.8);
    const latitude=Math.asin((edge-159)/9.35)*(1-u)+Math.PI/2*u;
    return [7.86*Math.cos(latitude)*Math.sin(a),159+9.35*Math.sin(latitude),-.32+8.15*Math.cos(latitude)*Math.cos(a)];});put(head,cap,hair);
  oval(head,hair,0,157.1,-8.1,4.75,4.8,4.2);
  for(const sign of [-1,1]) {
    line(head,[[sign*1.6,158.4,7.45],[sign*2.75,158.15,7.08],[sign*3.7,158.35,6.65]],.13,joint);
    line(head,[[sign*1.6,159.65,7.42],[sign*2.7,159.9,7.1],[sign*3.75,159.75,6.56]],.15,hair);
    oval(head,wood,sign*6.6,156.3,.1,1.05,1.75,1.25);
    oval(head,brass,sign*6.9,154.65,.4,.44,.46,.44);
  }
  line(head,[[-1.3,153.9,7.59],[0,153.72,7.85],[1.3,153.9,7.59]],.11,joint);
  const arms={} as Record<Arm,T.Group>;
  for(const [key,side] of [["left",1],["right",-1]] as const){
    const arm=new T.Group();arm.position.set(side*16.7,139.2,0);arms[key]=arm;torso.add(arm);
    oval(arm,joint,0,0,0,4.6,4.6,4.4);
    put(arm,loft([[-29.2,2.95,3.02,.9],[-25.2,3.4,3.3,.7],[-17,3.8,4.05,.3],[-8,4.65,4.7],[-2,4.05,4.22]]),wood);
    oval(arm,joint,0,-29.2,.8,3.0,3.15,3.0);
    const fore=new T.Group(); fore.position.set(0,-29.2,.8);fore.rotation.x=-.10;arm.add(fore);
    put(fore,loft([[-24,2,2.1],[-18.5,2.4,2.1],[-9,3.5,3.25],[-4,3.6,3.35],[-1,2.65,2.67]]),wood);
    oval(fore,joint,0,-24,0,2.1,1.9,2.1);oval(fore,wood,0,-28.4,.45,2.1,4.7,2.8);
    oval(fore,wood,-side*1.5,-27.6,2,1,2.6,1);
    arm.traverse(o=>{o.userData.arm=key;});
    const leg=new T.Group();torso.add(leg); leg.position.set(side*8.2,0,side===1?-1.4:1.4);
    put(leg,loft([[46.6,3.6,4.25],[54,4.7,5.21],[64,6.35,7.35],[77,7.05,8.2],[87,7.3,8.1]]),linen);
    oval(leg,joint,0,46.1,.2,3.65,3.55,4.0);
    put(leg,loft([[7.4,2.5,3.1],[13,2.9,3.55],[25,4.7,5.35,-.7],[35,5.05,5.55,-.5],[42.6,3.6,4.05],[45,3.5,3.8]]),wood);
    oval(leg,joint,0,7.7,.2,2.55,2.8,3);oval(leg,wood,0,4.1,3.4,3.5,3.45,8.3);
    oval(leg,material("#976046"),0,1.72,3.8,3.7,1.15,8.8);
    line(leg,[[-3.1,3.15,8.5],[-2.4,5.1,7.2],[0,6.25,6.85],[2.4,5.1,7.2],[3.1,3.15,8.5]],.45,brass);
  }
  let prevBust=-1,prevWaist=-1;
  return {body,garment,apply(params:Form) {
    torso.scale.y=params.height/initial.height;
    if(params.bust!==prevBust||params.waist!==prevWaist){
      const hip=96+(params.waist-initial.waist)*.55;
      const spec=[[79,hip*.66,.72],[83,hip*.86,.72],[88,hip,.71],[94,(hip+params.waist)*.52,.71],
        [101.8,params.waist,.71],[109,params.waist*.62+params.bust*.38,.71],[119,params.bust*.95,.71],[124.8,params.bust,.71],
        [130,params.bust*.97,.70],[136,params.bust*.88,.64],[140.8,params.bust*.82,.51],[143,50,.52],[146.4,31,.78]];
      const rings:Ring[]=spec.map(([y,c,r])=>{const [rx,rz]=axes(c,r);return [y,rx,rz];});
      trunk.geometry.dispose();trunk.geometry=loft(rings,96);prevBust=params.bust;prevWaist=params.waist;
    }
    garment.position.y=shoulderLine(params.height)-dress.shoulderToWaist;
    arms.left.rotation.z=T.MathUtils.degToRad(params.left);arms.right.rotation.z=-T.MathUtils.degToRad(params.right);
  }};
}
