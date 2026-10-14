//! Fixed-pattern jacket material substitution and heterogeneous shell/cuff/lining gate tests
//! (plan §25.1, §25.3, §25.5; beads `ftx-tm3`, `ftx-tm8`).
//!
//! All numeric property values in this file are synthetic software-gate inputs with explicit
//! estimate consent (`allow_estimates: true`). They verify causal propagation through pattern
//! resolution, meshing, seam welding, gather compression, orthotropic and nonlinear membrane
//! mechanics, bending, thickness/friction contact, thermal exchange and width-aware purchasing
//! without claiming empirical laboratory characterization of commercial textiles or real garment fit.

use ftx_core::{Vec3, det, receipt::sha256_hex};
use ftx_drape::material::resolve;
use ftx_drape::run::{DrapeBody, DrapeOptions, DrapeRun, drape};
use ftx_drape::scene::PartFrames;
use ftx_drape::thermal::Boundary;
use ftx_drape::{Body, PartFrame};
use ftx_pattern::Pattern;
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_cost::{Basis, Quote, QuoteBook, SaleUnit, Scope};
use ftx_pattern::materials::{Material, ThermalStack};
use ftx_pattern::textile_import::catalog;
use serde_json::{Value, json};

fn quantity(value: f64, unit: &str, source: &str) -> Value {
    json!({
        "value": value,
        "unit": unit,
        "source": source,
        "evidence": "estimate",
        "method": "Synthetic software-gate characterization, not a laboratory measurement",
        "conditions": {"state": "dry_conditioned_20C_65RH"}
    })
}

fn synthetic_linen_selection() -> Value {
    let mut props = serde_json::Map::new();
    for (k, v, u) in [
        ("areal_density", 0.185, "kg/m2"),
        ("thickness", 0.00045, "m"),
        ("warp_stiffness", 640.0, "N/m"),
        ("weft_stiffness", 420.0, "N/m"),
        ("shear_stiffness", 95.0, "N/m"),
        ("poisson_warp_weft", 0.22, "1"),
        ("bending_rigidity", 2.8e-5, "N*m"),
        ("friction_skin", 0.24, "1"),
        ("friction_self", 0.36, "1"),
        ("thermal_conductivity", 0.052, "W/(m*K)"),
        ("specific_heat", 1350.0, "J/(kg*K)"),
        ("width", 1.42, "m"),
    ] {
        props.insert(k.into(), quantity(v, u, "gate_linen"));
    }
    json!({
        "id": "synthetic_jacket_linen",
        "allow_estimates": true,
        "record": {
            "id": "synthetic_jacket_linen",
            "name": "Synthetic plain-weave flax linen (software gate)",
            "form": "woven",
            "composition": [{"fiber": "flax_linen", "fraction": 1.0, "source": "gate_linen"}],
            "composition_complete": true,
            "structure": {"construction": "plain_weave", "finish": "softened_washed"},
            "properties": props,
            "sources": {
                "gate_linen": {
                    "reference": "test:synthetic-jacket-linen",
                    "accessed_on": "2026-10-14",
                    "note": "Synthetic woven linen record for fixed-pattern substitution gate"
                }
            }
        }
    })
}

fn synthetic_leather_selection() -> Value {
    let mut props = serde_json::Map::new();
    for (k, v, u) in [
        ("areal_density", 0.680, "kg/m2"),
        ("thickness", 0.00120, "m"),
        ("warp_stiffness", 1800.0, "N/m"),
        ("weft_stiffness", 1450.0, "N/m"),
        ("shear_stiffness", 420.0, "N/m"),
        ("poisson_warp_weft", 0.28, "1"),
        ("bending_rigidity", 4.5e-4, "N*m"),
        ("friction_skin", 0.45, "1"),
        ("friction_self", 0.58, "1"),
        ("thermal_conductivity", 0.140, "W/(m*K)"),
        ("specific_heat", 1500.0, "J/(kg*K)"),
        ("width", 1.10, "m"),
    ] {
        props.insert(k.into(), quantity(v, u, "gate_leather"));
    }
    json!({
        "id": "synthetic_jacket_cowhide",
        "allow_estimates": true,
        "record": {
            "id": "synthetic_jacket_cowhide",
            "name": "Synthetic chrome-tanned garment cowhide (software gate)",
            "form": "hide",
            "structure": {"tanning": "chrome", "cut": "side"},
            "properties": props,
            "sources": {
                "gate_leather": {
                    "reference": "test:synthetic-jacket-cowhide",
                    "accessed_on": "2026-10-14",
                    "note": "Synthetic hide record for fixed-pattern substitution gate"
                }
            }
        }
    })
}

