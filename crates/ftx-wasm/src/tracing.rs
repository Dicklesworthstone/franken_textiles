//! Local image-to-pattern workshop. Each edit starts from the immutable traced draft, so repeating
//! names/scale operations cannot double-mirror or double-scale it. No files or network are accessed.

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_pattern::{Pattern, doc::PatternDoc};
use ftx_trace::{
    AuditOptions, OValue, TraceOptions, annotate, audit, fit_scale,
    raster::image::{Bgr, encode_png_gray, load_png_with_limit},
    review, trace_image,
};
use serde::Deserialize;
use serde_json::{Value, json};

const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;
const MAX_TEXT: usize = 256 * 1024;
const MAX_OUTPUT: usize = 64 * 1024 * 1024;

fn bad(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}
fn budget(message: &str) -> Refusal {
    Refusal::new(RefusalCode::BudgetExceeded, message)
}
fn parse<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, Refusal> {
    if text.len() > MAX_TEXT {
        return Err(budget("workshop JSON exceeds 256 KiB"));
    }
    serde_json::from_str(text).map_err(|e| bad(e.to_string()))
}
fn json_bytes(value: &impl serde::Serialize) -> Result<Vec<u8>, Refusal> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| bad(e.to_string()))?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Options {
    channel: String,
    threshold: String,
    invert: Option<bool>,
    crop: Option<[u32; 4]>,
    stroke: Option<f64>,
    min_area_frac: f64,
    dashes: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            channel: "auto".into(),
            threshold: "otsu".into(),
            invert: None,
            crop: None,
            stroke: None,
            min_area_frac: 0.01,
            dashes: true,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    reference: String,
    target_mm: f64,
}

/// One immutable uploaded sheet, decoded and traced once; no persisted user data or hidden clock.
pub struct Sheet {
    image: Bgr,
    draft: PatternDoc,
    source_sha256: String,
    diagnostics: Value,
    original: Vec<(String, Vec<u8>)>,
}

/// Complete review products, returned only when all calculations succeed. These are not cutting files.
pub struct Review {
    /// Separate scale and audit status, diagnostics and the bounds of the inference.
    pub report: Value,
    /// Fixed relative names and their bytes. No name is taken from a user-provided path.
    pub files: Vec<(String, Vec<u8>)>,
}
impl Review {
    /// Deterministic archive, stamped 1980-01-01 (there is no capture date to infer).
    #[must_use]
    pub fn zip(&self) -> Vec<u8> {
        crate::zip::zip(&self.files, "1980-01-01T00:00")
    }
}

impl Sheet {
    /// Trace explicit PNG bytes. Options: channel, threshold, invert, crop [x,y,w,h], stroke,
    /// min_area_frac and dashes. The 4-million-pixel browser limit is checked before inflation;
    /// an oversized image is refused, never resized. Coordinates use the native decoder's frame.
    pub fn new(png: &[u8], options: &str) -> Result<Self, Refusal> {
        if png.len() > MAX_IMAGE_BYTES {
            return Err(budget("source PNG exceeds 16 MiB"));
        }
        let options: Options = parse(options)?;
        if !["otsu", "adaptive"].contains(&options.threshold.as_str())
            || !ftx_trace::trace::CHANNELS.contains(&options.channel.as_str())
            || !options.min_area_frac.is_finite()
            || !(0.0001..=0.5).contains(&options.min_area_frac)
            || options
                .stroke
                .is_some_and(|v| !v.is_finite() || !(0.1..=100.0).contains(&v))
        {
            return Err(bad(
                "invalid channel/threshold, area fraction (0.0001..0.5), or stroke (0.1..100 px)",
            ));
        }
        let image = load_png_with_limit(png, 4_000_000).map_err(bad)?;
        if let Some([x, y, w, h]) = options.crop
            && (w == 0
                || h == 0
                || u64::from(x) + u64::from(w) > image.w as u64
                || u64::from(y) + u64::from(h) > image.h as u64)
        {
            return Err(bad(
                "crop must be nonempty and entirely inside the decoded image",
            ));
        }
        let opts = TraceOptions {
            channel: options.channel,
            threshold: options.threshold,
            invert: options.invert,
            crop: options.crop.map(|r| r.map(f64::from)),
            stroke: options.stroke,
            min_area_frac: options.min_area_frac,
            dashes: options.dashes,
            ..TraceOptions::default()
        };
        let traced = trace_image(&image, Some("browser-sheet.png"), &opts).map_err(bad)?;
        // Parse the original text, not a sorted Value: vertex order remains the trace's own.
        let text = traced.pattern.dumps(Some(1)) + "\n";
        let draft: PatternDoc = serde_json::from_str(&text).map_err(|e| bad(e.to_string()))?;
        if draft.pieces.is_empty() {
            return Err(bad(
                "no pieces found; try a different channel, crop or threshold",
            ));
        }
        let mut original = vec![
            ("draft.pattern.json".into(), text.into_bytes()),
            ("mask.png".into(), encode_png_gray(&traced.mask)),
        ];
        for (i, crop) in review::label_crops(&traced, &image)?
            .into_iter()
            .enumerate()
        {
            original.push((format!("labels/p{}.png", i + 1), crop.png));
        }
        Ok(Self {
            image,
            draft,
            source_sha256: sha256_hex(png),
            diagnostics: traced.diagnostics.to_value(),
            original,
        })
    }

