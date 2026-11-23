"use client";

import { publishedGeometry } from "../replayAsset";
import { capturePixelRatio } from "../atelier3d/capture-resolution";
import {useEffect,useRef} from "react";
import * as T from "three";
import {OrbitControls} from "three/addons/controls/OrbitControls.js";
import {finishEdges,linenCover} from "./textiles";
import {ClothShell} from "../atelier3d/cloth";
import {fullControls} from "../atelier3d/viewControls";
import {cuttingTable,sewingFrame,type SewingPhases} from "../construction/replay";

export type Surface="Check"|"Linen"|"Print"|"Panels"|"Strain"|"Hems";
type Topology={tri:number[];uvMm:number[];panels:number[][];seams:number[][];joins:{side_a:number[];side_b:number[]}[];waist:number[][];hem:number[][];frames:number[];phases:SewingPhases};
type Bust={xyz:number[];tri:number[];capStart:number;stitches:number[];lowerXYZ:number[];lowerTri:number[]};
type Props={cut:string;waist:number;patternStem:string;surface:Surface;step:number;detail:boolean;dressForm:boolean;alternateHem?:number[];ready:(key:string)=>void;fail:(key:string)=>void};
declare global {interface Window {ftxSkirtCamera?:(angle:number)=>void}}

async function asset(path:string,signal:AbortSignal){
  const response=await publishedGeometry("/atelier/"+path,signal);
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

export default function Scene({cut,waist,patternStem,surface,step,detail,dressForm,alternateHem,ready,fail}:Props){
  const host=useRef<HTMLDivElement>(null);
  const current=useRef({surface,step});const refresh=useRef<null|(()=>void)>(null);
  const closeup=useRef(detail),reframe=useRef<null|(()=>void)>(null);
  useEffect(()=>{closeup.current=detail;reframe.current?.();},[detail]);
  useEffect(()=>{current.current={surface,step};refresh.current?.();},[surface,step]);
  useEffect(()=>{
    const el=host.current!,key=cut+"-"+waist,abort=new AbortController();
    let renderer:T.WebGLRenderer;
    try{renderer=new T.WebGLRenderer({alpha:true,antialias:true,powerPreference:"low-power"});}catch{fail(key);return;}
    renderer.setPixelRatio(capturePixelRatio(2));renderer.setClearColor(0,0);
    renderer.toneMapping=T.ACESFilmicToneMapping;renderer.toneMappingExposure=1.05;
    renderer.shadowMap.enabled=true;renderer.shadowMap.type=T.PCFSoftShadowMap;
    const canvas=renderer.domElement;el.append(canvas);canvas.tabIndex=0;
    canvas.setAttribute("role","img");canvas.setAttribute("aria-label","Six-gore skirt on a fitted adult dress form. Drag or use the arrow keys to turn it, right-drag or Shift with the arrow keys to move it, scroll, pinch or use plus and minus to zoom. Home resets the view.");
    const scene=new T.Scene(),camera=new T.OrthographicCamera(-1,1,1,-1,.01,10);
    const controls=new OrbitControls(camera,canvas);controls.minZoom=.7;controls.maxZoom=4;
    controls.minPolarAngle=.5;controls.maxPolarAngle=1.7;controls.target.set(0,dressForm?-.035:-.205,0); // Never from under the floor.
    // The point the camera was last aimed at for the frame on show; the visitor moves the view from there.
    const aim=controls.target.clone();
    const cameraAt=(angle:number)=>{camera.position.set(controls.target.x+Math.sin(angle)*2,controls.target.y+.10,controls.target.z+Math.cos(angle)*2);controls.update();};
    cameraAt(.33);
    const paint=()=>renderer.render(scene,camera);
    controls.addEventListener("change",paint);
    const home=()=>{camera.zoom=1;camera.updateProjectionMatrix();controls.target.copy(aim);cameraAt(.33);};
    const look=fullControls(controls,canvas,{paint,home,turnKeys:true});
    let board:T.Box3|null=null;
    const resize=()=>{const w=el.clientWidth,h=el.clientHeight;if(!w||!h)return;
      const a=w/h,normal=closeup.current?Math.max(.205,.23/a):dressForm?Math.max(.744,.34/a):Math.max(.432,.295/a);
      const span=board?.getSize(new T.Vector3());
      const half=span?Math.max(normal,span.y*.6,Math.hypot(span.x,span.z)*.58/a):normal;
      Object.assign(camera,{left:-half*a,right:half*a,top:half,bottom:-half});
      camera.updateProjectionMatrix();renderer.setSize(w,h,false);paint();};
    reframe.current=()=>{const focus=board?.getCenter(new T.Vector3())??new T.Vector3(0,closeup.current ? .068 : dressForm ? -.035 : -.205,0);
      // Follow the subject, keeping however far the visitor has moved and zoomed the view.
      const shift=focus.clone().sub(aim);camera.position.add(shift);controls.target.add(shift);aim.copy(focus);look.retarget(aim);controls.update();resize();};
    reframe.current();
    const observer=new ResizeObserver(resize);observer.observe(el);resize();
    scene.add(new T.HemisphereLight(0xfff7ee,0x797166,.82));
    const keyLight=new T.DirectionalLight(0xffecd9,2.6);keyLight.position.set(-1.75,1.65,1.1);
    keyLight.castShadow=true;keyLight.shadow.mapSize.set(1024,1024);
    Object.assign(keyLight.shadow.camera,{left:-.8,right:.8,top:1,bottom:-1,near:.1,far:4});
    keyLight.shadow.normalBias=.0015;keyLight.shadow.bias=-.0003;scene.add(keyLight);
    for(const [x,y,z,color,intensity] of [[1.1,1,-1.4,0xffdfbb,1.55],[.8,.4,1,0xd3e3eb,.45]]){
      const light=new T.DirectionalLight(color,intensity);light.position.set(x,y,z);scene.add(light);
    }
    const materials:T.Material[]=[],geometries:T.BufferGeometry[]=[],maps:T.Texture[]=[];
    const col=(color:number,metalness=0,roughness=.75)=>{
      const m=new T.MeshStandardMaterial({color,metalness,roughness,side:T.DoubleSide});materials.push(m);return m;};
    function mesh(geom:T.BufferGeometry,mat:T.Material|T.Material[]){
      geometries.push(geom);const obj=new T.Mesh(geom,mat);obj.receiveShadow=true;scene.add(obj);return obj;}
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
    Promise.all([asset(patternStem+"-mesh.json",signal).then(r=>r.json() as Promise<Topology>),
      asset("bust-"+waist+".json.gz",signal).then(r=>r.json() as Promise<Bust>),
      asset(key+".gz",signal).then(r=>r.arrayBuffer())]).then(([top,bust,data])=>{
      if(signal.aborted)return;
      const loading=new T.LoadingManager(),loader=new T.TextureLoader(loading);
      let textureFailed=false;
      loading.onError=()=>{textureFailed=true;if(!signal.aborted)fail(key);};
      loading.onLoad=()=>{if(!signal.aborted&&!textureFailed){paint();canvas.dataset.run=key;ready(key);}};
      const clothPrint=loader.load("/atelier/madder.webp");
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
        human.setIndex(bust.lowerTri);human.computeVertexNormals();mesh(human,col(0xc29d87,0,.87));
      }
      const neck=Array.from({length:bp.length/3},(_,i)=>i).filter(i=>Math.abs(bp[i*3+1]-.57)<.00015);
      const neckZ=neck.reduce((s,i)=>s+bp[i*3+2],0)/Math.max(1,neck.length);
      turned([[.043,.569],[.046,.573],[.046,.578],[.043,.582],[.026,.583],[.022,.589],[.016,.593],[.014,.604],[.02,.608],[.021,.615],[.019,.623],[.013,.629],[.001,.633]],wood,neckZ);
      turned([[.044,.575],[.0468,.576],[.0468,.578],[.044,.579]],brass,neckZ);
      const displayForm=scene.children.filter(o=>o.visible&&(o instanceof T.Mesh||o instanceof T.LineSegments));
      const n=top.uvMm.length/2,k=top.frames.length,frameBytes=n*3*2;
      if(data.byteLength!==frameBytes*k+n*3)throw Error("Skirt frames do not match the pattern");
      const source=new Int16Array(data,0,n*3*k),color=new Uint8Array(data,frameBytes*k);
      const arrange=cuttingTable(positions(source,n),top.panels);
      const grain=loader.load("/materials/linen-normal.webp");
      grain.wrapS=grain.wrapT=T.RepeatWrapping;grain.repeat.set(410/270.7,410/271.3);grain.anisotropy=8;maps.push(grain);
      const flax=loader.load("/materials/linen.webp");
      flax.colorSpace=T.SRGBColorSpace;flax.wrapS=flax.wrapT=T.RepeatWrapping;flax.repeat.copy(grain.repeat);flax.anisotropy=8;maps.push(flax);
      const print=new T.MeshPhysicalMaterial({map:clothPrint,normalMap:grain,normalScale:new T.Vector2(.48,.48),roughness:.96,
        sheen:.25,sheenColor:0xdeab96,sheenRoughness:.89});
      const reverse=print.clone();reverse.color.set(0xcda996);materials.push(print,reverse);
      const dyed=print.clone();dyed.map=flax;dyed.color.set(0xd3d4cb);dyed.normalScale.set(.33,.33);
      dyed.sheen=.19;dyed.sheenColor.set(0xaab4bc);const lining=dyed.clone();lining.color.set(0xbfc0b9);materials.push(dyed,lining);
      const woven=loader.load("/atelier/glen-check.webp");
      woven.wrapS=woven.wrapT=T.MirroredRepeatWrapping;woven.repeat.setScalar(410/160);woven.colorSpace=T.SRGBColorSpace;woven.anisotropy=8;maps.push(woven);
      const check=dyed.clone();check.color.set(0xffffff);check.map=woven;materials.push(check);
      for(const face of [dyed,lining,print,reverse,check])finishEdges(face);
      const shell=new ClothShell({vertices:n,triangles:top.tri,seams:top.joins},0,positions(source,n,n*3*(k-1)),0);
      shell.attribute("uv",top.uvMm.map(x=>x/410),2);
      const waistDistance=new Float32Array(n);
      for(const [start,end] of top.panels){
        const edge=top.waist.find(e=>e[0]>=start&&e[0]<end)!;
        for(let i=start;i<end;i++){
          const x=top.uvMm[2*i],y=top.uvMm[2*i+1];let best=Infinity;
          for(let j=1;j<edge.length;j++){
            const a=2*edge[j-1],b=2*edge[j],dx=top.uvMm[b]-top.uvMm[a],dy=top.uvMm[b+1]-top.uvMm[a+1];
            const t=T.MathUtils.clamp(((x-top.uvMm[a])*dx+(y-top.uvMm[a+1])*dy)/(dx*dx+dy*dy||1),0,1);
            best=Math.min(best,Math.hypot(x-top.uvMm[a]-t*dx,y-top.uvMm[a+1]-t*dy));
          }
          waistDistance[i]=best;
        }
      }
      shell.attribute("waistDistance",waistDistance,1);
      const printedFace=[print,reverse,reverse],plainFace=[dyed,lining,lining],checkFace=[check,check,check];
      const finished=mesh(shell.geometry,plainFace);finished.frustumCulled=false;finished.castShadow=true;
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
        const {surface,step}=current.current,frame=sewingFrame(top.frames,top.phases,step);
        if(frame!==prev){
          const xyz=positions(source,n,n*3*frame);
          const tracking=top.frames[frame]<top.phases.fold;
          if(tracking)arrange(xyz);
          shell.update(xyz);clothXYZ.array.set(xyz);clothXYZ.needsUpdate=true;geom.computeVertexNormals();geom.computeBoundingSphere();
          for(const obj of displayForm)obj.visible=!tracking;
          if(tracking||board){board=tracking?new T.Box3().setFromBufferAttribute(clothXYZ):null;reframe.current?.();}
          canvas.dataset.nativeFrame=String(top.frames[frame]);
          edgePairs.forEach((v,i)=>seamXYZ.setXYZ(i,xyz[3*v],xyz[3*v+1],xyz[3*v+2]));
          bound.forEach((v,i)=>hemXYZ.setXYZ(i,xyz[3*v],xyz[3*v+1],xyz[3*v+2]));
          seamXYZ.needsUpdate=hemXYZ.needsUpdate=true;
          stitchGeometry.computeBoundingSphere();hemGeometry.computeBoundingSphere();prev=frame;
        }
        cloth.visible=surface==="Panels"||surface==="Strain";finished.visible=!cloth.visible;
        cloth.material=surface==="Panels"?panel:strain;
        finished.material=surface==="Print"?printedFace:surface==="Check"?checkFace:plainFace;
        other.visible=surface==="Hems";piping.visible=surface==="Print"||surface==="Hems";
        geom.setAttribute("color",surface==="Strain"?heat:dyes);seams.visible=surface==="Panels";
        canvas.dataset.surface=surface;canvas.dataset.step=String(step);paint();
      };
      refresh.current();
      if(new URLSearchParams(location.search).has("capture"))window.ftxSkirtCamera=cameraAt;
    }).catch(()=>{if(!signal.aborted)fail(key);});
    return ()=>{abort.abort();refresh.current=null;reframe.current=null;if(window.ftxSkirtCamera===cameraAt)delete window.ftxSkirtCamera;
      canvas.removeEventListener("webglcontextlost",lose);observer.disconnect();look.dispose();controls.dispose();
      geometries.forEach(g=>g.dispose());materials.forEach(m=>m.dispose());maps.forEach(t=>t.dispose());renderer.dispose();renderer.forceContextLoss();canvas.remove();};
  },[cut,waist,patternStem,dressForm,alternateHem,ready,fail]);
  return <div className="sa-canvas" ref={host}/>;
}