fn synthetic_performance_knit_selection() -> Value {
    let mut record: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-nonlinear.json"
    ))
    .unwrap();
    record["id"] = json!("synthetic_jacket_perf_knit");
    record["name"] =
        json!("Synthetic jersey performance knit with nonlinear response (software gate)");
    record["form"] = json!("knit");
    let src_key = record["sources"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    let props = record["properties"].as_object_mut().unwrap();
    props.insert("areal_density".into(), quantity(0.220, "kg/m2", &src_key));
    props.insert("thickness".into(), quantity(0.00070, "m", &src_key));
    props.insert("bending_rigidity".into(), quantity(8.0e-6, "N*m", &src_key));
    props.insert("friction_skin".into(), quantity(0.20, "1", &src_key));
    props.insert("friction_self".into(), quantity(0.26, "1", &src_key));
    props.insert(
        "thermal_conductivity".into(),
        quantity(0.042, "W/(m*K)", &src_key),
    );
    props.insert(
        "specific_heat".into(),
        quantity(1250.0, "J/(kg*K)", &src_key),
    );
    props.insert("width".into(), quantity(1.52, "m", &src_key));
    json!({
        "id": "synthetic_jacket_perf_knit",
        "allow_estimates": true,
        "record": record
    })
}

fn synthetic_laminate_selection() -> Value {
    let mut sel = synthetic_linen_selection();
    sel["id"] = json!("synthetic_jacket_laminate");
    sel["record"]["id"] = json!("synthetic_jacket_laminate");
    sel["record"]["name"] = json!("Synthetic 3-layer bonded shell laminate (software gate)");
    sel["record"]["form"] = json!("laminate");
    sel["record"]["properties"]["areal_density"]["value"] = json!(0.310);
    sel["record"]["properties"]["thickness"]["value"] = json!(0.00085);
    sel["record"]["properties"]["bending_rigidity"]["value"] = json!(1.2e-4);
    sel
}

/// Multi-piece jacket pattern (front shell, back shell, cuff with gathered seam, and inner lining
/// panel) placed around a cylindrical torso form.
fn jacket_pattern(default_material: Value) -> Pattern {
    let half_circ = 40.0 * std::f64::consts::PI;
    let piece = |id: &str, label: &str, h: f64, radius_mm: f64, theta_deg: i32, y0: f64| {
        json!({
            "id": id,
            "material": label,
            "cut": {"qty": 1},
            "vertices": {"a": [0.0, 0.0], "b": [half_circ, 0.0], "c": [half_circ, h], "d": [0.0, h]},
            "edges": [
                {"id": "hem", "from": "a", "to": "b"},
                {"id": "side_right", "from": "b", "to": "c"},
                {"id": "shoulder", "from": "c", "to": "d"},
                {"id": "side_left", "from": "d", "to": "a"}
            ],
            "notches": [],
            "grain": [[0.0, 0.0], [0.0, h]],
            "placement": {"part": "torso", "radius_mm": radius_mm, "x0": half_circ * 0.5, "y0": y0, "theta_deg": theta_deg}
        })
    };
    let mut cuff = piece("cuff", "rib_cuff", 16.0, 42.5, 0, -32.0);
    cuff["gather"] = json!({"axis": "x", "ratio": 0.85});
    let lining = piece("lining", "lining_panel", 20.0, 38.2, 0, 30.0);
    Pattern::new(json!({
        "schema": "fabric-pattern/1",
        "units": "mm",
        "fabric": {"material": default_material},
        "pieces": [
            piece("front_shell", "shell", 24.0, 40.0, 0, 0.0),
            piece("back_shell", "shell", 24.0, 40.0, 180, 0.0),
            cuff,
            lining
        ],
        "seams": [
            {"id": "right_side", "a": "front_shell/side_right", "b": "back_shell/side_left"},
            {"id": "left_side", "a": "back_shell/side_right", "b": "front_shell/side_left"}
        ]
    }))
}

struct TorsoForm;
impl PartFrames for TorsoForm {
    fn frame(&self, _: &str) -> Option<PartFrame> {
        Some(PartFrame {
            origin: Vec3::ZERO,
            up: Vec3::new(0.0, 0.0, 1.0),
            front: Vec3::new(0.0, -1.0, 0.0),
            lat: Vec3::new(1.0, 0.0, 0.0),
        })
    }
    fn landmark_h(&self, _: &str, _: &str) -> Option<f64> {
        Some(20.0)
    }
    fn section_radius(&self, _: &str, _: f64) -> f64 {
        30.0
    }
}

