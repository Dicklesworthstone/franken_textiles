//! Paired sewing-path sensitivity (plan §11.1, §11.9 KAT 10).
//!
//! Run the same garment, body and physics with two different zero-gravity sewing durations. This
//! exercises the existing frankensim-derived Newton/LDLT/contact pipeline twice, not a second solver
//! or a rescaled garment. The primary animation is retained; both reports and final-state digests
//! record the comparison. Two agreeing paths are neither a statistical confidence interval nor proof
//! of a unique equilibrium, and never establish fit on a real wearer.

use std::collections::BTreeSet;

use ftx_core::{Refusal, RefusalCode, Vec3, receipt::sha256_hex};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

use crate::run::{DrapeBody, DrapeOptions, DrapeRun};

/// Explicit review policy for two sewing paths. All distances are millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TwoStartOptions {
    /// Number of zero-gravity steps in the alternate run; must differ from the primary.
    pub alternate_sew_steps: usize,
    /// Maximum displacement of ANY corresponding final mesh vertex in the fixed body frame.
    pub vertex_mm: f64,
    /// Maximum difference of corresponding hem heights (KAT 10's review band is 6 mm).
    pub hem_mm: f64,
    /// Maximum difference of landmark mean clearances (KAT 10: 3 mm).
    pub clearance_mm: f64,
    /// Maximum difference of seam p90 strain, in percentage points (KAT 10: 1.5).
    pub seam_strain_pp: f64,
}

impl Default for TwoStartOptions {
    fn default() -> Self {
        Self {
            alternate_sew_steps: 18,
            vertex_mm: 10.0,
            hem_mm: 6.0,
            clearance_mm: 3.0,
            seam_strain_pp: 1.5,
        }
    }
}

impl TwoStartOptions {
    /// Validate both complete schedules and the explicit review bounds before either solve.
    ///
    /// # Errors
    /// Invalid run settings, identical sewing durations, or non-finite/nonpositive bounds.
    pub fn validate(&self, primary: &DrapeOptions) -> Result<(), Refusal> {
        primary.validate()?;
        if self.alternate_sew_steps == primary.sew_steps {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "two-start sewing durations must differ; repeating the same path is not a sensitivity check",
            ));
        }
        DrapeOptions {
            sew_steps: self.alternate_sew_steps,
            ..*primary
        }
        .validate()?;
        for (name, bound) in [
            ("vertex spread", self.vertex_mm),
            ("hem spread", self.hem_mm),
            ("clearance spread", self.clearance_mm),
            ("seam strain spread", self.seam_strain_pp),
        ] {
            Refusal::finite(name, bound)?;
            if bound <= 0.0 {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!("{name} bound must be positive"),
                ));
            }
        }
        Ok(())
    }

    fn json(&self) -> Value {
        json!({
            "alternate_sew_steps": self.alternate_sew_steps,
            "vertex_mm": self.vertex_mm,
            "hem_mm": self.hem_mm,
            "clearance_mm": self.clearance_mm,
            "seam_strain_pp": self.seam_strain_pp,
        })
    }
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn flag(report: &mut Value, text: &str) -> Result<(), Refusal> {
    // These are not tightness flags: qualification must never demote path sensitivity.
    for key in ["flags", "gating"] {
        let list = report[key]
            .as_array_mut()
            .ok_or_else(|| invalid(format!("drape report lacks {key}")))?;
        if !list.iter().any(|v| v.as_str() == Some(text)) {
            list.push(json!(text));
        }
    }
    Ok(())
}

fn digest(x: &[Vec3]) -> String {
    let bytes: Vec<u8> = x
        .iter()
        .flat_map(|p| [p.x, p.y, p.z])
        .flat_map(f64::to_le_bytes)
        .collect();
    sha256_hex(&bytes)
}

fn geometry(a: &[Vec3], b: &[Vec3]) -> Result<Value, Refusal> {
    if a.is_empty() || a.len() != b.len() {
        return Err(invalid(
            "two-start final meshes must have matching nonempty vertex sets",
        ));
    }
    let mut worst = (0, 0.0f64);
    let mut sum = 0.0;
    for (i, (a, b)) in a.iter().zip(b).enumerate() {
        if !a.is_finite() || !b.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "non-finite two-start final position",
            ));
        }
        let d2 = (*a - *b).norm2() * 1e6;
        sum += d2;
        Refusal::finite("two-start squared displacement", sum)?;
        if d2 > worst.1 {
            worst = (i, d2);
        }
    }
    Ok(json!({
        "vertices": a.len(), "rms_mm": (sum / a.len() as f64).sqrt(),
        "max_mm": worst.1.sqrt(), "worst_vertex": worst.0,
        "primary_sha256": digest(a), "alternate_sha256": digest(b),
        "frame": "unchanged body frame; no best-fit alignment",
    }))
}

