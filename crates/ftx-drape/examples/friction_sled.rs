//! Generate the native synthetic friction-sled report for the browser friction exhibit (`ftx-13b`).
//!
//! Runs the production `ftx_pattern::material_friction::calibrate` on the bundled synthetic
//! steady-sliding fixture (`tests/fixtures/materials/synthetic-friction-sliding.json`) plus a
//! scaled higher-grip synthetic state, then steps the production `ftx_drape::Solver` and
//! `ftx_drape::Contact` (IPC barrier + lagged smoothed Coulomb friction) through four declared
//! tangential load-ramp histories. Also audits real catalog textiles to retain their explicit
//! missing-friction unavailability without fabricating coefficients.
//!
//! Run from repo root:
//!   CARGO_TARGET_DIR=/tmp/ftx_sled_target cargo run --release -p ftx-drape --example friction_sled \
//!     > showcase/public/materials/friction-sled.json
//!
//! Units: metres (m) / millimetres (mm), newtons (N), seconds (s), dimensionless mu.
//! Synthetic software/physics verification only — never a measured textile or garment fit claim.

use std::error::Error;

use ftx_core::{Vec2, Vec3, receipt::sha256_hex};
use ftx_drape::{Body, ClothModel, ContactParams, Membrane, Solver, grid};
use ftx_pattern::material_friction::{Input, calibrate};
use ftx_pattern::textile_import::catalog;
use serde_json::{Value, json};

const SLED_SIDE_M: f64 = 0.10; // 100 mm x 100 mm swatch footprint
const SLED_MASS_KG: f64 = 0.20; // 200 g standard sled mass (explicitly declared)
const RHO_EFF: f64 = SLED_MASS_KG / (SLED_SIDE_M * SLED_SIDE_M); // 20.0 kg/m^2 effective sled+swatch density
const DT_S: f64 = 0.01;
const STEPS: usize = 44;

struct HistorySpec {
    id: &'static str,
    state_id: &'static str,
    label: &'static str,
    description: &'static str,
    normal_n: f64,
    dir: [f64; 2], // unit pull direction in table (x, y) plane
    dir_label: &'static str,
    pull_max_n: f64,
    mu: f64,
}

fn contact_params(mu: f64) -> ContactParams {
    ContactParams {
        dhat: 2e-4,
        thickness: 5e-4,
        kappa: 20.0,
        mu,
        eps_v: 1e-3, // 1.0 mm/s Coulomb stick regularization band
    }
}

fn flat_counterface() -> Result<Body, Box<dyn Error>> {
    // 2.0 m x 2.0 m flat rigid counterface plate at z = 0 (outward normal +z).
    let s = 1.0;
    let verts = vec![
        Vec3::new(-s, -s, 0.0),
        Vec3::new(s, -s, 0.0),
        Vec3::new(s, s, 0.0),
        Vec3::new(-s, s, 0.0),
    ];
    let tris = vec![[0, 1, 2], [0, 2, 3]];
    Ok(Body::new(verts, tris, 0.05)?)
}

fn scaled_sliding_input(base: &Input, target_mu: f64) -> Input {
    let mut out = base.clone();
    if (target_mu - 0.4).abs() > 1e-12 {
        let ratio = target_mu / 0.4;
        out.method = format!(
            "Synthetic corrected steady-pull vector oracle scaled to coefficient {target_mu:.2}; no apparatus drag or inertia"
        );
        for exp in &mut out.experiments {
            for sample in &mut exp.samples {
                sample.pull_n = sample
                    .pull_n
                    .map(|opt| opt.map(|v| (v * ratio * 1e6).round() / 1e6));
            }
        }
    }
    out
}

