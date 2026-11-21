"use client";

import { useEffect, useRef } from "react";
import * as T from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { formMaterial, lightStudio, prepareRenderer, shadowFloor } from "../atelier3d/studio";

export type CameraPreset = "right-chest" | "left-chest" | "chest-front" | "full";
export type RegimentLook = "Blue" | "Yellow";

export type RegisteredStitchPoint = {
  index: number;
  xMm: number;
  yMm: number;
  kind: "stitch" | "jump" | "stop";
  pass: "placement" | "couching" | "stop";
  flatMm: [number, number];
  onSurface: boolean;
  reason: string;
  tri: number | null;
  bary: [number, number, number] | null;
  residualMm: number | null;
  pos3d: [number, number, number] | null;
  normal3d: [number, number, number] | null;
};

export type DolmanMeshData = {
  bodyUpright: Float32Array;
  bodyIndex: Uint32Array;
  clothUpright: Float32Array;
  triangles: number[];
  panels: Record<string, [number, number]>;
};

const TRUNK_HALF_WIDTH = 0.27;
const MOUNT_AT = 0.78;
const SURFACE_LIFT_M = 0.0018; // 1.8 mm outward along triangle normal so thread sits cleanly on outer wool face

const CAMERA_PRESETS: Record<CameraPreset, { yaw: number; targetY: number; span: number }> = {
  "right-chest": { yaw: -0.34, targetY: 1.29, span: 0.24 },
  "left-chest": { yaw: 0.34, targetY: 1.29, span: 0.24 },
  "chest-front": { yaw: 0.0, targetY: 1.28, span: 0.28 },
  full: { yaw: -0.22, targetY: 1.18, span: 0.56 },
};

