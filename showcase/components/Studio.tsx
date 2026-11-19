"use client";

import { useEffect, useRef } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { type Garment, type StudioMode, type PanelOutline, loadGarment } from "./garmentMesh";

type StudioProps = {
  garment: Garment;
  mode?: StudioMode;
  frame?: number;
  wire?: boolean;
  seam?: string;
  spread?: number;
  yaw?: number;
  resetKey?: number;
  onReady?: () => void;
  onError?: () => void;
  onSeams?: (names: string[]) => void;
  panel?: string;
  onPanels?: (panels: PanelOutline[]) => void;
  onStrain?: (strain: { min: number; max: number; p95: number }) => void;
};

const homeYaw = 0.32;
const homeElevation = 0.15;

export default function Studio({ garment, mode = "cloth", frame = 1, wire = false, seam, spread = 0, yaw,
  resetKey = 0, onReady, onError, onSeams, onStrain, panel, onPanels }: StudioProps) {
  const host = useRef<HTMLDivElement>(null);
  const state = useRef({ mode, frame, wire, seam, spread, yaw, resetKey, onReady, onError, onSeams, onStrain, panel, onPanels });
  const redraw = useRef<(() => void) | null>(null);

  useEffect(() => {
    state.current = { mode, frame, wire, seam, spread, yaw, resetKey, onReady, onError, onSeams, onStrain, panel, onPanels };
    redraw.current?.();
  }, [mode, frame, wire, seam, spread, yaw, resetKey, onReady, onError, onSeams, onStrain, panel, onPanels]);

  useEffect(() => {
    const surface = host.current;
    if (!surface) return;
    let renderer: THREE.WebGLRenderer;
    try {
      renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true, powerPreference: "low-power" });
    } catch {
      state.current.onError?.();
      return;
    }
    const canvas = renderer.domElement;
    canvas.style.cssText = "width:100%;height:100%;display:block;cursor:grab";
    canvas.tabIndex = 0;
    canvas.setAttribute("role", "img");
    canvas.setAttribute("aria-label", `Interactive ${garment} garment. Drag or use arrow keys to orbit. Pinch or use plus and minus to zoom. Home resets the view.`);
    surface.appendChild(canvas);
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    renderer.setClearColor(0, 0);
    renderer.outputColorSpace = THREE.SRGBColorSpace;
    renderer.toneMapping = THREE.ACESFilmicToneMapping;
    renderer.toneMappingExposure = 1.08;
    renderer.shadowMap.enabled = true;
    renderer.shadowMap.type = THREE.PCFShadowMap;
    const scene = new THREE.Scene();
    scene.add(new THREE.HemisphereLight(0xfff2d8, 0x544650, 1.35));
    const key = new THREE.DirectionalLight(0xffe1c8, 2.65);
    key.position.set(-3, 4, 3);
    key.castShadow = true;
    key.shadow.mapSize.set(1024, 1024);
    Object.assign(key.shadow.camera, { left: -2.2, right: 2.2, top: 2.2, bottom: -2.2, near: 0.1, far: 12 });
    key.shadow.camera.updateProjectionMatrix();
    key.shadow.bias = -0.0003;
    key.shadow.normalBias = 0.002;
    const rim = new THREE.DirectionalLight(0xffbc87, 2.0);
    rim.position.set(2, 1, -2);
    const fill = new THREE.DirectionalLight(0xc4d0f9, 0.6);
    fill.position.set(3, 0.5, 2);
    scene.add(key, rim, fill);
    const camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.01, 40);
    const orbit = new OrbitControls(camera, canvas);
    orbit.minZoom = 0.4;
    orbit.maxZoom = 4;
    orbit.rotateSpeed = 0.8;
    orbit.zoomSpeed = 0.9;
    orbit.enablePan = false;
    orbit.minPolarAngle = 0.03;
    orbit.maxPolarAngle = Math.PI - 0.03;
    const controller = new AbortController();
    let garmentMesh: Awaited<ReturnType<typeof loadGarment>> | undefined;
    let ready = false;
    let width = 0;
    let height = 0;
    let lastYaw: number | undefined;
    let lastReset = state.current.resetKey;
    const radius = 5;
    function orient(azimuth: number, elevation: number) {
      camera.position.set(radius * Math.cos(elevation) * Math.sin(azimuth), radius * Math.sin(elevation),
        radius * Math.cos(elevation) * Math.cos(azimuth));
      orbit.update();
    }
    function reset() {
      camera.zoom = 1;
      camera.updateProjectionMatrix();
      orbit.target.set(0, 0, 0);
      orient(state.current.yaw ?? homeYaw, homeElevation);
    }
    reset();
    function paint() {
      if (!garmentMesh || !width || !height || controller.signal.aborted) return;
      renderer.render(scene, camera);
      canvas.dataset.frame = String(state.current.frame);
      canvas.dataset.yaw = String(state.current.yaw ?? orbit.getAzimuthalAngle());
      canvas.dataset.ready = "true";
      if (!ready) {
        ready = true;
        state.current.onReady?.();
      }
    }
    function update() {
      if (!garmentMesh || !width || !height) return;
      const view = state.current;
      garmentMesh.pose(view.mode, view.frame, view.wire, width / height, view.seam, view.spread, view.panel);
      if (lastReset !== view.resetKey) {
        lastReset = view.resetKey;
        reset();
      }
      if (lastYaw !== view.yaw) {
        lastYaw = view.yaw;
        if (view.yaw !== undefined) orient(view.yaw, homeElevation);
      }
      paint();
    }
    const resize = () => {
      width = surface.clientWidth;
      height = surface.clientHeight;
      if (!width || !height) return;
      renderer.setSize(width, height, false);
      camera.left = -width / height;
      camera.right = width / height;
      camera.updateProjectionMatrix();
      update();
    };
    const observer = new ResizeObserver(resize);
    observer.observe(surface);
    resize();
    const keydown = (event: KeyboardEvent) => {
      if (event.altKey || event.ctrlKey || event.metaKey) return;
      const a = orbit.getAzimuthalAngle();
      const e = Math.PI / 2 - orbit.getPolarAngle();
      const step = Math.PI / 18;
      switch (event.key) {
        case "ArrowLeft": orient(a - step, e); break;
        case "ArrowRight": orient(a + step, e); break;
        case "ArrowUp": orient(a, Math.min(1.5, e + step)); break;
        case "ArrowDown": orient(a, Math.max(-1.5, e - step)); break;
        case "+": case "=": camera.zoom = Math.min(orbit.maxZoom, camera.zoom * 1.16); break;
        case "-": camera.zoom = Math.max(orbit.minZoom, camera.zoom / 1.16); break;
        case "Home": reset(); break;
        default: return;
      }
      event.preventDefault();
      camera.updateProjectionMatrix();
      paint();
    };
    const down = () => { canvas.style.cursor = "grabbing"; };
    const up = () => { canvas.style.cursor = "grab"; };
    const lost = (event: Event) => {
      event.preventDefault();
      canvas.dataset.ready = "false";
      state.current.onError?.();
    };
    orbit.addEventListener("change", paint);
    orbit.addEventListener("start", down);
    orbit.addEventListener("end", up);
    canvas.addEventListener("keydown", keydown);
    canvas.addEventListener("dblclick", reset);
    canvas.addEventListener("webglcontextlost", lost);
    redraw.current = update;
    loadGarment(garment, controller.signal).then((result) => {
      if (controller.signal.aborted) { result.dispose(); return; }
      garmentMesh = result;
      scene.add(result.group);
      state.current.onSeams?.(result.seamNames);
      state.current.onPanels?.(result.panels);
      state.current.onStrain?.(result.strainRange);
      update();
    }).catch(() => {
      if (!controller.signal.aborted) state.current.onError?.();
    });
    return () => {
      controller.abort();
      redraw.current = null;
      observer.disconnect();
      canvas.removeEventListener("keydown", keydown);
      canvas.removeEventListener("dblclick", reset);
      canvas.removeEventListener("webglcontextlost", lost);
      orbit.removeEventListener("change", paint);
      orbit.removeEventListener("start", down);
      orbit.removeEventListener("end", up);
      orbit.dispose();
      garmentMesh?.dispose();
      key.shadow.dispose();
      renderer.dispose();
      renderer.forceContextLoss();
      canvas.remove();
    };
  }, [garment]);

  return <div ref={host} className="garment-studio" style={{ width: "100%", height: "100%", isolation: "isolate" }} />;
}
