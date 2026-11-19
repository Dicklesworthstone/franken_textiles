import * as THREE from "three";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";

// One lighting set-up for every figure on the site: soft light from all round, a warm key light
// that casts the fold shadows, and a cool rim light that separates the figure from the backdrop.

export function prepareRenderer(renderer: THREE.WebGLRenderer) {
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.02;
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFShadowMap;   // hardware-filtered; softness comes from shadow.radius
}

export interface StudioLight {
  key: THREE.DirectionalLight;
  /** Aim the key light's shadow at a subject of this centre and radius (metres). */
  frame(center: THREE.Vector3, radius: number): void;
  dispose(): void;
}

export function lightStudio(renderer: THREE.WebGLRenderer, scene: THREE.Scene, tone: "warm" | "cool" = "warm"): StudioLight {
  const pmrem = new THREE.PMREMGenerator(renderer);
  const room = new RoomEnvironment();
  const environment = pmrem.fromScene(room, 0.04).texture;
  room.dispose();
  pmrem.dispose();
  scene.environment = environment;
  scene.environmentIntensity = 0.4;
  const key = new THREE.DirectionalLight(tone === "warm" ? 0xfff0dc : 0xf2f6ff, 3.0);
  key.castShadow = true;
  key.shadow.mapSize.set(2048, 2048);
  key.shadow.radius = 4;
  key.shadow.bias = -0.00012;
  key.shadow.normalBias = 0.0022;
  const rim = new THREE.DirectionalLight(tone === "warm" ? 0xdfeeff : 0xfff1dd, 0.9);
  const fill = new THREE.DirectionalLight(0xffffff, 0.22);
  scene.add(key, key.target, rim, fill);
  // High and well to one side, so that folds throw shadows across the cloth.
  const direction = new THREE.Vector3(-0.78, 0.98, 0.5).normalize();
  function frame(center: THREE.Vector3, radius: number) {
    key.target.position.copy(center);
    key.position.copy(center).addScaledVector(direction, radius * 3);
    rim.position.copy(center).add(new THREE.Vector3(0.9, 0.7, -1.2).multiplyScalar(radius * 2));
    fill.position.copy(center).add(new THREE.Vector3(0.8, 0.1, 1.1).multiplyScalar(radius * 2));
    const camera = key.shadow.camera;
    camera.left = camera.bottom = -radius * 1.08;
    camera.right = camera.top = radius * 1.08;
    camera.near = radius * 0.5;
    camera.far = radius * 5.5;
    camera.updateProjectionMatrix();
    key.shadow.needsUpdate = true;
  }
  frame(new THREE.Vector3(0, 0, 0), 0.6);
  return {
    key, frame,
    dispose() {
      scene.remove(key, key.target, rim, fill);
      scene.environment = null;
      environment.dispose();
      key.shadow.dispose();
    },
  };
}

/**
 * The body is shown as a tailor's form: the fitted body's own shape in a plain matt finish.
 * It carries no skin, face or hair, because none of those come from the body model.
 */
export function formMaterial(colour = "#c4b6a2") {
  return new THREE.MeshPhysicalMaterial({
    color: colour, roughness: 0.82, metalness: 0,
    sheen: 0.22, sheenRoughness: 0.55, sheenColor: "#f1e3cf", envMapIntensity: 0.8,
  });
}

/** A floor that shows only the figure's shadow, so the figure stands on the page's own backdrop. */
export function shadowFloor(y: number, opacity = 0.2) {
  const material = new THREE.ShadowMaterial({ opacity });
  const mesh = new THREE.Mesh(new THREE.CircleGeometry(1.6, 48), material);
  mesh.rotation.x = -Math.PI / 2;
  mesh.position.y = y;
  mesh.receiveShadow = true;
  return { mesh, dispose() { mesh.geometry.dispose(); material.dispose(); } };
}
