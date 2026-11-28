//! Public source-topology/material integration; all bodies and material values are synthetic.
use ftx_core::shaping::{BoundaryRing, EaseShellMesh};
use ftx_core::{RefusalCode, Vec2, Vec3, det};
use ftx_drape::knit::{
    CoupledKnitMembrane, KnitDrapeOptions, KnitSurfaceTopology, audit_solver_knit_patch,
    drape_knit_doc,
};
use ftx_drape::run::{DrapeBody, DrapeOptions};
use ftx_drape::scene::PartFrames;
use ftx_drape::{ClothModel, Membrane, PartFrame, Solver};
use ftx_pattern::knit::{
    AutoKnitConfig, CarrierId, GaugeSpec, KnitDoc, MachineProfile, ParametricPanelSpec,
    WearerAgeClass, draft_autoknit_from_shell, draft_shaped_panel,
};
use ftx_pattern::material_catalog::ResolvedMaterial;
use ftx_pattern::materials::Evidence;
use serde_json::{Value, json};

struct TestFrames(Option<f64>);

impl PartFrames for TestFrames {
    fn frame(&self, part: &str) -> Option<PartFrame> {
        (self.0.is_some() && part == "torso").then_some(PartFrame {
            origin: Vec3::ZERO,
            up: Vec3::new(0.0, 0.0, 1.0),
            front: Vec3::new(0.0, -1.0, 0.0),
            lat: Vec3::new(1.0, 0.0, 0.0),
        })
    }
    fn landmark_h(&self, part: &str, name: &str) -> Option<f64> {
        self.0.filter(|_| part == "torso" && name == "snp")
    }
    fn section_radius(&self, _part: &str, _height: f64) -> f64 {
        0.0
    }
}

fn material_selection() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-coupled-knit.json"
    ))
    .unwrap()
}

fn selected_material(selection: Value) -> ResolvedMaterial {
    ftx_pattern::textile_import::resolve_fabric(&json!({"material":selection}))
        .unwrap()
        .unwrap()
}

fn flat_material(with_history: bool) -> ResolvedMaterial {
    let mut selection = material_selection();
    selection["record"]["knit_response"]["bending"]["parameters"]["kappa0"] =
        json!([0.0, 0.0, 0.0]);
    if !with_history {
        selection["record"]["knit_response"]
            .as_object_mut()
            .unwrap()
            .remove("hysteresis");
    }
    selected_material(selection)
}

fn gauge(profile: &MachineProfile) -> GaugeSpec {
    GaugeSpec {
        wales_per_100mm: 20.0,
        courses_per_100mm: 25.0,
        needle_pitch_mm: profile.needle_pitch_mm,
        yarn_tex: Some(200.0),
        thickness_mm: Some(2.0),
        relaxed_after_wash: true,
    }
}

fn panel(width_mm: f64, length_mm: f64, rib_hem_courses: u32) -> KnitDoc {
    let profile = MachineProfile::kniterate_7e();
    draft_shaped_panel(
        &ParametricPanelSpec {
            name: "source_panel".to_string(),
            wearer_age_class: WearerAgeClass::Adult,
            hem_width_mm: width_mm,
            chest_width_mm: width_mm,
            length_mm,
            armhole_depth_mm: 0.0,
            armhole_inset_total_mm: 0.0,
            neck_width_mm: 0.0,
            neck_depth_mm: 0.0,
            rib_hem_courses,
            cam_setting: 7.0,
            yarn_id: "synthetic-yarn".to_string(),
            carrier_id: CarrierId("1".to_string()),
            has_hood_or_neck_drawstring: false,
        },
        &gauge(&profile),
        &profile,
    )
    .unwrap()
}

