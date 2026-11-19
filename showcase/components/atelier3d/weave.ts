import * as THREE from "three";

// Procedural woven cloth: one tile of interlaced yarns, as a colour map and a matching normal map.
// The tile is laid out in flat-pattern coordinates, so warp and weft follow each piece's grain.

export type WeaveKind = "twill" | "plain";

const THREADS = 12;   // divisible by the plain (2) and twill (3) repeats
const SIZE = 384;     // 32 texels per yarn

/** Height of the cloth surface in a tile, 0..1. Warp runs along v, weft along u. */
function height(kind: WeaveKind, x: number, y: number) {
  const cell = SIZE / THREADS;
  const i = Math.floor(x / cell), j = Math.floor(y / cell);
  const fu = x / cell - i - 0.5, fv = y / cell - j - 0.5;
  // Plain weave alternates; a 2/1 twill floats the warp over two wefts, stepping one yarn per row.
  const warpOver = kind === "plain" ? (i + j) % 2 === 0 : ((i - j) % 3 + 3) % 3 !== 0;
  const across = warpOver ? fu : fv, along = warpOver ? fv : fu;
  const profile = Math.sqrt(Math.max(0, 1 - (across / 0.47) ** 2));
  const crown = 0.74 + 0.26 * Math.cos(along * Math.PI);
  return profile * crown;
}

// A stable per-yarn tone, so neighbouring yarns differ slightly the way spun yarn does.
function yarnTone(n: number) {
  const s = Math.sin(n * 127.1 + 311.7) * 43758.5453;
  return (s - Math.floor(s)) - 0.5;
}

export function weaveMaps(kind: WeaveKind, anisotropy = 8) {
  const heights = new Float32Array(SIZE * SIZE);
  for (let y = 0; y < SIZE; y++) for (let x = 0; x < SIZE; x++) heights[y * SIZE + x] = height(kind, x + 0.5, y + 0.5);
  const colour = new Uint8Array(SIZE * SIZE * 4), normal = new Uint8Array(SIZE * SIZE * 4);
  const cell = SIZE / THREADS;
  const at = (x: number, y: number) => heights[((y + SIZE) % SIZE) * SIZE + (x + SIZE) % SIZE];
  for (let y = 0; y < SIZE; y++) for (let x = 0; x < SIZE; x++) {
    const k = 4 * (y * SIZE + x), h = heights[y * SIZE + x];
    const i = Math.floor(x / cell), j = Math.floor(y / cell);
    const warpOver = kind === "plain" ? (i + j) % 2 === 0 : ((i - j) % 3 + 3) % 3 !== 0;
    // Shade the valleys between yarns and vary each yarn a little.
    const tone = (0.80 + 0.20 * h) * (1 + 0.07 * yarnTone(warpOver ? i : 100 + j));
    const value = Math.max(0, Math.min(255, Math.round(255 * tone)));
    colour.set([value, value, value, 255], k);
    const dx = (at(x + 1, y) - at(x - 1, y)) * 2.2, dy = (at(x, y + 1) - at(x, y - 1)) * 2.2;
    const inv = 1 / Math.hypot(dx, dy, 1);
    normal.set([Math.round(127.5 * (1 - dx * inv)), Math.round(127.5 * (1 - dy * inv)), Math.round(127.5 * (1 + inv)), 255], k);
  }
  function texture(data: Uint8Array, colorSpace: THREE.ColorSpace) {
    const map = new THREE.DataTexture(data, SIZE, SIZE, THREE.RGBAFormat);
    map.colorSpace = colorSpace;
    map.wrapS = map.wrapT = THREE.RepeatWrapping;
    map.generateMipmaps = true;
    map.minFilter = THREE.LinearMipmapLinearFilter;
    map.magFilter = THREE.LinearFilter;
    map.anisotropy = anisotropy;
    map.needsUpdate = true;
    return map;
  }
  return { colour: texture(colour, THREE.SRGBColorSpace), normal: texture(normal, THREE.NoColorSpace) };
}

