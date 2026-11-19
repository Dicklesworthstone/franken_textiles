import * as T from "three";

export type Point3 = [number,number,number];
const turn = Math.PI*2;

export function seedRandom(seed:number) {
  let state=seed>>>0;
  return () => {state=(Math.imul(state,1664525)+1013904223)>>>0;return state/4294967296;};
}

/** Fulled broadcloth has a fine surface nap. Its warp and weft are largely concealed. */
export function broadclothTexture() {
  const size=384, canvas=document.createElement("canvas");
  canvas.width=canvas.height=size;
  const paint=canvas.getContext("2d")!, random=seedRandom(1761);
  const pixels=paint.createImageData(size,size);
  for(let y=0;y<size;y++)for(let x=0;x<size;x++){
    const d=(y*size+x)*4, weave=2*Math.sin((y+x*.29)*1.4);
    const shade=213+weave+16*random()+2*Math.sin(turn*y/size);
    pixels.data.set([shade,shade,shade,255],d);
  }
  paint.putImageData(pixels,0,0);
  for(let i=0;i<9500;i++){
    const x=random()*size, y=random()*size;
    paint.strokeStyle=i%4===0?"rgba(49,45,41,.13)":"rgba(255,251,239,.24)";
    paint.lineWidth=.32+random()*.55;
    paint.beginPath();paint.moveTo(x,y);
    paint.quadraticCurveTo(x+random()*3-1,y-2,x+random()*4-1.2,y-2-random()*4);
    paint.stroke();
  }
  const tex=new T.CanvasTexture(canvas);
  tex.wrapS=tex.wrapT=T.RepeatWrapping;tex.repeat.set(6,3);
  tex.colorSpace=T.SRGBColorSpace;tex.anisotropy=4;
  return tex;
}

/** A flattened soutache band with a midrib and two oppositely twisted strands. */
export function soutache(points:Point3[],halfWidth:number) {
  const curve=new T.CatmullRomCurve3(points.map(v=>new T.Vector3(...v)),false,"centripetal");
  const length=curve.getLength(), steps=Math.max(18,Math.ceil(length*5.5));
  const centers=curve.getSpacedPoints(steps), positions:number[]=[],uv:number[]=[],indices:number[]=[];
  const tangent=new T.Vector3(), normal=new T.Vector3(), across=new T.Vector3(), point=new T.Vector3();
  const sections=8;
  centers.forEach((center,i)=>{
    tangent.subVectors(centers[Math.min(steps,i+1)],centers[Math.max(0,i-1)]).normalize();
    normal.set(center.x*.032,0,1).normalize();
    across.crossVectors(tangent,normal).normalize();normal.crossVectors(across,tangent).normalize();
    const arc = i/steps*length;
    for(let j=0;j<=sections;j++){
      const a=j/sections*turn;
      const ribs=.80+.20*Math.cos(arc*12.5+Math.sin(a)*2.3);
      const depth=(.29+.19*Math.abs(Math.cos(a*2)))*Math.sin(a);
      point.copy(center).addScaledVector(across,Math.cos(a)*halfWidth*ribs).addScaledVector(normal,depth*halfWidth);
      positions.push(point.x,point.y,point.z);uv.push(arc*.055,j/sections);
      if(i<steps&&j<sections){
        const k=i*(sections+1)+j;
        indices.push(k,k+1,k+sections+1,k+1,k+sections+2,k+sections+1);
      }
    }
  });
  const g=new T.BufferGeometry();
  g.setAttribute("position",new T.Float32BufferAttribute(positions,3));
  g.setAttribute("uv",new T.Float32BufferAttribute(uv,2));g.setIndex(indices);g.computeVertexNormals();
  return g;
}

/** Fine tapered fibres follow a fur border or fill a busby without spherical bumps. */
export function napFibres(roots:T.Vector3[],length:number,seed:number,down:number) {
  const random=seedRandom(seed),v:number[]=[],colors:number[]=[];
  for (const root of roots) {
    const spread=.7+.6*random();
    const middle=root.clone().add(new T.Vector3((random()-.5)*length*.56,down*length*.43,(random()-.5)*length*.6));
    const tip=middle.clone().add(new T.Vector3((random()-.45)*length*.54,down*length*.45,(random()-.45)*length*.55));
    const shade=(.38+.62*random())*spread;
    v.push(...root.toArray(),...middle.toArray(),...middle.toArray(),...tip.toArray());
    for(const q of [.60,.84,.84,1.0])colors.push(shade*q,shade*q*.94,shade*q*.88);
  }
  const geo=new T.BufferGeometry();geo.setAttribute("position",new T.Float32BufferAttribute(v,3));
  geo.setAttribute("color",new T.Float32BufferAttribute(colors,3));
  return geo;
}

