//! The fold-up animation and its files (spec 04 §9): the pattern board, each piece's rigid flight off it
//! and isometric roll-up onto its wrap, the zip from wrap to the sewn start, then the solver's frames;
//! written as a PC2 point cache, a mesh map and the `fabric-fold-viewer/1` payload of the browser viewer.
//!
//! Rotations come from Horn's quaternion method (the eigenvector of the largest eigenvalue of a 4 × 4
//! symmetric matrix, with the deterministic Jacobi solver) where the reference calls numpy's SVD Kabsch:
//! the same rotation for these point sets, and a quaternion directly for the slerp.

use std::fmt::Write as _;

use ftx_core::np::linspace;
use ftx_core::{Vec3, det};
use serde_json::{Value, json};

use crate::eigen::jacobi_eigh;
use crate::garment::GarmentMesh;
use crate::start::{MM, WrapPlacement, edge_strain, face_edges, roll_points};

/// Frames of the zip segment (wrap → sewn start).
pub const ZIP_FRAMES: usize = 18;
/// The reference's default fold frames.
pub const FOLD_FRAMES: usize = 70;

/// The pattern board (spec 04 §9.3): instances left to right in order, 40 mm apart, centred on x = 0, on
/// the plane y = −0.55 m, each piece's lowest point at z = 0.15 m.
#[must_use]
pub fn board_layout(mesh: &GarmentMesh) -> Vec<Vec3> {
    let mut out = vec![Vec3::ZERO; mesh.verts2d.len()];
    let mut boxes = Vec::new();
    let mut x = 0.0;
    for &(lo, hi) in &mesh.inst_range {
        let p = &mesh.verts2d[lo..hi];
        let (xmin, xmax) = p
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| {
                (a.min(q.x), b.max(q.x))
            });
        let ymin = p.iter().map(|q| q.y).fold(f64::INFINITY, f64::min);
        boxes.push((lo, hi, x - xmin, ymin));
        x += (xmax - xmin) + 40.0;
    }
    let total = x - 40.0;
    for (lo, hi, xoff, ymin) in boxes {
        for i in lo..hi {
            let q = mesh.verts2d[i];
            out[i] = Vec3::new(
                (q.x + xoff - total / 2.0) * MM,
                -0.55,
                (q.y - ymin) * MM + 0.15,
            );
        }
    }
    out
}

/// A unit quaternion (w, x, y, z).
type Quat = [f64; 4];

fn mean(p: &[Vec3]) -> Vec3 {
    p.iter().fold(Vec3::ZERO, |a, b| a + *b) / p.len().max(1) as f64
}

/// The rotation (as a quaternion) and translation taking point set `a` onto `b` in the least-squares
/// sense (Horn 1987).
fn horn(a: &[Vec3], b: &[Vec3]) -> (Quat, Vec3) {
    let (ca, cb) = (mean(a), mean(b));
    let mut s = [[0.0; 3]; 3];
    for (p, q) in a.iter().zip(b) {
        let (u, v) = (*p - ca, *q - cb);
        let (u, v) = ([u.x, u.y, u.z], [v.x, v.y, v.z]);
        for i in 0..3 {
            for j in 0..3 {
                s[i][j] += u[i] * v[j];
            }
        }
    }
    let n = [
        [
            s[0][0] + s[1][1] + s[2][2],
            s[1][2] - s[2][1],
            s[2][0] - s[0][2],
            s[0][1] - s[1][0],
        ],
        [
            s[1][2] - s[2][1],
            s[0][0] - s[1][1] - s[2][2],
            s[0][1] + s[1][0],
            s[2][0] + s[0][2],
        ],
        [
            s[2][0] - s[0][2],
            s[0][1] + s[1][0],
            -s[0][0] + s[1][1] - s[2][2],
            s[1][2] + s[2][1],
        ],
        [
            s[0][1] - s[1][0],
            s[2][0] + s[0][2],
            s[1][2] + s[2][1],
            -s[0][0] - s[1][1] + s[2][2],
        ],
    ];
    let flat: Vec<f64> = n.iter().flatten().copied().collect();
    let (_, vecs) = jacobi_eigh(&flat, 4);
    let mut q: Quat = std::array::from_fn(|i| vecs[i * 4 + 3]);
    if q[0] < 0.0 {
        q = q.map(|v| -v);
    }
    let t = cb - rotate(q, ca);
    (q, t)
}

