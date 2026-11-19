import * as T from "three";
import { axes, cutPosition, dress, type Arm } from "./form";

type Point = [number, number];
type Landmarks = Record<"shoulderLeft" | "shoulderRight" | "waist" | "chest", readonly number[]>;
const tau=Math.PI*2;
const cross=(o:Point,a:Point,b:Point)=>(a[0]-o[0])*(b[1]-o[1])-(a[1]-o[1])*(b[0]-o[0]);

// Convex sections bridge the space between the breasts as a single sheet of woven fabric would.
function hull(points:Point[]){
  points.sort((a,b)=>a[0]-b[0]||a[1]-b[1]);
  function chain(vertices:Point[]){
    const result:Point[]=[];
    for(const p of vertices){
      while(result.length>1&&cross(result[result.length-2],result[result.length-1],p)<=0)result.pop();
      result.push(p);
    }
    result.pop();return result;
  }
  return chain(points).concat(chain(points.reverse()));
}
function girth(y:number){
  if(y<9)return dress.waist+(84-dress.waist)*y/9;
  if(y<20)return 84+(dress.bust-84)*(y-9)/11;
  return y<23?dress.bust:dress.bust-(y-23)*.7;
}

// Register the cut on the evaluated skin. Keep the available cloth length when a cross-section
// is too large by opening its side closure. This geometric wrap does not solve cloth contact.
export function registerDress(garment:T.Group,surface:T.BufferGeometry,armAt:(i:number)=>Arm|null){
  const index=surface.getIndex()!,body=surface.getAttribute("position");
  const items=garment.children.map(o=>{
    const mesh=o as T.Mesh<T.BufferGeometry>;mesh.updateMatrix();
    mesh.geometry.applyMatrix4(mesh.matrix);mesh.position.set(0,0,0);mesh.scale.setScalar(1);mesh.rotation.set(0,0,0);
    const p=mesh.geometry.getAttribute("position") as T.BufferAttribute;
    return {mesh,p,source:(p.array as Float32Array).slice()};
  });
  const torso:number[]=[]; // Anny's arm surface must never be sewn inside the bodice.
  for(let f=0;f<index.count/3;f++){
    if(armAt(f))continue;
    const a=index.getX(3*f),b=index.getX(3*f+1),c=index.getX(3*f+2);
    if(body.getY(a)>32&&body.getY(a)<151&&Math.abs(body.getX(a))<25)torso.push(a,b,c);
  }
  const edges:number[]=[],seen=new Set<number>();
  for(let i=0;i<torso.length;i+=3)for(let j=0;j<3;j++){
    const a=torso[i+j],b=torso[i+(j+1)%3],key=Math.min(a,b)*body.count+Math.max(a,b);
    if(!seen.has(key)){seen.add(key);edges.push(a,b);}
  }
  const shoulderCrest=(x:number,low:number,high:number)=>{
    let height=-Infinity,depth=0;
    for(let i=0;i<torso.length;i+=3){
      for(let j=0;j<3;j++){
        const a=torso[i+j],b=torso[i+(j+1)%3],ax=body.getX(a),bx=body.getX(b);
        if((ax>x)===(bx>x))continue;
        const t=(x-ax)/(bx-ax),y=T.MathUtils.lerp(body.getY(a),body.getY(b),t);
        if(y>=low&&y<=high&&y>height){height=y;depth=T.MathUtils.lerp(body.getZ(a),body.getZ(b),t);}
      }
    }
    return {height,depth};
  };
  const shoulderFaces=new Map<number,number[]>();
  const bin=(x:number,y:number)=>Math.floor(x/3)+256*Math.floor(y/3);
  function shoulderDepth(x:number,y:number,front:boolean){
    let outer=front?-Infinity:Infinity;
    for(const i of shoulderFaces.get(bin(x,y))??[]){
      const a=torso[i],b=torso[i+1],c=torso[i+2];
      const ax=body.getX(a),ay=body.getY(a);
      if(Math.abs(ay-y)>7||Math.abs(ax-x)>5)continue;
      const bx=body.getX(b)-ax,by=body.getY(b)-ay,cx=body.getX(c)-ax,cy=body.getY(c)-ay;
      const det=bx*cy-by*cx;if(Math.abs(det)<1e-8)continue;
      const u=((x-ax)*cy-(y-ay)*cx)/det,v=(bx*(y-ay)-by*(x-ax))/det;
      if(u<0||v<0||u+v>1)continue;
      const z=body.getZ(a)*(1-u-v)+body.getZ(b)*u+body.getZ(c)*v;
      outer=front?Math.max(outer,z):Math.min(outer,z);
    }
    return outer;
  }
  function section(points:Point[],length:number,zGuess:number,skirtDepth=0){
    const poly=hull(points),samples:Point[]=[];
    const cz=poly.length?(Math.max(...poly.map(p=>p[1]))+Math.min(...poly.map(p=>p[1])))/2:zGuess;
    const [rx,rz]=axes(length);
    for(let i=0;i<96;i++){
      const a=tau*i/96-Math.PI/2,dx=Math.sin(a),dz=Math.cos(a);
      let radial=0;
      for(let j=0;j<poly.length;j++){
        const [x,z]=poly[j],[nx,nz]=poly[(j+1)%poly.length],ex=nx-x,ez=nz-z,det=dx*ez-dz*ex;
        if(Math.abs(det)<1e-7)continue;
        const r=(x*ez-(z-cz)*ex)/det,t=(x*dz-(z-cz)*dx)/det;
        if(t>=0&&t<=1&&r>radial)radial=r;
      }
      if(!radial)radial=1/Math.hypot(dx/rx,dz/rz);
      radial+=.32;
      if(skirtDepth>0){
        const t=skirtDepth/dress.skirtLength,shape=1/Math.hypot(dx/(14.5+9*t),dz/(10.3+10*t));
        const fold=(.52+.48*Math.cos(15*a+.65*t))*(.70+.30*Math.sin(7*a-t));
        radial=Math.max(radial,shape)+(.75+4.0*t)*fold*Math.min(1,skirtDepth/6);
      }
      samples.push([dx*radial,cz+dz*radial]);
    }
    const dist=(a:Point,b:Point)=>Math.hypot(a[0]-b[0],a[1]-b[1]);
    const total=samples.reduce((s,p,i)=>s+dist(p,samples[(i+1)%96]),0);
    const scale=Math.max(1,length/total),gap=Math.max(0,(total-length)/2),d=[0];
    for(let i=1;i<=96;i++)d[i]=d[i-1]+dist(samples[i-1],samples[i%96]);
    return (angle:number)=>{
      // Start at the right side seam, centered on the missing arc when the closure opens.
      const u=T.MathUtils.clamp((angle+Math.PI/2)/tau,0,1)*96,b=Math.min(95,Math.floor(u));
      const arc=gap+T.MathUtils.lerp(d[b],d[b+1],u-b)*(1-2*gap/total);
      let lo=0,hi=96;while(hi-lo>1){const mid=(lo+hi)>>1;if(d[mid]<=arc)lo=mid;else hi=mid;}
      const t=(arc-d[lo])/(d[lo+1]-d[lo]),p=samples[lo],q=samples[(lo+1)%96];
      return [scale*T.MathUtils.lerp(p[0],q[0],t),cz+scale*(T.MathUtils.lerp(p[1],q[1],t)-cz)] as Point;
    };
  }
  return (landmarks:Landmarks)=>{
    const shoulderY=(landmarks.shoulderLeft[1]+landmarks.shoulderRight[1])*.5;
    const shoulderZ=(landmarks.shoulderLeft[2]+landmarks.shoulderRight[2])*.5;
    const crest=shoulderCrest(11,shoulderY-4,shoulderY+10).height;
    // The strap travels forward over the shoulder, so the vertical drop is shorter than the
    // fixed 41 cm pattern length. Offset is taken from the reference shoulder and waist anchors.
    const verticalDrop=dress.shoulderToWaist-2.8;
    const waist=(isFinite(crest)?crest:landmarks.waist[1]+verticalDrop)+.28-verticalDrop;
    shoulderFaces.clear();
    for(let i=0;i<torso.length;i+=3){
      const a=torso[i],b=torso[i+1],c=torso[i+2],ay=body.getY(a),by=body.getY(b),cy=body.getY(c);
      const y0=Math.min(ay,by,cy),y1=Math.max(ay,by,cy);
      if(y1<waist+23||y0>waist+48)continue;
      const ax=body.getX(a),bx=body.getX(b),cx=body.getX(c);
      for(let x=Math.floor(Math.min(ax,bx,cx)/3);x<=Math.floor(Math.max(ax,bx,cx)/3);x++)
        for(let y=Math.floor(y0/3);y<=Math.floor(y1/3);y++){
          const key=x+256*y,faces=shoulderFaces.get(key);if(faces)faces.push(i);else shoulderFaces.set(key,[i]);
        }
    }
    const cuts=Array.from({length:91},():Point[]=>[]);
    for(let i=0;i<edges.length;i+=2){
      const a=edges[i],b=edges[i+1],ay=body.getY(a),by=body.getY(b);
      const first=Math.max(-60,Math.ceil(Math.min(ay,by)-waist)),end=Math.min(31,Math.ceil(Math.max(ay,by)-waist));
      for(let y=first;y<end;y++){
        if(y<0&&y%2!==0)continue;
        const t=(waist+y-ay)/(by-ay);
        cuts[y+60].push([T.MathUtils.lerp(body.getX(a),body.getX(b),t),T.MathUtils.lerp(body.getZ(a),body.getZ(b),t)]);
      }
    }
    const rows=Array.from({length:31},(_,i)=>section([...cuts[60+i]],girth(i),landmarks.waist[2]));
    const skirt=Array.from({length:31},(_,i)=>{
      const t=i/30,desired=dress.waist+(dress.gatheredEdge-dress.waist)*(1-Math.exp(-t/.115))+(dress.hem-dress.gatheredEdge)*t;
      return section(cuts[60-2*i],desired,landmarks.waist[2],i*2);
    });
    function onBodice(angle:number,y:number){
      const h=T.MathUtils.clamp(y,0,30),i=Math.min(29,Math.floor(h)),a=rows[i](angle),b=rows[i+1](angle);
      return [T.MathUtils.lerp(a[0],b[0],h-i),T.MathUtils.lerp(a[1],b[1],h-i)] as Point;
    }
    const shoulderPeaks=[-1,1].map(side=>shoulderCrest(side*11.5,shoulderY-4,shoulderY+10));
    for(const {mesh,p,source} of items){
      for(let i=0;i<p.count;i++){
        const x=source[3*i],y=source[3*i+1],z=source[3*i+2];
        const [rx,rz]=axes(girth(Math.max(0,y))),radial=Math.hypot(x/rx,z/rz);
        let a=Math.atan2(x/rx,z/rz);if(a< -Math.PI/2)a+=tau;
        const side=mesh.userData.sideSeam;
        if(side&&i%(side+1)===side)a=1.5*Math.PI;
        if(side&&i%(side+1)===0)a=-Math.PI/2;
        if(y<0){
          const level=T.MathUtils.clamp(-y/2,0,30),j=Math.min(29,Math.floor(level)),s=skirt[j](a),t=skirt[j+1](a);
          const trim=mesh.userData.hem?.11:0;
          p.setXYZ(i,T.MathUtils.lerp(s[0],t[0],level-j)+trim*Math.sin(a),waist+y,T.MathUtils.lerp(s[1],t[1],level-j)+trim*Math.cos(a));
        }else if(y>24){
          const start=onBodice(a,24),{x:flatX,isFront}=cutPosition(a);
          const v=T.MathUtils.clamp((y-24)/(dress.shoulderToWaist-24),0,1);
          const xx=T.MathUtils.lerp(start[0],flatX,T.MathUtils.smoothstep(v,0,1));
          const crown=shoulderPeaks[flatX<0?0:1];
          const apex=(isFinite(crown.height)?crown.height:shoulderY+6)+.55-.13*(Math.abs(flatX)-11.5),zc=isFinite(crown.height)?crown.depth:shoulderZ;
          const yy=T.MathUtils.lerp(waist+24,apex,v*(1.35-.35*v));
          const blend=Math.cos(v*Math.PI/2),guess=zc+(start[1]-zc)*blend;
          const nativeZ=shoulderDepth(xx,yy-.35*v,isFront),sign=isFront?1:-1;
          const envelope=isFinite(nativeZ)?nativeZ+sign*(.42+.2*v):guess;
          let zz=isFront?Math.max(envelope,guess):Math.min(envelope,guess);
          zz=T.MathUtils.lerp(zz,zc,T.MathUtils.smoothstep(v,.985,1));
          const trim=mesh.userData.binding?.11:0;
          p.setXYZ(i,xx+trim*Math.sin(a),yy+trim*v,zz+trim*sign*blend);
        }else{
          const [sx,sz]=onBodice(a,y),cos=Math.cos(a),sin=Math.sin(a);
          p.setXYZ(i,sx+rx*(radial-1)*sin,waist+y,sz+rz*(radial-1)*cos);
        }
      }
      p.needsUpdate=true;mesh.geometry.computeVertexNormals();mesh.geometry.computeBoundingSphere();
    }
    return waist-dress.skirtLength;
  };
}
