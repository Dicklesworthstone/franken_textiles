"use client";

import { useEffect, useRef } from "react";
import * as T from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";
import type { Experiment, Specimen } from "./materialExperiments";

declare global { interface Window { ftxRigView?: (yaw: number) => void } }
type CameraView = { name: "Perspective" | "Fabric detail" | "Front" | "Top"; revision: number };
type Props = { data: Experiment; step: number; mesh: boolean; focus: number; camera: CameraView; onFailure: () => void };

function buildSpecimen(scene: T.Scene, model: Specimen, index: number, tubular: boolean,
    steel: T.Material, insert: T.Material, black: T.Material, cloth: T.Material) {
  const group = new T.Group();
  group.position.x = tubular ? 0 : (index - 1) * (model.width + 32);
  scene.add(group);
  function part(geometry: T.BufferGeometry, material: T.Material, x: number, y: number, z: number, parent: T.Object3D = group) {
    const object = new T.Mesh(geometry, material);
    object.position.set(x, y, z); object.castShadow = object.receiveShadow = true; parent.add(object);
    return object;
  }
  const [nx, ny] = model.cells;
  const geom = new T.BufferGeometry();
  // Duplicate each cylinder's wrap vertex for a continuous pattern through the UV seam.
  const source: number[] = [], uv: number[] = [];
  for (let row = 0; row <= ny; row++) for (let col = 0; col <= nx; col++) {
    source.push(tubular ? row * nx + col % nx : row * (nx + 1) + col);
    // The photographed repeat measures 270.7 by 271.3 mm. Rotate the cut in material space.
    const circumference=tubular?nx*model.width*Math.sin(Math.PI/nx):model.width;
    const x=col/nx*circumference,y=row/ny*model.height,angle=tubular?0:[0,Math.PI/2,Math.PI/4][index];
    uv.push((x*Math.cos(angle)-y*Math.sin(angle))/270.7,(x*Math.sin(angle)+y*Math.cos(angle))/271.3);
  }
  const indices = [...model.triangles];
  if (tubular) for (let i = 0; i < indices.length; i += 3) {
    const seam = indices.slice(i, i+3).some(v => v % nx === nx-1);
    for (let j = i; j < i+3; j++) {
      const v = indices[j], col = v % nx;
      indices[j] = Math.floor(v / nx) * (nx + 1) + (seam && col === 0 ? nx : col);
    }
  }
  geom.setIndex(indices);
  const position = new T.Float32BufferAttribute(new Float32Array(source.length * 3), 3);
  geom.setAttribute("position", position);
  geom.setAttribute("uv", new T.Float32BufferAttribute(uv, 2));
  part(geom, cloth, 0, 0, 0);
  const wire = new T.MeshBasicMaterial({ color: "#fbedc6", wireframe: true, transparent: true, opacity: .60 });
  const lattice = new T.Mesh(geom, wire); group.add(lattice);
  const top = new T.Group(); group.add(top);
  function middleRing(points:Specimen["frames"][number]["positions_m"]){
    return Array.from({length:nx+1},(_,i)=>{
      const [x,y,z]=points[ny/2*nx+i%nx];return new T.Vector3(x*1000,z*1000,-y*1000);
    });
  }
  const cross=tubular?new T.Line(new T.BufferGeometry().setFromPoints(middleRing(model.frames[0].positions_m)),
    new T.LineBasicMaterial({color:0x183d51,depthTest:false})):null;
  const reference=tubular?new T.Line(new T.BufferGeometry().setFromPoints(middleRing(model.frames[0].positions_m)),
    new T.LineDashedMaterial({color:0xb65316,dashSize:2,gapSize:1,depthTest:false})):null;
  if(cross&&reference){reference.computeLineDistances();group.add(reference,cross);cross.renderOrder=4;reference.renderOrder=3;}
  const guide=new T.Group();group.add(guide);
  if(!tubular){
    // The carriage runs behind the jaws; only the recorded end rows carry specimen load.
    part(new T.BoxGeometry(8,model.height+126,8),steel,model.width*.70,model.height*.5,-16,guide);
    part(new T.BoxGeometry(4,model.height+126,1.6),black,model.width*.70,model.height*.5,-11.8,guide);
  }
  for (const upper of [false, true]) {
    const holder = upper ? top : group;
    const sign = upper ? 1 : -1;
    if (tubular) {
      const radius = model.width / 2;
      part(new T.CylinderGeometry(radius+3,radius+3,8,80), insert, 0,sign*4,0,holder);
      part(new T.CylinderGeometry(radius+4,radius+4,4,80), steel, 0,sign*10,0,holder);
      part(new T.CylinderGeometry(radius*.62,radius*.75,7,64), black, 0,sign*15,0,holder);
      part(new T.CylinderGeometry(8,8,13,32), steel, 0,sign*23,0,holder);
      for (let k = 0; k < 60; k++) {
        const angle = k * Math.PI / 30, r = radius + 3.1;
        const tick = part(new T.BoxGeometry(k % 5 === 0 ? .75 : .35,k % 5 === 0 ? 5.8 : 2.7,.28), black,
          r*Math.sin(angle),sign*4,r*Math.cos(angle),holder);
        tick.rotation.y = angle;
      }
    } else {
      part(new T.BoxGeometry(model.width+9,11,8), steel, 0,sign*5.5,-3.9,holder);
      part(new T.BoxGeometry(model.width+9,9,5), black, 0,sign*4.5,2.6,holder);
      part(new T.BoxGeometry(model.width+6,1.2,3), insert, 0,sign*.6,1.45,holder);
      part(new T.BoxGeometry(18,15,9), steel, 0,sign*15,0,holder);
      part(new T.CylinderGeometry(3.8,3.8,59,32),steel,0,sign*51,0,holder);
      part(new T.CylinderGeometry(6.4,6.4,4,32),black,0,sign*24,0,holder);
      part(new T.BoxGeometry(model.width*.76,10,6),black,model.width*.34,sign*14,-10,holder);
      for (const x of [-model.width*.38,model.width*.38]) {
        const bolt = part(new T.CylinderGeometry(2,2,1.4,20), steel, x,sign*5,5.6,holder);
        bolt.rotation.x = Math.PI/2;
        part(new T.BoxGeometry(2.2,.45,.2),black,x,sign*5,6.35,holder);
      }
    }
  }
  return (step: number, grid: boolean, angle: number, focus: number, section:boolean) => {
    group.visible=tubular||focus===-1||focus===index;
    if(!tubular){
      guide.visible=focus!==-1;
      group.rotation.z=focus===-1?0:-Math.PI/2;
      group.position.set(focus===-1?(index-1)*(model.width+24):-model.height*.53,focus===-1?0:47,0);
    }
    const points = model.frames[step].positions_m;
    source.forEach((v, i) => {
      const [x, y, z] = points[v];
      position.setXYZ(i, x*1000 - (tubular ? 0 : model.width/2), (tubular ? z : y)*1000, (tubular ? -y : z)*1000);
    });
    position.needsUpdate = true;
    geom.computeVertexNormals(); geom.computeBoundingSphere();
    if(tubular){
      const normals=geom.getAttribute("normal"),mean=new T.Vector3(),other=new T.Vector3();
      for(let row=0;row<=ny;row++){
        const a=row*(nx+1),b=a+nx;
        mean.fromBufferAttribute(normals,a).add(other.fromBufferAttribute(normals,b)).normalize();
        normals.setXYZ(a,mean.x,mean.y,mean.z);normals.setXYZ(b,mean.x,mean.y,mean.z);
      }
      normals.needsUpdate=true;
    }
    const end = points[points.length-1];
    // The native topology orders pairs of triangles around each ring before the next row.
    top.visible=!section;
    geom.setDrawRange(0,section?nx*(ny/2)*6:indices.length);
    if(cross&&reference){cross.visible=reference.visible=section;if(section)cross.geometry.setFromPoints(middleRing(points));}
    top.position.y = (tubular ? end[2] : end[1])*1000;
    top.rotation.y = tubular ? angle*Math.PI/180 : 0;
    lattice.visible = grid;
  };
}

