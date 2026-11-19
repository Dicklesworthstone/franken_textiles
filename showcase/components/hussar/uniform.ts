import * as T from "three";
import { mergeGeometries } from "three/addons/utils/BufferGeometryUtils.js";
import sampler from "../../public/wardrobe/hussar/braid-sampler.json";

export type Regiment = "Blue" | "Yellow";
export type RegimentView = Regiment | "Compare";
export type Layer = "Dressed" | "Dolman" | "Pelisse";
export type Framing = "Uniform" | "Chest" | "Reverse";
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

function woven() {
  const image=document.createElement("canvas");image.width=image.height=96;
  const c=image.getContext("2d")!;c.fillStyle="#b8b8b8";c.fillRect(0,0,96,96);
  for(let y=0;y<96;y+=3)for(let x=0;x<96;x+=3){
    const a=((x*31+y*23+x*y)%29)/170; c.fillStyle=`rgba(255,255,255,${.13+a})`;
    c.fillRect(x,y,(x+y)%6===0?2:1,2); c.fillStyle="rgba(15,15,15,.17)";c.fillRect(x+1,y+2,2,1);
  }
  const t=new T.CanvasTexture(image);t.wrapS=t.wrapT=T.RepeatWrapping;t.repeat.set(7,4);
  return t;
}
export function buildUniform(regiment:Regiment) {
  const blue=regiment==="Blue", grain=woven();
  const mat=(color:T.ColorRepresentation,roughness=.8,metalness=0,cloth=false)=>new T.MeshStandardMaterial({
    color,roughness,metalness,side:T.DoubleSide,...(cloth?{map:grain,bumpMap:grain,bumpScale:.14}:{}),
  });
  const wool=mat(blue?"#183550":"#202424",.94,0,true), outer=mat(blue?"#243f58":"#aa8151",.95,0,true);
  const brass=mat("#ba9557",.36,.58), cord=mat(blue?"#d6ad55":"#dbdacc",.63,.17,true);
  const cuff=mat(blue?"#203d57":"#bc934a",.91,0,true), fur=mat("#252726",.99,0,true);
  const lining=mat(blue?"#393832":"#d6ccae",.96,0,true);
  const plume=mat("#e0ddcc",.98,0,true), furLight=mat("#34332e",.97,0,true), yellow=mat("#d7ad41",.84,0,true);
  const boots=mat("#262729",.55,.08), bootEdge=mat("#39312b",.65);
  const buff=mat("#bb9f76",.91,0,true), wood=mat("#af8867",.86), endGrain=mat("#73553e",.85);
  const felt=mat("#20252b",.97,0,true), darkCord=mat("#3b3830",.9,0,true);
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
  const base=new T.Group(), dolman=new T.Group(), pelisse=new T.Group(), headwear=new T.Group();
  const torso:Profile[]=[[0,14.5,10.6],[2,15,10.8],[8,14.8,10.3],[15,15.3,10.5],[25,18.3,11.9],[33,20,12.3],[40,20.3,11.3],[44,19.3,10.1],[47,13,8.9],[48.5,7.3,7]];
  function profile(y:number) {
    let i=1;while(i<torso.length-1&&torso[i][0]<y)i++;
    const [y0,w0,d0]=torso[i-1],[y1,w1,d1]=torso[i],t=Math.max(0,Math.min(1,(y-y0)/(y1-y0)));
    return [T.MathUtils.lerp(w0,w1,t),T.MathUtils.lerp(d0,d1,t)];
  }
  function front(x:number,y:number){const [w,d]=profile(y);return d*Math.sqrt(Math.max(.004,1-(x/w)**2))+.47;}
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
      if(isOuter)for(let i=0;i<84;i++){
        const y=i*.56,[w,d]=profile(y),x=s*Math.sin(open)*w,z=Math.cos(open)*d;
        const a=i*2.39996, curl=oval(parent,i%6===0?furLight:fur,[x+Math.sin(a)*1.05,y,z+.22+Math.cos(a)*.84],[.46,.85,.34]);curl.rotation.z=.52*Math.sin(i);
      }
      // Braiding follows the face of the jacket. The same one-side motif is supplied as a flat DST sampler.
      for(let j=0;j<11;j++){
        const y0=8+j*3.05, [w]=profile(y0), gap=isOuter?w*Math.sin(open)+.8:1.05;
        const span=w*(.77+.025*Math.cos(j)) - gap;
        const p:XYZ[]=chestLoop.map(([px,py])=>{
          const x=s*(gap+(px-5)*span/87), y=y0+(py-18)*.106+(px-5)/87*(1.7-j*.06);
          return [x,y,front(x,y)+.2];
        });
        tube(parent,cord,p,.28);
        for(const x of [s*(gap+.15),s*(gap+span*.86)]) oval(parent,brass,[x,y0+.32+(Math.abs(x)-gap)/span*1.65,front(x,y0)+.54],[.71,.71,.51]);
      }
      const mark:XYZ[]=torso.slice(1,-2).map(([y,w,d])=>[s*.60*w,y,-d*.81]);
      tube(parent,cord,mark,.29);
      const sleeve=new T.Group();parent.add(sleeve);
      const sleeveRows:Profile[]=[[-10,4.4,4.4,s*26,3.5],[-8,4.8,4.7,s*26.1,3],[-2,4.9,4.9,s*25.5,2],[9,5.3,5,s*24.3,.6],[16,5.6,5.7,s*24,.1],[18,5.9,5.4,s*24.4,-.5],[20,5.6,5.5,s*23.8,-.2],[28,6.1,6.1,s*22.8,.4],[36,6.7,6.7,s*20.6,.8],[41,5.5,5.6,s*19,.3],[43,2,3,s*18,0]];
      mesh(sleeve,shell(sleeveRows,0,tau,.026),textile);
      mesh(sleeve,shell([[-10,4.65,4.65,s*26,3.5],[-4.6,5.05,5.05,s*25.72,2.6]]),isOuter?fur:cuff);
      ring(sleeve,isOuter?fur:cord,-9.7,4.6,4.7,isOuter?.65:.35,s*26,3.5);
      const s0=s*25.7;
      const cuffDetail:XYZ[]=[[s0-3.6,-4.8,6.2],[s0-1.4,-3.4,7.55],[s0,-1.6,7.35],[s0+1.3,-3,7.55],[s0,-4.3,7.75],[s0-1.3,-3,7.55],[s0,-1.7,7.35],[s0+3.6,-4.8,6.2]];
      tube(sleeve,cord,cuffDetail,.28);
      tube(sleeve,textile,[[s*25.8,-4.4,-2.4],[s*24,9,-4.45],[s*23,19,-5.7],[s*22,32,-5.25],[s*20,39,-4.95]],.29);
      if(!isOuter) {oval(parent,wood,[s*26.1,-14.4,3.9],[3.55,5.7,3]);oval(parent,wood,[s*23.65,-13.9,5],[1.36,3.1,1.51]);}
    }
    mesh(parent,shell([[47,7.7,7],[51.6,7.2,6.9]]),isOuter?fur:textile);
    ring(parent,isOuter?fur:cord,51.2,7.2,6.9,isOuter?1.25:.39);
  }
  jacket(dolman,false);dolman.position.y=96;
  // The second jacket hangs from the wearer's left shoulder, with an open front and empty sleeves.
  jacket(pelisse,true);
  pelisse.scale.set(.77,.86,.78);pelisse.rotation.set(-.13,.94,.40);
  pelisse.position.set(33,102,-4.4);
  mesh(base,shell([[92,14,10],[96,14.4,10.25],[99,13.8,9.8]]),buff);
  for(const s of [-1,1]){
    const leg:Profile[]=[[37,4.8,4.8,s*10,0],[49,5.6,6.15,s*10,0],[50.5,5.25,5.8,s*10,.15],[52.5,5.8,6.1,s*10,.1],[56,5.82,6.1,s*10.1,.1],[60,6.4,6.7,s*10.5,.5],[63,6.95,7.55,s*10.3,1.1],[65,6.85,7.43,s*10.3,1.4],[74,7.7,8.8,s*9.8,.4],[88,8.5,9.4,s*8.5,.6],[93,8.4,9.1,s*7.8,.3],[98,7,8,s*7,.1]];
    mesh(base,shell(leg,0,tau,.027),buff);
    const boot:Profile[]=[[1,5.1,11.6,s*10.5,4.7],[3,5.35,12.1,s*10.5,5.2],[5,5.25,10.7,s*10.5,4.2],[8,4.7,8.6,s*10.5,1.8],[11,4.15,5.9,s*10.4,0],[15,4.35,5.8,s*10.2,0],[22,4.7,6.2,s*10.2,-.6],[30,5.65,6.7,s*10.3,-.8],[37,5.65,6.05,s*10.5,-.2],[44,5,5.7,s*10.5,.5],[48,5.65,6.2,s*10.5,.6]];
    mesh(base,shell(boot,0,tau,.02),boots);ring(base,bootEdge,47.8,5.7,6.2,.65,s*10.5,.6);
    mesh(base,shell([[.25,5.05,11.6,s*10.5,4.7],[1.9,5.4,12.2,s*10.5,5]]),bootEdge);
  }
  for(let i=0;i<6;i++)ring(base,i%2===0?darkCord:yellow,96.6+i*1.13,15.3+(i%3)*.26,11.3,.76);
  for(let i=0;i<22;i++){
    const a=tau*i/22;const barrel=oval(base,brass,[15.7*Math.sin(a),99.35,11.4*Math.cos(a)],[.85,1.88,1.06]);barrel.rotation.y=a;
  }
  for(const s of [-1,1]) {
    tube(base,yellow,[[s*12,99.4,7],[s*14,97.4,8],[s*17,88.8,8]],.65);
    oval(base,yellow,[s*17,87.8,8],[1.06,2,1.15]);
  }
  mesh(base,shell([[143,5.2,5.1],[153,5.25,5.6],[159,6.5,6]]),wood);
  mesh(base,shell([[145.2,6.7,6.75],[151.5,6.55,6.7]]),felt);
  const head=new T.Group();base.add(head);head.position.y=151;
  mesh(head,shell([[0,1.8,2.9,0,2.2],[1.4,4,4.4,0,2.6],[4,5.9,6.4,0,1.1],[8.7,6.9,7.7,0,.3],[11.3,7.25,8,0,.1],[15,7.4,8.15,0,-.3],[18.5,6.6,7.6,0,-.65],[21.2,4,5.1,0,-.8],[22.4,.12,.12,0,-.8]]),wood);
  oval(head,wood,[0,9.1,7.4],[1.48,2.9,2.15]);
  for(const s of [-1,1]) {oval(head,wood,[s*7.05,9.3,-.45],[1.1,2.17,1.35]);tube(head,endGrain,[[s*1.6,12.4,7.75],[s*3.15,12.77,7.3],[s*4.8,12.3,6.4]],.14);}
  headwear.position.y=168;
  mesh(headwear,shell([[0,7.75,8.5],[2,8.1,8.9],[6,8.7,9.2],[13,blue?7.9:9.3,blue?8.6:9.9],[17,blue?6.3:8.6,blue?7.2:9],[17.8,.05,.05]]),blue?felt:fur);
  ring(headwear,cord,1.25,8,8.75,.38);
  const bag=new T.Group();headwear.add(bag);bag.position.set(blue?6:7,14,.4);bag.rotation.z=-.35;
  mesh(bag,shell([[-7,.75,1,2,0],[-2,2,2.1,1.4,0],[3,3.3,3.1,0,0],[4.6,1.9,2.7,-1,0]]),yellow);
  if(!blue)for(let i=0;i<240;i++){
    const a=i*2.39996, v=(i+.5)/240, radius=8.15+1.1*Math.sin(v*Math.PI);
    const curl=oval(headwear,i%6===0?furLight:fur,[radius*Math.sin(a),.8+v*16.25,(radius+.55)*Math.cos(a)],[.57,1.4,.49]);curl.rotation.z=.37*Math.sin(i);
  }
  oval(headwear,brass,[0,5.15,9.1],[2.35,2.6,.42]);
  for(let i=0;i<17;i++){const t=i/16;const feather=oval(headwear,plume,[(blue?1:-1)*(6.7+t*3.5),11+t*16,1.6-t],[1.05+1.20*Math.sin(t*Math.PI),2.25,.75+Math.sin(t*Math.PI)]);feather.rotation.z=.25+Math.sin(i*1.8)*.21;}
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
  const groups=[base,dolman,pelisse,headwear];groups.forEach(combine);
  const outfit=new T.Group();outfit.add(...groups);
  return {outfit, setLayer(layer:Layer){base.visible=headwear.visible=layer!=="Pelisse";dolman.visible=layer!=="Pelisse";pelisse.visible=layer!=="Dolman";
    if(layer==="Pelisse"){pelisse.scale.setScalar(1);pelisse.position.set(0,96,0);pelisse.rotation.set(0,0,0);}
    else{pelisse.scale.set(.77,.86,.78);pelisse.rotation.set(-.13,.94,.40);pelisse.position.set(33,102,-4.4);}
  },dispose(){outfit.traverse(o=>{if(o instanceof T.Mesh)o.geometry.dispose();});
    [wool,outer,brass,cord,cuff,fur,furLight,lining,plume,yellow,boots,bootEdge,buff,wood,endGrain,felt,darkCord].forEach(m=>m.dispose());grain.dispose();}};
}
