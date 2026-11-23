"use client";

import { useEffect, useRef, useState } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { fullControls } from "../atelier3d/viewControls";
import { publishedGeometry } from "../replayAsset";
import { COMMON_WORLD_SCALE_M_PER_UNIT, parsePackedDrapeBuffer } from "../../public/labs/sundress-compare.mjs";

export type CompareCameraPreset = "Full length" | "Hem elevation" | "Waist profile" | "Back";

// One camera for both viewports. Whoever changes it raises `stamp` and signs `from`: a viewport signs with
// its own token and the page with nothing, so each viewport applies every change but its own.
export type SharedCameraState = {
  yaw: number;
  pitch: number;
  dist: number;
  targetY: number;
  /** Where the visitor has moved the view sideways (m); zero in every preset. */
  x?: number;
  z?: number;
  stamp?: number;
  from?: unknown;
};

export const COMPARE_CAMERAS: Record<CompareCameraPreset, SharedCameraState> = {
  "Full length": { yaw: -0.26, pitch: 0.035, dist: 3.25, targetY: 0.84 },
  "Hem elevation": { yaw: -0.18, pitch: 0.02, dist: 1.38, targetY: 0.48 },
  "Waist profile": { yaw: -Math.PI / 2, pitch: 0.04, dist: 1.24, targetY: 0.96 },
  Back: { yaw: Math.PI - 0.26, pitch: 0.035, dist: 3.25, targetY: 0.84 },
};

type ParsedMesh = ReturnType<typeof parsePackedDrapeBuffer>;

// Module-level cache so swapping sides or showing the same state on both sides is instant & zero-refetch!
const packedCache = new Map<string, Promise<ParsedMesh>>();

async function gunzipBytes(buf: ArrayBuffer): Promise<ArrayBuffer> {
  if (typeof DecompressionStream !== "undefined") {
    const ds = new DecompressionStream("gzip");
    const stream = new Blob([buf]).stream().pipeThrough(ds);
    return new Response(stream).arrayBuffer();
  }
  throw new Error("Browser lacks DecompressionStream for gzip drape geometry.");
}

function loadPackedDrape(sourceBase: string, retry: boolean): Promise<ParsedMesh> {
  let pending = packedCache.get(sourceBase);
  if (!pending) {
    pending = (async () => {
      // The HTTP cache can retain a response even when the integrity check rejects it.
      const cache = retry ? "reload" : undefined;
      const [metaRes, gzRes] = await Promise.all([
        publishedGeometry(`${sourceBase}.json`, undefined, cache),
        publishedGeometry(`${sourceBase}.gz`, undefined, cache),
      ]).catch(() => {
        throw new Error("Could not fetch native drape. A replay file is unavailable or differs from the published copy.");
      });
      const [meta, gzBuf] = await Promise.all([metaRes.json(), gzRes.arrayBuffer()]);
      const raw = await gunzipBytes(gzBuf);
      return parsePackedDrapeBuffer(meta, raw);
    })().catch(error => {
      packedCache.delete(sourceBase);
      throw error;
    });
    packedCache.set(sourceBase, pending);
  }
  return pending;
}

function buildElevationRing(yMeters: number, colorHex: number, radius = 0.36, opacity = 0.85) {
  const pts: THREE.Vector3[] = [];
  const segs = 48;
  for (let i = 0; i <= segs; i++) {
    const a = (i / segs) * Math.PI * 2;
    pts.push(new THREE.Vector3(Math.cos(a) * radius, yMeters, Math.sin(a) * radius));
  }
  const geom = new THREE.BufferGeometry().setFromPoints(pts);
  const mat = new THREE.LineBasicMaterial({ color: colorHex, transparent: true, opacity });
  return { line: new THREE.Line(geom, mat), geom, mat };
}