fn compare_metrics(
    a: &Value,
    b: &Value,
    policy: &TwoStartOptions,
) -> Result<(Value, Vec<String>), Refusal> {
    let mut rows = Vec::new();
    let mut flags = Vec::new();
    for (group, field, units, bound) in [
        ("hem_heights_mm", None, "mm", policy.hem_mm),
        (
            "landmarks",
            Some("clearance_mean_mm"),
            "mm",
            policy.clearance_mm,
        ),
        (
            "seams",
            Some("seam_strain_p90_pct"),
            "percentage_points",
            policy.seam_strain_pp,
        ),
    ] {
        let left = a[group]
            .as_object()
            .ok_or_else(|| invalid(format!("primary report lacks {group}")))?;
        let right = b[group]
            .as_object()
            .ok_or_else(|| invalid(format!("alternate report lacks {group}")))?;
        // Union, not intersection: a missing reading must not disappear from the comparison.
        let keys: BTreeSet<_> = left.keys().chain(right.keys()).collect();
        for key in keys {
            let number = |v: Option<&Value>| {
                v.and_then(|v| field.map_or(Some(v), |f| v.get(f)))
                    .and_then(Value::as_f64)
                    .filter(|n| n.is_finite())
            };
            let (x, y) = (number(left.get(key)), number(right.get(key)));
            let delta = x
                .zip(y)
                .map(|(x, y)| (x - y).abs())
                .filter(|n| n.is_finite());
            let ok = delta.is_some_and(|d| d <= bound);
            if !ok {
                flags.push(match delta {
                    Some(d) => {
                        format!("two-start {group}/{key}: spread {d:.3} {units} exceeds {bound}")
                    }
                    None => format!("two-start {group}/{key}: missing or non-finite comparison"),
                });
            }
            rows.push(json!({
                "group": group, "id": key, "field": field, "units": units,
                "primary": x, "alternate": y, "spread": delta, "bound": bound, "passed": ok,
            }));
        }
    }
    if rows.is_empty() {
        flags.push("two-start: no comparable garment metrics; vertex agreement alone does not qualify a drape".into());
    }
    Ok((json!(rows), flags))
}

fn run_health(name: &str, run: &DrapeRun, flags: &mut Vec<String>) -> Result<(), Refusal> {
    if run.report["settled"].as_bool() != Some(true) {
        flags.push(format!("two-start {name}: drape has not settled"));
    }
    for f in run.report["gating"]
        .as_array()
        .ok_or_else(|| invalid("drape report lacks gating"))?
    {
        let f = f
            .as_str()
            .ok_or_else(|| invalid("drape gating flag must be text"))?;
        flags.push(format!("two-start {name}: {f}"));
    }
    if run.sew.unconverged != 0 || run.drape.unconverged != 0 {
        flags.push(format!(
            "two-start {name}: {} sewing and {} draping steps did not converge",
            run.sew.unconverged, run.drape.unconverged,
        ));
    }
    // A reduced start offset is not the requested physical clearance.
    let achieved = run
        .scene
        .solver
        .contact
        .as_ref()
        .map_or(0.0, |c| c.params.thickness);
    if achieved < run.scene.target_thickness {
        flags.push(format!(
            "two-start {name}: body thickness ramp did not reach its target"
        ));
    }
    Ok(())
}

