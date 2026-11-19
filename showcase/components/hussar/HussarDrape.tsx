"use client";

import { useEffect, useRef } from "react";
import * as T from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { dressCloth } from "../atelier3d/garment";
import type { Fabric } from "../atelier3d/weave";
import { formMaterial, lightStudio, prepareRenderer, shadowFloor } from "../atelier3d/studio";

export type Regiment = "Blue" | "Yellow";
export type Framing = "Jacket" | "Chest" | "Reverse" | "Figure";
export interface DrapeInfo { frames: number; phases: { fold: number; zip: number; sew: number; drape: number }; sourceFrames: number[] }
type Settings = { regiment: Regiment; framing: { preset: Framing }; frame: number };
type Props = Settings & { onReady: (info: DrapeInfo) => void; onFailure: () => void };

declare global { interface Window { ftxHussarFrame?: (yaw: number, targetCm: number, spanCm: number) => void } }

interface Cord { instance: string; width_mm: number; anchors: number[] }
interface Package {
  vertices: number; triangles: number[]; panels: Record<string, [number, number]>;
  seams: Record<string, { side_a: number[]; side_b: number[] }>;
  frames: number[]; phases: DrapeInfo["phases"]; cords: Cord[]; buttons: { anchor: [number, number, number] }[];
}

// Fulled wool broadcloth is a plain weave with the yarns closed up, lined in unbleached linen.
// Colours follow the 1761 plate; the fabric constants of the drape are in the method note.
const cloth: Record<Regiment, Fabric> = {
  Blue: { colour: "#111a31", lining: "#b7aa90", sheen: "#46587f", weave: "plain", yarnPitchMm: 0.9, thickness: 0.0022 },
  Yellow: { colour: "#161717", lining: "#b7aa90", sheen: "#55595d", weave: "plain", yarnPitchMm: 0.9, thickness: 0.0022 },
};
const lace: Record<Regiment, { cord: string; button: string }> = {
  Blue: { cord: "#cfa648", button: "#a98a43" },
  Yellow: { cord: "#dedacb", button: "#b9b8b2" },
};
// Camera presets in metres above the floor: where the camera looks and half the height it shows.
const views: Record<Framing, { target: number; span: number; yaw: number }> = {
  Jacket: { target: 1.3, span: 0.5, yaw: -0.3 },
  Chest: { target: 1.33, span: 0.25, yaw: -0.22 },
  Reverse: { target: 1.3, span: 0.5, yaw: Math.PI - 0.3 },
  Figure: { target: 0.9, span: 0.96, yaw: -0.3 },
};
// Where the display mount ends the form (metres above the floor), and how far from the axis counts as trunk
// and legs, not arms and hands.
const HIP_LINE = 0.94, TRUNK_HALF_WIDTH = 0.27, FORM_INSET = 0.0025;

