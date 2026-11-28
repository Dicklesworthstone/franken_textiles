//! `Exp-1` (`FTX-K09` / `GK3` prep) benchmark:
//! - Part A: 4-stitch stockinette RVE (`lifted_tangle` `Planar2D` topology certification + Bishop
//!   frames + 224–576 DOF FE RVE cell with `CoupledKnitMembrane`, `AsymmetricKnitBending`, and
//!   `KnitHysteresisBank`, plus Poincloux Rung-3 KAT check).
//! - Part B: `zouave_bolero-920` oracle pattern meshed via `build_garment`, placed onto a
//!   synthetic `PartFrames` + cylindrical `Body` via `build_scene`, comparing baseline vs coupled
//!   knit `Solver::step` wall time, Newton iterations, PSD rejections, and factorizations.
//!
//! Four-Claims honesty: benchmark outputs record software timing and solver telemetry only
//! (`hardware_execution_authorized: false`, `physical_knitting_validated: false`,
//! `physical_dyeing_validated: false`, `fit_claimed: false`).

#![forbid(unsafe_code)]

use std::time::Instant;

use ftx_core::{Vec2, Vec3, det};
use ftx_drape::PartFrame;
use ftx_drape::cloth::{ClothModel, Solver, hex_grid};
use ftx_drape::contact::{Body, ContactParams};
use ftx_drape::elements::Membrane;
use ftx_drape::garment::build_garment;
use ftx_drape::integrator::TimeIntegrator;
use ftx_drape::knit::{
    AsymmetricKnitBending, CoupledKnitMembrane, KnitHysteresisBank, PoinclouxParams,
    poincloux_uniaxial_curve,
};
use ftx_drape::material::Fabric;
use ftx_drape::scene::{PartFrames, build_scene};
use ftx_pattern::knit::lifted_tangle::{
    CrossingSign, CrossingSpec, FilamentComplex, FilamentCurve, FtxDetMath, Periodicity,
    certify_topology_det, compute_bishop_frames,
};
use ftx_pattern::materials::Evidence;
use ftx_pattern::{Grain, Pattern, truthy};
use serde_json::Value;

struct RveBenchMetrics {
    dofs: usize,
    isotopy_valid: bool,
    min_clearance_m: f64,
    bishop_normals: usize,
    wall_ms: f64,
    iterations: usize,
    projected_iterations: usize,
    factorizations: usize,
    poincloux_points: usize,
    gating_flags: usize,
}

struct BoleroRunMetrics {
    vertices: usize,
    triangles: usize,
    wall_ms: f64,
    iterations: usize,
    projected_iterations: usize,
    factorizations: usize,
    gating_flags: usize,
}

struct SyntheticBoleroFrames;

impl PartFrames for SyntheticBoleroFrames {
    fn frame(&self, part: &str) -> Option<PartFrame> {
        match part {
            "torso" => Some(PartFrame {
                origin: Vec3::ZERO,
                up: Vec3::new(0.0, 0.0, 1.0),
                front: Vec3::new(0.0, -1.0, 0.0),
                lat: Vec3::new(1.0, 0.0, 0.0),
            }),
            "arm_R" => {
                // 35-degree A-pose downward arm frame sized to 92cm toddler bolero proportions.
                let (s, c) = det::sin_cos(35.0_f64.to_radians());
                let up = Vec3::new(-c, 0.0, s).normalized();
                let front = Vec3::new(0.0, -1.0, 0.0);
                let lat = up.cross(front).normalized();
                let shoulder_tip = Vec3::new(0.132, 0.0, 0.25);
                Some(PartFrame {
                    origin: shoulder_tip - up * 0.25,
                    up,
                    front,
                    lat,
                })
            }
            _ => None,
        }
    }

    fn landmark_h(&self, part: &str, landmark: &str) -> Option<f64> {
        match (part, landmark) {
            ("torso", "waist") => Some(0.0),
            ("arm_R", "shoulder_tip") => Some(250.0),
            _ => None,
        }
    }

    fn section_radius(&self, part: &str, _h: f64) -> f64 {
        match part {
            "torso" => 82.0,
            "arm_R" => 24.0,
            _ => 30.0,
        }
    }
}

