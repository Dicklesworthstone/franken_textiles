"use client";

import { useEffect, useRef } from "react";
import * as T from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { dressCloth } from "./garment";
import type { Fabric } from "./weave";
import { formMaterial, lightStudio, prepareRenderer, shadowFloor } from "./studio";

// A replay of one packaged `ftx drape` run (scripts/drape_package.py): the solver's own frames from the flat
// cut to the settled garment, on the body it was draped on. What this file adds is display only and each
// exhibit's method note says so: cloth thickness and weave, the smoothing between solver points, cord drawn
// along the pattern's decoration lines, an optional jacket mount, and the form drawn slightly inside its
// solved surface.

export interface DrapeInfo { frames: number; phases: { fold: number; zip: number; sew: number; drape: number }; sourceFrames: number[] }
/** Where the camera looks (metres above the floor), half the height it shows, and its angle round the form. */
export interface DrapeView { target: number; span: number; yaw: number }
export interface DrapeLook { fabric: Fabric; edge?: "braid" | "hem"; cord?: string; button?: string }
type Pose = (yaw: number, targetCm: number, spanCm: number) => void;
declare global { interface Window { ftxHussarFrame?: Pose; ftxDressFrameDrape?: Pose } }

export interface SolverDrapeProps {
  /** The packaged drape without its extension: `<source>.json` and `<source>.gz`. */
  source: string;
  looks: Record<string, DrapeLook>; look: string;
  views: Record<string, DrapeView>; view: { preset: string };
  /** Index into the package's kept frames. */
  frame: number;
  /** Show the form as a jacket mount, ending it this far above the floor (metres) with a plate and a pole. */
  mountAt?: number;
  label: string; className: string;
  /** With `?capture=1` in the address, the window function that poses the camera for a film script. */
  captureHook?: "ftxHussarFrame" | "ftxDressFrameDrape";
  onReady: (info: DrapeInfo) => void; onFailure: () => void;
}

interface Package {
  vertices: number; triangles: number[]; panels: Record<string, [number, number]>;
  seams: Record<string, { side_a: number[]; side_b: number[] }>;
  frames: number[]; phases: DrapeInfo["phases"];
  cords: { width_mm: number; anchors: number[] }[]; buttons: { anchor: [number, number, number] }[];
}
// How far from the axis counts as trunk and legs, not arms and hands, when the form is cut for a mount; and how
// far inside the solver's surface the form is drawn, so flat cloth facets over a curved arm do not show it through.
const TRUNK_HALF_WIDTH = 0.27, FORM_INSET = 0.0025;

