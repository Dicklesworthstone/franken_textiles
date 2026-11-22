"use client";

import { useEffect, useRef } from "react";
import * as THREE from "three";
import { buildPieceOutline, projectWaistcoatPointTo3D } from "../public/labs/waistcoat.mjs";

type SeamRecord = {
  index: number;
  id: string;
  a: string;
  b: string;
  instanceA: string;
  edgeA: string;
  layerA: string;
  instanceB: string;
  edgeB: string;
  layerB: string;
  lenAMm: number;
  lenBMm: number;
  residualMm: number;
};

type UnpairedRecord = {
  ref: string;
  instance: string;
  piece: string;
  edge: string;
  layer: string;
  kind: string;
  reason: string;
};

type PatternDoc = {
  pieces: {
    id: string;
    name: string;
    cut: { qty: number; material?: string };
    vertices: Record<string, [number, number]>;
    edges: { id: string; from: string; to: string; curve?: { type: string; c: [number, number][] } }[];
    internal?: { kind: string; label: string; pts: [number, number][] }[];
    notches?: { id: string; edge: string; s: number; type: string }[];
  }[];
};

export type CameraPreset = "Front" | "Three-quarter" | "Back" | "Inside-out" | "Neck & collar";

const PRESETS: Record<CameraPreset, { yaw: number; pitch: number; dist: number; targetY: number }> = {
  Front: { yaw: 0.0, pitch: 0.08, dist: 1.28, targetY: 0.01 },
  "Three-quarter": { yaw: -0.62, pitch: 0.14, dist: 1.32, targetY: 0.01 },
  Back: { yaw: Math.PI, pitch: 0.08, dist: 1.28, targetY: 0.01 },
  "Inside-out": { yaw: -2.42, pitch: 0.16, dist: 1.20, targetY: 0.01 },
  "Neck & collar": { yaw: -0.45, pitch: 0.26, dist: 0.86, targetY: 0.20 },
};

const LAYER_STYLES: Record<string, { color: number; emissive: number; opacity: number }> = {
  shell: { color: 0xebe0c9, emissive: 0x1a150e, opacity: 1.0 },
  facing: { color: 0xdca747, emissive: 0x261a05, opacity: 0.98 },
  lining: { color: 0x4b9384, emissive: 0x0b1f1b, opacity: 0.97 },
  pocket: { color: 0x39677d, emissive: 0x0b1921, opacity: 0.98 },
};

function yRangeAtX(poly: [number, number][], x: number): [number, number] | null {
  const ys: number[] = [];
  for (let i = 0; i < poly.length; i++) {
    const [x0, y0] = poly[i];
    const [x1, y1] = poly[(i + 1) % poly.length];
    if ((x0 <= x && x1 >= x) || (x1 <= x && x0 >= x)) {
      if (Math.abs(x1 - x0) < 1e-6) {
        if (Math.abs(x - x0) < 1e-5) {
          ys.push(y0, y1);
        }
      } else {
        const t = (x - x0) / (x1 - x0);
        if (t >= -1e-6 && t <= 1 + 1e-6) {
          ys.push(y0 + t * (y1 - y0));
        }
      }
    }
  }
  if (ys.length < 2) return null;
  let lo = Infinity;
  let hi = -Infinity;
  for (const y of ys) {
    if (y < lo) lo = y;
    if (y > hi) hi = y;
  }
  return hi - lo > 0.5 ? [lo, hi] : null;
}

