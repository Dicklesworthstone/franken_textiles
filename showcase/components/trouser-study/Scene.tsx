"use client";

import {useEffect, useRef} from "react";
import * as T from "three";
import {OrbitControls} from "three/addons/controls/OrbitControls.js";
import type {loadSurface,loadTopology} from "./surface";
import {dressCloth} from "../atelier3d/garment";
import {fabrics} from "../atelier3d/weave";
import {formMaterial,lightStudio,prepareRenderer} from "../atelier3d/studio";
import {fullControls} from "../atelier3d/viewControls";
import {cuttingTable,sewingFrame,type SewingPhases} from "../construction/replay";

export type Model = Awaited<ReturnType<typeof loadSurface>> & {
  mesh:Awaited<ReturnType<typeof loadTopology>>; floor:number; key:string; replay:Int16Array; frames:number[]; phases:SewingPhases
};
type Props = {model:Model; plane:number|null; clip:boolean; strain:boolean; view:number; step:number; onError:()=>void};
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
    canvas.setAttribute("role","img");canvas.setAttribute("aria-label","The trouser form. Drag or use the arrow keys to turn it, right-drag or Shift with the arrow keys to move it, scroll, pinch or use plus and minus to zoom. Home resets the view.");
    canvas.tabIndex=0;canvas.dataset.run=model.key;el.append(canvas);
    prepareRenderer(renderer);renderer.localClippingEnabled=true;renderer.setClearColor(0,0);
    const scene=new T.Scene(), camera=new T.OrthographicCamera(-1,1,1,-1,.01,8);
    const studio=lightStudio(renderer,scene,"warm");
    const controls=new OrbitControls(camera,canvas);
    controls.minZoom=.7;controls.maxZoom=4;controls.minPolarAngle=.2;controls.maxPolarAngle=1.66; // Never from under the floor.
    // The point the camera was last aimed at for the frame on show; the visitor moves the view from there.
    const aim=controls.target.clone();
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
    // The lower-body form is open at the waist, so its inside is shown too.
    const bodyMaterial=formMaterial();
    bodyMaterial.clippingPlanes=[cutPlane];bodyMaterial.side=T.DoubleSide;
    const body=object(geo(model.body,model.mesh.body),bodyMaterial);
    const ink=new T.MeshBasicMaterial({vertexColors:true,side:T.DoubleSide});
    materials.push(ink);
    let cloth:ReturnType<typeof dressCloth>|undefined;
    if(model.cloth.length){
      cloth=dressCloth(model.mesh.garment,model.mesh.flat,fabrics.navyTwill,{sewn:model.cloth,edge:"hem"});
      cloth.update(model.cloth);
      const colors=new Float32Array(model.colors.length), color=new T.Color();
      for(let i=0;i<colors.length;i+=3)color.setRGB(model.colors[i]/255,model.colors[i+1]/255,model.colors[i+2]/255,T.SRGBColorSpace).toArray(colors,i);
      cloth.shell.attribute("color",colors,3);
      scene.add(cloth.mesh);
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
    const capMaterial=new T.MeshStandardMaterial({color:"#907968",roughness:.93,side:T.DoubleSide});
    materials.push(capMaterial);
    const cap=new T.Group();scene.add(cap);
    if(points.length){
      const center=points.reduce((sum,v)=>sum.add(v),new T.Vector3()).divideScalar(points.length);
      points.sort((a,b)=>Math.atan2(a.z-center.z,a.x-center.x)-Math.atan2(b.z-center.z,b.x-center.x));
      const capGeometry=new T.BufferGeometry().setFromPoints([center,...points]);
      capGeometry.setIndex(points.flatMap((_,i)=>[0,(i+1)%points.length+1,i+1]));capGeometry.computeVertexNormals();meshes.push(capGeometry);
      cap.add(new T.Mesh(capGeometry,capMaterial));
    }
    let minZ=Infinity,maxZ=-Infinity;
    for(let i=0;i<model.body.length;i+=3)if(model.body[i+1]<floor+.035){minZ=Math.min(minZ,model.body[i+2]);maxZ=Math.max(maxZ,model.body[i+2]);}
    const baseZ=(minZ+maxZ)/2;
    const ground=new T.PlaneGeometry(8,8);ground.rotateX(-Math.PI/2);meshes.push(ground);
    const shade=new T.ShadowMaterial({opacity:.27});materials.push(shade);
    const floorShadow=new T.Mesh(ground,shade);floorShadow.receiveShadow=true;floorShadow.position.set(0,floor-.001,baseZ);scene.add(floorShadow);
    const surface=new T.PlaneGeometry(.54,.46);surface.rotateX(-Math.PI/2);meshes.push(surface);
    const glass=new T.MeshBasicMaterial({color:"#7ee7cd",transparent:true,opacity:.18,side:T.DoubleSide,depthWrite:false});
    const sheet=new T.Mesh(surface,glass);materials.push(glass);sheet.position.z=.078;scene.add(sheet);
    const edge=new T.EdgesGeometry(surface);meshes.push(edge);
    const border=new T.LineBasicMaterial({color:"#d5ffdf"});materials.push(border);
    sheet.add(new T.LineSegments(edge,border));
    studio.frame(new T.Vector3(0,(ceiling+floor)/2,.085),(ceiling-floor)*.78);
    const render=()=>renderer.render(scene,camera);
    let view=-1,lastFrame=-1,board:T.Box3|null=null;
    const neutral=new T.Vector3(0,(ceiling+floor)/2,.085);
    const xyz=new Float32Array(model.cloth.length),attr=new T.BufferAttribute(xyz,3);
    function sample(index:number){
      const offset=index*xyz.length;
      for(let i=0;i<xyz.length;i+=3){
        xyz[i]=model.replay[offset+i]/10000;xyz[i+1]=model.replay[offset+i+2]/10000;xyz[i+2]=-model.replay[offset+i+1]/10000;
      }
      return xyz;
    }
    const arrange=cuttingTable(sample(0),Object.values(model.mesh.garment.panels??{}));
    function orbit(angle:number){
      camera.position.set(1.7*Math.sin(angle),.27,1.7*Math.cos(angle)).add(controls.target);
      controls.update();render();
    }
    function update(p:Props){
      if(p.model!==model)return;
      const y=p.plane??0;sheet.visible=p.plane!==null;sheet.position.y=y;
      cap.visible=!p.clip||!sheet.visible||y>=ceiling;
      cutPlane.constant=p.clip&&sheet.visible?Math.min(y,ceiling):ceiling;
      const limit=p.clip&&sheet.visible?[cutPlane]:[];
      if(cloth){
        const index=sewingFrame(model.frames,model.phases,p.step),frame=model.frames[index];
        if(index!==lastFrame){
          const staging=frame<model.phases.fold;
          sample(index);if(staging)arrange(xyz);
          cloth.update(xyz);
          board=staging?new T.Box3().setFromBufferAttribute(attr):null;
          const focus=board?.getCenter(new T.Vector3())??neutral;
          // Follow the subject, keeping however far the visitor has moved the view.
          const shift=focus.clone().sub(aim);
          camera.position.add(shift);controls.target.add(shift);aim.copy(focus);look.retarget(aim);controls.update();
          body.visible=!staging;
          canvas.dataset.nativeFrame=String(frame);lastFrame=index;size();
        }
        cap.visible=cap.visible&&body.visible;
        for(const material of cloth.materials)material.clippingPlanes=limit;
        ink.clippingPlanes=limit;cloth.mesh.material=p.strain?[ink,ink,ink]:cloth.materials;
      }
      if(view!==p.view){view=p.view;orbit([.57,1.57,3.25][view]);}
      canvas.dataset.slice=sheet.visible?String(Math.round((y-floor)*1000)):"";
      canvas.dataset.cloth=String(!!cloth);render();
    }
    function size(){
      const w=el.clientWidth,h=el.clientHeight;if(!w||!h)return;
      const span=board?.getSize(new T.Vector3());
      const half=Math.max((ceiling-floor)*.69,.31*h/w,span?span.y*.62:0,span?Math.hypot(span.x,span.z)*.6*h/w:0);
      camera.left=-half*w/h;camera.right=-camera.left;camera.top=half;camera.bottom=-half;
      camera.updateProjectionMatrix();renderer.setSize(w,h,false);render();
    }
    function home(){
      camera.zoom=1;camera.updateProjectionMatrix();
      camera.position.add(aim.clone().sub(controls.target));controls.target.copy(aim);
      orbit([.57,1.57,3.25][Math.max(view,0)]);
    }
    function lost(e:Event){e.preventDefault();latest.current.onError();}
    controls.addEventListener("change",render);canvas.addEventListener("webglcontextlost",lost);
    const look=fullControls(controls,canvas,{paint:render,home,turnKeys:true});
    const observer=new ResizeObserver(size);observer.observe(el);
    draw.current=update;size();update(latest.current);
    if(new URLSearchParams(location.search).has("capture"))window.ftxTrouserCamera=orbit;
    return ()=>{
      draw.current=()=>{};if(window.ftxTrouserCamera===orbit)delete window.ftxTrouserCamera;
      observer.disconnect();look.dispose();controls.dispose();canvas.removeEventListener("webglcontextlost",lost);
      for(const g of meshes)g.dispose();for(const m of new Set(materials))m.dispose();
      cloth?.dispose();studio.dispose();renderer.dispose();renderer.forceContextLoss();canvas.remove();
    };
  },[props.model]);
  return <div className="ts-canvas" ref={host}/>;
}