fn rotate(q: Quat, v: Vec3) -> Vec3 {
    let [w, x, y, z] = q;
    let n = (w * w + x * x + y * y + z * z).sqrt();
    let (w, x, y, z) = (w / n, x / n, y / n, z / n);
    Vec3::new(
        (1.0 - 2.0 * (y * y + z * z)) * v.x
            + 2.0 * (x * y - z * w) * v.y
            + 2.0 * (x * z + y * w) * v.z,
        2.0 * (x * y + z * w) * v.x
            + (1.0 - 2.0 * (x * x + z * z)) * v.y
            + 2.0 * (y * z - x * w) * v.z,
        2.0 * (x * z - y * w) * v.x
            + 2.0 * (y * z + x * w) * v.y
            + (1.0 - 2.0 * (x * x + y * y)) * v.z,
    )
}

/// The reference's slerp (normalised lerp above a dot of 0.9995).
fn slerp(q0: Quat, q1: Quat, t: f64) -> Quat {
    let mut d: f64 = (0..4).map(|i| q0[i] * q1[i]).sum();
    let q1 = if d < 0.0 {
        d = -d;
        q1.map(|v| -v)
    } else {
        q1
    };
    if d > 0.9995 {
        let q: Quat = std::array::from_fn(|i| q0[i] + t * (q1[i] - q0[i]));
        let n = q.iter().map(|v| v * v).sum::<f64>().sqrt();
        return q.map(|v| v / n);
    }
    let th = det::acos(d);
    let (a, b, s) = (det::sin((1.0 - t) * th), det::sin(t * th), det::sin(th));
    std::array::from_fn(|i| (a * q0[i] + b * q1[i]) / s)
}

/// The fold segment (spec 04 §9.3): per instance, frames before its staggered start stay on the board;
/// then a rigid flight (slerped rotation, centroid along a raised path) for u < 0.55 and the isometric
/// roll-up after.
#[must_use]
pub fn fold_frames(
    mesh: &GarmentMesh,
    places: &[WrapPlacement],
    board: &[Vec3],
    n_frames: usize,
) -> Vec<Vec<Vec3>> {
    let mut frames = vec![board.to_vec(); n_frames];
    let count = mesh.inst_range.len();
    for (idx, (&(lo, hi), p)) in mesh.inst_range.iter().zip(places).enumerate() {
        let pts = &mesh.verts2d[lo..hi];
        let flat = roll_points(pts, p, 0.0);
        let (q1, t) = horn(&board[lo..hi], &flat);
        let q0 = [1.0, 0.0, 0.0, 0.0];
        let c0 = mean(&board[lo..hi]);
        let target = rotate(q1, c0) + t;
        let start = if count > 1 {
            (idx as f64 * 0.35 * n_frames as f64 / (count - 1) as f64) as usize
        } else {
            0
        };
        let dur = n_frames.saturating_sub(start);
        for (f, frame) in frames.iter_mut().enumerate().skip(start) {
            let u = (f - start) as f64 / dur.saturating_sub(1).max(1) as f64;
            if u < 0.55 {
                let w = 0.5 - 0.5 * det::cos(std::f64::consts::PI * u / 0.55);
                let qf = slerp(q0, q1, w);
                let cf = c0
                    + (target - c0) * w
                    + Vec3::new(0.0, 0.0, 0.08 * det::sin(std::f64::consts::PI * w));
                for i in lo..hi {
                    frame[i] = rotate(qf, board[i] - c0) + cf;
                }
            } else {
                let k = 0.5 - 0.5 * det::cos(std::f64::consts::PI * (u - 0.55) / 0.45);
                frame[lo..hi].copy_from_slice(&roll_points(pts, p, k));
            }
        }
    }
    frames
}

