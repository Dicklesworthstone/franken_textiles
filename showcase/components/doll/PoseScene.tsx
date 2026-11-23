"use client";

import { useEffect, useRef } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { formMaterial, lightStudio, prepareRenderer, shadowFloor } from "../atelier3d/studio";
import { fullControls } from "../atelier3d/viewControls";
import { dressCloth } from "../atelier3d/garment";
import { fabrics } from "../atelier3d/weave";

// The posing room: a bolero sewn and draped by the production cloth solver, running in this tab as
// WebAssembly in a worker (public/labs/pose-worker.mjs), on a baked synthetic form whose arms the
// visitor drags. Every cloth state shown after the opening is one the solver accepted while moving the
// body: nothing here is animated by hand.

export type PoseStatus = {
  phase: "loading" | "sewing" | "replay" | "ready" | "solving" | "failed";
  lines: string[];
  message?: string;
  openingSeconds?: number;
  clothVertices?: number;
  range?: [number, number];
  arms: [number, number];
  target: [number, number];
  last?: { degrees: number; clothSeconds: number; steps: number; milliseconds: number };
  refused?: string;
};

export type PoseCommand = { kind: "arms"; left: number; right: number } | { kind: "settle" };

type Ready = {
  summary: {
    cloth_vertices: number; base_arm_deg: number; arm_range_deg: [number, number];
    pieces: { id: string; first: number; end: number }[];
    seams: { side_a: number[]; side_b: number[] }[];
  };
  seconds: number; cloth: Float32Array; triangles: Uint32Array; flat: Float32Array; display: Float32Array;
  displayTriangles: Uint32Array; raised: Float32Array[]; frames: Float32Array;
  shoulders: { left: number[]; right: number[] }; form: { floorMm: number; cutAboveFloorMm: number | null };
};

const BODY = "/exhibits/pose/child-520";
const PATTERN = "/exhibits/poses/bolero-start.json";
const FIT = "/exhibits/poses/fit-spec.json";
// The opening drape: the drape room's bolero settings with the browser's step budget.
const SETTINGS = { spacing_mm: 12, sew_steps: 12, drape_steps: 36, max_settle_steps: 0, time_subdivision: true,
  self_contact_mm: 0, motion_step_s: 1 / 30 };
// A drag is followed in steps of at most STEP degrees, each moved at SPEED degrees per second of cloth time.
const STEP = 10, SPEED = 60;

