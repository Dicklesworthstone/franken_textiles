//! Soutache couching files for embroidery machines (`docs/spec/03` §7.12): per cut piece that carries soutache, a
//! Tajima DST with a placement run on the sew line, a stop to lay the braid, and a straight stitch down the braid's
//! centre along every braid line; plus a README. Units 0.1 mm, y up, the design centred on the hoop.

use ftx_core::polyline::{at_fractions, polyline_length};
use ftx_core::{Refusal, Vec2, det, np, py};

use crate::ctx::Ctx;
use crate::pyjson::PyJson;
use crate::text::{ascii_text, safe_name};

const STEPS: [i64; 5] = [81, 27, 9, 3, 1];
const MAX_STEPS: i64 = 121;
const JUMP: u8 = 0x80;
const STOP: u8 = 0xC0;
/// The END record.
pub const DST_END: [u8; 3] = [0, 0, 0xF3];
/// Couching stitch length (mm).
pub const COUCH_STITCH_MM: f64 = 2.5;
/// Placement stitch length (mm).
pub const PLACE_STITCH_MM: f64 = 5.0;
const HOOPS_MM: [(i64, i64); 5] = [(100, 100), (130, 180), (160, 260), (200, 300), (240, 360)];

fn half(p: i64) -> i64 {
    match p {
        81 => 40,
        27 => 13,
        9 => 4,
        3 => 1,
        _ => 0,
    }
}

/// (byte, plus bit, minus bit) of a step on the x axis or the y axis.
fn bits(p: i64, y: bool) -> (usize, u8, u8) {
    let byte = match p {
        81 => 2,
        27 | 3 => 1,
        _ => 0,
    };
    let (plus, minus) = match (p, y) {
        (81 | 27 | 9, false) => (0x04, 0x08),
        (_, false) => (0x01, 0x02),
        (81 | 27 | 9, true) => (0x20, 0x10),
        (_, true) => (0x80, 0x40),
    };
    (byte, plus, minus)
}

/// One record: a move of at most 121 steps per axis in balanced ternary, with its flag (0 stitch, 0x80 jump,
/// 0xC0 stop).
///
/// # Errors
/// A move longer than 121 steps.
pub fn dst_record(dx: i64, dy: i64, flag: u8) -> Result<[u8; 3], Refusal> {
    let mut b = [0u8, 0u8, 0x03 | flag];
    for (v0, y) in [(dx, false), (dy, true)] {
        let mut v = v0;
        for p in STEPS {
            let (i, plus, minus) = bits(p, y);
            if v > half(p) {
                b[i] |= plus;
                v -= p;
            } else if v < -half(p) {
                b[i] |= minus;
                v += p;
            }
        }
        if v != 0 {
            return Err(Refusal::new(
                ftx_core::RefusalCode::OutOfRange,
                format!("DST move {dx},{dy} exceeds {MAX_STEPS} steps"),
            ));
        }
    }
    Ok(b)
}

/// What a record moves and is: `(dx, dy, kind)` with kind `stitch`, `jump`, `stop` or `end`.
#[must_use]
pub fn dst_decode(rec: [u8; 3]) -> (i64, i64, &'static str) {
    if rec == DST_END {
        return (0, 0, "end");
    }
    let mut d = [0i64; 2];
    for (axis, y) in [(0, false), (1, true)] {
        for p in STEPS {
            let (i, plus, minus) = bits(p, y);
            d[axis] += p * (i64::from(rec[i] & plus != 0) - i64::from(rec[i] & minus != 0));
        }
    }
    let kind = if rec[2] & STOP == STOP {
        "stop"
    } else if rec[2] & JUMP != 0 {
        "jump"
    } else {
        "stitch"
    };
    (d[0], d[1], kind)
}

/// A design block: a run stitched point to point (mm), or a stop.
pub enum Block {
    /// Stitch through these points (mm).
    Run(Vec<Vec2>),
    /// Stop for the operator.
    Stop,
}