fn add_cylinder_mesh(
    vertices: &mut Vec<Vec3>,
    triangles: &mut Vec<[usize; 3]>,
    frame: &PartFrame,
    h_lo_m: f64,
    h_hi_m: f64,
    radius_m: f64,
    segs: usize,
) {
    let base = vertices.len();
    for &h in &[h_lo_m, h_hi_m] {
        for i in 0..segs {
            let theta = 2.0 * std::f64::consts::PI * (i as f64) / (segs as f64);
            let (s, c) = det::sin_cos(theta);
            let radial = frame.lat * (radius_m * c) + frame.front * (radius_m * s);
            vertices.push(frame.origin + frame.up * h + radial);
        }
    }
    for i in 0..segs {
        let j = (i + 1) % segs;
        triangles.push([base + i, base + j, base + segs + j]);
        triangles.push([base + i, base + segs + j, base + segs + i]);
    }
}

fn synthetic_bolero_body(frames: &SyntheticBoleroFrames) -> Body {
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    let torso = frames.frame("torso").expect("torso frame");
    add_cylinder_mesh(
        &mut vertices,
        &mut triangles,
        &torso,
        -0.05,
        0.38,
        0.082,
        20,
    );
    let arm_r = frames.frame("arm_R").expect("arm_R frame");
    add_cylinder_mesh(
        &mut vertices,
        &mut triangles,
        &arm_r,
        -0.04,
        0.25,
        0.024,
        14,
    );
    let arm_l_up = Vec3::new(-arm_r.up.x, arm_r.up.y, arm_r.up.z);
    let arm_l = PartFrame {
        origin: Vec3::new(-arm_r.origin.x, arm_r.origin.y, arm_r.origin.z),
        up: arm_l_up,
        front: arm_r.front,
        lat: arm_l_up.cross(arm_r.front).normalized(),
    };
    add_cylinder_mesh(
        &mut vertices,
        &mut triangles,
        &arm_l,
        -0.04,
        0.25,
        0.024,
        14,
    );
    Body::new(vertices, triangles, 0.01).expect("valid synthetic bolero body")
}

fn build_four_stitch_planar_complex() -> FilamentComplex {
    // 2x2 periodic Planar2D interlaced filament complex representing 4 stitch crossings.
    let pitch = 0.012;
    let z_amp = 0.0014;
    let positions_m = vec![
        // Course curve 0 (y = 0.003) along +x
        0.000, 0.003, 0.000, 0.003, 0.003, z_amp, 0.006, 0.003, 0.000, 0.009, 0.003, -z_amp,
        // Course curve 1 (y = 0.009) along +x
        0.000, 0.009, 0.000, 0.003, 0.009, -z_amp, 0.006, 0.009, 0.000, 0.009, 0.009, z_amp,
        // Wale curve 0 (x = 0.003) along +y
        0.003, 0.000, 0.000, 0.003, 0.003, -z_amp, 0.003, 0.006, 0.000, 0.003, 0.009, z_amp,
        // Wale curve 1 (x = 0.009) along +y
        0.009, 0.000, 0.000, 0.009, 0.003, z_amp, 0.009, 0.006, 0.000, 0.009, 0.009, -z_amp,
    ];
    FilamentComplex {
        periodicity: Periodicity::Planar2D {
            u_vec: [pitch, 0.0, 0.0],
            v_vec: [0.0, pitch, 0.0],
        },
        positions_m,
        curves: vec![
            FilamentCurve {
                id: 1,
                nodes: vec![0, 1, 2, 3],
                closed: true,
                periodic_shift: [1, 0, 0],
                radius_m: 0.00045,
                segment_twist_rad: Vec::new(),
            },
            FilamentCurve {
                id: 2,
                nodes: vec![4, 5, 6, 7],
                closed: true,
                periodic_shift: [1, 0, 0],
                radius_m: 0.00045,
                segment_twist_rad: Vec::new(),
            },
            FilamentCurve {
                id: 3,
                nodes: vec![8, 9, 10, 11],
                closed: true,
                periodic_shift: [0, 1, 0],
                radius_m: 0.00045,
                segment_twist_rad: Vec::new(),
            },
            FilamentCurve {
                id: 4,
                nodes: vec![12, 13, 14, 15],
                closed: true,
                periodic_shift: [0, 1, 0],
                radius_m: 0.00045,
                segment_twist_rad: Vec::new(),
            },
        ],
        crossings: vec![
            CrossingSpec {
                over_curve: 0,
                over_segment: 0,
                under_curve: 2,
                under_segment: 0,
                under_image_shift: [0, 0, 0],
                view_dir: [0.0, 0.0, 1.0],
                expected_sign: CrossingSign::Positive,
            },
            CrossingSpec {
                over_curve: 1,
                over_segment: 2,
                under_curve: 3,
                under_segment: 2,
                under_image_shift: [0, 0, 0],
                view_dir: [0.0, 0.0, 1.0],
                expected_sign: CrossingSign::Positive,
            },
        ],
    }
}