/** Solver frame (z up, the wearer facing -y) to three.js (y up, the wearer facing +z). */
function toThree(source: ArrayLike<number>, target = new Float32Array(source.length)) {
  for (let i = 0; i < source.length; i += 3) {
    target[i] = source[i]; target[i + 1] = source[i + 2]; target[i + 2] = -source[i + 1];
  }
  return target;
}

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
    const status: PoseStatus = { phase: "loading", lines: [], arms: [35, 35], target: [35, 35] };
    const publish = () => report.current({ ...status, lines: [...status.lines] });
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
    canvas.setAttribute("aria-label", "A bolero on a child's form. Drag an arm up or down to pose it; the cloth is solved again as the arm moves. Drag elsewhere to turn the view, right-drag to move it, scroll or pinch to zoom.");
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
      camera.position.copy(homeTarget).add(new THREE.Vector3(0.55, 0.12, 1.75));
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
    const worker = new Worker("/labs/pose-worker.mjs", { type: "module" });
    let raf = 0;

    // Everything below needs the first drape.
    let ready: Ready | null = null;
    let bodyGeometry: THREE.BufferGeometry | null = null, bodyMesh: THREE.Mesh | null = null;
    let garment: ReturnType<typeof dressCloth> | null = null;
    let armOf = Uint8Array.from([]);
    let shownBody: Float32Array = Float32Array.from([]);
    const shoulders: THREE.Vector3[] = [];
    let range: [number, number] = [35, 135];
    let reached: [number, number] = [35, 35];
    const target: [number, number] = [35, 35];
    let busy = false, pendingSettle = false, moveId = 0;
    type Play = { from: Float32Array; to: Float32Array; samples: Float32Array; seconds: number; began: number };
    const plays: Play[] = [];
    let replay: { frames: Float32Array; count: number; began: number } | null = null;

    function stopReplay() {
      if (!replay || !ready || !garment) return;
      replay = null;
      garment.update(toThree(ready.cloth));
      if (status.phase === "replay") { status.phase = "ready"; publish(); }
      paint();
    }

    const guide = new THREE.Line(new THREE.BufferGeometry(), new THREE.LineDashedMaterial({ color: "#a24628", dashSize: 0.012, gapSize: 0.008 }));
    guide.visible = false;
    scene.add(guide);
    disposables.push(guide.geometry, guide.material as THREE.Material);
    let armLength = 0.3;
    function showGuide(side: number) {
      const s = shoulders[side];
      const a = THREE.MathUtils.degToRad(target[side]), sign = Math.sign(s.x) || 1;
      const end = s.clone().add(new THREE.Vector3(sign * Math.sin(a), -Math.cos(a), 0).multiplyScalar(armLength));
      guide.geometry.setFromPoints([s, end]);
      guide.computeLineDistances();
      guide.visible = true;
      paint();
    }

    function pump() {
      if (busy || !ready) return;
      const d0 = target[0] - reached[0], d1 = target[1] - reached[1];
      const most = Math.max(Math.abs(d0), Math.abs(d1));
      if (most < 0.5) {
        if (pendingSettle && !dragging) {
          pendingSettle = false;
          settle();
          return;
        }
        if (!dragging && plays.length === 0) { status.phase = "ready"; publish(); }
        return;
      }
      const k = Math.min(1, STEP / most);
      const next: [number, number] = [reached[0] + d0 * k, reached[1] + d1 * k];
      busy = true;
      status.phase = "solving";
      status.target = [...target];
      publish();
      worker.postMessage({ type: "move", id: ++moveId, left: next[0], right: next[1], seconds: Math.max(0.08, most * k / SPEED) });
    }

    function settle() {
      if (!ready) return;
      stopReplay();
      if (busy) { pendingSettle = true; return; }
      busy = true;
      status.phase = "solving";
      publish();
      worker.postMessage({ type: "move", id: ++moveId, left: reached[0], right: reached[1], seconds: 0.5 });
    }

    command.current = (c) => {
      if (!ready) return;
      if (c.kind === "settle") { settle(); return; }
      stopReplay();
      pendingSettle = false;
      target[0] = THREE.MathUtils.clamp(c.left, range[0], range[1]);
      target[1] = THREE.MathUtils.clamp(c.right, range[0], range[1]);
      status.target = [...target];
      status.refused = undefined;
      publish();
      pump();
    };

    function setBody(points: Float32Array) {
      if (!bodyGeometry) return;
      (bodyGeometry.getAttribute("position") as THREE.BufferAttribute).copyArray(points).needsUpdate = true;
      bodyGeometry.computeVertexNormals();
      bodyGeometry.computeBoundingSphere();
      shownBody = points;
    }

    worker.onmessage = (event: MessageEvent) => {
      if (!alive) return;
      const reply = event.data;
      if (reply.type === "progress") {
        status.lines = [...status.lines.slice(-5), reply.line];
        if (status.phase === "loading" && /^start:|^sew/.test(reply.line)) status.phase = "sewing";
        publish();
      } else if (reply.type === "failed") {
        Object.assign(status, { phase: "failed", message: reply.message });
        publish();
      } else if (reply.type === "ready") {
        ready = reply as Ready;
        build(ready);
      } else if (reply.type === "moved" && ready) {
        const stride = 1 + ready.summary.cloth_vertices * 3;
        const degrees = Math.max(Math.abs(reply.left - reached[0]), Math.abs(reply.right - reached[1]));
        reached = [reply.left, reply.right];
        const to = toThree(reply.display);
        const seconds = reply.samples.length ? reply.samples[reply.samples.length - stride] : 0;
        plays.push({ from: plays.length ? plays[plays.length - 1].to : shownBody, to, samples: reply.samples, seconds, began: -1 });
        status.arms = [...reached];
        status.last = { degrees, clothSeconds: seconds, steps: reply.samples.length / stride, milliseconds: reply.milliseconds };
        busy = false;
        pump();
        publish();
      } else if (reply.type === "refused") {
        busy = false;
        pendingSettle = false;
        target[0] = reached[0];
        target[1] = reached[1];
        Object.assign(status, { phase: "ready", refused: reply.message, target: [...reached] });
        publish();
      }
    };
    worker.onerror = (e) => {
      Object.assign(status, { phase: "failed", message: e.message || "The solver worker stopped." });
      publish();
    };
    worker.postMessage({ type: "start", body: BODY, pattern: PATTERN, fit: FIT, settings: SETTINGS });

    function build(r: Ready) {
      const n = r.summary.cloth_vertices;
      range = r.summary.arm_range_deg;
      reached = [r.summary.base_arm_deg, r.summary.base_arm_deg];
      target[0] = target[1] = r.summary.base_arm_deg;
      // The body: a plain tailor's form, cut below the waist, on a turned stand.
      const display = toThree(r.display);
      bodyGeometry = new THREE.BufferGeometry();
      bodyGeometry.setAttribute("position", new THREE.BufferAttribute(display.slice(), 3));
      bodyGeometry.setIndex(new THREE.BufferAttribute(r.displayTriangles, 1));
      bodyGeometry.computeVertexNormals();
      shownBody = display;
      const plaster = formMaterial(undefined, { inset: 0.0025 });
      bodyMesh = new THREE.Mesh(bodyGeometry, plaster);
      bodyMesh.castShadow = bodyMesh.receiveShadow = true;
      scene.add(bodyMesh);
      disposables.push(bodyGeometry, plaster);
      // Which displayed vertices belong to each arm: those the baked arm samples move.
      armOf = new Uint8Array(display.length / 3);
      for (let v = 0; v < armOf.length; v++) {
        for (const [side, raised] of [[1, r.raised[0]], [2, r.raised[1]]] as const) {
          const dx = raised[v * 3] - r.display[v * 3], dy = raised[v * 3 + 1] - r.display[v * 3 + 1], dz = raised[v * 3 + 2] - r.display[v * 3 + 2];
          if (dx * dx + dy * dy + dz * dz > 1e-6) armOf[v] = side;
        }
      }
      for (const s of [r.shoulders.left, r.shoulders.right]) shoulders.push(new THREE.Vector3(s[0] / 1000, s[2] / 1000, -s[1] / 1000));
      // Arm length from the shoulder to the farthest arm vertex.
      for (let v = 0; v < armOf.length; v++) if (armOf[v]) {
        const s = shoulders[armOf[v] - 1];
        armLength = Math.max(armLength, Math.hypot(display[v * 3] - s.x, display[v * 3 + 1] - s.y, display[v * 3 + 2] - s.z) * 0.92);
      }
      const box = new THREE.Box3().setFromBufferAttribute(bodyGeometry.getAttribute("position") as THREE.BufferAttribute);
      const floorY = r.form.floorMm / 1000;
      const cutY = r.form.cutAboveFloorMm === null ? box.min.y : floorY + r.form.cutAboveFloorMm / 1000;
      const brass = new THREE.MeshStandardMaterial({ color: "#a08255", metalness: 0.5, roughness: 0.42 });
      const lacquer = new THREE.MeshStandardMaterial({ color: "#423b34", roughness: 0.52 });
      const centre = box.getCenter(new THREE.Vector3());
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
      // The cloth: the solver's own surface, drawn with the site's woven-cloth material.
      const sewn = toThree(r.cloth);
      garment = dressCloth({
        vertices: n, triangles: r.triangles,
        seams: r.summary.seams.map(s => ({ side_a: s.side_a, side_b: s.side_b })),
        panels: Object.fromEntries(r.summary.pieces.map(p => [p.id, [p.first, p.end] as [number, number]])),
      }, Float32Array.from(r.flat, v => v / 1000), fabrics.greenWool, { sewn, edge: "braid" });
      garment.tintAmount.value = 0;
      scene.add(garment.mesh);
      disposables.push(garment);
      Object.assign(status, { openingSeconds: r.seconds, clothVertices: n, range, arms: [...reached], target: [...target] });
      // Replay the sewing and the drape the solver has just done, then hand the figure over.
      const count = r.frames.length / (n * 3);
      if (count > 1 && !matchMedia("(prefers-reduced-motion: reduce)").matches) {
        replay = { frames: r.frames, count, began: -1 };
        status.phase = "replay";
      } else {
        garment.update(sewn);
        status.phase = "ready";
      }
      publish();
      paint();
    }

    // Dragging an arm.
    const pointer = new THREE.Vector2(), ray = new THREE.Raycaster(), plane = new THREE.Plane(), hit = new THREE.Vector3();
    let dragging = 0, dragFrom = { y: 0, angle: 0 }, hover = 0;
    const armAt = (event: PointerEvent) => {
      if (!bodyMesh || !bodyGeometry) return 0;
      const rect = canvas.getBoundingClientRect();
      if (!rect.width || !rect.height) return 0;
      pointer.set(((event.clientX - rect.left) / rect.width) * 2 - 1, -((event.clientY - rect.top) / rect.height) * 2 + 1);
      ray.setFromCamera(pointer, camera);
      const found = ray.intersectObject(bodyMesh, false)[0];
      if (!found || found.faceIndex == null) return 0;
      const index = bodyGeometry.getIndex()!;
      return armOf[index.getX(found.faceIndex * 3)];
    };
    const angleAt = (event: PointerEvent, side: number) => {
      const rect = canvas.getBoundingClientRect();
      if (!rect.width || !rect.height) return target[side];
      pointer.set(((event.clientX - rect.left) / rect.width) * 2 - 1, -((event.clientY - rect.top) / rect.height) * 2 + 1);
      ray.setFromCamera(pointer, camera);
      const s = shoulders[side];
      // The arms swing in the body's frontal plane. Seen nearly edge-on, follow the pointer's height instead.
      if (Math.abs(ray.ray.direction.z) < 0.3) return dragFrom.angle + (dragFrom.y - event.clientY) * 0.45;
      plane.set(new THREE.Vector3(0, 0, 1), -s.z);
      if (!ray.ray.intersectPlane(plane, hit)) return target[side];
      const outward = (hit.x - s.x) * (Math.sign(s.x) || 1), down = s.y - hit.y;
      return THREE.MathUtils.radToDeg(Math.atan2(outward, down));
    };
    const down = (event: PointerEvent) => {
      if (event.button !== 0 || !event.isPrimary || status.phase === "loading" || status.phase === "sewing" || status.phase === "failed") return;
      const arm = armAt(event);
      if (!arm) return;
      stopReplay();
      pendingSettle = false;
      dragging = arm;
      dragFrom = { y: event.clientY, angle: target[arm - 1] };
      orbit.enabled = false;
      canvas.setPointerCapture(event.pointerId);
      canvas.style.cursor = "grabbing";
      status.refused = undefined;
      event.stopImmediatePropagation();
      event.preventDefault();
      showGuide(arm - 1);
    };
    const move = (event: PointerEvent) => {
      if (!dragging) {
        if (event.pointerType === "mouse" && !event.buttons) {
          const arm = armAt(event);
          if (arm !== hover) { hover = arm; canvas.style.cursor = arm ? "grab" : ""; }
        }
        return;
      }
      const side = dragging - 1;
      target[side] = THREE.MathUtils.clamp(angleAt(event, side), range[0], range[1]);
      status.target = [...target];
      publish();
      showGuide(side);
      pump();
      event.preventDefault();
    };
    const up = (event: PointerEvent) => {
      if (!dragging) return;
      dragging = 0;
      orbit.enabled = true;
      if (canvas.hasPointerCapture(event.pointerId)) canvas.releasePointerCapture(event.pointerId);
      canvas.style.cursor = "";
      guide.visible = false;
      paint();
      pump();
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

    // One loop draws only when something changed: the camera, a replay, or accepted solver samples.
    let blend = Float32Array.from([]), bodyBlend = Float32Array.from([]);
    function frame(now: number) {
      raf = requestAnimationFrame(frame);
      if (orbit.update()) dirty = true;
      if (ready && garment && replay) {
        if (replay.began < 0) replay.began = now;
        const n = ready.summary.cloth_vertices * 3, t = Math.min(1, (now - replay.began) / 3600);
        const at = t * (replay.count - 1), i = Math.min(replay.count - 2, Math.floor(at)), f = at - i;
        if (blend.length !== n) blend = new Float32Array(n);
        for (let k = 0; k < n; k++) blend[k] = replay.frames[i * n + k] * (1 - f) + replay.frames[(i + 1) * n + k] * f;
        garment.update(toThree(blend, new Float32Array(n)));
        dirty = true;
        if (t >= 1) {
          replay = null;
          garment.update(toThree(ready.cloth));
          if (status.phase === "replay") { status.phase = "ready"; publish(); }
        }
      } else if (ready && garment && plays.length) {
        const play = plays[0], n = ready.summary.cloth_vertices * 3, stride = n + 1;
        if (play.began < 0) play.began = now;
        const count = Math.floor(play.samples.length / stride);
        if (count === 0) {
          setBody(play.to);
          dirty = true;
          plays.shift();
          if (!plays.length && !busy) pump();
        } else {
          const span = Math.max(play.seconds, 0.12) * 1000, t = Math.min(1, (now - play.began) / span) * play.seconds;
          // The sample before and after the clock, and the body between the move's two ends as the solver moved it.
          let k = 0;
          while (k < count - 1 && play.samples[k * stride] < t) k++;
          const t1 = play.samples[k * stride], t0 = k ? play.samples[(k - 1) * stride] : 0;
          const f = t1 > t0 ? Math.min(1, Math.max(0, (t - t0) / (t1 - t0))) : 1;
          if (blend.length !== n) blend = new Float32Array(n);
          for (let j = 0; j < n; j++) {
            const b = play.samples[k * stride + 1 + j];
            const a = k ? play.samples[(k - 1) * stride + 1 + j] : b;
            blend[j] = a * (1 - f) + b * f;
          }
          garment.update(toThree(blend, new Float32Array(n)));
          const g = play.seconds > 0 ? t / play.seconds : 1;
          if (bodyBlend.length !== play.to.length) bodyBlend = new Float32Array(play.to.length);
          for (let j = 0; j < play.to.length; j++) bodyBlend[j] = play.from[j] * (1 - g) + play.to[j] * g;
          setBody(bodyBlend.slice());
          dirty = true;
          if (t >= play.seconds) {
            setBody(play.to);
            plays.shift();
            if (!plays.length && !busy) pump();
          }
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
      worker.terminate();
      observer.disconnect();
      look.dispose();
      orbit.dispose();
      canvas.removeEventListener("pointerdown", down, true);
      canvas.removeEventListener("pointermove", move);
      canvas.removeEventListener("pointerup", up);
      canvas.removeEventListener("pointercancel", up);
      canvas.removeEventListener("webglcontextlost", lost);
      for (const item of disposables) item.dispose();
      studio.dispose();
      renderer.dispose();
      renderer.forceContextLoss();
      canvas.remove();
    };
  }, [command]);

  return <div ref={host} className="pose-canvas" />;
}
