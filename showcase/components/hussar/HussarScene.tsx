"use client";

import { useEffect, useRef } from "react";
import * as T from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";
import { buildUniform, type Framing, type Layer, type Regiment, type RegimentView } from "./uniform";

type Settings = {regiment:RegimentView;layer:Layer;framing:{preset:Framing}};
declare global {interface Window {ftxHussarFrame?: (yaw:number,target:number,span:number)=>void}}
export default function HussarScene({regiment,layer,framing,onFailure}:{onFailure:()=>void}&Settings){
  const host=useRef<HTMLDivElement>(null), apply=useRef<((s:Settings)=>void)|null>(null);
  useEffect(()=>{
    const surface=host.current!;
    let renderer:T.WebGLRenderer;
    try {renderer=new T.WebGLRenderer({alpha:true,antialias:true,powerPreference:"low-power"});}
    catch {onFailure();return;}
    renderer.setPixelRatio(Math.min(2,devicePixelRatio));renderer.toneMapping=T.ACESFilmicToneMapping;renderer.toneMappingExposure=.99;
    const el=renderer.domElement;el.tabIndex=0;el.setAttribute("role","img");
    el.setAttribute("aria-label","Swedish hussar garment model. Drag or use arrow keys to orbit, and plus or minus to zoom.");surface.appendChild(el);
    const scene=new T.Scene(), camera=new T.OrthographicCamera(-110,110,110,-110,1,1800);
    const env=new T.PMREMGenerator(renderer), room=new RoomEnvironment(), lights=env.fromScene(room,.05);
    room.dispose();env.dispose();scene.environment=lights.texture;scene.environmentIntensity=.53;
    scene.add(new T.HemisphereLight(0xffeade,0x5c6b74,1.7));
    const light=(color:number,amount:number,x:number,y:number,z:number)=>{const l=new T.DirectionalLight(color,amount);l.position.set(x,y,z);scene.add(l);};
    light(0xffe6c8,2,-90,245,144);light(0xe7eefc,1.2,110,180,-105);
    const plinth=new T.Mesh(new T.CylinderGeometry(41,43,3.8,72),new T.MeshStandardMaterial({color:0x3b423d,roughness:.83}));plinth.position.y=-1.85;scene.add(plinth);
    const rim=new T.Mesh(new T.CylinderGeometry(42.8,43,1.15,72),new T.MeshStandardMaterial({color:0x927344,metalness:.72,roughness:.4}));rim.position.y=-3.65;scene.add(rim);
    const orbit=new OrbitControls(camera,el);orbit.enablePan=false;orbit.minZoom=.62;orbit.maxZoom=4;orbit.rotateSpeed=.72;
    let height=112,aspect=1,models:ReturnType<typeof buildUniform>[]=[],previous: Settings|undefined;
    function draw(){renderer.render(scene,camera);}
    function project(){camera.left=-height*aspect;camera.right=height*aspect;camera.top=height;camera.bottom=-height;camera.updateProjectionMatrix();}
    function orient(yaw:number,y:number,span:number,elevation=.075) {
      height=span;project();orbit.target.set(0,y,0);
      camera.position.set(360*Math.sin(yaw)*Math.cos(elevation),y+360*Math.sin(elevation),360*Math.cos(yaw)*Math.cos(elevation));
      orbit.update();draw();
    }
    const resize=()=>{const w=surface.clientWidth,h=surface.clientHeight;if(!w||!h)return;aspect=w/h;renderer.setSize(w,h,false);project();draw();};
    function change(next:Settings){
      const paired=next.regiment==="Compare";
      if(next.regiment!==previous?.regiment){
        models.forEach(m=>{scene.remove(m.outfit);m.dispose();});
        const choices:Regiment[]=next.regiment==="Compare"?["Blue","Yellow"]:[next.regiment];
        models=choices.map((choice,i)=>{
          const model=buildUniform(choice);scene.add(model.outfit);
          model.outfit.position.x=paired?(i===0?-41:41):0;
          return model;
        });
      }
      models.forEach(m=>m.setLayer(next.layer));plinth.visible=rim.visible=next.layer!=="Pelisse";
      plinth.scale.x=rim.scale.x=paired?2.2:1;
      const isolated = next.layer === "Pelisse";
      const detail = next.framing.preset === "Chest";
      const targetHeight = detail ? 125 : isolated ? 117 : 99;
      const preferredSpan = detail ? (isolated ? 28 : 41) : (isolated ? 42 : 109);
      const span = Math.max(preferredSpan, (paired ? 76 : 32) / aspect);
      const yaw = next.framing.preset === "Reverse" ? Math.PI - .28 : isolated ? .34 : -.27;
      camera.zoom = 1;
      orient(yaw, targetHeight, span);
      previous = next;
      el.dataset.ready = next.regiment + "-" + next.layer;
    }
    const keydown=(e:KeyboardEvent)=>{
      if(e.altKey||e.ctrlKey||e.metaKey)return;
      let a=orbit.getAzimuthalAngle(), b=Math.PI/2-orbit.getPolarAngle();
      switch(e.key){case "ArrowLeft":a-=.14;break;case "ArrowRight":a+=.14;break;case "ArrowUp":b+=.12;break;case "ArrowDown":b-=.12;break;
        case "+":case "=":camera.zoom=Math.min(orbit.maxZoom,camera.zoom*1.15);break;case "-":camera.zoom=Math.max(orbit.minZoom,camera.zoom/1.15);break;default:return;}
      e.preventDefault();orient(a,orbit.target.y,height,T.MathUtils.clamp(b,-1.3,1.3));
    };
    const failure=(e:Event)=>{e.preventDefault();onFailure();};
    const ro=new ResizeObserver(resize);ro.observe(surface);resize();orbit.addEventListener("change",draw);
    el.addEventListener("keydown",keydown);el.addEventListener("webglcontextlost",failure);
    apply.current=change;
    if(new URLSearchParams(location.search).has("capture"))window.ftxHussarFrame=orient;
    return ()=>{apply.current=null;if(window.ftxHussarFrame===orient)delete window.ftxHussarFrame;
      ro.disconnect();orbit.removeEventListener("change",draw);orbit.dispose();el.removeEventListener("keydown",keydown);el.removeEventListener("webglcontextlost",failure);
      models.forEach(m=>m.dispose());plinth.geometry.dispose();plinth.material.dispose();rim.geometry.dispose();rim.material.dispose();lights.dispose();renderer.dispose();renderer.forceContextLoss();el.remove();};
  },[onFailure]);
  useEffect(()=>{apply.current?.({regiment,layer,framing});},[regiment,layer,framing,onFailure]);
  return <div className="hu-scene" ref={host}/>;
}
