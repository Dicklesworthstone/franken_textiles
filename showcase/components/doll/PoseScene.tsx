"use client";

import { useEffect, useRef } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { formMaterial, lightStudio, prepareRenderer, shadowFloor } from "../atelier3d/studio";
import { fullControls } from "../atelier3d/viewControls";
import { dressCloth } from "../atelier3d/garment";
import { fabrics } from "../atelier3d/weave";

// The posing room. The visitor poses a baked synthetic form by dragging its arms; the page moves the form
// itself, from the same baked arm samples the solver uses. When the pose is set, a worker
// (public/labs/pose-worker.mjs) sews and drapes the bolero onto the form in that pose with the production
// cloth solver compiled to WebAssembly, and the page replays what the solver did, from the flat pieces to
// the hung jacket. Nothing shown on the cloth is animated by hand.

export type PoseStatus = {
  phase: "loading" | "sewing" | "moving" | "replay" | "ready" | "failed";
  lines: string[];
  message?: string;
  /** The angles the form's arms were sampled at; a pose is sewn with each arm at one of them. */
  angles: number[];
  /** The form's pose now, left then right (degrees from the arm hanging down). */
  shown: [number, number];
  /** The pose the jacket on show was sewn in. */
  sewn?: [number, number];
  /** How long the last sew took in this tab (s). */
  sewSeconds?: number;
  /** How long the last move under the sewn jacket took in this tab (s), and its accepted substeps. */
  moveSeconds?: number;
  moveSubsteps?: number;
  /** When the running sew began (performance.now()). */
  sewingSince?: number;
  clothVertices?: number;
  /** The solver's own warnings about the jacket on show (its gating flags), word for word. */
  flags?: string[];
};

export type PoseCommand = { kind: "arms"; left: number; right: number };

type Sewn = {
  left: number; right: number; seconds: number;
  summary: {
    cloth_vertices: number; frames: { fold: number; zip: number; sew: number; drape: number };
    pieces: { id: string; first: number; end: number }[];
    seams: { side_a: number[]; side_b: number[] }[];
    report?: { gating?: string[] };
  };
  cloth: Float32Array; triangles: Uint32Array; flat: Float32Array; frames: Float32Array;
};

type Arm = { indices: Uint32Array; offsets: Float32Array[] };
type Form = {
  base: Float32Array; triangles: Uint32Array; angles: number[]; arms: [Arm, Arm];
  shoulders: [THREE.Vector3, THREE.Vector3]; floorMm: number; cutMm: number | null; baseDeg: number;
  /** Side-neck point above the floor (mm): the form ends a little above it, under a cap, like the films' forms. */
  neckMm: number;
};

const BODY = "/exhibits/pose/child-520";
const PATTERN = "/exhibits/poses/bolero-start.json";
const FIT = "/exhibits/poses/fit-spec.json";
// The drape room's bolero, at the browser's step budget.
// Settling stops as soon as the cloth is quiet; raised arms need a few more steps than hanging ones.
// A move under the sewn jacket takes substeps of up to 50 ms, each starting with the cloth within 20 mm
// of the body carried along with it (ftx_drape::motion::advance_body_carried). Every substep but a move's
// last stops at a Newton tolerance of 5 mm/s instead of the drape's 1 mm/s (MotionOptions::intermediate_tol);
// the last converges fully. Measured 2026-11-25 in Node on one thread over two sequences of page moves, it
// took 24-27 % less time. Against a run whose intermediate substeps were 10x tighter (1e-4 m/s), a move's
// final cloth was 0.07-0.98 mm away on average (0.05-1.23 mm without the setting): within the default's own
// spread, though on a move where no fold flips it was up to 0.2 mm further on average (1.1 mm at the 95th
// percentile). The bolero's folds can settle either way, 12-18 mm apart at a few vertices, with or without it.
const SETTINGS = { spacing_mm: 16, sew_steps: 12, drape_steps: 36, max_settle_steps: 48, time_subdivision: true, self_contact_mm: 0,
  motion_carry_mm: 20, motion_step_s: 0.05, motion_intermediate_tol_m_s: 5e-3 };