fn cylinder(radius_m: f64, height_m: f64) -> (Vec<Vec3>, Vec<[usize; 3]>) {
    let rings = 6usize;
    let segs = 12usize;
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for r in 0..=rings {
        for s in 0..segs {
            let theta = std::f64::consts::TAU * s as f64 / segs as f64;
            let (sin, cos) = det::sin_cos(theta);
            vertices.push(Vec3::new(
                radius_m * cos,
                radius_m * sin,
                r as f64 / rings as f64 * height_m,
            ));
        }
    }
    for r in 0..rings {
        for s in 0..segs {
            let next = (s + 1) % segs;
            let a = r * segs + s;
            let b = r * segs + next;
            let c = (r + 1) * segs + next;
            let d = (r + 1) * segs + s;
            triangles.extend([[a, b, c], [a, c, d]]);
        }
    }
    (vertices, triangles)
}

fn body<'a>(
    frames: &'a TestFrames,
    vertices: &'a [Vec3],
    triangles: &'a [[usize; 3]],
) -> DrapeBody<'a> {
    DrapeBody {
        frames,
        collision: (vertices, triangles),
        display: (vertices, triangles),
        arm_deg: ftx_body::Pose::A_POSE_DEG,
        arm_pose: None,
    }
}

fn preparation(material: ResolvedMaterial) -> KnitDrapeOptions {
    KnitDrapeOptions {
        drape_opts: DrapeOptions {
            sew_steps: 6,
            drape_steps: 0,
            step: 1e-6,
            ..DrapeOptions::default()
        },
        material,
    }
}

#[test]
fn actual_panel_uses_source_mass_contacts_and_all_requested_intervals() {
    let doc = panel(40.0, 32.0, 2);
    let (vertices, triangles) = cylinder(0.08, 0.55);
    let frames = TestFrames(Some(500.0));
    let body = body(&frames, &vertices, &triangles);
    let mut options = preparation(flat_material(false));
    options.drape_opts.sew_steps = 25;
    options.drape_opts.step = DrapeOptions::default().step;
    options.drape_opts.self_contact = Some(DrapeOptions::default().contact);
    let mut result = drape_knit_doc(&doc, &body, &options).unwrap();
    assert!(
        !result.is_tubular,
        "alternating rib beds do not make a closed tube"
    );
    assert_eq!(result.surface.vertices.len(), doc.stitches.len());
    assert_eq!(
        result.steps_run, 25,
        "physical intervals must not be capped at 24"
    );
    assert_eq!(result.sew_steps_run, 25);
    assert_eq!(result.drape_steps_run, 0);
    assert!(
        !result.settled,
        "zero-gravity preparation cannot establish a gravity equilibrium"
    );
    assert_eq!(result.assessment.receipt.evidence, Evidence::Estimate);
    assert!(result.solver.knit_hysteresis_report().unwrap().is_none());
    let area = result
        .solver
        .model
        .membranes
        .iter()
        .map(|e| e.area)
        .sum::<f64>();
    let mass = result.solver.model.mass.iter().sum::<f64>();
    assert!((mass - 0.2 * area).abs() < 1e-14);
    assert_eq!(result.material_report["body_offset_m"], json!(0.0003));
    assert_eq!(
        result.material_report["mechanics"]["friction_skin"],
        json!(0.2)
    );
    assert_eq!(result.material_report["self_contact_enabled"], json!(true));
    let self_contact = result
        .solver
        .contact
        .as_ref()
        .unwrap()
        .self_contact_params()
        .unwrap();
    assert_eq!(self_contact.thickness, 0.0006);
    assert_eq!(self_contact.mu, 0.3);
    assert_eq!(
        result.material_report["complete_knit_assessment"],
        json!(false)
    );

    let audit =
        audit_solver_knit_patch(&result.solver, "selvedge", &options.material, 0.005).unwrap();
    assert!(!audit.hardware_execution_authorized && !audit.fit_claimed);
    assert!(audit.selected_triangles > 0);
    assert_eq!(audit.material_sha256, options.material.sha256());
    assert_eq!(audit.pore_open_fraction, None);
    assert!(audit_solver_knit_patch(&result.solver, "collar", &options.material, 0.005).is_err());
    let mut different = material_selection();
    different["record"]["knit_response"]["membrane"]["parameters"]["q11"] = json!(181.0);
    let mismatch = selected_material(different);
    assert_eq!(
        audit_solver_knit_patch(&result.solver, "all", &mismatch, 0.005)
            .unwrap_err()
            .code,
        RefusalCode::VerificationMismatch
    );
    let mut truncated = result.solver.clone();
    truncated.x.pop();
    assert_eq!(
        audit_solver_knit_patch(&truncated, "all", &options.material, 0.005)
            .unwrap_err()
            .code,
        RefusalCode::OutOfRange
    );
    result.solver.x[0].z = f64::NAN;
    assert_eq!(
        audit_solver_knit_patch(&result.solver, "all", &options.material, 0.005)
            .unwrap_err()
            .code,
        RefusalCode::NonFinite
    );

    let with_history = drape_knit_doc(&doc, &body, &preparation(flat_material(true))).unwrap();
    let history = with_history
        .solver
        .knit_hysteresis_report()
        .unwrap()
        .unwrap();
    assert_eq!(history["accepted_steps"], json!(6));
}

