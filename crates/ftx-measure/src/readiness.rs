//! Garment-specific evidence gates (spec 02 A.2.2 and A.7). This is not a real-fit certificate.

use crate::{date_day, definition, fuse, invalid, unit};
use ftx_core::Refusal;
use serde_json::{Value, json};

/// Options that change which measurements a garment needs and their uncertainty targets.
#[derive(Clone, Debug)]
pub struct ReadinessOptions<'a> {
    /// The bodice must pass over the hips.
    pub passes_hip: bool,
    /// The sleeve cuff must pass over the hand without a placket.
    pub closed_cuff: bool,
    /// Include nappy-covered seat; `None` inherits the wearer setting.
    pub nappies: Option<bool>,
    /// Relax the waist target to 30 mm for an elastic waist.
    pub elastic_waist: bool,
    /// Gathered-trouser cuff location: knee, calf or ankle (default calf).
    pub cuff_level: &'a str,
    /// The trouser cuff opens, so it need not pass over the foot.
    pub cuff_opening: bool,
    /// Use automatic chart priors in fusion (sanity bounds remain when false).
    pub use_priors: bool,
}

impl Default for ReadinessOptions<'_> {
    fn default() -> Self {
        Self {
            passes_hip: false,
            closed_cuff: false,
            nappies: None,
            elastic_waist: false,
            cuff_level: "calf",
            cuff_opening: false,
            use_priors: true,
        }
    }
}

/// Canonical garment kind, accepting the specification aliases.
pub fn garment_kind(kind: &str) -> Result<&'static str, Refusal> {
    match kind.trim().to_lowercase().replace('-', "_").as_str() {
        "bodice" | "jacket" | "bolero" | "waistcoat" => Ok("bodice"),
        "set_in_sleeve" | "sleeve" => Ok("set_in_sleeve"),
        "trousers" | "straight_trousers" | "pants" => Ok("trousers"),
        "full_trousers_gathered" | "zouave" | "harem" | "sirwal" => Ok("full_trousers_gathered"),
        "hat_fez" | "hat" | "fez" | "cap" => Ok("hat_fez"),
        _ => Err(invalid(format!(
            "unknown garment kind '{kind}'; use one of bodice, set_in_sleeve, trousers, full_trousers_gathered, hat_fez"
        ))),
    }
}

fn requirements(kind: &str) -> (Vec<&str>, Vec<&str>, &str) {
    match kind {
        "bodice" => (
            vec![
                "stature",
                "chest",
                "back_neck_to_waist",
                "neck_base",
                "across_back",
                "shoulder_length",
            ],
            vec!["waist", "shoulder_slope_deg", "belly_depth"],
            "03 §1.2 bodice row; should-list adds 06 Q3 (shoulder slope, belly depth for front-hem balance)",
        ),
        "set_in_sleeve" => (
            vec!["arm_length", "upper_arm"],
            vec!["wrist"],
            "03 §1.2 sleeve row; hand_pass becomes must with --closed-cuff",
        ),
        "trousers" => (
            vec![
                "waist",
                "hip",
                "body_rise",
                "outseam",
                "total_crotch_length",
            ],
            vec!["inseam", "thigh", "belly_depth"],
            "03 §1.2 trousers row; total crotch length promoted to must (06 Q3, defaults item 11)",
        ),
        "full_trousers_gathered" => (
            vec![
                "waist",
                "hip",
                "outseam",
                "body_rise",
                "total_crotch_length",
            ],
            vec!["thigh", "inseam", "belly_depth"],
            "03 §1.2 Zouave row + 06 Q4 pass-over inequalities; rise and crotch length are minimums (06 Q3)",
        ),
        _ => (vec!["head"], vec![], "03 §1.2 hat row"),
    }
}

/// Base requirements before applying garment options, for interactive measurement guidance.
pub fn garment_catalogue(kind: &str) -> Result<Value, Refusal> {
    let kind = garment_kind(kind)?;
    let (must, should, basis) = requirements(kind);
    let overrides = if kind == "full_trousers_gathered" {
        json!({"waist":30.0,"hip":20.0,"thigh":20.0})
    } else {
        json!({})
    };
    Ok(
        json!({"garment":kind,"must":must,"should":should,"basis":basis,"targets_override":overrides}),
    )
}