// A change of pose up to 10° (the arm is sampled every 10-15°) moves the arm under the sewn jacket with the
// solver's checked moving-body step; a larger one sews the jacket on again in the nearest sampled pose, the
// faster of the two there (measured in Node on one thread, 2026-11-25: a 5° move at 20°/s 4.4-5.3 s; a 10°
// move 14.7 s; a fresh sew about 16 s).
const MOVE_LIMIT_DEG = 10;
const ARM_SPEED_DEG_S = 20;
const same = (a?: [number, number] | null, b?: [number, number] | null) => !!a && !!b && a[0] === b[0] && a[1] === b[1];

/** Solver frame (z up, the wearer facing -y) to three.js (y up, the wearer facing +z). */
function toThree(source: ArrayLike<number>, target: Float32Array = new Float32Array(source.length)) {
  for (let i = 0; i < source.length; i += 3) {
    target[i] = source[i]; target[i + 1] = source[i + 2]; target[i + 2] = -source[i + 1];
  }
  return target;
}

/** The baked form (ftx-posable-body/1) as the page needs it: the displayed surface and each arm's samples. */
async function readForm(): Promise<Form> {
  const [header, packed] = await Promise.all([
    fetch(BODY + ".json").then(r => { if (!r.ok) throw Error("Could not load the form"); return r.json(); }),
    fetch(BODY + ".bin.gz").then(r => { if (!r.ok || !r.body) throw Error("Could not load the form"); return r.body; }),
  ]);
  const bytes = await new Response(packed.pipeThrough(new DecompressionStream("gzip"))).arrayBuffer();
  const array = (name: string) => {
    const entry = header.arrays[name];
    return entry.dtype === "u32" ? new Uint32Array(bytes, entry.offset, entry.count) : new Float32Array(bytes, entry.offset, entry.count);
  };
  const angles: number[] = header.arm_angles_deg;
  const arm = (side: string): Arm => {
    const indices = array(`${side}_display_indices`) as Uint32Array, all = array(`${side}_display_offsets`) as Float32Array;
    const stride = indices.length * 3;
    return { indices, offsets: angles.map((_, s) => all.subarray(s * stride, (s + 1) * stride)) };
  };
  const shoulder = (part: string) => { const o = header.frames[part].origin; return new THREE.Vector3(o[0] / 1000, o[2] / 1000, -o[1] / 1000); };
  return {
    base: array("display_vertices") as Float32Array, triangles: array("display_triangles") as Uint32Array, angles,
    arms: [arm("left"), arm("right")], shoulders: [shoulder("arm_L"), shoulder("arm_R")],
    floorMm: header.floor_mm, cutMm: header.trunk_cut_above_floor_mm ?? null, baseDeg: header.base_arm_deg,
    neckMm: header.landmarks.torso.snp,
  };
}

/** The form's displayed surface with the arms at these angles, as the session interpolates its samples. */
function posed(form: Form, left: number, right: number, out = new Float32Array(form.base.length)) {
  out.set(form.base);
  const { angles } = form;
  for (const [arm, angle] of [[form.arms[0], left], [form.arms[1], right]] as const) {
    const a = THREE.MathUtils.clamp(angle, angles[0], angles[angles.length - 1]);
    let upper = angles.findIndex(x => x >= a);
    upper = Math.max(1, upper < 0 ? angles.length - 1 : upper);
    const t = (a - angles[upper - 1]) / (angles[upper] - angles[upper - 1]);
    const from = arm.offsets[upper - 1], to = arm.offsets[upper];
    for (let k = 0; k < arm.indices.length; k++) {
      const v = arm.indices[k] * 3;
      for (let c = 0; c < 3; c++) out[v + c] += from[k * 3 + c] * (1 - t) + to[k * 3 + c] * t;
    }
  }
  return out;
}

const nearest = (angles: number[], a: number) => angles.reduce((best, x) => Math.abs(x - a) < Math.abs(best - a) ? x : best, angles[0]);

