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
                if v.len() < 3 {
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
}

struct Hit {
    t: f64,
    tri: usize,
    bary: [f64; 3],
}

impl Scene {
    fn new(meshes: &[&ColouredMesh]) -> Self {
        let (mut p, mut tris, mut colours) = (Vec::new(), Vec::new(), Vec::new());
        for m in meshes {
            let base = p.len();
            p.extend(m.vertices.iter().map(|v| [v.x, v.y, v.z]));
            tris.extend(
                m.triangles
                    .iter()
                    .map(|t| [t[0] + base, t[1] + base, t[2] + base]),
            );
            if m.colours.is_empty() {
                colours.extend(std::iter::repeat_n(SKIN, m.vertices.len()));
            } else {
                colours.extend(m.colours.iter().copied());
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
    let scene = Scene::new(meshes);
    let fwd = (cam.target - cam.eye).normalized();
    let right = fwd.cross(Vec3::new(0.0, 0.0, 1.0)).normalized();
    let up = right.cross(fwd);
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
    let half_w = 0.5 / cam.focal_over_sensor;
    let aspect = height as f64 / width as f64;
    let pixel = |x: usize, y: usize| -> [u8; 3] {
        let px = ((x as f64 + 0.5) / width as f64 * 2.0 - 1.0) * half_w;
        let py = (1.0 - (y as f64 + 0.5) / height as f64 * 2.0) * half_w * aspect;
        let d = (fwd + right * px + up * py).normalized();
        let o = [cam.eye.x, cam.eye.y, cam.eye.z];
        let dd = [d.x, d.y, d.z];
        let rgb = match scene.cast(o, dd, f64::INFINITY, false) {
            None => {
                let g = 0.32 + 0.18 * (1.0 - y as f64 / height as f64);
                [g, g, g * 1.02]
            }
            Some(h) => {
                let t = scene.tris[h.tri];
                let mut n = Vec3::ZERO;
                let mut col = [0.0; 3];
                for (k, &v) in t.iter().enumerate() {
                    n += scene.normals[v] * h.bary[k];
                    for (c, &cv) in col.iter_mut().zip(&scene.colours[v]) {
                        *c += cv * h.bary[k];
                    }
                }
                let mut n = n.normalized();
                if n.dot(d) > 0.0 {
                    n = -n;
                }
                let p = cam.eye + d * h.t;
                let lift = p + n * 1e-4;
                let lo = [lift.x, lift.y, lift.z];
                let lit = |l: Vec3| -> f64 {
                    let c = n.dot(l);
                    if c <= 0.0
                        || scene
                            .cast(lo, [l.x, l.y, l.z], f64::INFINITY, true)
                            .is_some()
                    {
                        0.0
                    } else {
                        c
                    }
                };
                // ambient occlusion over 0.1 m in the frame of n
                let a = if n.x.abs() < 0.9 {
                    Vec3::new(1.0, 0.0, 0.0)
                } else {
                    Vec3::new(0.0, 1.0, 0.0)
                };
                let tu = n.cross(a).normalized();
                let tv = n.cross(tu);
                let open = ao
                    .iter()
                    .filter(|s| {
                        let w = tu * s[0] + tv * s[1] + n * s[2];
                        scene.cast(lo, [w.x, w.y, w.z], 0.1, true).is_none()
                    })
                    .count() as f64
                    / ao.len() as f64;
                let light = 0.30 * open + 0.75 * lit(key) + 0.25 * n.dot(fill).max(0.0);
                col.map(|c| c * light)
            }
        };
        rgb.map(srgb8)
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
    for row in rgb.chunks(width * 3) {
        raw.push(0); // filter: none
        raw.extend_from_slice(row);
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

/// The reference's six stills of a drape: (name, PNG). `garment` and `strain` are the same cloth in the
/// seam and in the strain colours.
#[must_use]
pub fn stills(
    body: &ColouredMesh,
    garment: &ColouredMesh,
    strain: &ColouredMesh,
    width: usize,
    height: usize,
) -> Vec<(String, Vec<u8>)> {
    let zs = |m: &ColouredMesh| {
        m.vertices
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
                (a.min(v.z), b.max(v.z))
            })
    };
    let (blo, bhi) = zs(body);
    let (glo, ghi) = zs(garment);
    let tall = bhi - blo;
    let zc = (0.5 * (glo + ghi))
        .max(blo + 0.25 * tall)
        .min(blo + 0.8 * tall);
    let target = Vec3::new(0.0, 0.0, zc);
    let shots: [(&str, f64, &ColouredMesh); 6] = [
        ("front", 0.0, garment),
        ("side", 90.0, garment),
        ("back", 180.0, garment),
        ("three_quarter", 35.0, garment),
        ("strain_front", 0.0, strain),
        ("strain_back", 180.0, strain),
    ];
    shots
        .iter()
        .map(|&(name, az, cloth)| {
            let (s, c) = det::sin_cos(az.to_radians());
            let cam = Camera {
                eye: Vec3::new(2.0 * s, -2.0 * c, zc + 0.12),
                target,
                focal_over_sensor: 50.0 / 36.0,
            };
            let rgb = render(&[body, cloth], cam, width, height);
            (format!("still_{name}.png"), png(width, height, &rgb))
        })
        .collect()
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
    }
}
