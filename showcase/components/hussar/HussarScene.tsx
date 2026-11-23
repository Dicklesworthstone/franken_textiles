"use client";

import { useEffect, useRef } from "react";
import { capturePixelRatio } from "../atelier3d/capture-resolution";
import * as T from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";
import { fullControls } from "../atelier3d/viewControls";
import {readStudy,type Framing,type Layer,type Regiment,type RegimentView} from "./RegimentalStudy";
import {makeMount,selectedPiece,type Ensemble,type Section,type Mount,type PieceReading} from "./PatternMount";

type Settings = {regiment:RegimentView;layer:Layer;framing:{preset:Framing};section:Section;layout:Mount};
declare global {interface Window {ftxCostumeFrame?: (yaw:number,target:number,span:number)=>void}}
export default function HussarScene({regiment,layer,framing,section,layout,onFailure,onPiece}:{onFailure:()=>void;onPiece:(value:PieceReading|null)=>void}&Settings){
  const host=useRef<HTMLDivElement>(null), apply=useRef<((s:Settings)=>void)|null>(null);
  useEffect(()=>{
    const surface=host.current!;
    let renderer:T.WebGLRenderer;
    const capture=new URLSearchParams(location.search).has("costumeCapture");
    try {renderer=new T.WebGLRenderer({alpha:true,antialias:true,powerPreference:"low-power",preserveDrawingBuffer:capture});}
    catch {onFailure();return;}
    renderer.setPixelRatio(capturePixelRatio(2));renderer.toneMapping=T.ACESFilmicToneMapping;renderer.toneMappingExposure=.99;
    renderer.shadowMap.enabled=true;
    renderer.shadowMap.type=T.PCFSoftShadowMap;
    renderer.shadowMap.autoUpdate=false;
    const el=renderer.domElement;el.tabIndex=0;el.setAttribute("role","img");
    el.setAttribute("aria-label","Swedish regimental costume and pattern study. Drag or use arrow keys to orbit, and plus or minus to zoom.");surface.appendChild(el);
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
    const floor=new T.Mesh(new T.PlaneGeometry(1100,1100),new T.ShadowMaterial({color:0x111712,opacity:.24}));
    floor.rotation.x=-Math.PI/2;floor.position.y=-.6;floor.receiveShadow=true;scene.add(floor);
    const contactCanvas=document.createElement("canvas");contactCanvas.width=contactCanvas.height=96;
    const brush=contactCanvas.getContext("2d")!, contactFade=brush.createRadialGradient(48,48,9,48,48,47);
    contactFade.addColorStop(0,"rgba(7,11,9,.8)");contactFade.addColorStop(.5,"rgba(7,11,9,.46)");
    contactFade.addColorStop(1,"rgba(7,11,9,0)");brush.fillStyle=contactFade;brush.fillRect(0,0,96,96);
    const contactMap=new T.CanvasTexture(contactCanvas);
    const contactMaterial=new T.MeshBasicMaterial({map:contactMap,transparent:true,depthWrite:false});
    const contactGeometry=new T.PlaneGeometry(16.5,34);
    const contacts=new T.Group();scene.add(contacts);
    for(let i=0;i<4;i++){const patch=new T.Mesh(contactGeometry,contactMaterial);patch.rotation.x=-Math.PI/2;contacts.add(patch);}
    const orbit=new OrbitControls(camera,el);orbit.minZoom=.62;orbit.maxZoom=4;orbit.rotateSpeed=.72;
    let height=112,aspect=1,previous:Settings|undefined,pending:Settings|undefined, mounted=true;
    const studies:Partial<Record<Regiment,Awaited<ReturnType<typeof readStudy>>>>={};
    const ensembles:Partial<Record<Regiment,Ensemble>>={}, mounts:ReturnType<typeof makeMount>[]=[];
    let wool:T.Texture|undefined;
    function clearCuts(){for(const cut of mounts){scene.remove(cut.group);cut.dispose();}mounts.length=0;}
    function draw(){
      const yaw=orbit.getAzimuthalAngle(),pair=pending?.regiment==="Compare";
      const position=(object:T.Object3D,offset:number)=>{object.position.x=offset*Math.cos(yaw);object.position.z=-offset*Math.sin(yaw);};
      for(const [name,study] of Object.entries(studies)){study.angle(yaw);position(study.figure,pair?(name==="Blue"?-36:36):0);}
      mounts.forEach((cut,i)=>position(cut.group,pair?(i===0?-25:25):0));
      contacts.children.forEach((foot,i)=>{
        const x=pair?(i<2?-36:36):0;
        foot.position.set(x*Math.cos(yaw)+(i%2===0?-7:7),-.45,-x*Math.sin(yaw)+.5);
      });
      renderer.render(scene,camera);
    }
    function project(){camera.left=-height*aspect;camera.right=height*aspect;camera.top=height;camera.bottom=-height;camera.updateProjectionMatrix();}
    function orient(yaw:number,y:number,span:number,elevation=.075) {
      height=span;project();orbit.target.set(0,y,0);look.retarget(orbit.target);
      camera.position.set(360*Math.sin(yaw)*Math.cos(elevation),y+360*Math.sin(elevation),360*Math.cos(yaw)*Math.cos(elevation));
      if(!orbit.update())draw();
    }
    const resize=()=>{const w=surface.clientWidth,h=surface.clientHeight;if(!w||!h)return;aspect=w/h;renderer.setSize(w,h,false);project();draw();};
    function change(next:Settings){
      pending=next;
      if(!studies.Blue||!studies.Yellow||!ensembles.Blue||!ensembles.Yellow||!wool)return;
      const paired=next.regiment==="Compare";
      const dressed=next.layer==="Dressed";
      // The eight costume elevations support a level turn around the figure. The measured flat
      // pieces can also be examined from above or below their physical display support.
      orbit.minPolarAngle=dressed?Math.PI/2-.13:.2;
      orbit.maxPolarAngle=dressed?Math.PI/2+.10:Math.PI-.2;
      if(next.regiment!==previous?.regiment||next.layer!==previous?.layer||next.section!==previous.section||next.layout!==previous.layout){
        clearCuts();
        for(const name of ["Blue","Yellow"] as const){
          const x=paired?(name==="Blue"?-34:34):0,visible=paired||next.regiment===name,study=studies[name]!;
          study.figure.position.x=x;study.figure.visible=dressed&&visible;
          if(visible&&next.layer!=="Dressed"){
            const cut=makeMount(selectedPiece(ensembles[name]!,next.layer,next.section),name,next.layout,wool);
            cut.group.position.x=paired?x*.73:0;scene.add(cut.group);mounts.push(cut);
          }
        }
      }
      floor.visible=dressed&&!paired;contacts.visible=dressed;
      contacts.children.forEach((foot,index)=>{
        const pair=Math.floor(index/2);
        foot.visible=paired||pair===0;
        foot.position.set((paired?(pair===0?-34:34):0)+(index%2===0?-7:7),-.45,.5);
      });
      renderer.shadowMap.needsUpdate=true;
      const detail = next.framing.preset === "Chest";
      const bust = next.framing.preset === "Bust";
      const targetHeight=dressed?(bust?155:detail?135:103):125;
      const maxExtent=Math.max(14,...mounts.map(c=>c.extent.y));
      const preferredSpan=dressed?(bust?58:detail?39:115):Math.max(16,maxExtent*(detail?.48:.63));
      const sceneWidth=dressed?(paired?74:36):Math.max(16,...mounts.map(c=>c.extent.x*.57))+(paired?25:0);
      const span=Math.max(preferredSpan,sceneWidth/aspect);
      const yaw=next.framing.preset==="Reverse"?Math.PI:next.framing.preset==="Side"?Math.PI/2:dressed?.05:-.13;
      camera.zoom = 1;
      orient(yaw, targetHeight, span,dressed?.014:.07);
      const reading=mounts[0]?.reading??null;
      el.dataset.piece=reading?.id??"";
      onPiece(reading);
      previous = next;
      el.dataset.ready = next.regiment + "-" + next.layer;
    }
    const failure=(e:Event)=>{e.preventDefault();onFailure();};
    // Turning, moving, zooming and the keys are the site's shared ones; the reset returns to the chosen viewpoint.
    const look=fullControls(orbit,el,{paint:draw,turnKeys:true,home:()=>{if(previous)change(previous);}});
    const ro=new ResizeObserver(resize);ro.observe(surface);resize();orbit.addEventListener("change",draw);
    el.addEventListener("webglcontextlost",failure);
    apply.current=change;
    const readPattern=async(reg:string):Promise<Ensemble>=>{
      const response=await fetch("/patterns/hussar-"+reg+".json");
      if(!response.ok)throw Error("Could not load "+reg+" ensemble");return response.json();
    };
    Promise.all([readStudy("Blue"),readStudy("Yellow"),readPattern("blue"),readPattern("yellow"),new T.TextureLoader().loadAsync("/wardrobe/hussar/broadcloth.webp")]).then(([blue,yellow,blueCut,yellowCut,map])=>{
      if(!mounted){blue.dispose();yellow.dispose();map.dispose();return;}
      studies.Blue=blue;studies.Yellow=yellow;ensembles.Blue=blueCut;ensembles.Yellow=yellowCut;
      wool=map;map.wrapS=map.wrapT=T.RepeatWrapping;map.colorSpace=T.SRGBColorSpace;map.anisotropy=4;
      scene.add(blue.figure,yellow.figure);
      if(pending)change(pending);
    }).catch(()=>{if(mounted)onFailure();});
    if(capture)window.ftxCostumeFrame=orient;
    return ()=>{mounted=false;apply.current=null;if(window.ftxCostumeFrame===orient)delete window.ftxCostumeFrame;
      ro.disconnect();look.dispose();orbit.removeEventListener("change",draw);orbit.dispose();el.removeEventListener("webglcontextlost",failure);
      clearCuts();Object.values(studies).forEach(s=>s.dispose());wool?.dispose();key.shadow.dispose();floor.geometry.dispose();floor.material.dispose();contactMap.dispose();contactGeometry.dispose();contactMaterial.dispose();lights.dispose();renderer.dispose();renderer.forceContextLoss();el.remove();};
  },[onFailure,onPiece]);
  useEffect(()=>{apply.current?.({regiment,layer,framing,section,layout});},[regiment,layer,framing,section,layout,onFailure]);
  return <div className="hu-scene" ref={host}/>;
}
