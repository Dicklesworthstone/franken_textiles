//! Source-topology and intrinsic-metric regressions for knitted continuum surfaces.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::RefusalCode;
use ftx_drape::knit::{KnitSurfaceMesh, KnitSurfaceSide, KnitSurfaceTopology, mesh_knit_doc};
use ftx_pattern::knit::{
    Bed, BranchedPulloverSpec, CarrierId, GaugeSpec, LoopKind, MachineProfile, ParametricPanelSpec,
    RaglanPulloverSpec, WearerAgeClass, draft_branched_raglan_pullover, draft_seamless_raglan_tube,
    draft_shaped_panel, draft_short_row_neckline_panel,
};

fn gauge() -> GaugeSpec {
    GaugeSpec {
        wales_per_100mm: 25.0,
        courses_per_100mm: 25.0,
        needle_pitch_mm: MachineProfile::kniterate_7e().needle_pitch_mm,
        yarn_tex: Some(180.0),
        thickness_mm: Some(1.5),
        relaxed_after_wash: true,
    }
}

fn panel_spec() -> ParametricPanelSpec {
    ParametricPanelSpec {
        name: "source-shaped-panel".to_string(),
        wearer_age_class: WearerAgeClass::Adult,
        hem_width_mm: 32.0,
        chest_width_mm: 40.0,
        length_mm: 48.0,
        armhole_depth_mm: 16.0,
        armhole_inset_total_mm: 8.0,
        neck_width_mm: 8.0,
        neck_depth_mm: 16.0,
        rib_hem_courses: 2,
        cam_setting: 6.0,
        yarn_id: "synthetic-surface-fixture".to_string(),
        carrier_id: CarrierId("1".to_string()),
        has_hood_or_neck_drawstring: false,
    }
}

fn tube_spec(rib: bool) -> RaglanPulloverSpec {
    RaglanPulloverSpec {
        name: "source-shaped-tube".to_string(),
        wearer_age_class: WearerAgeClass::Adult,
        neck_girth_mm: 64.0,
        chest_girth_mm: 80.0,
        yoke_depth_mm: 24.0,
        body_length_below_arm_mm: 24.0,
        collar_rib_courses: if rib { 2 } else { 0 },
        hem_rib_courses: if rib { 2 } else { 0 },
        cam_setting: 6.0,
        yarn_id: "synthetic-surface-fixture".to_string(),
        carrier_id: CarrierId("1".to_string()),
        has_hood_or_neck_drawstring: false,
    }
}

fn surface_edges(surface: &KnitSurfaceMesh) -> BTreeSet<(usize, usize)> {
    surface
        .triangles
        .iter()
        .flat_map(|triangle| {
            [(0, 1), (1, 2), (2, 0)]
                .map(|(a, b)| (triangle[a].min(triangle[b]), triangle[a].max(triangle[b])))
        })
        .collect()
}

#[test]
fn rectangular_source_area_and_height_are_not_enlarged_to_preview_minima() {
    let mut spec = panel_spec();
    spec.hem_width_mm = 16.0;
    spec.chest_width_mm = 16.0;
    spec.length_mm = 24.0;
    spec.armhole_depth_mm = 0.0;
    spec.armhole_inset_total_mm = 0.0;
    spec.neck_width_mm = 0.0;
    spec.neck_depth_mm = 0.0;
    spec.rib_hem_courses = 0;
    let doc = draft_shaped_panel(&spec, &gauge(), &MachineProfile::kniterate_7e()).unwrap();
    let surface = mesh_knit_doc(&doc, 1_000).unwrap();
    assert_eq!(surface.topology, KnitSurfaceTopology::Panel);
    assert!(surface.course_growth_upward);
    assert_eq!(surface.vertices.len(), doc.stitches.len());
    assert_eq!(surface.vertices.len(), 4 * 7);
    assert_eq!(surface.triangles.len(), 2 * 3 * 6);
    let area: f64 = surface
        .tri_rest_m
        .iter()
        .map(|chart| (chart[1] - chart[0]).cross(chart[2] - chart[0]) * 0.5)
        .sum();
    // Four loop centres span three wale intervals; six formed courses span 24 mm.
    assert!((area - 0.012 * 0.024).abs() < 1e-15);
    assert_eq!(surface.vertices.last().unwrap().course_distance_m, 0.024);
    assert_eq!(surface.boundary_loops.len(), 1);
}

