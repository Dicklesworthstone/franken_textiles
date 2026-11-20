import * as THREE from "three";
import { ClothShell, type ClothTopology } from "./cloth";
import { clothMaterial, type Fabric } from "./weave";

export interface GarmentTopology extends ClothTopology {
  /** Vertex range of each cut piece, when known. Distances are then measured within a piece. */
  panels?: Record<string, [number, number]>;
}

export interface DressOptions {
  /** A pose with the seams closed, in any axes. */
  sewn?: ArrayLike<number>;
  /** How the unsewn edges are finished: a bound edge with two cords, or a turned and stitched hem. */
  edge?: "braid" | "hem";
}

/** For every vertex, the flat-pattern distance in metres to the nearest of the given edges, capped at 8 cm. */
function distanceTo(edges: number[], flat: ArrayLike<number>, topology: GarmentTopology) {
  const result = new Float32Array(topology.vertices).fill(0.08);
  const ranges = topology.panels ? Object.values(topology.panels) : [[0, topology.vertices] as [number, number]];
  for (const [start, end] of ranges) {
    const own: number[] = [];
    for (let e = 0; e < edges.length; e += 2) if (edges[e] >= start && edges[e] < end && edges[e + 1] >= start && edges[e + 1] < end) own.push(edges[e], edges[e + 1]);
    for (let v = start; v < end; v++) {
      let best = 0.08 * 0.08;
      const px = flat[v * 2], py = flat[v * 2 + 1];
      for (let e = 0; e < own.length; e += 2) {
        const ax = flat[own[e] * 2], ay = flat[own[e] * 2 + 1];
        const dx = flat[own[e + 1] * 2] - ax, dy = flat[own[e + 1] * 2 + 1] - ay;
        const t = Math.max(0, Math.min(1, ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy || 1)));
        best = Math.min(best, (px - ax - t * dx) ** 2 + (py - ay - t * dy) ** 2);
      }
      result[v] = Math.sqrt(best);
    }
  }
  return result;
}

const hook = (edge: DressOptions["edge"], tintAmount: { value: number }) => ({
  key: "garment-" + edge,
  uniforms: { tintAmount },
  vertexHead: "attribute float cutDistance; attribute float seamDistance; attribute vec3 tint; varying float edgeMetres; varying float seamMetres; varying vec3 pieceTint;\n",
  vertexBody: "edgeMetres = cutDistance; seamMetres = seamDistance; pieceTint = tint;\n",
  fragmentHead: "varying float edgeMetres; varying float seamMetres; varying vec3 pieceTint; uniform float tintAmount;\n",
  colour: `
  // While a garment is being assembled each cut piece can carry its own colour, as on the flat pattern.
  diffuseColor.rgb = mix(diffuseColor.rgb, pieceTint * (.8 + .16 * (mottle - .5)), tintAmount);
  // A sewn seam is a shallow ditch with a row of topstitching beside it.
  float seamWidth = max(fwidth(seamMetres), 0.00012);
  float ditch = 1. - smoothstep(0., .0012 + seamWidth, seamMetres);
  diffuseColor.rgb *= 1. - .40 * ditch;
  float topstitch = 1. - smoothstep(.0004, .0004 + seamWidth, abs(seamMetres - .0052));
  diffuseColor.rgb = mix(diffuseColor.rgb, diffuseColor.rgb * 1.28 + .015, topstitch * .5);
  float edgeWidth = max(fwidth(edgeMetres), 0.00012);
  ${edge === "braid" ? `
  // A bound edge, with two rows of couched cord.
  float binding = 1. - smoothstep(.0062 - edgeWidth, .0062 + edgeWidth, edgeMetres);
  diffuseColor.rgb = mix(diffuseColor.rgb, diffuseColor.rgb * .46, binding);
  float cord = 1. - smoothstep(.0007, .0007 + edgeWidth, min(abs(edgeMetres - .0016), abs(edgeMetres - .0046)));
  diffuseColor.rgb = mix(diffuseColor.rgb, vec3(.62, .40, .16), cord * .95);` : `
  // A turned hem: the fold is slightly proud and a single row of stitching holds it.
  float hem = 1. - smoothstep(.0118 - edgeWidth, .0118 + edgeWidth, edgeMetres);
  diffuseColor.rgb *= 1. + .05 * hem;
  float hemStitch = 1. - smoothstep(.0004, .0004 + edgeWidth, abs(edgeMetres - .0105));
  diffuseColor.rgb = mix(diffuseColor.rgb, diffuseColor.rgb * 1.3 + .015, hemStitch * .5);`}`,
});

