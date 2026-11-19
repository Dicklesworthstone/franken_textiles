import * as THREE from "three";

type Girths = {height:number; bust:number; waist:number; weight:number};
type Axis = keyof Girths;
type Point = [number, number, number];
type Anchor = "headTop"|"neck"|"chin"|"waist"|"chest"|"hip"|"eyeLeft"|"eyeRight"|
  "shoulderLeft"|"shoulderRight"|"wristLeft"|"wristRight";
export type BodyLandmarks = Record<Anchor, Point>;
export type BodyControls = Omit<Girths,"weight"> & {weight?:number; left:number; right:number};
type Sample = {name:string; targets:Girths; outcomes:Girths; landmarks:BodyLandmarks[]};
type GeometryInfo = {vertices:number; indices:number; skinIndices:number};
type DressInfo = GeometryInfo & {base:Girths; domains:Record<Axis,[number,number]>; angles:number[]; samples:Sample[]};
type Part = "head"|"leftHand"|"rightHand";
type UniformInfo = {parts:Record<Part,GeometryInfo & {bounds:[Point,Point]}>; eyeLeft:Point; eyeRight:Point; eyeRadius:number};
type RawAsset = {metadata:unknown; bytes:ArrayBuffer; masks:Uint8Array};
const cache = new Map<string,Promise<RawAsset>>();

async function unpack(file:Response) {
  const packed = await file.arrayBuffer(), magic = new Uint8Array(packed,0,2);
  return magic[0]===31 && magic[1]===139
    ? new Response(new Blob([packed]).stream().pipeThrough(new DecompressionStream("gzip"))).arrayBuffer()
    : packed;
}

function loadAsset(name:string) {
  let request = cache.get(name);
  if (!request) {
    request = Promise.all([".json",".bin.gz",".surface.gz"].map(async (suffix) => {
      const response = await fetch("/fitted-bodies/"+name+suffix);
      if (!response.ok) throw new Error("Could not load fitted "+name+" body ("+response.status+").");
      return response;
    })).then(async ([description,file,skin]) => {
      const [metadata,bytes,regions] = await Promise.all([description.json(),unpack(file),unpack(skin)]);
      return {metadata,bytes,masks:new Uint8Array(regions)};
    }).catch(error => { cache.delete(name); throw error; });
    cache.set(name,request);
  }
  return request;
}

function surface(info:GeometryInfo, index:Uint16Array, coords:Float32Array) {
  const geometry = new THREE.BufferGeometry();
  geometry.setIndex(new THREE.BufferAttribute(index,1));
  geometry.setAttribute("position",new THREE.BufferAttribute(coords,3));
  geometry.addGroup(0,info.skinIndices,0);
  if(info.skinIndices<info.indices) geometry.addGroup(info.skinIndices, info.indices-info.skinIndices, 1);
  return geometry;
}

function pigment(geometry:THREE.BufferGeometry, masks:Uint8Array, rest:Float32Array) {
  if(masks.length*3!==rest.length*8)throw Error("Unexpected fitted surface mask size");
  const buffer=new THREE.InterleavedBuffer(masks,8);
  geometry.setAttribute("skinZones",new THREE.InterleavedBufferAttribute(buffer,4,0,true));
  geometry.setAttribute("skinDetail",new THREE.InterleavedBufferAttribute(buffer,4,4,true));
  geometry.setAttribute("skinRest",new THREE.BufferAttribute(rest,3));
}

export async function loadHussarBody() {
  const {metadata,bytes,masks} = await loadAsset("hussar");
  const data = metadata as UniformInfo;
  const geometries = {} as Record<Part,THREE.BufferGeometry>;
  const [min,max] = data.parts.head.bounds;
  const dx = (min[0]+max[0])/2, dz = (min[2]+max[2])/2;
  let offset = 0, maskOffset=0;
  for(const part of ["head","leftHand","rightHand"] as const) {
    const info = data.parts[part];
    const indices = new Uint16Array(bytes,offset,info.indices); offset += indices.byteLength;
    const raw = new Int16Array(bytes,offset,info.vertices*3); offset += raw.byteLength;
    const positions = Float32Array.from(raw,v => v*.01);
    const geometry = surface(info,indices.slice(),positions);
    if(part==="head") geometry.translate(-dx,0,-dz);
    pigment(geometry, masks.subarray(maskOffset, maskOffset+info.vertices*8), positions.slice());
    maskOffset+=info.vertices*8;
    geometry.computeVertexNormals();
    geometry.computeBoundingSphere();
    geometries[part] = geometry;
  }
  if(offset!==bytes.byteLength) throw new Error("Unexpected hussar geometry size");
  const eye = (p:Point):Point => [p[0]-dx,p[1],p[2]-dz];
  return {...geometries,eyeLeft:eye(data.eyeLeft),eyeRight:eye(data.eyeRight),eyeRadius:data.eyeRadius};
}

