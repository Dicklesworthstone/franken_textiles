"use client";

import { type ReactNode, useEffect, useRef, useState } from "react";
import * as T from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { PlaybackMark } from "../InlineFilm";
import {cuttingTable,sampleIndex} from "../construction/replay";
import { dressCloth } from "./garment";
import type { Fabric } from "./weave";
import { formMaterial, lightStudio, prepareRenderer, shadowFloor } from "./studio";
import "./solver-drape.css";

// From flat pattern to sewn article: a replay of one packaged `ftx drape` run (scripts/prepare-hussar-drape.py).
// Panels bend independently on the cutting table before the cut to the recorded sewing states.
// The flat pattern stays in view underneath, in the same colours.
//
// What this file adds is display only, and each exhibit's method note says so: cloth thickness and weave,
// smoothing between solver points, the piece colours and threads, cord drawn along the pattern's decoration
// lines, an optional jacket mount, and the form drawn slightly inside its solved surface.

export interface DrapeInfo { frames: number; phases: { fold: number; zip: number; sew: number; drape: number }; sourceFrames: number[] }
/** Where the camera looks (metres above the floor), half the height it shows, and its angle round the form. */
export interface DrapeView { target: number; span: number; yaw: number; fitWidth?: boolean }
export interface DrapeLook { fabric: Fabric; edge?: "braid" | "hem"; cord?: string; button?: string }
type Pose = (yaw: number, targetCm: number, spanCm: number) => void;
declare global { interface Window { ftxHussarFrame?: Pose; ftxDressFrameDrape?: Pose; ftxMakingFrame?: Pose } }

export interface SolverDrapeProps {
  /** The packaged drape without its extension: `<source>.json` and `<source>.gz`. */
  source: string;
  looks: Record<string, DrapeLook>; look: string;
  views: Record<string, DrapeView>; view: { preset: string };
  /** Show the form as a jacket mount, ending it this far above the floor (metres) with a plate and a pole. */
  mountAt?: number;
  /** What the canvas is, for a screen reader. */
  label: string;
  /** A readable name for a packaged panel, such as "Front, right" for `dolman_front.R`. */
  pieceName?: (panel: string) => string;
  /** With `?capture=1` in the address, the window function that poses the camera for a film script. */
  captureHook?: "ftxHussarFrame" | "ftxDressFrameDrape" | "ftxMakingFrame";
  className?: string;
  /** Captions laid over the stage. */
  children?: ReactNode;
  onFailure?: () => void;
}

interface Package {
  vertices: number; triangles: number[]; panels: Record<string, [number, number]>;
  seams: Record<string, { side_a: number[]; side_b: number[] }>;
  frames: number[]; phases: DrapeInfo["phases"];
  cords: { width_mm: number; anchors: number[] }[]; buttons: { anchor: [number, number, number] }[];
}
interface Outline { panel: string; path: string; colour: string; at: [number, number] }
interface Sheet { box: [number, number, number, number]; pieces: Outline[] }

// How far from the axis counts as trunk and legs, not arms and hands, when the form is cut for a mount; and how
// far inside the solver's surface the form is drawn, so flat cloth facets over a curved arm do not show it through.
const TRUNK_HALF_WIDTH = 0.27, FORM_INSET = 0.0025;
// One colour per cut piece while it is being assembled; a mirrored pair shares its colour.
const PIECE_COLOURS = ["#c9785c", "#d9b25f", "#7fa6a0", "#8c8fb8", "#b98fb0", "#9cb37a", "#6f9bc0", "#c2a878", "#d08f6b", "#a8a59c"];
const STAGES = ["Cut flat", "Shape the panels", "Seams sewn", "Hung under gravity"] as const;
const stem = (panel: string) => panel.replace(/\.[RLC]$/, "");
/** A visitor who prefers no motion, and a film capture, start on the finished garment and not on the flat cut. */
const startsFinished = () => matchMedia("(prefers-reduced-motion: reduce)").matches || new URLSearchParams(location.search).has("capture");

