//! Stills of a drape without Blender: the reference renders a finished fold-up in Blender from four sides
//! (front, side, back, three-quarter) in the seam colours and front and back in the strain colours; here the
//! same six views are ray cast from `ftx drape`'s OBJs (body, garment, garment in strain colours).
//!
//! Camera as the reference's stills: a 50 mm lens on a 36 mm sensor, 2 m from the target and 0.12 m above
//! it, turning about the vertical through the world origin (0° in front, the wearer faces −y); the target is
//! the garment's middle height, kept between a quarter and four fifths of the body's height. Shading: each
//! vertex's colour (the body a skin tone) under a key light from the camera's upper left with a shadow ray, a
//! fill light, and ambient occlusion from 8 fixed Halton directions over 0.1 m; normals smoothed per
//! vertex and turned towards the eye (the cloth is a single sheet); a grey backdrop. The sRGB encode is
//! applied to linear light; 8-bit PNG (RGB, zlib).
//!
//! Every pixel is computed on its own from the same arithmetic (`det` for trigonometry and powers), so the
//! image is the same bits however many threads draw it (rows are shared across `available_parallelism`;
//! one thread on wasm).
//!
//! [`Shading::Legible`] is a second way to draw the same geometry through the same camera, for reading
//! folds and edges by eye. One ray a pixel aliases a fine wrinkle and a matte surface under 8 ambient
//! rays flattens it, so it casts four rays a pixel on a fixed rotated grid and averages them in linear
//! light, takes 16 stratified ambient directions a ray (64 different ones a pixel), and on cloth adds a
//! highlight that grows as the view grazes the surface and a dark line just inside every hem and
//! opening. A seam gets no line: it carries its seam colour, which the line would dim. Where the
//! cloth's colours are the strain scale it adds neither, because there the colour is the reading. It is
//! a drawing aid and not a picture of a fabric: its constants are fixed here, come from no material
//! record and are the same for every cloth. The standard still is untouched by it, bit for bit, and a
//! pixel of either still names the same place (`pick_still` casts through the pixel's centre).
//!
//! Provenance (frankensim, same author, MIT + OpenAI/Anthropic rider, commit `7b70bc9e`): the deterministic
//! median-split BVH (`TriMesh::build`) and the watertight shear-space ray–triangle test (`tri_hit`, Woop,
//! Benthin and Wald) from `crates/fs-render/src/charts.rs`; the sRGB encode and 8-bit quantize from
//! `crates/fs-img/src/film.rs`; the CRC-32 from `crates/fs-img/src/png.rs`. Changed on lifting: no
//! cancellation context, fingerprint or parameter rescaling; f64 throughout; the slab test without
//! outward rounding (a pixel ray misses a node by more than rounding or not at all).

use ftx_core::{Refusal, RefusalCode, Vec3, det};

/// A triangle mesh with per-vertex colours (linear RGB in [0, 1]).
#[derive(Clone, Debug, Default)]
pub struct ColouredMesh {
    /// Vertices (metres).
    pub vertices: Vec<Vec3>,
    /// Triangles.
    pub triangles: Vec<[usize; 3]>,
    /// Per-vertex colours; empty: the body's skin tone.
    pub colours: Vec<[f64; 3]>,
}

/// The skin tone of the body (linear RGB, the reference's form material).
pub const SKIN: [f64; 3] = [0.82, 0.68, 0.58];

/// Reads a Wavefront OBJ as `ftx drape` writes it (`v x y z [r g b]`, 1-based `f a b c`).
///
/// # Errors
/// A line that cannot be read, with its number.
pub fn read_obj(text: &str) -> Result<ColouredMesh, String> {
    let mut m = ColouredMesh::default();
    for (n, line) in text.lines().enumerate() {
        let mut it = line.split_whitespace();
        let bad = || format!("line {}: {line}", n + 1);
        match it.next() {
            Some("v") => {
                let v: Vec<f64> = it
                    .map(str::parse)
                    .collect::<Result<_, _>>()
                    .map_err(|_| bad())?;
                if v.len() < 3 || v.iter().any(|x| !x.is_finite()) {
                    return Err(bad());
                }
                m.vertices.push(Vec3::new(v[0], v[1], v[2]));
                if v.len() >= 6 {
                    m.colours.push([v[3], v[4], v[5]]);
                }
            }
            Some("f") => {
                let f: Vec<usize> = it
                    .map(|s| s.split('/').next().unwrap_or("").parse::<usize>())
                    .collect::<Result<_, _>>()
                    .map_err(|_| bad())?;
                if f.len() < 3 || f.contains(&0) {
                    return Err(bad());
                }
                // fan a polygon into triangles
                for k in 1..f.len() - 1 {
                    m.triangles.push([f[0] - 1, f[k] - 1, f[k + 1] - 1]);
                }
            }
            _ => {}
        }
    }
    if m.triangles.iter().flatten().any(|&i| i >= m.vertices.len()) {
        return Err("a face names a vertex the file does not have".into());
    }
    if !m.colours.is_empty() && m.colours.len() != m.vertices.len() {
        return Err("colours on some vertices only".into());
    }
    Ok(m)
}

#[derive(Clone, Copy, Debug)]
struct Node {
    lo: [f64; 3],
    hi: [f64; 3],
    a: usize,
    b: usize,
    leaf: bool,
}

/// All meshes of a scene in one BVH (frankensim's deterministic median split, leaves of up to 4).
struct Scene {
    p: Vec<[f64; 3]>,
    tris: Vec<[usize; 3]>,
    normals: Vec<Vec3>,
    colours: Vec<[f64; 3]>,
    nodes: Vec<Node>,
    order: Vec<usize>,
    /// The first triangle of each mesh, in the order the meshes were given.
    first: Vec<usize>,
}

struct Hit {
    t: f64,
    tri: usize,
    bary: [f64; 3],
}

/// What a primary ray found, ready for lighting.
struct Surface {
    /// Colour interpolated from the triangle's corners (linear RGB).
    col: [f64; 3],
    /// Unit normal, smoothed over the corners and turned towards the eye.
    n: Vec3,
    /// A point just off the surface, where secondary rays start.
    lo: [f64; 3],
}

impl Scene {
    fn new(meshes: &[&ColouredMesh]) -> Self {
        let (mut p, mut tris, mut colours) = (Vec::new(), Vec::new(), Vec::new());
        let mut first = Vec::with_capacity(meshes.len());
        for m in meshes {
            first.push(tris.len());
            let base = p.len();
            p.extend(m.vertices.iter().map(|v| [v.x, v.y, v.z]));
            tris.extend(
                m.triangles
                    .iter()
                    .filter(|t| t.iter().all(|&i| i < m.vertices.len()))
                    .map(|t| [t[0] + base, t[1] + base, t[2] + base]),
            );
            if m.colours.len() == m.vertices.len() {
                colours.extend(m.colours.iter().copied());
            } else {
                colours.extend(
                    (0..m.vertices.len()).map(|i| m.colours.get(i).copied().unwrap_or(SKIN)),
                );
            }
        }
        // area-weighted vertex normals
        let mut normals = vec![Vec3::ZERO; p.len()];
        for t in &tris {
            let v = |i: usize| Vec3::new(p[t[i]][0], p[t[i]][1], p[t[i]][2]);
            let n = (v(1) - v(0)).cross(v(2) - v(0));
            for &i in t {
                normals[i] += n;
            }
        }
        for n in &mut normals {
            let l = n.norm();
            if l > 0.0 {
                *n = *n * (1.0 / l);
            }
        }
        let mut s = Scene {
            p,
            tris,
            normals,
            colours,
            nodes: Vec::new(),
            order: Vec::new(),
            first,
        };
        let mut order: Vec<usize> = (0..s.tris.len()).collect();
        if !order.is_empty() {
            let n = order.len();
            s.build(&mut order, 0, n);
        }
        s.order = order;
        s
    }

