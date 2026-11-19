"use client";

import { useEffect, useRef } from "react";
import * as T from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { createGarmentMesh, type Exhibit, type StudioMode } from "./garmentMesh";

type Props = {
  run: string; strain: {min: number; max: number; p95: number};
  frame: number; mode: StudioMode; reset: number; outfit: boolean; full: boolean; registration:number[];
  onReady: (run: string) => void; onError: (run: string) => void;
};
declare global {interface Window {ftxPoseCamera?: (yaw:number, y:number, half:number, z?:number)=>void}}

async function response(path: string, signal: AbortSignal) {
  const result = await fetch(path, {signal});
  if (!result.ok) throw Error("Could not load native drape data.");
  return result;
}

async function readRun(run: string, strain: Props["strain"], outfit: boolean, signal: AbortSignal) {
  const [layout, trousers, packed]: [Exhibit, Exhibit|null, ArrayBuffer] = await Promise.all([
    response("/exhibits/bolero.json", signal).then(r => r.json()),
    outfit ? response("/exhibits/trousers.json", signal).then(r=>r.json()) : null,
    response("/exhibits/poses/"+run+".bin.gz", signal).then(async r => {
      if (!r.body) throw Error("Empty garment replay.");
      return new Response(r.body.pipeThrough(new DecompressionStream("gzip"))).arrayBuffer();
    }),
  ]);
  const header = new DataView(packed);
  const vertices = header.getUint32(0,true), indices = header.getUint32(4,true);
  let offset = 8;
  function coordinates(length: number) {
    const encoded = new Int16Array(packed, offset, length);
    offset += 2 * length;
    return Float32Array.from(encoded, x => x / 10000);
  }
  const skin = coordinates(vertices*3);
  const triangles = new Uint16Array(packed, offset, indices);
  offset += indices * 2;
  const frames = coordinates(layout.frames.length * layout.vertices * 3);
  const lower=trousers ? new T.BufferGeometry() : null;
  if(trousers && lower){
    lower.setAttribute("position",new T.BufferAttribute(coordinates(trousers.vertices*3),3));
    lower.setIndex(trousers.triangles);lower.rotateX(-Math.PI/2);lower.computeVertexNormals();
  }
  // Packed forces follow int16 coordinates and may start on a two-byte boundary.
  const view = new DataView(packed,offset);
  const forces = Float32Array.from({length:layout.vertices},(_,i)=>view.getFloat32(i*4,true));
  offset += layout.vertices*4;
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
  return {body,garment,lower,floor:body.boundingBox!.min.y,dispose(){lower?.dispose();body.dispose();garment.dispose();}};
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
    renderer.setPixelRatio(Math.min(window.devicePixelRatio,2));
    renderer.outputColorSpace=T.SRGBColorSpace; renderer.toneMapping=T.ACESFilmicToneMapping;
    renderer.toneMappingExposure=1.12; renderer.localClippingEnabled=true;
    renderer.shadowMap.enabled=true; renderer.shadowMap.type=T.PCFShadowMap;
    const scene=new T.Scene();
    scene.add(new T.HemisphereLight(0xfff5e5,0x705a54,1.65));
    const key=new T.DirectionalLight(0xffead3,3.1); key.position.set(-1.1,1.7,1.5);key.castShadow=true;
    key.shadow.mapSize.set(1024,1024);
    Object.assign(key.shadow.camera,{left:-.8,right:.8,top:.8,bottom:-.8,near:.05,far:4});
    key.shadow.camera.updateProjectionMatrix(); key.shadow.bias=-.00027;key.shadow.normalBias=.0006;
    const rim=new T.DirectionalLight(0xf1fbff,1.7);rim.position.set(.9,.8,-1.1);
    scene.add(key,rim);
    const camera=new T.OrthographicCamera(-1,1,1,-1,.02,6);
    const reverse=new T.OrthographicCamera(-1,1,.56,-.56,.02,6);
    const controls=new OrbitControls(camera,canvas);
    canvas.style.touchAction="pan-y";
    controls.enablePan=false;controls.enableZoom=false; // A wheel over the landing page still scrolls.
    controls.minPolarAngle=.65;controls.maxPolarAngle=1.73;
    controls.touches.ONE=T.TOUCH.ROTATE;
    const plaster=new T.MeshStandardMaterial({color:"#c99777",roughness:.86});
    const registration=new T.Matrix4();
    const portrait=new T.TextureLoader().load("/media/drape-room-portrait.webp",()=>render());
    portrait.colorSpace=T.SRGBColorSpace;
    function pigment(material:T.MeshStandardMaterial,eye=false){
      material.onBeforeCompile=shader=>{
        shader.uniforms.faceAtlas={value:portrait};shader.uniforms.headFrame={value:registration};
        shader.vertexShader="uniform mat4 headFrame; varying vec3 headPoint; varying vec3 headNormal;\n"+shader.vertexShader.replace(
          "#include <begin_vertex>","#include <begin_vertex>\nheadPoint=(headFrame*modelMatrix*vec4(position,1.)).xyz; headNormal=normalize(mat3(modelMatrix)*normal);");
        shader.fragmentShader="uniform sampler2D faceAtlas; varying vec3 headPoint; varying vec3 headNormal;\n"+shader.fragmentShader.replace("#include <color_fragment>",`#include <color_fragment>
          vec3 p=headPoint, n=normalize(headNormal);
          vec3 front=texture2D(faceAtlas,vec2(.25+p.x/.496,.5+(p.y-.388)/.248)).rgb;
          vec3 side=texture2D(faceAtlas,vec2(.75-(p.z-.067)/.496,.5+(p.y-.388)/.248)).rgb;
          float frontness=smoothstep(.18,.78,n.z)*smoothstep(.082,.111,p.z);
          float face=smoothstep(.096,.12,p.z)*(1.-smoothstep(.043,.062,abs(p.x)));
          frontness=max(frontness,face);
          vec3 color=mix(side,front,${eye?"1.0":"frontness"});
          float scalp=smoothstep(.334,.368,p.y)*smoothstep(.08,.73,-n.z);
          vec3 crop=texture2D(faceAtlas,vec2(.82+abs(p.x)*.55,.5+(clamp(p.y,.407,.454)-.388)/.248)).rgb;
          color=mix(color,crop*.83,scalp);
          float skin=smoothstep(.286,.320,p.y)*smoothstep(-.015,.005,color.r-color.g);
          diffuseColor.rgb=mix(diffuseColor.rgb,color*.88,${eye?"1.0":"skin"});`).replace("#include <opaque_fragment>",
            `outgoingLight=mix(outgoingLight,color*1.03,${eye?"0.69":"skin*.38"});\n#include <opaque_fragment>`);
      };
      material.customProgramCacheKey=()=>eye?"drape-eyes":"drape-portrait";
    }
    pigment(plaster);
    const bust=plaster.clone();
    pigment(bust);
    bust.clippingPlanes=[new T.Plane(new T.Vector3(0,1,0),-.033)];bust.clipShadows=true;
    const form=new T.Mesh(new T.BufferGeometry(),[bust,plaster]);
    const empty=form.geometry;
    form.castShadow=form.receiveShadow=true;scene.add(form);
    const eyes=new T.Group(),eyeMaterial=new T.MeshStandardMaterial({roughness:.48});
    pigment(eyeMaterial,true);
    const eyeGeometry=new T.SphereGeometry(.011,20,14);
    for(const x of [-.027,.027]){
      const eye=new T.Mesh(eyeGeometry,eyeMaterial);eye.position.set(x,.3843,.1208);eyes.add(eye);
    }
    eyes.matrixAutoUpdate=false;scene.add(eyes);
    const twill=new T.MeshStandardMaterial({color:"#996540",roughness:.95,side:T.DoubleSide});
    const trousers=new T.Mesh(empty,twill);trousers.castShadow=trousers.receiveShadow=true;
    trousers.visible=false;scene.add(trousers);
    const brass=new T.MeshStandardMaterial({color:"#a08255",metalness:.5,roughness:.42});
    const lacquer=new T.MeshStandardMaterial({color:"#423b34",roughness:.52});
    const pedestal=new T.Group();scene.add(pedestal);
    function stand(top:number,bottom:number,height:number,y:number,material:T.Material,zScale=1){
      const mesh=new T.Mesh(new T.CylinderGeometry(top,bottom,height,80),material);
      mesh.position.set(0,y,.061);mesh.scale.z=zScale;mesh.castShadow=true;mesh.receiveShadow=true;pedestal.add(mesh);
    }
    stand(.016,.018,.149,-.031,brass);stand(.095,.105,.014,-.104,lacquer,.78);
    stand(.102,.105,.003,-.112,brass,.78);
    const groundMaterial=new T.ShadowMaterial({opacity:.095});
    const ground=new T.Mesh(new T.PlaneGeometry(8,8),groundMaterial);
    ground.rotation.x=-Math.PI/2;ground.position.y=-.114;ground.receiveShadow=true;scene.add(ground);
    const pad=document.createElement("canvas");pad.width=pad.height=64;
    const ctx=pad.getContext("2d")!, gradient=ctx.createRadialGradient(32,32,0,32,32,32);
    gradient.addColorStop(0,"rgba(55,40,24,.32)");gradient.addColorStop(.45,"rgba(55,40,24,.17)");gradient.addColorStop(1,"rgba(55,40,24,0)");
    ctx.fillStyle=gradient;ctx.fillRect(0,0,64,64);
    const shadowMap=new T.CanvasTexture(pad);shadowMap.colorSpace=T.SRGBColorSpace;
    const footInk=new T.MeshBasicMaterial({map:shadowMap,transparent:true,depthWrite:false,toneMapped:false});
    const footShadow=new T.Mesh(new T.PlaneGeometry(.34,.22),footInk);
    footShadow.rotation.x=-Math.PI/2;footShadow.position.z=.07;scene.add(footShadow);
    let current: Awaited<ReturnType<typeof readRun>> | undefined;
    let request: AbortController | undefined;
    let loaded="", reset=latest.current.reset, lastFull=false;
    let w=1,h=1,mainWidth=1;
    function home(){
      camera.zoom=1;controls.target.set(0,.178,.065);
      camera.position.set(.55,.42,1.8);camera.updateProjectionMatrix();controls.update();
    }
    function render(){
      if(!loaded)return;
      renderer.setScissorTest(true);renderer.setScissor(0,0,mainWidth,h);renderer.setViewport(0,0,mainWidth,h);
      renderer.render(scene,camera);
      if(mainWidth<w){
        renderer.setScissor(mainWidth,0,w-mainWidth,h);renderer.setViewport(mainWidth,0,w-mainWidth,h);
        renderer.render(scene,reverse);
      }
    }
    function update(){
      const {mode,frame,reset:nextReset,run,full}=latest.current;
      if(!current||loaded!==run)return;
      if(reset!==nextReset||full!==lastFull){reset=nextReset;lastFull=full;home();}
      const t=mode==="strain"||mode==="contact"?1:frame/135;
      const opened=Math.max(0,1-t*2.2);
      const companion=!!current.lower&&w>=580&&!full&&frame>=88;
      mainWidth=companion?Math.round(w*.64):w;
      const fullFigure=full&&!!current.lower;
      const centerY=fullFigure ? .01 : .177;
      camera.position.y+=centerY-controls.target.y; controls.target.y=centerY;
      const half=Math.max(fullFigure ? .53 : .34, (.47+opened*.23)/(mainWidth/h)) + opened*.045;
      camera.top=half;camera.bottom=-half;camera.left=-half*mainWidth/h;camera.right=-camera.left;camera.updateProjectionMatrix();
      if(companion){
        const halfY=Math.max(.61,.49*h/(w-mainWidth));
        reverse.top=halfY;reverse.bottom=-halfY;reverse.right=halfY*(w-mainWidth)/h;reverse.left=-reverse.right;
        reverse.position.set(-.52,.25,-1.75);reverse.lookAt(0,.01,.065);reverse.updateProjectionMatrix();
      }
      current.garment.pose(mode,t,false,mainWidth/h);
      canvas.dataset.companion=String(companion);canvas.dataset.full=String(full);
      canvas.dataset.frame=String(Math.round(t*135));canvas.dataset.mode=mode;
      canvas.dataset.run=loaded;render();
    }
    draw.current=update;
    change.current=()=>{
      const {run,strain,outfit}=latest.current;
      request?.abort();const controller=new AbortController();request=controller;
      delete canvas.dataset.run;
      readRun(run,strain,outfit,controller.signal).then(result=>{
        if(controller.signal.aborted){result.dispose();return;}
        if(current){scene.remove(current.garment.group);current.dispose();}
        current=result;loaded=run;form.geometry=result.body;
        registration.fromArray(latest.current.registration);
        eyes.matrix.copy(registration).invert();eyes.matrixWorldNeedsUpdate=true;
        form.material=[result.lower?plaster:bust,plaster];
        trousers.visible=!!result.lower;trousers.geometry=result.lower??empty;pedestal.visible=!result.lower;
        ground.visible=!result.lower;footShadow.visible=!!result.lower;footShadow.position.y=result.floor-.001;
        scene.add(result.garment.group);update();
        latest.current.onReady(run);
      }).catch(()=>{if(!controller.signal.aborted)latest.current.onError(run);});
    };
    function resize(){w=el.clientWidth;h=el.clientHeight;if(w&&h){renderer.setSize(w,h,false);update();}}
    const observer=new ResizeObserver(resize);observer.observe(el);
    const capture=(yaw:number,y:number,half:number,z=.067)=>{
      mainWidth=w;camera.zoom=1;controls.target.set(0,y,z);
      camera.position.set(Math.sin(yaw)*1.8,y,z+Math.cos(yaw)*1.8);
      camera.top=half;camera.bottom=-half;camera.left=-half*w/h;camera.right=-camera.left;
      camera.updateProjectionMatrix();controls.update();render();
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
      current?.dispose();empty.dispose();bust.dispose();plaster.dispose();portrait.dispose();eyeMaterial.dispose();eyeGeometry.dispose();twill.dispose();ground.geometry.dispose();groundMaterial.dispose();
      shadowMap.dispose();footInk.dispose();footShadow.geometry.dispose();
      pedestal.children.forEach(m=>(m as T.Mesh).geometry.dispose());brass.dispose();lacquer.dispose();key.shadow.dispose();
      renderer.dispose();renderer.forceContextLoss();canvas.remove();draw.current=()=>{};change.current=()=>{};
      if(window.ftxPoseCamera===capture)delete window.ftxPoseCamera;
    };
  },[]);
  useEffect(()=>{change.current();},[props.run]);
  return <div className="dr-canvas" ref={host}/>;
}
