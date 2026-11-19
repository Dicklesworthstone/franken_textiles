import * as THREE from "three";
import { dressCloth } from "./atelier3d/garment";
import { fabrics } from "./atelier3d/weave";

export type Garment = "bolero" | "trousers";
export type StudioMode = "cloth" | "construction" | "strain" | "contact";
export type PanelOutline = { name: string; path: string; box: string; color: string };

export type Exhibit = {
  vertices: number;
  triangles: number[];
  panels: Record<string, [number, number]>;
  seams: Record<string, { side_a: number[]; side_b: number[] }>;
  frames: number[];
  sourceFrames: number;
  strainRgb: ArrayLike<number>;
  summary: { edgeStrainPercent: { min: number; max: number; p95: number } };
};

// One small repeat tile, anchored to the flat board so the weave follows the panels.
export function weaveTexture() {
  const side = 64;
  const data = new Uint8Array(side * side * 4);
  for (let y = 0; y < side; y++) {
    for (let x = 0; x < side; x++) {
      const warp = Math.floor(x / 8);
      const weft = Math.floor(y / 8);
      const ridge = ((warp + weft) % 2 ? x : y) % 8;
      const value = Math.round(191 + 55 * Math.sin((ridge + 0.5) * Math.PI / 8));
      const i = 4 * (y * side + x);
      data.set([value, value, value, 255], i);
    }
  }
  const map = new THREE.DataTexture(data, side, side, THREE.RGBAFormat);
  map.colorSpace = THREE.SRGBColorSpace;
  map.wrapS = map.wrapT = THREE.RepeatWrapping;
  map.generateMipmaps = true;
  map.minFilter = THREE.LinearMipmapLinearFilter;
  map.magFilter = THREE.LinearFilter;
  map.needsUpdate = true;
  return map;
}

async function fetchAsset(path: string, signal: AbortSignal) {
  const response = await fetch(path, { signal });
  if (!response.ok) throw new Error(`Could not load ${path} (${response.status})`);
  return response;
}

export async function loadGarment(garment: Garment, signal: AbortSignal) {
  const root = `/exhibits/${garment}`;
  const [data, buffer]: [Exhibit, ArrayBuffer] = await Promise.all([
    fetchAsset(root + ".json", signal).then((r) => r.json()),
    fetchAsset(root + ".bin", signal).then((r) => r.arrayBuffer()),
  ]);
  return createGarmentMesh(data, new Float32Array(buffer));
}

