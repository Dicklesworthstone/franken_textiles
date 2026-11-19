import * as T from "three";
import { mergeGeometries } from "three/addons/utils/BufferGeometryUtils.js";
import sampler from "../../public/wardrobe/hussar/braid-sampler.json";
import {buildWearer, type Complexion, type FittedHussar} from "./portrait";
import {broadclothTexture,buckskinBreeches,napFibres,plumeGeometry,restArm,seedRandom,soutache} from "./haberdashery";
import {applyGarmentStudy,registerGarment,type GarmentMaps} from "./garmentSurface";

export type Regiment = "Blue" | "Yellow";
export type RegimentView = Regiment | "Compare";
export type Layer = "Dolman" | "Pelisse";
export type Framing = "Bust" | "Uniform" | "Chest" | "Reverse";
type Profile = [height: number, width: number, depth: number, cx?: number, cz?: number];
type XYZ = [number, number, number];
const chestLoop = sampler.pieces[0].decor[0].pts;
const tau = Math.PI * 2;

/** Each row is an elliptical cross-section in exhibition centimetres. The open pelisse leaves both facing edges visible. */
function shell(rows: Profile[], from=0, to=tau, folds=0) {
  const coords:number[]=[], uv:number[]=[], tri:number[]=[];
  const n=64, height=rows.at(-1)![0]-rows[0][0];
  rows.forEach(([y,w,d,cx=0,cz=0],j)=>{
    for(let i=0;i<=n;i++){
      const u=i/n, a=from+(to-from)*u, v=(y-rows[0][0])/height;
      const wave=1+folds*Math.cos(a*9+v*.9)*Math.sin(Math.PI*v);
      coords.push(cx+w*Math.sin(a)*wave,y,cz+d*Math.cos(a)*wave);uv.push(u,v);
      if(j<rows.length-1&&i<n){const k=j*(n+1)+i;tri.push(k,k+1,k+n+1,k+1,k+n+2,k+n+1);}
    }
  });
  const g=new T.BufferGeometry();
  g.setAttribute("position",new T.Float32BufferAttribute(coords,3));g.setAttribute("uv",new T.Float32BufferAttribute(uv,2));
  g.setIndex(tri);g.computeVertexNormals();return g;
}