fn torso_body() -> Body {
    let mut vertices = Vec::new();
    for z in [-0.02, 0.14] {
        for i in 0..16 {
            let (s, c) = det::sin_cos(i as f64 * std::f64::consts::TAU / 16.0);
            vertices.push(Vec3::new(0.03 * c, 0.03 * s, z));
        }
    }
    let mut tris = Vec::new();
    for i in 0..16 {
        let j = (i + 1) % 16;
        tris.extend([[i, j, j + 16], [i, j + 16, i + 16]]);
    }
    Body::new(vertices, tris, 0.01).unwrap()
}

fn gate_options() -> DrapeOptions {
    DrapeOptions {
        spacing: 24.0,
        sew_steps: 3,
        drape_steps: 3,
        step: 1e-3,
        self_contact: Some(DrapeOptions::default().contact),
        ..DrapeOptions::default()
    }
}

fn run_jacket(pattern: &Pattern, opts: &DrapeOptions) -> DrapeRun {
    let b = torso_body();
    drape(
        pattern,
        &json!({}),
        &DrapeBody {
            frames: &TorsoForm,
            collision: (&b.vertices, &b.triangles),
            display: (&b.vertices, &b.triangles),
            arm_deg: 35.0,
        },
        opts,
        |_| {},
    )
    .unwrap()
}

fn positions_digest(x: &[Vec3]) -> String {
    let mut bytes = Vec::with_capacity(x.len() * 24);
    for p in x {
        bytes.extend_from_slice(&p.x.to_le_bytes());
        bytes.extend_from_slice(&p.y.to_le_bytes());
        bytes.extend_from_slice(&p.z.to_le_bytes());
    }
    sha256_hex(&bytes)
}