    /// Apply names.json, optional scale targets (`[{reference,target_mm}]`) and label evidence to
    /// the ORIGINAL draft. A failed attempt returns no products and cannot mutate this sheet.
    /// All distances are explicit millimetres. Scale-fit residuals remain visible; an audit never
    /// certifies a real body, and no manufacturing exporter is called here.
    pub fn review(&self, names: &str, targets: &str, labels: &str) -> Result<Review, Refusal> {
        let names: OValue = parse(names)?;
        if !matches!(names, OValue::Object(_)) {
            return Err(bad("names must be a JSON object"));
        }
        let targets: Vec<Target> = parse(targets)?;
        let labels: Vec<Value> = parse(labels)?;
        if targets.len() > 64 || labels.len() > 512 {
            return Err(budget(
                "at most 64 scale targets and 512 label records are allowed",
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for target in &targets {
            if target.reference.is_empty()
                || target.reference.len() > 256
                || !seen.insert(&target.reference)
                || !target.target_mm.is_finite()
                || !(0.01..=100_000.0).contains(&target.target_mm)
            {
                return Err(bad(
                    "scale targets need unique references and positive lengths in 0.01..100000 mm",
                ));
            }
        }
        let mut doc = annotate(&self.draft, &names).map_err(bad)?;
        if doc.pieces.is_empty() {
            return Err(bad("annotation dropped every piece"));
        }
        let scale = if targets.is_empty() {
            Value::Null
        } else {
            let pairs: Vec<_> = targets
                .iter()
                .map(|t| (t.reference.clone(), t.target_mm))
                .collect();
            let (scaled, report) = fit_scale(&doc, &pairs).map_err(|e| bad(e.to_string()))?;
            doc = scaled;
            report
        };
        let errors = Pattern::from_doc(doc.clone()).structure_errors();
        if !errors.is_empty() {
            return Err(bad(errors.join("; ")));
        }
        let audit = audit(&doc, Some(&labels), &AuditOptions::default());
        let mut files = self.original.clone();
        files.extend([
            ("pattern.json".into(), json_bytes(&doc)?),
            (
                "overlay.png".into(),
                review::overlay(&doc, Some(&self.image))?,
            ),
            ("round0_audit.json".into(), json_bytes(&audit)?),
            (
                "round0_audit.png".into(),
                review::audit_image(&doc, &audit, Some(&self.image))?,
            ),
            ("names.json".into(), json_bytes(&names)?),
            (
                "scale-targets.json".into(),
                json_bytes(
                    &targets
                        .iter()
                        .map(|t| json!({"reference":t.reference,"target_mm":t.target_mm}))
                        .collect::<Vec<_>>(),
                )?,
            ),
            ("labels.json".into(), json_bytes(&labels)?),
        ]);
        let mut report = json!({
            "schema":"ftx-trace-workshop/1", "status":"REVIEW_ONLY",
            "source_sha256":self.source_sha256,
            "source_image_size":[self.image.w,self.image.h],
            "scale_status":doc.meta.as_ref().and_then(|m| m.get("scale_status")),
            "scale_fit":scale, "audit":audit, "diagnostics":self.diagnostics,
            "pieces":doc.pieces.iter().map(|p| json!({"id":p.id,"name":p.name,
                "edges":p.edges.iter().map(|e| &e.id).collect::<Vec<_>>(),
                "vertices":p.vertices.iter().map(|(id,_)| id).collect::<Vec<_>>()
            })).collect::<Vec<_>>(),
            "no_claim":"Review only: source interpretation and scale references are user evidence. No cutting files, body fit or sewability approval. Run garment/measurement checks before manufacture."
        });
        report["files"] = json!(
            files
                .iter()
                .map(|(name, bytes)| json!({"name":name,"bytes":bytes.len()}))
                .collect::<Vec<_>>()
        );
        files.push(("review.json".into(), json_bytes(&report)?));
        if files
            .iter()
            .try_fold(0usize, |n, (_, b)| n.checked_add(b.len()))
            .is_none_or(|n| n > MAX_OUTPUT)
        {
            return Err(budget("workshop output exceeds 64 MiB"));
        }
        Ok(Review { report, files })
    }
}