/** Which of the four stages a kept frame belongs to. */
function stageOf(info: DrapeInfo, index: number) {
  const f = info.sourceFrames[index], { fold, zip, sew } = info.phases;
  return f === 0 ? 0 : f < fold + zip - 1 ? 1 : f < fold + zip + sew ? 2 : 3;
}
/** The first kept frame of a stage; the last stage is entered at its end, the settled garment. */
function stageStart(info: DrapeInfo, stage: number) {
  if (stage >= 3) return info.frames - 1;
  if (stage === 1) return sampleIndex(info.sourceFrames,info.phases,info.phases.fold-1);
  for (let i = 0; i < info.frames; i++) if (stageOf(info, i) >= stage) return i;
  return info.frames - 1;
}
/** The outline of each cut piece on the flat pattern, as SVG paths in millimetres (y down). */
function patternSheet(pack: Package, flat: Float32Array, colours: Map<string, string>): Sheet {
  const box: [number, number, number, number] = [Infinity, Infinity, -Infinity, -Infinity], pieces: Outline[] = [];
  for (const [panel, [lo, hi]] of Object.entries(pack.panels)) {
    // An edge used by one triangle lies on the piece's outline; follow them head to tail.
    const next = new Map<number, number>(), twin = new Set<string>();
    for (let t = 0; t < pack.triangles.length; t += 3) {
      if (pack.triangles[t] < lo || pack.triangles[t] >= hi) continue;
      for (let k = 0; k < 3; k++) twin.add(pack.triangles[t + k] + ">" + pack.triangles[t + (k + 1) % 3]);
    }
    for (const edge of twin) { const [a, b] = edge.split(">").map(Number); if (!twin.has(b + ">" + a)) next.set(a, b); }
    let path = "", sx = 0, sy = 0, count = 0;
    while (next.size) {
      const first = next.keys().next().value as number;
      let v = first;
      do {
        const x = flat[v * 2] * 1000, y = -flat[v * 2 + 1] * 1000;
        path += (v === first ? "M" : "L") + x.toFixed(1) + " " + y.toFixed(1);
        box[0] = Math.min(box[0], x); box[1] = Math.min(box[1], y); box[2] = Math.max(box[2], x); box[3] = Math.max(box[3], y);
        sx += x; sy += y; count++;
        const to = next.get(v); next.delete(v); v = to ?? first;
      } while (v !== first && next.has(v));
      path += "Z";
    }
    if (count) pieces.push({ panel, path, colour: colours.get(stem(panel))!, at: [sx / count, sy / count] });
  }
  return { box, pieces };
}

