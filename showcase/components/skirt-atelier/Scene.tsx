"use client";
import {useEffect,useRef} from "react";
import * as T from "three";
import {OrbitControls} from "three/addons/controls/OrbitControls.js";
import {linenCover} from "./textiles";
import {ClothShell} from "../atelier3d/cloth";

export type Surface="Print"|"Panels"|"Strain"|"Hems";
type Topology={tri:number[];uvMm:number[];panels:number[][];seams:number[][];joins:{side_a:number[];side_b:number[]}[];waist:number[][];hem:number[][];frames:number[]};
type Bust={xyz:number[];tri:number[];capStart:number;stitches:number[];lowerXYZ:number[];lowerTri:number[]};
type Props={cut:string;waist:number;surface:Surface;step:number;dressForm:boolean;alternateHem?:number[];ready:(key:string)=>void;fail:(key:string)=>void};
declare global {interface Window {ftxSkirtCamera?:(angle:number)=>void}}

async function asset(path:string,signal:AbortSignal){
  const response=await fetch("/atelier/"+path,{signal});
  if(!response.ok||!response.body)throw Error("Missing atelier asset "+path);
  if(path.endsWith(".gz")){
    const bytes=new Uint8Array(await response.arrayBuffer());
    if(bytes[0]===31&&bytes[1]===139)return new Response(new Blob([bytes]).stream().pipeThrough(new DecompressionStream("gzip")));
    return new Response(bytes);
  }
  return response;
}
function positions(source:ArrayLike<number>,count:number,offset=0){
  const result=new Float32Array(count*3);
  for(let i=0;i<count*3;i+=3){
    result[i]=source[offset+i]/10000;result[i+1]=source[offset+i+2]/10000;result[i+2]=-source[offset+i+1]/10000-.10;
  }
  return result;
}