fn run_sled_history(spec: &HistorySpec) -> Result<Value, Box<dyn Error>> {
    let dlen = (spec.dir[0] * spec.dir[0] + spec.dir[1] * spec.dir[1]).sqrt();
    let dir = [spec.dir[0] / dlen, spec.dir[1] / dlen];

    // Build 100 mm x 100 mm swatch centered at origin, using friction-sheet-a elastic moduli.
    let (rest, tris) = grid(
        SLED_SIDE_M,
        SLED_SIDE_M,
        2,
        2,
        Vec2::new(-SLED_SIDE_M / 2.0, -SLED_SIDE_M / 2.0),
    );
    let tri_rest: Vec<[Vec2; 3]> = tris
        .iter()
        .map(|&[a, b, c]| [rest[a], rest[b], rest[c]])
        .collect();
    let model = ClothModel::from_triangles(
        rest.len(),
        &tris,
        &tri_rest,
        Membrane::orthotropic(100.0, 40.0, 0.2, 10.0, 0.05),
        3e-5,
        RHO_EFF,
    )?;
    let total_mass: f64 = model.mass.iter().sum();
    let p_zero = contact_params(0.0);
    let z_start = p_zero.thickness + p_zero.dhat * 0.65;
    let x0: Vec<Vec3> = rest.iter().map(|p| Vec3::new(p.x, p.y, z_start)).collect();
    let body = flat_counterface()?;

    // 1. Settle vertically onto the counterface with mu = 0 under the declared normal load
    //    so normal contact equilibrium is established before any tangential pull begins.
    let fixed_xy = vec![[true, true, false]; rest.len()];
    let g_normal = Vec3::new(0.0, 0.0, -spec.normal_n / total_mass);
    let mut settle =
        Solver::new(model.clone(), x0, &fixed_xy, g_normal).with_contact(body.clone(), p_zero)?;
    settle.tol = 1e-10;
    settle.max_newton = 200;
    let settle_rep = settle.static_solve()?;
    if !settle_rep.converged {
        return Err(format!("vertical settle did not converge for {}", spec.id).into());
    }
    let settled_x = settle.x.clone();

    // 2. Release x,y DOFs with the calibrated Coulomb coefficient mu and ramp tangential force.
    let free = vec![[false, false, false]; rest.len()];
    let params = contact_params(spec.mu);
    let mut solver =
        Solver::new(model, settled_x.clone(), &free, g_normal).with_contact(body, params)?;
    solver.tol = 1e-9;
    solver.max_newton = 300;
    solver.friction_rounds = 4;

    let cm_pos = |xs: &[Vec3], ms: &[f64]| -> Vec3 {
        let sum = xs
            .iter()
            .zip(ms)
            .fold(Vec3::ZERO, |acc, (&x, &m)| acc + x * m);
        sum * (1.0 / total_mass)
    };
    let cm_vel = |vs: &[Vec3], ms: &[f64]| -> Vec3 {
        let sum = vs
            .iter()
            .zip(ms)
            .fold(Vec3::ZERO, |acc, (&v, &m)| acc + v * m);
        sum * (1.0 / total_mass)
    };

    let start_cm = cm_pos(&solver.x, &solver.model.mass);
    let threshold_n = spec.mu * spec.normal_n;

    let mut samples = Vec::with_capacity(STEPS + 1);
    let mut first_slip_step: Option<usize> = None;
    let mut first_slip_pull_n: Option<f64> = None;
    let mut first_slip_time_s: Option<f64> = None;

    // Sample 0: settled rest state before tangential ramp.
    {
        let contact = solver.contact.as_ref().unwrap();
        let rmap = contact.reaction_map(&solver.x)?;
        let forces: Vec<Vec3> = serde_json::from_value(rmap["body_force_on_cloth_n"].clone())?;
        let fn_z: f64 = forces.iter().map(|f| f.z).sum();
        let min_gap_m = contact
            .min_gap(&solver.x, 0.01)
            .ok_or("no active contact gap")?;
        samples.push(json!({
            "step": 0,
            "time_s": 0.0,
            "applied_normal_n": spec.normal_n,
            "retained_normal_n": (fn_z * 1e6).round() / 1e6,
            "applied_pull_n": [0.0, 0.0],
            "applied_pull_along_n": 0.0,
            "tangential_reaction_n": [0.0, 0.0],
            "tangential_reaction_along_n": 0.0,
            "sled_center_mm": [0.0, 0.0, ((start_cm.z) * 1e6).round() / 1e3],
            "displacement_mm": 0.0,
            "velocity_mm_s": [0.0, 0.0],
            "speed_mm_s": 0.0,
            "min_gap_mm": (min_gap_m * 1e6).round() / 1e3,
            "regime": "stick",
            "converged": true,
            "iterations": 0
        }));
    }

    let mut prev_v_cm = cm_vel(&solver.v, &solver.model.mass);
    for k in 1..=STEPS {
        let frac = k as f64 / STEPS as f64;
        let pull_mag = spec.pull_max_n * frac;
        let pull_x = pull_mag * dir[0];
        let pull_y = pull_mag * dir[1];
        solver.gravity = Vec3::new(
            pull_x / total_mass,
            pull_y / total_mass,
            -spec.normal_n / total_mass,
        );

        let rep = solver.step(DT_S)?;
        if !rep.converged {
            return Err(format!("step {k} did not converge for {}", spec.id).into());
        }

        let contact = solver.contact.as_ref().unwrap();
        let rmap = contact.reaction_map(&solver.x)?;
        let forces: Vec<Vec3> = serde_json::from_value(rmap["body_force_on_cloth_n"].clone())?;
        let fn_z: f64 = forces.iter().map(|f| f.z).sum();
        let min_gap_m = contact
            .min_gap(&solver.x, 0.01)
            .ok_or("no active contact gap")?;
        if min_gap_m <= 0.0 {
            return Err(format!("penetrating contact gap at step {k} for {}", spec.id).into());
        }

        let pos_cm = cm_pos(&solver.x, &solver.model.mass);
        let vel_cm = cm_vel(&solver.v, &solver.model.mass);
        let acc_cm = (vel_cm - prev_v_cm) * (1.0 / DT_S);
        prev_v_cm = vel_cm;

        // Exact dynamic tangential balance: m * a_xy = F_pull_xy + F_friction_on_cloth_xy
        // Resisting tangential reaction opposing pull = -F_friction_on_cloth_xy = F_pull_xy - m * a_xy.
        let resist_x = pull_x - total_mass * acc_cm.x;
        let resist_y = pull_y - total_mass * acc_cm.y;
        let resist_along = resist_x * dir[0] + resist_y * dir[1];

        let disp_x_mm = (pos_cm.x - start_cm.x) * 1000.0;
        let disp_y_mm = (pos_cm.y - start_cm.y) * 1000.0;
        let disp_along_mm = disp_x_mm * dir[0] + disp_y_mm * dir[1];
        let vx_mm_s = vel_cm.x * 1000.0;
        let vy_mm_s = vel_cm.y * 1000.0;
        let speed_mm_s = (vx_mm_s * vx_mm_s + vy_mm_s * vy_mm_s).sqrt();
        let slipping = speed_mm_s > params.eps_v * 1000.0;
        if slipping && first_slip_step.is_none() {
            first_slip_step = Some(k);
            first_slip_pull_n = Some(pull_mag);
            first_slip_time_s = Some(k as f64 * DT_S);
        }

        samples.push(json!({
            "step": k,
            "time_s": ((k as f64 * DT_S) * 1e6).round() / 1e6,
            "applied_normal_n": spec.normal_n,
            "retained_normal_n": (fn_z * 1e6).round() / 1e6,
            "applied_pull_n": [(pull_x * 1e6).round() / 1e6, (pull_y * 1e6).round() / 1e6],
            "applied_pull_along_n": (pull_mag * 1e6).round() / 1e6,
            "tangential_reaction_n": [(resist_x * 1e6).round() / 1e6, (resist_y * 1e6).round() / 1e6],
            "tangential_reaction_along_n": (resist_along * 1e6).round() / 1e6,
            "sled_center_mm": [
                (disp_x_mm * 1e4).round() / 1e4,
                (disp_y_mm * 1e4).round() / 1e4,
                (pos_cm.z * 1e6).round() / 1e3
            ],
            "displacement_mm": (disp_along_mm * 1e4).round() / 1e4,
            "velocity_mm_s": [(vx_mm_s * 1e4).round() / 1e4, (vy_mm_s * 1e4).round() / 1e4],
            "speed_mm_s": (speed_mm_s * 1e4).round() / 1e4,
            "min_gap_mm": (min_gap_m * 1e6).round() / 1e3,
            "regime": if slipping { "slip" } else { "stick" },
            "converged": rep.converged,
            "iterations": rep.iterations
        }));
    }

    let payload = json!({
        "id": spec.id,
        "state_id": spec.state_id,
        "label": spec.label,
        "description": spec.description,
        "units": {
            "force": "N",
            "displacement": "mm",
            "velocity": "mm/s",
            "time": "s",
            "mu": "1"
        },
        "normal_load_n": spec.normal_n,
        "pull_direction": dir,
        "pull_direction_label": spec.dir_label,
        "pull_max_n": spec.pull_max_n,
        "mu": spec.mu,
        "coulomb_threshold_n": (threshold_n * 1e6).round() / 1e6,
        "eps_v_mm_s": params.eps_v * 1000.0,
        "first_slip_step": first_slip_step,
        "first_slip_time_s": first_slip_time_s,
        "first_slip_pull_n": first_slip_pull_n.map(|v| (v * 1e6).round() / 1e6),
        "samples": samples
    });
    Ok(payload)
}