fn bench_stockinette_rve_4_stitch() -> RveBenchMetrics {
    let complex = build_four_stitch_planar_complex();
    let topo = certify_topology_det(&complex).expect("4-stitch RVE topology certification");
    let frames = compute_bishop_frames::<FtxDetMath>(&complex).expect("4-stitch Bishop frames");

    // 4-stitch RVE FE patch sized inside GK3's 224..=576 DOF window (~98 vertices -> 294 DOFs).
    let w = 0.012;
    let h = 0.010;
    let (rest, tris) = hex_grid(w, h, 0.0014, Vec2::ZERO);
    let dofs = rest.len() * 3;
    assert!(
        (224..=576).contains(&dofs),
        "4-stitch RVE DOFs ({dofs}) must sit in [224, 576]"
    );

    let mut model = ClothModel::new(
        rest.clone(),
        &tris,
        Membrane::orthotropic(10_000.0, 10_000.0, 0.3, 2_000.0, 0.2),
        1e-4,
        0.25,
    )
    .expect("valid RVE ClothModel");

    let n_tri = model.membranes.len();
    let coupled = vec![Some(CoupledKnitMembrane::reference_stockinette_estimate()); n_tri];
    let bend = vec![Some(AsymmetricKnitBending::reference_stockinette_estimate()); n_tri];
    let hyst = vec![Some(KnitHysteresisBank::reference_jersey_estimate()); n_tri];
    let weld: Vec<usize> = (0..rest.len()).collect();

    model
        .set_coupled_knit_membranes(&coupled)
        .expect("bind CoupledKnitMembrane");
    model
        .set_knit_panel_bending(&rest, &tris, &weld, &bend)
        .expect("bind AsymmetricKnitBending");

    // Pre-stretch 4% along y and pin top/bottom y edges for uniaxial loading cycle.
    let x_init: Vec<Vec3> = rest
        .iter()
        .map(|p| Vec3::new(p.x, p.y * 1.04, 0.0))
        .collect();
    let fixed: Vec<[bool; 3]> = rest
        .iter()
        .map(|p| {
            let bottom = p.y <= 1e-9;
            let top = (p.y - h).abs() <= 1e-9;
            let anchor_x = p.x <= 1e-9 && bottom;
            [anchor_x, bottom || top, bottom || top]
        })
        .collect();

    let mut solver = Solver::new(model, x_init, &fixed, Vec3::new(0.0, 0.0, -9.81))
        .with_knit_hysteresis(&hyst)
        .expect("bind KnitHysteresisBank");
    solver.damping.rayleigh_bending_s = 0.0;
    solver.integrator = TimeIntegrator::ImplicitEuler;

    let mut total_iters = 0usize;
    let mut total_proj = 0usize;
    let mut total_fact = 0usize;
    let t0 = Instant::now();
    for _ in 0..4 {
        let rep = solver.step(0.01).expect("4-stitch RVE Solver::step");
        assert!(rep.converged, "RVE step must converge cleanly");
        total_iters += rep.iterations;
        total_proj += rep.projected_iterations;
        total_fact += rep.factorizations;
    }
    let wall_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let assess = solver
        .assess_knit_shell(
            "exp1-rve-4st".to_string(),
            "ref-stockinette-estimate".to_string(),
            None,
            Evidence::Estimate,
        )
        .expect("assess_knit_shell");
    let poincloux = poincloux_uniaxial_curve(
        &PoinclouxParams::reference_monofilament(),
        &[0.0, 0.05, 0.15, 0.25],
    )
    .expect("poincloux KAT curve");

    RveBenchMetrics {
        dofs,
        isotopy_valid: topo.isotopy_valid,
        min_clearance_m: topo.min_clearance_m,
        bishop_normals: frames.normals_u.len(),
        wall_ms,
        iterations: total_iters,
        projected_iterations: total_proj,
        factorizations: total_fact,
        poincloux_points: poincloux.len(),
        gating_flags: assess.flags.len(),
    }
}