const axes:Axis[] = ["height","bust","waist","weight"];
const anchors:Anchor[] = ["headTop","neck","chin","waist","chest","hip","eyeLeft","eyeRight",
  "shoulderLeft","shoulderRight","wristLeft","wristRight"];

export async function loadSundressBody() {
  const {metadata,bytes,masks} = await loadAsset("sundress");
  const info = metadata as DressInfo;
  const index = new Uint16Array(bytes,0,info.indices);
  const regions = new Uint8Array(bytes,index.byteLength,info.vertices);
  const offset = index.byteLength+regions.byteLength+(regions.byteLength%2);
  const count = info.vertices*3, step = info.angles.length*count;
  // Every fitted sample has identical public surface ordering. Individual geometry instances
  // own their positions, cached compressed source samples are shared between them.
  const packed = new Int16Array(bytes,offset);
  if(packed.length!==step*info.samples.length) throw new Error("Unexpected sundress sample size");
  const baseline = packed.slice(0,step);
  for(let i=count;i<step;i++) baseline[i]+=baseline[i-count];
  const positions = new Float32Array(count);
  const geometry = surface(info,index.slice(),positions);
  pigment(geometry,masks,Float32Array.from(baseline.subarray(0,count),n=>n*.01));
  const attribute = geometry.getAttribute("position") as THREE.BufferAttribute;
  attribute.setUsage(THREE.DynamicDrawUsage);
  const sampleIDs = new Map(info.samples.map((s,i) => [s.name,i]));
  const landmarks = {} as BodyLandmarks;
  const base = info.samples[0];

  function pose(degrees:number) {
    const hi = info.angles.findIndex(a => a>=degrees);
    if(!Number.isFinite(degrees)||hi<0||degrees<info.angles[0]) throw new RangeError("Arm outside recorded pose range");
    const lo = Math.max(0,hi-1);
    return {lo,hi,u:lo===hi ? 0 : (degrees-info.angles[lo])/(info.angles[hi]-info.angles[lo])};
  }
  function apply(controls:BodyControls) {
    const active: {sample:number; weight:number}[] = [];
    for(const axis of axes) {
      const value=controls[axis] ?? info.base[axis], [low,high]=info.domains[axis], center=info.base[axis];
      if(!Number.isFinite(value)||value<low||value>high) throw new RangeError("Body control outside recorded "+axis+" range");
      if(value===center) continue;
      const lower = value<center;
      active.push({sample:sampleIDs.get(axis+(lower?"Low":"High"))!, weight:(value-center)/((lower?low:high)-center)});
    }
    const left = pose(controls.left), right = pose(controls.right);
    for(let i=0;i<count;i+=3) {
      const {hi,lo,u} = baseline[i]>0?left:right;
      const a=lo*count+i, b=hi*count+i;
      for(let k=0;k<3;k++) {
        let start=baseline[a+k], end=baseline[b+k];
        for(const {sample,weight} of active) {
          start+=packed[sample*step+a+k]*weight;
          end+=packed[sample*step+b+k]*weight;
        }
        positions[i+k]=.01*(start+(end-start)*u);
      }
    }
    attribute.needsUpdate=true;
    geometry.computeVertexNormals();
    geometry.computeBoundingSphere();
    geometry.computeBoundingBox();
    const mid = pose((controls.left+controls.right)*.5);
    for(const key of anchors) {
      const {hi,lo,u}=key.endsWith("Left")?left:key.endsWith("Right")?right:mid;
      const point:Point=[0,0,0];
      for(let k=0;k<3;k++) {
        const baseA=base.landmarks[lo][key][k],baseB=base.landmarks[hi][key][k];
        let a=baseA,b=baseB;
        for(const {sample,weight} of active) {
          const m=info.samples[sample].landmarks;
          a+=(m[lo][key][k]-baseA)*weight;
          b+=(m[hi][key][k]-baseB)*weight;
        }
        point[k]=a+(b-a)*u;
      }
      landmarks[key]=point;
    }
    const readings={...base.outcomes};
    for(const axis of axes) for(const {sample,weight} of active) {
      readings[axis]+=(info.samples[sample].outcomes[axis]-base.outcomes[axis])*weight;
    }
    return {...readings,landmarks};
  }
  function armAt(faceIndex:number):"left"|"right"|null {
    if (!Number.isInteger(faceIndex) || faceIndex<0 || faceIndex>=index.length/3) return null;
    let l=0,r=0;
    for(let i=faceIndex*3;i<faceIndex*3+3;i++){
      const group=regions[index[i]];
      if(group===1||group===3) l++;
      if(group===2||group===4) r++;
    }
    return l>=2?"left":r>=2?"right":null;
  }
  apply({...info.base,left:14,right:19});
  return {geometry,apply,armAt,landmarks,domains:info.domains};
}