export default function PoseScene({ onStatus, command }: {
  onStatus: (status: PoseStatus) => void;
  command: React.MutableRefObject<((c: PoseCommand) => void) | null>;
}) {
  const host = useRef<HTMLDivElement>(null);
  const report = useRef(onStatus);
  useEffect(() => { report.current = onStatus; }, [onStatus]);

  useEffect(() => {
    const surface = host.current;
    if (!surface) return;
    const status: PoseStatus = { phase: "loading", lines: [], angles: [], shown: [35, 35] };
    const publish = () => report.current({ ...status, lines: [...status.lines], shown: [...status.shown] });
    let renderer: THREE.WebGLRenderer;
    try {
      renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true, powerPreference: "high-performance" });
    } catch {
      Object.assign(status, { phase: "failed", message: "This browser could not start WebGL, which the 3D view needs." });
      publish();
      return;
    }
    const canvas = renderer.domElement;
    canvas.style.cssText = "width:100%;height:100%;display:block";
    canvas.setAttribute("role", "img");
    canvas.setAttribute("aria-label", "A child's form wearing a bolero. Drag an arm up or down to pose it; when you let go, a small move carries the jacket along with the arm and a larger one sews it on again in that pose. Drag elsewhere to turn the view, right-drag to move it, scroll or pinch to zoom.");
    surface.appendChild(canvas);
    prepareRenderer(renderer);
    renderer.setClearColor(0, 0);
    const scene = new THREE.Scene();
    const studio = lightStudio(renderer, scene, "warm");
    const camera = new THREE.PerspectiveCamera(26, 1, 0.05, 20);
    const orbit = new OrbitControls(camera, canvas);
    orbit.enableDamping = true;
    orbit.dampingFactor = 0.12;
    orbit.minDistance = 0.45;
    orbit.maxDistance = 4;
    orbit.minPolarAngle = 0.25;
    orbit.maxPolarAngle = 1.75;
    let dirty = true, alive = true;
    const paint = () => { dirty = true; };
    const homeTarget = new THREE.Vector3(0, 0.75, 0);
    const home = () => {
      orbit.target.copy(homeTarget);
      camera.position.copy(homeTarget).add(new THREE.Vector3(0.55, 0.1, 1.75));
      orbit.update();
      paint();
    };
    home();
    const look = fullControls(orbit, canvas, { paint, home, turnKeys: true });
    // An arm is dragged with one finger, so the view keeps every touch on the canvas.
    canvas.style.touchAction = "none";
    const resize = () => {
      const w = surface.clientWidth, h = surface.clientHeight;
      if (!w || !h) return;
      renderer.setSize(w, h, false);
      camera.aspect = w / h;
      camera.updateProjectionMatrix();
      paint();
    };
    const observer = new ResizeObserver(resize);
    observer.observe(surface);
    resize();

    const disposables: { dispose(): void }[] = [];
    let form: Form | null = null;
    let bodyGeometry: THREE.BufferGeometry | null = null, bodyMesh: THREE.Mesh | null = null;
    let armOf = Uint8Array.from([]);
    let garment: ReturnType<typeof dressCloth> | null = null;
    let sewn: Sewn | null = null;
    let replay: { began: number } | null = null;
    let worker: Worker | null = null, sewing: [number, number] | null = null;
    // A move under the sewn jacket: asked of the worker (`moving`), then played back (`carry`). `clothNow` is
    // the jacket's present positions (solver frame), where a move starts from.
    let moving: { from: [number, number]; to: [number, number]; seconds: number } | null = null;
    let carry: { from: [number, number]; to: [number, number]; seconds: number; samples: Float32Array; start: Float32Array; began: number } | null = null;
    let clothNow: Float32Array | null = null;
    let raf = 0;

    function setBody(left: number, right: number) {
      if (!form || !bodyGeometry) return;
      const attribute = bodyGeometry.getAttribute("position") as THREE.BufferAttribute;
      toThree(posed(form, left, right), attribute.array as Float32Array);
      attribute.needsUpdate = true;
      bodyGeometry.computeVertexNormals();
      bodyGeometry.computeBoundingSphere();
      status.shown = [left, right];
      // The jacket on show belongs to one pose; in any other it would cut through the arms.
      // (During a move the jacket is the solver's own state at the form's shown pose, so it stays.)
      if (garment) garment.mesh.visible = same(status.shown, status.sewn ?? null) || !!replay || !!carry || !!moving;
      paint();
    }

    function sew(pose: [number, number]) {
      if (same(pose, sewing)) return;
      if (same(pose, status.sewn ?? null) && !sewing && !moving) {
        if (garment) garment.mesh.visible = true;
        status.phase = "ready";
        publish();
        return;
      }
      // A sew in progress is for a pose the visitor has left: drop it and start this one.
      moving = null;
      carry = null;
      worker?.terminate();
      worker = new Worker("/labs/pose-worker.mjs", { type: "module" });
      worker.onmessage = onWorker;
      worker.onerror = (e) => { Object.assign(status, { phase: "failed", message: e.message || "The solver worker stopped." }); publish(); };
      sewing = pose;
      replay = null;
      Object.assign(status, { phase: "sewing", sewingSince: performance.now(), lines: [], message: undefined });
      publish();
      worker.postMessage({ type: "sew", body: BODY, pattern: PATTERN, fit: FIT, settings: SETTINGS, left: pose[0], right: pose[1] });
    }

    // A nudge of at most MOVE_LIMIT_DEG from the sewn pose keeps the jacket on and moves the arm under it.
    const canMove = (pose: [number, number]) => {
      if (!worker || !sewn || !clothNow || sewing || moving || carry || !status.sewn) return false;
      const change = Math.max(Math.abs(pose[0] - status.sewn[0]), Math.abs(pose[1] - status.sewn[1]));
      return change >= 0.5 && change <= MOVE_LIMIT_DEG;
    };

    function moveTo(pose: [number, number]) {
      const from: [number, number] = [...status.sewn!];
      const degrees = Math.max(Math.abs(pose[0] - from[0]), Math.abs(pose[1] - from[1]));
      const seconds = Math.max(0.1, degrees / ARM_SPEED_DEG_S);
      moving = { from, to: pose, seconds };
      // The jacket stays on, in its pose, while the solver works; the playback then moves both together.
      setBody(from[0], from[1]);
      if (garment) garment.mesh.visible = true;
      Object.assign(status, { phase: "moving", sewingSince: performance.now(), message: undefined });
      publish();
      worker!.postMessage({ type: "move", left: pose[0], right: pose[1], seconds });
    }

    command.current = (c) => {
      if (!form) return;
      const pose: [number, number] = [nearest(form.angles, c.left), nearest(form.angles, c.right)];
      setBody(pose[0], pose[1]);
      sew(pose);
    };

    function onWorker(event: MessageEvent) {
      if (!alive) return;
      const reply = event.data;
      if (reply.type === "progress") {
        status.lines = [...status.lines.slice(-4), reply.line];
        publish();
      } else if (reply.type === "failed") {
        if (!same([reply.left, reply.right], sewing)) return;
        sewing = null;
        Object.assign(status, { phase: "ready", message: `The solver refused to sew the jacket on in this pose: ${reply.message}` });
        publish();
      } else if (reply.type === "sewn") {
        if (!same([reply.left, reply.right], sewing)) return;
        sewing = null;
        dress(reply as Sewn);
      } else if (reply.type === "moved") {
        if (!moving || !same([reply.left, reply.right], moving.to) || !clothNow) return;
        carry = { ...moving, samples: reply.samples as Float32Array, start: clothNow, began: -1 };
        moving = null;
        const n = clothNow.length;
        Object.assign(status, {
          phase: "replay", sewn: [reply.left, reply.right], moveSeconds: reply.seconds,
          moveSubsteps: carry.samples.length / (1 + n), sewingSince: undefined,
        });
        publish();
      } else if (reply.type === "move-failed") {
        if (!moving || !same([reply.left, reply.right], moving.to) || !form) return;
        const to = moving.to;
        moving = null;
        // The session is where it was; sew the jacket on in the nearest sampled pose instead.
        const pose: [number, number] = [nearest(form.angles, to[0]), nearest(form.angles, to[1])];
        const resewing = !same(pose, status.sewn ?? null);
        setBody(pose[0], pose[1]);
        sew(pose);
        status.message = `The solver could not carry the jacket to that pose (${reply.message}). ` +
          (resewing ? "It is sewing the jacket on again in the nearest sampled pose." : "The arm is back in the pose the jacket was sewn in.");
        publish();
      }
    }

    function dress(next: Sewn) {
      const n = next.summary.cloth_vertices;
      if (!garment || !sewn || sewn.summary.cloth_vertices !== n) {
        if (garment) { scene.remove(garment.mesh); garment.dispose(); }
        garment = dressCloth({
          vertices: n, triangles: next.triangles,
          seams: next.summary.seams.map(s => ({ side_a: s.side_a, side_b: s.side_b })),
          panels: Object.fromEntries(next.summary.pieces.map(p => [p.id, [p.first, p.end] as [number, number]])),
        }, Float32Array.from(next.flat, v => v / 1000), fabrics.greenWool, { sewn: toThree(next.cloth), edge: "braid" });
        garment.tintAmount.value = 0;
        scene.add(garment.mesh);
      }
      sewn = next;
      clothNow = next.cloth;
      garment.mesh.visible = true;
      Object.assign(status, {
        sewn: [next.left, next.right], sewSeconds: next.seconds, clothVertices: n, sewingSince: undefined,
        flags: next.summary.report?.gating ?? [],
      });
      if (!matchMedia("(prefers-reduced-motion: reduce)").matches) {
        replay = { began: -1 };
        status.phase = "replay";
      } else {
        garment.update(toThree(next.cloth));
        status.phase = "ready";
      }
      publish();
      paint();
    }

    readForm().then(f => {
      if (!alive) return;
      form = f;
      status.angles = f.angles;
      // The form: a plain tailor's form, cut below the waist, on a turned stand.
      bodyGeometry = new THREE.BufferGeometry();
      bodyGeometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array(f.base.length), 3));
      bodyGeometry.setIndex(new THREE.BufferAttribute(f.triangles, 1));
      const plaster = formMaterial(undefined, { inset: 0.0025 });
      bodyMesh = new THREE.Mesh(bodyGeometry, plaster);
      bodyMesh.castShadow = bodyMesh.receiveShadow = true;
      scene.add(bodyMesh);
      disposables.push(bodyGeometry, plaster);
      armOf = new Uint8Array(f.base.length / 3);
      f.arms.forEach((arm, side) => arm.indices.forEach(v => { armOf[v] = side + 1; }));
      setBody(f.baseDeg, f.baseDeg);
      // A tailor's form has no head: it ends a little above the side-neck point under a turned cap. The head's
      // triangles are left out (not clipped by a plane, which would also cut off a raised arm); the cap
      // covers the stepped edge they leave.
      const floorY = f.floorMm / 1000, neckY = floorY + (f.neckMm + 30) / 1000;
      {
        const at = bodyGeometry.getAttribute("position") as THREE.BufferAttribute;
        const head = (v: number) => !armOf[v] && at.getY(v) > neckY + 0.006;
        const kept: number[] = [];
        for (let t = 0; t < f.triangles.length; t += 3) {
          const [a, b, c] = [f.triangles[t], f.triangles[t + 1], f.triangles[t + 2]];
          if (!(head(a) || head(b) || head(c))) kept.push(a, b, c);
        }
        bodyGeometry.setIndex(kept);
      }
      const positions = bodyGeometry.getAttribute("position") as THREE.BufferAttribute;
      const box = new THREE.Box3().setFromBufferAttribute(positions);
      box.max.y = Math.min(box.max.y, neckY);
      const neck = new THREE.Box3();
      for (let v = 0; v < positions.count; v++) {
        if (Math.abs(positions.getY(v) - neckY) < 0.006 && !armOf[v]) neck.expandByPoint(new THREE.Vector3().fromBufferAttribute(positions, v));
      }
      const walnut = new THREE.MeshStandardMaterial({ color: "#4a3a31", roughness: 0.5 });
      if (!neck.isEmpty()) {
        const cap = new THREE.Mesh(new THREE.CylinderGeometry(1, 1.04, 0.017, 64), walnut);
        const size = neck.getSize(new THREE.Vector3()), mid = neck.getCenter(new THREE.Vector3());
        cap.scale.set(size.x / 2 * 1.05, 1, size.z / 2 * 1.05);
        cap.position.set(mid.x, neckY + 0.0045, mid.z);
        cap.castShadow = cap.receiveShadow = true;
        scene.add(cap);
        disposables.push(cap.geometry);
      }
      disposables.push(walnut);
      const cutY = f.cutMm === null ? box.min.y : floorY + f.cutMm / 1000;
      const centre = box.getCenter(new THREE.Vector3());
      const brass = new THREE.MeshStandardMaterial({ color: "#a08255", metalness: 0.5, roughness: 0.42 });
      const lacquer = new THREE.MeshStandardMaterial({ color: "#423b34", roughness: 0.52 });
      const pole = new THREE.Mesh(new THREE.CylinderGeometry(0.014, 0.016, cutY - floorY, 48), brass);
      pole.position.set(centre.x, (cutY + floorY) / 2, centre.z);
      const foot = new THREE.Mesh(new THREE.CylinderGeometry(0.1, 0.11, 0.014, 80), lacquer);
      foot.position.set(centre.x, floorY + 0.007, centre.z);
      for (const mesh of [pole, foot]) { mesh.castShadow = mesh.receiveShadow = true; scene.add(mesh); disposables.push(mesh.geometry); }
      disposables.push(brass, lacquer);
      const floor = shadowFloor(floorY, 0.22);
      scene.add(floor.mesh);
      disposables.push(floor);
      homeTarget.set(centre.x, (box.max.y + cutY) / 2 + 0.02, centre.z);
      look.retarget(homeTarget);
      home();
      look.setHome();
      studio.frame(new THREE.Vector3(centre.x, (box.max.y + floorY) / 2, centre.z), Math.max(0.5, (box.max.y - floorY) * 0.7));
      sew([f.baseDeg, f.baseDeg]);
    }).catch(error => {
      Object.assign(status, { phase: "failed", message: String(error?.message ?? error) });
      publish();
    });

    // Dragging an arm moves the form at once; letting go sews the jacket on in the nearest sampled pose.
    const pointer = new THREE.Vector2(), ray = new THREE.Raycaster(), plane = new THREE.Plane(), hit = new THREE.Vector3();
    let dragging = 0, dragFrom = { y: 0, angle: 0 }, hover = 0;
    const aim = (event: PointerEvent) => {
      const rect = canvas.getBoundingClientRect();
      if (!rect.width || !rect.height) return false;
      pointer.set(((event.clientX - rect.left) / rect.width) * 2 - 1, -((event.clientY - rect.top) / rect.height) * 2 + 1);
      ray.setFromCamera(pointer, camera);
      return true;
    };
    // An arm is picked by a direct hit, or else by the arm point nearest the pointer's ray within 3.5 cm:
    // a child's arm is thin, and a visitor aims at the sleeve over it as often as at the arm itself.
    const point = new THREE.Vector3();
    const armAt = (event: PointerEvent) => {
      if (!form || !bodyMesh || !bodyGeometry || !aim(event)) return 0;
      const found = ray.intersectObject(bodyMesh, false)[0];
      if (found && found.faceIndex != null) {
        const arm = armOf[bodyGeometry.getIndex()!.getX(found.faceIndex * 3)];
        if (arm) return arm;
      }
      const positions = bodyGeometry.getAttribute("position") as THREE.BufferAttribute;
      let best = 0, nearest = 0.035 * 0.035;
      for (const [side, arm] of form.arms.entries()) {
        for (let k = 0; k < arm.indices.length; k += 2) {
          point.fromBufferAttribute(positions, arm.indices[k]);
          const d = ray.ray.distanceSqToPoint(point);
          // Not an arm seen through the torso.
          if (d < nearest && (!found || ray.ray.origin.distanceTo(point) < found.distance + 0.03)) { nearest = d; best = side + 1; }
        }
      }
      return best;
    };
    const angleAt = (event: PointerEvent, side: number) => {
      if (!form || !aim(event)) return status.shown[side];
      const s = form.shoulders[side];
      // The arms swing in the body's frontal plane. Seen nearly edge-on, follow the pointer's height instead.
      if (Math.abs(ray.ray.direction.z) < 0.3) return dragFrom.angle + (dragFrom.y - event.clientY) * 0.45;
      plane.set(new THREE.Vector3(0, 0, 1), -s.z);
      if (!ray.ray.intersectPlane(plane, hit)) return status.shown[side];
      const outward = (hit.x - s.x) * (Math.sign(s.x) || 1), down = s.y - hit.y;
      return THREE.MathUtils.radToDeg(Math.atan2(outward, down));
    };
    const down = (event: PointerEvent) => {
      // While the solver carries the jacket (or it is being played back) the arms wait; the view still turns.
      if (event.button !== 0 || !event.isPrimary || !form || moving || carry) return;
      const arm = armAt(event);
      if (!arm) return;
      dragging = arm;
      dragFrom = { y: event.clientY, angle: status.shown[arm - 1] };
      orbit.enabled = false;
      canvas.setPointerCapture(event.pointerId);
      canvas.style.cursor = "grabbing";
      event.stopImmediatePropagation();
      event.preventDefault();
    };
    const move = (event: PointerEvent) => {
      if (!dragging) {
        if (event.pointerType === "mouse" && !event.buttons) {
          const arm = armAt(event);
          // Over an arm the cursor is a pointing hand ready to lift it; elsewhere the view's own grab cursor.
          if (arm !== hover) { hover = arm; canvas.style.cursor = arm ? "pointer" : "grab"; canvas.dataset.arm = String(arm); }
        }
        return;
      }
      if (!form) return;
      const side = dragging - 1, angles = form.angles;
      const angle = THREE.MathUtils.clamp(angleAt(event, side), angles[0], angles[angles.length - 1]);
      const pose: [number, number] = [...status.shown];
      pose[side] = angle;
      setBody(pose[0], pose[1]);
      publish();
      event.preventDefault();
    };
    const up = (event: PointerEvent) => {
      if (!dragging || !form) return;
      dragging = 0;
      orbit.enabled = true;
      if (canvas.hasPointerCapture(event.pointerId)) canvas.releasePointerCapture(event.pointerId);
      canvas.style.cursor = "pointer";
      // A small nudge moves the arm under the jacket to exactly where it was let go; anything larger sews
      // the jacket on again in the nearest sampled pose.
      const released: [number, number] = [status.shown[0], status.shown[1]];
      if (canMove(released)) {
        moveTo(released);
        return;
      }
      const pose: [number, number] = [nearest(form.angles, released[0]), nearest(form.angles, released[1])];
      setBody(pose[0], pose[1]);
      sew(pose);
    };
    canvas.addEventListener("pointerdown", down, true);
    canvas.addEventListener("pointermove", move);
    canvas.addEventListener("pointerup", up);
    canvas.addEventListener("pointercancel", up);
    const lost = (event: Event) => {
      event.preventDefault();
      Object.assign(status, { phase: "failed", message: "The 3D view lost its graphics context." });
      publish();
    };
    canvas.addEventListener("webglcontextlost", lost);

    // The replay: the solver's own frames, the flat pieces' journey to the form quickly, the sewing and the
    // hang at a pace the eye can follow.
    let blend = Float32Array.from([]);
    function frame(now: number) {
      raf = requestAnimationFrame(frame);
      if (orbit.update()) dirty = true;
      if (sewn && garment && replay) {
        if (replay.began < 0) replay.began = now;
        const n = sewn.summary.cloth_vertices * 3, count = sewn.frames.length / n;
        const { fold, zip, sew: sewing_ } = sewn.summary.frames;
        const seconds = (now - replay.began) / 1000;
        const keys: [number, number][] = [[0, 0], [1.6, fold + zip - 1], [3.0, fold + zip + sewing_ - 1], [4.6, count - 1]];
        let k = 1;
        while (k < keys.length - 1 && seconds > keys[k][0]) k++;
        const [t0, f0] = keys[k - 1], [t1, f1] = keys[k];
        const at = Math.min(count - 1, f0 + (f1 - f0) * Math.min(1, Math.max(0, (seconds - t0) / (t1 - t0))));
        const i = Math.min(count - 2, Math.floor(at)), f = at - i;
        if (blend.length !== n) blend = new Float32Array(n);
        for (let j = 0; j < n; j++) blend[j] = sewn.frames[i * n + j] * (1 - f) + sewn.frames[(i + 1) * n + j] * f;
        garment.update(toThree(blend, new Float32Array(n)));
        dirty = true;
        if (seconds >= keys[keys.length - 1][0]) {
          replay = null;
          garment.update(toThree(sewn.cloth));
          if (status.phase === "replay") { status.phase = "ready"; publish(); }
        }
      }
      // A move under the jacket: the solver's accepted substeps, shown four times slower than the cloth's
      // own time (at least a second), with the form between its two poses at the same moment.
      if (garment && carry) {
        if (carry.began < 0) carry.began = now;
        const n = carry.start.length, stride = 1 + n, count = carry.samples.length / stride;
        const t = Math.min(1, (now - carry.began) / 1000 / Math.max(1, carry.seconds * 4)) * carry.seconds;
        let k = 0;
        while (k < count - 1 && carry.samples[k * stride] < t) k++;
        const t1 = carry.samples[k * stride], t0 = k > 0 ? carry.samples[(k - 1) * stride] : 0;
        const f = t1 > t0 ? Math.min(1, Math.max(0, (t - t0) / (t1 - t0))) : 1;
        if (blend.length !== n) blend = new Float32Array(n);
        for (let j = 0; j < n; j++) {
          const before = k > 0 ? carry.samples[(k - 1) * stride + 1 + j] : carry.start[j];
          blend[j] = before * (1 - f) + carry.samples[k * stride + 1 + j] * f;
        }
        garment.update(toThree(blend, new Float32Array(n)));
        const u = t / carry.seconds;
        setBody(carry.from[0] + (carry.to[0] - carry.from[0]) * u, carry.from[1] + (carry.to[1] - carry.from[1]) * u);
        dirty = true;
        if (t >= carry.seconds) {
          clothNow = carry.samples.slice((count - 1) * stride + 1, count * stride);
          garment.update(toThree(clothNow));
          setBody(carry.to[0], carry.to[1]);
          carry = null;
          if (status.phase === "replay") { status.phase = "ready"; publish(); }
        }
      }
      if (dirty && alive) {
        studio.key.shadow.needsUpdate = true;
        renderer.render(scene, camera);
        dirty = false;
      }
    }
    raf = requestAnimationFrame(frame);

    return () => {
      alive = false;
      command.current = null;
      cancelAnimationFrame(raf);
      worker?.terminate();
      observer.disconnect();
      look.dispose();
      orbit.dispose();
      canvas.removeEventListener("pointerdown", down, true);
      canvas.removeEventListener("pointermove", move);
      canvas.removeEventListener("pointerup", up);
      canvas.removeEventListener("pointercancel", up);
      canvas.removeEventListener("webglcontextlost", lost);
      garment?.dispose();
      for (const item of disposables) item.dispose();
      studio.dispose();
      renderer.dispose();
      renderer.forceContextLoss();
      canvas.remove();
    };
  }, [command]);

  return <div ref={host} className="pose-canvas" />;
}
