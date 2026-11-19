"use client";

import {useEffect, useRef} from "react";
import * as T from "three";
import {OrbitControls} from "three/addons/controls/OrbitControls.js";
import {weaveTexture} from "../garmentMesh";
import type {loadSurface,loadTopology} from "./surface";

export type Model = Awaited<ReturnType<typeof loadSurface>> & {
  mesh:Awaited<ReturnType<typeof loadTopology>>; floor:number; key:string
};
type Props = {model:Model; plane:number|null; clip:boolean; strain:boolean; view:number; onError:()=>void};
declare global {interface Window {ftxTrouserCamera?:(angle:number)=>void}}

export default function Scene(props:Props) {
  const host=useRef<HTMLDivElement>(null);
  const draw=useRef<(p:Props)=>void>(()=>{});
  const latest=useRef(props);
  useEffect(()=>{latest.current=props;draw.current(props);},[props]);
  useEffect(()=>{
    const el=host.current!, model=props.model;
    let renderer:T.WebGLRenderer;
    try {renderer=new T.WebGLRenderer({antialias:true,alpha:true,powerPreference:"low-power"});}
    catch {latest.current.onError();return;}
    const canvas=renderer.domElement;
    canvas.setAttribute("role","img");canvas.setAttribute("aria-label","Orbit the trouser form. Left and right arrow keys rotate the camera.");
    canvas.tabIndex=0;canvas.dataset.run=model.key;el.append(canvas);
    renderer.setPixelRatio(Math.min(devicePixelRatio,2));renderer.shadowMap.enabled=true;
    renderer.shadowMap.type=T.PCFShadowMap;renderer.localClippingEnabled=true;renderer.setClearColor(0,0);
    renderer.toneMapping=T.ACESFilmicToneMapping;renderer.toneMappingExposure=1.12;
    const scene=new T.Scene(), camera=new T.OrthographicCamera(-1,1,1,-1,.01,8);
    const controls=new OrbitControls(camera,canvas);
    controls.enablePan=false;controls.enableZoom=false;controls.minPolarAngle=.42;controls.maxPolarAngle=1.66;
    const meshes:T.BufferGeometry[]=[], materials:T.Material[]=[];
    function geo(v:Float32Array,ix:Uint16Array) {
      const g=new T.BufferGeometry();g.setAttribute("position",new T.BufferAttribute(v,3));
      g.setIndex(new T.BufferAttribute(ix,1));g.computeVertexNormals();meshes.push(g);return g;
    }
    function object(geometry:T.BufferGeometry,material:T.Material) {
      const obj=new T.Mesh(geometry,material);scene.add(obj);obj.castShadow=obj.receiveShadow=true;
      materials.push(material);return obj;
    }
    const cutPlane=new T.Plane(new T.Vector3(0,-1,0),.12);
    const weave=weaveTexture();
    const bodyMaterial=new T.MeshStandardMaterial({color:model.cloth.length?"#cba97e":"#9db8ad",roughness:.86,
      clippingPlanes:[cutPlane],side:T.DoubleSide});
    const body=object(geo(model.body,model.mesh.body),bodyMaterial);
    const clothMaterial=new T.MeshPhysicalMaterial({map:weave,bumpMap:weave,bumpScale:.00010,color:"#344a72",
      roughness:.91,sheen:.5,sheenRoughness:.8,sheenColor:"#979bb8",side:T.DoubleSide});
    const ink=new T.MeshBasicMaterial({vertexColors:true,side:T.DoubleSide});
    materials.push(ink,clothMaterial);
    const seams=new T.Group();scene.add(seams);
    let cloth:T.Mesh|undefined;
    if(model.cloth.length){
      const geometry=geo(model.cloth,model.mesh.cloth);
      geometry.setAttribute("uv",new T.BufferAttribute(model.mesh.uv,2));
      const colors=new Float32Array(model.colors.length), color=new T.Color();
      for(let i=0;i<colors.length;i+=3)color.setRGB(model.colors[i]/255,model.colors[i+1]/255,model.colors[i+2]/255,T.SRGBColorSpace).toArray(colors,i);
      geometry.setAttribute("color",new T.BufferAttribute(colors,3));
      cloth=object(geometry,clothMaterial);
      const thread=new T.LineBasicMaterial({color:"#d0ac72",transparent:true,opacity:.78});
      materials.push(thread);
      for(const sequence of model.mesh.seams){
        const points=sequence.map(i=>new T.Vector3().fromArray(model.cloth,3*i));
        const path=new T.BufferGeometry().setFromPoints(points);meshes.push(path);
        seams.add(new T.Line(path,thread));
      }
    } else {
      const wireMaterial=new T.MeshBasicMaterial({wireframe:true,color:"#2c534f",transparent:true,opacity:.15,clippingPlanes:[cutPlane]});
      materials.push(wireMaterial);scene.add(new T.Mesh(body.geometry,wireMaterial));
    }
    const ceiling=.119, floor=model.floor;
    const points:T.Vector3[]=[];
    const p=model.body,ix=model.mesh.body;
    for(let j=0;j<ix.length;j+=3)for(let k=0;k<3;k++){
      const a=ix[j+k]*3,b=ix[j+(k+1)%3]*3;
      if((p[a+1]>ceiling)===(p[b+1]>ceiling))continue;
      const t=(ceiling-p[a+1])/(p[b+1]-p[a+1]);
      points.push(new T.Vector3(p[a]+t*(p[b]-p[a]),ceiling,p[a+2]+t*(p[b+2]-p[a+2])));
    }
    // The visible wood cap follows the cropped waist outline of this particular pose.
    const capMaterial=new T.MeshStandardMaterial({color:"#a18660",roughness:.6,side:T.DoubleSide});
    materials.push(capMaterial);
    const cap=new T.Group();scene.add(cap);
    if(points.length){
      const center=points.reduce((sum,v)=>sum.add(v),new T.Vector3()).divideScalar(points.length);
      points.sort((a,b)=>Math.atan2(a.z-center.z,a.x-center.x)-Math.atan2(b.z-center.z,b.x-center.x));
      const capGeometry=new T.BufferGeometry().setFromPoints([center,...points]);
      capGeometry.setIndex(points.flatMap((_,i)=>[0,(i+1)%points.length+1,i+1]));capGeometry.computeVertexNormals();meshes.push(capGeometry);
      cap.add(new T.Mesh(capGeometry,capMaterial));
      const knob=new T.SphereGeometry(.014,32,16);meshes.push(knob);
      const finial=new T.Mesh(knob,capMaterial);finial.position.copy(center);finial.scale.y=.52;cap.add(finial);
    }
    let minZ=Infinity,maxZ=-Infinity;
    for(let i=0;i<model.body.length;i+=3)if(model.body[i+1]<floor+.035){minZ=Math.min(minZ,model.body[i+2]);maxZ=Math.max(maxZ,model.body[i+2]);}
    const baseZ=(minZ+maxZ)/2;
    const stand=new T.CylinderGeometry(.228,.236,.013,96);meshes.push(stand);
    const base=object(stand,new T.MeshStandardMaterial({color:"#625545",roughness:.52,metalness:.28}));
    base.position.set(0,floor-.008,baseZ);base.scale.z=.87;
    const rim=new T.CylinderGeometry(.237,.235,.004,96);meshes.push(rim);
    const rimObj=object(rim,new T.MeshStandardMaterial({color:"#b59a63",roughness:.4,metalness:.45}));
    rimObj.position.copy(base.position);rimObj.position.y-=.008;rimObj.scale.z=.87;
    const surface=new T.PlaneGeometry(.54,.46);surface.rotateX(-Math.PI/2);meshes.push(surface);
    const glass=new T.MeshBasicMaterial({color:"#7ee7cd",transparent:true,opacity:.18,side:T.DoubleSide,depthWrite:false});
    const sheet=new T.Mesh(surface,glass);materials.push(glass);sheet.position.z=.078;scene.add(sheet);
    const edge=new T.EdgesGeometry(surface);meshes.push(edge);
    const border=new T.LineBasicMaterial({color:"#d5ffdf"});materials.push(border);
    sheet.add(new T.LineSegments(edge,border));
    const light=new T.DirectionalLight(0xffe4c6,3.8);light.position.set(-1.5,1.5,1.4);
    light.castShadow=true;light.shadow.mapSize.set(1024,1024);
    Object.assign(light.shadow.camera,{left:-.7,right:.7,top:.7,bottom:-.7,near:.1,far:4});
    light.shadow.camera.updateProjectionMatrix();light.shadow.bias=-.00025;light.shadow.normalBias=.00055;
    const fill=new T.DirectionalLight(0xb5e1da,2.2);fill.position.set(.7,.7,-1);
    scene.add(light,fill,new T.HemisphereLight(0xffefd4,0x8b8072,1.25));
    const render=()=>renderer.render(scene,camera);
    let view=-1;
    function orbit(angle:number){
      controls.target.set(0,(ceiling+floor)/2,.085);
      camera.position.set(1.7*Math.sin(angle),controls.target.y+.27,.085+1.7*Math.cos(angle));
      controls.update();render();
    }
    function update(p:Props){
      if(p.model!==model)return;
      const y=p.plane??0;sheet.visible=p.plane!==null;sheet.position.y=y;
      cap.visible=!p.clip||!sheet.visible||y>=ceiling;
      cutPlane.constant=p.clip&&sheet.visible?Math.min(y,ceiling):ceiling;
      const limit=p.clip&&sheet.visible?[cutPlane]:[];
      if(cloth){cloth.material=p.strain?ink:clothMaterial;clothMaterial.clippingPlanes=limit;ink.clippingPlanes=limit;}
      seams.visible=!p.strain&&!p.clip;
      if(view!==p.view){view=p.view;orbit([.57,1.57,3.25][view]);}
      canvas.dataset.slice=sheet.visible?String(Math.round((y-floor)*1000)):"";
      canvas.dataset.cloth=String(!!cloth);render();
    }
    function size(){
      const w=el.clientWidth,h=el.clientHeight;if(!w||!h)return;
      const half=Math.max((ceiling-floor)*.69,.31*h/w);
      camera.left=-half*w/h;camera.right=-camera.left;camera.top=half;camera.bottom=-half;
      camera.updateProjectionMatrix();renderer.setSize(w,h,false);render();
    }
    function key(e:KeyboardEvent){
      if(e.key!=="ArrowLeft"&&e.key!=="ArrowRight")return;
      const d=camera.position.clone().sub(controls.target);
      d.applyAxisAngle(new T.Vector3(0,1,0),e.key==="ArrowLeft"?-.13:.13);
      camera.position.copy(d).add(controls.target);controls.update();e.preventDefault();
    }
    function lost(e:Event){e.preventDefault();latest.current.onError();}
    controls.addEventListener("change",render);canvas.addEventListener("keydown",key);canvas.addEventListener("webglcontextlost",lost);
    const observer=new ResizeObserver(size);observer.observe(el);
    draw.current=update;size();update(latest.current);
    if(new URLSearchParams(location.search).has("capture"))window.ftxTrouserCamera=orbit;
    return ()=>{
      draw.current=()=>{};if(window.ftxTrouserCamera===orbit)delete window.ftxTrouserCamera;
      observer.disconnect();controls.dispose();canvas.removeEventListener("keydown",key);canvas.removeEventListener("webglcontextlost",lost);
      for(const g of meshes)g.dispose();for(const m of new Set(materials))m.dispose();
      weave.dispose();light.shadow.dispose();renderer.dispose();renderer.forceContextLoss();canvas.remove();
    };
  },[props.model]);
  return <div className="ts-canvas" ref={host}/>;
}