    fn centroid(&self, t: usize, axis: usize) -> f64 {
        self.tris[t].iter().map(|&v| self.p[v][axis] / 3.0).sum()
    }

    fn build(&mut self, order: &mut [usize], start: usize, count: usize) -> usize {
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for &t in &order[start..start + count] {
            for &v in &self.tris[t] {
                for k in 0..3 {
                    lo[k] = lo[k].min(self.p[v][k]);
                    hi[k] = hi[k].max(self.p[v][k]);
                }
            }
        }
        let id = self.nodes.len();
        self.nodes.push(Node {
            lo,
            hi,
            a: start,
            b: count,
            leaf: true,
        });
        if count <= 4 {
            return id;
        }
        let axis = (0..3)
            .max_by(|&a, &b| (hi[a] - lo[a]).total_cmp(&(hi[b] - lo[b])))
            .unwrap_or(0);
        order[start..start + count].sort_by(|&x, &y| {
            self.centroid(x, axis)
                .total_cmp(&self.centroid(y, axis))
                .then(x.cmp(&y))
        });
        let half = count / 2;
        let left = self.build(order, start, half);
        let right = self.build(order, start + half, count - half);
        self.nodes[id] = Node {
            lo,
            hi,
            a: left,
            b: right,
            leaf: false,
        };
        id
    }

    fn slab(o: [f64; 3], inv: [f64; 3], n: &Node, t_max: f64) -> bool {
        let (mut t0, mut t1) = (0.0f64, t_max);
        for k in 0..3 {
            if !inv[k].is_finite() {
                if o[k] < n.lo[k] || o[k] > n.hi[k] {
                    return false;
                }
                continue;
            }
            let (a, b) = ((n.lo[k] - o[k]) * inv[k], (n.hi[k] - o[k]) * inv[k]);
            let (a, b) = if a <= b { (a, b) } else { (b, a) };
            t0 = t0.max(a);
            t1 = t1.min(b);
            if t0 > t1 {
                return false;
            }
        }
        true
    }

    /// frankensim's watertight shear-space test: the hit's t and barycentrics (of the triangle's vertices).
    fn tri_hit(&self, o: [f64; 3], d: [f64; 3], ti: usize) -> Option<(f64, [f64; 3])> {
        let [a, b, c] = self.tris[ti].map(|v| self.p[v]);
        let ad = [d[0].abs(), d[1].abs(), d[2].abs()];
        let kz = if ad[0] > ad[1] {
            if ad[0] > ad[2] { 0 } else { 2 }
        } else if ad[1] > ad[2] {
            1
        } else {
            2
        };
        if d[kz] == 0.0 {
            return None;
        }
        let mut kx = (kz + 1) % 3;
        let mut ky = (kx + 1) % 3;
        if d[kz] < 0.0 {
            std::mem::swap(&mut kx, &mut ky);
        }
        let (sx, sy, sz) = (d[kx] / d[kz], d[ky] / d[kz], 1.0 / d[kz]);
        let shear = |v: [f64; 3]| {
            let t = [v[0] - o[0], v[1] - o[1], v[2] - o[2]];
            [t[kx] - sx * t[kz], t[ky] - sy * t[kz], t[kz] * sz]
        };
        let (pa, pb, pc) = (shear(a), shear(b), shear(c));
        let ea = pb[0] * pc[1] - pb[1] * pc[0];
        let eb = pc[0] * pa[1] - pc[1] * pa[0];
        let ec = pa[0] * pb[1] - pa[1] * pb[0];
        if (ea < 0.0 || eb < 0.0 || ec < 0.0) && (ea > 0.0 || eb > 0.0 || ec > 0.0) {
            return None;
        }
        let det_ = ea + eb + ec;
        if det_ == 0.0 || !det_.is_finite() {
            return None;
        }
        let st = ea * pa[2] + eb * pb[2] + ec * pc[2];
        if (det_ > 0.0 && st <= 0.0) || (det_ < 0.0 && st >= 0.0) {
            return None;
        }
        let inv = 1.0 / det_;
        let t = st * inv;
        (t > 0.0 && t.is_finite()).then_some((t, [ea * inv, eb * inv, ec * inv]))
    }

    /// The nearest hit along `o + t d`, t in (0, t_max); `any`: stop at the first (shadow rays).
    fn cast(&self, o: [f64; 3], d: [f64; 3], t_max: f64, any: bool) -> Option<Hit> {
        if self.nodes.is_empty() {
            return None;
        }
        let inv = d.map(|x| 1.0 / x);
        let mut best: Option<Hit> = None;
        let mut stack = vec![0usize];
        while let Some(id) = stack.pop() {
            let n = self.nodes[id];
            if !Self::slab(o, inv, &n, best.as_ref().map_or(t_max, |h| h.t)) {
                continue;
            }
            if n.leaf {
                for &ti in &self.order[n.a..n.a + n.b] {
                    if let Some((t, bary)) = self.tri_hit(o, d, ti)
                        && t < t_max
                        && best
                            .as_ref()
                            .is_none_or(|h| t < h.t || (t == h.t && ti < h.tri))
                    {
                        best = Some(Hit { t, tri: ti, bary });
                        if any {
                            return best;
                        }
                    }
                }
            } else {
                stack.push(n.b);
                stack.push(n.a);
            }
        }
        best
    }

    /// The surface a primary ray from `eye` along `d` met at `h`.
    fn surface(&self, eye: Vec3, d: Vec3, h: &Hit) -> Surface {
        let t = self.tris[h.tri];
        let mut n = Vec3::ZERO;
        let mut col = [0.0; 3];
        for (k, &v) in t.iter().enumerate() {
            n += self.normals[v] * h.bary[k];
            for (c, &cv) in col.iter_mut().zip(&self.colours[v]) {
                *c += cv * h.bary[k];
            }
        }
        let mut n = n.normalized();
        if n.dot(d) > 0.0 {
            n = -n;
        }
        let p = eye + d * h.t;
        let lift = p + n * 1e-4;
        Surface {
            col,
            n,
            lo: [lift.x, lift.y, lift.z],
        }
    }

    /// The cosine of the light from `l` on the surface; nothing where it faces away or lies in shadow.
    fn lit(&self, s: &Surface, l: Vec3) -> f64 {
        let c = s.n.dot(l);
        if c <= 0.0
            || self
                .cast(s.lo, [l.x, l.y, l.z], f64::INFINITY, true)
                .is_some()
        {
            0.0
        } else {
            c
        }
    }

    /// The share of `dirs` (in the frame of the normal) that leave the surface without meeting anything
    /// within 0.1 m: ambient occlusion.
    fn open(&self, s: &Surface, dirs: &[[f64; 3]]) -> f64 {
        let n = s.n;
        let a = if n.x.abs() < 0.9 {
            Vec3::new(1.0, 0.0, 0.0)
        } else {
            Vec3::new(0.0, 1.0, 0.0)
        };
        let tu = n.cross(a).normalized();
        let tv = n.cross(tu);
        dirs.iter()
            .filter(|d| {
                let w = tu * d[0] + tv * d[1] + n * d[2];
                self.cast(s.lo, [w.x, w.y, w.z], 0.1, true).is_none()
            })
            .count() as f64
            / dirs.len() as f64
    }

