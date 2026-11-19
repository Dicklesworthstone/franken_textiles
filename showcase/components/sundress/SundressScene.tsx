"use client";
import { useEffect, useRef, useState } from "react";
import * as T from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { buildDress } from "./model";
import { registerDress } from "./registration";
import { styleFigure } from "./styling";
import { loadSundressBody } from "../fittedBody";
import { initial, type Arm, type Form, type Readings } from "./form";

export type View = "Full length" | "Bodice" | "Side seam";
type Props = {form:Form;view:View;onArm:(arm:Arm,angle:number)=>void;onError:()=>void;onReadings:(v:Readings)=>void};
declare global {
  interface Window { ftxDressFrame?: (form:Form, yaw:number, zoom?: number,targetY?:number,portrait?:boolean)=>Readings }
}

function heightRuler() {
  const canvas=document.createElement("canvas");canvas.width=140;canvas.height=1536;
  const ctx=canvas.getContext("2d")!;
  ctx.strokeStyle="#575f53";ctx.fillStyle="#414c43";ctx.font="500 36px monospace";
  ctx.textAlign="right";ctx.textBaseline="middle";ctx.lineWidth=2;
  for(let cm=0;cm<=190;cm+=2){
    const y=1514-cm*7.85;
    ctx.beginPath();ctx.moveTo(135-(cm%10===0?29:11),y);ctx.lineTo(135,y);ctx.stroke();
    if(cm%20===0&&cm>0)ctx.fillText(String(cm),99,y);
  }
  const map=new T.CanvasTexture(canvas);map.colorSpace=T.SRGBColorSpace;
  const material=new T.MeshBasicMaterial({map,transparent:true,depthWrite:false,side:T.DoubleSide});
  const plaque=new T.Mesh(new T.PlaneGeometry(17.8,195.65),material);
  plaque.position.set(-54,95.04,-14);return plaque;
}