/// The zip segment: `wrapped + w·(start − wrapped)` with `w = ½ − ½cos(πt)` over [`ZIP_FRAMES`] frames.
#[must_use]
pub fn zip_frames(wrapped: &[Vec3], start: &[Vec3]) -> Vec<Vec<Vec3>> {
    linspace(0.0, 1.0, ZIP_FRAMES)
        .into_iter()
        .map(|t| {
            let w = 0.5 - 0.5 * det::cos(std::f64::consts::PI * t);
            wrapped
                .iter()
                .zip(start)
                .map(|(a, b)| *a + (*b - *a) * w)
                .collect()
        })
        .collect()
}

/// The ten seam colours (spec 04 §9.4).
pub const SEAM_COLORS: [[f64; 3]; 10] = [
    [0.95, 0.75, 0.10],
    [0.10, 0.80, 0.90],
    [0.95, 0.30, 0.80],
    [0.30, 0.95, 0.30],
    [1.00, 0.50, 0.10],
    [0.60, 0.50, 1.00],
    [0.00, 0.50, 0.45],
    [0.90, 0.20, 0.20],
    [0.50, 0.30, 0.10],
    [0.15, 0.30, 0.90],
];

/// `seam_rgb` (spec 04 §9.4): each sewn vertex in its seam family's colour, the rest in the base colour
/// (navy for a bolero style, red otherwise).
#[must_use]
pub fn seam_colours(mesh: &GarmentMesh, style: &str) -> Vec<[f64; 3]> {
    let base = if style.contains("BOLERO") {
        [0.07, 0.10, 0.30]
    } else {
        [0.55, 0.06, 0.06]
    };
    let family = |id: &str| id.rsplit_once('_').map_or(id, |(a, _)| a).to_string();
    let mut names: Vec<String> = mesh.seams.iter().map(|s| family(&s.id)).collect();
    names.sort();
    names.dedup();
    let mut col = vec![base; mesh.verts2d.len()];
    for s in &mesh.seams {
        let k = names.iter().position(|n| *n == family(&s.id)).unwrap_or(0);
        for &(a, b) in &s.pairs {
            col[a] = SEAM_COLORS[k % SEAM_COLORS.len()];
            col[b] = SEAM_COLORS[k % SEAM_COLORS.len()];
        }
    }
    col
}

/// `strain_rgb` (spec 04 §9.4): per vertex the mean strain of its edges at `x`, white at 0, red at +3 %,
/// blue at −3 %.
#[must_use]
pub fn strain_colours(mesh: &GarmentMesh, x: &[Vec3]) -> Vec<[f64; 3]> {
    let edges = face_edges(&mesh.faces);
    let strain = edge_strain(x, &mesh.verts2d, &edges);
    let (mut sum, mut count) = (vec![0.0; x.len()], vec![0usize; x.len()]);
    for (&(a, b), e) in edges.iter().zip(&strain) {
        sum[a] += e;
        sum[b] += e;
        count[a] += 1;
        count[b] += 1;
    }
    (0..x.len())
        .map(|i| {
            let v = if count[i] > 0 {
                sum[i] / count[i] as f64
            } else {
                0.0
            };
            let t = (v / 0.03).clamp(-1.0, 1.0);
            [
                if t > 0.0 { 1.0 } else { 1.0 + t },
                1.0 - t.abs(),
                if t < 0.0 { 1.0 } else { 1.0 - t },
            ]
        })
        .collect()
}

/// A fold-up: every frame (board → fold → zip → sew → drape) and the segment lengths.
#[derive(Clone, Debug, Default)]
pub struct FoldUp {
    /// Positions per frame (m).
    pub frames: Vec<Vec<Vec3>>,
    /// Fold frames.
    pub fold: usize,
    /// Zip frames.
    pub zip: usize,
    /// Sew frames.
    pub sew: usize,
    /// Drape frames.
    pub drape: usize,
}