    /// The cloth's open edges: a hem or an opening. An edge is open if it belongs to exactly one of the
    /// triangles from `first_cloth` on and no other such edge lies on the same two points. That second
    /// test leaves seams out: a drape's `garment.obj` keeps the two sides of a seam as vertices of their
    /// own at the same welded positions, and a seam carries its seam colour, which a line would dim.
    fn open_edges(&self, first_cloth: usize) -> OpenEdges {
        let mut sides: Vec<[usize; 2]> = Vec::new();
        for tri in self.tris.iter().skip(first_cloth) {
            for k in 0..3 {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                sides.push([a.min(b), a.max(b)]);
            }
        }
        // sorted, never hashed: the same edges in the same order on every run and every target
        sides.sort_unstable();
        let mut rim = Vec::new();
        let mut i = 0;
        while i < sides.len() {
            let mut j = i + 1;
            while j < sides.len() && sides[j] == sides[i] {
                j += 1;
            }
            if j == i + 1 {
                rim.push(sides[i]);
            }
            i = j;
        }
        // where an edge lies: its two points as bits (−0 read as 0), the lesser first
        let place = |e: &[usize; 2]| {
            let [a, b] = e.map(|v| self.p[v].map(|x| (x + 0.0).to_bits()));
            if a <= b { [a, b] } else { [b, a] }
        };
        let mut placed: Vec<([[u64; 3]; 2], [usize; 2])> =
            rim.iter().map(|e| (place(e), *e)).collect();
        placed.sort_unstable();
        let mut edges = Vec::new();
        let mut i = 0;
        while i < placed.len() {
            let mut j = i + 1;
            while j < placed.len() && placed[j].0 == placed[i].0 {
                j += 1;
            }
            if j == i + 1 {
                edges.push(placed[i].1);
            }
            i = j;
        }
        edges.sort_unstable();
        let mut start = vec![0usize; self.p.len() + 1];
        for e in &edges {
            for &v in e {
                start[v + 1] += 1;
            }
        }
        for v in 0..self.p.len() {
            start[v + 1] += start[v];
        }
        let mut next = start.clone();
        let mut at = vec![0usize; 2 * edges.len()];
        for (e, ends) in edges.iter().enumerate() {
            for &v in ends {
                at[next[v]] = e;
                next[v] += 1;
            }
        }
        OpenEdges { edges, start, at }
    }

    /// Distance (m) from the hit to the nearest open edge that ends at a corner of the hit's triangle;
    /// `None` if no such edge does. Exact for a line narrower than the mesh's triangles.
    fn open_edge_distance(&self, open: &OpenEdges, h: &Hit) -> Option<f64> {
        let corner = |v: usize| Vec3::new(self.p[v][0], self.p[v][1], self.p[v][2]);
        let t = self.tris[h.tri];
        let p = corner(t[0]) * h.bary[0] + corner(t[1]) * h.bary[1] + corner(t[2]) * h.bary[2];
        let mut nearest: Option<f64> = None;
        for &v in &t {
            for &e in &open.at[open.start[v]..open.start[v + 1]] {
                let [a, b] = open.edges[e].map(corner);
                let along = b - a;
                let length2 = along.dot(along);
                let s = if length2 > 0.0 {
                    ((p - a).dot(along) / length2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let d = (p - (a + along * s)).norm();
                nearest = Some(nearest.map_or(d, |best| best.min(d)));
            }
        }
        nearest
    }
}

/// The cloth's hems and openings, and which of those edges end at each vertex.
struct OpenEdges {
    /// Each edge's two vertices.
    edges: Vec<[usize; 2]>,
    /// `at[start[v]..start[v + 1]]` are the edges that end at vertex `v`.
    start: Vec<usize>,
    at: Vec<usize>,
}

/// A pinhole camera.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    /// Eye (m).
    pub eye: Vec3,
    /// Point looked at (m).
    pub target: Vec3,
    /// Focal length over sensor width (Blender's 50 mm on 36 mm).
    pub focal_over_sensor: f64,
}

impl Camera {
    fn axes(self) -> (Vec3, Vec3, Vec3) {
        let forward = (self.target - self.eye).normalized();
        let right = forward.cross(Vec3::new(0.0, 0.0, 1.0)).normalized();
        (forward, right, right.cross(forward))
    }

    fn pixel_rays(self, [w, h]: [usize; 2]) -> impl Fn(usize, usize) -> Vec3 {
        let (forward, right, up) = self.axes();
        let half_w = 0.5 / self.focal_over_sensor;
        let aspect = h as f64 / w as f64;
        move |x, y| {
            let px = ((x as f64 + 0.5) / w as f64 * 2.0 - 1.0) * half_w;
            let py = (1.0 - (y as f64 + 0.5) / h as f64 * 2.0) * half_w * aspect;
            (forward + right * px + up * py).normalized()
        }
    }

    /// Rays through a point of a pixel, given as fractions of the pixel from its upper-left corner;
    /// (0.5, 0.5) is the centre, where `pixel_rays` casts.
    fn sub_pixel_rays(self, [w, h]: [usize; 2]) -> impl Fn(usize, usize, [f64; 2]) -> Vec3 {
        let (forward, right, up) = self.axes();
        let half_w = 0.5 / self.focal_over_sensor;
        let aspect = h as f64 / w as f64;
        move |x, y, [fx, fy]| {
            let px = ((x as f64 + fx) / w as f64 * 2.0 - 1.0) * half_w;
            let py = (1.0 - (y as f64 + fy) / h as f64 * 2.0) * half_w * aspect;
            (forward + right * px + up * py).normalized()
        }
    }
}

/// How a still is shaded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shading {
    /// One ray through each pixel's centre, a matte surface and 8 ambient rays. Every still written
    /// before this choice existed is this one, bit for bit.
    #[default]
    Standard,
    /// For reading folds and edges by eye: four rays a pixel, 16 ambient rays a ray, and on cloth a
    /// grazing highlight and a dark line inside hems and openings. A drawing aid whose constants describe no
    /// fabric.
    Legible,
}

impl Shading {
    /// Reads `standard` or `legible`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "standard" => Some(Self::Standard),
            "legible" => Some(Self::Legible),
            _ => None,
        }
    }

    /// The name [`Shading::parse`] reads.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Legible => "legible",
        }
    }

    /// What a still's file name carries after its view. Nothing for the standard still, whose name is
    /// the one it always had.
    #[must_use]
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Standard => "",
            Self::Legible => "_legible",
        }
    }
}

/// Where in a pixel the legible still casts its four rays: a rotated grid, one ray in each row and each
/// column of the pixel's 4 × 4 subdivision.
const SUB_PIXELS: [[f64; 2]; 4] = [
    [0.375, 0.125],
    [0.875, 0.375],
    [0.125, 0.625],
    [0.625, 0.875],
];

/// Strength of the legible still's grazing highlight. A drawing constant, the same for every cloth,
/// chosen by eye on a dark bolero: enough to show a fold's flank, short of washing the seam colours out.
const HIGHLIGHT: f64 = 2.0;

/// The legible still darkens cloth within this distance (m) of a hem or opening, by up to `EDGE_DARKENING`
/// at the edge itself: about three pixels of a default 960-pixel still, whose camera stands 2 m away.
const EDGE_LINE_M: f64 = 0.005;
const EDGE_DARKENING: f64 = 0.45;

