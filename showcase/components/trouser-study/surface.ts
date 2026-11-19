const base = "/exhibits/trouser-study/";
export type Surface = { positions: Float32Array; indices: Uint16Array };
export type Cut = { path: string; left: number; right: number; front: number; back: number };
export type Layout = {bodyVertices: number; bodyIndices: number; clothVertices: number};
type GarmentLayout = {triangles: number[]; seams: Record<string, {side_a:number[]; side_b:number[]}>; panels?: Record<string,[number,number]>};

async function packed(name: string, signal: AbortSignal) {
  const response = await fetch(base+name, {signal});
  if (!response.ok || !response.body) throw Error("Missing study surface");
  return new Response(response.body.pipeThrough(new DecompressionStream("gzip"))).arrayBuffer();
}

export async function loadSewing(id:string,vertices:number,frames:number,signal:AbortSignal){
  const raw=new Int16Array(await packed(id+"-sewing.gz",signal));
  if(raw.length!==3*vertices*frames)throw Error("Incomplete trouser sewing replay");
  return raw;
}

export async function loadTopology(layout:Layout, signal:AbortSignal, key?:string) {
  const [buffer, garment] = await Promise.all([packed(key?"mesh-"+key+".gz":"mesh.gz", signal), fetch(key?base+key+"-topology.json":"/exhibits/trousers.json",{signal}).then(async r=>{
    if(!r.ok) throw Error("Missing trouser pattern");
    return await r.json() as GarmentLayout;
  })]);
  const body = new Uint16Array(buffer,0,layout.bodyIndices);
  // Each cloth vertex's place on its flat cut piece, in metres. The weave and the seam finish are laid out in these.
  const flat = Float32Array.from(new Int16Array(buffer,body.byteLength),n=>n/10000);
  if(flat.length !== 2*layout.clothVertices) throw Error("Incomplete trouser grain coordinates");
  return {body, cloth:new Uint16Array(garment.triangles), flat,
    garment:{vertices:layout.clothVertices, triangles:garment.triangles, seams:Object.values(garment.seams), panels:garment.panels}};
}

export async function loadSurface(name:string, hasCloth:boolean, layout:Layout, signal:AbortSignal) {
  const buffer=await packed(name,signal);
  const n=layout.bodyVertices*3, m=hasCloth?layout.clothVertices*3:0;
  if(buffer.byteLength!==2*(n+m)+m) throw Error("Incomplete trouser study");
  // Right-handed native Z-up to WebGL Y-up, with the wearer's front facing +Z.
  function points(offset:number,count:number) {
    const view=new Int16Array(buffer,offset,count), out=new Float32Array(count);
    for(let i=0;i<count;i+=3) {out[i]=view[i]/10000;out[i+1]=view[i+2]/10000;out[i+2]=-view[i+1]/10000;}
    return out;
  }
  return {body:points(0,n),cloth:points(n*2,m),colors:new Uint8Array(buffer,(n+m)*2)};
}

/** Intersect indexed triangles with a horizontal plane. Half-open edges share one crossing at a vertex. */
export function section(surface:Surface,y:number):Cut|null {
  const p=surface.positions, ix=surface.indices;
  let path="", left=Infinity,right=-Infinity,front=-Infinity,back=Infinity;
  for(let face=0;face<ix.length;face+=3){
    const hits:number[]=[];
    for(let i=0;i<3;i++){
      const a=ix[face+i]*3, b=ix[face+(i+1)%3]*3;
      if((p[a+1]>y)===(p[b+1]>y)) continue;
      const t=(y-p[a+1])/(p[b+1]-p[a+1]);
      hits.push(1000*(p[a]+t*(p[b]-p[a])),1000*(p[a+2]+t*(p[b+2]-p[a+2])));
    }
    if(hits.length!==4 || (hits[0]===hits[2]&&hits[1]===hits[3]))continue;
    path+="M"+hits[0].toFixed(2)+" "+hits[1].toFixed(2)+"L"+hits[2].toFixed(2)+" "+hits[3].toFixed(2);
    left=Math.min(left,hits[0],hits[2]);right=Math.max(right,hits[0],hits[2]);
    back=Math.min(back,hits[1],hits[3]);front=Math.max(front,hits[1],hits[3]);
  }
  return path?{path,left,right,front,back}:null;
}