function buildPanelGeometry(instanceId: string, outlinePts: [number, number][], radialOffsetMm: number) {
  // Build a smooth x-monotone vertical-strip grid so every curved panel hugs the 3D elliptical torso cleanly
  // with zero chord flattening across wide torso spans.
  let minX = Infinity;
  let maxX = -Infinity;
  for (const [x] of outlinePts) {
    if (x < minX) minX = x;
    if (x > maxX) maxX = x;
  }
  const cols = Math.max(10, Math.ceil((maxX - minX) / 14));
  const xsSet = new Set<number>();
  for (let i = 0; i <= cols; i++) {
    xsSet.add(Number((minX + ((maxX - minX) * i) / cols).toFixed(3)));
  }
  // Include key corner x breakpoints (e.g. back vent step at ±16 mm, facing split at ±82 mm) cleanly.
  for (const [x] of outlinePts) {
    xsSet.add(Number(x.toFixed(3)));
  }
  const xsRaw = Array.from(xsSet).sort((a, b) => a - b);
  // Deduplicate ultra-close x values (< 1.5 mm) except exact endpoints.
  const xs: number[] = [];
  for (let i = 0; i < xsRaw.length; i++) {
    if (i === 0 || i === xsRaw.length - 1 || xsRaw[i] - xs[xs.length - 1] >= 1.5) {
      xs.push(xsRaw[i]);
    }
  }

  const rows = 10;
  const pos: number[] = [];
  const indices: number[] = [];
  let prevColStart = -1;

  for (let c = 0; c < xs.length; c++) {
    // Evaluate slightly inside endpoints so vertical step edges resolve cleanly to their upper/lower range.
    const xVal = xs[c];
    const probeX =
      c === 0
        ? Math.min(maxX, xVal + 0.25)
        : c === xs.length - 1
        ? Math.max(minX, xVal - 0.25)
        : xVal;
    const range = yRangeAtX(outlinePts, probeX);
    if (!range) {
      prevColStart = -1;
      continue;
    }
    const [yLo, yHi] = range;
    const colStart = pos.length / 3;
    for (let r = 0; r <= rows; r++) {
      const y = yLo + ((yHi - yLo) * r) / rows;
      const p3 = projectWaistcoatPointTo3D(instanceId, xVal, y, radialOffsetMm);
      pos.push(p3[0], p3[1], p3[2]);
    }
    if (prevColStart >= 0) {
      for (let r = 0; r < rows; r++) {
        const a = prevColStart + r;
        const b = colStart + r;
        const d = colStart + r + 1;
        const e = prevColStart + r + 1;
        indices.push(a, b, d, a, d, e);
      }
    }
    prevColStart = colStart;
  }

  const geom = new THREE.BufferGeometry();
  geom.setAttribute("position", new THREE.Float32BufferAttribute(pos, 3));
  geom.setIndex(indices);
  geom.computeVertexNormals();
  return geom;
}

function buildEdgeLineGeometry(instanceId: string, edgePts: [number, number][], radialOffsetMm: number) {
  const dense: [number, number][] = [];
  for (let i = 0; i < edgePts.length - 1; i++) {
    const a = edgePts[i];
    const b = edgePts[i + 1];
    const d = Math.hypot(b[0] - a[0], b[1] - a[1]);
    const steps = Math.max(1, Math.ceil(d / 18));
    for (let s = 0; s < steps; s++) {
      const t = s / steps;
      dense.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
    }
  }
  dense.push(edgePts[edgePts.length - 1]);
  const pts3 = dense.map(([x, y]) => {
    const [px, py, pz] = projectWaistcoatPointTo3D(instanceId, x, y, radialOffsetMm);
    return new THREE.Vector3(px, py, pz);
  });
  return { geom: new THREE.BufferGeometry().setFromPoints(pts3), pts3 };
}