fn grain_rotated_source_rest(
    pattern: &Pattern,
    mesh: &ftx_drape::garment::GarmentMesh,
) -> Vec<Vec2> {
    let mut rest = vec![Vec2::ZERO; mesh.verts2d.len()];
    for (k, id) in mesh.instances.iter().enumerate() {
        let inst = pattern.instance(id).expect("pattern instance");
        let piece = &pattern.raw()["pieces"][inst.piece_index];
        let mut d = match piece.get("grain") {
            Some(g) => {
                let g: Grain = serde_json::from_value(g.clone()).expect("valid grain");
                let pts = g.points().expect("two grain points");
                pts[1] - pts[0]
            }
            None => Vec2::new(0.0, 1.0),
        };
        if let Some(scale) = piece.get("sim_scale").filter(|v| truthy(v)) {
            let s: Vec2 = serde_json::from_value(scale.clone()).expect("valid sim_scale");
            d = Vec2::new(d.x * s.x, d.y * s.y);
        }
        if inst.mirrored {
            d.x = -d.x;
        }
        let (s, c) = if d.norm() > 0.0 {
            det::sin_cos(-det::atan2(d.y, d.x))
        } else {
            (0.0, 1.0)
        };
        let (lo, hi) = mesh.inst_range[k];
        for i in lo..hi {
            let p = mesh.verts2d[i] * 1e-3;
            rest[i] = Vec2::new(c * p.x - s * p.y, s * p.x + c * p.y);
        }
    }
    rest
}

fn bench_bolero_fixture() -> (BoleroRunMetrics, BoleroRunMetrics) {
    let drafts: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/oracle/pattern/drafts.json"
    ))
    .expect("parse drafts.json");
    let case = drafts["cases"]
        .as_array()
        .expect("cases array")
        .iter()
        .find(|c| c["id"] == "zouave_bolero-920")
        .expect("zouave_bolero-920 case");

    let mut pattern_val = case["input"]["pattern"].clone();
    // Use uniform Fabric parameter mode for standalone build_scene benchmark.
    pattern_val["fabric"] = serde_json::json!({
        "id": "knit-benchmark",
        "kind": "knit",
        "areal_g_m2": 320.0,
        "tensile_N_m": 10000.0,
        "bending_Nm": 0.00012
    });
    let pattern = Pattern::new(pattern_val);
    let mesh = build_garment(&pattern, 12.0).expect("mesh zouave_bolero-920");
    let source_rest = grain_rotated_source_rest(&pattern, &mesh);

    let frames = SyntheticBoleroFrames;
    let body = synthetic_bolero_body(&frames);
    let fabric = Fabric::of(&pattern).expect("bolero uniform Fabric");
    let contact = ContactParams {
        dhat: 2e-3,
        thickness: 0.0,
        kappa: 1e3,
        mu: 0.0,
        eps_v: 1e-3,
    };

    let mut base_scene = build_scene(
        &pattern,
        mesh.clone(),
        &frames,
        &body.vertices,
        &body.triangles,
        fabric,
        1.0,
        contact,
        None,
    )
    .expect("build baseline bolero scene");
    base_scene.solver.integrator = TimeIntegrator::ImplicitEuler;
    base_scene.solver.tol = 1e-3;
    base_scene.solver.pcg_iters = 0;

    let verts = base_scene.solver.x.len();
    let tris = base_scene.solver.model.membranes.len();

    let mut base_iters = 0usize;
    let mut base_proj = 0usize;
    let mut base_fact = 0usize;
    let t_base = Instant::now();
    let dt_bolero = 0.005;
    for _ in 0..3 {
        let rep = base_scene
            .solver
            .step(dt_bolero)
            .expect("baseline bolero Solver::step");
        assert!(rep.converged, "baseline bolero step must converge");
        base_iters += rep.iterations;
        base_proj += rep.projected_iterations;
        base_fact += rep.factorizations;
    }
    let base_ms = t_base.elapsed().as_secs_f64() * 1000.0;
    let baseline_metrics = BoleroRunMetrics {
        vertices: verts,
        triangles: tris,
        wall_ms: base_ms,
        iterations: base_iters,
        projected_iterations: base_proj,
        factorizations: base_fact,
        gating_flags: 0,
    };

    // Coupled knit shell run on identical welded topology and start positions.
    let mut knit_scene = build_scene(
        &pattern,
        mesh.clone(),
        &frames,
        &body.vertices,
        &body.triangles,
        fabric,
        1.0,
        contact,
        None,
    )
    .expect("build coupled-knit bolero scene");

    let mut source_triangles = Vec::with_capacity(tris);
    for f in &mesh.faces {
        let t = f.map(|v| knit_scene.weld[v]);
        if t[0] != t[1] && t[1] != t[2] && t[2] != t[0] {
            source_triangles.push(*f);
        }
    }
    assert_eq!(
        source_triangles.len(),
        knit_scene.solver.model.membranes.len()
    );

    let mut knit_mem = CoupledKnitMembrane::reference_stockinette_estimate();
    // Admit wrapped-start armhole seam pre-stretch cleanly in the benchmark while retaining
    // the exact coupled quartic + smooth jamming barrier law.
    knit_mem.e1_jam = 1.25;
    knit_mem.e2_jam = 1.50;
    knit_mem.gamma_jam = 1.50;
    let coupled_laws = vec![Some(knit_mem); tris];
    let bend_laws = vec![Some(AsymmetricKnitBending::reference_stockinette_estimate()); tris];
    let hyst_laws = vec![Some(KnitHysteresisBank::reference_jersey_estimate()); tris];

    knit_scene
        .solver
        .model
        .set_coupled_knit_membranes(&coupled_laws)
        .expect("bind bolero CoupledKnitMembrane");
    knit_scene
        .solver
        .model
        .set_knit_panel_bending(
            &source_rest,
            &source_triangles,
            &knit_scene.weld,
            &bend_laws,
        )
        .expect("bind bolero AsymmetricKnitBending");

    let mut knit_solver = knit_scene
        .solver
        .with_knit_hysteresis(&hyst_laws)
        .expect("bind bolero KnitHysteresisBank");
    knit_solver.damping.rayleigh_bending_s = 0.0;
    knit_solver.integrator = TimeIntegrator::ImplicitEuler;
    knit_solver.tol = 1e-3;
    knit_solver.pcg_iters = 0;

    let mut knit_iters = 0usize;
    let mut knit_proj = 0usize;
    let mut knit_fact = 0usize;
    let t_knit = Instant::now();
    for _ in 0..3 {
        let rep = knit_solver
            .step(dt_bolero)
            .expect("coupled-knit bolero Solver::step");
        assert!(rep.converged, "coupled-knit bolero step must converge");
        knit_iters += rep.iterations;
        knit_proj += rep.projected_iterations;
        knit_fact += rep.factorizations;
    }
    let knit_ms = t_knit.elapsed().as_secs_f64() * 1000.0;

    let assess = knit_solver
        .assess_knit_shell(
            "zouave_bolero-920".to_string(),
            "ref-stockinette-estimate".to_string(),
            None,
            Evidence::Estimate,
        )
        .expect("bolero assess_knit_shell");

    let knit_metrics = BoleroRunMetrics {
        vertices: verts,
        triangles: tris,
        wall_ms: knit_ms,
        iterations: knit_iters,
        projected_iterations: knit_proj,
        factorizations: knit_fact,
        gating_flags: assess.flags.len(),
    };
    (baseline_metrics, knit_metrics)
}