#[test]
fn source_placement_keeps_narrow_tall_extent_and_requires_actual_frame() {
    let doc = panel(35.0, 80.0, 0);
    let (vertices, triangles) = cylinder(0.008, 0.055);
    let frames = TestFrames(Some(50.0));
    let options = preparation(flat_material(false));
    let result = drape_knit_doc(&doc, &body(&frames, &vertices, &triangles), &options).unwrap();
    let initial = &result.frames[0];
    let height = initial
        .iter()
        .map(|p| p.z)
        .fold(f64::NEG_INFINITY, f64::max)
        - initial.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
    let width = initial
        .iter()
        .map(|p| p.x)
        .fold(f64::NEG_INFINITY, f64::max)
        - initial.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
    let surface = &result.surface;
    let expected_height =
        f64::from(surface.course_range[1] - surface.course_range[0]) * surface.course_height_m;
    let expected_width =
        f64::from(surface.wale_range[1] - surface.wale_range[0]) * surface.wale_pitch_m;
    assert!((height - expected_height).abs() < 1e-12 && height > 0.055);
    assert!((width - expected_width).abs() < 1e-12 && width < 0.04);
    let absent = TestFrames(None);
    assert_eq!(
        drape_knit_doc(&doc, &body(&absent, &vertices, &triangles), &options)
            .unwrap_err()
            .code,
        RefusalCode::InvalidDocument
    );
}

#[test]
fn real_annular_autoknit_preserves_outward_normals_and_top_supports_in_both_directions() {
    let (source_vertices, source_triangles) = cylinder(0.013, 0.032);
    let mut shell = EaseShellMesh {
        vertices_m: source_vertices.iter().map(|p| [p.x, p.y, p.z]).collect(),
        triangles: source_triangles
            .iter()
            .map(|t| t.map(|i| i as u32))
            .collect(),
        boundary_rings: vec![
            BoundaryRing {
                name: "hem".to_string(),
                is_start: true,
                vertices: (0..12).collect(),
                perimeter_m: 0.0,
            },
            BoundaryRing {
                name: "neck".to_string(),
                is_start: false,
                vertices: (72..84).rev().collect(),
                perimeter_m: 0.0,
            },
        ],
        source_body_sha256: "synthetic-cylinder-anatomy".to_string(),
    };
    for ring in &mut shell.boundary_rings {
        ring.perimeter_m = (0..ring.vertices.len())
            .map(|i| {
                let a = source_vertices[ring.vertices[i] as usize];
                let b = source_vertices[ring.vertices[(i + 1) % ring.vertices.len()] as usize];
                (a - b).norm()
            })
            .sum();
    }
    let profile = MachineProfile::kniterate_7e();
    let config = AutoKnitConfig {
        name: "source_torso".to_string(),
        wearer_age_class: WearerAgeClass::Adult,
        yarn_id: "synthetic-yarn".to_string(),
        carrier_id: CarrierId("1".to_string()),
        cam_setting: 7.0,
        max_idt_flips: 1000,
        has_hood_or_neck_drawstring: false,
    };
    let (vertices, triangles) = cylinder(0.005, 0.055);
    let frames = TestFrames(Some(50.0));
    let options = preparation(flat_material(false));
    for upward in [true, false] {
        shell.boundary_rings[0].is_start = upward;
        shell.boundary_rings[1].is_start = !upward;
        let doc = draft_autoknit_from_shell(&shell, &gauge(&profile), &profile, &config)
            .unwrap()
            .doc;
        let result = drape_knit_doc(&doc, &body(&frames, &vertices, &triangles), &options).unwrap();
        assert_eq!(result.surface.topology, KnitSurfaceTopology::Tube);
        assert_eq!(result.surface.course_growth_upward, upward);
        let support_course = result.surface.course_range[usize::from(upward)];
        for id in result.material_report["placement"]["support_stitch_ids"]
            .as_array()
            .unwrap()
        {
            let id = id.as_u64().unwrap() as u32;
            assert_eq!(
                result
                    .surface
                    .vertices
                    .iter()
                    .find(|v| v.stitch_id == id)
                    .unwrap()
                    .course,
                support_course
            );
        }
        let initial = &result.frames[0];
        for &[a, b, c] in &result.triangles {
            let normal = (initial[b] - initial[a]).cross(initial[c] - initial[a]);
            let centre = (initial[a] + initial[b] + initial[c]) / 3.0;
            assert!(normal.dot(Vec3::new(centre.x, centre.y, 0.0)) > 0.0);
        }
    }
}

