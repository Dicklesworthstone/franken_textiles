"use client";

import { useEffect, useRef } from "react";
import * as T from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { createGarmentMesh, type Exhibit, type StudioMode } from "./garmentMesh";
import { formMaterial, lightStudio, prepareRenderer } from "./atelier3d/studio";
import { replayAsset } from "./replayAsset";

type Props = {
  run: string; strain: {min: number; max: number; p95: number};
  layoutIntegrity: string;
  timeline: Pick<Exhibit,"frames"|"sourceFrames"|"phases"> & {geometryIntegrity:string};
  frame: number; mode: StudioMode; reset: number;
  onReady: (run: string) => void; onError: (run: string) => void;
};
declare global {interface Window {ftxPoseCamera?: (yaw:number, y:number, half?:number, z?:number)=>void}}

async function readRun({run, strain, timeline, layoutIntegrity}: Props, signal: AbortSignal) {
  const unpack = async (r: Response) => {
    if (!r.body) throw Error("Empty garment replay.");
    return new Response(r.body.pipeThrough(new DecompressionStream("gzip"))).arrayBuffer();
  };
  const [topology, packed]: [Exhibit, ArrayBuffer] = await Promise.all([
    replayAsset("/exhibits/bolero.json", layoutIntegrity, signal).then(r => r.json()),
    replayAsset("/exhibits/poses/"+run+".bin.gz", timeline.geometryIntegrity, signal).then(unpack),
  ]);
  const layout = {...topology, frames:timeline.frames, sourceFrames:timeline.sourceFrames, phases:timeline.phases};
  const header = new DataView(packed);
  const vertices = header.getUint32(0,true), indices = header.getUint32(4,true);
  let offset = 8;
  function coordinates(length: number) {
    const points = new Float32Array(packed, offset, length);
    offset += 4 * length;
    return points;
  }
  const skin = coordinates(vertices*3);
  // The form turns about this Z-up body axis. Raised fingertips set the widest view.
  let turnRadius=0;
  for(let i=0;i<skin.length;i+=3){
    turnRadius=Math.max(turnRadius,Math.hypot(skin[i],-skin[i+1]-.065));
  }
  const triangles = new Uint32Array(packed, offset, indices);
  offset += indices * 4;
  const frames = coordinates(layout.frames.length * layout.vertices * 3);
  const forces = coordinates(layout.vertices);
  const rgb = new Uint8Array(packed, offset);
  if (rgb.length !== layout.vertices*3) throw Error("Incomplete garment colors.");
  const garment = createGarmentMesh({...layout, strainRgb:rgb, summary:{edgeStrainPercent:strain}}, frames, true, forces);
  const body = new T.BufferGeometry();
  body.setIndex(new T.BufferAttribute(triangles,1));
  body.setAttribute("position",new T.BufferAttribute(skin,3));
  body.rotateX(-Math.PI/2);
  body.computeVertexNormals();
  // Clip only the torso at the stand. Keep the hands intact, including lowered wrists.
  const torso:number[]=[],arms:number[]=[];
  for(let i=0;i<triangles.length;i+=3){
    const a=triangles[i],b=triangles[i+1],c=triangles[i+2];
    const atArm=Math.abs((skin[a*3]+skin[b*3]+skin[c*3])/3)>.15;
    (atArm?arms:torso).push(a,b,c);
  }
  body.setIndex([...torso,...arms]);
  body.addGroup(0,torso.length,0);body.addGroup(torso.length,arms.length,1);
  body.computeBoundingBox();
  return {body,garment,halfWidth:Math.max(.50,turnRadius*1.12),frames:layout.sourceFrames,dispose(){body.dispose();garment.dispose();}};
}