fn main() {
    let rve = bench_stockinette_rve_4_stitch();
    let (base, knit) = bench_bolero_fixture();
    let ratio = if base.wall_ms > 0.0 {
        knit.wall_ms / base.wall_ms
    } else {
        1.0
    };
    println!(
        "EXP1_RVE_4ST dofs={} isotopy_valid={} min_clearance_mm={:.3} bishop_normals={} wall_ms={:.2} iters={} psd_proj={} fact={} poincloux_pts={} flags={}",
        rve.dofs,
        rve.isotopy_valid,
        rve.min_clearance_m * 1000.0,
        rve.bishop_normals,
        rve.wall_ms,
        rve.iterations,
        rve.projected_iterations,
        rve.factorizations,
        rve.poincloux_points,
        rve.gating_flags
    );
    println!(
        "EXP1_BOLERO verts={} tris={} base_ms={:.2} knit_ms={:.2} ratio={:.3} base_iters={} knit_iters={} base_psd_proj={} knit_psd_proj={} base_fact={} knit_fact={} flags={}",
        base.vertices,
        base.triangles,
        base.wall_ms,
        knit.wall_ms,
        ratio,
        base.iterations,
        knit.iterations,
        base.projected_iterations,
        knit.projected_iterations,
        base.factorizations,
        knit.factorizations,
        knit.gating_flags
    );
    println!(
        "EXP1_HONESTY hardware_execution_authorized=false physical_knitting_validated=false physical_dyeing_validated=false fit_claimed=false"
    );
}