export default function SolverDrape(props: SolverDrapeProps) {
  const { source, mountAt, label, className, captureHook, onReady, onFailure } = props;
  const host = useRef<HTMLDivElement>(null), apply = useRef<(() => void) | null>(null), latest = useRef(props);
  useEffect(() => { latest.current = props; apply.current?.(); });
  useEffect(() => {
    const surface = host.current!, abort = new AbortController(), { signal } = abort;
    let renderer: T.WebGLRenderer;
    try { renderer = new T.WebGLRenderer({ alpha: true, antialias: true }); } catch { onFailure(); return; }
    renderer.setClearColor(0, 0); prepareRenderer(renderer);
    const el = renderer.domElement; el.tabIndex = 0; el.setAttribute("role", "img"); el.setAttribute("aria-label", label);
    surface.appendChild(el);
    const scene = new T.Scene(), camera = new T.OrthographicCamera(-1, 1, 1, -1, 0.05, 12);
    const orbit = new OrbitControls(camera, el);
    orbit.enablePan = false; orbit.minZoom = 0.6; orbit.maxZoom = 5; orbit.rotateSpeed = 0.72;
    orbit.minPolarAngle = 0.9; orbit.maxPolarAngle = 1.85;
    const studio = lightStudio(renderer, scene, "warm");
    const form = formMaterial(), body = new T.BufferGeometry();
    form.onBeforeCompile = shader => {
      shader.vertexShader = shader.vertexShader.replace("#include <begin_vertex>", `#include <begin_vertex>\ntransformed -= objectNormal * ${FORM_INSET.toFixed(4)};`);
    };
    form.customProgramCacheKey = () => "solver-drape-form-inset";
    const disposables: { dispose(): void }[] = [];
    let span = 0.5, aspect = 1;
    // Where the form stands in the solver's frame: its floor height and the middle of its footprint.
    const stand = new T.Vector3(), place2 = new T.Vector2();
    const draw = () => renderer.render(scene, camera);
    const project = () => {
      const half = Math.max(span, 0.34 / aspect);
      Object.assign(camera, { left: -half * aspect, right: half * aspect, top: half, bottom: -half });
      camera.updateProjectionMatrix();
    };
    function orient(yaw: number, target: number, half: number, elevation = 0.07) {
      span = half; camera.zoom = 1; project(); orbit.target.set(stand.x, stand.y + target, stand.z);
      camera.position.set(4 * Math.sin(yaw) * Math.cos(elevation), 4 * Math.sin(elevation), 4 * Math.cos(yaw) * Math.cos(elevation)).add(orbit.target);
      orbit.update(); draw();
    }
    const resize = () => {
      const w = surface.clientWidth, h = surface.clientHeight; if (!w || !h) return;
      aspect = w / h; renderer.setSize(w, h, false); project(); draw();
    };
    const keydown = (e: KeyboardEvent) => {
      if (e.altKey || e.ctrlKey || e.metaKey) return;
      let yaw = orbit.getAzimuthalAngle(), lift = Math.PI / 2 - orbit.getPolarAngle();
      switch (e.key) {
        case "ArrowLeft": yaw -= 0.14; break; case "ArrowRight": yaw += 0.14; break;
        case "ArrowUp": lift += 0.1; break; case "ArrowDown": lift -= 0.1; break;
        case "+": case "=": camera.zoom = Math.min(orbit.maxZoom, camera.zoom * 1.15); camera.updateProjectionMatrix(); draw(); e.preventDefault(); return;
        case "-": camera.zoom = Math.max(orbit.minZoom, camera.zoom / 1.15); camera.updateProjectionMatrix(); draw(); e.preventDefault(); return;
        default: return;
      }
      e.preventDefault();
      const zoom = camera.zoom; orient(yaw, orbit.target.y - stand.y, span, T.MathUtils.clamp(lift, -0.28, 0.66));
      camera.zoom = zoom; camera.updateProjectionMatrix(); draw();
    };
    const lost = (e: Event) => { e.preventDefault(); onFailure(); };
    const observer = new ResizeObserver(resize); observer.observe(surface); resize();
    orbit.addEventListener("change", draw); el.addEventListener("keydown", keydown); el.addEventListener("webglcontextlost", lost);
    const pose: Pose = (yaw, targetCm, spanCm) => orient(yaw, targetCm / 100, spanCm / 100);
    if (captureHook && new URLSearchParams(location.search).has("capture")) window[captureHook] = pose;

    const get = async (path: string) => { const r = await fetch(path, { signal }); if (!r.ok || !r.body) throw Error("Missing packaged drape"); return r; };
    Promise.all([
      get(source + ".json").then(r => r.json() as Promise<Package>),
      get(source + ".gz").then(r => new Response(r.body!.pipeThrough(new DecompressionStream("gzip"))).arrayBuffer()),
    ]).then(([pack, data]) => {
      if (signal.aborted) return;
      const head = new DataView(data), bodyCount = head.getUint32(0, true), indexCount = head.getUint32(4, true);
      const n = head.getUint32(8, true), frameCount = head.getUint32(12, true);
      if (n !== pack.vertices || frameCount !== pack.frames.length) throw Error("Mismatched packaged drape");
      let at = 16;
      const bodyXYZ = new Float32Array(data, at, bodyCount * 3); at += bodyCount * 12;
      const bodyIndex = new Uint32Array(data, at, indexCount); at += indexCount * 4;
      const flat = new Float32Array(data, at, n * 2); at += n * 8;
      const frames = new Float32Array(data, at, frameCount * n * 3);
      body.setAttribute("position", new T.Float32BufferAttribute(bodyXYZ, 3)); body.setIndex(new T.BufferAttribute(bodyIndex, 1));
      // The solver's frame is z up with the wearer facing −y; the page's is y up, facing +z.
      body.rotateX(-Math.PI / 2); body.computeVertexNormals(); body.computeBoundingBox(); body.computeBoundingSphere();
      studio.frame(body.boundingSphere!.center, body.boundingSphere!.radius);
      const box = body.boundingBox!;
      stand.set((box.min.x + box.max.x) / 2, box.min.y, (box.min.z + box.max.z) / 2);
      const floor = shadowFloor(box.min.y - 0.0005, 0.26); floor.mesh.position.x = stand.x; floor.mesh.position.z = stand.z;
      scene.add(floor.mesh); disposables.push(floor);
      if (mountAt !== undefined) {
        // The solver draped the garment on the whole body. Below the line the pelvis and legs are left out
        // and a plate, a pole and a base are drawn, as on a museum jacket form.
        const hip = stand.y + mountAt, at3 = body.getAttribute("position"), kept: number[] = [];
        const below = (i: number) => at3.getY(i) < hip && Math.abs(at3.getX(i) - stand.x) < TRUNK_HALF_WIDTH;
        const section = new T.Box2();
        // The plate is as wide as the trunk anywhere it overlaps it.
        for (let i = 0; i < at3.count; i++) if (at3.getY(i) > hip - 0.02 && at3.getY(i) < hip + 0.06 && Math.abs(at3.getX(i) - stand.x) < TRUNK_HALF_WIDTH) section.expandByPoint(place2.set(at3.getX(i), at3.getZ(i)));
        // A triangle that reaches below the line goes entirely, so the ragged edge is above it and inside the plate.
        for (let t = 0; t < bodyIndex.length; t += 3) if (!(below(bodyIndex[t]) || below(bodyIndex[t + 1]) || below(bodyIndex[t + 2]))) kept.push(bodyIndex[t], bodyIndex[t + 1], bodyIndex[t + 2]);
        body.setIndex(kept);
        const wood = new T.MeshStandardMaterial({ color: "#4f4133", roughness: 0.58 }), iron = new T.MeshStandardMaterial({ color: "#2a2d2b", roughness: 0.46, metalness: 0.62 });
        const centre = section.getCenter(new T.Vector2()), reach = section.getSize(new T.Vector2()).multiplyScalar(0.5);
        const plate = new T.Mesh(new T.CylinderGeometry(1, 0.97, 0.06, 56), wood);
        plate.scale.set(reach.x + 0.009, 1, reach.y + 0.009); plate.position.set(centre.x, hip + 0.012, centre.y);
        const pole = new T.Mesh(new T.CylinderGeometry(0.014, 0.014, hip - stand.y - 0.018, 24), iron); pole.position.set(centre.x, (hip + stand.y) / 2 - 0.009, centre.y);
        const foot = new T.Mesh(new T.CylinderGeometry(0.17, 0.19, 0.02, 56), iron); foot.position.set(centre.x, stand.y + 0.01, centre.y);
        for (const part of [plate, pole, foot]) { part.castShadow = part.receiveShadow = true; scene.add(part); disposables.push(part.geometry); }
        disposables.push(wood, iron);
      }
      const figure = new T.Mesh(body, form); figure.castShadow = figure.receiveShadow = true; scene.add(figure);

      const points = new Float32Array(n * 3);
      const upright = (index: number) => {
        const from = index * n * 3;
        for (let i = 0; i < n * 3; i += 3) { points[i] = frames[from + i]; points[i + 1] = frames[from + i + 2]; points[i + 2] = -frames[from + i + 1]; }
        return points;
      };
      const topology = { vertices: n, triangles: pack.triangles, seams: Object.values(pack.seams), panels: pack.panels };
      const sewn = Float32Array.from(upright(frameCount - 1));
      const tri = pack.triangles, a = new T.Vector3(), b = new T.Vector3(), c = new T.Vector3(), normal = new T.Vector3();
      // A point carried on the cloth: its triangle and two weights, lifted off the face along the triangle's normal.
      const carried = (t: number, w0: number, w1: number, lift: number, out: T.Vector3) => {
        a.fromArray(points, tri[t * 3] * 3); b.fromArray(points, tri[t * 3 + 1] * 3); c.fromArray(points, tri[t * 3 + 2] * 3);
        normal.copy(b).sub(a).cross(out.copy(c).sub(a)).normalize();
        return out.copy(a).multiplyScalar(w0).addScaledVector(b, w1).addScaledVector(c, 1 - w0 - w1).addScaledVector(normal, lift);
      };
      const cordMaterial = new T.MeshStandardMaterial({ roughness: 0.62, metalness: 0.12 });
      const buttonMaterial = new T.MeshStandardMaterial({ roughness: 0.34, metalness: 0.86 });
      const buttonShape = new T.SphereGeometry(0.0042, 14, 10);
      const trim = new T.Group(); scene.add(trim);
      const cords = pack.cords.map(() => { const mesh = new T.Mesh(new T.BufferGeometry(), cordMaterial); mesh.castShadow = true; mesh.frustumCulled = false; trim.add(mesh); return mesh; });
      const buttons = new T.InstancedMesh(buttonShape, buttonMaterial, pack.buttons.length); buttons.castShadow = true; buttons.frustumCulled = false; trim.add(buttons);
      disposables.push(cordMaterial, buttonMaterial, buttonShape, { dispose() { for (const mesh of cords) mesh.geometry.dispose(); } });
      let garment: ReturnType<typeof dressCloth> | undefined, shownLook: string | undefined, shownFrame = -1, shownView: string | undefined;
      const place = new T.Matrix4(), spot = new T.Vector3(), up = new T.Vector3(0, 0, 1), turn = new T.Quaternion(), squash = new T.Vector3(1, 1, 0.55);
      function carryTrim(thickness: number) {
        pack.cords.forEach((cord, k) => {
          const radius = cord.width_mm / 2000, path: T.Vector3[] = [];
          for (let i = 0; i < cord.anchors.length; i += 3) path.push(carried(cord.anchors[i], cord.anchors[i + 1], cord.anchors[i + 2], thickness / 2 + radius * 0.8, new T.Vector3()));
          cords[k].geometry.dispose();
          cords[k].geometry = new T.TubeGeometry(new T.CatmullRomCurve3(path, false, "centripetal"), path.length * 2, radius, 6, false);
        });
        pack.buttons.forEach((button, k) => {
          carried(button.anchor[0], button.anchor[1], button.anchor[2], thickness / 2 + 0.0016, spot);
          buttons.setMatrixAt(k, place.compose(spot, turn.setFromUnitVectors(up, normal), squash));
        });
        buttons.instanceMatrix.needsUpdate = true;
      }
      function change() {
        const next = latest.current, look = next.looks[next.look];
        if (next.look !== shownLook) {
          if (garment) { scene.remove(garment.mesh); garment.dispose(); }
          garment = dressCloth(topology, flat, look.fabric, { sewn, edge: look.edge ?? "hem" });
          scene.add(garment.mesh);
          cordMaterial.color.set(look.cord ?? "#cfa648"); buttonMaterial.color.set(look.button ?? "#a98a43");
          shownLook = next.look; shownFrame = -1;
        }
        const index = Math.max(0, Math.min(frameCount - 1, Math.round(next.frame)));
        if (index !== shownFrame) {
          garment!.update(upright(index)); carryTrim(look.fabric.thickness);
          studio.key.shadow.needsUpdate = true; shownFrame = index; el.dataset.frame = String(pack.frames[index]);
        }
        if (next.view.preset !== shownView) {
          const view = next.views[next.view.preset]; orient(view.yaw, view.target, view.span); shownView = next.view.preset;
        }
        draw(); el.dataset.ready = next.look;
      }
      disposables.push({ dispose() { garment?.dispose(); } });
      apply.current = change; change();
      onReady({ frames: frameCount, phases: pack.phases, sourceFrames: pack.frames });
    }).catch(() => { if (!signal.aborted) onFailure(); });
    return () => {
      abort.abort(); apply.current = null; if (captureHook && window[captureHook] === pose) delete window[captureHook];
      observer.disconnect(); orbit.removeEventListener("change", draw); orbit.dispose();
      el.removeEventListener("keydown", keydown); el.removeEventListener("webglcontextlost", lost);
      for (const item of disposables) item.dispose();
      body.dispose(); form.dispose(); studio.dispose(); renderer.dispose(); renderer.forceContextLoss(); el.remove();
    };
  }, [source, mountAt, label, captureHook, onReady, onFailure]);
  return <div className={className} ref={host} />;
}
