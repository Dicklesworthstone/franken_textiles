import * as THREE from "three";

// Turns the solver's cloth surface (a sheet of triangles, one set of vertices per cut piece) into
// something with a face, a reverse and an edge.
//
// The solver's own vertices are never moved. Between them the surface is refined for display: each
// triangle edge gets a midpoint that follows the curvature implied by the vertex normals (Phong
// tessellation), so a sleeve reads as round and not as a ring of flat facets. The face and reverse
// then sit half a thickness either side of that surface.
//
// Vertex layout: [face M] [reverse M] [edge on face B] [edge on reverse B], where M counts refined
// vertices and B the vertices on a piece's outline. Material groups: 0 face, 1 reverse, 2 edge.

export interface ClothTopology {
  vertices: number;
  triangles: ArrayLike<number>;
  /** Pairs of vertex runs that the pattern sews together. */
  seams: { side_a: number[]; side_b: number[] }[];
}

/** How far a refined midpoint moves from the flat chord toward the curved surface (0 flat, 1 full). */
const CURVE = 0.75;

// Two per-vertex readings of the solver's surface are passed to the material, which uses them for shading
// detail the solver's mesh is too coarse to carry as geometry:
//   `fold`  the surface's curvature at the vertex in 1/m, from how far its neighbours rise above it along
//           its normal: positive in the bottom of a fold seen from the face, negative on a ridge;
//   `press` how much the cloth is shortened relative to its flat pattern, and in which pattern direction,
//           as (amount·cos 2θ, amount·sin 2θ) so that it interpolates without a sign.

export class ClothShell {
  readonly geometry = new THREE.BufferGeometry();
  private readonly n: number;                // solver vertices
  private readonly m: number;                // refined vertices; the first n are the solver's
  private readonly parents: Uint32Array;     // for each refined vertex past n, the two it lies between
  private readonly coarse: Uint32Array;      // the solver's triangles, for normals
  private readonly rimVertex: Uint32Array;   // outline slot -> refined vertex
  private readonly rimEdges: Uint32Array;    // directed outline edges, as pairs of slots
  private readonly pairs: Uint32Array;       // sewn solver-vertex pairs
  private readonly points: Float32Array;     // refined mid-surface
  private readonly normals: Float32Array;
  private readonly out: Float32Array;        // per outline slot, pointing away from the piece
  private readonly position: THREE.BufferAttribute;
  private readonly normal: THREE.BufferAttribute;
  private readonly fold: Float32Array;       // per solver vertex
  private readonly press: Float32Array;      // per solver vertex, two numbers
  private readonly around: Float32Array;     // scratch: neighbour sum (3), neighbour count, edge-length sum
  private readonly weight: Float32Array;     // scratch: triangle area gathered at each vertex
  private restInverse?: Float32Array;        // per solver triangle, the inverse of its two flat edge vectors
  /** Scales `press` before it reaches the material: 0 hides drawn wrinkles while pieces are still being assembled. */
  detail = 1;
  /**
   * Displayed thickness in metres, applied at the next `update`. A diagram view sets it near zero, so
   * that lines drawn on the solver's own surface (the mesh, a seam) are not buried inside the cloth.
   */
  thickness: number;