export function createGarmentMesh(data: Exhibit, frames: Float32Array, onBody = false, loads?: Float32Array) {
  const stride = data.vertices * 3;
  if (!stride || frames.length !== stride * data.frames.length ||
      data.strainRgb.length !== stride || data.frames[0] !== 0 ||
      data.frames.at(-1) !== data.sourceFrames - 1) {
    throw new Error("Incomplete garment replay");
  }
  const geometry = new THREE.BufferGeometry();
  // Diagram offsets are uniform within each cut piece, derived from its sewn endpoint.
  const endpoint = frames.subarray(frames.length - stride);
  const finalBounds = new THREE.Box3();
  const point = new THREE.Vector3();
  for (let i = 0; i < stride; i += 3) finalBounds.expandByPoint(point.fromArray(endpoint, i));
  const finalCenter = finalBounds.getCenter(new THREE.Vector3());
  const dimensions = finalBounds.getSize(new THREE.Vector3());
  const extent = Math.max(dimensions.x, dimensions.y, dimensions.z);
  const offsets = new Float32Array(stride);
  for (const [start, end] of Object.values(data.panels)) {
    const panelBounds = new THREE.Box3();
    for (let v = start; v < end; v++) panelBounds.expandByPoint(point.fromArray(endpoint, 3 * v));
    const delta = panelBounds.getCenter(new THREE.Vector3()).sub(finalCenter).normalize().multiplyScalar(extent * 0.38);
    for (let v = start; v < end; v++) delta.toArray(offsets, 3 * v);
  }
  geometry.setIndex(data.triangles);
  const position = new THREE.Float32BufferAttribute(new Float32Array(stride), 3);
  position.setUsage(THREE.DynamicDrawUsage);
  geometry.setAttribute("position", position);
  // The first frame is the flat pattern board: its x and z are each vertex's place on the cut piece, in metres.
  const flat = new Float32Array(data.vertices * 2);
  for (let i = 0; i < data.vertices; i++) {
    flat[i * 2] = frames[i * 3];
    flat[i * 2 + 1] = frames[i * 3 + 2];
  }
  const panelColors = new Float32Array(stride);
  const palette = ["#e3b24a", "#71a989", "#df8269", "#718cab", "#cfbc9b", "#c384aa", "#7db5bd"];
  // Count edges once on the flat board. Only exterior edges belong to the panel silhouette.
  const boundary = new Map<number,number>();
  for (let t=0; t<data.triangles.length; t+=3) for (let j=0;j<3;j++){
    const a=data.triangles[t+j], b=data.triangles[t+(j+1)%3];
    const key=Math.min(a,b)*data.vertices+Math.max(a,b);
    boundary.set(key,(boundary.get(key)??0)+1);
  }
  const outer=[...boundary].filter(([,count])=>count===1).map(([key])=>[Math.floor(key/data.vertices),key%data.vertices]);
  const panels: PanelOutline[]=Object.entries(data.panels).map(([name,[start,end]],i)=>{
    const flat=(v:number)=>[frames[v*3]*1000,-frames[v*3+2]*1000];
    const edges=outer.filter(([a,b])=>a>=start && b<end);
    const coords=edges.flatMap(([a,b])=>[flat(a),flat(b)]);
    const xs=coords.map(p=>p[0]), ys=coords.map(p=>p[1]);
    const x=Math.min(...xs),y=Math.min(...ys),w=Math.max(...xs)-x,h=Math.max(...ys)-y;
    const pad=Math.max(w,h)*.05;
    return {name,color:palette[i%palette.length],path:edges.map(([a,b])=>"M"+flat(a).join(",")+"L"+flat(b).join(",")).join(" "),
      box:[x-pad,y-pad,w+2*pad,h+2*pad].join(" ")};
  });
  for (const [index, [start, end]] of Object.values(data.panels).entries()) {
    const color = new THREE.Color(palette[index % palette.length]);
    for (let i = start; i < end; i++) color.toArray(panelColors, i * 3);
  }
  const strainColors = new Float32Array(stride);
  const rgb = new THREE.Color();
  for (let i = 0; i < stride; i += 3) {
    rgb.setRGB(data.strainRgb[i] / 255, data.strainRgb[i + 1] / 255, data.strainRgb[i + 2] / 255, THREE.SRGBColorSpace);
    rgb.toArray(strainColors, i);
  }
  // The garment itself: a shell with thickness, woven on the grain, seams and edges finished.
  const dressed = dressCloth({ vertices: data.vertices, triangles: data.triangles, seams: Object.values(data.seams), panels: data.panels },
    flat, onBody ? fabrics.greenWool : fabrics.madderWool, { sewn: endpoint, edge: onBody ? "braid" : "hem" });
  const shell = dressed.shell;
  shell.attribute("color", panelColors, 3, true);
  const contactColors = loads && new Float32Array(stride);
  if (loads && contactColors) {
    const palette = ["#23394c","#56b7ad","#f4c06d","#b42d32"].map(v=>new THREE.Color(v));
    const stops = [0,.02,.1,1];
    loads.forEach((v,i)=>{
      const end=v<=stops[1]?1:v<=stops[2]?2:3;
      rgb.lerpColors(palette[end-1],palette[end],Math.min(1,(v-stops[end-1])/(stops[end]-stops[end-1]))).toArray(contactColors,i*3);
    });
  }

  const cut = new THREE.MeshStandardMaterial({
    vertexColors: true, side: THREE.DoubleSide, roughness: 0.93,
    polygonOffset: true, polygonOffsetFactor: 1, polygonOffsetUnits: 1,
  });
  const strain = new THREE.MeshBasicMaterial({
    vertexColors: true, side: THREE.DoubleSide, toneMapped: false,
    polygonOffset: true, polygonOffsetFactor: 1, polygonOffsetUnits: 1,
  });
  const group = new THREE.Group();
  const fabric = dressed.mesh;
  const woven = dressed.materials, cutAll = [cut, cut, cut], strainAll = [strain, strain, strain];
  const lattice = new THREE.MeshBasicMaterial({
    color: "#ffdfad", side: THREE.DoubleSide, wireframe: true,
    transparent: true, opacity: 0.38, depthWrite: false,
  });
  const wire = new THREE.Mesh(geometry, lattice);
  const edges: number[] = [];
  for (const { side_a, side_b } of Object.values(data.seams)) {
    for (const side of [side_a, side_b]) {
      for (let i = 1; i < side.length; i++) edges.push(side[i - 1], side[i]);
    }
  }
  const seamGeometry = new THREE.BufferGeometry();
  seamGeometry.setAttribute("position", position);
  seamGeometry.setIndex(edges);
  const thread = new THREE.LineBasicMaterial({
    color: "#ffdfaa", transparent: true, opacity: 0.73, depthWrite: false,
  });
  const seams = new THREE.LineSegments(seamGeometry, thread);
  wire.renderOrder = 1;
  seams.renderOrder = 2;
  // The bound moves at every frame; the camera is always composed around the entire exhibit.
  fabric.frustumCulled = wire.frustumCulled = seams.frustumCulled = false;
  group.add(fabric, wire, seams);
  const seamNames = Object.keys(data.seams).sort();
  const capacity = Math.max(0, ...Object.values(data.seams).flatMap(s => [s.side_a.length, s.side_b.length]));
  // Both sides share the animated vertex buffer. Fixed screen-size dots remain legible from the
  // board to the sewn endpoint, where the larger outer dot is visible around the smaller one.
  function markSide(color: number, diameter: number, order: number) {
    const path = new THREE.BufferGeometry();
    path.setAttribute("position", position);
    const indices = new THREE.BufferAttribute(new Uint32Array(capacity), 1);
    path.setIndex(indices);
    const ink = new THREE.LineBasicMaterial({ color, toneMapped: false, depthTest: false, depthWrite: false });
    const dot = new THREE.PointsMaterial({ color, size: diameter, sizeAttenuation: false,
      toneMapped: false, depthTest: false, depthWrite: false });
    dot.onBeforeCompile = (shader) => {
      shader.fragmentShader = shader.fragmentShader.replace("#include <clipping_planes_fragment>",
        "#include <clipping_planes_fragment>\nif (distance(gl_PointCoord, vec2(0.5)) > 0.5) discard;");
    };
    const line = new THREE.Line(path, ink);
    const points = new THREE.Points(path, dot);
    line.renderOrder = order;
    points.renderOrder = order + 1;
    line.frustumCulled = points.frustumCulled = line.visible = points.visible = false;
    group.add(line, points);
    return {
      select(side: number[] | undefined) {
        line.visible = points.visible = !!side;
        if (!side) return;
        indices.array.set(side);
        indices.needsUpdate = true;
        path.setDrawRange(0, side.length);
      },
      dispose() { path.dispose(); ink.dispose(); dot.dispose(); },
    };
  }
  const sideA = markSide(0x40f3ee, 9, 3);
  const sideB = markSide(0xffb44c, 5, 5);
  let selected: Exhibit["seams"][string] | undefined;
  let lastFrame = -1;
  let lastSpread = -1;
  let colorMode: StudioMode = "construction";
  let highlighted = "";
  const size = new THREE.Vector3();
  const center = new THREE.Vector3();
  function pose(mode: StudioMode, time: number, showWire: boolean, aspect: number, seam?: string, spread = 0, panel = "") {
    const endpointView = mode === "strain" || mode === "contact";
    const sourceFrame = (endpointView ? 1 : THREE.MathUtils.clamp(time, 0, 1)) * (data.sourceFrames - 1);
    const separation = mode === "construction" ? THREE.MathUtils.clamp(spread, 0, 1) : 0;
    if (lastFrame !== sourceFrame || lastSpread !== separation) {
      let right = data.frames.findIndex((v) => v >= sourceFrame);
      if (right < 0) right = data.frames.length - 1;
      const left = Math.max(0, right - 1);
      const mix = (sourceFrame - data.frames[left]) / Math.max(1, data.frames[right] - data.frames[left]);
      const a = left * stride;
      const b = right * stride;
      for (let j = 0; j < stride; j += 3) {
        // Right-handed Z-up, front -Y → right-handed Y-up, front +Z.
        position.setXYZ(j / 3,
          THREE.MathUtils.lerp(frames[a + j], frames[b + j], mix) + offsets[j] * separation,
          THREE.MathUtils.lerp(frames[a + j + 2], frames[b + j + 2], mix) + offsets[j + 2] * separation,
          -(THREE.MathUtils.lerp(frames[a + j + 1], frames[b + j + 1], mix) + offsets[j + 1] * separation));
      }
      position.needsUpdate = true;
      shell.update(position.array);
      geometry.computeBoundingBox();
      geometry.boundingBox!.getSize(size);
      geometry.boundingBox!.getCenter(center);
      lastFrame = sourceFrame;
      lastSpread = separation;
    }
    if (!onBody) {
      const scale = 1.66 / Math.max(size.y + size.z * 0.16, Math.hypot(size.x, size.z) / aspect, 0.001);
      group.scale.setScalar(scale);
      group.position.copy(center).multiplyScalar(-scale);
    }
    fabric.material = endpointView ? strainAll : mode === "construction" ? cutAll : woven;
    const focus = mode === "construction" && Object.hasOwn(data.panels,panel) ? panel : "";
    if (colorMode !== mode || highlighted !== focus) {
      highlighted=focus;
      colorMode = mode;
      const shown = (mode==="contact" && contactColors ? contactColors : mode==="strain" ? strainColors : panelColors).slice();
      if (focus) {
        const [start,end]=data.panels[focus];
        const muted=new THREE.Color("#465052");
        for(let v=0;v<data.vertices;v++) if(v<start || v>=end) muted.toArray(shown,v*3);
      }
      shell.attribute("color", shown, 3, true);
    }
    const pair = mode === "construction" && seam && Object.hasOwn(data.seams, seam) ? data.seams[seam] : undefined;
    if (selected !== pair) {
      selected = pair;
      sideA.select(pair?.side_a);
      sideB.select(pair?.side_b);
      cut.color.setHex(pair ? 0x747b79 : 0xffffff);
      lattice.opacity = pair ? 0.12 : 0.38;
    }
    wire.visible = showWire;
    // On the woven cloth a seam is shown by the cloth itself. The drawn seam lines belong to the cut-panel diagram.
    seams.visible = mode === "construction" && !pair;
  }
  return { group, pose, seamNames, panels, strainRange: data.summary.edgeStrainPercent, dispose() {
    geometry.dispose();
    seamGeometry.dispose();
    sideA.dispose();
    sideB.dispose();
    dressed.dispose();
    for (const material of [cut, strain, lattice, thread]) material.dispose();
  }};
}
