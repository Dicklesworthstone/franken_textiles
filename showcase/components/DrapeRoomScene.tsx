"use client";

import { useEffect, useRef } from "react";
import * as T from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { createGarmentMesh, type Exhibit, type StudioMode } from "./garmentMesh";
import { formMaterial, lightStudio, prepareRenderer } from "./atelier3d/studio";
import { fullControls } from "./atelier3d/viewControls";
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
  // A tailor's form has no head, as in the films and the posing room: it ends at the narrowest section of
  // the neck, under a turned cap. The height comes from this run's own skin (Z up, metres), searched within
  // 80 mm of the body's axis. No arm reaches that height in any of the nine runs, even at 90 degrees.
  let top=-Infinity;
  for(let i=2;i<skin.length;i+=3)top=Math.max(top,skin[i]);
  let neckZ=top-.16,narrowest=Infinity;
  for(let z=top-.20;z<top-.11;z+=.002){
    let half=0;
    for(let i=0;i<skin.length;i+=3)if(Math.abs(skin[i+2]-z)<.002&&Math.abs(skin[i])<.08)half=Math.max(half,Math.abs(skin[i]));
    if(half>0&&half<narrowest){narrowest=half;neckZ=z;}
  }
  const ring=new T.Box3(),point=new T.Vector3();
  for(let i=0;i<skin.length;i+=3)if(Math.abs(skin[i+2]-neckZ)<.006)ring.expandByPoint(point.set(skin[i],skin[i+2],-skin[i+1]));
  const neck={centre:ring.getCenter(new T.Vector3()),size:ring.getSize(new T.Vector3()),y:neckZ};
  // Read before the turn to Y up below, which rewrites `skin` in place.
  const head=new Uint8Array(skin.length/3);
  for(let v=0;v<head.length;v++)head[v]=skin[v*3+2]>neckZ+.006?1:0;
  const body = new T.BufferGeometry();
  body.setIndex(new T.BufferAttribute(triangles,1));
  body.setAttribute("position",new T.BufferAttribute(skin,3));
  body.rotateX(-Math.PI/2);
  body.computeVertexNormals();
  // Clip only the torso at the stand. Keep the hands intact, including lowered wrists.
  const torso:number[]=[],arms:number[]=[];
  for(let i=0;i<triangles.length;i+=3){
    const a=triangles[i],b=triangles[i+1],c=triangles[i+2];
    if(head[a]||head[b]||head[c])continue;
    const atArm=Math.abs((skin[a*3]+skin[b*3]+skin[c*3])/3)>.15;
    (atArm?arms:torso).push(a,b,c);
  }
  body.setIndex([...torso,...arms]);
  body.addGroup(0,torso.length,0);body.addGroup(torso.length,arms.length,1);
  body.computeBoundingBox();
  return {body,garment,neck,halfWidth:Math.max(.50,turnRadius*1.12),frames:layout.sourceFrames,dispose(){body.dispose();garment.dispose();}};
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
    canvas.setAttribute("aria-label","Bolero on its fitted Anny form. Drag or use the arrow keys to turn it, right-drag or Shift with the arrow keys to move it, scroll, pinch or use plus and minus to zoom. Home resets the camera.");
    el.append(canvas);
    renderer.setClearColor(0,0);
    prepareRenderer(renderer); renderer.localClippingEnabled=true;
    const scene=new T.Scene();
    const studio=lightStudio(renderer,scene,"warm");
    const camera=new T.OrthographicCamera(-1,1,1,-1,.02,6);
    const controls=new OrbitControls(camera,canvas);
    controls.minZoom=.7;controls.maxZoom=4;
    controls.minPolarAngle=.2;controls.maxPolarAngle=1.73; // From above, never from under the floor.
    // The body is a tailor's form in the fitted body's own shape. Nothing is painted onto it.
    const plaster=formMaterial(undefined,{inset:.0025});
    const bust=plaster.clone();
    bust.onBeforeCompile=plaster.onBeforeCompile;bust.customProgramCacheKey=plaster.customProgramCacheKey;
    bust.clippingPlanes=[new T.Plane(new T.Vector3(0,1,0),-.033)];bust.clipShadows=true;
    const form=new T.Mesh(new T.BufferGeometry(),[bust,plaster]);
    const empty=form.geometry;
    form.castShadow=form.receiveShadow=true;scene.add(form);
    // The turned cap over the cut neck, sized to each run's neck section (the posing room's cap).
    const walnut=new T.MeshStandardMaterial({color:"#4a3a31",roughness:.5});
    const cap=new T.Mesh(new T.CylinderGeometry(1,1.04,.017,64),walnut);
    cap.castShadow=cap.receiveShadow=true;cap.visible=false;scene.add(cap);
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
      // Centred on the headless form, from its base (y −0.114) to the neck cap (about 0.33).
      camera.zoom=1;controls.target.set(0,.135,.065);
      camera.position.set(.55,.378,1.8);camera.updateProjectionMatrix();controls.update();
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
      const half=Math.max(.36,(current.halfWidth+opened*.23)/(w/h))+opened*.045;
      camera.top=half;camera.bottom=-half;camera.left=-half*w/h;camera.right=-camera.left;camera.updateProjectionMatrix();
      current.garment.pose(mode,t,false,w/h);
      form.visible=pedestal.visible=ground.visible=cap.visible=current.garment.sampledFrame>=87;
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
        cap.scale.set(result.neck.size.x/2*1.05,1,result.neck.size.z/2*1.05);
        cap.position.set(result.neck.centre.x,result.neck.y+.0045,result.neck.centre.z);
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
    const lost=(e:Event)=>{e.preventDefault();latest.current.onError(latest.current.run);};
    controls.addEventListener("change",render);
    canvas.addEventListener("webglcontextlost",lost);
    home();
    // Turning, moving, zooming, the keys and the reset are the site's shared ones.
    const view=fullControls(controls,canvas,{paint:render,home,turnKeys:true});
    resize();
    return ()=>{
      request?.abort();observer.disconnect();view.dispose();controls.removeEventListener("change",render);controls.dispose();
      canvas.removeEventListener("webglcontextlost",lost);
      current?.dispose();empty.dispose();bust.dispose();plaster.dispose();ground.geometry.dispose();groundMaterial.dispose();
      pedestal.children.forEach(m=>(m as T.Mesh).geometry.dispose());brass.dispose();lacquer.dispose();studio.dispose();
      cap.geometry.dispose();walnut.dispose();
      renderer.dispose();renderer.forceContextLoss();canvas.remove();draw.current=()=>{};change.current=()=>{};
      if(window.ftxPoseCamera===capture)delete window.ftxPoseCamera;
    };
  },[]);
  useEffect(()=>{change.current();},[props.run]);
  return <div className="dr-canvas" ref={host}/>;
}