#[test]
fn patch_audit_averages_actual_nonlinear_energies_and_keeps_local_jamming_extrema() {
    let mut selection = material_selection();
    let response = selection["record"]["knit_response"]
        .as_object_mut()
        .unwrap();
    response.remove("bending");
    response.remove("hysteresis");
    let material = selected_material(selection);
    let compiled = material.coupled_knit().unwrap().unwrap();
    let law: CoupledKnitMembrane =
        serde_json::from_value(serde_json::to_value(compiled.membrane).unwrap()).unwrap();
    let rest = vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(0.0, 1.0),
        Vec2::new(2.0, 0.0),
        Vec2::new(4.0, 0.0),
        Vec2::new(2.0, 1.0),
    ];
    let mut model = ClothModel::new(
        rest.clone(),
        &[[0, 1, 2], [3, 4, 5]],
        Membrane::orthotropic(180.0, 95.0, 0.2, 48.0, 1.0),
        0.00003,
        0.2,
    )
    .unwrap();
    model
        .set_coupled_knit_membranes(&[Some(law), Some(law)])
        .unwrap();
    let mut positions: Vec<_> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
    positions[1].x = det::sqrt(1.1);
    positions[5].y = det::sqrt(1.4);
    let solver = Solver::new(model, positions, &[[false; 3]; 6], Vec3::ZERO);
    let report = audit_solver_knit_patch(&solver, "all", &material, 0.005).unwrap();
    let first = law.evaluate([0.05, 0.0, 0.0]).unwrap();
    let second = law.evaluate([0.0, 0.2, 0.0]).unwrap();
    let expected = (0.5 * first.energy_density + second.energy_density) / 1.5;
    assert!((report.energy_density_j_m2 - expected).abs() < 1e-12);
    assert!(
        (report.jamming_margin - first.jamming_margin.min(second.jamming_margin)).abs() < 1e-12
    );
    let surrogate = law.evaluate(report.macro_strain).unwrap().energy_density;
    assert!((report.energy_density_j_m2 - surrogate).abs() > 0.1);

    // A matching coupled coefficient block is insufficient if the consuming solver changes
    // either directional compression coordinate before evaluating that block.
    for (triangle, mapping) in [(0, [0.5, 1.0]), (1, [1.0, 0.5])] {
        let mut softened = solver.clone();
        softened.model.compression[triangle] = mapping;
        let refusal = audit_solver_knit_patch(&softened, "all", &material, 0.005).unwrap_err();
        assert_eq!(refusal.code, RefusalCode::VerificationMismatch);
        assert!(refusal.message.contains("compression mapping"));
    }
    let mut missing = solver.clone();
    missing.model.compression.pop();
    assert_eq!(
        audit_solver_knit_patch(&missing, "all", &material, 0.005)
            .unwrap_err()
            .code,
        RefusalCode::VerificationMismatch
    );
}