export default function SolverDrape(props: SolverDrapeProps) {
  const { source, mountAt, label, captureHook, className, children, pieceName } = props;
  const root = useRef<HTMLDivElement>(null), host = useRef<HTMLDivElement>(null), apply = useRef<(() => void) | null>(null);
  const [info, setInfo] = useState<DrapeInfo | null>(null), [sheet, setSheet] = useState<Sheet | null>(null);
  const [frame, setFrame] = useState(-1), [playing, play] = useState(false), [failed, setFailed] = useState(false), [tinted, keepTint] = useState(false);
  const latest = useRef({ ...props, frame, tinted }), began = useRef(0), offered = useRef(false);
  useEffect(() => { latest.current = { ...props, frame, tinted }; apply.current?.(); });
  const fail = useRef(() => { setFailed(true); play(false); latest.current.onFailure?.(); });

  // Play the making once, the first time most of the stage is on screen. A visitor who prefers no motion
  // starts on the finished garment and can play or scrub. (The opening frame itself is set when the drape loads.)
  useEffect(() => {
    if (!info || offered.current) return;
    if (startsFinished()) { offered.current = true; return; }
    const watch = new IntersectionObserver(([entry]) => {
      if (!entry.isIntersecting || offered.current) return;
      offered.current = true; began.current = performance.now(); play(true); watch.disconnect();
    }, { threshold: 0.55 });
    watch.observe(host.current!);
    return () => watch.disconnect();
  }, [info]);
  useEffect(() => {
    if (!playing || !info) return;
    // The flat cut is held long enough to be read, and each new stage a moment, before the frames run on.
    const due: number[] = []; let clock = 0;
    for (let i = 0; i < info.frames; i++) { due.push(clock); clock += i === 0 ? 1500 : stageOf(info, i) !== stageOf(info, i - 1) ? 620 : 150; }
    let raf = 0;
    const tick = (now: number) => {
      const elapsed = now - began.current; let next = info.frames - 1;
      for (let i = 0; i < info.frames; i++) if (due[i] > elapsed) { next = Math.max(0, i - 1); break; }
      setFrame(next);
      if (next < info.frames - 1) raf = requestAnimationFrame(tick); else play(false);
    };
    const hidden = () => { if (document.hidden) play(false); };
    document.addEventListener("visibilitychange", hidden);
    raf = requestAnimationFrame(tick);
    return () => { cancelAnimationFrame(raf); document.removeEventListener("visibilitychange", hidden); };
  }, [playing, info]);
  function replay() {
    if (!info) return;
    if (playing) { play(false); return; }
    offered.current = true; setFrame(0); began.current = performance.now(); play(true);
  }
  function jump(stage: number) { if (!info) return; offered.current = true; play(false); setFrame(stageStart(info, stage)); }

  useEffect(() => {
    const surface = host.current!, abort = new AbortController(), { signal } = abort;
    let renderer: T.WebGLRenderer;
    try { renderer = new T.WebGLRenderer({ alpha: true, antialias: true }); } catch { fail.current(); return; }
    renderer.setClearColor(0, 0); prepareRenderer(renderer);
    const el = renderer.domElement; el.tabIndex = 0; el.setAttribute("role", "img"); el.setAttribute("aria-label", label);
    surface.appendChild(el);
    const scene = new T.Scene(), camera = new T.OrthographicCamera(-1, 1, 1, -1, 0.05, 12);
    const orbit = new OrbitControls(camera, el);
    orbit.enablePan = false; orbit.minZoom = 0.6; orbit.maxZoom = 5; orbit.rotateSpeed = 0.72;
    orbit.minPolarAngle = 0.9; orbit.maxPolarAngle = 1.85;
    const studio = lightStudio(renderer, scene, "warm");
    const form = formMaterial(undefined, { inset: FORM_INSET }), body = new T.BufferGeometry();
    const disposables: { dispose(): void }[] = [];
    // The camera shows `span` either side of its target vertically and at least `reach` either side horizontally.
    let span = 0.5, reach = 0.34, aspect = 1, steered = false;
    // Where the form stands in the solver's frame: its floor height and the middle of its footprint.
    const stand = new T.Vector3(), place2 = new T.Vector2();
    const draw = () => renderer.render(scene, camera);
    const project = () => {
      const half = Math.max(span, reach / aspect);
      Object.assign(camera, { left: -half * aspect, right: half * aspect, top: half, bottom: -half });
      camera.updateProjectionMatrix();
    };
    function orient(yaw: number, target: number, half: number, wide = 0.34, elevation = 0.07, x=stand.x, z=stand.z) {
      span = half; reach = wide; camera.zoom = 1; project(); orbit.target.set(x, stand.y + target, z);
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
        case "+": case "=": camera.zoom = Math.min(orbit.maxZoom, camera.zoom * 1.15); camera.updateProjectionMatrix(); draw(); e.preventDefault(); steered = true; return;
        case "-": camera.zoom = Math.max(orbit.minZoom, camera.zoom / 1.15); camera.updateProjectionMatrix(); draw(); e.preventDefault(); steered = true; return;
        default: return;
      }
      e.preventDefault(); steered = true;
      const zoom = camera.zoom; orient(yaw, orbit.target.y - stand.y, span, reach, T.MathUtils.clamp(lift, -0.28, 0.66), orbit.target.x, orbit.target.z);
      camera.zoom = zoom; camera.updateProjectionMatrix(); draw();
    };
    const lost = (e: Event) => { e.preventDefault(); fail.current(); };
    const took = () => { steered = true; };
    const observer = new ResizeObserver(resize); observer.observe(surface); resize();
    orbit.addEventListener("change", draw); orbit.addEventListener("start", took);
    el.addEventListener("keydown", keydown); el.addEventListener("webglcontextlost", lost);
    const pose: Pose = (yaw, targetCm, spanCm) => { steered = true; orient(yaw, targetCm / 100, spanCm / 100); };
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
      const drape: DrapeInfo = { frames: frameCount, phases: pack.phases, sourceFrames: pack.frames };
      body.setAttribute("position", new T.Float32BufferAttribute(bodyXYZ, 3)); body.setIndex(new T.BufferAttribute(bodyIndex, 1));
      // The solver's frame is z up with the wearer facing −y; the page's is y up, facing +z.
      body.rotateX(-Math.PI / 2); body.computeVertexNormals(); body.computeBoundingBox(); body.computeBoundingSphere();
      studio.frame(body.boundingSphere!.center, body.boundingSphere!.radius);
      const box = body.boundingBox!;
      stand.set((box.min.x + box.max.x) / 2, box.min.y, (box.min.z + box.max.z) / 2);
      const floor = shadowFloor(box.min.y - 0.0005, 0.26); floor.mesh.position.x = stand.x; floor.mesh.position.z = stand.z;
      const mount = new T.Group(); scene.add(mount); mount.add(floor.mesh); disposables.push(floor);
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
        const centre = section.getCenter(new T.Vector2()), size = section.getSize(new T.Vector2()).multiplyScalar(0.5);
        const plate = new T.Mesh(new T.CylinderGeometry(1, 0.97, 0.06, 56), wood);
        plate.scale.set(size.x + 0.009, 1, size.y + 0.009); plate.position.set(centre.x, hip + 0.012, centre.y);
        const pole = new T.Mesh(new T.CylinderGeometry(0.014, 0.014, hip - stand.y - 0.018, 24), iron); pole.position.set(centre.x, (hip + stand.y) / 2 - 0.009, centre.y);
        const foot = new T.Mesh(new T.CylinderGeometry(0.17, 0.19, 0.02, 56), iron); foot.position.set(centre.x, stand.y + 0.01, centre.y);
        for (const part of [plate, pole, foot]) { part.castShadow = part.receiveShadow = true; mount.add(part); disposables.push(part.geometry); }
        disposables.push(wood, iron);
      }
      const figure = new T.Mesh(body, form); figure.castShadow = figure.receiveShadow = true; mount.add(figure);

      const points = new Float32Array(n * 3);
      const upright = (index: number) => {
        const from = index * n * 3;
        for (let i = 0; i < n * 3; i += 3) { points[i] = frames[from + i]; points[i + 1] = frames[from + i + 2]; points[i + 2] = -frames[from + i + 1]; }
        return points;
      };
      const topology = { vertices: n, triangles: pack.triangles, seams: Object.values(pack.seams), panels: pack.panels };
      const sewn = Float32Array.from(upright(frameCount - 1));
      const dressedBounds = new T.Box3().setFromArray(sewn).union(box), extent = dressedBounds.getSize(new T.Vector3());
      const frameForm = (view:DrapeView) => {
        const halfWidth = view.fitWidth ? 0.53*(extent.x*Math.abs(Math.cos(view.yaw))+extent.z*Math.abs(Math.sin(view.yaw))) : undefined;
        orient(view.yaw,view.target,view.span,halfWidth);
      };

      // One colour per cut piece, on the cloth while it is assembled and on the flat pattern under the stage.
      const colours = new Map<string, string>(), tints = new Float32Array(n * 3), tone = new T.Color();
      for (const [panel, [lo, hi]] of Object.entries(pack.panels)) {
        if (!colours.has(stem(panel))) colours.set(stem(panel), PIECE_COLOURS[colours.size % PIECE_COLOURS.length]);
        tone.set(colours.get(stem(panel))!);
        for (let v = lo; v < hi; v++) tone.toArray(tints, v * 3);
      }
      setSheet(patternSheet(pack, flat, colours));

      // The flat cut fills the opening view: the pieces lie on a board in front of the form.
      const arrange = cuttingTable(upright(0), Object.values(pack.panels));
      const boardBounds = new T.Box3(), boardSize = new T.Vector3(), boardCentre = new T.Vector3();
      const frameBoard = () => {
        boardBounds.setFromArray(points); boardBounds.getCenter(boardCentre); boardBounds.getSize(boardSize);
        orient(0,boardCentre.y-stand.y,boardSize.y*0.54+.015,boardSize.x*0.54+.015,0,boardCentre.x,boardCentre.z);
      };

      // Threads: each seam's two edges, point to matching point, until the seam has closed.
      const ends: number[] = [];
      const apart = (i: number, j: number) => Math.hypot(sewn[i * 3] - sewn[j * 3], sewn[i * 3 + 1] - sewn[j * 3 + 1], sewn[i * 3 + 2] - sewn[j * 3 + 2]);
      for (const seam of topology.seams) {
        const length = Math.min(seam.side_a.length, seam.side_b.length);
        if (!length) continue;
        const flipped = apart(seam.side_a[0], seam.side_b[0]) > apart(seam.side_a[0], seam.side_b[length - 1]);
        const i = Math.floor((length-1)/2);
        ends.push(seam.side_a[i], seam.side_b[flipped ? length - 1 - i : i]);
      }
      const threadXYZ = new T.BufferAttribute(new Float32Array(ends.length * 3), 3).setUsage(T.DynamicDrawUsage);
      const threadShape = new T.BufferGeometry().setAttribute("position", threadXYZ);
      const threadMaterial = new T.LineBasicMaterial({ color: "#f6ecd2", transparent: true, opacity: 0.62 });
      const threads = new T.LineSegments(threadShape, threadMaterial); threads.frustumCulled = false; scene.add(threads);
      disposables.push(threadShape, threadMaterial);
      function carryThreads() {
        const xyz = threadXYZ.array as Float32Array;
        for (let e = 0; e < ends.length; e += 2) {
          const a = ends[e] * 3, b = ends[e + 1] * 3;
          // A thread shorter than 2 mm has done its work.
          const closed = Math.hypot(points[a] - points[b], points[a + 1] - points[b + 1], points[a + 2] - points[b + 2]) < 0.002;
          for (let k = 0; k < 3; k++) { xyz[e * 3 + k] = points[a + k]; xyz[e * 3 + 3 + k] = points[(closed ? a : b) + k]; }
        }
        threadXYZ.needsUpdate = true;
      }

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
      disposables.push(cordMaterial, buttonMaterial, buttonShape, buttons, { dispose() { for (const mesh of cords) mesh.geometry.dispose(); } });
      // `shownView` is the page's view object, not its name: choosing the same preset again is a new object, and
      // brings the camera back after the visitor has turned it.
      let garment: ReturnType<typeof dressCloth> | undefined, shownLook: string | undefined, shownFrame = -1, shownView: SolverDrapeProps["view"] | undefined;
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
      // Kept-frame indices: the first frame of sewing, the first under gravity, and the settled endpoint.
      const wrapped = stageStart(drape, 2), hung = frameCount - 1;
      let firstHung = frameCount - 1;
      for (let i = 0; i < frameCount; i++) if (stageOf(drape, i) === 3) { firstHung = i; break; }
      function change() {
        const next = latest.current, look = next.looks[next.look];
        if (next.look !== shownLook) {
          if (garment) { scene.remove(garment.mesh); garment.dispose(); }
          garment = dressCloth(topology, flat, look.fabric, { sewn, edge: look.edge ?? "hem" });
          garment.tints(tints); scene.add(garment.mesh);
          cordMaterial.color.set(look.cord ?? "#cfa648"); buttonMaterial.color.set(look.button ?? "#a98a43");
          shownLook = next.look; shownFrame = -1;
        }
        const index = Math.max(0, Math.min(frameCount - 1, Math.round(next.frame < 0 ? frameCount - 1 : next.frame))), stage = stageOf(drape, index);
        const sourceIndex=sampleIndex(pack.frames,pack.phases,pack.frames[index]);
        const onBoard=pack.frames[sourceIndex]<pack.phases.fold;
        if (index !== shownFrame) {
          // Wrinkles are read off the settled cloth only: while pieces are still travelling they mean nothing.
          garment!.shell.detail = stage === 3 ? 1 : 0;
          upright(sourceIndex);
          if(onBoard)arrange(points);
          mount.visible=!onBoard;
          garment!.update(points); carryTrim(look.fabric.thickness);
          threads.visible = stage === 1; if (threads.visible) carryThreads();
          studio.key.shadow.needsUpdate = true; shownFrame = index; el.dataset.frame = String(pack.frames[sourceIndex]);
          // Follow the making unless the visitor has taken the camera: wide on the flat cut, then in on the form.
          if (!steered) {
            const view = next.views[next.view.preset];
            if(onBoard) frameBoard(); else frameForm(view);
            shownView = next.view;
          }
        }
        // Pieces keep their own colours until the seams are sewn, then take the cloth's as the garment hangs.
        const fade = stage < 2 ? 1 : stage === 2 ? 1 - 0.5 * (index - wrapped) / Math.max(1, firstHung - wrapped) : Math.max(0, 0.5 - 0.5 * (index - firstHung) / Math.max(1, Math.min(4, hung - firstHung)));
        garment!.tintAmount.value = next.tinted ? 1 : fade;
        if (next.view !== shownView) {
          steered = false;
          if(onBoard) frameBoard(); else frameForm(next.views[next.view.preset]);
          shownView = next.view;
        }
        draw(); el.dataset.ready = next.look;
      }
      disposables.push({ dispose() { garment?.dispose(); } });
      // Draw the opening frame straight away, so the finished garment does not flash before the flat cut.
      const opening = startsFinished() ? frameCount - 1 : 0;
      latest.current = { ...latest.current, frame: opening };
      apply.current = change; change(); setFrame(opening); setInfo(drape);
    }).catch(() => { if (!signal.aborted) fail.current(); });
    return () => {
      abort.abort(); apply.current = null; if (captureHook && window[captureHook] === pose) delete window[captureHook];
      observer.disconnect(); orbit.removeEventListener("change", draw); orbit.removeEventListener("start", took); orbit.dispose();
      el.removeEventListener("keydown", keydown); el.removeEventListener("webglcontextlost", lost);
      for (const item of disposables) item.dispose();
      body.dispose(); form.dispose(); studio.dispose(); renderer.dispose(); renderer.forceContextLoss(); el.remove();
    };
  }, [source, mountAt, label, captureHook]);

  const shown = info ? Math.max(0, frame < 0 ? info.frames - 1 : frame) : 0, stage = info ? stageOf(info, shown) : 3;
  const name = pieceName ?? ((panel: string) => panel.replace(/_/g, " "));
  return <div className={"mk" + (className ? " " + className : "")} ref={root}>
    <div className="mk-stage">
      {failed ? <p className="mk-failed">This view needs WebGL. The flat pattern and the solver summary are still below.</p> : <div className="mk-canvas" ref={host} />}
      {!failed && <ol className="mk-steps" aria-label="From flat pattern to sewn article">{STAGES.map((title, k) =>
        <li key={title} className={k === stage ? "is-now" : k < stage ? "is-done" : ""}>
          <button type="button" disabled={!info} aria-current={k === stage ? "step" : undefined} onClick={() => jump(k)}><b>{k + 1}</b>{title}</button></li>)}</ol>}
      {children}
    </div>
    {!failed && <div className="mk-bar">
      <button type="button" className="mk-play" onClick={replay} disabled={!info} aria-label={playing ? "Pause the making" : "Play the making from the flat pattern"}><PlaybackMark playing={playing} /></button>
      <input type="range" min="0" max={info ? info.frames - 1 : 1} value={shown} disabled={!info} aria-label="Sewing replay frame" aria-valuetext={info ? STAGES[stage] : "Loading"}
        onChange={event => { offered.current = true; play(false); setFrame(Number(event.target.value)); }} />
      <span>{info ? (shown === info.frames - 1 ? "Final frame" : STAGES[stage]) : "Loading the drape…"}</span>
    </div>}
    {sheet && <figure className="mk-pattern">
      <svg viewBox={`${sheet.box[0] - 30} ${sheet.box[1] - 30} ${sheet.box[2] - sheet.box[0] + 60} ${sheet.box[3] - sheet.box[1] + 130}`} role="img"
        aria-label={`The flat pattern: ${sheet.pieces.length} cut pieces`}>
        {sheet.pieces.map((piece, k) => <g key={piece.panel}><path d={piece.path} fill={piece.colour} fillOpacity=".86" stroke="#2c2a25" strokeWidth="3.2" strokeLinejoin="round" />
          {/* Names sit on two lines in turn, so neighbouring narrow pieces do not print over each other. */}
          <text x={piece.at[0]} y={sheet.box[3] + (k % 2 ? 96 : 52)} textAnchor="middle" fontSize="34">{name(piece.panel)}</text></g>)}
      </svg>
      <figcaption><span><b>The flat pattern</b> · {sheet.pieces.length} cut pieces, each in the colour it carries on the form until it is sewn</span>
        <label><input type="checkbox" checked={tinted} onChange={event => keepTint(event.target.checked)} /> Keep the pieces coloured</label></figcaption>
    </figure>}
  </div>;
}