/** Texture repeats per metre of cloth for a given yarn pitch. */
export const weaveRepeat = (yarnPitchMm: number) => 1000 / (THREADS * yarnPitchMm);

export interface Fabric {
  /** Face colour of the cloth. */
  colour: string;
  /** Colour of the reverse side. */
  lining: string;
  /** Tint of the soft highlight that fibres give at grazing angles. */
  sheen: string;
  weave: WeaveKind;
  /** Centre-to-centre yarn spacing. Drawn larger than life so the weave reads at figure scale. */
  yarnPitchMm: number;
  /** Displayed cloth thickness in metres. */
  thickness: number;
}

export const fabrics = {
  greenWool: { colour: "#124038", lining: "#0f2a27", sheen: "#4f8f7c", weave: "twill", yarnPitchMm: 1.5, thickness: 0.002 },
  rustTwill: { colour: "#8a4a26", lining: "#55301c", sheen: "#c98554", weave: "twill", yarnPitchMm: 1.3, thickness: 0.002 },
  navyTwill: { colour: "#1f3156", lining: "#16223c", sheen: "#6f81b3", weave: "twill", yarnPitchMm: 1.3, thickness: 0.002 },
  madderWool: { colour: "#b9441f", lining: "#672a18", sheen: "#e39368", weave: "twill", yarnPitchMm: 1.5, thickness: 0.002 },
} satisfies Record<string, Fabric>;

/**
 * A woven-cloth material. `inject` lets a scene add its own colour logic (a braid, a strain map)
 * after the cloth's own mottling, in the same shader.
 */
export interface ClothShaderHook { vertexHead?: string; vertexBody?: string; fragmentHead?: string; colour?: string; key?: string }

export function clothMaterial(fabric: Fabric, reverse = false, inject?: ClothShaderHook) {
  const maps = weaveMaps(fabric.weave);
  const repeat = weaveRepeat(fabric.yarnPitchMm);
  maps.colour.repeat.set(repeat, repeat);
  maps.normal.repeat.set(repeat, repeat);
  const material = new THREE.MeshPhysicalMaterial({
    color: reverse ? fabric.lining : fabric.colour,
    map: maps.colour, normalMap: maps.normal, normalScale: new THREE.Vector2(0.55, 0.55),
    roughness: 0.92, metalness: 0,
    sheen: reverse ? 0.2 : 0.55, sheenRoughness: 0.5, sheenColor: fabric.sheen,
    side: THREE.FrontSide, envMapIntensity: 0.75,
  });
  material.onBeforeCompile = shader => {
    shader.vertexShader = "varying vec2 clothMetres;\n" + (inject?.vertexHead ?? "") + shader.vertexShader.replace(
      "#include <begin_vertex>", "#include <begin_vertex>\nclothMetres = uv;\n" + (inject?.vertexBody ?? ""));
    // Two octaves of value noise at a few centimetres: uneven dye take-up and nap, not a flat fill.
    shader.fragmentShader = `varying vec2 clothMetres;
${inject?.fragmentHead ?? ""}
float clothHash(vec2 p){ return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
float clothNoise(vec2 p){
  vec2 i = floor(p), f = fract(p); f = f * f * (3. - 2. * f);
  return mix(mix(clothHash(i), clothHash(i + vec2(1., 0.)), f.x), mix(clothHash(i + vec2(0., 1.)), clothHash(i + vec2(1., 1.)), f.x), f.y);
}
` + shader.fragmentShader.replace("#include <color_fragment>", `#include <color_fragment>
  float mottle = clothNoise(clothMetres * 38.) * .62 + clothNoise(clothMetres * 131.) * .38;
  diffuseColor.rgb *= .93 + .14 * mottle;
  ${inject?.colour ?? ""}`);
  };
  material.customProgramCacheKey = () => "atelier-cloth-" + (inject?.key ?? "plain");
  return { material, dispose() { material.dispose(); maps.colour.dispose(); maps.normal.dispose(); } };
}