#[test]
fn patch_audit_refuses_vector_hessian_curvature_without_changing_its_energy() {
    let mut selection = material_selection();
    let response = selection["record"]["knit_response"]
        .as_object_mut()
        .unwrap();
    response.remove("bending");
    response.remove("hysteresis");
    selection["record"]["structure"]["bending_model"] = json!(ftx_pattern::material_bending::MODEL);
    for (key, value) in [
        ("bending_warp", 0.0002),
        ("bending_weft", 0.00005),
        ("bending_coupling", 0.00001),
        ("bending_twist", 0.000025),
    ] {
        let mut quantity = selection["record"]["properties"]["bending_rigidity"].clone();
        quantity["value"] = json!(value);
        quantity["method"] = json!("Explicit synthetic directional bending fixture");
        selection["record"]["properties"][key] = quantity;
    }
    let material = selected_material(selection);
    let compiled = material.coupled_knit().unwrap().unwrap();
    let membrane: CoupledKnitMembrane =
        serde_json::from_value(serde_json::to_value(compiled.membrane).unwrap()).unwrap();
    let tensor = material.bending_response().unwrap().unwrap();
    assert!(!tensor.uses_normal());
    let rest: Vec<_> = (0..3)
        .flat_map(|y| (0..3).map(move |x| Vec2::new(f64::from(x) * 0.01, f64::from(y) * 0.01)))
        .collect();
    let mut triangles = Vec::new();
    for y in 0..2 {
        for x in 0..2 {
            let a = y * 3 + x;
            triangles.extend([[a, a + 1, a + 4], [a, a + 4, a + 3]]);
        }
    }
    let mut model = ClothModel::new(
        rest.clone(),
        &triangles,
        Membrane::orthotropic(180.0, 95.0, 0.2, 48.0, 1.0),
        0.00003,
        0.2,
    )
    .unwrap();
    model
        .set_coupled_knit_membranes(&vec![Some(membrane); triangles.len()])
        .unwrap();
    model
        .set_panel_bending(
            &rest,
            &triangles,
            &(0..rest.len()).collect::<Vec<_>>(),
            &vec![Some(tensor); triangles.len()],
        )
        .unwrap();
    let positions: Vec<_> = rest
        .iter()
        .map(|p| Vec3::new(p.x, p.y, p.x * p.x))
        .collect();
    let energy: f64 = model
        .panel_bending_patches()
        .iter()
        .map(|p| p.energy(&positions))
        .sum();
    // z = u² has vector curvature [2, 0, 0] and the 20 mm square has area 0.0004 m².
    let expected_energy = 0.5 * 0.0002 * 4.0 * 0.0004;
    assert!(
        (energy - expected_energy).abs() < 1e-16,
        "the source vector-Hessian energy remains supported"
    );
    let solver = Solver::new(model, positions, &vec![[false; 3]; rest.len()], Vec3::ZERO);
    let refusal = audit_solver_knit_patch(&solver, "all", &material, 0.005).unwrap_err();
    assert_eq!(refusal.code, RefusalCode::UnqualifiedProcess);
    assert!(refusal.message.contains("vector-Hessian"));
    assert!(refusal.message.contains("normal-curvature"));
}

#[test]
fn unsupported_legacy_mesh_and_compression_overrides_refuse_before_placement() {
    let doc = panel(40.0, 32.0, 0);
    let absent = TestFrames(None);
    let (vertices, triangles) = cylinder(0.008, 0.055);
    let body = body(&absent, &vertices, &triangles);
    let mut spacing = preparation(flat_material(false));
    spacing.drape_opts.spacing = 6.0;
    let refused = drape_knit_doc(&doc, &body, &spacing).unwrap_err();
    assert_eq!(refused.code, RefusalCode::InvalidDocument);
    assert!(refused.message.contains("spacing"));
    let mut compression = preparation(flat_material(false));
    compression.drape_opts.compression = 0.5;
    let refused = drape_knit_doc(&doc, &body, &compression).unwrap_err();
    assert_eq!(refused.code, RefusalCode::InvalidDocument);
    assert!(refused.message.contains("compression"));
}