fn main() -> Result<(), Box<dyn Error>> {
    let base_input: Input = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-friction-sliding.json"
    ))?;
    let db = catalog()?;

    // Calibrate two declared synthetic surface states through the exact native friction calibrator.
    let input_a = scaled_sliding_input(&base_input, 0.40);
    let report_a = calibrate(&input_a, &db)?;
    let input_b = scaled_sliding_input(&base_input, 0.75);
    let report_b = calibrate(&input_b, &db)?;

    // Audit real catalog pairs to confirm missing source-qualified friction stays unavailable.
    let catalog_total = db.materials.len();
    let catalog_with_friction = db
        .materials
        .iter()
        .filter(|m| {
            m.properties.contains_key("friction_self") || m.properties.contains_key("friction_skin")
        })
        .count();
    let unavailable_examples: Vec<Value> = [
        (
            "denim1_vs_cotton_jersey",
            "Kanat & Ozdil Denim 1 vs De Mey Cotton Jersey",
            "kanat_ozdil_2026_denim_1_unwashed",
            "demey/2025/cotton-jersey",
        ),
        (
            "cotton_muslin_vs_wool_blend",
            "Dharma Cotton Muslin 3.5 oz vs De Mey Wool/Linen/Cotton",
            "dharma_cotton_muslin_35",
            "demey/2025/wool70-linen20-cotton10-woven",
        ),
    ]
    .into_iter()
    .map(|(pair_id, label, id_a, id_b)| {
        let missing = |id: &str| {
            db.get(id)
                .map(|m| {
                    let mut miss = Vec::new();
                    if !m.properties.contains_key("friction_self") {
                        miss.push("friction_self");
                    }
                    if !m.properties.contains_key("friction_skin") {
                        miss.push("friction_skin");
                    }
                    json!({"id": m.id, "name": m.name, "missing": miss})
                })
                .unwrap_or_else(|_| json!({"id": id, "missing": ["friction_self", "friction_skin"]}))
        };
        json!({
            "id": pair_id,
            "label": label,
            "available": false,
            "reason": "Missing source-qualified friction coefficient for both catalog counterfaces; no pair interface or generic preset is invented.",
            "sheets": [missing(id_a), missing(id_b)]
        })
    })
    .collect();

    let histories_spec = [
        HistorySpec {
            id: "state_a_2n_pos_x",
            state_id: "synthetic_pair_040",
            label: "2.0 N normal · +x pull ramp (0 → 1.40 N)",
            description: "Synthetic pair A/B (μ = 0.40) under 2.0 N normal load, pulled along +x past the 0.80 N Coulomb threshold.",
            normal_n: 2.0,
            dir: [1.0, 0.0],
            dir_label: "+x (1.00, 0.00)",
            pull_max_n: 1.40,
            mu: report_a.coefficient.value,
        },
        HistorySpec {
            id: "state_a_5n_diag",
            state_id: "synthetic_pair_040",
            label: "5.0 N normal · diagonal (-0.6, +0.8) ramp (0 → 3.40 N)",
            description: "Synthetic pair A/B (μ = 0.40) under 5.0 N normal load, pulled along the fitting acquisition's (-0.6, +0.8) vector past 2.00 N.",
            normal_n: 5.0,
            dir: [-0.6, 0.8],
            dir_label: "(-0.60, +0.80)",
            pull_max_n: 3.40,
            mu: report_a.coefficient.value,
        },
        HistorySpec {
            id: "state_b_2n_pos_x",
            state_id: "synthetic_pair_075",
            label: "2.0 N normal · +x pull ramp (0 → 2.40 N)",
            description: "Higher-grip synthetic pair (μ = 0.75) under the same 2.0 N normal load, holding in stick until 1.50 N before sliding along +x.",
            normal_n: 2.0,
            dir: [1.0, 0.0],
            dir_label: "+x (1.00, 0.00)",
            pull_max_n: 2.40,
            mu: report_b.coefficient.value,
        },
        HistorySpec {
            id: "state_b_3n_neg_y",
            state_id: "synthetic_pair_075",
            label: "3.0 N normal · -y pull ramp (0 → 3.40 N)",
            description: "Higher-grip synthetic pair (μ = 0.75) under 3.0 N normal load, pulled along the held-out check's -y direction past 2.25 N.",
            normal_n: 3.0,
            dir: [0.0, -1.0],
            dir_label: "-y (0.00, -1.00)",
            pull_max_n: 3.40,
            mu: report_b.coefficient.value,
        },
    ];

    let mut histories = Vec::new();
    for spec in &histories_spec {
        histories.push(run_sled_history(spec)?);
    }

    let mut doc = json!({
        "schema": "ftx-friction-sled-exhibit/1",
        "scope": "Synthetic software/physics sliding-sled runs computed by production ftx-drape (IPC barrier + smoothed Coulomb friction) and ftx-pattern material_friction::calibrate. Not measured textile qualification, static breakaway calibration, or garment fit.",
        "sled": {
            "footprint_mm": [SLED_SIDE_M * 1000.0, SLED_SIDE_M * 1000.0],
            "mass_kg": SLED_MASS_KG,
            "mesh_cells": [2, 2],
            "time_step_s": DT_S,
            "steps": STEPS,
            "eps_v_mm_s": 1.0,
            "barrier_dhat_mm": 0.2,
            "contact_thickness_mm": 0.5
        },
        "catalog_audit": {
            "total_materials": catalog_total,
            "materials_with_source_qualified_friction": catalog_with_friction,
            "unavailable_pairs": unavailable_examples
        },
        "states": [
            {
                "id": "synthetic_pair_040",
                "label": "Synthetic pair A/B (μ = 0.40 · bundled oracle)",
                "evidence": "estimate",
                "synthetic": true,
                "mu": report_a.coefficient.value,
                "calibration_input_sha256": report_a.input_sha256,
                "fit_normal_n_range": report_a.fit_normal_n_range,
                "fit_speed_m_s_range": report_a.fit_speed_m_s_range,
                "fit_rms_scaled": report_a.fit.rms_scaled,
                "check_rms_scaled": report_a.check.as_ref().map(|c| c.rms_scaled),
                "calibration_input": input_a
            },
            {
                "id": "synthetic_pair_075",
                "label": "Higher-grip synthetic pair (μ = 0.75 · scaled oracle)",
                "evidence": "estimate",
                "synthetic": true,
                "mu": report_b.coefficient.value,
                "calibration_input_sha256": report_b.input_sha256,
                "fit_normal_n_range": report_b.fit_normal_n_range,
                "fit_speed_m_s_range": report_b.fit_speed_m_s_range,
                "fit_rms_scaled": report_b.fit.rms_scaled,
                "check_rms_scaled": report_b.check.as_ref().map(|c| c.rms_scaled),
                "calibration_input": input_b
            }
        ],
        "histories": histories
    });

    let digest = sha256_hex(&serde_json::to_vec(&doc)?);
    doc["report_sha256"] = json!(digest);
    println!("{}", serde_json::to_string_pretty(&doc)?);
    Ok(())
}