/** A soft, flat tapered feather. Barbs originate independently along the curved shaft. */
export function plumeGeometry(height:number,lean:number,seed:number) {
  const random=seedRandom(seed),pos:number[]=[],colors:number[]=[],indices:number[]=[];
  for(let i=0;i<1600;i++){
    const t=.015+.97*random();
    const root=new T.Vector3(lean*t*t,t*height,.4*Math.sin(Math.PI*t));
    const breadth=.16+2.76*Math.pow(t,.61)*(.65+.35*Math.sin(Math.PI*t));
    const angle=random()*turn, reach=breadth*(.46+.62*random());
    const end=root.clone().add(new T.Vector3(Math.cos(angle)*reach,.8+height*.11*t+random()*1.4,Math.sin(angle)*reach*.54));
    const rib=end.clone().sub(root).normalize();
    const normal=new T.Vector3(.4,0,1);
    const side=new T.Vector3().crossVectors(rib,normal).normalize().multiplyScalar(.028+.052*random());
    const k=pos.length/3, shade=.67+.26*random();
    for(const p of [root.clone().sub(side),root.clone().add(side),end])pos.push(...p.toArray());
    for(const f of [.89,.95,1])colors.push(shade*f,shade*f*.98,shade*f*.91);
    indices.push(k,k+1,k+2);
  }
  const g=new T.BufferGeometry();g.setAttribute("position",new T.Float32BufferAttribute(pos,3));
  g.setAttribute("color",new T.Float32BufferAttribute(colors,3));g.setIndex(indices);g.computeVertexNormals();
  return g;
}

/** Each leg blends into one half of the seat between the crotch and waist. */
export function buckskinBreeches(sign:number){
  const rows=[[79.7,7.03,7.59,9.48,0],[82.6,7.93,8.47,9.16,0],[85.6,8.39,9.03,8.84,0],
    [88.3,8.59,9.48,8.51,.11],[90.5,8.76,9.92,8.31,.40],[92.8,8.50,10.18,8.07,.78],
    [95.1,8.12,10.44,7.9,1],[98.4,7.9,10.13,7.75,1],[100.6,7.8,9.88,7.54,1]];
  const points:number[]=[],uv:number[]=[],indices:number[]=[],n=80;
  rows.forEach(([y,w,d,c,join],j)=>{
    for(let i=0;i<=n;i++){
      const u=i/n,a=u*turn, side=Math.sin(a), outer=Math.max(0,side);
      const x=sign*T.MathUtils.lerp(c+w*side,(w+c)*outer,join);
      const fold=(1-join)*Math.sin(4*a+.31*y)*.16;
      points.push(x,y,T.MathUtils.lerp(d,d*(1-.05*outer),join)*Math.cos(a)+.5+fold);
      uv.push(u,(y-79.7)/21);
      if(i<n&&j<rows.length-1){const k=j*(n+1)+i;indices.push(k,k+1,k+n+1,k+1,k+n+2,k+n+1);}
    }
  });
  if(sign<0)for(let i=0;i<indices.length;i+=3){const v=indices[i];indices[i]=indices[i+1];indices[i+1]=v;}
  const geom=new T.BufferGeometry();geom.setAttribute("position",new T.Float32BufferAttribute(points,3));
  geom.setAttribute("uv",new T.Float32BufferAttribute(uv,2));geom.setIndex(indices);geom.computeVertexNormals();
  return geom;
}

/** Lower the relaxed forearm forward from the elbow, placing the glove and hand in the same pose. */
export function restArm(x:number,y:number,z:number,sign:number):Point3 {
  const t=1-T.MathUtils.smoothstep(y,84,113);
  const angle=-(sign<0?.25:.13)*t, elbow=112;
  return [x-sign*(sign<0?1.65:.4)*t,elbow+(y-elbow)*Math.cos(angle)-z*Math.sin(angle),
    z*Math.cos(angle)+(y-elbow)*Math.sin(angle)];
}
