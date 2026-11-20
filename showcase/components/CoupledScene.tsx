"use client";
import {useEffect,useRef} from "react";
import * as T from "three";
import {OrbitControls} from "three/addons/controls/OrbitControls.js";
import joint from "../public/exhibits/poses/ensemble.json";
import {dressCloth} from "./atelier3d/garment";
import {fabrics} from "./atelier3d/weave";
import {formMaterial,lightStudio,prepareRenderer,shadowFloor} from "./atelier3d/studio";
import {cuttingTable,displayFrame} from "./construction/replay";

export default function CoupledScene({frame,onReady,onError}:{frame:number;onReady:(key:string)=>void;onError:(key:string)=>void}){
  const host=useRef<HTMLDivElement>(null),step=useRef(frame),update=useRef<()=>void>(()=>{});
  useEffect(()=>{step.current=frame;update.current();},[frame]);
  useEffect(()=>{
    const el=host.current!,abort=new AbortController(),{signal}=abort;
    let renderer:T.WebGLRenderer;try{renderer=new T.WebGLRenderer({antialias:true,alpha:true});}catch{onError(joint.case);return;}
    renderer.setClearColor(0,0);prepareRenderer(renderer);
    const canvas=renderer.domElement;el.append(canvas);canvas.tabIndex=0;canvas.setAttribute("role","img");
    canvas.setAttribute("aria-label","Bolero and trousers sewn together in one cloth-contact simulation. Drag or use arrow keys to turn.");
    const scene=new T.Scene(),camera=new T.OrthographicCamera(-1,1,1,-1,.01,6);
    const controls=new OrbitControls(camera,canvas);controls.target.set(0,-.056,.067);camera.position.set(.7,.17,1.7);controls.update();
    controls.enablePan=false;controls.enableZoom=false;controls.minPolarAngle=1.19;controls.maxPolarAngle=1.68;
    const draw=()=>renderer.render(scene,camera);
    const key=(e:KeyboardEvent)=>{if(!["ArrowLeft","ArrowRight"].includes(e.key))return;e.preventDefault();
      const d=camera.position.clone().sub(controls.target).applyAxisAngle(new T.Vector3(0,1,0),e.key==="ArrowLeft"?-.16:.16);
      camera.position.copy(d.add(controls.target));controls.update();};
    const lost=(e:Event)=>{e.preventDefault();onError(joint.case);};
    controls.addEventListener("change",draw);canvas.addEventListener("keydown",key);canvas.addEventListener("webglcontextlost",lost);
    // Half the figure's height and half its width; set from the body once it has loaded, so the whole figure is in view.
    let tall=.359,wide=.31;
    const resize=()=>{const w=el.clientWidth,h=el.clientHeight;if(!w||!h)return;const a=w/h,half=Math.max(tall,wide/a);
      Object.assign(camera,{left:-half*a,right:half*a,top:half,bottom:-half});camera.updateProjectionMatrix();renderer.setSize(w,h,false);draw();};
    const observer=new ResizeObserver(resize);observer.observe(el);resize();
    const studio=lightStudio(renderer,scene,"warm");
    const body=new T.BufferGeometry(),form=formMaterial();
    const disposables:{dispose():void}[]=[];
    async function get(path:string){const r=await fetch(path,{signal});if(!r.ok||!r.body)throw Error("Missing coupled drape");return r;}
    Promise.all([get("/exhibits/poses/ensemble.gz").then(r=>new Response(r.body!.pipeThrough(new DecompressionStream("gzip"))).arrayBuffer()),
      ...["bolero","trousers"].map(name=>get("/exhibits/"+name+".json").then(r=>r.json()))]).then(([data,upper,lower])=>{
      if(signal.aborted)return;
      const head=new DataView(data),bn=head.getUint32(0,true),bi=head.getUint32(4,true),n=upper.vertices+lower.vertices;
      const bodyXYZ=new Float32Array(data,8,bn*3),bodyIndex=new Uint32Array(data,8+bn*12,bi);
      body.setAttribute("position",new T.Float32BufferAttribute(bodyXYZ,3));body.setIndex(new T.BufferAttribute(bodyIndex,1));body.rotateX(-Math.PI/2);body.computeVertexNormals();
      const figure=new T.Mesh(body,form);figure.castShadow=figure.receiveShadow=true;scene.add(figure);
      body.computeBoundingBox();body.computeBoundingSphere();
      studio.frame(body.boundingSphere!.center,body.boundingSphere!.radius);
      const box=body.boundingBox!,middle=box.getCenter(new T.Vector3()),extent=box.getSize(new T.Vector3());
      tall=extent.y*.54;wide=Math.max(extent.x,extent.z)*.56;
      camera.position.add(new T.Vector3(0,middle.y-controls.target.y,0));controls.target.set(0,middle.y,middle.z);controls.update();resize();
      const floor=shadowFloor(body.boundingBox!.min.y-.0005);scene.add(floor.mesh);disposables.push(floor);
      // The first native sample supplies flat pattern coordinates. Bolero vertices come first.
      const packed=new Float32Array(data,8+bn*12+bi*4);
      if(packed.length!==n*3*joint.frames)throw Error("Mismatched native outfit cache");
      const last=(joint.frames-1)*n*3;
      // Right-handed Z-up, front -Y → right-handed Y-up, front +Z.
      const upright=(from:number,count:number,out=new Float32Array(count*3))=>{
        for(let i=0;i<count*3;i+=3){out[i]=packed[from+i];out[i+1]=packed[from+i+2];out[i+2]=-packed[from+i+1];}
        return out;};
      const garments=([[upper,0,fabrics.greenWool,"braid"],[lower,upper.vertices,fabrics.rustTwill,"hem"]] as const).map(([layout,start,fabric,edge])=>{
        const flat=new Float32Array(layout.vertices*2);
        for(let i=0;i<layout.vertices;i++){flat[i*2]=packed[(start+i)*3];flat[i*2+1]=packed[(start+i)*3+2];}
        const garment=dressCloth({vertices:layout.vertices,triangles:layout.triangles,seams:Object.values(layout.seams),panels:layout.panels},
          flat,fabric,{sewn:upright(last+start*3,layout.vertices),edge});
        scene.add(garment.mesh);disposables.push(garment);
        return {garment,start,points:new Float32Array(layout.vertices*3)};
      });
      const xyz=new Float32Array(n*3),ranges:number[][]=[];
      for(const [layout,start] of [[upper,0],[lower,upper.vertices]] as const)
        for(const [a,b] of Object.values(layout.panels) as number[][])ranges.push([a+start,b+start]);
      const arrange=cuttingTable(upright(0,n,xyz),ranges);
      update.current=()=>{const index=displayFrame(Math.round((joint.frames-1)*step.current/135),joint.phases);
        const board=new T.Box3(),tracking=index<joint.phases.fold;
        upright(index*n*3,n,xyz);if(tracking)arrange(xyz);
        for(const {garment,start,points} of garments){
          points.set(xyz.subarray(start*3,start*3+points.length));garment.update(points);
          if(tracking)board.union(new T.Box3().setFromBufferAttribute(new T.BufferAttribute(points,3)));
        }
        figure.visible=floor.mesh.visible=!tracking;
        const focus=tracking?board.getCenter(new T.Vector3()):middle;
        const span=tracking?board.getSize(new T.Vector3()):extent;
        tall=Math.max(extent.y*.54,span.y*.58);wide=tracking?Math.hypot(span.x,span.z)*.57:Math.max(extent.x,extent.z)*.56;
        camera.position.add(focus.clone().sub(controls.target));controls.target.copy(focus);controls.update();resize();
        canvas.dataset.nativeFrame=String(index);draw();};
      update.current();canvas.dataset.run=joint.case;canvas.dataset.full="true";canvas.dataset.coupled="true";onReady(joint.case);
    }).catch(()=>{if(!signal.aborted)onError(joint.case);});
    return ()=>{abort.abort();update.current=()=>{};observer.disconnect();controls.dispose();canvas.removeEventListener("keydown",key);canvas.removeEventListener("webglcontextlost",lost);
      for(const item of disposables)item.dispose();
      body.dispose();form.dispose();studio.dispose();renderer.dispose();renderer.forceContextLoss();canvas.remove();};
  },[onReady,onError]);
  return <div className="dr-canvas" ref={host}/>;
}
