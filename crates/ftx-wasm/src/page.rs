//! The page's round, the reference's `webapp.py` (`program_info`, `form_fields`, `body_file`, `run`): a body
//! from a size chart, tape readings or a measurements file; a measured draft, or someone else's pattern; flat
//! validation, the measurement gate, every export with its acceptance checks, the round status and its
//! basis; the documents, the exports and one zip, all in memory. The clock is an input (`today` dates tape
//! files and judges staleness, `created` stamps the exports), so a page and a test give the same answer.

use ftx_core::py;
use ftx_export::text::safe_name;
use ftx_export::{Ctx, ExportOptions, PyJson, Target, check_files, export_targets};
use ftx_measure::{
    canonical_name, catalogue, fuse, garment_checks, make_record, readiness, reference_wearer,
    round_readiness, unit, validate as validate_measurements,
};
use ftx_pattern::draft::{self, PROGRAMS};
use ftx_pattern::round::round_status;
use ftx_pattern::{Measurements, Pattern, default_fit_spec, format_table, validate};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const LABELS: [&str; 3] = [
    "Zouave jacket (open bolero, set-in sleeves)",
    "Zouave trousers (full legs, elastic waist, cuffs)",
    "Fez (cone crown, round tip)",
];

/// The files a home sewist or a cutting service asks for first; everything is also in the zip.
const KEY_FILES: [(&str, &str); 8] = [
    ("_LETTER.pdf", "Print at home, US Letter (tiled, 100 %)"),
    ("_A4.pdf", "Print at home, A4 (tiled, 100 %)"),
    ("_PROJECTOR.pdf", "Projector (full size)"),
    ("_COPYSHOP_A0.pdf", "Copy shop, A0"),
    ("_master_mm.svg", "Master SVG (mm)"),
    ("_AAMA_METRIC.dxf", "DXF-AAMA for cutting services"),
    ("_plot.plt", "HPGL plotter / cutter"),
    (
        "_soutache_README.txt",
        "Soutache: how to use the embroidery-machine files (DST, in the zip)",
    ),
];

const BROWSER_SIM: &str =
    "not run (the Blender fold-up needs the command line: `fab round WS --sim`)";
const FLAT_ONLY: &str = "not applicable (no piece has a fold-up placement; judged flat)";

/// A finished round: what the page shows, the files written (`/`-separated paths) and their zip.
#[derive(Clone, Debug)]
pub struct Round {
    /// The page's result, in the reference's key order.
    pub page: PyJson,
    /// The documents and the exports (`exports/…`), in the order made.
    pub files: Vec<(String, Vec<u8>)>,
    /// The zip's file name (empty for a BLOCKED pattern).
    pub zip_name: String,
    /// Every file in one zip (empty for a BLOCKED pattern).
    pub zip: Vec<u8>,
}

fn msg(r: ftx_core::Refusal) -> String {
    r.message
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Python `float(x)` of a JSON number or numeric string.
fn number(v: &PyJson) -> Result<f64, String> {
    match v {
        PyJson::Int(i) => Ok(*i as f64),
        PyJson::Float(f) => Ok(*f),
        PyJson::Str(s) => s
            .trim()
            .parse::<f64>()
            .map_err(|_| format!("could not convert string to float: {}", py::repr_str(s))),
        PyJson::Bool(b) => Ok(f64::from(u8::from(*b))),
        _ => Err(format!(
            "float() argument must be a string or a real number, not {v:?}"
        )),
    }
}

/// Python `str(x)` of a JSON scalar.
fn py_str(v: &PyJson) -> String {
    match v {
        PyJson::Int(i) => i.to_string(),
        PyJson::Float(f) => py::repr_f64(*f),
        PyJson::Str(s) => s.clone(),
        PyJson::Bool(b) => if *b { "True" } else { "False" }.into(),
        PyJson::Null => "None".into(),
        other => other.dumps(None),
    }
}

/// Python `format(x, "g")`: six significant digits, trailing zeros dropped, exponent outside 1e-4..1e6.
fn py_g(x: f64) -> String {
    if x == 0.0 {
        return "0".into();
    }
    let sci = format!("{x:.5e}");
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let trim = |s: String| {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s
        }
    };
    if (-4..6).contains(&exp) {
        trim(format!("{:.*}", (5 - exp).max(0) as usize, x))
    } else {
        format!(
            "{}e{}{:02}",
            trim(mant.to_string()),
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        )
    }
}