/// The legible still's ambient directions for the ray through `SUB_PIXELS[sample]`: a cosine-weighted
/// hemisphere cut into 4 rings × 4 sectors, one direction in each of the 16 cells. The four rays of a
/// pixel take four different points of every cell (the same rotated grid as the sub-pixels), so a pixel
/// looks along 64 different directions.
fn stratified_hemisphere(sample: usize) -> Vec<[f64; 3]> {
    let [in_ring, in_sector] = SUB_PIXELS[sample % SUB_PIXELS.len()];
    let mut dirs = Vec::with_capacity(16);
    for ring in 0..4 {
        for sector in 0..4 {
            let u = (f64::from(ring) + in_ring) / 4.0;
            let v = (f64::from(sector) + in_sector) / 4.0;
            let r = det::sqrt(u);
            let (s, c) = det::sin_cos(2.0 * std::f64::consts::PI * v);
            dirs.push([r * c, r * s, det::sqrt(1.0 - u)]);
        }
    }
    dirs
}

/// The legible still's highlight from one light, before its strength: the Charlie sheen distribution at
/// roughness 0.25, where its exponent is 4 and it reads (3/π) sin⁴ of the half-angle (Estevez and Kulla,
/// "Production Friendly Microfacet Sheen BRDF", 2017), with the visibility term 1 / (4 (n·l + n·v − n·l n·v))
/// of Neubelt and Pettineo ("Crafting a Next-Gen Material Pipeline for The Order: 1886", 2013). `lit` is the
/// light's cosine on the surface, already nothing in shadow. The term grows as the view grazes the
/// surface, which is what shows a fold turning away. It measures no cloth.
fn grazing(n: Vec3, to_eye: Vec3, l: Vec3, lit: f64) -> f64 {
    if lit <= 0.0 {
        return 0.0;
    }
    // a floor on the view's cosine keeps the term finite at a silhouette
    let nov = n.dot(to_eye).max(0.05);
    let noh = n.dot((to_eye + l).normalized()).clamp(0.0, 1.0);
    let sin2 = 1.0 - noh * noh;
    let distribution = 3.0 * std::f64::consts::FRAC_1_PI * sin2 * sin2;
    distribution * lit / (4.0 * (lit + nov - lit * nov))
}