export default function SundressCompareScene({
  sourceBase,
  ownHemMm,
  otherHemMm,
  accentHex,
  cameraRef,
  onCameraHome,
  label,
}: {
  sourceBase: string;
  ownHemMm: number;
  otherHemMm: number | null;
  accentHex: string;
  cameraRef: React.MutableRefObject<SharedCameraState>;
  /** The visitor asked for the opening view: the page applies its chosen preset to the shared camera. */
  onCameraHome: () => void;
  label: string;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  // Kept in a ref so that a new callback from the page does not rebuild the WebGL scene.
  const homeRef = useRef(onCameraHome);
  useEffect(() => { homeRef.current = onCameraHome; }, [onCameraHome]);
  const ringGroupRef = useRef<{ group: THREE.Group; disposables: { dispose: () => void }[] } | null>(null);
  const [loadErr, setLoadErr] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    let alive = true;
    setLoaded(false);
    setLoadErr("");

    let renderer: THREE.WebGLRenderer;
    try {
      renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false, preserveDrawingBuffer: true });
    } catch {
      setLoadErr("WebGL 3D context unavailable.");
      return;
    }
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    renderer.setClearColor(0x142127, 1);
    host.appendChild(renderer.domElement);

    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera(34, 1, 0.05, 20);

    // Three-point studio light identical on both viewports
    scene.add(new THREE.AmbientLight(0xfff6e8, 0.72));
    const key = new THREE.DirectionalLight(0xfffaf0, 1.02);
    key.position.set(1.8, 2.5, 2.4);
    scene.add(key);
    const fill = new THREE.DirectionalLight(0xc2dcd7, 0.56);
    fill.position.set(-2.0, 1.2, -1.6);
    scene.add(fill);
    const rim = new THREE.DirectionalLight(0xf3cba0, 0.4);
    rim.position.set(0.3, 0.8, -2.4);
    scene.add(rim);

    // Common world floor grid at exact Y = 0.000 m
    const floor = new THREE.GridHelper(1.2, 12, 0x455a60, 0x26383e);
    floor.position.y = 0;
    scene.add(floor);

    const ringGroup = new THREE.Group();
    scene.add(ringGroup);
    ringGroupRef.current = { group: ringGroup, disposables: [] };

    const disposables: { dispose: () => void }[] = [];

    let rafId = 0;
    // Load native packed drape at exact COMMON_WORLD_SCALE_M_PER_UNIT (1.0)
    loadPackedDrape(sourceBase, attempt > 0)
      .then(parsed => {
        if (!alive) return;
        const bodyGeom = new THREE.BufferGeometry();
        bodyGeom.setAttribute("position", new THREE.BufferAttribute(parsed.bodyPos, 3));
        bodyGeom.setIndex(new THREE.BufferAttribute(parsed.bodyIndices, 1));
        bodyGeom.computeVertexNormals();
        const bodyMat = new THREE.MeshStandardMaterial({
          color: 0xc8ae93,
          roughness: 0.78,
          metalness: 0.02,
          polygonOffset: true,
          polygonOffsetFactor: 1.5,
          polygonOffsetUnits: 1.5,
        });
        const bodyMesh = new THREE.Mesh(bodyGeom, bodyMat);
        bodyMesh.scale.setScalar(COMMON_WORLD_SCALE_M_PER_UNIT);
        scene.add(bodyMesh);

        const clothGeom = new THREE.BufferGeometry();
        clothGeom.setAttribute("position", new THREE.BufferAttribute(parsed.clothPos, 3));
        clothGeom.setIndex(new THREE.BufferAttribute(parsed.clothIndices, 1));
        clothGeom.computeVertexNormals();
        const clothMat = new THREE.MeshStandardMaterial({
          color: new THREE.Color(accentHex),
          roughness: 0.68,
          metalness: 0.03,
          side: THREE.DoubleSide,
        });
        const clothMesh = new THREE.Mesh(clothGeom, clothMat);
        clothMesh.scale.setScalar(COMMON_WORLD_SCALE_M_PER_UNIT);
        scene.add(clothMesh);

        disposables.push(bodyGeom, bodyMat, clothGeom, clothMat);
        setLoaded(true);
      })
      .catch(err => {
        if (!alive) return;
        cancelAnimationFrame(rafId);
        setLoadErr(err instanceof Error ? err.message : String(err));
      });

    const resize = () => {
      const w = Math.max(220, host.clientWidth);
      const h = Math.max(340, host.clientHeight);
      renderer.setSize(w, h, false);
      camera.aspect = w / h;
      camera.updateProjectionMatrix();
    };
    resize();
    const ro = new ResizeObserver(resize);
    ro.observe(host);

    // The site's shared camera on each viewport. Turning, moving or zooming either one writes the single
    // shared state, and the other viewport follows it on its next frame.
    const canvas = renderer.domElement;
    const orbit = new OrbitControls(camera, canvas);
    orbit.minDistance = 0.6;
    orbit.maxDistance = 4.2;
    orbit.minPolarAngle = 0.3;
    orbit.maxPolarAngle = 2.0;
    const token = {};
    const offset = new THREE.Vector3();
    let seen = -1;
    let applying = false;
    const look = fullControls(orbit, canvas, { dark: true, turnKeys: true, home: () => homeRef.current() });
    const apply = (st: SharedCameraState) => {
      applying = true;
      orbit.target.set(st.x ?? 0, st.targetY, st.z ?? 0);
      camera.position.set(
        st.dist * Math.sin(st.yaw) * Math.cos(st.pitch),
        st.dist * Math.sin(st.pitch),
        st.dist * Math.cos(st.yaw) * Math.cos(st.pitch),
      ).add(orbit.target);
      // A preset is where "moved from" is measured from, in both viewports alike.
      if (st.from == null) look.retarget(orbit.target);
      orbit.update();
      applying = false;
    };
    const publish = () => {
      if (applying) return;
      const st = cameraRef.current;
      offset.copy(camera.position).sub(orbit.target);
      st.dist = offset.length();
      st.pitch = Math.asin(THREE.MathUtils.clamp(offset.y / st.dist, -1, 1));
      st.yaw = Math.atan2(offset.x, offset.z);
      st.targetY = orbit.target.y;
      st.x = orbit.target.x;
      st.z = orbit.target.z;
      st.stamp = (st.stamp ?? 0) + 1;
      st.from = token;
      seen = st.stamp;
    };
    // Added after the shared controls, so a view carried too far is brought back before it is published.
    orbit.addEventListener("change", publish);

    const loop = () => {
      const st = cameraRef.current;
      const stamp = st.stamp ?? 0;
      if (stamp !== seen) {
        seen = stamp;
        if (st.from !== token) apply(st);
      }
      renderer.render(scene, camera);
      rafId = requestAnimationFrame(loop);
    };
    rafId = requestAnimationFrame(loop);

    return () => {
      alive = false;
      cancelAnimationFrame(rafId);
      ro.disconnect();
      look.dispose();
      orbit.removeEventListener("change", publish);
      orbit.dispose();
      for (const d of ringGroupRef.current?.disposables ?? []) d.dispose();
      ringGroupRef.current = null;
      for (const d of disposables) d.dispose();
      floor.dispose();
      renderer.dispose();
      renderer.forceContextLoss();
      if (renderer.domElement.parentNode === host) host.removeChild(renderer.domElement);
    };
  }, [sourceBase, accentHex, cameraRef, attempt]);

  // Update lightweight elevation reference rings whenever partner hem changes without tearing down WebGL!
  useEffect(() => {
    const holder = ringGroupRef.current;
    if (!holder) return;
    while (holder.group.children.length > 0) holder.group.remove(holder.group.children[0]);
    for (const d of holder.disposables) d.dispose();
    holder.disposables.length = 0;

    const add = (yM: number, hex: number, radius: number, opacity: number) => {
      const built = buildElevationRing(yM, hex, radius, opacity);
      holder.group.add(built.line);
      holder.disposables.push(built.geom, built.mat);
    };
    add(0.5, 0x678085, 0.42, 0.45);
    add(ownHemMm / 1000, new THREE.Color(accentHex).getHex(), 0.36, 0.95);
    if (otherHemMm !== null && Math.abs(otherHemMm - ownHemMm) >= 1) {
      add(otherHemMm / 1000, 0xe1b55f, 0.39, 0.72);
    }
  }, [sourceBase, ownHemMm, otherHemMm, accentHex, attempt]);

  return (
    <>
      <div
        ref={hostRef}
        className="sc-canvas-host"
        hidden={Boolean(loadErr)}
        data-drape-loaded={loaded ? "true" : "false"}
        data-world-scale={COMMON_WORLD_SCALE_M_PER_UNIT}
        role="img"
        aria-label={label}
      />
      {loadErr && (
        <div className="sc-fallback">
          <p role="alert">{loadErr}</p>
          <button type="button" className="sc-dl-sec" onClick={() => setAttempt(n => n + 1)}>Retry 3D view</button>
        </div>
      )}
    </>
  );
}