export default function HussarDrape({ regiment, framing, frame, onReady, onFailure }: Props) {
  const host = useRef<HTMLDivElement>(null), apply = useRef<((s: Settings) => void) | null>(null);
  const latest = useRef<Settings>({ regiment, framing, frame });
  useEffect(() => { latest.current = { regiment, framing, frame }; apply.current?.(latest.current); }, [regiment, framing, frame]);
  useEffect(() => {
    const surface = host.current!, abort = new AbortController(), { signal } = abort;
    let renderer: T.WebGLRenderer;
    try { renderer = new T.WebGLRenderer({ alpha: true, antialias: true }); } catch { onFailure(); return; }
    renderer.setClearColor(0, 0); prepareRenderer(renderer);
    const el = renderer.domElement; el.tabIndex = 0; el.setAttribute("role", "img");
    el.setAttribute("aria-label", "The 1761 dolman draped by the cloth solver on a fitted adult form. Drag or use arrow keys to turn, and plus or minus to zoom.");
    surface.appendChild(el);
    const scene = new T.Scene(), camera = new T.OrthographicCamera(-1, 1, 1, -1, 0.05, 12);
    const orbit = new OrbitControls(camera, el);
    orbit.enablePan = false; orbit.minZoom = 0.6; orbit.maxZoom = 5; orbit.rotateSpeed = 0.72;
    orbit.minPolarAngle = 0.9; orbit.maxPolarAngle = 1.85;
    const studio = lightStudio(renderer, scene, "warm");
    const form = formMaterial(), body = new T.BufferGeometry();
    // Display only: the form is drawn this far inside the surface the solver used, so that flat cloth facets
    // spanning a curved arm do not let it show through. The cloth itself is not moved.
    form.onBeforeCompile = shader => {
      shader.vertexShader = shader.vertexShader.replace("#include <begin_vertex>", `#include <begin_vertex>\ntransformed -= objectNormal * ${FORM_INSET.toFixed(4)};`);
    };
    form.customProgramCacheKey = () => "hussar-form-inset";
    const disposables: { dispose(): void }[] = [];
    let span = views.Jacket.span, aspect = 1;
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
    if (new URLSearchParams(location.search).has("capture")) window.ftxHussarFrame = (yaw, targetCm, spanCm) => orient(yaw, targetCm / 100, spanCm / 100);

    const get = async (path: string) => { const r = await fetch(path, { signal }); if (!r.ok || !r.body) throw Error("Missing hussar drape"); return r; };
    Promise.all([
      get("/exhibits/hussar/dolman.json").then(r => r.json() as Promise<Package>),
      get("/exhibits/hussar/dolman.gz").then(r => new Response(r.body!.pipeThrough(new DecompressionStream("gzip"))).arrayBuffer()),
    ]).then(([pack, data]) => {
      if (signal.aborted) return;
      const head = new DataView(data), bodyCount = head.getUint32(0, true), indexCount = head.getUint32(4, true);
      const n = head.getUint32(8, true), frameCount = head.getUint32(12, true);
      if (n !== pack.vertices || frameCount !== pack.frames.length) throw Error("Mismatched hussar drape");
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
      // Display only: the form is mounted like a museum jacket form. The solver draped the jacket on the
      // whole body; below the hip line the pelvis and legs are left out and a plate, a pole and a base are drawn.
      const hip = stand.y + HIP_LINE, at3 = body.getAttribute("position"), kept: number[] = [];
      const below = (i: number) => at3.getY(i) < hip && Math.abs(at3.getX(i) - stand.x) < TRUNK_HALF_WIDTH;
      const section = new T.Box2();
      // The plate is as wide as the trunk anywhere it overlaps it.
      for (let i = 0; i < at3.count; i++) if (at3.getY(i) > hip - 0.02 && at3.getY(i) < hip + 0.06 && Math.abs(at3.getX(i) - stand.x) < TRUNK_HALF_WIDTH) section.expandByPoint(place2.set(at3.getX(i), at3.getZ(i)));
      // A triangle that reaches below the line goes entirely, so the ragged edge is above it and inside the plate.
      for (let t = 0; t < bodyIndex.length; t += 3) if (!(below(bodyIndex[t]) || below(bodyIndex[t + 1]) || below(bodyIndex[t + 2]))) kept.push(bodyIndex[t], bodyIndex[t + 1], bodyIndex[t + 2]);
      body.setIndex(kept);
      const figure = new T.Mesh(body, form); figure.castShadow = figure.receiveShadow = true; scene.add(figure);
      const wood = new T.MeshStandardMaterial({ color: "#4f4133", roughness: 0.58 }), iron = new T.MeshStandardMaterial({ color: "#2a2d2b", roughness: 0.46, metalness: 0.62 });
      const centre = section.getCenter(new T.Vector2()), reach = section.getSize(new T.Vector2()).multiplyScalar(0.5);
      const plate = new T.Mesh(new T.CylinderGeometry(1, 0.97, 0.06, 56), wood);
      plate.scale.set(reach.x + 0.009, 1, reach.y + 0.009); plate.position.set(centre.x, hip + 0.012, centre.y);
      const pole = new T.Mesh(new T.CylinderGeometry(0.014, 0.014, hip - stand.y - 0.018, 24), iron); pole.position.set(centre.x, (hip + stand.y) / 2 - 0.009, centre.y);
      const foot = new T.Mesh(new T.CylinderGeometry(0.17, 0.19, 0.02, 56), iron); foot.position.set(centre.x, stand.y + 0.01, centre.y);
      for (const part of [plate, pole, foot]) { part.castShadow = part.receiveShadow = true; scene.add(part); disposables.push(part.geometry); }
      disposables.push(wood, iron);

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
      let garment: ReturnType<typeof dressCloth> | undefined, shown: Regiment | undefined, shownFrame = -1, shownView: Framing | undefined;
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
      function change(next: Settings) {
        if (next.regiment !== shown) {
          if (garment) { scene.remove(garment.mesh); garment.dispose(); }
          garment = dressCloth(topology, flat, cloth[next.regiment], { sewn, edge: "hem" });
          scene.add(garment.mesh);
          cordMaterial.color.set(lace[next.regiment].cord); buttonMaterial.color.set(lace[next.regiment].button);
          shown = next.regiment; shownFrame = -1;
        }
        const index = Math.max(0, Math.min(frameCount - 1, Math.round(next.frame)));
        if (index !== shownFrame) {
          garment!.update(upright(index)); carryTrim(cloth[next.regiment].thickness);
          studio.key.shadow.needsUpdate = true; shownFrame = index; el.dataset.frame = String(pack.frames[index]);
        }
        if (next.framing.preset !== shownView) {
          const view = views[next.framing.preset]; orient(view.yaw, view.target, view.span); shownView = next.framing.preset;
        }
        draw(); el.dataset.ready = next.regiment;
      }
      disposables.push({ dispose() { garment?.dispose(); } });
      apply.current = change; change(latest.current);
      onReady({ frames: frameCount, phases: pack.phases, sourceFrames: pack.frames });
    }).catch(() => { if (!signal.aborted) onFailure(); });
    return () => {
      abort.abort(); apply.current = null; if (window.ftxHussarFrame) delete window.ftxHussarFrame;
      observer.disconnect(); orbit.removeEventListener("change", draw); orbit.dispose();
      el.removeEventListener("keydown", keydown); el.removeEventListener("webglcontextlost", lost);
      for (const item of disposables) item.dispose();
      body.dispose(); form.dispose(); studio.dispose(); renderer.dispose(); renderer.forceContextLoss(); el.remove();
    };
  }, [onReady, onFailure]);
  return <div className="hu-scene" ref={host} />;
}
