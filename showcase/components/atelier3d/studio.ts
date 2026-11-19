import * as THREE from "three";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";
import { weaveMaps } from "./weave";

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
 * The body is shown as a tailor's form: the fitted body's own shape, covered in unbleached linen as a
 * dressmaker's dummy is. It carries no skin, face or hair, because none of those come from the body model.
 *
 * `scale` is how many of the scene's units make a metre (1 for metres, 100 for a scene in centimetres), which
 * sets the size of the linen's weave. `inset` draws the form that far inside its own surface (in scene units),
 * for a scene whose flat cloth facets would otherwise let a curved limb show through.
 */
export function formMaterial(colour = "#bfb09a", { scale = 1, inset = 0 }: { scale?: number; inset?: number } = {}) {
  const maps = weaveMaps("plain"), linen = maps.colour;
  maps.normal.dispose();
  const material = new THREE.MeshPhysicalMaterial({
    color: colour, roughness: 1, metalness: 0, specularIntensity: 0.12,
    sheen: 0.35, sheenRoughness: 0.9, sheenColor: "#f4ead8", envMapIntensity: 0.85,
  });
  material.onBeforeCompile = shader => {
    shader.uniforms.linen = { value: linen };
    shader.vertexShader = "varying vec3 formAt; varying vec3 formFacing;\n" + shader.vertexShader.replace("#include <begin_vertex>",
      `#include <begin_vertex>\nformAt = position; formFacing = normal;${inset ? `\ntransformed -= objectNormal * ${inset.toFixed(5)};` : ""}`);
    // The weave is laid on from three sides and blended by which way the surface faces: a body has no pattern
    // coordinates of its own. About 1.6 mm from yarn to yarn, a coarse tailor's linen.
    shader.fragmentShader = "varying vec3 formAt; varying vec3 formFacing; uniform sampler2D linen;\n" + shader.fragmentShader.replace("#include <color_fragment>", `#include <color_fragment>
  vec3 facing = pow(abs(normalize(formFacing)), vec3(4.)); facing /= facing.x + facing.y + facing.z;
  vec3 at = formAt * ${(52 / scale).toFixed(4)};
  float weave = dot(facing, vec3(texture2D(linen, at.yz).r, texture2D(linen, at.xz).r, texture2D(linen, at.xy).r));
  diffuseColor.rgb *= .74 + .34 * weave;`);
  };
  material.customProgramCacheKey = () => "atelier-form-" + scale + "-" + inset;
  material.addEventListener("dispose", () => linen.dispose());
  return material;
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
