"use client";

import { useEffect, useRef } from "react";
import * as T from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";
import type { Experiment, Specimen } from "./materialExperiments";

declare global { interface Window { ftxRigView?: (yaw: number) => void } }
type CameraView = { name: "Perspective" | "Front" | "Top"; revision: number };
type Props = { data: Experiment; step: number; mesh: boolean; camera: CameraView; onFailure: () => void };

function textile(color: string, angle: number, tubular: boolean) {
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 256;
  const ctx = canvas.getContext("2d")!;
  ctx.fillStyle = color;
  ctx.fillRect(0, 0, 256, 256);
  for (let i = 0; i < 256; i += 4) {
    ctx.fillStyle = "rgba(255,241,207,.32)"; ctx.fillRect(i, 0, 1.5, 256);
    ctx.fillStyle = "rgba(13,21,25,.31)"; ctx.fillRect(0, i, 256, 1.5);
  }
  // Woven register stripes travel with the specimen's material coordinates.
  for (const start of [12, 22, 94]) {
    ctx.fillStyle = "rgba(249,217,153,.77)"; ctx.fillRect(start, 0, start === 94 ? 2 : 4, 256);
  }
  for (const y of [24, 29, 152]) {
    ctx.fillStyle = "rgba(241,217,170,.48)"; ctx.fillRect(0, y, 256, 2);
  }
  const texture = new T.CanvasTexture(canvas);
  texture.wrapS = texture.wrapT = T.RepeatWrapping;
  texture.colorSpace = T.SRGBColorSpace;
  texture.repeat.set(tubular ? 4 : 1, 1);
  texture.center.set(.5,.5); texture.rotation = angle; texture.anisotropy = 4;
  return texture;
}

function buildSpecimen(scene: T.Scene, model: Specimen, index: number, tubular: boolean,
    steel: T.Material, brass: T.Material, black: T.Material) {
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
    // Equal physical UV units keep bias stripes at 45° on the rectangular reference strip.
    uv.push(col/nx * (tubular ? 1 : model.width/40), row/ny * (tubular ? 1 : model.height/40));
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
  const map = textile(tubular ? "#884524" : ["#324e5e","#905729","#59633d"][index],
    tubular ? 0 : [0, Math.PI/2, Math.PI/4][index], tubular);
  const cloth = new T.MeshStandardMaterial({ map, roughness: .88, side: T.DoubleSide, polygonOffset: true, polygonOffsetFactor: 1, polygonOffsetUnits: 1 });
  part(geom, cloth, 0, 0, 0);
  const wire = new T.MeshBasicMaterial({ color: "#fbedc6", wireframe: true, transparent: true, opacity: .60 });
  const lattice = new T.Mesh(geom, wire); group.add(lattice);
  const top = new T.Group(); group.add(top);
  for (const upper of [false, true]) {
    const holder = upper ? top : group;
    const sign = upper ? 1 : -1;
    if (tubular) {
      const radius = model.width / 2;
      part(new T.CylinderGeometry(radius+3,radius+3,8,80), brass, 0,sign*4,0,holder);
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
      part(new T.BoxGeometry(model.width+8,9,9), steel, 0,sign*4,0,holder);
      part(new T.BoxGeometry(model.width+6,2,10), brass, 0,sign*1,0,holder);
      part(new T.BoxGeometry(12,11,6), black, 0,sign*12,0,holder);
      for (const x of [-model.width*.38,model.width*.38]) {
        const bolt = part(new T.CylinderGeometry(2,2,1.5,6), black, x,sign*5,4.8,holder);
        bolt.rotation.x = Math.PI/2;
      }
    }
  }
  return (step: number, grid: boolean, angle: number) => {
    const points = model.frames[step].positions_m;
    source.forEach((v, i) => {
      const [x, y, z] = points[v];
      position.setXYZ(i, x*1000 - (tubular ? 0 : model.width/2), (tubular ? z : y)*1000, (tubular ? -y : z)*1000);
    });
    position.needsUpdate = true;
    geom.computeVertexNormals(); geom.computeBoundingSphere();
    const end = points[points.length-1];
    top.position.y = (tubular ? end[2] : end[1])*1000;
    top.rotation.y = tubular ? angle*Math.PI/180 : 0;
    lattice.visible = grid;
  };
}