export function buildUniform(regiment: Regiment, body: FittedHussar, complexion:Complexion, swatches:GarmentMaps) {
  const blue=regiment==="Blue", grain=broadclothTexture(), random=seedRandom(blue?1761:1765);
  const mat=(color:T.ColorRepresentation,roughness=.8,metalness=0,cloth=false)=>new T.MeshPhysicalMaterial({
    color,roughness,metalness,side:T.DoubleSide,...(cloth?{map:grain,bumpMap:grain,bumpScale:.032,sheen:.22,sheenRoughness:.85}:{}),
  });
  const wool=mat(blue?"#172b47":"#1c201f",.95,0,true), outer=mat(blue?"#21344e":"#ba9761",.96,0,true);
  const brass=mat("#88724c",.40,.72), cord=mat(blue?"#b69a52":"#cbc6b2",.85,.025,true);
  const cuff=mat(blue?"#203d57":"#bc934a",.91,0,true), fur=mat("#252726",.99,0,true);
  const lining=mat(blue?"#393832":"#d6ccae",.96,0,true);
  const plume=mat("#dfdac6",.95,0), yellow=mat("#b59237",.89,0,true);
  const boots=mat("#222421",.49,.045), bootEdge=mat("#302c25",.72);
  const buff=mat("#c7a269",.92,0,true), chamois=mat("#caab66",.86,0,true);
  const gaiter=mat(blue?"#1c2d44":"#252724",.94,0,true);
  const agedGold=mat("#75613c",.57,.44), welt=mat("#8e7041",.85,0);
  const pileInk=new T.LineBasicMaterial({color:"#4b443c",vertexColors:true,transparent:true,opacity:.87});
  const featherBarbs=new T.MeshStandardMaterial({color:"#e0daca",vertexColors:true,roughness:.98,side:T.DoubleSide});
  const extraMaterials:T.Material[]=[];
  const felt=mat("#20252b",.97,0,true), darkCord=mat("#3b3830",.9,0,true);
  const selectedPlate={value:swatches[regiment].Dressed};
  for(const m of [wool,outer,cuff,lining,gaiter,fur]){
    m.map=m.bumpMap=swatches.wool;m.bumpScale=.105;
  }
  for(const m of [wool,outer,cuff,lining,fur,gaiter,boots,bootEdge,buff,chamois,felt,yellow,darkCord])
    applyGarmentStudy(m,selectedPlate,.97);
  function mesh(parent:T.Object3D,g:T.BufferGeometry,m:T.Material) {
    const o=new T.Mesh(g,m);parent.add(o);return o;
  }
  function oval(parent:T.Object3D,m:T.Material,p:XYZ,scale:XYZ){
    const fine=Math.max(...scale)>3;
    const o=mesh(parent,new T.SphereGeometry(1,fine?24:10,fine?16:7),m);o.position.set(...p);o.scale.set(...scale);return o;
  }
  function tube(parent:T.Object3D,m:T.Material,points:XYZ[],r:number,close=false) {
    const line=new T.CatmullRomCurve3(points.map(p=>new T.Vector3(...p)),close,"centripetal");
    return mesh(parent,new T.TubeGeometry(line,Math.max(12,points.length*2),r,6,close),m);
  }
  function ring(parent:T.Object3D,m:T.Material,y:number,w:number,d:number,r:number,cx=0,cz=0) {
    const pts:XYZ[]=Array.from({length:40},(_,i)=>[cx+w*Math.sin(i*tau/40),y,cz+d*Math.cos(i*tau/40)]);
    tube(parent,m,pts,r,true);
  }
  const base=new T.Group(), dolman=new T.Group(), pelisse=new T.Group(), headwear=new T.Group(), shoulderCord=new T.Group();
  const torso:Profile[]=[[0,14.7,10.55],[2.1,15.35,10.75],[7.3,14.7,10.21],[13.7,15.18,10.65],[18.4,16.0,10.96],[23.6,17.42,11.73],[27.1,18.4,12.19],[30.4,19.0,12.50],[34.0,19.45,12.49],[37.7,19.7,12.15],[39.5,19.35,11.62],[41.3,18.7,11.09],[43.5,16.9,10.08],[45.4,14.22,9.31],[47.3,10.65,8.05],[48.5,7.25,7.05]];
  function profile(y:number) {
    let i=1;while(i<torso.length-1&&torso[i][0]<y)i++;
    const [y0,w0,d0]=torso[i-1],[y1,w1,d1]=torso[i],t=Math.max(0,Math.min(1,(y-y0)/(y1-y0)));
    return [T.MathUtils.lerp(w0,w1,t),T.MathUtils.lerp(d0,d1,t)];
  }
  function front(x:number,y:number){const [w,d]=profile(y);return d*Math.sqrt(Math.max(.004,1-(x/w)**2))+.17;}
  const pileRoots:T.Vector3[]=[];
  function jacket(parent:T.Group,isOuter:boolean){
    const textile=isOuter?outer:wool, open=isOuter?.43:0;
    mesh(parent,shell(torso,open,tau-open,.009),textile);
    if(isOuter){const liningRows:Profile[]=torso.map(([y,w,d])=>[y,w-.3,d-.3]);mesh(parent,shell(liningRows,open,tau-open),lining);}
    const hem:XYZ[]=[];
    for(let i=0;i<=56;i++){const a=open+(tau-2*open)*i/56;hem.push([14.55*Math.sin(a),.25,10.6*Math.cos(a)]);}
    tube(parent,isOuter?fur:cord,hem,isOuter?1.6:.36);
    for(const s of [-1,1]){
      const edge=torso.map(([y,w,d]):XYZ=>[s*Math.sin(open)*w,y,Math.cos(open)*d+.25]);
      tube(parent,isOuter?fur:cord,edge,isOuter?1.4:.36);
      if(isOuter)for(let i=0;i<1550;i++){
        const y=random()*47.1,[w,d]=profile(y),x=s*Math.sin(open)*w,z=Math.cos(open)*d;
        const a=random()*tau;
        pileRoots.push(new T.Vector3(x+Math.sin(a)*1.14,y,z+.4+Math.cos(a)*1.11));
      }
      // Braiding follows the face of the jacket. The same one-side motif is supplied as a flat DST sampler.
      const rail:XYZ[]=[];
      for(let j=0;j<12;j++){
        const y0=7.6+j*2.74, [w]=profile(y0), gap=isOuter?w*Math.sin(open)+.79:.48;
        const span=w*(.735+.075*j/11) - gap;
        const p:XYZ[]=chestLoop.map(([px,py])=>{
          const a=(px-5)/87;
          const x=s*(gap+a*span), y=y0+(py-18)*.066+a*(.56+j*.084)-.3*Math.sin(Math.PI*a);
          return [x,y,front(x,y)+.18];
        });
        mesh(parent,soutache(p,.205),cord);
        const x=s*(gap+span*.85),y=y0+.29+.85*(.56+j*.084);
        rail.push([x,y,front(x,y)+.15]);
        for(const bx of (isOuter ? [s*(gap+.1),x] : s===1 ? [0,x] : [x])){
          const by=bx===x?y:y0+.13, z=front(bx,by);
          oval(parent,agedGold,[bx,by,z+.13],[.58,.58,.25]);
          oval(parent,brass,[bx,by,z+.37],[.49,.49,.37]);
        }
      }
      mesh(parent,soutache(rail,.135),cord);
      const mark:XYZ[]=torso.slice(1,-2).map(([y,w,d])=>[s*.60*w,y,-d*.81]);
      tube(parent,cord,mark,.29);
      const sleeve=new T.Group();parent.add(sleeve);
      const sleeveRows:Profile[]=[[-10,4.4,4.4,s*26,3.5],[-8,4.65,4.56,s*26.1,3],[-2,4.87,4.62,s*25.5,2],[6.8,5.27,4.77,s*24.4,.65],[12.2,5.46,5.21,s*24.05,.08],[15.3,5.56,5.52,s*24.16,-.43],[16.6,5.85,5.58,s*24.27,-.62],[17.7,5.38,5.36,s*24.10,-.72],[18.8,5.72,5.54,s*24.08,-.57],[20.6,5.56,5.49,s*23.77,-.26],[25.2,5.88,5.74,s*23.33,.10],[30.1,6.1,6.17,s*22.15,.45],[34.6,6.45,6.25,s*20.65,.55],[37.8,6.31,6.07,s*19.82,.32],[40.5,5.47,5.36,s*18.97,.19],[42.9,4.09,4.17,s*17.76,.08],[44.8,2.67,2.92,s*16.15,0],[45.7,1.2,1.75,s*15.1,0]];
      mesh(sleeve,shell(sleeveRows,0,tau,.026),textile);
      mesh(sleeve,shell([[-10,4.65,4.65,s*26,3.5],[-4.6,5.05,5.05,s*25.72,2.6]]),isOuter?fur:cuff);
      ring(sleeve,isOuter?fur:cord,-9.7,4.6,4.7,isOuter?.65:.35,s*26,3.5);
      if(isOuter)for(let i=0;i<800;i++){const a=random()*tau,y=-9.65+random()*4.6;pileRoots.push(new T.Vector3(s*26+4.75*Math.sin(a),y,3.34+4.80*Math.cos(a)));}
      const s0=s*25.7;
      const cuffDetail:XYZ[]=[[s0-3.6,-4.8,6.2],[s0-1.4,-3.4,7.55],[s0,-1.6,7.35],[s0+1.3,-3,7.55],[s0,-4.3,7.75],[s0-1.3,-3,7.55],[s0,-1.7,7.35],[s0+3.6,-4.8,6.2]];
      tube(sleeve,cord,cuffDetail,.28);
      tube(sleeve,textile,[[s*25.8,-4.4,-2.4],[s*24,9,-4.45],[s*23,19,-5.7],[s*22,32,-5.25],[s*20,39,-4.95]],.29);
    }
    mesh(parent,shell([[47,7.7,7],[49.7,7.34,7.17],[isOuter?50.6:54.1,7.05,isOuter?6.95:7.28,0,-.3]],isOuter?.45:.035,isOuter?tau-.45:tau-.035),isOuter?fur:textile);
    ring(parent,isOuter?fur:cord,isOuter?50.65:53.9,7.06,isOuter?6.95:7.27,isOuter?.9:.20,0,-.30);
    if(!isOuter){ring(parent,cord,50.0,7.34,7.17,.18);ring(parent,cord,49.45,7.4,7.19,.12);}
  }
  jacket(dolman,false);dolman.position.y=96;
  // The second jacket hangs from the wearer's left shoulder, with an open front and empty sleeves.
  jacket(pelisse,true);
  // The broad leather belt travels around both sides of the dressed figure.
  function crossbelt(side:number,halfWidth:number,reverse:boolean) {
    const positions:number[]=[],uv:number[]=[],tri:number[]=[],edges:XYZ[][]=[[],[]];
    const rows=54;
    for(let i=0;i<=rows;i++){
      const t=i/rows, x=side*(-11.85+23.80*t), y=2.45+42.5*t;
      for(const [j,k] of [-1,1].entries()){
        const bx=x+k*halfWidth*.847, by=y-k*halfWidth*.527*side;
        const lift=reverse?.42:.41*(1-T.MathUtils.smoothstep(t,.86,1))+.57;
        const z=(reverse?-1:1)*(front(bx,by)+lift);
        positions.push(bx,96+by,z);uv.push(j,t);
        edges[j].push([bx,96+by,z+(reverse?-.035:.035)]);
      }
      if(i<rows){const a=i*2;tri.push(a,a+2,a+1,a+1,a+2,a+3);}
    }
    if(!reverse)for(let i=0;i<tri.length;i+=3){const k=tri[i];tri[i]=tri[i+1];tri[i+1]=k;}
    const g=new T.BufferGeometry();g.setAttribute("position",new T.Float32BufferAttribute(positions,3));
    g.setAttribute("uv",new T.Float32BufferAttribute(uv,2));g.setIndex(tri);g.computeVertexNormals();
    mesh(shoulderCord,g,chamois);edges.forEach(p=>tube(shoulderCord,welt,p,.066));
    if(!reverse){
      const left=edges[0][rows],right=edges[1][rows],v:number[]=[],tris:number[]=[],map:number[]=[];
      for(let i=0;i<=15;i++){
        const a=Math.PI*i/15;
        for(const [j,p] of [left,right].entries()){
          const bx=p[0],bz=p[2]*Math.cos(a);
          let shoulder=39;
          for(let y=48.5;y>=39;y-=.11){const [w,d]=profile(y);if((bx/w)**2+(bz/d)**2<=1){shoulder=y;break;}}
          const margin=Math.min(1,4*Math.sin(a));
          v.push(bx,T.MathUtils.lerp(p[1],96+shoulder+.87,margin),bz);map.push(j,i/15);
        }
        if(i<15){const k=2*i;tris.push(k,k+2,k+1,k+1,k+2,k+3);}
      }
      const bridge=new T.BufferGeometry();bridge.setAttribute("position",new T.Float32BufferAttribute(v,3));
      bridge.setAttribute("uv",new T.Float32BufferAttribute(map,2));bridge.setIndex(tris);bridge.computeVertexNormals();mesh(shoulderCord,bridge,chamois);
    }
  }
  crossbelt(1,2.57,false);crossbelt(1,2.57,true);
  crossbelt(-1,1.07,false);crossbelt(-1,1.07,true);
  for(const s of [-1,1]){
    mesh(shoulderCord,shell([[83.8,3.88,4.1,s*26.1,3.5],[85.4,4.75,4.95,s*26.0,3.46],[87.1,5.33,5.35,s*26.0,3.3],[89.4,5.51,5.60,s*25.93,2.83],[92.0,5.74,5.76,s*25.72,2.53]],0,tau,.025),chamois);
    ring(shoulderCord,welt,91.95,5.75,5.76,.11,s*25.72,2.53);
    const leg:Profile[]=[[37,3.7,3.7,s*10.5,0],[46,4.1,4.45,s*10.5,.55],[47.7,4.45,5.2,s*10.5,.6],[49,5.6,6.15,s*10,0],[50.5,5.25,5.8,s*10,.15],[52.5,5.8,6.1,s*10,.1],[56,5.82,6.1,s*10.1,.1],[60,6.4,6.7,s*10.5,.5],[63,6.95,7.55,s*10.3,1.1],[65,6.85,7.43,s*10.3,1.4],[74,7.7,8.8,s*9.8,.4],[88,8.5,9.4,s*8.5,.6],[93,8.4,9.1,s*7.8,.3],[98,7,8,s*7,.1]];
    mesh(base,buckskinBreeches(s),buff);
    const hose=leg.filter(row=>row[0]<=88).map(([y,w,d,x,z]):Profile=>[y,w+.16,d+.16,x,z]);
    hose[hose.length-1]=[83.1,8.56,9.37,s*9.08,.45];
    const charivade=shell(hose,0,tau,.023);
    const garmentPoints=charivade.getAttribute("position");
    const edging:XYZ[]=[];
    for(let i=0;i<65;i++){
      const a=i*tau/64, k=garmentPoints.count-65+i;
      const y=83.1+1.00*Math.cos(2*a)-.55*Math.cos(4*a);
      garmentPoints.setY(k,y);edging.push([garmentPoints.getX(k),y,garmentPoints.getZ(k)]);
    }
    charivade.computeVertexNormals();mesh(base,charivade,gaiter);tube(base,cord,edging,.22);
    tube(base,cord,[[s*15.2,48,1.55],[s*15.4,55,1.3],[s*16.3,62,1.8],[s*17.2,73,1.5],[s*17.43,83.8,1.05]],.16);
    const boot:Profile[]=[[1,5.1,11.6,s*10.5,4.7],[3,5.35,12.1,s*10.5,5.2],[5,5.25,10.7,s*10.5,4.2],[8,4.7,8.6,s*10.5,1.8],[11,4.15,5.9,s*10.4,0],[15,4.35,5.8,s*10.2,0],[22,4.7,6.2,s*10.2,-.6],[30,5.65,6.7,s*10.3,-.8],[37,5.65,6.05,s*10.5,-.2],[44,5,5.7,s*10.5,.5],[48,5.65,6.2,s*10.5,.6]];
    const leather=shell(boot,0,tau,.026), raw=leather.getAttribute("position"), topline:XYZ[]=[];
    for(let i=0;i<65;i++){
      const a=i*tau/64, k=raw.count-65+i;
      const y=47.7+1.9*Math.cos(a)-.91*Math.cos(2*a);
      raw.setY(k,y);topline.push([raw.getX(k),y,raw.getZ(k)]);
    }
    leather.computeVertexNormals();mesh(base,leather,boots);tube(base,cord,topline,.21);
    for(let row=0;row<4;row++){
      const fold:XYZ[]=[];
      for(let i=0;i<=24;i++){
        const a=-1.38+2.76*i/24;
        fold.push([s*10.4+(4.49+row*.06)*Math.sin(a),10.8+row*1.65+.9*Math.sin(a*2+row*.59),.15+5.95*Math.cos(a)]);
      }
      tube(base,bootEdge,fold,.085);
    }
    mesh(base,shell([[.25,5.05,11.6,s*10.5,4.7],[1.9,5.4,12.2,s*10.5,5]]),bootEdge);
    const heel=mesh(base,new T.BoxGeometry(7.05,1.8,5.98),bootEdge);
    heel.position.set(s*10.5,.98,-3.3);
  }
  for(let i=0;i<12;i++)ring(base,i%3===0?darkCord:yellow,97.15+i*.37,15.42+(i%3)*.08,11.36,.265);
  for(let i=0;i<18;i++){
    const a=tau*i/18;const barrel=oval(base,agedGold,[15.6*Math.sin(a),99.22,11.50*Math.cos(a)],[.41,1.76,.40]);barrel.rotation.y=a;
    for(let k=0;k<3;k++)oval(base,brass,[15.68*Math.sin(a),98.15+k*.97,11.56*Math.cos(a)],[.47,.25,.47]);
  }
  for(let k=0;k<2;k++) {
    const x=-15.2-k*1.72,y=90.6-k*2.8;
    tube(base,yellow,[[-12.4,99.6,6.9],[-14.5-k*.41,97.4,7.6],[x,y,7.4]],.27);
    oval(base,yellow,[x,y-.39,7.4],[.64,.96,.59]);
    for(let i=0;i<26;i++){
      const a=i*tau/26;
      tube(base,i%4===0?darkCord:yellow,[[x+.19*Math.sin(a),y-.93,7.4+.19*Math.cos(a)],
        [x+.38*Math.sin(a),y-2.1,7.4+.38*Math.cos(a)],[x+.68*Math.sin(a),y-4.15+random()*.81,7.4+.65*Math.cos(a)]],.060);
    }
  }
  mesh(base,shell([[145.2,6.7,6.75,0,-.3],[147.2,7.1,7.55,0,-.75],[149.6,7.0,7.92,0,-1.0],[151.5,6.75,7.7,0,-1.0],[151.6,5.1,6.1,0,-1.0]]),felt);
  headwear.position.y=168;
  headwear.position.z=-.8;
  headwear.scale.z=1.3;
  headwear.rotation.z=blue?.045:-.018;
  const hatRoots:T.Vector3[]=[];
  if(blue) {
    mesh(headwear,shell([[0,7.65,8.34],[1.1,7.86,8.54],[7.4,8.22,8.85],[9.7,8.13,8.56]]),felt);
    mesh(headwear,shell([[9.55,8.13,8.58],[14.6,7.14,7.98,-.50,-.18],[17.5,6.47,7.45,-.69,-.24],[18.1,5.82,6.72,-.8,-.28],[18.66,.04,.04,-.95,-.30]],0,tau,.015),yellow);
    ring(headwear,cord,9.69,8.12,8.6,.13);
    mesh(headwear,shell([[-24.6,.81,.19,-10.64,-2.7],[-18,1.10,.27,-10.75,-2.23],[-10,1.15,.25,-10.24,-1.88],[-3,1.60,.38,-9.1,-2.14],[4.8,1.75,.41,-8.89,-2.9],[10.3,1.80,.44,-8.3,-3.1],[15.2,1.19,.60,-7.26,-2.4],[17.4,.45,.4,-6.3,-2.0]],0,tau,.064),yellow);
    oval(headwear,brass,[.0,5.2,8.91],[1.33,1.69,.25]);
  } else {
    mesh(headwear,shell([[0,7.83,8.45],[1.1,8.16,8.78],[5,8.75,9.18],[12.4,8.56,9.05],[16.7,7.86,8.58],[17.56,7.62,8.29],[18.01,.05,.05]],0,tau,.015),fur);
    for(let i=0;i<12500;i++){
      const a=random()*tau,t=random(), w=8.18+.52*Math.sin(Math.PI*t);
      hatRoots.push(new T.Vector3(w*Math.sin(a),.15+t*17,(w+.55)*Math.cos(a)));
    }
    mesh(headwear,shell([[-4.4,.53,.37,7.59,-3.3],[1.0,1.56,.83,8.04,-2.9],[6.9,2.36,1.09,8.16,-2.5],[11.2,2.41,1.52,7.84,-2.1],[14.6,1.76,1.58,6.66,-2.0]]),yellow);
    const bone=mat("#b5b3a6",.48,.62);
    const badge=new T.Group();headwear.add(badge);badge.position.set(0,6.75,9.32);
    for(const s of [-1,1]){
      tube(badge,bone,[[-2.18*s,-2.31,.10],[0,-1.26,.2],[2.18*s,-.23,.10]],.18);
      for(const k of [-1,1])oval(badge,bone,[2.1*s,-.24+k*.14,.11],[.28,.18,.18]);
    }
    oval(badge,bone,[0,.8,0],[1.23,1.36,.37]);oval(badge,bone,[0,-.24,.12],[.77,.55,.28]);
    for(const s of [-1,1])oval(badge,felt,[s*.44,.84,.345],[.26,.22,.075]);
    oval(badge,felt,[0,.42,.36],[.13,.20,.055]); extraMaterials.push(bone);
  }
  const cordSide=blue?-1:1;
  for(let k=0;k<2;k++){
    const x=cordSide*(blue?11.4+k*.8:9.0+k*.5), bottom=blue?-22-k*4:-8-k*2;
    tube(headwear,cord,[[cordSide*7.5,13.55,-2.0],[x,5.5,-1.35],[x+cordSide*.64,-3,-.9],[x+cordSide*.27,bottom+.9,-.65]],.20);
    const tx=x+cordSide*.27;oval(headwear,cord,[tx,bottom,-.65],[.60,.93,.52]);
    for(let i=0;i<21;i++){
      const a=i*tau/21;
      tube(headwear,yellow,[[tx+.24*Math.sin(a),bottom-.48,-.65+.19*Math.cos(a)],[tx+.37*Math.sin(a),bottom-2,-.65+.28*Math.cos(a)],[tx+.61*Math.sin(a),bottom-4.65+random()*.75,-.65+.44*Math.cos(a)]],.075);
    }
  }
  const feather=new T.Group();feather.position.set(blue?-4.3:3.8,15.6,.58);
  headwear.add(feather);tube(feather,plume,[[0,0,0],[-.35,5,.1],[-1.02,12,.21],[-2.0,20.8,.06]],.10);
  mesh(feather,plumeGeometry(21,blue?-4.0:-1.7,blue?1001:1002),featherBarbs);
  const combine=(group:T.Group)=>{
    const bins = new Map<T.Material, T.BufferGeometry[]>();
    group.updateMatrixWorld(true);
    const inverse = new T.Matrix4().copy(group.matrixWorld).invert();
    const local = new T.Matrix4();
    group.traverse(object => {
      if (!(object instanceof T.Mesh)) return;
      // Every shell, tube and sphere owns an indexed geometry. Keep the shared vertices when batching by textile.
      const geometry = object.geometry as T.BufferGeometry;
      geometry.applyMatrix4(local.multiplyMatrices(inverse, object.matrixWorld));
      const bucket = bins.get(object.material) ?? [];
      bucket.push(geometry);
      bins.set(object.material, bucket);
    });
    group.clear();
    for (const [material, geometries] of bins) {
      mesh(group, mergeGeometries(geometries), material);
      geometries.forEach(geometry => geometry.dispose());
    }
  };
  const groups=[base,dolman,pelisse,headwear,shoulderCord];groups.forEach(combine);
  for(const group of [dolman,shoulderCord]){
    group.traverse(part=>{
      if(!(part instanceof T.Mesh))return;
      const geometry=part.geometry, points=geometry.getAttribute("position"), baseY=group.position.y;
      for(let i=0;i<points.count;i++){
        const x=points.getX(i), y=baseY+points.getY(i);
        if(Math.abs(x)<17.7||y>113)continue;
        const moved=restArm(x,y,points.getZ(i),Math.sign(x));
        points.setXYZ(i,moved[0],moved[1]-baseY,moved[2]);
      }
      geometry.computeVertexNormals();geometry.computeBoundingSphere();
    });
  }
  if(hatRoots.length)headwear.add(new T.LineSegments(napFibres(hatRoots,.94,761,-1),pileInk));
  pelisse.add(new T.LineSegments(napFibres(pileRoots,.73,762,-1),pileInk));
  pelisse.position.y=96;
  groups.forEach(group=>registerGarment(group,group===pelisse ? (blue ? -41 : 41) : undefined));
  const portrait = buildWearer(body, blue, complexion, selectedPlate);
  const outfit=new T.Group();outfit.add(...groups, portrait.wearer);
  return {outfit, setLayer(layer:Layer){portrait.wearer.visible=base.visible=headwear.visible=layer!=="Pelisse";dolman.visible=layer!=="Pelisse";pelisse.visible=layer==="Pelisse";
    shoulderCord.visible=false;
    portrait.gloves(false);
    selectedPlate.value=layer==="Pelisse" ? swatches.pelisses : swatches[regiment].Dolman;
  },dispose(){
    groups.forEach(group => group.traverse(o=>{if(o instanceof T.Mesh||o instanceof T.LineSegments)o.geometry.dispose();}));
    portrait.dispose();
    [wool,outer,brass,cord,cuff,fur,lining,plume,yellow,boots,bootEdge,buff,felt,darkCord,chamois,gaiter,agedGold,welt,pileInk,featherBarbs,...extraMaterials].forEach(m=>m.dispose());grain.dispose();}};
}