fn attach(
    primary: &mut DrapeRun,
    alternate: DrapeRun,
    opts: &DrapeOptions,
    policy: &TwoStartOptions,
) -> Result<(), Refusal> {
    let (a, b) = (&primary.scene.mesh, &alternate.scene.mesh);
    if a.instances != b.instances
        || a.verts2d != b.verts2d
        || a.faces != b.faces
        || a.owner != b.owner
        || a.inst_range != b.inst_range
        || a.pairs != b.pairs
        || primary.scene.weld != alternate.scene.weld
    {
        return Err(invalid(
            "two-start mesh identity changed; cannot compare corresponding vertices",
        ));
    }
    let positions = geometry(&primary.scene.positions(), &alternate.scene.positions())?;
    let (rows, mut flags) = compare_metrics(&primary.report, &alternate.report, policy)?;
    if positions["max_mm"].as_f64().unwrap() > policy.vertex_mm {
        flags.push(format!(
            "two-start vertex spread exceeds {} mm",
            policy.vertex_mm
        ));
    }
    run_health("primary", primary, &mut flags)?;
    run_health("alternate", &alternate, &mut flags)?;
    let health = |r: &DrapeRun| {
        json!({
            "sew_steps": r.sew.steps, "drape_steps": r.drape.steps,
            "sew_unconverged": r.sew.unconverged, "drape_unconverged": r.drape.unconverged,
            "max_speed_m_s": r.drape.max_speed, "thickness_m": r.drape.thickness,
        })
    };
    let primary_health = health(primary);
    let alternate_health = health(&alternate);
    let mut alternate_report = alternate.report;
    // The exported clip is the primary's; do not create dangling references to a second fold.pc2.
    alternate_report
        .as_object_mut()
        .ok_or_else(|| invalid("alternate report must be an object"))?
        .remove("animation");
    let comparison = json!({
        "schema": "ftx-two-start/1", "method": "two_sewing_durations", "sampled_paths": 2,
        "status": if flags.is_empty() { "WITHIN_BOUNDS" } else { "REVIEW_REQUIRED" },
        "policy": policy.json(),
        "shared_settings": {"step_s": opts.step, "drape_steps": opts.drape_steps, "tol_m_s": opts.tol,
            "spacing_mm": opts.spacing, "pcg_iters": opts.pcg_iters, "lag_rounds": opts.lag_rounds,
            "compression": opts.compression, "elastic_ea_n": opts.elastic_ea},
        "primary": primary_health, "alternate": alternate_health,
        "positions": positions, "metrics": rows, "flags": flags,
        "alternate_report": alternate_report, "animation": "primary run only",
        "no_claim": "Two sewing paths, not distinct placements, a statistical confidence interval, a unique-equilibrium proof or real-wearer fit. Bounds are review policy, not calibrated accuracy.",
    });
    for f in &flags {
        flag(&mut primary.report, f)?;
    }
    primary.report["two_start"] = comparison;
    Ok(())
}