export default function MaterialScene({ data, step, mesh, camera, onFailure }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const update = useRef<((step: number, mesh: boolean, view: CameraView) => void) | null>(null);
  useEffect(() => {
    const surface = host.current!;
    let renderer: T.WebGLRenderer;
    try { renderer = new T.WebGLRenderer({ antialias: true, alpha: true, powerPreference: "low-power" }); }
    catch { onFailure(); return; }
    const tubular = data.kind === "cylinder";
    renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
    renderer.toneMapping = T.ACESFilmicToneMapping;
    renderer.toneMappingExposure = .98;
    renderer.shadowMap.enabled = true; renderer.shadowMap.type = T.PCFSoftShadowMap;
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
    key.position.set(-75,240,100); key.castShadow=true; key.shadow.mapSize.set(1024,1024);
    Object.assign(key.shadow.camera,{left:-185,right:185,top:170,bottom:-180,far:650}); key.shadow.camera.updateProjectionMatrix(); key.shadow.bias=-.001;
    scene.add(key);
    const rim = new T.DirectionalLight(0xd8edf3,2); rim.position.set(70,90,-110);scene.add(rim);
    const steel = new T.MeshStandardMaterial({color:0x939ca1,metalness:.88,roughness:.27});
    const brass = new T.MeshStandardMaterial({color:0xb08a4f,metalness:.76,roughness:.32});
    const black = new T.MeshStandardMaterial({color:0x344049,metalness:.60,roughness:.34});
    const poses = data.specimens.map((s,i)=>buildSpecimen(scene,s,i,tubular,steel,brass,black));
    const floor = new T.Mesh(new T.PlaneGeometry(4000,4000),new T.ShadowMaterial({opacity:.17}));
    floor.rotation.x=-Math.PI/2; floor.position.y=tubular?-30:-18; floor.receiveShadow=true;scene.add(floor);
    const resize = () => {
      const w = surface.clientWidth, h = surface.clientHeight;
      if (!w || !h) return;
      const aspect=w/h, half=Math.max(tubular?72:89,(tubular?72:145)/aspect);
      renderer.setSize(w,h,false); cam.left=-half*aspect; cam.right=half*aspect; cam.top=half; cam.bottom=-half;
      cam.updateProjectionMatrix(); draw();
    };
    const observer = new ResizeObserver(resize); observer.observe(surface);
    let last = -1;
    update.current = (step,mesh,view) => {
      poses.forEach(apply=>apply(step,mesh,data.readings[step].angle));
      if (view.revision !== last) {
        last=view.revision; cam.zoom=1; cam.updateProjectionMatrix();
        orient(view.name==="Perspective"?home:0,view.name==="Top"?1.47:view.name==="Front"?0:.24);
      }
      draw(); el.dataset.step=String(step);
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
      update.current=null; if(window.ftxRigView===capture) delete window.ftxRigView;
      observer.disconnect(); orbit.removeEventListener("change",draw);orbit.dispose();
      el.removeEventListener("keydown",keydown);el.removeEventListener("webglcontextlost",lost);
      const resources=new Set<T.BufferGeometry | T.Material | T.Texture>();
      scene.traverse(o=>{if(o instanceof T.Mesh){resources.add(o.geometry);for(const m of [o.material].flat()){
        resources.add(m);if(m.map)resources.add(m.map);
      }}});
      resources.forEach(resource=>resource.dispose());key.shadow.dispose();environment.dispose();
      renderer.dispose();renderer.forceContextLoss();el.remove();
    };
  },[data,onFailure]);
  useEffect(()=>{update.current?.(step,mesh,camera);},[step,mesh,camera,data]);
  return <div className="rig-scene" ref={host}/>;
}