/// What a written DST holds.
#[derive(Clone, Debug, PartialEq)]
pub struct DstInfo {
    /// Records (without END).
    pub records: usize,
    /// Stops.
    pub stops: usize,
    /// Sewn length of each run (mm, 2 dp).
    pub sewn_mm: Vec<f64>,
    /// Width and height (mm, 1 dp).
    pub extent_mm: [f64; 2],
}

fn round_i(v: f64) -> i64 {
    v.round_ties_even() as i64
}

/// The DST file of `blocks`: the design starts at the hoop centre (0, 0); moves longer than one record are
/// split; each run starts with a stitch in place where its jump lands and ends with a 1 mm tie-off.
///
/// # Errors
/// As [`dst_record`].
pub fn write_dst(label: &str, blocks: &[Block]) -> Result<(Vec<u8>, DstInfo), Refusal> {
    let mut recs: Vec<[u8; 3]> = Vec::new();
    let mut pos = (0i64, 0i64);
    let (mut xs, mut ys) = (vec![0i64], vec![0i64]);
    let mut stops = 0;
    let mut sewn: Vec<f64> = Vec::new();
    let mut go = |target: (i64, i64),
                  flag: u8,
                  pos: &mut (i64, i64),
                  recs: &mut Vec<[u8; 3]>,
                  sewn: &mut Vec<f64>|
     -> Result<(), Refusal> {
        let (dx, dy) = (target.0 - pos.0, target.1 - pos.1);
        let n = ((dx.abs().max(dy.abs()) as f64 / MAX_STEPS as f64).ceil() as i64).max(1);
        for k in 1..=n {
            let step = |d: i64| {
                round_i(d as f64 * k as f64 / n as f64)
                    - round_i(d as f64 * (k - 1) as f64 / n as f64)
            };
            let q = (pos.0 + step(dx), pos.1 + step(dy));
            recs.push(dst_record(q.0 - pos.0, q.1 - pos.1, flag)?);
            if flag == 0
                && let Some(last) = sewn.last_mut()
            {
                *last += det::hypot((q.0 - pos.0) as f64, (q.1 - pos.1) as f64) * 0.1;
            }
            *pos = q;
            xs.push(q.0);
            ys.push(q.1);
        }
        Ok(())
    };
    for blk in blocks {
        match blk {
            Block::Stop => {
                recs.push(dst_record(0, 0, STOP)?);
                stops += 1;
            }
            Block::Run(pts) => {
                let steps: Vec<(i64, i64)> = pts
                    .iter()
                    .map(|p| (round_i(p.x / 0.1), round_i(p.y / 0.1)))
                    .collect();
                sewn.push(0.0);
                go(steps[0], JUMP, &mut pos, &mut recs, &mut sewn)?;
                recs.push(dst_record(0, 0, 0)?);
                let mut prev = pos;
                for &q in &steps[1..] {
                    if q != pos {
                        prev = pos;
                        go(q, 0, &mut pos, &mut recs, &mut sewn)?;
                    }
                }
                let seg = ((pos.0 - prev.0) as f64, (pos.1 - prev.1) as f64);
                let len = det::hypot(seg.0, seg.1);
                if len > 0.0 {
                    let end = pos;
                    let m = (1.0 / 0.1f64).min(len);
                    let back = (
                        round_i(pos.0 as f64 - seg.0 / len * m),
                        round_i(pos.1 as f64 - seg.1 / len * m),
                    );
                    go(back, 0, &mut pos, &mut recs, &mut sewn)?;
                    go(end, 0, &mut pos, &mut recs, &mut sewn)?;
                }
            }
        }
    }
    let name: String = ascii_text(label).chars().take(16).collect();
    let (maxx, minx) = (
        *xs.iter().max().unwrap_or(&0),
        *xs.iter().min().unwrap_or(&0),
    );
    let (maxy, miny) = (
        *ys.iter().max().unwrap_or(&0),
        *ys.iter().min().unwrap_or(&0),
    );
    let sign = |v: i64| if v >= 0 { '+' } else { '-' };
    let head = format!(
        "LA:{name:<16}\rST:{:7}\rCO:{stops:3}\r+X:{maxx:5}\r-X:{:5}\r+Y:{maxy:5}\r-Y:{:5}\rAX:{}{:5}\rAY:{}{:5}\rMX:+{:5}\rMY:+{:5}\rPD:******\r",
        recs.len(),
        -minx,
        -miny,
        sign(pos.0),
        pos.0.abs(),
        sign(pos.1),
        pos.1.abs(),
        0,
        0
    );
    let mut bytes = head.into_bytes();
    bytes.push(0x1A);
    bytes.resize(bytes.len().max(512), b' ');
    for r in &recs {
        bytes.extend_from_slice(r);
    }
    bytes.extend_from_slice(&DST_END);
    let info = DstInfo {
        records: recs.len(),
        stops,
        sewn_mm: sewn.iter().map(|s| py::round(*s, 2)).collect(),
        extent_mm: [
            py::round((maxx - minx) as f64 * 0.1, 1),
            py::round((maxy - miny) as f64 * 0.1, 1),
        ],
    };
    Ok((bytes, info))
}