  /**
   * `sewn` is any pose in which the seams are closed; it settles which end of each run meets which.
   * `levels` is how many times each triangle is split in four for display.
   */
  constructor(topology: ClothTopology, thickness: number, sewn?: ArrayLike<number>, levels = 2) {
    this.thickness = thickness;
    const n = this.n = topology.vertices;
    this.coarse = Uint32Array.from(topology.triangles);
    let tris = this.coarse, count = n;
    const parents: number[] = [];
    for (let level = 0; level < levels; level++) {
      const middle = new Map<number, number>(), next = new Uint32Array(tris.length * 4);
      const mid = (a: number, b: number) => {
        const key = a < b ? a * count + b : b * count + a;   // `count` is fixed while this level's keys are made
        let v = middle.get(key);
        if (v === undefined) { v = base + middle.size; middle.set(key, v); parents.push(a, b); }
        return v;
      };
      const base = count;
      for (let t = 0; t < tris.length; t += 3) {
        const a = tris[t], b = tris[t + 1], c = tris[t + 2], ab = mid(a, b), bc = mid(b, c), ca = mid(c, a);
        next.set([a, ab, ca, ab, b, bc, ca, bc, c, ab, bc, ca], t * 4);
      }
      count = base + middle.size;
      tris = next;
    }
    const m = this.m = count;
    this.parents = Uint32Array.from(parents);
    // An edge used by one triangle lies on the outline of its piece.
    const directed = new Set<number>();
    for (let t = 0; t < tris.length; t += 3) for (let k = 0; k < 3; k++) directed.add(tris[t + k] * m + tris[t + (k + 1) % 3]);
    const slot = new Map<number, number>(), rimVertex: number[] = [], rimEdges: number[] = [];
    const slotOf = (v: number) => { let s = slot.get(v); if (s === undefined) { s = rimVertex.length; slot.set(v, s); rimVertex.push(v); } return s; };
    for (const key of directed) {
      const a = Math.floor(key / m), b = key % m;
      if (!directed.has(b * m + a)) rimEdges.push(slotOf(a), slotOf(b));
    }
    this.rimVertex = Uint32Array.from(rimVertex);
    this.rimEdges = Uint32Array.from(rimEdges);
    const pairs: number[] = [];
    const apart = (a: number, b: number) => sewn ? Math.hypot(sewn[a * 3] - sewn[b * 3], sewn[a * 3 + 1] - sewn[b * 3 + 1], sewn[a * 3 + 2] - sewn[b * 3 + 2]) : 0;
    for (const seam of topology.seams) {
      const length = Math.min(seam.side_a.length, seam.side_b.length);
      if (!length) continue;
      const flipped = apart(seam.side_a[0], seam.side_b[0]) > apart(seam.side_a[0], seam.side_b[length - 1]);
      for (let i = 0; i < length; i++) pairs.push(seam.side_a[i], seam.side_b[flipped ? length - 1 - i : i]);
    }
    this.pairs = Uint32Array.from(pairs);
    const b = rimVertex.length, total = 2 * m + 2 * b;
    this.points = new Float32Array(m * 3);
    this.normals = new Float32Array(m * 3);
    this.out = new Float32Array(b * 3);
    this.fold = new Float32Array(n);
    this.press = new Float32Array(n * 2);
    this.around = new Float32Array(n * 5);
    this.weight = new Float32Array(n);
    this.position = new THREE.BufferAttribute(new Float32Array(total * 3), 3).setUsage(THREE.DynamicDrawUsage);
    this.normal = new THREE.BufferAttribute(new Float32Array(total * 3), 3).setUsage(THREE.DynamicDrawUsage);
    this.geometry.setAttribute("position", this.position);
    this.geometry.setAttribute("normal", this.normal);
    const index = new Uint32Array(tris.length * 2 + rimEdges.length * 3);
    index.set(tris, 0);
    for (let t = 0; t < tris.length; t += 3) {          // the reverse faces the other way
      index[tris.length + t] = tris[t] + m; index[tris.length + t + 1] = tris[t + 2] + m; index[tris.length + t + 2] = tris[t + 1] + m;
    }
    const face = 2 * m, back = 2 * m + b;
    for (let e = 0, o = tris.length * 2; e < rimEdges.length; e += 2, o += 6) {
      const s = rimEdges[e], t = rimEdges[e + 1];
      index.set([face + s, back + t, face + t, face + s, back + s, back + t], o);
    }
    this.geometry.setIndex(new THREE.BufferAttribute(index, 1));
    this.geometry.addGroup(0, tris.length, 0);
    this.geometry.addGroup(tris.length, tris.length, 1);
    this.geometry.addGroup(tris.length * 2, rimEdges.length * 3, 2);
  }