export default function Scene({cut,waist,surface,step,dressForm,alternateHem,ready,fail}:Props){
  const host=useRef<HTMLDivElement>(null);
  const current=useRef({surface,step});const refresh=useRef<null|(()=>void)>(null);
  useEffect(()=>{current.current={surface,step};refresh.current?.();},[surface,step]);
  useEffect(()=>{
    const el=host.current!,key=cut+"-"+waist,abort=new AbortController();
    let renderer:T.WebGLRenderer;
    try{renderer=new T.WebGLRenderer({alpha:true,antialias:true,powerPreference:"low-power"});}catch{fail(key);return;}
    renderer.setPixelRatio(Math.min(devicePixelRatio,1.65));renderer.setClearColor(0,0);
    renderer.toneMapping=T.ACESFilmicToneMapping;renderer.toneMappingExposure=1.18;
    const canvas=renderer.domElement;el.append(canvas);canvas.tabIndex=0;
    canvas.setAttribute("role","img");canvas.setAttribute("aria-label","Six-gore skirt on a fitted adult dress form. Drag or use the arrow keys to turn the form.");
    const scene=new T.Scene(),camera=new T.OrthographicCamera(-1,1,1,-1,.01,10);
    const controls=new OrbitControls(camera,canvas);controls.enablePan=false;controls.enableZoom=false;
    controls.minPolarAngle=dressForm?1.16:1.44;controls.maxPolarAngle=dressForm?1.7:1.60;controls.target.set(0,dressForm?-.035:-.205,0);
    const cameraAt=(angle:number)=>{camera.position.set(Math.sin(angle)*2,controls.target.y+.10,Math.cos(angle)*2);controls.update();};
    cameraAt(.33);
    const paint=()=>renderer.render(scene,camera);
    controls.addEventListener("change",paint);
    const keydown=(e:KeyboardEvent)=>{if(e.key==="ArrowLeft"||e.key==="ArrowRight"){e.preventDefault();cameraAt(controls.getAzimuthalAngle()+(e.key==="ArrowLeft"?-.15:.15));}};
    canvas.addEventListener("keydown",keydown);
    const resize=()=>{const w=el.clientWidth,h=el.clientHeight;if(!w||!h)return;
      const a=w/h,half=dressForm?Math.max(.744,.34/a):Math.max(.432,.295/a);Object.assign(camera,{left:-half*a,right:half*a,top:half,bottom:-half});
      camera.updateProjectionMatrix();renderer.setSize(w,h,false);paint();};
    const observer=new ResizeObserver(resize);observer.observe(el);resize();
    scene.add(new T.HemisphereLight(0xffedd1,0x615442,1.5));
    for(const [x,y,z,color,intensity] of [[1.3,1.6,-.65,0xffdfa3,3.2],[-1,1,1.4,0xffe5c6,2.2],[1,.35,1,0xc9e5d8,.65]]){
      const light=new T.DirectionalLight(color,intensity);light.position.set(x,y,z);scene.add(light);
    }
    const materials:T.Material[]=[],geometries:T.BufferGeometry[]=[],maps:T.Texture[]=[];
    const col=(color:number,metalness=0,roughness=.75)=>{
      const m=new T.MeshStandardMaterial({color,metalness,roughness,side:T.DoubleSide});materials.push(m);return m;};
    function mesh(geom:T.BufferGeometry,mat:T.Material|T.Material[]){
      geometries.push(geom);const obj=new T.Mesh(geom,mat);scene.add(obj);return obj;}
    const wood=col(0x66432c,.12,.33),brass=col(0xca9c57,.63,.32);
    function turned(profile:number[][],mat:T.Material,z=0){
      const o=mesh(new T.LatheGeometry(profile.map(([r,h])=>new T.Vector2(r,h)),72),mat);o.position.z=z;o.visible=dressForm;return o;}
    turned([[.001,-.746],[.142,-.746],[.145,-.739],[.14,-.726],[.131,-.72],[.081,-.713],[.067,-.705],[.046,-.696],[.038,-.679],[.023,-.676],[.022,-.665],[.027,-.659],[.022,-.652],[.013,-.626],[.012,.04]],wood);
    turned([[.025,-.666],[.028,-.661],[.028,-.655],[.024,-.653]],brass);
    const shadowCanvas=document.createElement("canvas");shadowCanvas.width=shadowCanvas.height=96;
    const ctx=shadowCanvas.getContext("2d")!,grad=ctx.createRadialGradient(48,48,3,48,48,48);
    grad.addColorStop(0,"rgba(11,8,3,.80)");grad.addColorStop(.38,"rgba(11,8,3,.52)");grad.addColorStop(1,"rgba(11,8,3,0)");
    ctx.fillStyle=grad;ctx.fillRect(0,0,96,96);const shadowMap=new T.CanvasTexture(shadowCanvas);maps.push(shadowMap);
    const shMat=new T.MeshBasicMaterial({map:shadowMap,transparent:true,depthWrite:false});materials.push(shMat);
    const shadow=mesh(new T.PlaneGeometry(.84,.54),shMat);shadow.rotation.x=-Math.PI/2;shadow.position.set(-.09,-.747,.04);
    shadow.visible=dressForm;
    const lose=(e:Event)=>{e.preventDefault();fail(key);};canvas.addEventListener("webglcontextlost",lose);
    const signal=abort.signal;
    Promise.all([asset(cut+"-mesh.json",signal).then(r=>r.json() as Promise<Topology>),
      asset("bust-"+waist+".json.gz",signal).then(r=>r.json() as Promise<Bust>),
      asset(key+".gz",signal).then(r=>r.arrayBuffer())]).then(([top,bust,data])=>{
      if(signal.aborted)return;
      const clothPrint=new T.TextureLoader().load("/atelier/madder.webp",()=>{if(!signal.aborted)paint();});
      clothPrint.wrapS=clothPrint.wrapT=T.RepeatWrapping;clothPrint.colorSpace=T.SRGBColorSpace;clothPrint.anisotropy=8;
      const linen=linenCover();maps.push(clothPrint,linen);
      const bodyGeom=new T.BufferGeometry(),bp=positions(bust.xyz,bust.xyz.length/3);
      bodyGeom.setAttribute("position",new T.BufferAttribute(bp,3));
      const bodyUV=new Float32Array(bp.length/3*2);
      for(let i=0;i<bp.length/3;i++){bodyUV[2*i]=Math.atan2(bp[3*i],bp[3*i+2])*2;bodyUV[2*i+1]=bp[3*i+1]*12;}
      bodyGeom.setAttribute("uv",new T.BufferAttribute(bodyUV,2));bodyGeom.setIndex(bust.tri);bodyGeom.computeVertexNormals();
      bodyGeom.addGroup(0,bust.capStart,0);bodyGeom.addGroup(bust.capStart,bust.tri.length-bust.capStart,1);
      const canvasCloth=col(0xf5f2e5);canvasCloth.map=linen;mesh(bodyGeom,[canvasCloth,wood]).visible=dressForm;
      const twill=new T.LineBasicMaterial({color:0x978d76});materials.push(twill);
      const dart=new T.BufferGeometry();geometries.push(dart);
      dart.setAttribute("position",new T.BufferAttribute(positions(bust.stitches,bust.stitches.length/3),3));
      const stitches=new T.LineSegments(dart,twill);stitches.visible=dressForm;scene.add(stitches);
      if(!dressForm){
        const human=new T.BufferGeometry();
        human.setAttribute("position",new T.BufferAttribute(positions(bust.lowerXYZ,bust.lowerXYZ.length/3),3));
        human.setIndex(bust.lowerTri);human.computeVertexNormals();mesh(human,col(0x87573f,0,.79));
      }
      const neck=Array.from({length:bp.length/3},(_,i)=>i).filter(i=>Math.abs(bp[i*3+1]-.57)<.00015);
      const neckZ=neck.reduce((s,i)=>s+bp[i*3+2],0)/Math.max(1,neck.length);
      turned([[.043,.569],[.046,.573],[.046,.578],[.043,.582],[.026,.583],[.022,.589],[.016,.593],[.014,.604],[.02,.608],[.021,.615],[.019,.623],[.013,.629],[.001,.633]],wood,neckZ);
      turned([[.044,.575],[.0468,.576],[.0468,.578],[.044,.579]],brass,neckZ);
      const n=top.uvMm.length/2,k=top.frames.length,frameBytes=n*3*2;
      if(data.byteLength!==frameBytes*k+n*3)throw Error("Skirt frames do not match the pattern");
      const source=new Int16Array(data,0,n*3*k),color=new Uint8Array(data,frameBytes*k);
      const grain=new T.TextureLoader().load("/materials/linen-normal.webp",()=>{if(!signal.aborted)paint();});
      grain.wrapS=grain.wrapT=T.RepeatWrapping;grain.repeat.set(410/270.7,410/271.3);grain.anisotropy=8;maps.push(grain);
      const print=new T.MeshPhysicalMaterial({map:clothPrint,normalMap:grain,normalScale:new T.Vector2(.48,.48),roughness:.96,
        sheen:.25,sheenColor:0xdeab96,sheenRoughness:.89});
      const reverse=print.clone();reverse.color.set(0xcda996);materials.push(print,reverse);
      const shell=new ClothShell({vertices:n,triangles:top.tri,seams:top.joins},.00045,positions(source,n,n*3*(k-1)),1);
      shell.attribute("uv",top.uvMm.map(x=>x/410),2);
      const finished=mesh(shell.geometry,[print,reverse,reverse]);finished.frustumCulled=false;
      const panel=col(0xffffff);panel.vertexColors=true;
      const strain=new T.MeshBasicMaterial({vertexColors:true,side:T.DoubleSide});materials.push(strain);
      const geom=new T.BufferGeometry(),cloth=mesh(geom,print);
      geom.setIndex(top.tri);geom.setAttribute("uv",new T.Float32BufferAttribute(top.uvMm.map(x=>x/410),2));
      const palette=[0xb86a42,0xddb475,0x61918a,0x738ca4,0x8d6e8d,0xa4a569].map(x=>new T.Color(x));
      const panelRGB=new Float32Array(n*3);
      top.panels.forEach(([a,b],p)=>{for(let i=a;i<b;i++)palette[p].toArray(panelRGB,3*i);});
      const dyes=new T.BufferAttribute(panelRGB,3),heat=new T.BufferAttribute(color,3,true);
      const thread=new T.LineBasicMaterial({color:0xd1a576});materials.push(thread);
      const stitchGeometry=new T.BufferGeometry();geometries.push(stitchGeometry);
      const seams=new T.LineSegments(stitchGeometry,thread);scene.add(seams);
      const otherGeometry=new T.BufferGeometry();geometries.push(otherGeometry);
      const otherInk=new T.LineDashedMaterial({color:0xffe69c,dashSize:.012,gapSize:.006,depthTest:false});materials.push(otherInk);
      const other=new T.Line(otherGeometry,otherInk);
      if(alternateHem){
        const ring=[...alternateHem,...alternateHem.slice(0,3)];
        otherGeometry.setAttribute("position",new T.BufferAttribute(positions(ring,ring.length/3),3));other.computeLineDistances();scene.add(other);other.renderOrder=3;
      }
      const hemGeometry=new T.BufferGeometry();geometries.push(hemGeometry);
      const hemInk=new T.LineBasicMaterial({color:0x834a35});materials.push(hemInk);
      const piping=new T.LineSegments(hemGeometry,hemInk);scene.add(piping);
      const bound=[...top.waist,...top.hem].flatMap(e=>e.slice(1).flatMap((v,i)=>[e[i],v]));
      const edgePairs=[...top.waist,...top.hem,...top.seams].flatMap(edge=>edge.slice(1).flatMap((v,i)=>[edge[i],v]));
      const clothXYZ=new T.BufferAttribute(new Float32Array(n*3),3).setUsage(T.DynamicDrawUsage);
      const seamXYZ=new T.BufferAttribute(new Float32Array(edgePairs.length*3),3).setUsage(T.DynamicDrawUsage);
      const hemXYZ=new T.BufferAttribute(new Float32Array(bound.length*3),3).setUsage(T.DynamicDrawUsage);
      geom.setAttribute("position",clothXYZ);stitchGeometry.setAttribute("position",seamXYZ);hemGeometry.setAttribute("position",hemXYZ);
      let prev=-1;
      refresh.current=()=>{
        const {surface,step}=current.current,frame=Math.round((k-1)*step/100);
        if(frame!==prev){
          const xyz=positions(source,n,n*3*frame);
          shell.update(xyz);clothXYZ.array.set(xyz);clothXYZ.needsUpdate=true;geom.computeVertexNormals();geom.computeBoundingSphere();
          edgePairs.forEach((v,i)=>seamXYZ.setXYZ(i,xyz[3*v],xyz[3*v+1],xyz[3*v+2]));
          bound.forEach((v,i)=>hemXYZ.setXYZ(i,xyz[3*v],xyz[3*v+1],xyz[3*v+2]));
          seamXYZ.needsUpdate=hemXYZ.needsUpdate=true;
          stitchGeometry.computeBoundingSphere();hemGeometry.computeBoundingSphere();prev=frame;
        }
        cloth.visible=surface==="Panels"||surface==="Strain";finished.visible=!cloth.visible;
        cloth.material=surface==="Panels"?panel:strain;
        other.visible=surface==="Hems";piping.visible=surface==="Print"||surface==="Hems";
        geom.setAttribute("color",surface==="Strain"?heat:dyes);seams.visible=surface==="Panels";
        canvas.dataset.surface=surface;canvas.dataset.step=String(step);paint();
      };
      refresh.current();canvas.dataset.run=key;
      if(new URLSearchParams(location.search).has("capture"))window.ftxSkirtCamera=cameraAt;
      ready(key);
    }).catch(()=>{if(!signal.aborted)fail(key);});
    return ()=>{abort.abort();refresh.current=null;if(window.ftxSkirtCamera===cameraAt)delete window.ftxSkirtCamera;
      canvas.removeEventListener("webglcontextlost",lose);canvas.removeEventListener("keydown",keydown);observer.disconnect();controls.dispose();
      geometries.forEach(g=>g.dispose());materials.forEach(m=>m.dispose());maps.forEach(t=>t.dispose());renderer.dispose();renderer.forceContextLoss();canvas.remove();};
  },[cut,waist,dressForm,alternateHem,ready,fail]);
  return <div className="sa-canvas" ref={host}/>;
}
