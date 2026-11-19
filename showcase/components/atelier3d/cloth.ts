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

  /**
   * `sewn` is any pose in which the seams are closed; it settles which end of each run meets which.
   * `levels` is how many times each triangle is split in four for display.
   */
  constructor(topology: ClothTopology, private readonly thickness: number, sewn?: ArrayLike<number>, levels = 2) {
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