export default function MaterialScene({ data, step, mesh, focus, camera, onFailure }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const update = useRef<((step: number, mesh: boolean, focus:number, view: CameraView) => void) | null>(null);
  useEffect(() => {
    const surface = host.current!;
    let renderer: T.WebGLRenderer;
    try { renderer = new T.WebGLRenderer({ antialias: true, alpha: true, powerPreference: "low-power" }); }
    catch { onFailure(); return; }
    const tubular = data.kind === "cylinder";
    renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
    renderer.toneMapping = T.ACESFilmicToneMapping;
    renderer.toneMappingExposure = .98;
    renderer.shadowMap.enabled = true; renderer.shadowMap.type = T.PCFShadowMap;
    const el = renderer.domElement;
    surface.appendChild(el); el.tabIndex=0; el.setAttribute("role","img");
    el.setAttribute("aria-label", "3D material experiment. Drag or use arrow keys to orbit. Use plus and minus to zoom.");
    const scene = new T.Scene();
    const generator = new T.PMREMGenerator(renderer), room = new RoomEnvironment();
    const environment = generator.fromScene(room, .04); scene.environment = environment.texture; scene.environmentIntensity=.75;
    room.dispose(); generator.dispose();
    const targetY = tubular ? 23 : 47;
    const cam = new T.OrthographicCamera(-90,90,90,-90,.1,2400);
    const orbit = new OrbitControls(cam, el);
    orbit.target.set(0,targetY,0); orbit.enablePan=false; orbit.minZoom=.6; orbit.maxZoom=3;
    const draw = () => { renderer.render(scene,cam); };
    function orient(yaw: number, elevation: number) {
      cam.position.set(450*Math.sin(yaw)*Math.cos(elevation), targetY+450*Math.sin(elevation), 450*Math.cos(yaw)*Math.cos(elevation));
      orbit.update(); draw();
    }
    const home = tubular ? .52 : .15;
    orient(home,.24);
    scene.add(new T.HemisphereLight(0xffe9cd,0x737e81,.85));
    const key = new T.DirectionalLight(0xffead3,2.35);
    key.position.set(-190,140,75); key.castShadow=true; key.shadow.mapSize.set(1024,1024);
    Object.assign(key.shadow.camera,{left:-185,right:185,top:170,bottom:-180,far:650}); key.shadow.camera.updateProjectionMatrix(); key.shadow.bias=-.001;
    scene.add(key);
    const rim = new T.DirectionalLight(0xd8edf3,2); rim.position.set(70,90,-110);scene.add(rim);
    const steel = new T.MeshStandardMaterial({color:0x939ca1,metalness:.88,roughness:.27});
    const insert = new T.MeshStandardMaterial({color:0x817668,metalness:.62,roughness:.46});
    const black = new T.MeshStandardMaterial({color:0x28312f,metalness:.63,roughness:.49});
    let alive=true;
    const loader=new T.TextureLoader(),loaded=()=>{if(alive)draw();};
    const map=loader.load("/materials/linen.webp",loaded), normalMap=loader.load("/materials/linen-normal.webp",loaded);
    for(const tex of [map,normalMap]){tex.wrapS=tex.wrapT=T.RepeatWrapping;tex.anisotropy=renderer.capabilities.getMaxAnisotropy();}
    map.colorSpace=T.SRGBColorSpace;
    const cloth=new T.MeshPhysicalMaterial({map,normalMap,roughness:.98,sheen:.32,sheenColor:0x929ea6,sheenRoughness:.96,
      side:T.DoubleSide,polygonOffset:true,polygonOffsetFactor:1,polygonOffsetUnits:1});
    const poses = data.specimens.map((s,i)=>buildSpecimen(scene,s,i,tubular,steel,insert,black,cloth));
    const floor = new T.Mesh(new T.PlaneGeometry(4000,4000),new T.ShadowMaterial({opacity:.17}));
    floor.rotation.x=-Math.PI/2; floor.position.y=tubular?-30:-18; floor.receiveShadow=true;scene.add(floor);
    let selected=2;
    const resize = () => {
      const w = surface.clientWidth, h = surface.clientHeight;
      if (!w || !h) return;
      const aspect=w/h, half=tubular?Math.max(51,50/aspect):selected===-1?Math.max(78,130/aspect):Math.max(44,80/aspect);
      renderer.setSize(w,h,false); cam.left=-half*aspect; cam.right=half*aspect; cam.top=half; cam.bottom=-half;
      cam.updateProjectionMatrix(); draw();
    };
    const observer = new ResizeObserver(resize); observer.observe(surface);
    let last = -1;
    update.current = (step,mesh,focus,view) => {
      if(selected!==focus){selected=focus;resize();}
      poses.forEach(apply=>apply(step,mesh,data.readings[step].angle,focus,tubular&&view.name==="Top"));
      if (view.revision !== last) {
        last=view.revision; cam.zoom=view.name==="Fabric detail"?(tubular?1.7:1.5):1; cam.updateProjectionMatrix();
        const yaw=view.name==="Perspective"?home:view.name==="Fabric detail"?-.19:view.name==="Top"&&!tubular?Math.PI:0;
        orient(yaw,view.name==="Top"&&tubular?1.53:view.name==="Fabric detail"?.10:view.name==="Perspective"?.24:0);
      }
      draw(); el.dataset.step=String(step);el.dataset.specimen=tubular?"Cylinder":data.specimens[focus]?.name??"All cuts";
    };
    const capture = (angle: number) => orient(angle,.24);
    if (new URLSearchParams(location.search).has("capture")) window.ftxRigView=capture;
    const keydown = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      let yaw=orbit.getAzimuthalAngle(), elevation=Math.PI/2-orbit.getPolarAngle();
      switch(e.key) {
        case "ArrowLeft": yaw-=.12; break; case "ArrowRight": yaw+=.12; break;
        case "ArrowUp": elevation+=.12; break; case "ArrowDown": elevation-=.12; break;
        case "+": case "=": cam.zoom=Math.min(3,cam.zoom*1.15);break;
        case "-": cam.zoom=Math.max(.6,cam.zoom/1.15);break; default:return;
      }
      e.preventDefault();cam.updateProjectionMatrix();orient(yaw,Math.max(-1.46,Math.min(1.46,elevation)));
    };
    const lost = (e: Event) => { e.preventDefault();onFailure(); };
    orbit.addEventListener("change",draw); el.addEventListener("keydown",keydown);el.addEventListener("webglcontextlost",lost);
    resize();
    return () => {
      alive=false;update.current=null; if(window.ftxRigView===capture) delete window.ftxRigView;
      observer.disconnect(); orbit.removeEventListener("change",draw);orbit.dispose();
      el.removeEventListener("keydown",keydown);el.removeEventListener("webglcontextlost",lost);
      const resources=new Set<T.BufferGeometry | T.Material | T.Texture>();
      scene.traverse(o=>{if(o instanceof T.Mesh||o instanceof T.Line){resources.add(o.geometry);for(const m of [o.material].flat()){
        resources.add(m);if(m.map)resources.add(m.map);
      }}});
      resources.forEach(resource=>resource.dispose());normalMap.dispose();key.shadow.dispose();environment.dispose();
      renderer.dispose();renderer.forceContextLoss();el.remove();
    };
  },[data,onFailure]);
  useEffect(()=>{update.current?.(step,mesh,focus,camera);},[step,mesh,focus,camera,data]);
  return <div className="rig-scene" ref={host}/>;
}
