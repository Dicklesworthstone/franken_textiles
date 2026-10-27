//! Reviewed sharpness regions become native binary masks; acquisition metadata stays authoritative.

mod editor;

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_measure::triangulate::FrameId;
use ftx_trace::raster::{
    draw::fill_poly,
    image::{Gray, encode_png_gray},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Args, bad_input, known_options, required, workspace};

const MAX_BYTES: usize = 256 * 1024 * 1024;
const MAX_PIXELS: usize = 200_000_000;
const MAX_WORK: u64 = 2_000_000_000;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Region {
    outer: Vec<[i64; 2]>,
    #[serde(default)]
    holes: Vec<Vec<[i64; 2]>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    index: usize,
    image_sha256: String,
    decoded_gray_sha256: String,
    image_size_px: [usize; 2],
    person_regions: Option<Vec<Region>>,
    sheet_regions: Option<Vec<Region>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Annotations {
    schema: String,
    source_manifest: String,
    source_manifest_sha256: String,
    frames: Vec<Frame>,
}

fn budget(message: &str) -> Refusal {
    Refusal::new(RefusalCode::BudgetExceeded, message)
}

fn output(args: &Args, command: &str) -> Result<PathBuf, Refusal> {
    known_options(args, command, &["out"])?;
    let path = PathBuf::from(required(args, "out")?);
    match fs::symlink_metadata(&path) {
        Ok(_) => {
            return Err(bad_input(
                "mask output must be a new directory, not an existing path or link",
            ));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(workspace::io_error(&path, e)),
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if path.file_name().is_none() || !parent.is_dir() {
        return Err(bad_input(
            "mask output needs an existing parent and a new directory name",
        ));
    }
    Ok(path)
}

#[derive(Default)]
struct Bundle {
    files: Vec<(String, Vec<u8>)>,
    bytes: usize,
}

impl Bundle {
    fn push(&mut self, name: String, bytes: Vec<u8>) -> Result<Value, Refusal> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .filter(|&n| n <= MAX_BYTES)
            .ok_or_else(|| {
                budget("mask bundle exceeds 256 MiB; prepare an explicitly shorter source manifest")
            })?;
        let record = json!({"file":name,"bytes":bytes.len(),"sha256":sha256_hex(&bytes)});
        self.files.push((name, bytes));
        Ok(record)
    }

    fn publish(self, path: &Path, name: &str, report: &Value) -> Result<(), Refusal> {
        let report = workspace::json_bytes(report)?;
        if self
            .bytes
            .checked_add(report.len())
            .is_none_or(|n| n > MAX_BYTES)
        {
            return Err(budget("mask bundle exceeds 256 MiB"));
        }
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(path)
            .map_err(|e| workspace::io_error(path, e))?;
        for (file, bytes) in self.files {
            workspace::write_new(&path.join(file), &bytes)?;
        }
        // A partial I/O failure never publishes a completion report.
        workspace::write_new(&path.join(name), &report)
    }
}

fn image_name(index: usize, bytes: &[u8]) -> String {
    let extension = if bytes.starts_with(&[0xff, 0xd8]) {
        "jpg"
    } else {
        "png"
    };
    format!("source-{index:04}.{extension}")
}

fn admit_pixels(image: &Gray, total: &mut usize) -> Result<(), Refusal> {
    let pixels = image
        .w
        .checked_mul(image.h)
        .filter(|&n| image.w >= 3 && image.h >= 3 && n <= 40_000_000)
        .ok_or_else(|| bad_input("mask sources need 3 x 3 through 40 million pixels per frame"))?;
    *total = total
        .checked_add(pixels)
        .filter(|&n| n <= MAX_PIXELS)
        .ok_or_else(|| budget("mask preparation exceeds 200 million total source pixels"))?;
    Ok(())
}

pub(super) fn review(args: &Args) -> Result<(Value, i32, String), Refusal> {
    let out = output(args, "measure video-mask-review")?;
    let (input, bytes, source) =
        super::video::read_manifest(Path::new(super::single(args, "video-mask-review")?))?;
    let base = source
        .parent()
        .ok_or_else(|| bad_input("source manifest needs a parent directory"))?;
    let source_name = source
        .to_str()
        .ok_or_else(|| bad_input("source manifest path must be UTF-8"))?;
    let mut bundle = Bundle::default();
    let mut frames = Vec::new();
    let mut images = Vec::new();
    let mut pixels = 0;
    for (index, frame) in input.frames.iter().enumerate() {
        let (image, bytes) = super::video::load_pixels(base, &frame.image, false)?;
        admit_pixels(&image, &mut pixels)?;
        let name = image_name(index, &bytes);
        frames.push(Frame {
            index,
            image_sha256: sha256_hex(&bytes),
            decoded_gray_sha256: sha256_hex(&image.data),
            image_size_px: [image.w, image.h],
            person_regions: None,
            sheet_regions: None,
        });
        let metadata = &frame.metadata;
        // Keep the original u64 PTS out of editable browser JSON numbers.
        images.push(json!({"file":name,"label":format!("{} @ {} µs · lens {} · {}° clockwise",
            metadata.frame.stream,metadata.frame.timestamp_us,metadata.camera_epoch,metadata.rotation_cw_deg)}));
        bundle.push(name, bytes)?;
    }
    let annotations = Annotations {
        schema: "ftx-video-mask-polygons/1".into(),
        source_manifest: source_name.into(),
        source_manifest_sha256: sha256_hex(&bytes),
        frames,
    };
    let annotations = serde_json::to_value(annotations).map_err(|e| bad_input(e.to_string()))?;
    let receipt = bundle.push(
        "annotations.json".into(),
        workspace::json_bytes(&annotations)?,
    )?;
    bundle.push("source-manifest.json".into(), bytes)?;
    bundle.push("index.html".into(), editor::html(&annotations, &images)?)?;
    bundle.push("editor.js".into(), editor::SCRIPT.as_bytes().to_vec())?;
    let report = json!({"schema":"ftx-video-mask-review/1","status":"MASK_REVIEW_REQUIRED",
        "source_manifest":source,"source_manifest_sha256":annotations["source_manifest_sha256"],
        "annotations":receipt,"frames":images,"source_pixels":pixels,
        "no_claim":"Manual person/sheet sharpness regions only; no inferred anatomy, silhouette, stillness, camera accuracy or selection."});
    bundle.publish(&out, "review.json", &report)?;
    Ok((
        report,
        0,
        format!(
            "Open {}/index.html, review person/sheet regions and download the polygon document. Run video-masks on that document before video-select.",
            out.display()
        ),
    ))
}

fn cross(a: [i64; 2], b: [i64; 2], c: [i64; 2]) -> i128 {
    (i128::from(b[0]) - i128::from(a[0])) * (i128::from(c[1]) - i128::from(a[1]))
        - (i128::from(b[1]) - i128::from(a[1])) * (i128::from(c[0]) - i128::from(a[0]))
}

fn intersects(a: [i64; 2], b: [i64; 2], c: [i64; 2], d: [i64; 2]) -> bool {
    if (0..2).any(|axis| {
        a[axis].max(b[axis]) < c[axis].min(d[axis]) || c[axis].max(d[axis]) < a[axis].min(b[axis])
    }) {
        return false;
    }
    let opposite = |x: i128, y: i128| x == 0 || y == 0 || x.signum() != y.signum();
    opposite(cross(a, b, c), cross(a, b, d)) && opposite(cross(c, d, a), cross(c, d, b))
}

fn ring(points: &[[i64; 2]], size: [usize; 2]) -> Result<Vec<(i64, i64)>, Refusal> {
    if !(3..=256).contains(&points.len())
        || points
            .iter()
            .any(|p| (0..2).any(|axis| !(0..size[axis] as i64).contains(&p[axis])))
        || points.iter().collect::<BTreeSet<_>>().len() != points.len()
    {
        return Err(bad_input(
            "each ring needs 3..256 distinct integer vertices inside the stored image; do not repeat its closing vertex",
        ));
    }
    let n = points.len();
    let area: i128 = (0..n)
        .map(|i| {
            i128::from(points[i][0]) * i128::from(points[(i + 1) % n][1])
                - i128::from(points[i][1]) * i128::from(points[(i + 1) % n][0])
        })
        .sum();
    if area == 0 {
        return Err(bad_input("mask rings must have nonzero area"));
    }
    for i in 0..n {
        for j in i + 1..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            if intersects(
                points[i],
                points[(i + 1) % n],
                points[j],
                points[(j + 1) % n],
            ) {
                return Err(bad_input(
                    "mask rings must be simple; crossing or touching nonadjacent edges refuse",
                ));
            }
        }
    }
    Ok(points.iter().map(|p| (p[0], p[1])).collect())
}

fn spend(work: &mut u64, amount: u64) -> Result<(), Refusal> {
    *work = work
        .checked_add(amount)
        .filter(|&n| n <= MAX_WORK)
        .ok_or_else(|| budget("mask raster work exceeds two billion bounded work units"))?;
    Ok(())
}

fn raster(
    regions: &[Region],
    size: [usize; 2],
    vertices: &mut usize,
    work: &mut u64,
) -> Result<Gray, Refusal> {
    if regions.len() > 16 || regions.iter().any(|r| r.holes.len() > 16) {
        return Err(budget(
            "each mask permits at most 16 components and 16 holes per component",
        ));
    }
    let [w, h] = size;
    let pixels = (w * h) as u64;
    // Account for allocation/union, line drawing, edge scans/sorts and topology checks before fill.
    spend(work, pixels * (1 + 2 * regions.len() as u64))?;
    let mut checked = Vec::with_capacity(regions.len());
    for region in regions {
        let mut rings = Vec::new();
        for points in std::iter::once(&region.outer).chain(&region.holes) {
            *vertices = vertices
                .checked_add(points.len())
                .filter(|&n| n <= 4096)
                .ok_or_else(|| {
                    budget("each frame permits at most 4096 total person/sheet vertices")
                })?;
            if !(3..=256).contains(&points.len()) {
                return Err(bad_input("each ring needs 3..256 vertices"));
            }
            let n = points.len() as u64;
            spend(
                work,
                n * n + (w as u64 + h as u64) * n + h as u64 * n * 8 + pixels,
            )?;
            rings.push(ring(points, size)?);
        }
        checked.push(rings);
    }
    let mut result = Gray::filled(w, h, 0);
    for rings in checked {
        // A hole subtracts from its own component, never from another overlapping component.
        let mut component = Gray::filled(w, h, 0);
        fill_poly(&mut component, &rings[..1], 255);
        for hole in &rings[1..] {
            fill_poly(&mut component, std::slice::from_ref(hole), 0);
        }
        for (a, b) in result.data.iter_mut().zip(component.data) {
            *a |= b;
        }
    }
    Ok(result)
}

pub(super) fn apply(args: &Args) -> Result<(Value, i32, String), Refusal> {
    let out = output(args, "measure video-masks")?;
    let annotation_bytes = super::video::read_limited(
        Path::new(super::single(args, "video-masks")?),
        16 * 1024 * 1024,
    )?;
    let annotations: Annotations =
        serde_json::from_slice(&annotation_bytes).map_err(|e| bad_input(e.to_string()))?;
    if annotations.schema != "ftx-video-mask-polygons/1"
        || !Path::new(&annotations.source_manifest).is_absolute()
    {
        return Err(bad_input(
            "mask annotations need schema ftx-video-mask-polygons/1 and the original absolute source manifest",
        ));
    }
    let (input, bytes, source) =
        super::video::read_manifest(Path::new(&annotations.source_manifest))?;
    if annotations.source_manifest_sha256 != sha256_hex(&bytes)
        || annotations.frames.len() != input.frames.len()
    {
        return Err(bad_input(
            "reviewed source manifest changed or mask annotations omit source frames",
        ));
    }
    // Preserve the original metadata/options JSON, including omitted defaults and integer-valued
    // focal/threshold observations. Only the three file paths and canonical frame order change.
    let mut manifest: Value =
        serde_json::from_slice(&bytes).map_err(|e| bad_input(e.to_string()))?;
    let mut original_frames = manifest["frames"]
        .as_array()
        .ok_or_else(|| bad_input("missing source frames"))?
        .iter()
        .map(|frame| {
            serde_json::from_value::<FrameId>(frame["metadata"]["frame"].clone())
                .map(|id| (id, frame.clone()))
                .map_err(|e| bad_input(e.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    original_frames.sort_by(|a, b| a.0.cmp(&b.0));
    let base = source
        .parent()
        .ok_or_else(|| bad_input("source manifest needs a parent directory"))?;
    let mut bundle = Bundle::default();
    let mut evidence = Vec::new();
    let mut pixels = 0;
    let mut work = 0;
    for (index, (frame, entry)) in input.frames.iter().zip(&annotations.frames).enumerate() {
        if entry.index != index {
            return Err(bad_input(
                "mask frame indices must retain the complete canonical source order",
            ));
        }
        let person = entry.person_regions.as_ref().ok_or_else(|| {
            bad_input("person regions are unreviewed; draw regions or explicitly mark not visible")
        })?;
        let sheet = entry.sheet_regions.as_ref().ok_or_else(|| {
            bad_input("sheet regions are unreviewed; draw regions or explicitly mark not visible")
        })?;
        let (image, bytes) = super::video::load_pixels(base, &frame.image, false)?;
        admit_pixels(&image, &mut pixels)?;
        let size = [image.w, image.h];
        if entry.image_size_px != size
            || entry.image_sha256 != sha256_hex(&bytes)
            || entry.decoded_gray_sha256 != sha256_hex(&image.data)
        {
            return Err(bad_input(
                "reviewed image pixels, dimensions or source hash changed; prepare a new review",
            ));
        }
        let mut vertices = 0;
        let person = raster(person, size, &mut vertices, &mut work)?;
        let sheet = raster(sheet, size, &mut vertices, &mut work)?;
        let image_name = image_name(index, &bytes);
        let person_name = format!("person-{index:04}.png");
        let sheet_name = format!("sheet-{index:04}.png");
        let original = &mut original_frames[index].1;
        original["image"] = json!(image_name);
        original["person_mask"] = json!(person_name);
        original["sheet_mask"] = json!(sheet_name);
        let image_record = bundle.push(image_name, bytes)?;
        let person_record = bundle.push(person_name, encode_png_gray(&person))?;
        let sheet_record = bundle.push(sheet_name, encode_png_gray(&sheet))?;
        evidence.push(json!({"index":index,"metadata":frame.metadata,"image":image_record,
            "person_mask":person_record,"sheet_mask":sheet_record,
            "person_mask_sha256":sha256_hex(&person.data),"sheet_mask_sha256":sha256_hex(&sheet.data)}));
    }
    manifest["frames"] = Value::Array(
        original_frames
            .into_iter()
            .map(|(_, frame)| frame)
            .collect(),
    );
    let manifest_bytes = workspace::json_bytes(&manifest)?;
    let report = json!({"schema":"ftx-video-mask-report/1","status":"MASKS_MATERIALIZED",
        "source_manifest":source,"source_manifest_sha256":annotations.source_manifest_sha256,
        "annotations_sha256":sha256_hex(&annotation_bytes),"frames":evidence,
        "manifest":"frames.json","manifest_sha256":sha256_hex(&manifest_bytes),
        "source_pixels":pixels,"raster_work_units":work,
        "rasterizer":"Native fill_poly: integer stored-pixel vertices, inclusive 8-connected boundaries, per-component holes then component union; no antialiasing or resizing.",
        "no_claim":"Reviewed sharpness regions only. Run video-select for eligibility; no anatomical segmentation, stillness, calibration or body-fit readiness is inferred."});
    bundle.push("annotations.json".into(), annotation_bytes)?;
    bundle.push("source-manifest.json".into(), bytes)?;
    bundle.push("frames.json".into(), manifest_bytes)?;
    bundle.publish(&out, "masks.json", &report)?;
    Ok((
        report,
        0,
        format!(
            "Materialized masks for {} unchanged frame identities into {}. Run video-select on its frames.json.",
            annotations.frames.len(),
            out.display()
        ),
    ))
}