/// Run and assess two sewing paths. Only the primary animation is returned; the alternative's
/// metrics and diagnostics are retained in `report.two_start.alternate_report`.
///
/// The pattern, body, placement, material, solver settings and gravity phase stay fixed. Numerical
/// refusals from EITHER run propagate with their original code, so callers cannot publish a partial
/// pair as success. Completed but unsettled, unconverged, uncomparable or divergent pairs add gating
/// flags to the primary report. Calling the ordinary [`crate::run::drape`] remains a single run.
///
/// # Errors
/// Invalid schedules/bounds, a refusal from either drape, or incompatible report/mesh identity.
pub fn drape(
    pattern: &Pattern,
    fit_spec: &Value,
    body: &DrapeBody,
    opts: &DrapeOptions,
    policy: &TwoStartOptions,
    mut progress: impl FnMut(&str),
) -> Result<DrapeRun, Refusal> {
    policy.validate(opts)?;
    let mut solve = |name: &str, options: &DrapeOptions| {
        progress(&format!(
            "two-start {name}: {} sewing steps",
            options.sew_steps
        ));
        crate::run::drape(pattern, fit_spec, body, options, &mut progress).map_err(|mut r| {
            r.message = format!("two-start {name}: {}", r.message);
            r
        })
    };
    let mut primary = solve("primary", opts)?;
    let alternate = solve(
        "alternate",
        &DrapeOptions {
            sew_steps: policy.alternate_sew_steps,
            ..*opts
        },
    )?;
    attach(&mut primary, alternate, opts, policy)?;
    progress(&format!(
        "two-start: {}",
        primary.report["two_start"]["status"].as_str().unwrap()
    ));
    Ok(primary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> Value {
        json!({"hem_heights_mm": {"front": 30.0}, "landmarks": {"chest": {"clearance_mean_mm": 2.0}},
            "seams": {"side": {"seam_strain_p90_pct": 1.0}}, "flags": [], "gating": [], "tightness_gating": []})
    }

    #[test]
    fn metric_bounds_are_inclusive_and_percentage_points() {
        let a = report();
        let mut b = report();
        let policy = TwoStartOptions::default();
        b["hem_heights_mm"]["front"] = json!(36.0);
        b["landmarks"]["chest"]["clearance_mean_mm"] = json!(5.0);
        b["seams"]["side"]["seam_strain_p90_pct"] = json!(2.5);
        let (rows, flags) = compare_metrics(&a, &b, &policy).unwrap();
        assert!(flags.is_empty());
        assert_eq!(rows[2]["spread"], 1.5);
        b["seams"]["side"]["seam_strain_p90_pct"] = json!(2.5001);
        assert_eq!(compare_metrics(&a, &b, &policy).unwrap().1.len(), 1);
    }

    #[test]
    fn missing_readings_and_empty_coverage_never_pass() {
        let a = report();
        let mut b = report();
        let policy = TwoStartOptions::default();
        b["hem_heights_mm"].as_object_mut().unwrap().remove("front");
        b["landmarks"]["chest"]["clearance_mean_mm"] = Value::Null;
        b["seams"]["new"] = json!({"seam_strain_p90_pct": 1.0});
        assert_eq!(compare_metrics(&a, &b, &policy).unwrap().1.len(), 3);
        let empty = json!({"hem_heights_mm": {}, "landmarks": {}, "seams": {}});
        assert_eq!(compare_metrics(&empty, &empty, &policy).unwrap().1.len(), 1);
        assert!(compare_metrics(&a, &json!({}), &policy).is_err());
    }

    #[test]
    fn overflow_is_unavailable_not_a_zero_spread() {
        let mut a = report();
        let mut b = report();
        a["hem_heights_mm"]["front"] = json!(f64::MAX);
        b["hem_heights_mm"]["front"] = json!(-f64::MAX);
        let (rows, flags) = compare_metrics(&a, &b, &TwoStartOptions::default()).unwrap();
        assert_eq!(flags.len(), 1);
        assert!(rows[0]["spread"].is_null());
    }

    #[test]
    fn geometry_uses_the_body_frame_without_alignment() {
        let a = [Vec3::ZERO, Vec3::new(0.02, 0.0, 0.0)];
        let b = a.map(|p| p + Vec3::new(0.003, 0.004, 0.0));
        let g = geometry(&a, &b).unwrap();
        assert!((g["max_mm"].as_f64().unwrap() - 5.0).abs() < 1e-12);
        assert!((g["rms_mm"].as_f64().unwrap() - 5.0).abs() < 1e-12);
        assert_ne!(g["primary_sha256"], g["alternate_sha256"]);
        assert!(geometry(&a, &b[..1]).is_err());
        assert!(geometry(&[], &[]).is_err());
        assert!(geometry(&a, &[Vec3::new(f64::NAN, 0.0, 0.0); 2]).is_err());
    }

    #[test]
    fn invalid_policy_refuses_before_solving() {
        let opts = DrapeOptions::default();
        for policy in [
            TwoStartOptions {
                alternate_sew_steps: 12,
                ..TwoStartOptions::default()
            },
            TwoStartOptions {
                alternate_sew_steps: usize::MAX,
                ..TwoStartOptions::default()
            },
            TwoStartOptions {
                hem_mm: 0.0,
                ..TwoStartOptions::default()
            },
            TwoStartOptions {
                vertex_mm: f64::NAN,
                ..TwoStartOptions::default()
            },
            TwoStartOptions {
                clearance_mm: f64::INFINITY,
                ..TwoStartOptions::default()
            },
            TwoStartOptions {
                seam_strain_pp: -1.0,
                ..TwoStartOptions::default()
            },
        ] {
            assert!(policy.validate(&opts).is_err());
        }
        TwoStartOptions::default().validate(&opts).unwrap();
    }

    #[test]
    fn sensitivity_flags_cannot_be_demoted_as_tightness() {
        let mut r = report();
        flag(&mut r, "two-start review required").unwrap();
        flag(&mut r, "two-start review required").unwrap();
        assert_eq!(r["flags"], json!(["two-start review required"]));
        assert_eq!(r["gating"], r["flags"]);
        assert_eq!(r["tightness_gating"], json!([]));
    }
}
