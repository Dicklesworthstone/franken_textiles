"use client";

import { useEffect, useRef } from "react";
import * as T from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";
import { buildUniform, type Framing, type Layer, type Regiment, type RegimentView } from "./uniform";
import { loadHussarBody } from "../fittedBody";
import type { Complexion, FittedHussar } from "./portrait";
import {disposeGarmentMaps,loadGarmentMaps,type GarmentMaps} from "./garmentSurface";

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
    renderer.shadowMap.enabled=true;
    renderer.shadowMap.type=T.PCFSoftShadowMap;
    renderer.shadowMap.autoUpdate=false;
    const el=renderer.domElement;el.tabIndex=0;el.setAttribute("role","img");
    el.setAttribute("aria-label","Swedish hussar garment model. Drag or use arrow keys to orbit, and plus or minus to zoom.");surface.appendChild(el);
    const scene=new T.Scene(), camera=new T.OrthographicCamera(-110,110,110,-110,1,1800);
    const env=new T.PMREMGenerator(renderer), room=new RoomEnvironment(), lights=env.fromScene(room,.05);
    room.dispose();env.dispose();scene.environment=lights.texture;scene.environmentIntensity=.36;
    scene.add(new T.HemisphereLight(0xffeee2,0x596465,.76));
    const key = new T.DirectionalLight(0xffeadb,2.35);
    key.position.set(-147,241,139);
    key.target.position.y=115;
    key.castShadow=true;
    key.shadow.mapSize.set(2048,2048);
    Object.assign(key.shadow.camera,{left:-139,right:139,top:126,bottom:-126,near:35,far:460});
    key.shadow.camera.updateProjectionMatrix();
    key.shadow.normalBias=.08;
    key.shadow.bias=-.00016;
    const rimLight = new T.DirectionalLight(0xf6ecd0,1.7);
    rimLight.position.set(109,191,-112);
    rimLight.target.position.y=135;
    scene.add(key,key.target,rimLight,rimLight.target);
    const plinth=new T.Mesh(new T.CylinderGeometry(41,43,3.8,72),new T.MeshStandardMaterial({color:0x3b423d,roughness:.83}));plinth.position.y=-1.85;scene.add(plinth);
    const rim=new T.Mesh(new T.CylinderGeometry(42.8,43,1.15,72),new T.MeshStandardMaterial({color:0x927344,metalness:.72,roughness:.4}));rim.position.y=-3.65;scene.add(rim);
    plinth.receiveShadow=true;
    const floor=new T.Mesh(new T.PlaneGeometry(1100,1100),new T.ShadowMaterial({color:0x111712,opacity:.24}));
    floor.rotation.x=-Math.PI/2;floor.position.y=-4.28;floor.receiveShadow=true;scene.add(floor);
    const contactCanvas=document.createElement("canvas");contactCanvas.width=contactCanvas.height=96;
    const brush=contactCanvas.getContext("2d")!, contactFade=brush.createRadialGradient(48,48,9,48,48,47);
    contactFade.addColorStop(0,"rgba(7,11,9,.8)");contactFade.addColorStop(.5,"rgba(7,11,9,.46)");
    contactFade.addColorStop(1,"rgba(7,11,9,0)");brush.fillStyle=contactFade;brush.fillRect(0,0,96,96);
    const contactMap=new T.CanvasTexture(contactCanvas);
    const contactMaterial=new T.MeshBasicMaterial({map:contactMap,transparent:true,depthWrite:false});
    const contactGeometry=new T.PlaneGeometry(16.5,34);
    const contacts=new T.Group();scene.add(contacts);
    for(let i=0;i<4;i++){const patch=new T.Mesh(contactGeometry,contactMaterial);patch.rotation.x=-Math.PI/2;contacts.add(patch);}
    const orbit=new OrbitControls(camera,el);orbit.enablePan=false;orbit.minZoom=.62;orbit.maxZoom=4;orbit.rotateSpeed=.72;
    let height=112,aspect=1,models:ReturnType<typeof buildUniform>[]=[],previous: Settings|undefined;
    let mounted=true, pending:Settings|undefined, body:FittedHussar|undefined, complexion:Complexion|undefined, garmentMaps:GarmentMaps|undefined;
    function disposeFitted(source:FittedHussar){source.head.dispose();source.leftHand.dispose();source.rightHand.dispose();}
    function draw(){renderer.render(scene,camera);}
    function project(){camera.left=-height*aspect;camera.right=height*aspect;camera.top=height;camera.bottom=-height;camera.updateProjectionMatrix();}
    function orient(yaw:number,y:number,span:number,elevation=.075) {
      height=span;project();orbit.target.set(0,y,0);
      camera.position.set(360*Math.sin(yaw)*Math.cos(elevation),y+360*Math.sin(elevation),360*Math.cos(yaw)*Math.cos(elevation));
      orbit.update();draw();
    }
    const resize=()=>{const w=surface.clientWidth,h=surface.clientHeight;if(!w||!h)return;aspect=w/h;renderer.setSize(w,h,false);project();draw();};
    function change(next:Settings){
      pending=next;
      if(!body||!complexion||!garmentMaps)return;
      const fitted=body, maps=complexion, garments=garmentMaps;
      const paired=next.regiment==="Compare";
      if(next.regiment!==previous?.regiment){
        models.forEach(m=>{scene.remove(m.outfit);m.dispose();});
        const choices:Regiment[]=next.regiment==="Compare"?["Blue","Yellow"]:[next.regiment];
        models=choices.map((choice,i)=>{
          const model=buildUniform(choice,fitted,maps,garments);scene.add(model.outfit);
          model.outfit.position.x=paired?(i===0?-41:41):0;
          model.outfit.traverse(part=>{if(part instanceof T.Mesh){part.castShadow=true;part.receiveShadow=true;}});
          return model;
        });
      }
      models.forEach(m=>m.setLayer(next.layer));plinth.visible=rim.visible=next.layer!=="Pelisse";
      floor.visible=contacts.visible=next.layer!=="Pelisse";
      contacts.children.forEach((foot,index)=>{
        const pair=Math.floor(index/2);
        foot.visible=paired||pair===0;
        foot.position.set((paired?(pair===0?-41:41):0)+(index%2===0?-10.5:10.5),.12,2.65);
      });
      renderer.shadowMap.needsUpdate=true;
      plinth.scale.x=rim.scale.x=paired?2.2:1;
      const isolated = next.layer === "Pelisse";
      const detail = next.framing.preset === "Chest";
      const bust = next.framing.preset === "Bust" && !isolated;
      const targetHeight = bust ? 151 : detail ? 125 : isolated ? 117 : 101;
      const preferredSpan = bust ? 59 : detail ? (isolated ? 28 : 41) : (isolated ? 42 : 113);
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
    const textureLoader=new T.TextureLoader();
    const loadPigment=async(name:string)=>{
      const texture=await textureLoader.loadAsync("/wardrobe/hussar/complexion-"+name+".webp");
      texture.colorSpace=T.SRGBColorSpace; texture.anisotropy=4;
      if(!mounted)texture.dispose();
      return texture;
    };
    Promise.all([loadHussarBody(),loadPigment("front"),loadPigment("side"),loadGarmentMaps(textureLoader)]).then(([source,front,side,garments])=>{
      if(!mounted){disposeFitted(source);disposeGarmentMaps(garments);return;}
      body=source;
      complexion={front,side};
      garmentMaps=garments;
      if(pending)change(pending);
    }).catch(()=>{if(mounted)onFailure();});
    if(new URLSearchParams(location.search).has("capture"))window.ftxHussarFrame=orient;
    return ()=>{mounted=false;apply.current=null;if(window.ftxHussarFrame===orient)delete window.ftxHussarFrame;
      ro.disconnect();orbit.removeEventListener("change",draw);orbit.dispose();el.removeEventListener("keydown",keydown);el.removeEventListener("webglcontextlost",failure);
      models.forEach(m=>m.dispose());if(body)disposeFitted(body);if(garmentMaps)disposeGarmentMaps(garmentMaps);complexion?.front.dispose();complexion?.side.dispose();key.shadow.dispose();floor.geometry.dispose();floor.material.dispose();contactMap.dispose();contactGeometry.dispose();contactMaterial.dispose();plinth.geometry.dispose();plinth.material.dispose();rim.geometry.dispose();rim.material.dispose();lights.dispose();renderer.dispose();renderer.forceContextLoss();el.remove();};
  },[onFailure]);
  useEffect(()=>{apply.current?.({regiment,layer,framing});},[regiment,layer,framing,onFailure]);
  return <div className="hu-scene" ref={host}/>;
}