/// A decoded DST: header fields, moves, whether it ends with exactly one END and nothing after, and its size.
#[derive(Clone, Debug)]
pub struct DstRead {
    /// Header fields (`LA`, `ST`, ...), values trimmed.
    pub head: Vec<(String, String)>,
    /// Moves `(dx, dy, kind)`.
    pub moves: Vec<(i64, i64, &'static str)>,
    /// One END record, nothing after it.
    pub ended: bool,
    /// File size (bytes).
    pub size: usize,
}

/// Reads a DST back.
#[must_use]
pub fn read_dst(data: &[u8]) -> DstRead {
    let mut head = Vec::new();
    for item in data[..data.len().min(512)].split(|&b| b == b'\r') {
        if item.len() > 3 && item[2] == b':' {
            let s = |b: &[u8]| {
                b.iter()
                    .map(|&c| if c < 128 { char::from(c) } else { '\u{FFFD}' })
                    .collect::<String>()
            };
            head.push((s(&item[..2]), s(&item[3..]).trim().to_string()));
        }
    }
    let (mut moves, mut ended, mut trailing) = (Vec::new(), false, false);
    let mut i = 512;
    while i + 2 < data.len() {
        if ended {
            trailing = true;
            break;
        }
        let (dx, dy, kind) = dst_decode([data[i], data[i + 1], data[i + 2]]);
        ended = kind == "end";
        if !ended {
            moves.push((dx, dy, kind));
        }
        i += 3;
    }
    DstRead {
        head,
        moves,
        ended: ended && !trailing,
        size: data.len(),
    }
}

/// Points evenly spaced along a polyline at most `step` mm apart (closed lines are closed first).
fn resample(pts: &[Vec2], step: f64, closed: bool) -> Vec<Vec2> {
    let mut p = pts.to_vec();
    if closed && (p[0] - p[p.len() - 1]).norm() > 1e-6 {
        p.push(p[0]);
    }
    let n = ((polyline_length(&p) / step).ceil() as usize).max(1);
    at_fractions(&p, &np::linspace(0.0, 1.0, n + 1))
}

/// Joins braid lines whose ends meet (a half border and its mirror at the fold), then orders them nearest
/// first from the design centre, reversing open lines where that is nearer.
fn couch_order(decor: Vec<(Vec<Vec2>, bool)>, tol: f64) -> Vec<(Vec<Vec2>, bool)> {
    let mut paths: Vec<(Vec<Vec2>, bool)> =
        decor.into_iter().filter(|(p, _)| p.len() > 1).collect();
    let rev = |v: &[Vec2]| v.iter().rev().copied().collect::<Vec<_>>();
    'join: loop {
        for i in 0..paths.len() {
            for j in 0..paths.len() {
                if i >= j || paths[i].1 || paths[j].1 {
                    continue;
                }
                let (a, b) = (paths[i].0.clone(), paths[j].0.clone());
                for (aa, bb) in [
                    (a.clone(), b.clone()),
                    (a.clone(), rev(&b)),
                    (rev(&a), b.clone()),
                    (rev(&a), rev(&b)),
                ] {
                    if (aa[aa.len() - 1] - bb[0]).norm() <= tol {
                        let mut joined = aa;
                        joined.extend_from_slice(&bb[1..]);
                        paths[i] = (joined, false);
                        paths.remove(j);
                        continue 'join;
                    }
                }
            }
        }
        break;
    }
    let mut order = Vec::new();
    let mut here = Vec2::ZERO;
    while !paths.is_empty() {
        let mut best: Option<(f64, usize, bool)> = None;
        for (k, (p, closed)) in paths.iter().enumerate() {
            for r in if *closed {
                vec![false]
            } else {
                vec![false, true]
            } {
                let start = if r { p[p.len() - 1] } else { p[0] };
                let d = (start - here).norm();
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, k, r));
                }
            }
        }
        let Some((_, k, r)) = best else { break };
        let (mut pts, closed) = paths.remove(k);
        if r {
            pts.reverse();
        }
        here = if closed { pts[0] } else { pts[pts.len() - 1] };
        order.push((pts, closed));
    }
    order
}