/**
 * Dress a solver cloth surface as a garment: a shell with thickness, a woven face and a plainer
 * reverse, seams shown as seams and edges finished. `flat` holds each vertex's position on the
 * flat pattern in metres (two numbers per vertex), which is what lays the weave along the grain.
 */
export function dressCloth(topology: GarmentTopology, flat: ArrayLike<number>, fabric: Fabric, options: DressOptions = {}) {
  // Curved tessellation or an extruded shell can extend past a neighbouring panel.
  // Yarn, seam relief and smooth normals shade the exact contact surface.
  const shell = new ClothShell(topology, 0, options.sewn, 0);
  shell.attribute("uv", flat, 2);
  shell.rest(flat);
  const n = topology.vertices, count = new Map<number, number>();
  const key = (a: number, b: number) => Math.min(a, b) * n + Math.max(a, b);
  const triangles = topology.triangles;
  for (let t = 0; t < triangles.length; t += 3) for (let k = 0; k < 3; k++) {
    const e = key(triangles[t + k], triangles[t + (k + 1) % 3]);
    count.set(e, (count.get(e) ?? 0) + 1);
  }
  const sewn = new Set<number>(), seamEdges: number[] = [], cutEdges: number[] = [];
  for (const seam of topology.seams) for (const side of [seam.side_a, seam.side_b]) {
    for (let i = 1; i < side.length; i++) { sewn.add(key(side[i - 1], side[i])); seamEdges.push(side[i - 1], side[i]); }
  }
  for (const [e, uses] of count) if (uses === 1 && !sewn.has(e)) cutEdges.push(Math.floor(e / n), e % n);
  shell.attribute("seamDistance", distanceTo(seamEdges, flat, topology), 1);
  shell.attribute("cutDistance", distanceTo(cutEdges, flat, topology), 1);
  const tintAmount = { value: 0 };
  const face = clothMaterial(fabric, false, hook(options.edge ?? "hem", tintAmount));
  // The reverse carries the same seams, hems and piece colours: a piece seen from behind is still that piece.
  const reverse = clothMaterial(fabric, true, hook(options.edge ?? "hem", tintAmount));
  // Lines drawn on the cloth must not fight with it for depth.
  for (const { material } of [face, reverse]) { material.polygonOffset = true; material.polygonOffsetFactor = 1; material.polygonOffsetUnits = 1; }
  const materials: THREE.Material[] = [face.material, reverse.material, face.material];
  const mesh = new THREE.Mesh<THREE.BufferGeometry, THREE.Material | THREE.Material[]>(shell.geometry, materials);
  mesh.castShadow = mesh.receiveShadow = true;
  mesh.frustumCulled = false;   // the cloth moves every frame; the camera always frames the whole exhibit
  return {
    mesh, shell, materials,
    /** How much of each piece's own colour shows over the cloth's (0 none, 1 all). Set the colours with `tints`. */
    tintAmount,
    /** One colour per solver vertex (r, g, b in 0..1), usually constant over a cut piece. */
    tints: (colours: ArrayLike<number>) => { shell.attribute("tint", colours, 3); },
    /** Scales the fine wrinkles drawn where the cloth is compressed. */
    wrinkleGain: face.uniforms.wrinkleGain,
    /** Move the garment to the solver's vertex positions (y up). */
    update: (points: ArrayLike<number>) => shell.update(points),
    dispose() { shell.dispose(); face.dispose(); reverse.dispose(); },
  };
}