#[test]
fn ribbed_shaped_panel_preserves_neck_opening_decreases_and_exact_parent_edges() {
    let doc = draft_shaped_panel(&panel_spec(), &gauge(), &MachineProfile::kniterate_7e()).unwrap();
    assert!(doc.stitches.iter().any(|node| node.bed == Bed::Back));
    assert!(
        doc.stitches
            .iter()
            .any(|node| node.kind == LoopKind::Decrease)
    );
    let surface = mesh_knit_doc(&doc, 1_000).unwrap();
    assert_eq!(surface.topology, KnitSurfaceTopology::Panel);
    let by_id: BTreeMap<_, _> = surface
        .vertices
        .iter()
        .enumerate()
        .map(|(index, vertex)| (vertex.stitch_id, index))
        .collect();
    assert_eq!(by_id.len(), doc.stitches.len());
    let edges = surface_edges(&surface);
    for node in &doc.stitches {
        for parent in &node.parents {
            let a = by_id[&node.id];
            let b = by_id[&parent.stitch_id];
            assert!(edges.contains(&(a.min(b), a.max(b))));
        }
    }
    let held_ids: BTreeSet<u32> = doc.metadata["held_neck_stitch_ids"]
        .split(',')
        .map(|text| text.parse().unwrap())
        .collect();
    let boundary_ids: BTreeSet<u32> = surface.boundary_loops[0]
        .iter()
        .map(|&vertex| surface.vertices[vertex].stitch_id)
        .collect();
    assert!(held_ids.is_subset(&boundary_ids));
    let neck_start: u32 = doc.metadata["shoulder_start_course"].parse().unwrap();
    for triangle in &surface.triangles {
        let source = triangle.map(|vertex| &surface.vertices[vertex]);
        if source.iter().all(|vertex| vertex.course >= neck_start - 1) {
            assert!(
                source.iter().all(|vertex| vertex.logical_wale < -1)
                    || source.iter().all(|vertex| vertex.logical_wale >= 1),
                "a face must never span the held-centre neck opening: {source:?}"
            );
        }
    }
    assert_eq!(surface.boundary_loops.len(), 1);
    assert_eq!(surface.pattern_sha256, doc.canonical_sha256().unwrap());
    for (triangle, ids) in surface.triangles.iter().zip(&surface.triangle_stitch_ids) {
        assert_eq!(
            *ids,
            triangle.map(|vertex| surface.vertices[vertex].stitch_id)
        );
    }
}

#[test]
fn full_and_half_gauge_tubes_preserve_real_rings_and_logical_side_through_rib() {
    for rib in [false, true] {
        let doc =
            draft_seamless_raglan_tube(&tube_spec(rib), &gauge(), &MachineProfile::kniterate_7e())
                .unwrap();
        let surface = mesh_knit_doc(&doc, 1_000).unwrap();
        assert_eq!(surface.topology, KnitSurfaceTopology::Tube);
        assert!(!surface.course_growth_upward);
        assert_eq!(surface.vertices.len(), doc.stitches.len());
        assert_eq!(surface.boundary_loops.len(), 2);
        let edges = surface_edges(&surface);
        // A connected annulus has Euler characteristic zero, with no cap and no seam crack.
        assert_eq!(
            surface.vertices.len() + surface.triangles.len(),
            edges.len()
        );
        for ring in &surface.boundary_loops {
            let courses: BTreeSet<_> = ring
                .iter()
                .map(|&vertex| surface.vertices[vertex].course)
                .collect();
            assert_eq!(courses.len(), 1);
            let course = *courses.first().unwrap();
            assert!(surface.course_range.contains(&course));
            assert_eq!(ring.len(), surface.vertices[ring[0]].course_stitches);
        }
        if rib {
            assert!(surface.vertices.iter().any(|vertex| {
                vertex.logical_side == Some(KnitSurfaceSide::Front)
                    && doc
                        .stitches
                        .iter()
                        .any(|node| node.id == vertex.stitch_id && node.bed == Bed::Back)
            }));
        }
    }
}

#[test]
fn source_iteration_order_does_not_change_surface_correspondence() {
    let doc = draft_shaped_panel(&panel_spec(), &gauge(), &MachineProfile::kniterate_7e()).unwrap();
    let first = mesh_knit_doc(&doc, 1_000).unwrap();
    let mut reordered = doc.clone();
    reordered.stitches.reverse();
    let second = mesh_knit_doc(&reordered, 1_000).unwrap();
    assert_eq!(first.vertices, second.vertices);
    assert_eq!(first.triangles, second.triangles);
    assert_eq!(first.tri_rest_m, second.tri_rest_m);
    assert_eq!(first.boundary_loops, second.boundary_loops);
    assert_eq!(second.pattern_sha256, reordered.canonical_sha256().unwrap());
}