  /** Spread a per-solver-vertex attribute over the refined face, reverse and edge vertices. */
  attribute(name: string, source: ArrayLike<number>, itemSize: number, dynamic = false) {
    const { n, m, parents } = this, b = this.rimVertex.length;
    const existing = this.geometry.getAttribute(name) as THREE.BufferAttribute | undefined;
    const target = existing ?? new THREE.BufferAttribute(new Float32Array((2 * m + 2 * b) * itemSize), itemSize);
    const array = target.array as Float32Array;
    for (let i = 0; i < n * itemSize; i++) array[i] = source[i];
    for (let v = n, p = 0; v < m; v++, p += 2) for (let k = 0; k < itemSize; k++) {
      array[v * itemSize + k] = 0.5 * (array[parents[p] * itemSize + k] + array[parents[p + 1] * itemSize + k]);
    }
    array.copyWithin(m * itemSize, 0, m * itemSize);
    for (let s = 0; s < b; s++) for (let k = 0; k < itemSize; k++) {
      array[(2 * m + s) * itemSize + k] = array[(2 * m + b + s) * itemSize + k] = array[this.rimVertex[s] * itemSize + k];
    }
    if (dynamic) target.setUsage(THREE.DynamicDrawUsage);
    if (existing) existing.needsUpdate = true; else this.geometry.setAttribute(name, target);
    return target;
  }

  /**
   * Give the cloth's flat pattern (two numbers per solver vertex, metres). With it, `update` also measures
   * how far each part of the surface is shortened relative to the pattern.
   */
  rest(flat: ArrayLike<number>) {
    const { coarse } = this, inverse = this.restInverse = new Float32Array((coarse.length / 3) * 4);
    for (let t = 0, o = 0; t < coarse.length; t += 3, o += 4) {
      const a = coarse[t] * 2, b = coarse[t + 1] * 2, c = coarse[t + 2] * 2;
      const ux = flat[b] - flat[a], uy = flat[b + 1] - flat[a + 1], vx = flat[c] - flat[a], vy = flat[c + 1] - flat[a + 1];
      const det = ux * vy - vx * uy;
      if (Math.abs(det) < 1e-12) continue;            // a degenerate flat triangle measures nothing
      inverse[o] = vy / det; inverse[o + 1] = -vx / det; inverse[o + 2] = -uy / det; inverse[o + 3] = ux / det;
    }
  }