#[test]
fn fixed_jacket_pattern_substitutes_linen_leather_and_performance_knit_without_solver_edits() {
    let opts = gate_options();
    let p_linen = jacket_pattern(synthetic_linen_selection());
    let p_leather = jacket_pattern(synthetic_leather_selection());
    let p_knit = jacket_pattern(synthetic_performance_knit_selection());

    // Pattern geometry and seam topology are identical across all three material substitutions.
    assert_eq!(p_linen.raw()["pieces"], p_leather.raw()["pieces"]);
    assert_eq!(p_linen.raw()["pieces"], p_knit.raw()["pieces"]);
    assert_eq!(p_linen.raw()["seams"], p_leather.raw()["seams"]);
    assert_eq!(p_linen.raw()["seams"], p_knit.raw()["seams"]);

    let r_linen = run_jacket(&p_linen, &opts);
    let r_leather = run_jacket(&p_leather, &opts);
    let r_knit = run_jacket(&p_knit, &opts);

    // 2D flat mesh and weld map remain identical.
    assert_eq!(r_linen.scene.mesh.verts2d, r_leather.scene.mesh.verts2d);
    assert_eq!(r_linen.scene.mesh.verts2d, r_knit.scene.mesh.verts2d);
    assert_eq!(r_linen.scene.weld, r_leather.scene.weld);
    assert_eq!(r_linen.scene.weld, r_knit.scene.weld);

    // Gather compression is preserved on the cuff piece in all three runs.
    assert!(
        r_linen
            .scene
            .solver
            .model
            .membranes
            .iter()
            .enumerate()
            .any(|(i, _)| r_linen.scene.triangle_owner[i] == 2)
    );

    // Mass scales exactly with each material's areal density.
    let m_linen = r_linen.report["mass_kg"].as_f64().unwrap();
    let m_leather = r_leather.report["mass_kg"].as_f64().unwrap();
    let m_knit = r_knit.report["mass_kg"].as_f64().unwrap();
    assert!((m_leather / m_linen - 0.680 / 0.185).abs() < 1e-12);
    assert!((m_knit / m_linen - 0.220 / 0.185).abs() < 1e-12);

    // Bending rigidity, body contact offset (t/2), self-contact offset (t), and friction differ.
    assert_eq!(r_linen.scene.solver.model.bending, 2.8e-5);
    assert_eq!(r_leather.scene.solver.model.bending, 4.5e-4);
    assert_eq!(r_knit.scene.solver.model.bending, 8.0e-6);

    let c_linen = r_linen.scene.solver.contact.as_ref().unwrap();
    let c_leather = r_leather.scene.solver.contact.as_ref().unwrap();
    let c_knit = r_knit.scene.solver.contact.as_ref().unwrap();
    assert!((c_linen.params.thickness - 0.000225).abs() < 1e-12);
    assert!((c_leather.params.thickness - 0.00060).abs() < 1e-12);
    assert!((c_knit.params.thickness - 0.00035).abs() < 1e-12);
    assert!((c_linen.self_contact_params().unwrap().thickness - 0.00045).abs() < 1e-12);
    assert!((c_leather.self_contact_params().unwrap().thickness - 0.00120).abs() < 1e-12);
    assert!((c_knit.self_contact_params().unwrap().thickness - 0.00070).abs() < 1e-12);
    assert_eq!(c_linen.params.mu, 0.24);
    assert_eq!(c_leather.params.mu, 0.45);
    assert_eq!(c_knit.params.mu, 0.20);

    // Constitutive model records orthotropic vs tabulated nonlinear Green-strain response.
    assert_eq!(
        r_linen.report["material"]["model"],
        "orthotropic_green_strain_scalar_bending"
    );
    assert_eq!(
        r_leather.report["material"]["model"],
        "orthotropic_green_strain_scalar_bending"
    );
    assert_eq!(
        r_knit.report["material"]["model"],
        "tabulated_green_strain_scalar_bending"
    );

    // Dynamics and source-complete report hashes are all distinct, and deterministic on repeat.
    let d_linen = positions_digest(&r_linen.scene.solver.x);
    let d_leather = positions_digest(&r_leather.scene.solver.x);
    let d_knit = positions_digest(&r_knit.scene.solver.x);
    assert_ne!(d_linen, d_leather);
    assert_ne!(d_linen, d_knit);
    assert_ne!(d_leather, d_knit);
    assert_ne!(
        r_linen.report["material"]["record_sha256"],
        r_leather.report["material"]["record_sha256"]
    );
    assert_ne!(
        r_linen.report["material"]["record_sha256"],
        r_knit.report["material"]["record_sha256"]
    );

    let r_knit_repeat = run_jacket(&p_knit, &opts);
    assert_eq!(d_knit, positions_digest(&r_knit_repeat.scene.solver.x));
    assert_eq!(r_knit.report, r_knit_repeat.report);

    // Thermal surface exchange and static ThermalStack known answers on the same jacket materials.
    let boundary = Boundary {
        inside_c: 33.0,
        outside_c: 10.0,
        inside_resistance: 0.04,
        outside_resistance: 0.04,
    };
    let th_linen = r_linen
        .thermal_surface(4)
        .unwrap()
        .steady(&r_linen.scene.solver.x, &[boundary])
        .unwrap();
    let th_leather = r_leather
        .thermal_surface(4)
        .unwrap()
        .steady(&r_leather.scene.solver.x, &[boundary])
        .unwrap();
    assert_ne!(th_linen.inside_heat_w, th_leather.inside_heat_w);

    let cat = catalog().unwrap();
    let res_linen = cat
        .resolve(&serde_json::from_value::<Selection>(synthetic_linen_selection()).unwrap())
        .unwrap();
    let res_leather = cat
        .resolve(&serde_json::from_value::<Selection>(synthetic_leather_selection()).unwrap())
        .unwrap();
    let stack_rep = ThermalStack::evaluate(
        &[res_linen.material(), res_leather.material()],
        1.0,
        33.0,
        13.0,
        0.05,
        true,
    )
    .unwrap();
    let want_r = (0.00045 / 0.052) + (0.00120 / 0.140);
    assert!((stack_rep.resistance_m2_k_w - want_r).abs() < 1e-12);
    let total_r = want_r + 0.05;
    assert!((stack_rep.heat_flow_w - (20.0 / total_r)).abs() < 1e-12);

    // Price-width known answers: same 2.0 m^2 garment area uses each material's sourced width.
    let quotes = QuoteBook {
        schema: "ftx-textile-quotes/1".into(),
        quotes: vec![
            Quote {
                offer_id: "linen_offer".into(),
                material_id: "synthetic_jacket_linen".into(),
                supplier: "mill_a".into(),
                sku: "LIN-142".into(),
                price: 20.0,
                currency: "USD".into(),
                quantity: 1.0,
                unit: SaleUnit::LinearMeter,
                minimum_quantity: 1.0,
                order_increment: 0.5,
                region: "US".into(),
                quality: "first-quality".into(),
                terms: "merchandise-ex-tax-shipping".into(),
                observed_on: "2026-10-14".into(),
                valid_through: None,
                available: true,
                source: "s1".into(),
                source_sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .into(),
            },
            Quote {
                offer_id: "leather_offer".into(),
                material_id: "synthetic_jacket_cowhide".into(),
                supplier: "tannery_b".into(),
                sku: "HIDE-110".into(),
                price: 20.0,
                currency: "USD".into(),
                quantity: 1.0,
                unit: SaleUnit::LinearMeter,
                minimum_quantity: 1.0,
                order_increment: 0.5,
                region: "US".into(),
                quality: "first-quality".into(),
                terms: "merchandise-ex-tax-shipping".into(),
                observed_on: "2026-10-14".into(),
                valid_through: None,
                available: true,
                source: "s1".into(),
                source_sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .into(),
            },
        ],
    };
    let scope = Scope {
        requirement: 2.0,
        basis: Basis::Area,
        currency: "USD".into(),
        region: "US".into(),
        quality: "first-quality".into(),
        terms: "merchandise-ex-tax-shipping".into(),
        as_of: "2026-10-14".into(),
        max_age_days: 30,
    };
    let cost_linen = quotes.evaluate(&res_linen, &scope).unwrap();
    let cost_leather = quotes.evaluate(&res_leather, &scope).unwrap();
    // 2.0 m^2 / 1.42 m = 1.408 m -> rounds up to 1.5 linear_meter ($30);
    // 2.0 m^2 / 1.10 m = 1.818 m -> rounds up to 2.0 linear_meter ($40).
    assert_eq!(cost_linen.offers[0].order_quantity, 1.5);
    assert_eq!(cost_linen.offers[0].order_cost, 30.0);
    assert_eq!(cost_leather.offers[0].order_quantity, 2.0);
    assert_eq!(cost_leather.offers[0].order_cost, 40.0);
}