#[test]
fn autoknit_requires_retained_annulus_and_explicit_course_orientation() {
    let mut doc =
        draft_seamless_raglan_tube(&tube_spec(false), &gauge(), &MachineProfile::kniterate_7e())
            .unwrap();
    doc.metadata.insert(
        "pathway".to_string(),
        "autoknit_annular_intrinsic_harmonic".to_string(),
    );
    assert_eq!(
        mesh_knit_doc(&doc, 1_000).unwrap_err().code,
        RefusalCode::UnqualifiedProcess
    );
    doc.metadata
        .insert("surface_topology".to_string(), "torso_tube".to_string());
    doc.metadata.insert(
        "course_growth_direction".to_string(),
        "unplaced".to_string(),
    );
    assert_eq!(
        mesh_knit_doc(&doc, 1_000).unwrap_err().code,
        RefusalCode::UnqualifiedProcess
    );
    doc.metadata
        .insert("course_growth_direction".to_string(), "upward".to_string());
    let surface = mesh_knit_doc(&doc, 1_000).unwrap();
    assert!(surface.course_growth_upward);
    assert_eq!(surface.topology, KnitSurfaceTopology::Tube);
    assert_eq!(surface.boundary_loops.len(), 2);
    doc.metadata.insert(
        "pathway".to_string(),
        "autoknit_idt_harmonic_3d".to_string(),
    );
    assert_eq!(
        mesh_knit_doc(&doc, 1_000).unwrap_err().code,
        RefusalCode::UnqualifiedProcess
    );
}

#[test]
fn missing_and_reused_parent_loops_refuse_instead_of_reconstructing_bounds() {
    let doc = draft_shaped_panel(&panel_spec(), &gauge(), &MachineProfile::kniterate_7e()).unwrap();
    let mut missing = doc.clone();
    let node = missing
        .stitches
        .iter_mut()
        .find(|node| node.course == 2 && node.wale == 0)
        .unwrap();
    node.parents.clear();
    assert_eq!(
        mesh_knit_doc(&missing, 1_000).unwrap_err().code,
        RefusalCode::UnqualifiedProcess
    );
    let mut reused = doc.clone();
    let old = reused
        .stitches
        .iter()
        .find(|node| node.course == 1 && node.wale == 1)
        .unwrap()
        .id;
    reused
        .stitches
        .iter_mut()
        .find(|node| node.course == 2 && node.wale == 0)
        .unwrap()
        .parents[0]
        .stitch_id = old;
    assert_eq!(
        mesh_knit_doc(&reused, 1_000).unwrap_err().code,
        RefusalCode::UnqualifiedProcess
    );
    // Packing two separately held shoulders next to each other on the final needles
    // does not create a previous-course fabric edge across the open neck.
    let mut bridged = doc.clone();
    let terminal = bridged
        .stitches
        .iter()
        .map(|node| node.course)
        .max()
        .unwrap();
    let mut terminal_nodes: Vec<_> = bridged
        .stitches
        .iter_mut()
        .filter(|node| node.course == terminal)
        .collect();
    terminal_nodes.sort_by_key(|node| node.wale);
    let first_wale = -(terminal_nodes.len() as i32 / 2);
    for (index, node) in terminal_nodes.iter_mut().enumerate() {
        node.wale = first_wale + index as i32;
    }
    let error = mesh_knit_doc(&bridged, 1_000).unwrap_err();
    assert_eq!(error.code, RefusalCode::UnqualifiedProcess);
    assert!(error.to_string().contains("parent arcs"));
    assert_eq!(
        mesh_knit_doc(&doc, doc.stitches.len() - 1)
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
}

#[test]
fn unsupported_short_rows_and_branch_joins_are_not_filled_with_rectangular_fabric() {
    let short =
        draft_short_row_neckline_panel(&panel_spec(), &gauge(), &MachineProfile::kniterate_7e(), 1)
            .unwrap();
    assert_eq!(
        mesh_knit_doc(&short, 1_000).unwrap_err().code,
        RefusalCode::UnqualifiedProcess
    );
    let branch = draft_branched_raglan_pullover(
        &BranchedPulloverSpec {
            torso: tube_spec(true),
            sleeve_cuff_girth_mm: 32.0,
            sleeve_upper_arm_girth_mm: 48.0,
            sleeve_length_mm: 24.0,
            cuff_rib_courses: 2,
            left_sleeve_carrier: CarrierId("2".to_string()),
            right_sleeve_carrier: CarrierId("3".to_string()),
        },
        &gauge(),
        &MachineProfile::kniterate_7e(),
    )
    .unwrap();
    assert_eq!(
        mesh_knit_doc(&branch, 2_000).unwrap_err().code,
        RefusalCode::UnqualifiedProcess
    );
}