  /** Measure `fold` and `press` on the solver's own triangles and hand them to the geometry. */
  private measure() {
    const { n, coarse, points, normals, pairs, fold, press, around, weight, restInverse } = this;
    around.fill(0); press.fill(0); weight.fill(0);
    for (let t = 0, o = 0; t < coarse.length; t += 3, o += 4) {
      const ia = coarse[t], ib = coarse[t + 1], ic = coarse[t + 2], a = ia * 3, b = ib * 3, c = ic * 3;
      const ux = points[b] - points[a], uy = points[b + 1] - points[a + 1], uz = points[b + 2] - points[a + 2];
      const vx = points[c] - points[a], vy = points[c + 1] - points[a + 1], vz = points[c + 2] - points[a + 2];
      const wx = vx - ux, wy = vy - uy, wz = vz - uz;
      const lengths = [Math.hypot(ux, uy, uz), Math.hypot(wx, wy, wz), Math.hypot(vx, vy, vz)];
      // Every vertex hears from the other two of its triangle. An interior edge is heard twice, which a mean ignores.
      for (const [i, j, k, l1, l2] of [[ia, ib, ic, lengths[0], lengths[2]], [ib, ic, ia, lengths[1], lengths[0]], [ic, ia, ib, lengths[2], lengths[1]]] as const) {
        const o5 = i * 5;
        around[o5] += points[j * 3] + points[k * 3]; around[o5 + 1] += points[j * 3 + 1] + points[k * 3 + 1]; around[o5 + 2] += points[j * 3 + 2] + points[k * 3 + 2];
        around[o5 + 3] += 2; around[o5 + 4] += l1 + l2;
      }
      if (!restInverse) continue;
      // The pattern-to-surface stretch: columns F1, F2 are where the pattern's x and y directions go.
      const p = restInverse[o], q = restInverse[o + 1], r = restInverse[o + 2], s = restInverse[o + 3];
      if (p === 0 && q === 0 && r === 0 && s === 0) continue;
      const f1x = ux * p + vx * r, f1y = uy * p + vy * r, f1z = uz * p + vz * r;
      const f2x = ux * q + vx * s, f2y = uy * q + vy * s, f2z = uz * q + vz * s;
      const c11 = f1x * f1x + f1y * f1y + f1z * f1z, c22 = f2x * f2x + f2y * f2y + f2z * f2z, c12 = f1x * f2x + f1y * f2y + f1z * f2z;
      const half = (c11 - c22) / 2, root = Math.hypot(half, c12), least = Math.max(0, (c11 + c22) / 2 - root);
      const amount = Math.max(0, 1 - Math.sqrt(least));     // 0.2 means a fifth shorter than the pattern
      // The shortest direction is at right angles to the longest, so its doubled angle is opposite.
      const turn = root > 1e-12 ? 1 / root : 0, area = Math.hypot(uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx);
      const x = -half * turn * amount * area, y = -c12 * turn * amount * area;
      for (const i of [ia, ib, ic]) { press[i * 2] += x; press[i * 2 + 1] += y; weight[i] += area; }
    }
    // A sewn seam is one surface: both of its vertices take the sum of what each heard.
    for (let p = 0; p < pairs.length; p += 2) {
      const i = pairs[p], j = pairs[p + 1], a = i * 3, c = j * 3;
      const dx = points[a] - points[c], dy = points[a + 1] - points[c + 1], dz = points[a + 2] - points[c + 2];
      if (dx * dx + dy * dy + dz * dz > 1e-6) continue;
      for (let k = 0; k < 5; k++) around[i * 5 + k] = around[j * 5 + k] = around[i * 5 + k] + around[j * 5 + k];
    }
    for (let i = 0; i < n; i++) {
      const o5 = i * 5, count = around[o5 + 3] || 1, reach = around[o5 + 4] / count || 1, a = i * 3;
      const lift = (around[o5] / count - points[a]) * normals[a] + (around[o5 + 1] / count - points[a + 1]) * normals[a + 1] + (around[o5 + 2] / count - points[a + 2]) * normals[a + 2];
      fold[i] = 2 * lift / (reach * reach);      // a curvature in 1/m, so it does not depend on the mesh size
      const area = weight[i] || 1;
      press[i * 2] *= this.detail / area; press[i * 2 + 1] *= this.detail / area;
    }
    this.attribute("fold", fold, 1, true);
    this.attribute("press", press, 2, true);
  }