/// The PC2 point cache (spec 04 §9.1): float32, frame-major, metres.
#[must_use]
pub fn pc2(frames: &[Vec<Vec3>]) -> Vec<u8> {
    let n = frames.first().map_or(0, Vec::len);
    let mut out = Vec::with_capacity(32 + frames.len() * n * 12);
    out.extend_from_slice(b"POINTCACHE2\0");
    out.extend_from_slice(&1i32.to_le_bytes());
    out.extend_from_slice(&i32::try_from(n).unwrap_or(i32::MAX).to_le_bytes());
    out.extend_from_slice(&1.0f32.to_le_bytes());
    out.extend_from_slice(&1.0f32.to_le_bytes());
    out.extend_from_slice(
        &i32::try_from(frames.len())
            .unwrap_or(i32::MAX)
            .to_le_bytes(),
    );
    for f in frames {
        for p in f {
            for c in [p.x, p.y, p.z] {
                #[allow(clippy::cast_possible_truncation)] // float32 is the format
                out.extend_from_slice(&(c as f32).to_le_bytes());
            }
        }
    }
    out
}

/// `mesh_map.json` (spec 04 §9.2).
#[must_use]
pub fn mesh_map(mesh: &GarmentMesh, fold: &FoldUp) -> Value {
    let inst_range: serde_json::Map<String, Value> = mesh
        .instances
        .iter()
        .zip(&mesh.inst_range)
        .map(|(id, &(lo, hi))| (id.clone(), json!([lo, hi])))
        .collect();
    let corners: serde_json::Map<String, Value> = mesh
        .corner
        .iter()
        .map(|((i, v), g)| (format!("{i}:{v}"), json!(g)))
        .collect();
    let edges: serde_json::Map<String, Value> = mesh
        .index
        .iter()
        .map(|((i, e), idx)| (format!("{i}/{e}"), json!(idx)))
        .collect();
    let seams: serde_json::Map<String, Value> = mesh
        .seams
        .iter()
        .map(|s| {
            (
                s.id.clone(),
                json!({"side_a": s.side_a, "side_b": s.side_b}),
            )
        })
        .collect();
    json!({
        "vertices": mesh.verts2d.len(),
        "inst_range": inst_range,
        "corners": corners,
        "edges": edges,
        "seams": seams,
        "frames": {"fold": fold.fold, "zip": fold.zip, "sew": fold.sew, "drape": fold.drape},
        "units": "m",
    })
}