/// The README: its name and text.
pub type Readme = (String, String);

/// A DST file: name and bytes, its manifest key and entry.
#[derive(Clone, Debug)]
pub struct DstFile {
    /// File name.
    pub name: String,
    /// The file.
    pub bytes: Vec<u8>,
    /// Manifest key (`dst_<instance>`).
    pub key: String,
    /// Manifest entry.
    pub entry: PyJson,
}

/// The soutache DSTs of every instance that carries soutache and, when there is one, the README's name and
/// text.
///
/// # Errors
/// As [`Ctx::geom`] and [`dst_record`].
pub fn export_embroidery(ctx: &Ctx) -> Result<(Vec<DstFile>, Option<Readme>), Refusal> {
    let mut out = Vec::new();
    let mut rows = Vec::new();
    for g in ctx.instance_geoms(6.0)? {
        let braid: Vec<&crate::geom::Decor> =
            g.decor.iter().filter(|d| d.kind == "soutache").collect();
        if braid.is_empty() {
            continue;
        }
        let (mut lo, mut hi) = (
            Vec2::new(f64::INFINITY, f64::INFINITY),
            Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        for p in &g.sew {
            lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        let c = Vec2::new(0.5 * (lo.x + hi.x), 0.5 * (lo.y + hi.y));
        let centred: Vec<Vec2> = g.sew.iter().map(|&p| p - c).collect();
        let place = resample(&centred, PLACE_STITCH_MM, true);
        let runs: Vec<Vec<Vec2>> = couch_order(
            braid
                .iter()
                .map(|d| (d.pts.iter().map(|&p| p - c).collect(), d.closed))
                .collect(),
            0.5,
        )
        .into_iter()
        .map(|(p, closed)| resample(&p, COUCH_STITCH_MM, closed))
        .collect();
        let name = format!(
            "{}_{}_{}_soutache.dst",
            ctx.style,
            safe_name(&g.iid.replace('.', "_"), 40),
            safe_name(&ctx.size, 20)
        );
        let label = format!(
            "{} {}",
            ctx.style.chars().take(8).collect::<String>(),
            g.iid
        );
        let mut blocks = vec![Block::Run(place), Block::Stop];
        blocks.extend(runs.iter().cloned().map(Block::Run));
        let (bytes, info) = write_dst(&label, &blocks)?;
        let [w, h] = info.extent_mm;
        let hoop = HOOPS_MM
            .iter()
            .find(|&&(a, b)| (w <= a as f64 && h <= b as f64) || (w <= b as f64 && h <= a as f64))
            .map_or_else(
                || "over 240x360 mm: a commercial frame".to_string(),
                |(a, b)| format!("{a}x{b} mm"),
            );
        let braid_mm: f64 = runs.iter().map(|r| polyline_length(r)).sum();
        let couch: f64 = info.sewn_mm[1..].iter().sum();
        let entry = PyJson::dict(vec![
            ("file", PyJson::str(&name)),
            ("kind", PyJson::str("soutache couching")),
            ("instance", PyJson::str(&g.iid)),
            (
                "centre_mm",
                PyJson::floats(&[py::round(c.x, 3), py::round(c.y, 3)]),
            ),
            ("placement_mm", PyJson::Float(info.sewn_mm[0])),
            ("couch_mm", PyJson::Float(py::round(couch, 2))),
            ("braid_mm", PyJson::Float(py::round(braid_mm, 1))),
            ("braid_lines", PyJson::Int(runs.len() as i64)),
            ("size_mm", PyJson::floats(&[w, h])),
            ("hoop", PyJson::str(&hoop)),
            ("stitch_mm", PyJson::Float(COUCH_STITCH_MM)),
            ("records", PyJson::Int(info.records as i64)),
            ("stops", PyJson::Int(info.stops as i64)),
            ("sewn_mm", PyJson::floats(&info.sewn_mm)),
            ("extent_mm", PyJson::floats(&info.extent_mm)),
        ]);
        rows.push(format!(
            "{name}: {} x {} mm, hoop {hoop}, {} braid line(s)",
            py::repr_f64(w),
            py::repr_f64(h),
            runs.len()
        ));
        out.push(DstFile {
            key: format!("dst_{}", g.iid),
            name,
            bytes,
            entry,
        });
    }
    let readme = (!out.is_empty()).then(|| {
        (
            format!("{}_soutache_README.txt", ctx.style),
            format!(
                "Soutache couching files (Tajima DST, 0.1 mm units, design centred on the hoop).\n\
                 1. Hoop the UNCUT fabric (stabilizer behind). Colour 1 stitches the placement line = the sew line.\n\
                 2. At the stop, lay the soutache braid along the printed braid line (PDF layer 'Soutache placement').\n\
                 3. Colour 2 sews a {} mm straight stitch down the braid's centre groove; use a braiding\n   \
                 or cording foot / guide if the machine has one, otherwise couch by hand along the same line.\n\
                 4. Cut the piece out with its seam allowance outside the placement line (cutter files / PDF cut line).\n\
                 Braid ends that reach a seam line are caught in that seam. Left pieces are mirrored files.\n\n{}\n",
                py::repr_f64(COUCH_STITCH_MM),
                rows.join("\n")
            ),
        )
    });
    Ok((out, readme))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_match_the_spec_goldens_and_round_trip() {
        let hex = |r: [u8; 3]| format!("{:02x} {:02x} {:02x}", r[0], r[1], r[2]);
        for (dx, dy, flag, want) in [
            (0, 0, 0, "00 00 03"),
            (1, 0, 0, "01 00 03"),
            (-1, 0, 0, "02 00 03"),
            (0, 1, 0, "80 00 03"),
            (0, -1, 0, "40 00 03"),
            (121, 121, 0, "a5 a5 27"),
            (-121, -121, 0, "5a 5a 1b"),
            (40, 0, 0, "05 05 03"),
            (41, 0, 0, "0a 0a 07"),
            (13, -13, 0, "55 41 03"),
            (5, -7, JUMP, "56 82 83"),
            (0, 0, STOP, "00 00 c3"),
        ] {
            assert_eq!(hex(dst_record(dx, dy, flag).unwrap()), want, "({dx}, {dy})");
        }
        for dx in -121..=121 {
            for dy in [-121, -80, -40, -13, -4, -1, 0, 1, 4, 13, 40, 80, 121] {
                assert_eq!(
                    dst_decode(dst_record(dx, dy, 0).unwrap()),
                    (dx, dy, "stitch")
                );
                assert_eq!(dst_decode(dst_record(dx, dy, JUMP).unwrap()).2, "jump");
            }
        }
        assert_eq!(dst_decode(DST_END), (0, 0, "end"));
        assert!(dst_record(122, 0, 0).is_err());
    }
}