fn style_defaults(program: &str) -> Value {
    match program {
        "zouave_bolero" => draft::bolero::defaults(),
        "zouave_trousers" => draft::trousers::defaults(),
        "fez" => draft::fez::defaults(),
        _ => json!({}),
    }
}

/// `program_info()`: per program its label and style defaults, in the registry's order.
#[must_use]
pub fn program_info() -> PyJson {
    PyJson::Dict(
        PROGRAMS
            .iter()
            .zip(LABELS)
            .map(|(p, label)| {
                let info = PyJson::dict(vec![
                    ("label", PyJson::str(label)),
                    ("style", PyJson::from_value(&style_defaults(p))),
                ]);
                ((*p).to_string(), info)
            })
            .collect(),
    )
}

fn must_first(mut rows: Vec<Value>) -> Vec<Value> {
    rows.sort_by_key(|r| r["role"] != "must");
    rows
}

/// `form_fields(program, stature_cm, age_months, nappies)`: the tape measurements a garment needs (musts
/// first), with how to take each and its chart value; for someone else's pattern (no or an unknown program)
/// what any of the programs needs.
///
/// # Errors
/// A stature or age the charts do not cover.
pub fn form_fields(
    program: Option<&str>,
    stature_cm: f64,
    age_months: Option<f64>,
    nappies: bool,
    today: &str,
) -> Result<Vec<Value>, String> {
    let Some(program) = program.filter(|p| PROGRAMS.contains(p)) else {
        let mut union: Vec<Value> = Vec::new();
        for p in PROGRAMS {
            for r in form_fields(Some(p), stature_cm, age_months, nappies, today)? {
                match union.iter_mut().find(|u| u["name"] == r["name"]) {
                    Some(slot) if r["role"] == "must" => *slot = r,
                    Some(_) => {}
                    None => union.push(r),
                }
            }
        }
        return Ok(must_first(union));
    };
    let mut reference = reference_wearer(stature_cm * 10.0, age_months, None, None).map_err(msg)?;
    reference["wearer"]["nappies"] = json!(nappies);
    let (pattern, _) = draft::draft(program, &reference, None).map_err(msg)?;
    let mut rows = Vec::new();
    let mut seen = vec!["stature".to_string()];
    for (kind, options) in garment_checks(&pattern) {
        let rep = readiness(&reference, kind, &options, today).map_err(msg)?;
        for item in rep["items"].as_array().into_iter().flatten() {
            let name = item["name"].as_str().unwrap_or("").to_string();
            if seen.contains(&name) {
                continue;
            }
            seen.push(name.clone());
            let deg = unit(&name) == "deg";
            let v = &reference["fused"][&name];
            let chart = if deg {
                v["value_deg"].as_f64()
            } else {
                v["value_mm"].as_f64().map(|x| x / 10.0)
            };
            rows.push(
                json!({"name": name, "role": item["role"], "unit": if deg { "deg" } else { "cm" },
                "how": catalogue()[&name]["how"], "chart": chart.map(|c| py::round(c, 1))}),
            );
        }
    }
    Ok(must_first(rows))
}