fn fmt(value: f64, u: &str, precision: usize) -> String {
    if u == "deg" {
        format!("{value:.precision$}°")
    } else {
        format!("{:.precision$} cm", value / 10.0)
    }
}

fn flagged(entry: &Value, flag: &str) -> bool {
    entry["flags"]
        .as_array()
        .is_some_and(|f| f.iter().any(|f| f == flag))
}

fn item_status(entry: Option<&Value>, sigma: f64, target: f64) -> &'static str {
    let Some(entry) = entry else {
        return "MISSING";
    };
    if flagged(entry, "REMEASURE") {
        "REMEASURE"
    } else if flagged(entry, "PRIOR_ONLY") {
        "PRIOR_ONLY"
    } else if entry["primary_provenance"] == "MODEL_FIT"
        || entry["sources"].as_array().is_some_and(|sources| {
            sources
                .iter()
                .any(|s| s["used"] == true && s["provenance"] == "MODEL_FIT")
        })
    {
        // A model contribution cannot become independent evidence by declaring a precise weight.
        "MODEL_FIT"
    } else if sigma <= target + 1e-9 {
        "OK"
    } else if sigma <= 2.0 * target + 1e-9
        && entry["sources"].as_array().is_some_and(|sources| {
            sources.iter().any(|s| {
                s["used"] == true
                    && matches!(
                        s["provenance"].as_str(),
                        Some("MEASURED_TAPE" | "SCAN_MEASURED" | "GARMENT_DERIVED")
                    )
            })
        })
    {
        "OK_TAPE_LIMITED"
    } else {
        "TOO_UNCERTAIN"
    }
}