export default function DolmanStitchScene({
  mesh,
  points,
  cursor,
  activePanel,
  regiment,
  cameraPreset,
  resetKey,
  onReady,
  onError,
}: {
  mesh: DolmanMeshData;
  points: RegisteredStitchPoint[];
  cursor: number;
  activePanel: string | null;
  regiment: RegimentLook;
  cameraPreset: CameraPreset;
  resetKey: number;
  onReady?: () => void;
  onError?: () => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const stateRef = useRef({ points, cursor, activePanel, regiment, cameraPreset, resetKey, onReady, onError });
  const redrawRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    stateRef.current = { points, cursor, activePanel, regiment, cameraPreset, resetKey, onReady, onError };
    redrawRef.current?.();
  }, [points, cursor, activePanel, regiment, cameraPreset, resetKey, onReady, onError]);

  useEffect(() => {
    const surface = host.current;
    if (!surface) return;

    let renderer: T.WebGLRenderer;
    try {
      renderer = new T.WebGLRenderer({ antialias: true, alpha: true, powerPreference: "low-power" });
    } catch {
      stateRef.current.onError?.();
      return;
    }

    const canvas = renderer.domElement;
    canvas.style.cssText = "width:100%;height:100%;display:block;cursor:grab";
    canvas.tabIndex = 0;
    canvas.setAttribute("role", "img");
    canvas.setAttribute(
      "aria-label",
      "Settled 3D 1761 Swedish hussar dolman showing registered Tajima DST embroidery stitches on the chest. Drag or use arrow keys to orbit, scroll or +/- to zoom.",
    );
    surface.appendChild(canvas);
    prepareRenderer(renderer);
    renderer.setClearColor(0, 0);

    const scene = new T.Scene();
    const studio = lightStudio(renderer, scene, "warm");
    const camera = new T.OrthographicCamera(-1, 1, 1, -1, 0.01, 20);
    const orbit = new OrbitControls(camera, canvas);
    orbit.enablePan = false;
    orbit.minZoom = 0.55;
    orbit.maxZoom = 4.5;
    orbit.rotateSpeed = 0.85;
    orbit.zoomSpeed = 0.9;
    orbit.minPolarAngle = 0.12;
    orbit.maxPolarAngle = Math.PI - 0.12;

    const disposables: { dispose(): void }[] = [];

    // 1. Trimmed museum bust mount from solved Anny body
    const bodyGeo = new T.BufferGeometry();
    bodyGeo.setAttribute("position", new T.Float32BufferAttribute(mesh.bodyUpright, 3));
    bodyGeo.setIndex(new T.BufferAttribute(mesh.bodyIndex, 1));
    bodyGeo.computeBoundingBox();
    const bodyBox = bodyGeo.boundingBox!;
    const stand = new T.Vector3(
      (bodyBox.min.x + bodyBox.max.x) / 2,
      bodyBox.min.y,
      (bodyBox.min.z + bodyBox.max.z) / 2,
    );
    const hipY = stand.y + MOUNT_AT;
    const keptBody: number[] = [];
    const belowHip = (vi: number) =>
      mesh.bodyUpright[vi * 3 + 1] < hipY && Math.abs(mesh.bodyUpright[vi * 3] - stand.x) < TRUNK_HALF_WIDTH;
    for (let i = 0; i < mesh.bodyIndex.length; i += 3) {
      const a = mesh.bodyIndex[i], b = mesh.bodyIndex[i + 1], c = mesh.bodyIndex[i + 2];
      if (!(belowHip(a) || belowHip(b) || belowHip(c))) keptBody.push(a, b, c);
    }
    bodyGeo.setIndex(keptBody);
    bodyGeo.computeVertexNormals();
    bodyGeo.computeBoundingSphere();
    studio.frame(bodyGeo.boundingSphere!.center, bodyGeo.boundingSphere!.radius);
    const formMat = formMaterial();
    const bodyMesh = new T.Mesh(bodyGeo, formMat);
    bodyMesh.castShadow = true;
    bodyMesh.receiveShadow = true;
    scene.add(bodyMesh);
    disposables.push(bodyGeo, formMat);

    const floor = shadowFloor(bodyBox.min.y - 0.0005, 0.22);
    floor.mesh.position.set(stand.x, bodyBox.min.y - 0.0005, stand.z);
    scene.add(floor.mesh);
    disposables.push(floor);

    // 2. Settled 3D dolman jacket (split into non-active panels and highlighted active front panel)
    const clothGeo = new T.BufferGeometry();
    clothGeo.setAttribute("position", new T.Float32BufferAttribute(mesh.clothUpright, 3));
    clothGeo.setIndex(mesh.triangles);
    clothGeo.computeVertexNormals();
    const clothMat = new T.MeshStandardMaterial({
      color: "#21344d",
      roughness: 0.9,
      metalness: 0.02,
      side: T.DoubleSide,
      polygonOffset: true,
      polygonOffsetFactor: 1,
      polygonOffsetUnits: 1,
    });
    const clothMesh = new T.Mesh(clothGeo, clothMat);
    clothMesh.castShadow = true;
    clothMesh.receiveShadow = true;
    scene.add(clothMesh);
    disposables.push(clothGeo, clothMat);

    // Subtle highlighted overlay on the active registered front panel
    const activePanelGeo = new T.BufferGeometry();
    activePanelGeo.setAttribute("position", new T.Float32BufferAttribute(mesh.clothUpright, 3));
    const activePanelMat = new T.MeshStandardMaterial({
      color: "#2c4666",
      roughness: 0.86,
      metalness: 0.03,
      side: T.DoubleSide,
      polygonOffset: true,
      polygonOffsetFactor: -0.4,
      polygonOffsetUnits: -0.4,
    });
    const activePanelMesh = new T.Mesh(activePanelGeo, activePanelMat);
    scene.add(activePanelMesh);
    disposables.push(activePanelGeo, activePanelMat);

    // 3. Registered 3D DST embroidery lines (Placement pass = chalk cream, Couching pass = gold)
    const placementGeo = new T.BufferGeometry();
    const placementMat = new T.LineBasicMaterial({ color: "#f5efe0", transparent: true, opacity: 0.92 });
    const placementLines = new T.LineSegments(placementGeo, placementMat);
    placementLines.renderOrder = 3;
    scene.add(placementLines);
    disposables.push(placementGeo, placementMat);

    const couchGeo = new T.BufferGeometry();
    const couchMat = new T.LineBasicMaterial({ color: "#e8c46a" });
    const couchLines = new T.LineSegments(couchGeo, couchMat);
    couchLines.renderOrder = 4;
    scene.add(couchLines);
    disposables.push(couchGeo, couchMat);

    // 4. Live 3D needle drop beacon & outward normal pin
    const needleGroup = new T.Group();
    const dotGeo = new T.SphereGeometry(0.0046, 18, 14);
    const dotMat = new T.MeshBasicMaterial({ color: "#ff6938" });
    const dotMesh = new T.Mesh(dotGeo, dotMat);
    const pinGeo = new T.CylinderGeometry(0.0007, 0.0011, 0.022, 10);
    const pinMat = new T.MeshBasicMaterial({ color: "#fff7e0" });
    const pinMesh = new T.Mesh(pinGeo, pinMat);
    pinMesh.position.y = 0.011;
    needleGroup.add(dotMesh, pinMesh);
    needleGroup.visible = false;
    needleGroup.renderOrder = 6;
    scene.add(needleGroup);
    disposables.push(dotGeo, dotMat, pinGeo, pinMat);

    let width = 0;
    let height = 0;
    let readyFired = false;
    let lastPreset: CameraPreset | null = null;
    let lastReset = -1;
    let lastPanel: string | null | undefined = undefined;
    let lastPointsRef: RegisteredStitchPoint[] | null = null;
    let lastCursor = -1;
    let lastRegiment: RegimentLook | null = null;

    const upVec = new T.Vector3(0, 1, 0);
    const normVec = new T.Vector3();

    function applyCamera(preset: CameraPreset) {
      const cfg = CAMERA_PRESETS[preset];
      const aspect = width && height ? width / height : 1;
      const span = cfg.span;
      camera.left = -span * aspect;
      camera.right = span * aspect;
      camera.top = span;
      camera.bottom = -span;
      camera.zoom = 1;
      camera.updateProjectionMatrix();
      const ty = stand.y + cfg.targetY;
      orbit.target.set(stand.x, ty, stand.z + 0.02);
      const r = 2.6;
      const elev = 0.08;
      camera.position.set(
        stand.x + r * Math.cos(elev) * Math.sin(cfg.yaw),
        ty + r * Math.sin(elev),
        stand.z + r * Math.cos(elev) * Math.cos(cfg.yaw),
      );
      orbit.update();
    }

    function paint() {
      if (!width || !height) return;
      renderer.render(scene, camera);
      canvas.dataset.ready = "true";
      canvas.dataset.cursor = String(stateRef.current.cursor);
      canvas.dataset.panel = stateRef.current.activePanel ?? "unregistered";
      if (!readyFired) {
        readyFired = true;
        stateRef.current.onReady?.();
      }
    }

    function syncScene() {
      const st = stateRef.current;

      if (st.regiment !== lastRegiment) {
        lastRegiment = st.regiment;
        clothMat.color.set(st.regiment === "Blue" ? "#1e3047" : "#222726");
        activePanelMat.color.set(st.regiment === "Blue" ? "#284160" : "#2f3634");
        couchMat.color.set(st.regiment === "Blue" ? "#ebca73" : "#dcd8c8");
      }

      if (st.activePanel !== lastPanel) {
        lastPanel = st.activePanel;
        const bounds = st.activePanel ? mesh.panels[st.activePanel] : null;
        if (bounds) {
          const [lo, hi] = bounds;
          const sub: number[] = [];
          for (let t = 0; t < mesh.triangles.length; t += 3) {
            const a = mesh.triangles[t], b = mesh.triangles[t + 1], c = mesh.triangles[t + 2];
            if (a >= lo && a < hi && b >= lo && b < hi && c >= lo && c < hi) sub.push(a, b, c);
          }
          activePanelGeo.setIndex(sub);
          activePanelGeo.computeVertexNormals();
          activePanelMesh.visible = true;
        } else {
          activePanelMesh.visible = false;
        }
      }

      if (st.points !== lastPointsRef || st.cursor !== lastCursor) {
        lastPointsRef = st.points;
        lastCursor = st.cursor;
        const placeVerts: number[] = [];
        const couchVerts: number[] = [];
        const limit = Math.min(st.cursor, st.points.length - 1);

        let prev: RegisteredStitchPoint | null = null;
        for (let i = 0; i <= limit; i++) {
          const pt = st.points[i];
          if (pt.onSurface && pt.pos3d && pt.normal3d) {
            if (prev && prev.onSurface && prev.pos3d && prev.normal3d && prev.pass === pt.pass) {
              const arr = pt.pass === "couching" ? couchVerts : placeVerts;
              arr.push(
                prev.pos3d[0] + prev.normal3d[0] * SURFACE_LIFT_M,
                prev.pos3d[1] + prev.normal3d[1] * SURFACE_LIFT_M,
                prev.pos3d[2] + prev.normal3d[2] * SURFACE_LIFT_M,
                pt.pos3d[0] + pt.normal3d[0] * SURFACE_LIFT_M,
                pt.pos3d[1] + pt.normal3d[1] * SURFACE_LIFT_M,
                pt.pos3d[2] + pt.normal3d[2] * SURFACE_LIFT_M,
              );
            }
            prev = pt;
          } else {
            // Jump / stop / unmatched segment breaks the 3D stitch chain so jumps are NEVER drawn on the garment.
            prev = null;
          }
        }

        placementGeo.setAttribute("position", new T.Float32BufferAttribute(placeVerts, 3));
        placementGeo.computeBoundingSphere();
        couchGeo.setAttribute("position", new T.Float32BufferAttribute(couchVerts, 3));
        couchGeo.computeBoundingSphere();

        const cur = st.points[limit];
        if (cur && cur.onSurface && cur.pos3d && cur.normal3d) {
          needleGroup.visible = true;
          needleGroup.position.set(
            cur.pos3d[0] + cur.normal3d[0] * (SURFACE_LIFT_M + 0.001),
            cur.pos3d[1] + cur.normal3d[1] * (SURFACE_LIFT_M + 0.001),
            cur.pos3d[2] + cur.normal3d[2] * (SURFACE_LIFT_M + 0.001),
          );
          normVec.set(cur.normal3d[0], cur.normal3d[1], cur.normal3d[2]).normalize();
          needleGroup.quaternion.setFromUnitVectors(upVec, normVec);
          dotMat.color.set(cur.pass === "couching" ? "#ffb649" : "#ff6938");
        } else {
          needleGroup.visible = false;
        }
      }

      if (st.cameraPreset !== lastPreset || st.resetKey !== lastReset) {
        lastPreset = st.cameraPreset;
        lastReset = st.resetKey;
        if (width && height) applyCamera(st.cameraPreset);
      }

      paint();
    }

    redrawRef.current = syncScene;

    const resize = () => {
      width = surface.clientWidth;
      height = surface.clientHeight;
      if (!width || !height) return;
      renderer.setSize(width, height, false);
      applyCamera(stateRef.current.cameraPreset);
      syncScene();
    };
    const observer = new ResizeObserver(resize);
    observer.observe(surface);
    resize();

    const onKey = (e: KeyboardEvent) => {
      if (e.altKey || e.ctrlKey || e.metaKey) return;
      let yaw = orbit.getAzimuthalAngle();
      let polar = orbit.getPolarAngle();
      const step = Math.PI / 18;
      switch (e.key) {
        case "ArrowLeft": yaw -= step; break;
        case "ArrowRight": yaw += step; break;
        case "ArrowUp": polar = Math.max(0.15, polar - step); break;
        case "ArrowDown": polar = Math.min(Math.PI - 0.15, polar + step); break;
        case "+": case "=": camera.zoom = Math.min(orbit.maxZoom, camera.zoom * 1.15); camera.updateProjectionMatrix(); paint(); e.preventDefault(); return;
        case "-": camera.zoom = Math.max(orbit.minZoom, camera.zoom / 1.15); camera.updateProjectionMatrix(); paint(); e.preventDefault(); return;
        default: return;
      }
      e.preventDefault();
      const dist = camera.position.distanceTo(orbit.target);
      camera.position.set(
        orbit.target.x + dist * Math.sin(polar) * Math.sin(yaw),
        orbit.target.y + dist * Math.cos(polar),
        orbit.target.z + dist * Math.sin(polar) * Math.cos(yaw),
      );
      orbit.update();
      paint();
    };
    const onLost = (e: Event) => { e.preventDefault(); stateRef.current.onError?.(); };
    orbit.addEventListener("change", paint);
    canvas.addEventListener("keydown", onKey);
    canvas.addEventListener("webglcontextlost", onLost);

    return () => {
      redrawRef.current = null;
      observer.disconnect();
      orbit.removeEventListener("change", paint);
      canvas.removeEventListener("keydown", onKey);
      canvas.removeEventListener("webglcontextlost", onLost);
      orbit.dispose();
      for (const d of disposables) d.dispose();
      renderer.dispose();
      canvas.remove();
    };
  }, [mesh]);

  return <div ref={host} className="hs-3d-host" />;
}