/// The sRGB encode of linear light in [0, 1], then 8 bits (frankensim `fs-img` `film.rs`).
fn srgb8(linear: f64) -> u8 {
    let l = linear.clamp(0.0, 1.0);
    let e = if l <= 0.003_130_8 {
        12.92 * l
    } else {
        1.055 * det::exp(det::ln(l) / 2.4) - 0.055
    };
    (e.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// The k-th point of the base-b radical inverse.
fn radical_inverse(mut k: u32, b: u32) -> f64 {
    let (mut f, mut r) = (1.0, 0.0);
    while k > 0 {
        f /= f64::from(b);
        r += f * f64::from(k % b);
        k /= b;
    }
    r
}

/// Renders `meshes` from `cam` at `width` × `height`: 8-bit RGB rows, top first.
#[must_use]
pub fn render(meshes: &[&ColouredMesh], cam: Camera, width: usize, height: usize) -> Vec<u8> {
    render_with(
        meshes,
        meshes.len(),
        cam,
        [width, height],
        Shading::Standard,
        false,
    )
}

/// One still of a body and its cloth from `cam`: 8-bit RGB rows, top first. `strain` says the cloth's
/// colours are the strain scale, to which [`Shading::Legible`] adds no highlight and no edge line.
#[must_use]
pub fn still(
    body: &ColouredMesh,
    cloth: &ColouredMesh,
    cam: Camera,
    [width, height]: [usize; 2],
    shading: Shading,
    strain: bool,
) -> Vec<u8> {
    render_with(&[body, cloth], 1, cam, [width, height], shading, !strain)
}

/// `meshes[cloth..]` are cloth and the ones before them the body; `mark_cloth` lets the legible shading
/// add its highlight and edge line to the cloth. Neither matters to the standard shading.
fn render_with(
    meshes: &[&ColouredMesh],
    cloth: usize,
    cam: Camera,
    [width, height]: [usize; 2],
    shading: Shading,
    mark_cloth: bool,
) -> Vec<u8> {
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let scene = Scene::new(meshes);
    let (fwd, right, up) = cam.axes();
    // lights in the camera's frame: key from the upper left, fill from the right
    let key = (up * 0.8 - right * 0.6 - fwd * 0.9).normalized();
    let fill = (right * 0.7 + up * 0.2 - fwd * 0.6).normalized();
    // 8 cosine-weighted hemisphere directions (Halton 2, 3)
    let ao: Vec<[f64; 3]> = (1..=8u32)
        .map(|k| {
            let (u, v) = (radical_inverse(k, 2), radical_inverse(k, 3));
            let r = det::sqrt(u);
            let (s, c) = det::sin_cos(2.0 * std::f64::consts::PI * v);
            [r * c, r * s, det::sqrt((1.0 - u).max(0.0))]
        })
        .collect();
    let ray = cam.pixel_rays([width, height]);
    let o = [cam.eye.x, cam.eye.y, cam.eye.z];
    let backdrop = |y: usize| -> [f64; 3] {
        let g = 0.32 + 0.18 * (1.0 - y as f64 / height as f64);
        [g, g, g * 1.02]
    };
    // the standard still: one ray through the pixel's centre
    let standard = |x: usize, y: usize| -> [u8; 3] {
        let d = ray(x, y);
        let rgb = match scene.cast(o, [d.x, d.y, d.z], f64::INFINITY, false) {
            None => backdrop(y),
            Some(h) => {
                let s = scene.surface(cam.eye, d, &h);
                let open = scene.open(&s, &ao);
                let light = 0.30 * open + 0.75 * scene.lit(&s, key) + 0.25 * s.n.dot(fill).max(0.0);
                s.col.map(|c| c * light)
            }
        };
        rgb.map(srgb8)
    };
    // the legible still: four rays, each with its own turn of the ambient directions, averaged in linear
    // light before the sRGB encode
    let legible = shading == Shading::Legible;
    let sub_ray = cam.sub_pixel_rays([width, height]);
    let hemispheres: Vec<Vec<[f64; 3]>> = if legible {
        (0..SUB_PIXELS.len()).map(stratified_hemisphere).collect()
    } else {
        Vec::new()
    };
    let first_cloth = scene.first.get(cloth).copied().unwrap_or(scene.tris.len());
    let open_edges = (legible && mark_cloth).then(|| scene.open_edges(first_cloth));
    let detailed = |x: usize, y: usize| -> [u8; 3] {
        let mut sum = [0.0; 3];
        for (offset, dirs) in SUB_PIXELS.iter().zip(&hemispheres) {
            let d = sub_ray(x, y, *offset);
            let rgb = match scene.cast(o, [d.x, d.y, d.z], f64::INFINITY, false) {
                None => backdrop(y),
                Some(h) => {
                    let s = scene.surface(cam.eye, d, &h);
                    let open = scene.open(&s, dirs);
                    let (from_key, from_fill) = (scene.lit(&s, key), s.n.dot(fill).max(0.0));
                    let light = 0.30 * open + 0.75 * from_key + 0.25 * from_fill;
                    let mut rgb = s.col.map(|c| c * light);
                    if let Some(hems) = open_edges.as_ref().filter(|_| h.tri >= first_cloth) {
                        // the highlight takes half the cloth's colour and fades in a crease as the
                        // ambient light does
                        let glance = HIGHLIGHT
                            * open
                            * (0.75 * grazing(s.n, -d, key, from_key)
                                + 0.25 * grazing(s.n, -d, fill, from_fill));
                        for (c, base) in rgb.iter_mut().zip(s.col) {
                            *c += glance * (0.5 * base + 0.5);
                        }
                        if let Some(distance) = scene.open_edge_distance(hems, &h)
                            && distance < EDGE_LINE_M
                        {
                            let keep = 1.0 - EDGE_DARKENING * (1.0 - distance / EDGE_LINE_M);
                            rgb = rgb.map(|c| c * keep);
                        }
                    }
                    rgb
                }
            };
            for (total, c) in sum.iter_mut().zip(rgb) {
                *total += c;
            }
        }
        sum.map(|c| srgb8(c / SUB_PIXELS.len() as f64))
    };
    let pixel = |x: usize, y: usize| -> [u8; 3] {
        if legible {
            detailed(x, y)
        } else {
            standard(x, y)
        }
    };
    let threads = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(height.max(1));
    let mut out = vec![0u8; width * height * 3];
    if threads <= 1 {
        for y in 0..height {
            for x in 0..width {
                out[(y * width + x) * 3..][..3].copy_from_slice(&pixel(x, y));
            }
        }
        return out;
    }
    let rows_per = height.div_ceil(threads);
    std::thread::scope(|s| {
        for (i, chunk) in out.chunks_mut(rows_per * width * 3).enumerate() {
            let pixel = &pixel;
            s.spawn(move || {
                for (r, row) in chunk.chunks_mut(width * 3).enumerate() {
                    let y = i * rows_per + r;
                    for x in 0..width {
                        row[x * 3..x * 3 + 3].copy_from_slice(&pixel(x, y));
                    }
                }
            });
        }
    });
    out
}

/// CRC-32 (IEEE 802.3, reflected), the PNG chunk checksum.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// An 8-bit RGB PNG of `rgb` (rows top first).
#[must_use]
pub fn png(width: usize, height: usize, rgb: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(height * (width * 3 + 1));
    if width > 0 {
        for row in rgb.chunks(width * 3) {
            raw.push(0); // filter: none
            raw.extend_from_slice(row);
        }
    } else {
        raw.resize(height, 0);
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let chunk = |out: &mut Vec<u8>, kind: &[u8], data: &[u8]| {
        out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&u32::try_from(width).unwrap_or(0).to_be_bytes());
    ihdr.extend_from_slice(&u32::try_from(height).unwrap_or(0).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(
        &mut out,
        b"IDAT",
        &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6),
    );
    chunk(&mut out, b"IEND", &[]);
    out
}

/// Camera for a native still. Reuse it with identical raster dimensions to compare another mesh
/// in the same world frame. Unknown views have no camera.
#[must_use]
pub fn still_camera(body: &ColouredMesh, garment: &ColouredMesh, view: &str) -> Option<Camera> {
    let az: f64 = match view {
        "front" | "strain_front" => 0.0,
        "side" => 90.0,
        "back" | "strain_back" => 180.0,
        "three_quarter" => 35.0,
        _ => return None,
    };
    if body.vertices.iter().any(|v| !v.is_finite())
        || garment.vertices.iter().any(|v| !v.is_finite())
    {
        return None;
    }
    let zs = |m: &ColouredMesh| {
        m.vertices
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
                (a.min(v.z), b.max(v.z))
            })
    };
    let (blo, bhi) = zs(body);
    let (glo, ghi) = zs(garment);
    if ![blo, bhi, glo, ghi].iter().all(|v| v.is_finite()) || blo > bhi || glo > ghi {
        return None;
    }
    let tall = bhi - blo;
    let zc = (0.5 * (glo + ghi))
        .max(blo + 0.25 * tall)
        .min(blo + 0.8 * tall);
    if !zc.is_finite() {
        return None;
    }
    let (s, c) = det::sin_cos(az.to_radians());
    Some(Camera {
        eye: Vec3::new(2.0 * s, -2.0 * c, zc + 0.12),
        target: Vec3::new(0.0, 0.0, zc),
        focal_over_sensor: 50.0 / 36.0,
    })
}

/// First visible surface along the exact ray used for an unscaled native still.
#[derive(Debug, PartialEq)]
pub enum StillHit {
    /// The ray missed both meshes.
    Background,
    /// The body occludes any cloth behind the first hit.
    Body,
    /// The first hit belongs to the garment endpoint.
    Garment {
        /// Native garment vertex indices, before the renderer concatenates body and cloth.
        vertices: [usize; 3],
        /// Intersection in the posed, z-up simulation frame.
        position_m: Vec3,
    },
}

/// Pixels start at the upper-left. `None` denotes an unknown view or coordinates outside the image.
#[must_use]
pub fn pick_still(
    body: &ColouredMesh,
    garment: &ColouredMesh,
    view: &str,
    size: [usize; 2],
    pixel: [usize; 2],
) -> Option<StillHit> {
    if pixel[0] >= size[0] || pixel[1] >= size[1] {
        return None;
    }
    let cam = still_camera(body, garment, view)?;
    let d = cam.pixel_rays(size)(pixel[0], pixel[1]);
    let scene = Scene::new(&[body, garment]);
    let hit = scene.cast(
        [cam.eye.x, cam.eye.y, cam.eye.z],
        [d.x, d.y, d.z],
        f64::INFINITY,
        false,
    );
    Some(match hit {
        None => StillHit::Background,
        Some(h) if h.tri < scene.first[1] => StillHit::Body,
        Some(h) => StillHit::Garment {
            vertices: scene.tris[h.tri].map(|v| v - body.vertices.len()),
            position_m: cam.eye + d * h.t,
        },
    })
}

/// The reference's six stills of a drape. Construction and strain use the same geometry.
#[must_use]
pub fn stills(
    body: &ColouredMesh,
    garment: &ColouredMesh,
    strain: &ColouredMesh,
    width: usize,
    height: usize,
) -> Vec<(String, Vec<u8>)> {
    stills_shaded(body, garment, strain, [width, height], Shading::Standard)
}

/// The six stills in `shading`, named `still_<view><suffix>.png` with [`Shading::suffix`], so that a
/// legible set sits beside the standard one and replaces none of it.
#[must_use]
pub fn stills_shaded(
    body: &ColouredMesh,
    garment: &ColouredMesh,
    strain: &ColouredMesh,
    [width, height]: [usize; 2],
    shading: Shading,
) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::with_capacity(6);
    for name in [
        "front",
        "side",
        "back",
        "three_quarter",
        "strain_front",
        "strain_back",
    ] {
        let of_strain = name.starts_with("strain_");
        let cloth = if of_strain { strain } else { garment };
        let Some(cam) = still_camera(body, garment, name) else {
            return Vec::new();
        };
        let rgb = still(body, cloth, cam, [width, height], shading, of_strain);
        out.push((
            format!("still_{name}{}.png", shading.suffix()),
            png(width, height, &rgb),
        ));
    }
    out
}