/// Recomputes evidence and checks the selected garment. `as_of` is an explicit ISO date so native
/// and browser callers can produce identical staleness advisories without a hidden clock.
/// `blocking` contains measurement names; `remeasure_request` contains the corresponding instructions.
/// Reference wearers return `READY_REFERENCE`, retaining those instructions in `for_real_child`.
pub fn readiness(
    data: &Value,
    garment: &str,
    options: &ReadinessOptions<'_>,
    as_of: &str,
) -> Result<Value, Refusal> {
    let kind = garment_kind(garment)?;
    let today = date_day(as_of).ok_or_else(|| invalid("as_of must be YYYY-MM-DD"))?;
    let full = kind == "full_trousers_gathered";
    if full && !["knee", "calf", "ankle"].contains(&options.cuff_level) {
        return Err(invalid("cuff_level must be knee, calf or ankle"));
    }
    let data = fuse(data, options.use_priors)?;
    let reference = data["wearer"]["reference"] == true;
    let nappies = options.nappies.unwrap_or(data["wearer"]["nappies"] == true);
    let (mut must, should, basis) = requirements(kind);
    if kind == "bodice" && options.passes_hip {
        must.push("hip");
    }
    if kind == "set_in_sleeve" && options.closed_cuff {
        must.push("hand_pass");
    }
    if matches!(kind, "trousers" | "full_trousers_gathered") && nappies {
        must.push("seat_over_nappy");
    }
    if full {
        must.push(options.cuff_level);
        if !options.cuff_opening {
            must.push("foot_pass");
            if options.cuff_level == "knee" {
                must.push("calf");
            }
        }
    }
    let mut items = Vec::new();
    let mut blocking = Vec::new();
    let mut requests = Vec::new();
    let mut advisories = Vec::new();
    for (name, role) in must
        .iter()
        .map(|n| (*n, "must"))
        .chain(should.iter().map(|n| (*n, "should")))
    {
        let def = definition(name).expect("garment requirements are catalogue names");
        let u = unit(name);
        let target = match name {
            "waist" if full || kind == "trousers" && options.elastic_waist => 30.0,
            "hip" | "thigh" if full => 20.0,
            _ => def["target_sigma"]
                .as_f64()
                .or_else(|| def["tape_sigma"].as_f64())
                .unwrap_or(10.0),
        };
        let entry = data["fused"].get(name);
        let empty = Value::Null;
        let e = entry.unwrap_or(&empty);
        let vk = format!("value_{u}");
        let sk = format!("sigma_{u}");
        let value = e[&vk].as_f64().unwrap_or(0.0);
        let sigma = e[&sk].as_f64().unwrap_or(0.0);
        let status = item_status(entry, sigma, target);
        let mut item = json!({"name": name, "role": role, "status": status});
        item[format!("target_sigma_{u}")] = json!(target);
        if entry.is_some() {
            for key in [vk.as_str(), sk.as_str(), "primary_provenance", "flags"] {
                item[key] = e[key].clone();
            }
        }
        if flagged(e, "VERIFY_OUT_OF_BOUNDS") {
            advisories.push(format!(
                "{name}: {} is outside the chart P1–P99 for this height — confirm once.",
                fmt(value, u, 1)
            ));
        }
        if flagged(e, "Z_WARN") {
            advisories.push(format!(
                "{name}: sources disagree (z {}); fused, worth a second look.",
                e["z_max"]
            ));
        }
        if status == "OK_TAPE_LIMITED" {
            advisories.push(format!(
                "{name}: ±{} is the tape's limit (target ±{}); check it on the muslin.",
                fmt(sigma, u, 2),
                fmt(target, u, 2)
            ));
        }
        if matches!(status, "OK" | "OK_TAPE_LIMITED") && e["primary_provenance"] == "CHART_PRIOR" {
            advisories.push(format!("{name}: the size-chart prior outweighs the measurement here; fine for drafting, re-check on the muslin."));
        }
        if !matches!(status, "OK" | "OK_TAPE_LIMITED") {
            let have = match status {
                "MISSING" => "not measured yet".to_string(),
                "PRIOR_ONLY" => format!("only a size-chart guess (±{})", fmt(sigma, u, 1)),
                "REMEASURE" => "tape and photo disagree — re-measure".to_string(),
                "MODEL_FIT" => {
                    "includes a model estimate; take independent measurements".to_string()
                }
                _ => format!(
                    "have {} ±{} ({})",
                    fmt(value, u, 1),
                    fmt(sigma, u, 1),
                    e["primary_provenance"].as_str().unwrap_or("")
                ),
            };
            let line = format!(
                "{name}: {} Need ±{}; {have}.",
                def["how"].as_str().unwrap_or(""),
                fmt(target, u, 1)
            );
            if role == "must" {
                blocking.push(name);
                requests.push(line);
            } else {
                advisories.push(format!("optional — {line}"));
            }
        }
        items.push(item);
    }
    if !reference && let Some(taken) = data["taken_on"].as_str().and_then(date_day) {
        let days = today - taken;
        if days > 42 {
            advisories.push(format!(
                "STALE: measured {days} days ago; a toddler grows ~0.7 cm/month — re-take lengths."
            ));
        }
    }
    let status = if reference {
        "READY_REFERENCE"
    } else if blocking.is_empty() {
        "READY"
    } else {
        "NOT_READY"
    };
    let mut out = json!({"schema":"fabric-readiness/1", "garment":kind, "status":status, "basis":basis,
        "options":{"passes_hip":options.passes_hip,"closed_cuff":options.closed_cuff,"nappies":nappies,
            "elastic_waist":options.elastic_waist,"cuff_level":if full {Some(options.cuff_level)} else {None},"cuff_opening":options.cuff_opening},
        "items":items,"blocking":blocking,"remeasure_request":requests,"advisories":advisories});
    if reference {
        out["for_real_child"] = out["remeasure_request"].clone();
        out["blocking"] = json!([]);
        out["remeasure_request"] = json!([]);
        out["note"] = json!(
            "Reference wearer: values are definitions built from public priors; σ is population spread. A real child would still need the list in for_real_child."
        );
    }
    Ok(out)
}