fn b64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for k in 0..4 {
            if k <= chunk.len() {
                s.push(A[((n >> (18 - 6 * k)) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

/// Everything the viewer shows besides the cloth.
pub struct ViewerExtras<'a> {
    /// Page title (pattern `meta.title`, else `style`).
    pub title: &'a str,
    /// Pattern style (selects the base seam colour).
    pub style: &'a str,
    /// The body: vertices (m) and triangles.
    pub body: (&'a [Vec3], &'a [[usize; 3]]),
    /// Gating flags (texts).
    pub gating: Vec<String>,
    /// Advisory flags (texts).
    pub advisory: Vec<String>,
    /// Whether the drape settled.
    pub settled: bool,
    /// Arm abduction of the pose (degrees).
    pub arm_deg: f64,
}

/// The `fabric-fold-viewer/1` payload (spec 04 §9.7) of a fold-up, keeping at most `keep` frames (the
/// phase starts and the last always).
#[must_use]
pub fn viewer_payload(
    mesh: &GarmentMesh,
    fold: &FoldUp,
    extras: &ViewerExtras,
    keep: usize,
) -> Value {
    let n = fold.frames.len();
    let starts = [
        0,
        fold.fold,
        fold.fold + fold.zip,
        fold.fold + fold.zip + fold.sew,
    ];
    let mut kept: Vec<usize> = linspace(0.0, n.saturating_sub(1) as f64, keep.min(n))
        .into_iter()
        .map(|v| v.round_ties_even() as usize)
        .chain(starts.iter().map(|&s| s.min(n.saturating_sub(1))))
        .chain(std::iter::once(n.saturating_sub(1)))
        .collect();
    kept.sort_unstable();
    kept.dedup();
    let (mut lo, mut hi) = (
        Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
        Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
    );
    for &k in &kept {
        for p in &fold.frames[k] {
            lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
            hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
        }
    }
    let span = [hi.x - lo.x, hi.y - lo.y, hi.z - lo.z].map(|d| d.max(1e-9));
    let (lo3, hi3) = ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]);
    let mut q = Vec::with_capacity(kept.len() * mesh.verts2d.len() * 6);
    for &k in &kept {
        for p in &fold.frames[k] {
            for (i, c) in [p.x, p.y, p.z].into_iter().enumerate() {
                let v = ((c - lo3[i]) / span[i] * 65535.0)
                    .round_ties_even()
                    .clamp(0.0, 65535.0);
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                // clamped to u16
                q.extend_from_slice(&(v as u16).to_le_bytes());
            }
        }
    }
    let tris: Vec<u8> = mesh
        .faces
        .iter()
        .flatten()
        .flat_map(|&i| u32::try_from(i).unwrap_or(u32::MAX).to_le_bytes())
        .collect();
    let rgb = |c: &[[f64; 3]]| -> String {
        let bytes: Vec<u8> = c
            .iter()
            .flatten()
            .map(|v| {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 0..=255
                let b = (v.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8;
                b
            })
            .collect();
        b64(&bytes)
    };
    let last = fold.frames.last().cloned().unwrap_or_default();
    let names = [
        "Flat pattern: the pieces leave the board and roll up",
        "Seams pulled together",
        "Sewn (no gravity yet)",
        "Draped under gravity",
    ];
    let phase: Vec<&str> = kept
        .iter()
        .map(|&k| names[starts.iter().rposition(|&s| s <= k).unwrap_or(0)])
        .collect();
    let body_v: Vec<u8> = extras
        .body
        .0
        .iter()
        .flat_map(|p| [p.x, p.y, p.z])
        .flat_map(|c| {
            #[allow(clippy::cast_possible_truncation)] // float32 is the format
            let f = c as f32;
            f.to_le_bytes()
        })
        .collect();
    let body_t: Vec<u8> = extras
        .body
        .1
        .iter()
        .flatten()
        .flat_map(|&i| u32::try_from(i).unwrap_or(u32::MAX).to_le_bytes())
        .collect();
    let r6 = |v: f64| (v * 1e6).round() / 1e6;
    json!({
        "schema": "fabric-fold-viewer/1",
        "title": extras.title,
        "vertices": mesh.verts2d.len(),
        "frames": kept.len(),
        "source_frames": n,
        "lo": lo3.map(r6),
        "hi": hi3.map(r6),
        "positions": b64(&q),
        "triangles": b64(&tris),
        "colours": {
            "seam_rgb": rgb(&seam_colours(mesh, extras.style)),
            "strain_rgb": rgb(&strain_colours(mesh, &last)),
        },
        "phase": phase,
        "body": {"vertices": b64(&body_v), "triangles": b64(&body_t)},
        "report": {
            "gating": extras.gating,
            "advisory": extras.advisory,
            "settled": extras.settled,
            "arm_deg": extras.arm_deg,
        },
    })
}

/// The viewer page: `template` with the payload in place of `/*__FOLD_DATA__*/null` (every `</` written
/// `<\/`), the title (`Zouave Bolero Fold-up`) and the pinned offline renderer and its MIT notice.
///
/// # Errors
/// When a required template placeholder is absent or duplicated.
pub fn viewer_html(template: &str, payload: &Value) -> Result<String, String> {
    const MARKER: &str = "/*__FOLD_DATA__*/null";
    const THREE: &str = "__FTX_THREE_MODULE__";
    const ORBIT: &str = "__FTX_ORBIT_CONTROLS__";
    const LICENSE: &str = "__FTX_VIEWER_LICENSE__";
    for marker in [MARKER, THREE, ORBIT, LICENSE] {
        if template.matches(marker).count() != 1 {
            return Err(format!(
                "the viewer template needs exactly one {marker} placeholder"
            ));
        }
    }
    // Data URLs keep both file:// downloads and blob: browser previews independent of a server.
    // Replace only the template: a user's title or flag text is never treated as a module marker.
    let template = template
        .replacen(
            THREE,
            &format!(
                "data:text/javascript;base64,{}",
                b64(include_bytes!(
                    "../../../assets/three-r160/three.module.min.js"
                ))
            ),
            1,
        )
        .replacen(
            ORBIT,
            &format!(
                "data:text/javascript;base64,{}",
                b64(include_bytes!(
                    "../../../assets/three-r160/OrbitControls.js"
                ))
            ),
            1,
        )
        .replacen(
            LICENSE,
            include_str!("../../../assets/three-r160/LICENSE"),
            1,
        );
    let data = payload.to_string().replace("</", "<\\/");
    let title = payload["title"].as_str().unwrap_or("");
    let mut name = String::new();
    for w in title.split('(').next().unwrap_or("").split_whitespace() {
        let mut c = w.chars();
        if let Some(f) = c.next() {
            if !name.is_empty() {
                name.push(' ');
            }
            let _ = write!(name, "{}{}", f.to_uppercase(), c.as_str());
        }
    }
    if name.is_empty() {
        name = "Pattern".into();
    }
    let escaped = name
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    Ok(template.replacen(MARKER, &data, 1).replacen(
        "<title>Fold-up Viewer</title>",
        &format!("<title>{escaped} Fold-up</title>"),
        1,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Horn's rotation recovers a known rotation and translation; slerp ends at its ends.
    #[test]
    fn horn_and_slerp() {
        let a = vec![
            Vec3::new(0.1, 0.0, 0.0),
            Vec3::new(0.0, 0.2, 0.0),
            Vec3::new(0.0, 0.0, 0.3),
            Vec3::new(0.1, 0.1, 0.1),
        ];
        let (s, c) = det::sin_cos(0.7);
        let q: Quat = [c, s * 0.6, 0.0, s * 0.8];
        let t = Vec3::new(0.3, -0.2, 0.5);
        let b: Vec<Vec3> = a.iter().map(|p| rotate(q, *p) + t).collect();
        let (q2, t2) = horn(&a, &b);
        for (p, w) in a.iter().zip(&b) {
            assert!((rotate(q2, *p) + t2 - *w).norm() < 1e-14);
        }
        let q0 = [1.0, 0.0, 0.0, 0.0];
        assert!(
            slerp(q0, q, 0.0)
                .iter()
                .zip(&q0)
                .all(|(x, y)| (x - y).abs() < 1e-15)
        );
        assert!(
            slerp(q0, q, 1.0)
                .iter()
                .zip(&q)
                .all(|(x, y)| (x - y).abs() < 1e-14)
        );
    }

    /// The fold of two seamed rectangles: frame 0 is the board, the last fold frame is the wrap, every
    /// roll-up frame keeps edge lengths within 1 % (an isometry up to chord vs arc), and the zip ends on
    /// the start it was given.
    #[test]
    fn fold_kinematics() {
        use crate::start::{PartFrame, wrap_points};
        let rect = |id: &str, w: f64| {
            json!({
                "id": id, "name": id,
                "vertices": {"a": [0, 0], "b": [w, 0], "c": [w, 120.0], "d": [0, 120.0]},
                "edges": [
                    {"id": "bottom", "from": "a", "to": "b"}, {"id": "right", "from": "b", "to": "c"},
                    {"id": "top", "from": "c", "to": "d"}, {"id": "left", "from": "d", "to": "a"}
                ],
                "notches": [], "cut": {"qty": 1}, "grain": [[10, 10], [10, 40]],
                "placement": {"part": "torso"}
            })
        };
        let doc = json!({
            "schema": "fabric-pattern/1", "units": "mm",
            "pieces": [rect("A", 150.0), rect("B", 130.0)],
            "seams": [{"id": "s_1", "a": "A/right", "b": "B/left"}]
        });
        let mesh = crate::build_garment(&ftx_pattern::Pattern::new(doc), 12.0).unwrap();
        let frame = PartFrame {
            origin: Vec3::ZERO,
            up: Vec3::new(0.0, 0.0, 1.0),
            front: Vec3::new(0.0, -1.0, 0.0),
            lat: Vec3::new(1.0, 0.0, 0.0),
        };
        let places: Vec<WrapPlacement> = [(0.0, 150.0), (40.0, 0.0)]
            .iter()
            .map(|&(theta_deg, x0)| WrapPlacement {
                frame,
                h0: 500.0,
                radius: 90.0,
                theta_deg,
                x0,
                mirrored: false,
                own_side: false,
                own_side_left: false,
            })
            .collect();
        let board = board_layout(&mesh);
        let frames = fold_frames(&mesh, &places, &board, FOLD_FRAMES);
        // The first instance starts its flight at frame 0 (w = 0): the board up to rounding.
        assert!(
            frames[0]
                .iter()
                .zip(&board)
                .all(|(a, b)| (*a - *b).norm() < 1e-15)
        );
        let edges = face_edges(&mesh.faces);
        for (k, (&(lo, hi), p)) in mesh.inst_range.iter().zip(&places).enumerate() {
            let wrapped = wrap_points(&mesh.verts2d[lo..hi], p);
            assert_eq!(
                &frames[FOLD_FRAMES - 1][lo..hi],
                &wrapped[..],
                "instance {k}"
            );
            for kk in [0.0, 0.3, 0.7, 0.99] {
                let r = roll_points(&mesh.verts2d[lo..hi], p, kk);
                for &(a, b) in edges.iter().filter(|e| (lo..hi).contains(&e.0)) {
                    let l3 = (r[a - lo] - r[b - lo]).norm();
                    let l2 = (mesh.verts2d[a] - mesh.verts2d[b]).norm() * MM;
                    assert!((l3 / l2 - 1.0).abs() < 0.01, "k {kk}: {l3} vs {l2}");
                }
            }
        }
        let start: Vec<Vec3> = board
            .iter()
            .map(|p| *p + Vec3::new(0.0, 0.1, 0.0))
            .collect();
        let zip = zip_frames(&board, &start);
        assert_eq!(zip.len(), ZIP_FRAMES);
        assert_eq!(zip[0], board);
        assert!(
            zip[ZIP_FRAMES - 1]
                .iter()
                .zip(&start)
                .all(|(a, b)| (*a - *b).norm() < 1e-15)
        );
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(b64(b""), "");
        assert_eq!(b64(b"f"), "Zg==");
        assert_eq!(b64(b"fo"), "Zm8=");
        assert_eq!(b64(b"foo"), "Zm9v");
        assert_eq!(b64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn viewer_embeds_offline_modules_without_reinterpreting_payload_text() {
        let template = include_str!("../../../assets/fold_viewer_template.html");
        let payload = json!({"title":"__FTX_THREE_MODULE__ </script><&\" (test)","schema":"fabric-fold-viewer/1"});
        let html = viewer_html(template, &payload).unwrap();
        let imports: Value = serde_json::from_str(
            html.split("<script type=\"importmap\">")
                .nth(1)
                .unwrap()
                .split("</script>")
                .next()
                .unwrap(),
        )
        .unwrap();
        for name in ["three", "three/addons/controls/OrbitControls.js"] {
            assert!(
                imports["imports"][name]
                    .as_str()
                    .unwrap()
                    .starts_with("data:text/javascript;base64,")
            );
        }
        assert_eq!(imports["imports"].as_object().unwrap().len(), 2);
        assert!(html.contains(include_str!("../../../assets/three-r160/LICENSE")));
        assert!(!html.contains("https://cdn.jsdelivr.net/"));
        assert!(!html.contains("__FTX_ORBIT_CONTROLS__"));
        assert!(!html.contains("__FTX_VIEWER_LICENSE__"));
        assert!(html.contains(&payload.to_string().replace("</", "<\\/")));
        assert!(html.contains(
            "<title>__FTX_THREE_MODULE__ &lt;/script&gt;&lt;&amp;&quot; Fold-up</title>"
        ));
    }

    #[test]
    fn viewer_refuses_missing_or_duplicated_template_markers() {
        let template = include_str!("../../../assets/fold_viewer_template.html");
        for marker in [
            "/*__FOLD_DATA__*/null",
            "__FTX_THREE_MODULE__",
            "__FTX_ORBIT_CONTROLS__",
            "__FTX_VIEWER_LICENSE__",
        ] {
            assert!(viewer_html(&template.replace(marker, ""), &json!({})).is_err());
            assert!(viewer_html(&format!("{template}{marker}"), &json!({})).is_err());
        }
    }
}