export default function SundressScene({form,view,onArm,onError,onReadings}:Props){
  const host=useRef<HTMLDivElement>(null);
  const [native,setNative]=useState<Awaited<ReturnType<typeof loadSundressBody>>|null>(null);
  const latest=useRef({form,view,onArm,onError,onReadings}),redraw=useRef<(()=>void)|null>(null);
  useEffect(()=>{latest.current={form,view,onArm,onError,onReadings};redraw.current?.();},[form,view,onArm,onError,onReadings]);
  useEffect(()=>{
    let alive=true;loadSundressBody().then(body=>{if(alive)setNative(body);else body.geometry.dispose();}).catch(()=>{if(alive)latest.current.onError();});
    return ()=>{alive=false;};
  },[]);
  useEffect(()=>{
    if(!native)return;
    const surface=host.current!;
    let renderer:T.WebGLRenderer;
    try{renderer=new T.WebGLRenderer({antialias:true,alpha:true,powerPreference:"low-power",preserveDrawingBuffer:false});}
    catch{latest.current.onError();return;}
    renderer.setPixelRatio(Math.min(window.devicePixelRatio||1,1.65));renderer.outputColorSpace=T.SRGBColorSpace;
    renderer.toneMapping=T.ACESFilmicToneMapping;renderer.toneMappingExposure=.94;
    renderer.shadowMap.enabled=true;renderer.shadowMap.type=T.PCFSoftShadowMap;
    const el=renderer.domElement;surface.appendChild(el);el.tabIndex=0;
    el.setAttribute("role","img");el.setAttribute("aria-label","Fitted adult Anny model wearing a garden sundress. Drag an arm to raise it. Drag the background or press arrow keys to orbit. Use plus and minus to zoom.");
    const scene=new T.Scene(),camera=new T.OrthographicCamera(-100,100,100,-100,1,1800);
    camera.position.set(112,100,300);
    const controls=new OrbitControls(camera,el);
    controls.target.set(0,88,0);controls.enablePan=false;controls.minZoom=.68;controls.maxZoom=3;
    controls.minPolarAngle=Math.PI*.23;controls.maxPolarAngle=Math.PI*.65;controls.update();
    scene.add(new T.HemisphereLight(0xfff4e3,0xa9b6c4,1.83));
    const light=new T.DirectionalLight(0xfff0e2,2.08);light.position.set(-115,240,165);light.target.position.set(0,75,0);
    light.shadow.mapSize.set(2048,2048);light.castShadow=true;
    Object.assign(light.shadow.camera,{left:-120,right:120,top:135,bottom:-135,near:.1,far:650});light.shadow.camera.updateProjectionMatrix();
    light.shadow.bias=-.00012;light.shadow.normalBias=.05;scene.add(light,light.target);
    const rim=new T.DirectionalLight(0xffdbb4,1.65);rim.position.set(100,170,-165);scene.add(rim);
    const styled=styleFigure(native.geometry,native.landmarks),body=new T.Mesh(native.geometry,styled.materials);
    body.castShadow=body.receiveShadow=true;
    const garment=buildDress(),wrap=registerDress(garment,native.geometry,native.armAt);
    const ruler=heightRuler();scene.add(body,styled.head,garment,ruler);
    function fit(params:Form):Readings{
      const {landmarks,height,bust,waist,weight}=native!.apply(params);
      styled.apply(landmarks);return {height,bust,waist,weight,hem:wrap(landmarks)};
    }
    const plinth=new T.Mesh(new T.CylinderGeometry(32,33.25,2.25,80),new T.MeshStandardMaterial({color:0xb7a99a,roughness:.91}));
    plinth.position.y=-.6;plinth.receiveShadow=plinth.castShadow=true;scene.add(plinth);
    const floor=new T.Mesh(new T.PlaneGeometry(2400,2400),new T.ShadowMaterial({opacity:.19}));
    floor.rotation.x=-Math.PI/2;floor.position.y=-1.74;floor.receiveShadow=true;scene.add(floor);
    const paint=()=>renderer.render(scene,camera);
    const resize=()=>{const width=surface.clientWidth,height=surface.clientHeight;if(!width||!height)return;
      const a=width/height;const half=Math.max(99,63/a);camera.left=-half*a;camera.right=half*a;camera.top=half;camera.bottom=-half;
      renderer.setSize(width,height,false);camera.updateProjectionMatrix();paint();
    };
    let lastView:View|undefined;
    redraw.current=()=>{
      const {form,view}=latest.current;latest.current.onReadings(fit(form));
      if(lastView!==view){
        const base=view==="Bodice"?137:view==="Side seam"?115:89,delta=base-controls.target.y;
        controls.target.y+=delta;camera.position.y+=delta;camera.zoom=view==="Full length"?1:2.4;
        if(view==="Side seam")camera.position.set(-313,base+15,77);
        else if(lastView==="Side seam")camera.position.set(112,base+13,300);
        camera.updateProjectionMatrix();controls.update();lastView=view;
      }
      paint();el.dataset.ready="true";
    };
    const mode=(yaw:number,elevation:number)=>{
      const r=330;camera.position.set(r*Math.sin(yaw)*Math.cos(elevation),controls.target.y+r*Math.sin(elevation),r*Math.cos(yaw)*Math.cos(elevation));controls.update();paint();
    };
    const capture=(params:Form,yaw:number,zoom=1,targetY=89,portrait=false)=>{
      const readings=fit({...initial,...params});ruler.visible=!portrait;controls.target.y=targetY;camera.zoom=zoom;camera.updateProjectionMatrix();mode(yaw,portrait?.10:.055);return readings;
    };
    if(new URLSearchParams(location.search).has("capture"))window.ftxDressFrame=capture;
    const observer=new ResizeObserver(resize);observer.observe(surface);
    controls.addEventListener("change",paint);
    const pick=new T.Raycaster(),pointer=new T.Vector2();
    const hitArm=(e:PointerEvent):Arm|undefined=>{
      const r=el.getBoundingClientRect();pointer.set(2*(e.clientX-r.left)/r.width-1,1-2*(e.clientY-r.top)/r.height);pick.setFromCamera(pointer,camera);
      const hit=pick.intersectObjects([body,garment],true)[0];
      return hit?.object===body&&hit.faceIndex!=null?native.armAt(hit.faceIndex)??undefined:undefined;
    };
    let drag:{arm:Arm;x:number;y:number;angle:number;id:number}|null=null;
    const down=(e:PointerEvent)=>{
      if(e.button!==0||!e.isPrimary)return;const arm=hitArm(e);if(!arm)return;
      drag={arm,x:e.clientX,y:e.clientY,angle:latest.current.form[arm],id:e.pointerId};controls.enabled=false;el.setPointerCapture(e.pointerId);
      e.stopImmediatePropagation();e.preventDefault();el.style.cursor="ns-resize";
    };
    const move=(e:PointerEvent)=>{
      if(!drag){if(e.pointerType==="mouse"&&!e.buttons)el.style.cursor=hitArm(e)?"ns-resize":"grab";return;}
      if(e.pointerId!==drag.id)return;
      const side=drag.arm==="left"?1:-1;
      const angle=drag.angle+(drag.y-e.clientY)*140/el.clientHeight+(e.clientX-drag.x)*side*Math.cos(controls.getAzimuthalAngle())*90/el.clientHeight;
      latest.current.onArm(drag.arm,Math.round(T.MathUtils.clamp(angle,4,125)));
      e.preventDefault();
    };
    const up=(e:PointerEvent)=>{if(!drag||e.pointerId!==drag.id)return;drag=null;controls.enabled=true;
      if(el.hasPointerCapture(e.pointerId))el.releasePointerCapture(e.pointerId);el.style.cursor="grab";};
    const key=(e:KeyboardEvent)=>{
      if(e.altKey||e.metaKey||e.ctrlKey)return;let yaw=controls.getAzimuthalAngle(),elev=Math.PI/2-controls.getPolarAngle();
      switch(e.key){case "ArrowLeft":yaw-=.12;break;case "ArrowRight":yaw+=.12;break;case "ArrowUp":elev+=.08;break;case "ArrowDown":elev-=.08;break;
        case "+":case "=":camera.zoom=Math.min(3,camera.zoom*1.15);break;case "-":camera.zoom=Math.max(.68,camera.zoom/1.15);break;default:return;}
      e.preventDefault();camera.updateProjectionMatrix();mode(yaw,T.MathUtils.clamp(elev,-.38,.72));
    };
    const lost=(e:Event)=>{e.preventDefault();latest.current.onError();};
    el.addEventListener("pointerdown",down,true);el.addEventListener("pointermove",move);el.addEventListener("pointerup",up);
    el.addEventListener("pointercancel",up);el.addEventListener("lostpointercapture",up);el.addEventListener("keydown",key);el.addEventListener("webglcontextlost",lost);
    resize();redraw.current();
    return ()=>{
      redraw.current=null;if(window.ftxDressFrame===capture)delete window.ftxDressFrame;observer.disconnect();
      controls.removeEventListener("change",paint);controls.dispose();
      el.removeEventListener("pointerdown",down,true);el.removeEventListener("pointermove",move);el.removeEventListener("pointerup",up);el.removeEventListener("pointercancel",up);el.removeEventListener("lostpointercapture",up);
      el.removeEventListener("keydown",key);el.removeEventListener("webglcontextlost",lost);
      const resources=new Set<T.BufferGeometry|T.Material|T.Texture>();
      scene.traverse(o=>{if(o instanceof T.Mesh||o instanceof T.LineSegments){resources.add(o.geometry);for(const m of [o.material].flat()){resources.add(m);if(m.map)resources.add(m.map);}}});
      resources.forEach(r=>r.dispose());light.shadow.dispose();renderer.dispose();renderer.forceContextLoss();el.remove();
    };
  },[native]);
  return <div className="sd-scene" ref={host}>{!native&&<span className="sd-loading" role="status">Loading the fitted figure…</span>}</div>;
}