/// `body_file(body)`: a fused `fabric-measurements/1` file from the page's body section.
///
/// # Errors
/// The reference's messages: a stature outside 50–220 cm, an unknown measurement, a reading that is not a
/// measurement, an invalid file.
pub fn body_file(body: &PyJson, today: &str) -> Result<Value, String> {
    let get = |k: &str| body.get(k).cloned().unwrap_or(PyJson::Null);
    let mode = match get("mode") {
        PyJson::Str(s) => s,
        _ => "chart".into(),
    };
    if mode == "file" {
        let data = get("measurements").to_value();
        let errs = validate_measurements(&data);
        if !errs.is_empty() {
            return Err(format!(
                "measurements file: {}",
                errs[..errs.len().min(5)].join("; ")
            ));
        }
        return if truthy(&data["fused"]) {
            Ok(data)
        } else {
            fuse(&data, true).map_err(msg)
        };
    }
    let stature = number(&get("stature_cm"))? * 10.0;
    if !(500.0..=2200.0).contains(&stature) {
        return Err(format!(
            "stature {} cm: enter centimetres (50–220)",
            py_g(stature / 10.0)
        ));
    }
    let age = match get("age_months") {
        PyJson::Null => None,
        PyJson::Str(s) if s.is_empty() => None,
        v => Some(number(&v)?),
    };
    let sex = match get("sex") {
        PyJson::Str(s) if !s.is_empty() => Some(s),
        _ => None,
    };
    if mode == "chart" {
        return reference_wearer(stature, age, sex.as_deref(), None).map_err(msg);
    }
    let mut wearer =
        json!({"label": "tape measurements (browser)", "age_months": age, "stature_mm": stature});
    if let Some(s) = &sex {
        wearer["sex"] = json!(s);
    }
    if truthy(&get("nappies").to_value()) {
        wearer["nappies"] = json!(true);
    }
    let mut records = Vec::new();
    let method = "tape, entered in the browser";
    let sigma = |name: &str| catalogue()[name]["tape_sigma"].as_f64();
    records.push(
        make_record(
            "stature",
            stature,
            sigma("stature").unwrap_or(10.0),
            "MEASURED_TAPE",
            method,
            None,
            "",
        )
        .map_err(msg)?,
    );
    if let Some(PyJson::Dict(tape)) = body.get("tape") {
        for (name, value) in tape {
            if matches!(value, PyJson::Null) || matches!(value, PyJson::Str(s) if s.is_empty()) {
                continue;
            }
            let name = canonical_name(name);
            if catalogue().get(&name).is_none() {
                return Err(format!("unknown measurement {}", py::repr_str(&name)));
            }
            let deg = unit(&name) == "deg";
            let v = if deg {
                number(value)?
            } else {
                number(value)? * 10.0
            };
            if v <= 0.0 {
                return Err(format!("{name}: {} is not a measurement", py_str(value)));
            }
            let s = sigma(&name).filter(|&s| s != 0.0).unwrap_or(10.0);
            records.push(make_record(&name, v, s, "MEASURED_TAPE", method, None, "").map_err(msg)?);
        }
    }
    let raw = json!({"schema": "fabric-measurements/1", "wearer": wearer, "taken_on": today,
        "records": records, "fused": {}});
    let errs = validate_measurements(&raw);
    if !errs.is_empty() {
        return Err(errs[..errs.len().min(5)].join("; "));
    }
    fuse(&raw, true).map_err(msg)
}

fn doc(v: &PyJson) -> Vec<u8> {
    (v.dumps(Some(1)) + "\n").into_bytes()
}

/// Paths compared component by component, as Python sorts `Path`s.
fn path_key(p: &str) -> Vec<&str> {
    p.split('/').collect()
}

fn has_fold_up(data: &Value) -> bool {
    data["pieces"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|p| truthy(&p["placement"]) && p["sim"] != false)
}