export default function DrapeRoomScene(props: Props) {
  const host = useRef<HTMLDivElement>(null);
  const latest = useRef(props);
  const draw = useRef<() => void>(()=>{});
  const change = useRef<() => void>(()=>{});
  useEffect(() => { latest.current=props; draw.current(); }, [props]);
  useEffect(() => {
    const el=host.current!;
    let renderer: T.WebGLRenderer;
    try {renderer=new T.WebGLRenderer({alpha:true,antialias:true,powerPreference:"low-power"});}
    catch {latest.current.onError(latest.current.run);return;}
    const canvas=renderer.domElement;
    canvas.tabIndex=0; canvas.setAttribute("role","img");
    canvas.setAttribute("aria-label","Bolero on its fitted Anny form. Drag or use left and right arrows to turn. Pinch or use plus and minus to zoom. Home resets the camera.");
    el.append(canvas);
    renderer.setClearColor(0,0);
    prepareRenderer(renderer); renderer.localClippingEnabled=true;
    const scene=new T.Scene();
    const studio=lightStudio(renderer,scene,"warm");
    const camera=new T.OrthographicCamera(-1,1,1,-1,.02,6);
    const controls=new OrbitControls(camera,canvas);
    canvas.style.touchAction="pan-y";
    controls.enablePan=false;controls.enableZoom=false; // A wheel over the landing page still scrolls.
    controls.minPolarAngle=.65;controls.maxPolarAngle=1.73;
    controls.touches.ONE=T.TOUCH.ROTATE;
    // The body is a tailor's form in the fitted body's own shape. Nothing is painted onto it.
    const plaster=formMaterial(undefined,{inset:.0025});
    const bust=plaster.clone();
    bust.onBeforeCompile=plaster.onBeforeCompile;bust.customProgramCacheKey=plaster.customProgramCacheKey;
    bust.clippingPlanes=[new T.Plane(new T.Vector3(0,1,0),-.033)];bust.clipShadows=true;
    const form=new T.Mesh(new T.BufferGeometry(),[bust,plaster]);
    const empty=form.geometry;
    form.castShadow=form.receiveShadow=true;scene.add(form);
    const brass=new T.MeshStandardMaterial({color:"#a08255",metalness:.5,roughness:.42});
    const lacquer=new T.MeshStandardMaterial({color:"#423b34",roughness:.52});
    const pedestal=new T.Group();scene.add(pedestal);
    function stand(top:number,bottom:number,height:number,y:number,material:T.Material,zScale=1){
      const mesh=new T.Mesh(new T.CylinderGeometry(top,bottom,height,80),material);
      mesh.position.set(0,y,.061);mesh.scale.z=zScale;mesh.castShadow=true;mesh.receiveShadow=true;pedestal.add(mesh);
    }
    stand(.016,.018,.149,-.031,brass);stand(.095,.105,.014,-.104,lacquer,.78);
    stand(.102,.105,.003,-.112,brass,.78);
    const groundMaterial=new T.ShadowMaterial({opacity:.2});
    const ground=new T.Mesh(new T.PlaneGeometry(8,8),groundMaterial);
    ground.rotation.x=-Math.PI/2;ground.position.y=-.114;ground.receiveShadow=true;scene.add(ground);
    let current: Awaited<ReturnType<typeof readRun>> | undefined;
    let request: AbortController | undefined;
    let loaded="", reset=latest.current.reset;
    let w=1,h=1;
    function home(){
      camera.zoom=1;controls.target.set(0,.178,.065);
      camera.position.set(.55,.42,1.8);camera.updateProjectionMatrix();controls.update();
    }
    function render(){
      if(!loaded)return;
      renderer.render(scene,camera);
    }
    function update(){
      const {mode,frame,reset:nextReset,run}=latest.current;
      if(!current||loaded!==run)return;
      if(reset!==nextReset){reset=nextReset;home();}
      const t=mode==="strain"||mode==="contact"?1:frame/(current.frames-1);
      const opened=Math.max(0,1-t*2.2);
      const centerY=.177;
      camera.position.y+=centerY-controls.target.y; controls.target.y=centerY;
      const half=Math.max(.36,(current.halfWidth+opened*.23)/(w/h))+opened*.045;
      camera.top=half;camera.bottom=-half;camera.left=-half*w/h;camera.right=-camera.left;camera.updateProjectionMatrix();
      current.garment.pose(mode,t,false,w/h);
      form.visible=pedestal.visible=ground.visible=current.garment.sampledFrame>=87;
      canvas.dataset.nativeFrame=String(current.garment.sampledFrame);
      canvas.dataset.frame=String(Math.round(t*(current.frames-1)));canvas.dataset.mode=mode;
      canvas.dataset.run=loaded;render();
    }
    draw.current=update;
    change.current=()=>{
      const {run}=latest.current;
      request?.abort();const controller=new AbortController();request=controller;
      delete canvas.dataset.run;
      readRun(latest.current,controller.signal).then(result=>{
        if(controller.signal.aborted){result.dispose();return;}
        if(current){scene.remove(current.garment.group);current.dispose();}
        current=result;loaded=run;form.geometry=result.body;
        studio.frame(new T.Vector3(0,.2,.06),.42);
        scene.add(result.garment.group);update();
        latest.current.onReady(run);
      }).catch(()=>{if(!controller.signal.aborted)latest.current.onError(run);});
    };
    function resize(){w=el.clientWidth;h=el.clientHeight;if(w&&h){renderer.setSize(w,h,false);update();}}
    const observer=new ResizeObserver(resize);observer.observe(el);
    const capture=(yaw:number,y:number,half=Math.max(.36,(current?.halfWidth??.50)*h/w),z=.067)=>{
      camera.zoom=1;controls.target.set(0,y,z);
      camera.position.set(Math.sin(yaw)*1.8,y,z+Math.cos(yaw)*1.8);
      camera.top=half;camera.bottom=-half;camera.left=-half*w/h;camera.right=-camera.left;
      camera.updateProjectionMatrix();if(!controls.update())render();
    };
    if(new URLSearchParams(location.search).get("capture")==="pose-room")window.ftxPoseCamera=capture;
    function zoom(value:number){camera.zoom=T.MathUtils.clamp(value,.7,2.8);camera.updateProjectionMatrix();render();}
    function keys(e:KeyboardEvent){
      if(e.ctrlKey||e.metaKey||e.altKey)return;
      if(e.key==="Home")home();
      else if(e.key==="+"||e.key==="=")zoom(camera.zoom*1.14);
      else if(e.key==="-")zoom(camera.zoom/1.14);
      else if(e.key==="ArrowLeft"||e.key==="ArrowRight"){
        const relative=camera.position.clone().sub(controls.target);
        relative.applyAxisAngle(new T.Vector3(0,1,0),e.key==="ArrowLeft"?-.14:.14);
        camera.position.copy(relative).add(controls.target);controls.update();
      }else return;
      e.preventDefault();render();
    }
    let pinch=0,startZoom=1;
    const span=(e:TouchEvent)=>Math.hypot(e.touches[0].clientX-e.touches[1].clientX,e.touches[0].clientY-e.touches[1].clientY);
    const start=(e:TouchEvent)=>{if(e.touches.length===2){pinch=span(e);startZoom=camera.zoom;}};
    const move=(e:TouchEvent)=>{if(e.touches.length===2&&pinch)zoom(startZoom*span(e)/pinch);};
    const end=()=>{pinch=0;};
    const lost=(e:Event)=>{e.preventDefault();latest.current.onError(latest.current.run);};
    controls.addEventListener("change",render);
    canvas.addEventListener("keydown",keys);canvas.addEventListener("dblclick",home);canvas.addEventListener("webglcontextlost",lost);
    canvas.addEventListener("touchstart",start,{passive:true});canvas.addEventListener("touchmove",move,{passive:true});canvas.addEventListener("touchend",end);
    home();resize();
    return ()=>{
      request?.abort();observer.disconnect();controls.removeEventListener("change",render);controls.dispose();
      canvas.removeEventListener("keydown",keys);canvas.removeEventListener("dblclick",home);canvas.removeEventListener("webglcontextlost",lost);
      canvas.removeEventListener("touchstart",start);canvas.removeEventListener("touchmove",move);canvas.removeEventListener("touchend",end);
      current?.dispose();empty.dispose();bust.dispose();plaster.dispose();ground.geometry.dispose();groundMaterial.dispose();
      pedestal.children.forEach(m=>(m as T.Mesh).geometry.dispose());brass.dispose();lacquer.dispose();studio.dispose();
      renderer.dispose();renderer.forceContextLoss();canvas.remove();draw.current=()=>{};change.current=()=>{};
      if(window.ftxPoseCamera===capture)delete window.ftxPoseCamera;
    };
  },[]);
  useEffect(()=>{change.current();},[props.run]);
  return <div className="dr-canvas" ref={host}/>;
}