/// A schematic felt fez (spec 04 §9.9), from finished radii/height in metres. Its base is open;
/// the wall is 2 mm radially inward and the top cap 2 mm thick. It is not a simulated hat.
pub fn fez_frustum(
    base: Vec3,
    base_radius: f64,
    top_radius: f64,
    height: f64,
) -> Result<ColouredMesh, Refusal> {
    let thickness = 0.002;
    if !base.is_finite()
        || base.x.abs().max(base.y.abs()).max(base.z.abs()) > 1e6
        || [base_radius, top_radius, height]
            .iter()
            .any(|v| !v.is_finite() || *v <= thickness || *v > 10.0)
        || top_radius >= base_radius
    {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "fez needs finite radii/height above 2 mm, at most 10 m, with a smaller top radius",
        ));
    }
    const N: usize = 64;
    let inner_height = height - thickness;
    let inner_top = base_radius + (top_radius - base_radius) * inner_height / height - thickness;
    let mut mesh = ColouredMesh::default();
    for (radius, z) in [
        (base_radius, 0.0),
        (top_radius, height),
        (base_radius - thickness, 0.0),
        (inner_top, inner_height),
    ] {
        for i in 0..N {
            let (s, c) = det::sin_cos(2.0 * std::f64::consts::PI * i as f64 / N as f64);
            mesh.vertices
                .push(base + Vec3::new(radius * c, radius * s, z));
        }
    }
    mesh.vertices.extend([
        base + Vec3::new(0.0, 0.0, height),
        base + Vec3::new(0.0, 0.0, inner_height),
    ]);
    for i in 0..N {
        let j = (i + 1) % N;
        mesh.triangles.extend([
            [i, j, N + j],
            [i, N + j, N + i],
            [2 * N + i, 3 * N + j, 2 * N + j],
            [2 * N + i, 3 * N + i, 3 * N + j],
            [i, 2 * N + j, j],
            [i, 2 * N + i, 2 * N + j],
            [4 * N, N + i, N + j],
            [4 * N + 1, 3 * N + j, 3 * N + i],
        ]);
    }
    mesh.colours = vec![[0.55, 0.05, 0.05]; mesh.vertices.len()];
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fez_shell_has_finished_dimensions_and_consistently_closed_wall() {
        let base = Vec3::new(0.01, -0.02, 0.85);
        let m = fez_frustum(base, 0.08, 0.06, 0.11).unwrap();
        assert_eq!((m.vertices.len(), m.triangles.len()), (258, 512));
        assert!((m.vertices[0] - base - Vec3::new(0.08, 0.0, 0.0)).norm() < 1e-15);
        assert!((m.vertices[64].z - 0.96).abs() < 1e-15);
        let mut edges = std::collections::BTreeMap::<(usize, usize), (usize, i32)>::new();
        for t in &m.triangles {
            assert!(
                (m.vertices[t[1]] - m.vertices[t[0]])
                    .cross(m.vertices[t[2]] - m.vertices[t[0]])
                    .norm()
                    > 0.0
            );
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let e = edges.entry((a.min(b), a.max(b))).or_default();
                e.0 += 1;
                e.1 += if a < b { 1 } else { -1 };
            }
        }
        assert!(edges.values().all(|e| *e == (2, 0)));
        assert!(fez_frustum(base, 0.001, 0.0005, 0.1).is_err());
        assert!(fez_frustum(base, 0.1, 0.2, 0.1).is_err());
        assert!(fez_frustum(base, 0.1, 0.08, f64::NAN).is_err());
    }

    fn quad(z: f64, colour: [f64; 3]) -> ColouredMesh {
        ColouredMesh {
            vertices: vec![
                Vec3::new(-0.5, 0.0, z - 0.5),
                Vec3::new(0.5, 0.0, z - 0.5),
                Vec3::new(0.5, 0.0, z + 0.5),
                Vec3::new(-0.5, 0.0, z + 0.5),
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            colours: vec![colour; 4],
        }
    }

    #[test]
    fn a_square_facing_the_camera_fills_the_middle() {
        let red = quad(1.0, [0.8, 0.1, 0.1]);
        let cam = Camera {
            eye: Vec3::new(0.0, -2.0, 1.0),
            target: Vec3::new(0.0, 0.0, 1.0),
            focal_over_sensor: 50.0 / 36.0,
        };
        let img = render(&[&red], cam, 40, 30);
        let at = |x: usize, y: usize| &img[(y * 40 + x) * 3..(y * 40 + x) * 3 + 3];
        let (mid, corner) = (at(20, 15), at(0, 0));
        assert!(mid[0] > 2 * mid[1] && mid[0] > 60, "{mid:?}");
        assert!(
            corner[0] == corner[1] || corner[0].abs_diff(corner[1]) < 3,
            "{corner:?}"
        );
        // the same bits on one thread and on several
        assert_eq!(img, render(&[&red], cam, 40, 30));
    }

    /// A lit, shadowed and occluded scene small enough to pin: a strip standing on a floor in front of a
    /// wall, seen from the three-quarter side so that pixels miss, hit lit cloth, hit cloth in the strip's
    /// shadow and hit creases where the ambient rays are blocked.
    fn pinned_scene() -> (ColouredMesh, ColouredMesh, Camera) {
        let mut body = quad(0.5, [0.0; 3]);
        body.colours.clear(); // skin tone, as a body is read
        for v in &mut body.vertices {
            v.y += 0.12;
        }
        let mut cloth = ColouredMesh::default();
        // a 6 x 6 sheet folded into three pleats along x, coloured by column
        for j in 0..=6 {
            for i in 0..=6 {
                let (x, z) = (-0.3 + 0.1 * i as f64, 0.2 + 0.1 * j as f64);
                let y = if i % 2 == 0 { 0.0 } else { -0.06 };
                cloth.vertices.push(Vec3::new(x, y, z));
                cloth
                    .colours
                    .push([0.2 + 0.1 * i as f64, 0.35, 0.8 - 0.1 * i as f64]);
            }
        }
        for j in 0..6 {
            for i in 0..6 {
                let a = j * 7 + i;
                cloth
                    .triangles
                    .extend([[a, a + 1, a + 8], [a, a + 8, a + 7]]);
            }
        }
        let cam = Camera {
            eye: Vec3::new(0.9, -1.6, 0.75),
            target: Vec3::new(0.0, 0.0, 0.5),
            focal_over_sensor: 50.0 / 36.0,
        };
        (body, cloth, cam)
    }

    #[test]
    fn the_standard_still_keeps_the_bits_it_had_before_the_legible_mode_existed() {
        let (body, cloth, cam) = pinned_scene();
        let img = render(&[&body, &cloth], cam, 96, 72);
        assert_eq!(img.len(), 96 * 72 * 3);
        // CRC-32 of the 8-bit RGB rows, taken from the renderer as it stood at commit a1268bba, its last
        // change before the legible mode.
        assert_eq!(crc32(&img), 0x0c4d_f707, "crc {:#010x}", crc32(&img));
        // the standard shading is that picture whichever door it is asked through, strain colours or not
        for strain in [false, true] {
            let same = still(&body, &cloth, cam, [96, 72], Shading::Standard, strain);
            assert_eq!(same, img);
        }
    }

    #[test]
    fn the_legible_still_is_the_same_bits_every_time_and_softens_a_silhouette() {
        let (body, cloth, cam) = pinned_scene();
        let draw = || still(&body, &cloth, cam, [96, 72], Shading::Legible, false);
        let legible = draw();
        assert_eq!(legible.len(), 96 * 72 * 3);
        assert_eq!(legible, draw());
        assert_ne!(legible, render(&[&body, &cloth], cam, 96, 72));

        // one flat grey square on the backdrop: the standard still has two greys along a row that
        // crosses its side, the legible still a third between them where the side cuts a pixel
        let grey = quad(1.0, [0.5; 3]);
        let nothing = ColouredMesh::default();
        let cam = Camera {
            eye: Vec3::new(0.013, -2.0, 1.0),
            target: Vec3::new(0.013, 0.0, 1.0),
            focal_over_sensor: 50.0 / 36.0,
        };
        let row = |shading| -> Vec<u8> {
            let img = still(&nothing, &grey, cam, [64, 48], shading, true);
            (0..64).map(|x| img[(24 * 64 + x) * 3]).collect()
        };
        let (hard, soft) = (row(Shading::Standard), row(Shading::Legible));
        let (backdrop, square) = (hard[0], hard[32]);
        assert_ne!(backdrop, square);
        assert!(hard.iter().all(|&v| v == backdrop || v == square));
        let between = |v: u8| v > backdrop.min(square) && v < backdrop.max(square);
        assert!(soft.iter().any(|&v| between(v)), "{soft:?}");
        // away from the side the two stills agree: a strain still gets no highlight and no line
        assert_eq!((soft[0], soft[32]), (backdrop, square));
    }

    #[test]
    fn the_ambient_directions_are_64_different_unit_vectors_above_the_surface() {
        let mut all: Vec<[f64; 3]> = Vec::new();
        for sample in 0..SUB_PIXELS.len() {
            let dirs = stratified_hemisphere(sample);
            assert_eq!(dirs.len(), 16);
            all.extend(dirs);
        }
        // a cosine-weighted hemisphere has mean height 2/3, and the pixel's 64 directions keep it
        let mean = all.iter().map(|d| d[2]).sum::<f64>() / 64.0;
        assert!((mean - 2.0 / 3.0).abs() < 0.005, "{mean}");
        for d in &all {
            let length2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
            assert!((length2 - 1.0).abs() < 1e-12 && d[2] > 0.0, "{d:?}");
        }
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                let apart = (0..3).map(|k| (a[k] - b[k]).abs()).fold(0.0, f64::max);
                assert!(apart > 1e-3, "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn the_highlight_grows_at_a_glance_and_is_added_to_cloth_only() {
        let n = Vec3::new(0.0, 0.0, 1.0);
        let l = Vec3::new(0.6, 0.0, 0.8);
        let facing = grazing(n, n, l, n.dot(l));
        // seen edge-on from the light's side, as a fold turning away from a camera beside its light is
        let glancing = grazing(n, Vec3::new(0.995, 0.0, 0.1).normalized(), l, n.dot(l));
        assert!(
            facing > 0.0 && glancing > 20.0 * facing,
            "{facing} {glancing}"
        );
        assert!(glancing.is_finite() && glancing < 1.0);
        // nothing from a light the surface does not see, and nothing at the silhouette itself blows up
        assert_eq!(grazing(n, n, l, 0.0), 0.0);
        assert!(grazing(n, Vec3::new(1.0, 0.0, 0.0), l, n.dot(l)).is_finite());

        // the same slanted sheet, once as the cloth and once as the body
        let mut sheet = quad(1.0, [0.3, 0.3, 0.6]);
        for v in &mut sheet.vertices {
            v.y = 3.0 * v.x; // turned most of the way edge-on to a camera on -y
        }
        let far = ColouredMesh {
            vertices: vec![
                Vec3::new(50.0, 50.0, 50.0),
                Vec3::new(50.1, 50.0, 50.0),
                Vec3::new(50.0, 50.0, 50.1),
            ],
            triangles: vec![[0, 1, 2]],
            colours: Vec::new(),
        };
        let cam = Camera {
            eye: Vec3::new(0.0, -2.0, 1.0),
            target: Vec3::new(0.0, 0.0, 1.0),
            focal_over_sensor: 50.0 / 36.0,
        };
        let middle = |img: Vec<u8>| -> [u8; 3] {
            let at = (24 * 64 + 32) * 3;
            [img[at], img[at + 1], img[at + 2]]
        };
        let as_cloth = middle(still(&far, &sheet, cam, [64, 48], Shading::Legible, false));
        let as_strain = middle(still(&far, &sheet, cam, [64, 48], Shading::Legible, true));
        let as_body = middle(still(&sheet, &far, cam, [64, 48], Shading::Legible, false));
        assert!(
            (0..3).all(|k| as_cloth[k] > as_body[k]),
            "{as_cloth:?} {as_body:?}"
        );
        assert_eq!(as_strain, as_body);
    }

    /// A flat `cells` × `cells` sheet of side `side` (m) in the plane y = 0, its lower-left corner at
    /// (0, 0, 0); each cell is cut along the diagonal from its lower-left corner.
    fn sheet(cells: usize, side: f64, colour: [f64; 3]) -> ColouredMesh {
        let mut m = ColouredMesh::default();
        let step = side / cells as f64;
        for j in 0..=cells {
            for i in 0..=cells {
                m.vertices
                    .push(Vec3::new(step * i as f64, 0.0, step * j as f64));
                m.colours.push(colour);
            }
        }
        let w = cells + 1;
        for j in 0..cells {
            for i in 0..cells {
                let a = j * w + i;
                m.triangles
                    .extend([[a, a + 1, a + w + 1], [a, a + w + 1, a + w]]);
            }
        }
        m
    }

    /// `left` with `right` beside it, moved `shift` (m) along x and keeping vertices of its own: where
    /// the two meet they share positions and no vertex, as the two sides of a seam do in `garment.obj`.
    fn sewn_beside(mut left: ColouredMesh, right: &ColouredMesh, shift: f64) -> ColouredMesh {
        let base = left.vertices.len();
        left.triangles
            .extend(right.triangles.iter().map(|t| t.map(|v| v + base)));
        left.vertices.extend(
            right
                .vertices
                .iter()
                .map(|v| Vec3::new(v.x + shift, v.y, v.z)),
        );
        left.colours.extend(right.colours.iter().copied());
        left
    }

    #[test]
    fn open_edges_are_hems_and_openings_and_never_a_seam_or_the_body() {
        // an open body triangle, then a 3 x 3 sheet: 12 edges round the sheet, none of the body's
        let body = ColouredMesh {
            vertices: vec![
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(1.0, 1.0, 0.0),
                Vec3::new(0.0, 1.0, 1.0),
            ],
            triangles: vec![[0, 1, 2]],
            colours: Vec::new(),
        };
        let cloth = sheet(3, 0.03, [0.5; 3]);
        let scene = Scene::new(&[&body, &cloth]);
        assert_eq!(scene.first, [0, 1]);
        let open = scene.open_edges(scene.first[1]);
        assert_eq!(open.edges.len(), 12);
        assert!(open.edges.iter().all(|e| e[0] >= 3 && e[1] >= 3));
        assert_eq!(open.at.len(), 24);
        assert_eq!(open.start[..4], [0; 4]);
        // each of the sheet's 12 rim vertices ends two of them, each of its 4 inner vertices none
        let ends = |v: usize| open.start[v + 1] - open.start[v];
        let rim = |i: usize, j: usize| i == 0 || i == 3 || j == 0 || j == 3;
        for j in 0..4 {
            for i in 0..4 {
                assert_eq!(ends(3 + j * 4 + i), if rim(i, j) { 2 } else { 0 });
            }
        }

        // cell (0, 1) on the left side: its first triangle only touches the rim at a corner, and a point
        // a tenth of a cell in from that corner is a tenth of a cell from the rim
        let touching = 1 + 2 * 3;
        assert_eq!(scene.tris[touching], [3 + 4, 3 + 5, 3 + 9]);
        let near = Hit {
            t: 1.0,
            tri: touching,
            bary: [0.9, 0.05, 0.05],
        };
        let d = scene.open_edge_distance(&open, &near).unwrap();
        assert!((d - 0.001).abs() < 1e-12, "{d}");
        // the middle cell touches no rim vertex
        let inner = Hit {
            t: 1.0,
            tri: 1 + 2 * 4,
            bary: [0.3, 0.3, 0.4],
        };
        assert_eq!(scene.open_edge_distance(&open, &inner), None);

        // two squares sewn side by side: of their 8 rim edges the two on the seam are not open, and the
        // 6 that are open are the outline of the pair
        let square = sheet(1, 0.01, [0.5; 3]);
        let pair = sewn_beside(square.clone(), &square, 0.01);
        let scene = Scene::new(&[&body, &pair]);
        let open = scene.open_edges(1);
        assert_eq!(open.edges.len(), 6);
        let on_seam = |v: usize| scene.p[v][0] == 0.01;
        assert!(open.edges.iter().all(|e| !(on_seam(e[0]) && on_seam(e[1]))));
        // moved a hair apart, the two sides are no longer one place and both are open
        let apart = sewn_beside(square.clone(), &square, 0.010_001);
        assert_eq!(Scene::new(&[&body, &apart]).open_edges(1).edges.len(), 8);
    }

    #[test]
    fn the_legible_still_draws_a_dark_line_inside_a_hem_and_none_at_a_seam() {
        // a 10 cm sheet seen square on from 0.3 m: about 1.1 mm a pixel at 192 across
        let cloth = sheet(8, 0.1, [0.5; 3]);
        let nothing = ColouredMesh::default();
        let cam = Camera {
            eye: Vec3::new(0.05, -0.3, 0.05),
            target: Vec3::new(0.05, 0.0, 0.05),
            focal_over_sensor: 50.0 / 36.0,
        };
        let row = |shading, strain| -> Vec<u8> {
            let img = still(&nothing, &cloth, cam, [192, 144], shading, strain);
            (0..192).map(|x| img[(72 * 192 + x) * 3]).collect()
        };
        let hard = row(Shading::Standard, false);
        let first = hard.iter().position(|&v| v != hard[0]).unwrap();
        let lined = row(Shading::Legible, false);
        // the second pixel inside the sheet is wholly on it and within the 5 mm line; the middle is not
        assert!(lined[first + 1] + 8 < lined[96], "{lined:?}");
        // the line fades out by 5 mm (under five pixels) and the far side has one too
        assert!(lined[first + 6].abs_diff(lined[96]) <= 1, "{lined:?}");
        let last = 191 - hard.iter().rev().position(|&v| v != hard[0]).unwrap();
        assert!(lined[last - 1] + 8 < lined[96], "{lined:?}");
        // none where the colours are the strain scale
        let plain = row(Shading::Legible, true);
        assert_eq!(plain[first + 1], plain[96]);

        // a second sheet sewn to the right side: that side is now a seam and loses its line on both
        // sides, while the left side is still a hem
        let sewn = sewn_beside(cloth.clone(), &cloth, 0.1);
        let img = still(&nothing, &sewn, cam, [192, 144], Shading::Legible, false);
        let seamed: Vec<u8> = (0..192).map(|x| img[(72 * 192 + x) * 3]).collect();
        for x in [last - 1, last, last + 1, last + 2] {
            assert!(seamed[x].abs_diff(seamed[96]) <= 2, "{x} {seamed:?}");
        }
        assert!(seamed[first + 1] + 8 < seamed[96], "{seamed:?}");
    }

    #[test]
    fn png_is_well_formed() {
        let p = png(2, 1, &[255, 0, 0, 0, 255, 0]);
        assert_eq!(&p[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&p[12..16], b"IHDR");
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        // IDAT data: after the signature, IHDR (25 bytes) and IDAT's length and type; before its CRC and IEND
        let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&p[41..p.len() - 4 - 12])
            .unwrap_or_default();
        assert_eq!(raw, [0, 255, 0, 0, 0, 255, 0]);
    }

    #[test]
    fn obj_round_trips_with_colours() {
        let m = read_obj("v 0 0 0 1 0 0\nv 1 0 0 0 1 0\nv 0 1 0 0 0 1\nf 1 2 3\n").unwrap();
        assert_eq!(
            (m.vertices.len(), m.triangles.len(), m.colours[2]),
            (3, 1, [0.0, 0.0, 1.0])
        );
        assert!(read_obj("v 0 0 0\nf 1 2 3\n").is_err());
        assert!(read_obj("v NaN 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").is_err());
        assert!(read_obj("v 0 0 0 inf 0 0\nv 1 0 0 0 1 0\nv 0 1 0 0 0 1\nf 1 2 3\n").is_err());
    }

    #[test]
    fn slab_hits_axis_aligned_ray_on_box_boundary_and_zero_dim_or_empty_inputs_do_not_panic() {
        // Non-flat triangle spanning x in [0, 1], y in [0, 0], z in [0, 1].
        let mesh = ColouredMesh {
            vertices: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            ],
            triangles: vec![[0, 1, 2]],
            colours: vec![[0.5, 0.5, 0.5]; 3],
        };
        let scene = Scene::new(&[&mesh]);
        // Axis-aligned rays with d.x == 0 and origin exactly on x == lo.x (0.0) or x == hi.x (1.0)
        // must not produce 0 * inf = NaN false misses in Scene::slab.
        assert!(
            scene
                .cast([0.0, -1.0, 0.25], [0.0, 1.0, 0.0], f64::INFINITY, false)
                .is_some(),
            "ray on lower x slab face must hit the triangle edge"
        );
        assert!(
            scene
                .cast([1.0, -1.0, 0.0], [0.0, 1.0, 0.0], f64::INFINITY, false)
                .is_some(),
            "ray on upper x slab face must hit the triangle corner"
        );
        // Zero-dimension render/png and empty-mesh still_camera/stills must never panic.
        let cam = Camera {
            eye: Vec3::new(0.0, -2.0, 0.5),
            target: Vec3::new(0.0, 0.0, 0.5),
            focal_over_sensor: 50.0 / 36.0,
        };
        assert!(render(&[&mesh], cam, 0, 10).is_empty());
        assert!(render(&[&mesh], cam, 10, 0).is_empty());
        assert!(!png(0, 4, &[]).is_empty());
        let empty = ColouredMesh::default();
        assert!(still_camera(&empty, &mesh, "front").is_none());
        assert!(still_camera(&mesh, &empty, "front").is_none());
        assert!(stills(&empty, &mesh, &mesh, 16, 16).is_empty());

        // Non-finite vertices must not be masked by f64::min/max in still_camera.
        let mut nan_mesh = mesh.clone();
        nan_mesh.vertices[1].z = f64::NAN;
        assert!(still_camera(&nan_mesh, &mesh, "front").is_none());
        assert!(still_camera(&mesh, &nan_mesh, "front").is_none());

        // pick_still must index garment hits via Scene::first[1] even when an out-of-range
        // triangle was filtered from body or garment during Scene::new.
        let bad_body = ColouredMesh {
            vertices: vec![
                Vec3::new(-2.0, 2.0, 0.0),
                Vec3::new(2.0, 2.0, 0.0),
                Vec3::new(0.0, 2.0, 1.0),
            ],
            triangles: vec![[0, 1, 99], [0, 1, 2]],
            colours: vec![[0.5; 3]; 3],
        };
        let bad_garment = ColouredMesh {
            vertices: vec![
                Vec3::new(-1.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            ],
            triangles: vec![[0, 1, 42], [0, 1, 2]],
            colours: vec![[0.5; 3]; 3],
        };
        let picked = pick_still(&bad_body, &bad_garment, "front", [32, 32], [16, 16]);
        assert!(
            matches!(
                picked,
                Some(StillHit::Garment {
                    vertices: [0, 1, 2],
                    ..
                })
            ),
            "expected native garment triangle [0, 1, 2], got {picked:?}"
        );
    }
}