/// `run(req)`: one browser round.
///
/// # Errors
/// The request is not JSON, or the reference's message for a body it refuses or an unknown program; a
/// draft or export the library refuses.
#[allow(clippy::too_many_lines)]
pub fn run(req_text: &str, today: &str, created: &str) -> Result<Round, String> {
    let req = PyJson::parse(req_text)?;
    let field = |k: &str| req.get(k).cloned().unwrap_or(PyJson::Null);
    let body = field("body");
    let meas = body_file(&body, today)?;
    let pattern_in = field("pattern");
    let (data, data_doc, spec) = if truthy(&pattern_in.to_value()) {
        let spec = Some(field("fit_spec").to_value()).filter(truthy);
        (pattern_in.to_value(), pattern_in, spec)
    } else {
        let program = match field("program") {
            PyJson::Str(s) => s,
            PyJson::Null => return Err("'program'".into()),
            other => py_str(&other),
        };
        if !PROGRAMS.contains(&program.as_str()) {
            return Err(format!("unknown program {}", py::repr_str(&program)));
        }
        // `dict(req.get("style") or {})`: nothing (or anything falsy) is no override; a non-mapping refuses
        let mut style = match field("style").to_value() {
            Value::Object(o) => o,
            v if !truthy(&v) => serde_json::Map::new(),
            v => return Err(format!("style must be an object of overrides, not {v}")),
        };
        if truthy(&body.get("nappies").map_or(Value::Null, PyJson::to_value))
            && style_defaults(&program).get("nappy").is_some()
        {
            style.insert("nappy".into(), json!(true)); // seat over the nappy, a longer rise
        }
        let style = (!style.is_empty()).then_some(Value::Object(style));
        let (d, s) = draft::draft(&program, &meas, style.as_ref()).map_err(msg)?;
        let dd = PyJson::from_value(&d);
        (d, dd, Some(s))
    };
    let pat = Pattern::new(data.clone());
    let errs = pat.structure_errors();
    if !errs.is_empty() {
        let page = PyJson::dict(vec![
            ("status", PyJson::str("BLOCKED")),
            (
                "errors",
                PyJson::List(errs.into_iter().map(PyJson::Str).collect()),
            ),
        ]);
        return Ok(Round {
            page,
            files: Vec::new(),
            zip_name: String::new(),
            zip: Vec::new(),
        });
    }
    let spec = spec.unwrap_or_else(|| default_fit_spec(&pat, "generic"));
    let val = validate(&pat, &spec, &Measurements(meas.clone()));
    let val_v = serde_json::to_value(&val).map_err(|e| e.to_string())?;
    let flat = val_v["status"].as_str().unwrap_or("BLOCKED").to_string();
    let ready = round_readiness(&data, &meas, today);
    let ready_status = ready["status"]
        .as_str()
        .unwrap_or("NOT_CHECKED")
        .to_string();
    let style = data["style"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or("STYLE");
    let stem = safe_name(style, 20);
    let mut files: Vec<(String, Vec<u8>)> = vec![
        ("pattern.json".into(), doc(&data_doc)),
        ("fit_spec.json".into(), doc(&PyJson::from_value(&spec))),
        ("measurements.json".into(), doc(&PyJson::from_value(&meas))),
        ("validation.json".into(), doc(&PyJson::from_value(&val_v))),
        ("readiness.json".into(), doc(&PyJson::from_value(&ready))),
    ];
    let unscaled = data["meta"]["scale_status"] == "UNSCALED";
    let (exports, skip) = if flat == "BLOCKED" {
        (None, Some("skipped (validation BLOCKED)"))
    } else if unscaled {
        (
            None,
            Some("skipped (pattern UNSCALED: true-size files need `vectorize scale-fit` first)"),
        )
    } else {
        let stamp = data["meta"]["created"]
            .as_str()
            .unwrap_or(created)
            .to_string();
        let ctx = Ctx::new(
            &pat,
            ExportOptions {
                created: Some(stamp),
                ..ExportOptions::default()
            },
        )
        .map_err(msg)?;
        let set = export_targets(&ctx, &Target::ALL, None).map_err(msg)?;
        let map: BTreeMap<String, Vec<u8>> = set.files.iter().cloned().collect();
        let items = check_files(&map, "exports", Some(&pat));
        let passed = items.iter().filter(|i| i.ok).count();
        let ok = passed == items.len();
        let failures: Vec<PyJson> = items
            .iter()
            .filter(|i| !i.ok)
            .map(|i| PyJson::Str(format!("FAIL {} [{}] {}", i.check, i.file, i.detail)))
            .collect();
        let checks = format!(
            "{}: {passed}/{} checks",
            if ok { "PASS" } else { "FAIL" },
            items.len()
        );
        files.extend(
            set.files
                .into_iter()
                .map(|(name, bytes)| (format!("exports/{name}"), bytes)),
        );
        let ex = PyJson::dict(vec![
            ("pass", PyJson::Bool(ok)),
            ("checks", PyJson::str(checks)),
            ("failures", PyJson::List(failures)),
            ("skipped", PyJson::List(Vec::new())),
        ]);
        (Some(ex), None)
    };
    let ex_v = exports.as_ref().map(PyJson::to_value);
    let status = round_status(
        &flat,
        None,
        ex_v.as_ref(),
        &ready_status,
        skip.is_some_and(|s| s.contains("UNSCALED")),
    );
    let ex_basis = match (&exports, skip) {
        (Some(e), _) => e.get("checks").cloned().unwrap_or(PyJson::Null),
        (None, Some(s)) => PyJson::str(s),
        (None, None) => PyJson::Null,
    };
    let basis = PyJson::dict(vec![
        ("flat", PyJson::str(&flat)),
        (
            "sim",
            PyJson::str(if has_fold_up(&data) {
                BROWSER_SIM
            } else {
                FLAT_ONLY
            }),
        ),
        ("exports", ex_basis),
        ("measurements", PyJson::str(&ready_status)),
    ]);
    let zip_name = format!("{stem}_pattern_files.zip");
    let mut sorted: Vec<&(String, Vec<u8>)> = files.iter().collect();
    sorted.sort_by(|a, b| path_key(&a.0).cmp(&path_key(&b.0)));
    let owned: Vec<(String, Vec<u8>)> = sorted.into_iter().cloned().collect();
    let zip = crate::zip::zip(&owned, created);
    let file = |name: &str| files.iter().find(|(n, _)| n == name).map(|(_, b)| b);
    let key: Vec<PyJson> = KEY_FILES
        .iter()
        .filter_map(|(suffix, label)| {
            let name = format!("{stem}{suffix}");
            file(&format!("exports/{name}")).map(|b| {
                PyJson::dict(vec![
                    ("label", PyJson::str(*label)),
                    ("name", PyJson::str(&name)),
                    (
                        "bytes",
                        PyJson::Int(i64::try_from(b.len()).unwrap_or(i64::MAX)),
                    ),
                ])
            })
        })
        .collect();
    let svg = file(&format!("exports/{stem}_master_mm.svg"))
        .map(|b| String::from_utf8_lossy(b).into_owned());
    let meta = &data["meta"];
    let title = meta["title"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map_or(stem.clone(), str::to_string);
    let page = PyJson::dict(vec![
        ("status", PyJson::str(status)),
        ("basis", basis),
        ("validation", PyJson::from_value(&val_v)),
        ("readiness", PyJson::from_value(&ready)),
        ("exports", exports.unwrap_or(PyJson::Null)),
        ("table", PyJson::str(format_table(&val))),
        ("solved", PyJson::from_value(&meta["solved"])),
        ("title", PyJson::str(title)),
        ("zip", PyJson::str(&zip_name)),
        ("files", PyJson::List(key)),
        ("preview_svg", svg.map_or(PyJson::Null, PyJson::Str)),
    ]);
    Ok(Round {
        page,
        files,
        zip_name,
        zip,
    })
}

#[cfg(test)]
mod tests {
    use super::py_g;

    #[test]
    fn python_g_format() {
        for (x, s) in [
            (9.0, "9"),
            (9.5, "9.5"),
            (250.0, "250"),
            (0.0001, "0.0001"),
            (1e-5, "1e-05"),
            (2_500_000.0, "2.5e+06"),
        ] {
            assert_eq!(py_g(x), s);
        }
    }
}