  /** Place the shell around the solver's vertex positions (x, y, z per vertex, y up). */
  update(solver: ArrayLike<number>) {
    const { n, m, coarse, points, normals, out, pairs, parents } = this, b = this.rimVertex.length, half = this.thickness / 2;
    for (let i = 0; i < n * 3; i++) points[i] = solver[i];
    normals.fill(0, 0, n * 3);
    for (let t = 0; t < coarse.length; t += 3) {
      const a = coarse[t] * 3, c = coarse[t + 1] * 3, d = coarse[t + 2] * 3;
      const ux = points[c] - points[a], uy = points[c + 1] - points[a + 1], uz = points[c + 2] - points[a + 2];
      const vx = points[d] - points[a], vy = points[d + 1] - points[a + 1], vz = points[d + 2] - points[a + 2];
      const x = uy * vz - uz * vy, y = uz * vx - ux * vz, z = ux * vy - uy * vx;   // area-weighted
      normals[a] += x; normals[a + 1] += y; normals[a + 2] += z;
      normals[c] += x; normals[c + 1] += y; normals[c + 2] += z;
      normals[d] += x; normals[d + 1] += y; normals[d + 2] += z;
    }
    // Where two pieces are sewn (their vertices coincide) the surface is continuous, so shade it as one.
    // Two passes let a corner shared by several seams settle.
    for (let pass = 0; pass < 2; pass++) for (let p = 0; p < pairs.length; p += 2) {
      const a = pairs[p] * 3, c = pairs[p + 1] * 3;
      const dx = points[a] - points[c], dy = points[a + 1] - points[c + 1], dz = points[a + 2] - points[c + 2];
      if (dx * dx + dy * dy + dz * dz > 1e-6) continue;                              // more than 1 mm apart: not yet sewn
      for (let k = 0; k < 3; k++) normals[a + k] = normals[c + k] = normals[a + k] + normals[c + k];
    }
    for (let i = 0; i < n * 3; i += 3) {
      const length = Math.hypot(normals[i], normals[i + 1], normals[i + 2]) || 1;
      normals[i] /= length; normals[i + 1] /= length; normals[i + 2] /= length;
    }
    this.measure();
    // Refine. A midpoint starts on the chord and moves toward the tangent planes at its two ends.
    // Both pieces at a sewn seam see the same end points and normals, so they refine identically.
    for (let v = n, p = 0; v < m; v++, p += 2) {
      const a = parents[p] * 3, c = parents[p + 1] * 3, o = v * 3;
      const mx = (points[a] + points[c]) / 2, my = (points[a + 1] + points[c + 1]) / 2, mz = (points[a + 2] + points[c + 2]) / 2;
      const da = (mx - points[a]) * normals[a] + (my - points[a + 1]) * normals[a + 1] + (mz - points[a + 2]) * normals[a + 2];
      const dc = (mx - points[c]) * normals[c] + (my - points[c + 1]) * normals[c + 1] + (mz - points[c + 2]) * normals[c + 2];
      points[o] = mx - CURVE * 0.5 * (da * normals[a] + dc * normals[c]);
      points[o + 1] = my - CURVE * 0.5 * (da * normals[a + 1] + dc * normals[c + 1]);
      points[o + 2] = mz - CURVE * 0.5 * (da * normals[a + 2] + dc * normals[c + 2]);
      const nx = normals[a] + normals[c], ny = normals[a + 1] + normals[c + 1], nz = normals[a + 2] + normals[c + 2];
      const length = Math.hypot(nx, ny, nz) || 1;
      normals[o] = nx / length; normals[o + 1] = ny / length; normals[o + 2] = nz / length;
    }
    // The edge faces away from the piece: along the outline's tangent crossed with the surface normal.
    out.fill(0);
    for (let e = 0; e < this.rimEdges.length; e += 2) {
      const s = this.rimEdges[e], t = this.rimEdges[e + 1], a = this.rimVertex[s] * 3, c = this.rimVertex[t] * 3;
      const dx = points[c] - points[a], dy = points[c + 1] - points[a + 1], dz = points[c + 2] - points[a + 2];
      const nx = normals[a] + normals[c], ny = normals[a + 1] + normals[c + 1], nz = normals[a + 2] + normals[c + 2];
      const x = dy * nz - dz * ny, y = dz * nx - dx * nz, z = dx * ny - dy * nx;
      out[s * 3] += x; out[s * 3 + 1] += y; out[s * 3 + 2] += z;
      out[t * 3] += x; out[t * 3 + 1] += y; out[t * 3 + 2] += z;
    }
    const position = this.position.array as Float32Array, normal = this.normal.array as Float32Array;
    for (let i = 0; i < m * 3; i++) {
      position[i] = points[i] + normals[i] * half; normal[i] = normals[i];
      position[m * 3 + i] = points[i] - normals[i] * half; normal[m * 3 + i] = -normals[i];
    }
    for (let s = 0; s < b; s++) {
      const v = this.rimVertex[s] * 3, r = s * 3, f = (2 * m + s) * 3, k = (2 * m + b + s) * 3;
      const length = Math.hypot(out[r], out[r + 1], out[r + 2]) || 1;
      for (let j = 0; j < 3; j++) {
        // Blend a little of the surface normal in, which rounds the edge the way a turned hem is round.
        const o = out[r + j] / length;
        position[f + j] = position[v + j]; position[k + j] = position[m * 3 + v + j];
        normal[f + j] = o * 0.8 + normals[v + j] * 0.6; normal[k + j] = o * 0.8 - normals[v + j] * 0.6;
      }
    }
    this.position.needsUpdate = this.normal.needsUpdate = true;
    this.geometry.computeBoundingSphere();
  }

  dispose() { this.geometry.dispose(); }
}