export default function InsideOutScene({
  pattern,
  layers,
  layerVisible,
  radialOffsetMm,
  selectedSeam,
  selectedUnpaired,
  preset,
  onFailure,
}: {
  pattern: PatternDoc;
  layers: Record<string, string>;
  layerVisible: Record<string, boolean>;
  radialOffsetMm: Record<string, number>;
  selectedSeam: SeamRecord | null;
  selectedUnpaired: UnpairedRecord | null;
  preset: { name: CameraPreset; nonce: number };
  onFailure: () => void;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const stateRef = useRef({
    yaw: PRESETS["Three-quarter"].yaw,
    pitch: PRESETS["Three-quarter"].pitch,
    dist: PRESETS["Three-quarter"].dist,
    targetY: PRESETS["Three-quarter"].targetY,
    targetYaw: PRESETS["Three-quarter"].yaw,
    targetPitch: PRESETS["Three-quarter"].pitch,
    targetDist: PRESETS["Three-quarter"].dist,
    targetTargetY: PRESETS["Three-quarter"].targetY,
  });
  const sceneObjsRef = useRef<{
    renderer: THREE.WebGLRenderer;
    scene: THREE.Scene;
    camera: THREE.PerspectiveCamera;
    garmentGroup: THREE.Group;
    highlightGroup: THREE.Group;
    disposables: { dispose: () => void }[];
  } | null>(null);

type PieceOutline3D = {
  id: string;
  instance: string;
  polygon: [number, number][];
  edges: { id: string; pts: [number, number][] }[];
};

  // Precompute flat outlines for all 17 cut instances once.
  const outlinesRef = useRef<Map<string, PieceOutline3D>>(new Map());
  if (outlinesRef.current.size === 0) {
    for (const p of pattern.pieces) {
      if (p.cut.qty === 2) {
        outlinesRef.current.set(`${p.id}.R`, buildPieceOutline(pattern, p.id, "R") as unknown as PieceOutline3D);
        outlinesRef.current.set(`${p.id}.L`, buildPieceOutline(pattern, p.id, "L") as unknown as PieceOutline3D);
      } else {
        outlinesRef.current.set(p.id, buildPieceOutline(pattern, p.id, "R") as unknown as PieceOutline3D);
      }
    }
  }

  // Update camera targets on preset click.
  useEffect(() => {
    const p = PRESETS[preset.name];
    stateRef.current.targetYaw = p.yaw;
    stateRef.current.targetPitch = p.pitch;
    stateRef.current.targetDist = p.dist;
    stateRef.current.targetTargetY = p.targetY;
  }, [preset]);

  // Initialize Three.js renderer + lighting + drag controls once.
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    let renderer: THREE.WebGLRenderer;
    try {
      renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false, preserveDrawingBuffer: true });
    } catch {
      onFailure();
      return;
    }
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    renderer.setClearColor(0x132026, 1);
    host.appendChild(renderer.domElement);

    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera(38, 1, 0.05, 20);

    // Three-point atelier lighting
    scene.add(new THREE.AmbientLight(0xfff6e5, 0.72));
    const key = new THREE.DirectionalLight(0xfff8ec, 1.05);
    key.position.set(1.8, 2.2, 2.5);
    scene.add(key);
    const fill = new THREE.DirectionalLight(0xc2dcd8, 0.55);
    fill.position.set(-2.0, 1.0, -1.6);
    scene.add(fill);
    const rim = new THREE.DirectionalLight(0xf3d1a2, 0.45);
    rim.position.set(0.2, -1.5, -2.0);
    scene.add(rim);

    // Subtle dark mannequin core cylinder strictly inside the lining (zero intersection at any separation)
    const coreGeom = new THREE.CylinderGeometry(0.102, 0.122, 0.62, 36, 1, false);
    coreGeom.scale(1.16, 1, 0.88);
    const coreMat = new THREE.MeshStandardMaterial({
      color: 0x26343a,
      roughness: 0.9,
      metalness: 0.02,
      polygonOffset: true,
      polygonOffsetFactor: 2,
      polygonOffsetUnits: 2,
    });
    const coreMesh = new THREE.Mesh(coreGeom, coreMat);
    coreMesh.position.y = -0.01;
    scene.add(coreMesh);

    const garmentGroup = new THREE.Group();
    const highlightGroup = new THREE.Group();
    garmentGroup.add(highlightGroup);
    scene.add(garmentGroup);

    sceneObjsRef.current = { renderer, scene, camera, garmentGroup, highlightGroup, disposables: [] };

    const resize = () => {
      const w = Math.max(240, host.clientWidth);
      const h = Math.max(320, host.clientHeight);
      renderer.setSize(w, h, false);
      camera.aspect = w / h;
      camera.updateProjectionMatrix();
    };
    resize();
    const ro = new ResizeObserver(resize);
    ro.observe(host);

    // Pointer drag-to-turn (rotates shell, facing, lining & pockets together!)
    let dragging = false;
    let lastX = 0;
    let lastY = 0;
    const onDown = (e: PointerEvent) => {
      dragging = true;
      lastX = e.clientX;
      lastY = e.clientY;
      renderer.domElement.setPointerCapture(e.pointerId);
    };
    const onMove = (e: PointerEvent) => {
      if (!dragging) return;
      const dx = e.clientX - lastX;
      const dy = e.clientY - lastY;
      lastX = e.clientX;
      lastY = e.clientY;
      stateRef.current.targetYaw -= dx * 0.009;
      stateRef.current.targetPitch = Math.max(-0.55, Math.min(0.65, stateRef.current.targetPitch + dy * 0.007));
    };
    const onUp = () => {
      dragging = false;
    };
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      stateRef.current.targetDist = Math.max(0.55, Math.min(1.65, stateRef.current.targetDist + e.deltaY * 0.0008));
    };
    renderer.domElement.addEventListener("pointerdown", onDown);
    renderer.domElement.addEventListener("pointermove", onMove);
    renderer.domElement.addEventListener("pointerup", onUp);
    renderer.domElement.addEventListener("wheel", onWheel, { passive: false });

    let rafId = 0;
    const renderLoop = () => {
      const st = stateRef.current;
      st.yaw += (st.targetYaw - st.yaw) * 0.16;
      st.pitch += (st.targetPitch - st.pitch) * 0.16;
      st.dist += (st.targetDist - st.dist) * 0.16;
      st.targetY += (st.targetTargetY - st.targetY) * 0.16;
      camera.position.set(
        st.dist * Math.sin(st.yaw) * Math.cos(st.pitch),
        st.targetY + st.dist * Math.sin(st.pitch),
        st.dist * Math.cos(st.yaw) * Math.cos(st.pitch),
      );
      camera.lookAt(0, st.targetY, 0);
      renderer.render(scene, camera);
      rafId = requestAnimationFrame(renderLoop);
    };
    rafId = requestAnimationFrame(renderLoop);

    return () => {
      cancelAnimationFrame(rafId);
      ro.disconnect();
      renderer.domElement.removeEventListener("pointerdown", onDown);
      renderer.domElement.removeEventListener("pointermove", onMove);
      renderer.domElement.removeEventListener("pointerup", onUp);
      renderer.domElement.removeEventListener("wheel", onWheel);
      for (const d of sceneObjsRef.current?.disposables ?? []) d.dispose();
      coreGeom.dispose();
      coreMat.dispose();
      renderer.dispose();
      if (renderer.domElement.parentNode === host) host.removeChild(renderer.domElement);
      sceneObjsRef.current = null;
    };
  }, [onFailure]);

  // Rebuild garment panels + highlighted seam/unpaired edges whenever visibility, separation, or selection changes.
  useEffect(() => {
    const objs = sceneObjsRef.current;
    if (!objs) return;
    const { garmentGroup, highlightGroup, disposables } = objs;

    // Clear previous panel & highlight meshes
    for (let i = garmentGroup.children.length - 1; i >= 0; i--) {
      const child = garmentGroup.children[i];
      if (child !== highlightGroup) garmentGroup.remove(child);
    }
    while (highlightGroup.children.length > 0) {
      highlightGroup.remove(highlightGroup.children[0]);
    }
    for (const d of disposables) d.dispose();
    disposables.length = 0;

    // Build visible layered garment panels
    for (const [instanceId, outline] of outlinesRef.current.entries()) {
      const pieceId = instanceId.split(".")[0];
      const layer = layers[pieceId] ?? "shell";
      if (!layerVisible[layer]) continue;

      const offset = radialOffsetMm[layer] ?? 0;
      const geom = buildPanelGeometry(instanceId, outline.polygon, offset);
      const baseStyle = LAYER_STYLES[layer] ?? LAYER_STYLES.shell;
      const isBackShell = pieceId === "wc_back";
      const isSelectedPanel =
        (selectedSeam && (selectedSeam.instanceA === instanceId || selectedSeam.instanceB === instanceId)) ||
        (selectedUnpaired && selectedUnpaired.instance === instanceId);

      const mat = new THREE.MeshStandardMaterial({
        color: isBackShell ? 0xd9cfb8 : baseStyle.color,
        emissive: isSelectedPanel ? 0x2d1a0d : baseStyle.emissive,
        roughness: layer === "facing" ? 0.42 : 0.72,
        metalness: 0.04,
        side: THREE.DoubleSide,
        transparent: baseStyle.opacity < 1,
        opacity: baseStyle.opacity,
      });
      const mesh = new THREE.Mesh(geom, mat);
      garmentGroup.add(mesh);
      disposables.push(geom, mat);

      // Subtle panel outline wire so individual pieces read cleanly
      const wireLine = buildEdgeLineGeometry(instanceId, [...outline.polygon, outline.polygon[0]], offset + 0.6);
      const wireMat = new THREE.LineBasicMaterial({
        color: isSelectedPanel ? 0xf5d283 : 0x2a3b41,
        transparent: true,
        opacity: isSelectedPanel ? 0.9 : 0.42,
      });
      garmentGroup.add(new THREE.Line(wireLine.geom, wireMat));
      disposables.push(wireLine.geom, wireMat);
    }

    // Highlight selected paired construction seam across both joined layers (+ radial correspondence tie-bars!)
    if (selectedSeam) {
      const outA = outlinesRef.current.get(selectedSeam.instanceA);
      const outB = outlinesRef.current.get(selectedSeam.instanceB);
      const edgeA = outA?.edges.find(e => e.id === selectedSeam.edgeA);
      const edgeB = outB?.edges.find(e => e.id === selectedSeam.edgeB);
      if (edgeA && edgeB) {
        const offA = (radialOffsetMm[selectedSeam.layerA] ?? 0) + 1.8;
        const offB = (radialOffsetMm[selectedSeam.layerB] ?? 0) + 1.8;
        const lineA = buildEdgeLineGeometry(selectedSeam.instanceA, edgeA.pts, offA);
        const lineB = buildEdgeLineGeometry(selectedSeam.instanceB, edgeB.pts, offB);
        const matA = new THREE.LineBasicMaterial({ color: 0xff5a36, depthTest: false });
        const matB = new THREE.LineBasicMaterial({ color: 0x2ee0b8, depthTest: false });
        const meshA = new THREE.Line(lineA.geom, matA);
        const meshB = new THREE.Line(lineB.geom, matB);
        meshA.renderOrder = 10;
        meshB.renderOrder = 10;
        highlightGroup.add(meshA, meshB);
        disposables.push(lineA.geom, lineB.geom, matA, matB);

        // Draw correspondence stitch tie-bars connecting paired points across separated layers
        const tiePts: THREE.Vector3[] = [];
        const samples = 7;
        for (let i = 0; i < samples; i++) {
          const frac = i / (samples - 1);
          const idxA = Math.min(lineA.pts3.length - 1, Math.round(frac * (lineA.pts3.length - 1)));
          const idxB = Math.min(lineB.pts3.length - 1, Math.round((1 - frac) * (lineB.pts3.length - 1)));
          // Pick closest pairing end so tie-bars never cross when one side is mirrored
          const pa = lineA.pts3[idxA];
          const pbSame = lineB.pts3[Math.min(lineB.pts3.length - 1, Math.round(frac * (lineB.pts3.length - 1)))];
          const pbRev = lineB.pts3[idxB];
          const pb = pa.distanceTo(pbSame) <= pa.distanceTo(pbRev) ? pbSame : pbRev;
          tiePts.push(pa, pb);
        }
        const tieGeom = new THREE.BufferGeometry().setFromPoints(tiePts);
        const tieMat = new THREE.LineBasicMaterial({ color: 0xf6c762, transparent: true, opacity: 0.9, depthTest: false });
        const tieMesh = new THREE.LineSegments(tieGeom, tieMat);
        tieMesh.renderOrder = 9;
        highlightGroup.add(tieMesh);
        disposables.push(tieGeom, tieMat);
      }
    }

    // Highlight selected unpaired / missing-lining edge in amber warning stroke
    if (selectedUnpaired) {
      const outU = outlinesRef.current.get(selectedUnpaired.instance);
      const edgeU = outU?.edges.find(e => e.id === selectedUnpaired.edge);
      if (edgeU) {
        const offU = (radialOffsetMm[selectedUnpaired.layer] ?? 0) + 2.2;
        const lineU = buildEdgeLineGeometry(selectedUnpaired.instance, edgeU.pts, offU);
        const matU = new THREE.LineBasicMaterial({ color: 0xf7b239, depthTest: false });
        const meshU = new THREE.Line(lineU.geom, matU);
        meshU.renderOrder = 11;
        highlightGroup.add(meshU);
        disposables.push(lineU.geom, matU);
      }
    }
  }, [layers, layerVisible, radialOffsetMm, selectedSeam, selectedUnpaired]);

  return (
    <div
      ref={hostRef}
      className="io-canvas-host"
      role="img"
      aria-label="Interactive 3D layered view of the 1780s V&A 256-1880 waistcoat showing shell, front facing, linen lining, and pocket bags rotating together"
    />
  );
}