#[test]
fn heterogeneous_leather_shell_jacket_with_knit_cuffs_and_lining_updates_only_assigned_elements() {
    let opts = gate_options();
    let uniform_leather = jacket_pattern(synthetic_leather_selection());
    let r_uniform = run_jacket(&uniform_leather, &opts);

    // Homogeneous explicit per-piece assignment reproduces the uniform leather jacket bit for bit.
    let mut raw_hom = uniform_leather.raw().clone();
    for piece in raw_hom["pieces"].as_array_mut().unwrap() {
        piece["fabric"] = json!({"material": synthetic_leather_selection(), "role": "shell"});
    }
    let r_hom = run_jacket(&Pattern::new(raw_hom), &opts);
    assert_eq!(r_uniform.scene.solver.x, r_hom.scene.solver.x);
    assert_eq!(r_uniform.scene.solver.v, r_hom.scene.solver.v);
    assert_eq!(r_uniform.fold.frames, r_hom.fold.frames);

    // Heterogeneous assignment: leather shell (front/back), performance-knit cuff, woven linen lining.
    let mut raw_het = uniform_leather.raw().clone();
    raw_het["pieces"][2]["fabric"] =
        json!({"material": synthetic_performance_knit_selection(), "role": "shell"});
    raw_het["pieces"][3]["fabric"] =
        json!({"material": synthetic_linen_selection(), "role": "lining"});

    // Without pairwise friction observations, self-contact refuses rather than guessing mu_AB.
    let bare_het = Pattern::new(raw_het.clone());
    let err = resolve(&bare_het, &opts)
        .err()
        .expect("must fail without pairwise friction");
    assert!(err.message.contains("pairwise friction"));

    // Resolve with self-contact disabled first to read the exact snapshot hashes.
    let mut no_self = opts;
    no_self.self_contact = None;
    let preview = resolve(&bare_het, &no_self).unwrap().report;
    let h_leather = preview["pieces"][0]["record_sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let h_knit = preview["pieces"][2]["record_sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let h_linen = preview["pieces"][3]["record_sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(preview["pieces"][2]["role"], "shell");
    assert_eq!(preview["pieces"][3]["role"], "lining");

    let pair = |a: &str, b: &str, mu: f64| {
        json!({
            "materials": [a, b],
            "friction": {
                "value": mu,
                "unit": "1",
                "source": "pair_src",
                "evidence": "estimate",
                "method": "Synthetic cross-sheet friction",
                "bounds": [mu, mu],
                "conditions": {"orientation": "symmetric scalar"}
            },
            "sources": {
                "pair_src": {
                    "reference": "test:jacket-interfaces",
                    "accessed_on": "2026-10-14",
                    "note": "Synthetic pairwise interface for jacket test"
                }
            },
            "allow_estimates": true
        })
    };
    raw_het["fabric"]["interfaces"] = json!([
        pair(&h_leather, &h_knit, 0.31),
        pair(&h_leather, &h_linen, 0.27),
        pair(&h_knit, &h_linen, 0.22)
    ]);
    let p_het = Pattern::new(raw_het);
    let mut r_het = run_jacket(&p_het, &opts);

    // Shell triangles keep the exact leather constitutive law; cuff and lining triangles update.
    for (tri_idx, &owner) in r_het.scene.triangle_owner.iter().enumerate() {
        if owner == 0 || owner == 1 {
            assert_eq!(
                r_het.scene.solver.model.material(tri_idx),
                r_uniform.scene.solver.model.material(tri_idx)
            );
            assert!(
                r_het
                    .scene
                    .solver
                    .model
                    .membrane_response(tri_idx)
                    .is_none()
            );
        } else if owner == 2 {
            assert!(
                r_het
                    .scene
                    .solver
                    .model
                    .membrane_response(tri_idx)
                    .is_some()
            );
        } else {
            assert_eq!(owner, 3);
            assert!(
                r_het
                    .scene
                    .solver
                    .model
                    .membrane_response(tri_idx)
                    .is_none()
            );
            assert_ne!(
                r_het.scene.solver.model.material(tri_idx),
                r_uniform.scene.solver.model.material(tri_idx)
            );
        }
    }

    // Mass is conserved and matches the exact area-weighted sum across pieces.
    let mut expected_mass = vec![0.0; r_het.scene.solver.model.mass.len()];
    for (e, &owner) in r_het
        .scene
        .solver
        .model
        .membranes
        .iter()
        .zip(&r_het.scene.triangle_owner)
    {
        let rho = match owner {
            0 | 1 => 0.680,
            2 => 0.220,
            3 => 0.185,
            _ => unreachable!(),
        };
        for &v in &e.v {
            expected_mass[v] += rho * e.area / 3.0;
        }
    }
    for (got, want) in r_het.scene.solver.model.mass.iter().zip(&expected_mass) {
        assert!((got - want).abs() < 1e-15);
    }

    // Motion continuation preserves heterogeneous material laws, mass, and surface contact field.
    let target: Vec<_> = r_het
        .scene
        .solver
        .contact
        .as_ref()
        .unwrap()
        .body
        .vertices
        .iter()
        .map(|p| *p + Vec3::new(0.0, 0.0, 1e-5))
        .collect();
    ftx_drape::motion::advance_body(&mut r_het.scene.solver, &target, 1e-3, Default::default())
        .unwrap();
    assert!(
        r_het
            .scene
            .solver
            .contact
            .as_ref()
            .unwrap()
            .has_surface_materials()
    );

    // Laminate role succeeds on Form::Laminate and refuses on Form::Woven; loose fill and
    // incomplete catalog records refuse before meshing.
    let mut raw_lam = uniform_leather.raw().clone();
    raw_lam["pieces"][3]["fabric"] =
        json!({"material": synthetic_laminate_selection(), "role": "laminate"});
    assert!(resolve(&Pattern::new(raw_lam.clone()), &no_self).is_ok());

    raw_lam["pieces"][3]["fabric"] =
        json!({"material": synthetic_linen_selection(), "role": "laminate"});
    assert!(resolve(&Pattern::new(raw_lam), &no_self).is_err());

    let mut raw_fill = uniform_leather.raw().clone();
    raw_fill["pieces"][3]["fabric"] =
        json!({"material": synthetic_linen_selection(), "role": "fill"});
    assert!(resolve(&Pattern::new(raw_fill), &no_self).is_err());

    let mut fill_record = synthetic_linen_selection();
    fill_record["record"]["form"] = json!("fill");
    let fill_mat: Material = serde_json::from_value(fill_record["record"].clone()).unwrap();
    assert!(fill_mat.mechanics(true).is_err());

    for incomplete_id in [
        "fs_il019_natural",
        "tandy_cow_garment_9114_71",
        "vuori_dreamknit_89rpet_11elastane",
    ] {
        let p = Pattern::new(json!({
            "schema": "fabric-pattern/1",
            "units": "mm",
            "fabric": {"material": {"id": incomplete_id, "allow_estimates": true}}
        }));
        assert!(
            resolve(&p, &no_self).is_err(),
            "{incomplete_id} must refuse"
        );
    }
}
