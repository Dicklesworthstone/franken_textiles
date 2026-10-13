//! `tests/fixtures/oracle/trace/raster_trace.json` (captured by `tools/oracle/capture_raster.py`) against
//! `ftx_trace::trace`: the whole trace, image to draft, on synthetic sheets (outlines, fills, a coloured
//! panel, a dashed fold, crop and erase, every channel, knobs, a border piece, the Hough grain fallback,
//! refusals) and, from their masks, the three noisy benchmark sheets.

use ftx_conformance::{Compare, fixture_dir};
use ftx_export::pyjson::PyJson;
use ftx_trace::raster::image::{Gray, load_png};
use ftx_trace::trace::{MaskSource, Trace, TraceOptions, trace_image, trace_mask};

fn base64(s: &str) -> Vec<u8> {
    let val = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            _ => 63,
        }
    };
    let bytes: Vec<u8> = s.bytes().filter(|&c| c != b'=').collect();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let n = chunk.iter().fold(0u32, |acc, &c| (acc << 6) | val(c)) << (6 * (4 - chunk.len()));
        let take = chunk.len() * 6 / 8;
        out.extend(&n.to_be_bytes()[1..=take]);
    }
    out
}

fn get<'a>(v: &'a PyJson, key: &str) -> &'a PyJson {
    v.get(key).unwrap_or_else(|| panic!("missing {key}"))
}

fn num(v: &PyJson) -> f64 {
    match v {
        PyJson::Int(i) => *i as f64,
        PyJson::Float(f) => *f,
        _ => panic!("not a number: {v:?}"),
    }
}

fn text(v: &PyJson) -> &str {
    match v {
        PyJson::Str(s) => s,
        _ => panic!("not a string: {v:?}"),
    }
}

fn items(v: &PyJson) -> &[PyJson] {
    match v {
        PyJson::List(l) => l,
        _ => panic!("not a list: {v:?}"),
    }
}

fn quad(v: &PyJson) -> [f64; 4] {
    let l = items(v);
    [num(&l[0]), num(&l[1]), num(&l[2]), num(&l[3])]
}

/// The capture's keyword arguments as options.
fn options(kw: &PyJson) -> TraceOptions {
    let mut o = TraceOptions::default();
    let PyJson::Dict(kw) = kw else { panic!("opts") };
    for (k, v) in kw {
        match k.as_str() {
            "crop" => o.crop = Some(quad(v)),
            "erase" => o.erase = items(v).iter().map(quad).collect(),
            "channel" => o.channel = text(v).into(),
            "threshold" => o.threshold = text(v).into(),
            "invert" => o.invert = Some(*v == PyJson::Bool(true)),
            "bg_rgb" => {
                let l = items(v);
                o.bg_rgb = Some([num(&l[0]), num(&l[1]), num(&l[2])]);
            }
            "stroke" => o.stroke = Some(num(v)),
            "open_px" => o.open_px = Some(num(v) as i64),
            "close_px" => o.close_px = num(v) as i64,
            "corner_deg" => o.corner_deg = num(v),
            "soft_deg" => o.soft_deg = num(v),
            "simplify_frac" => o.simplify_frac = num(v),
            "smooth_px" => o.smooth_px = num(v),
            "min_area_frac" => o.min_area_frac = num(v),
            "marks" => o.marks = *v == PyJson::Bool(true),
            "dashes" => o.dashes = *v == PyJson::Bool(true),
            other => panic!("unknown option {other}"),
        }
    }
    o
}

fn gray(png: &PyJson) -> Gray {
    let bgr = load_png(&base64(text(png))).unwrap();
    Gray {
        w: bgr.w,
        h: bgr.h,
        data: bgr.data.iter().step_by(3).copied().collect(),
    }
}

#[test]
fn traces_match_the_reference() {
    let path = fixture_dir().join("trace/raster_trace.json");
    let doc = PyJson::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut cmp = Compare::new(1e-9);
    let (mut identical, mut total) = (0, 0);
    let mut fails = Vec::new();
    for case in items(get(&doc, "cases")) {
        let id = text(get(case, "id"));
        let (inp, out) = (get(case, "input"), get(case, "output"));
        let opts = options(get(inp, "opts"));
        total += 1;
        let got: Result<Trace, String> = if inp.get("from_mask").is_some() {
            let diag = get(out, "diagnostics");
            let pattern = get(out, "pattern");
            let size = items(get(get(get(pattern, "meta"), "source"), "image_size"));
            let bg = items(get(diag, "bg_rgb"));
            let src = MaskSource {
                source: Some(text(get(get(get(pattern, "meta"), "source"), "image"))),
                image_size: (num(&size[0]) as usize, num(&size[1]) as usize),
                origin: (0, 0),
                erased: Vec::new(),
                retries: items(get(diag, "retries"))
                    .iter()
                    .map(|r| text(r).to_string())
                    .collect(),
                threshold: get(diag, "threshold").clone(),
                bg_rgb: [num(&bg[0]) as i64, num(&bg[1]) as i64, num(&bg[2]) as i64],
            };
            trace_mask(gray(get(out, "mask_png")), &src, &opts)
        } else {
            let img = load_png(&base64(text(get(inp, "png")))).unwrap();
            trace_image(&img, Some("sheet.png"), &opts)
        };
        match (got, out.get("error")) {
            (Err(e), Some(_)) => {
                let want = text(get(out, "message"));
                if e == want {
                    identical += 1;
                } else {
                    fails.push(format!(
                        "{id}: refused with {e:?}, the reference with {want:?}"
                    ));
                }
            }
            (Ok(_), Some(_)) => fails.push(format!("{id}: traced; the reference refused")),
            (Err(e), None) if opts.threshold == "adaptive" => {
                // the adaptive threshold is not ported: refused, recorded in the contract
                assert!(e.contains("adaptive"), "{id}: {e}");
                println!("{id}: refused (adaptive threshold not ported)");
            }
            (Err(e), None) => fails.push(format!("{id}: {e}")),
            (Ok(t), None) => {
                if t.mask != gray(get(out, "mask_png")) {
                    fails.push(format!("{id}: mask differs"));
                    continue;
                }
                let (g, w) = (t.pattern.dumps(Some(1)), get(out, "pattern").dumps(Some(1)));
                let (gd, wd) = (
                    t.diagnostics.dumps(None),
                    get(out, "diagnostics").dumps(None),
                );
                if g == w && gd == wd {
                    identical += 1;
                    continue;
                }
                let before = cmp.failures.len();
                cmp.json(id, &t.pattern.to_value(), &get(out, "pattern").to_value());
                cmp.json(
                    &format!("{id} diagnostics"),
                    &t.diagnostics.to_value(),
                    &get(out, "diagnostics").to_value(),
                );
                if cmp.failures.len() == before {
                    println!("{id}: same values, other key order or number spelling");
                }
            }
        }
    }
    println!("{identical} of {total} traces byte-identical");
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    cmp.finish("trace drafts");

    // the comparison is not vacuous: a slightly different smoothing gives another draft
    let case = items(get(&doc, "cases"))
        .iter()
        .find(|c| text(get(c, "id")) == "outlines")
        .unwrap();
    let img = load_png(&base64(text(get(get(case, "input"), "png")))).unwrap();
    let opts = TraceOptions {
        smooth_px: 1.25,
        ..TraceOptions::default()
    };
    let other = trace_image(&img, Some("sheet.png"), &opts).unwrap();
    assert_ne!(
        other.pattern.dumps(Some(1)),
        get(get(case, "output"), "pattern").dumps(Some(1))
    );
}
