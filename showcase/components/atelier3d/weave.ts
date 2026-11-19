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
  /** How strongly the fibres return soft light at grazing angles: about 0.3 for a smooth cotton, 0.9 for a napped wool. */
  nap?: number;
  /** Wavelength in millimetres of the fine wrinkles drawn where the cloth is compressed. A stiffer cloth wrinkles wider. */
  wrinkleMm?: number;
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
export interface ClothShaderHook {
  vertexHead?: string; vertexBody?: string; fragmentHead?: string; colour?: string; key?: string;
  /** Uniforms the hook's code declares; the same objects stay live, so a scene can change their values. */
  uniforms?: Record<string, { value: unknown }>;
}

export function clothMaterial(fabric: Fabric, reverse = false, inject?: ClothShaderHook) {
  const maps = weaveMaps(fabric.weave);
  const repeat = weaveRepeat(fabric.yarnPitchMm);
  maps.colour.repeat.set(repeat, repeat);
  maps.normal.repeat.set(repeat, repeat);
  // Cloth is matte: almost no mirror-like highlight, and a soft return of light from the fibres at grazing
  // angles. A tight highlight is what makes a smooth simulated surface read as plastic.
  const material = new THREE.MeshPhysicalMaterial({
    color: reverse ? fabric.lining : fabric.colour,
    map: maps.colour, normalMap: maps.normal, normalScale: new THREE.Vector2(0.7, 0.7),
    roughness: 1, metalness: 0, specularIntensity: 0.16,
    sheen: reverse ? 0.2 : fabric.nap ?? 0.6, sheenRoughness: 0.85, sheenColor: fabric.sheen,
    side: THREE.FrontSide, envMapIntensity: 0.9,
  });
  // The scene may change these while it runs (the gain is zero while pieces are still being assembled).
  const uniforms = { wrinkleLength: { value: (fabric.wrinkleMm ?? 11) / 1000 }, wrinkleGain: { value: 0.62 } };
  material.onBeforeCompile = shader => {
    Object.assign(shader.uniforms, uniforms, inject?.uniforms, { yarnsPerMetre: { value: 1000 / fabric.yarnPitchMm } });
    shader.vertexShader = "attribute float fold; attribute vec2 press; varying vec2 clothMetres; varying float clothFold; varying vec2 clothPress;\n"
      + (inject?.vertexHead ?? "") + shader.vertexShader.replace(
      "#include <begin_vertex>", "#include <begin_vertex>\nclothMetres = uv; clothFold = fold; clothPress = press;\n" + (inject?.vertexBody ?? ""));
    shader.fragmentShader = `varying vec2 clothMetres; varying float clothFold; varying vec2 clothPress;
uniform float wrinkleLength; uniform float wrinkleGain; uniform float yarnsPerMetre;
${inject?.fragmentHead ?? ""}
float clothHash(vec2 p){ return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
float clothNoise(vec2 p){
  vec2 i = floor(p), f = fract(p); f = f * f * (3. - 2. * f);
  return mix(mix(clothHash(i), clothHash(i + vec2(1., 0.)), f.x), mix(clothHash(i + vec2(0., 1.)), clothHash(i + vec2(1., 1.)), f.x), f.y);
}
// Tilt the normal by the slope of a height field given in metres over the surface (no texture needed).
vec3 clothBump(vec3 at, vec3 n, float h){
  vec3 sx = dFdx(at), sy = dFdy(at), r1 = cross(sy, n), r2 = cross(n, sx);
  float det = dot(sx, r1);
  return normalize(abs(det) * n - sign(det) * (dFdx(h) * r1 + dFdy(h) * r2));
}
// One family of parallel wrinkles across a fixed pattern direction. Returns (height, shade) for unit slope.
vec2 clothWrinkle(vec2 at, vec2 across){
  float phase = 6.2832 * dot(at, across) / wrinkleLength;
  float seen = 1. - smoothstep(1., 2.4, fwidth(phase));     // gone before they are finer than a pixel
  return vec2(cos(phase) * seen, mix(.5, .5 - .5 * cos(phase), seen));
}
` + shader.fragmentShader.replace("#include <color_fragment>", `#include <color_fragment>
  // Uneven dye take-up and nap at a few centimetres, not a flat fill.
  float mottle = clothNoise(clothMetres * 38.) * .62 + clothNoise(clothMetres * 131.) * .38;
  diffuseColor.rgb *= .93 + .14 * mottle;
  // Yarn-to-yarn unevenness: faint streaks along warp and weft, faded out before they alias.
  vec2 yarn = clothMetres * yarnsPerMetre;
  float streaks = clothNoise(vec2(yarn.x, clothMetres.y * 9.)) + clothNoise(vec2(clothMetres.x * 9., yarn.y)) - 1.;
  diffuseColor.rgb *= 1. + .11 * streaks * (1. - smoothstep(.35, 1.1, max(fwidth(yarn.x), fwidth(yarn.y))));
  // The bottom of a fold sees less of the room than a ridge does. Curvature is in 1/m: a body's own roundness
  // (a radius of 10 cm or more) is left alone, and a fold 2 to 3 cm in radius is fully shaded.
  float bend = ${reverse ? "-" : ""}clothFold;
  diffuseColor.rgb *= 1. - .34 * smoothstep(9., 42., bend) + .07 * smoothstep(9., 42., -bend);
  // Wrinkles finer than the solver's mesh. Where the surface is shorter than its flat pattern by a fraction e,
  // the missing length is drawn as ripples of slope 2√e across the direction of shortening. Four fixed
  // directions are blended, because a ripple phase taken along a turning direction does not stay regular.
  float pressed = length(clothPress);
  vec2 lean = clothPress / max(pressed, 1e-6);            // (cos 2θ, sin 2θ) of the shortened direction
  vec4 share = pow(max(vec4(lean.x, lean.y, -lean.x, -lean.y), 0.), vec4(2.));
  vec2 warped = clothMetres + (vec2(clothNoise(clothMetres * 19.), clothNoise(clothMetres * 19. + 31.7)) - .5) * wrinkleLength * 1.7;
  vec2 ripple = share.x * clothWrinkle(warped, vec2(1., 0.)) + share.y * clothWrinkle(warped, vec2(.7071, .7071))
    + share.z * clothWrinkle(warped, vec2(0., 1.)) + share.w * clothWrinkle(warped, vec2(-.7071, .7071));
  float wrinkleSlope = 2. * sqrt(min(pressed, .3)) * smoothstep(.02, .09, pressed) * wrinkleGain
    * (.55 + .45 * clothNoise(clothMetres * 47.));
  float wrinkleHeight = wrinkleLength / 6.2832 * wrinkleSlope * ripple.x;
  diffuseColor.rgb *= 1. - .34 * wrinkleSlope * ripple.y;
  ${inject?.colour ?? ""}`).replace("#include <normal_fragment_maps>", `#include <normal_fragment_maps>
  normal = clothBump(-vViewPosition, normal, wrinkleHeight);`);
  };
  material.customProgramCacheKey = () => "atelier-cloth-2-" + (reverse ? "reverse-" : "face-") + (inject?.key ?? "plain");
  return { material, uniforms, dispose() { material.dispose(); maps.colour.dispose(); maps.normal.dispose(); } };
}
