//! Synthetic source-qualified finite-stock paths; no measured leather or machine-cycle claim.
use std::collections::BTreeSet;

use ftx_core::{RefusalCode, Vec2};
use ftx_export::cutting::{
    job,
    procurement::finite,
    program::{self, Machine, Ordering, Policy},
    stock::{self, Request},
};
use ftx_pattern::{Pattern, material_catalog::ResolvedMaterial};
use serde_json::{Value, json};

fn material(id: &str, form: &str) -> Value {
    json!({"id":id,"allow_estimates":true,"record":{"id":id,"name":"Synthetic knife specimen","form":form,
        "properties":{"thickness":{"value":1,"unit":"mm","bounds":[0.8,1.2],"evidence":"estimate","source":"test","method":"Synthetic unloaded thickness"}},
        "sources":{"test":{"reference":"test:finite-knife-material","accessed_on":"2026-10-30","note":"Invented values, no measured textile claim"}}}})
}
fn piece(id: &str, w: f64, h: f64) -> Value {
    json!({"id":id,"cut":{"qty":1},"grain":[[0,0],[0,10]],
        "vertices":{"a":[0,0],"b":[w,0],"c":[w,h],"d":[0,h]},
        "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
            {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}]})
}
fn pattern(parts: Vec<Value>) -> Pattern {
    Pattern::new(
        json!({"schema":"fabric-pattern/1","units":"mm","meta":{"sa_mm":0},
        "fabric":{"material":material("hide","hide")},"pieces":parts,"seams":[]}),
    )
}
fn stock(id: &str, selection: Value, x: f64, y: f64, w: f64, h: f64) -> Value {
    json!({"id":id,"material":selection,"outline_mm":[[x,y],[x+w,y],[x+w,y+h],[x,y+h]],
        "grain_mm":[[0,0],[0,10]],"geometry_source":{"reference":"test:finite-knife-stock",
            "accessed_on":"2026-10-30","note":"Synthetic stock, defects and prior cuts in a declared mm frame"}})
}
fn request(stocks: Vec<Value>) -> Request {
    serde_json::from_value(
        json!({"schema":"ftx-finite-stock-request/1","stocks":stocks,
        "options":{"copies":1,"gap_mm":2,"boundary_margin_mm":2,"step_mm":5,"work_limit":2000000}}),
    )
    .unwrap()
}
fn machine() -> Machine {
    serde_json::from_value(
        json!({"id":"test-cutter","blade":"synthetic-knife","bed_mm":[160,160],
        "maximum_thickness_mm":2,"maximum_feed_mm_s":100,"travel_mm_s":100,
        "lift_lower_seconds":0.25,"start_mm":[0,0],"return_to_start":true}),
    )
    .unwrap()
}
fn policy<'a>(materials: impl Iterator<Item = &'a ResolvedMaterial>) -> Policy {
    let machine = machine();
    let machine_sha256 = program::machine_sha256(&machine).unwrap();
    let mut seen = BTreeSet::new();
    let qualifications = materials.filter(|m| seen.insert(m.sha256().to_owned())).map(|m| {
        json!({"id":format!("test:{}",m.material().id),"material_sha256":m.sha256(),"machine_sha256":machine_sha256,
            "thickness_mm":[0.5,2],"feed_mm_s":20,"directions":"all_in_plane","evidence":"estimate",
            "source":{"reference":"test:finite-knife-process","accessed_on":"2026-10-30","note":"Synthetic timing, not an observed cutting cycle"},
            "method":"Synthetic constant-rate knife phases","allow_estimates":true})
    }).collect::<Vec<_>>();
    serde_json::from_value(
        json!({"model":"qualified_cold_knife_paths/1","machine":machine,
        "qualifications":qualifications,"work_limit":10000,"geometry_work_limit":100000}),
    )
    .unwrap()
}
fn qualify(p: &Pattern, r: &mut Request) {
    let plan = r.build(p).unwrap();
    r.options.program = Some(policy(
        plan.markers()
            .iter()
            .filter(|m| !m.placements.is_empty())
            .map(|m| &m.material),
    ));
}
fn relock(policy: &mut Policy) {
    let sha = program::machine_sha256(&policy.machine).unwrap();
    for q in &mut policy.qualifications {
        q.machine_sha256.clone_from(&sha);
    }
}
fn value(x: &(impl serde::Serialize + ?Sized)) -> Value {
    serde_json::to_value(x).unwrap()
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}
fn inputs() -> (Pattern, Request) {
    let mut front = piece("front", 40., 60.);
    front["internal"] = json!([
        {"kind":"cut","closed":true,"pts":[[5,5],[15,5],[15,15],[5,15]]},
        {"kind":"cut","closed":false,"pts":[[20,5],[25,5]]}]);
    let p = pattern(vec![front, piece("pocket", 15., 15.)]);
    let mut hide = stock("hide-001", material("hide", "hide"), -150., 80., 110., 140.);
    hide["defects_mm"] = json!([[[-60, 190], [-50, 190], [-50, 200], [-60, 200]]]);
    hide["occupied_mm"] = json!([[[-145, 190], [-125, 190], [-125, 210], [-145, 210]]]);
    hide["occupied_basis"] = json!("test:prior-cut-receipt");
    (p, request(vec![hide]))
}

#[test]
fn complete_hide_paths_keep_geometry_and_cut_features_before_release_in_a_reported_bed_frame() {
    let (p, mut r) = inputs();
    let original = r.build(&p).unwrap();
    assert!(value(&original).get("program").is_none());
    assert!(value(&r)["options"].get("program").is_none());
    qualify(&p, &mut r);
    let plan = r.build(&p).unwrap();
    assert_eq!(value(&plan.markers()), value(&original.markers()));
    assert_eq!(value(&plan), value(&plan.request().build(&p).unwrap()));
    assert_eq!(value(&plan)["schema"], "ftx-finite-stock-plan/1");
    let report = plan.program().unwrap();
    assert_eq!(report.schema, "ftx-knife-cut-program/2");
    assert!(plan.program_refusal().is_none());
    assert!(!report.hardware_execution_authorized && !report.physical_cutting_validated);
    assert_eq!(report.markers.len(), 1);
    let row = &report.markers[0];
    let frame = row.finite_stock.as_ref().unwrap();
    let identity = &finite::identities(&plan).unwrap()[0];
    assert_eq!(row.marker, 0);
    assert_eq!(frame.stock_id, identity.stock_id);
    assert_eq!(frame.stock_sha256, identity.stock_sha256);
    assert_eq!(frame.marker_to_machine_offset_mm, Vec2::new(150., -80.));
    near(row.selected.cut_length_mm, 305.);
    near(row.selected.cutting_seconds, 15.25);
    near(row.selected.lift_lower_seconds, 1.);
    assert_eq!(row.selected.runs.len(), 4); // Stock/defect/prior-cut boundaries are not emitted.
    let mut exterior_seen = false;
    for run in &row.selected.runs {
        if run.path.internal_cut.is_none() {
            exterior_seen = true;
        } else {
            assert!(!exterior_seen);
        }
        let placement = &plan.markers()[row.marker].placements[run.path.placement];
        let points = run
            .path
            .internal_cut
            .map_or(&placement.cut_mm, |i| &placement.internal_cut_mm[i].0);
        let end = if run.closed {
            points[0]
        } else {
            *points.last().unwrap()
        };
        assert_eq!(
            run.cut_endpoints_mm,
            [
                points[0] + frame.marker_to_machine_offset_mm,
                end + frame.marker_to_machine_offset_mm
            ]
        );
        assert_eq!(run.instance, placement.instance);
        assert_eq!(run.copy, placement.copy);
    }
    for (&original, &marker) in r.stocks[0]
        .outline_mm
        .iter()
        .zip(&plan.markers()[0].outline_mm)
    {
        let a = frame.source_to_machine;
        let bed = Vec2::new(
            a[0][0] * original.x + a[0][1] * original.y + a[0][2],
            a[1][0] * original.x + a[1][1] * original.y + a[1][2],
        );
        near(
            (bed - marker - frame.marker_to_machine_offset_mm).norm(),
            0.,
        );
        assert!(bed.x >= 0. && bed.y >= 0. && bed.x <= 160. && bed.y <= 160.);
    }
}

#[test]
fn finite_drills_preserve_mirrored_copies_and_loaded_source_frames_before_exterior_release() {
    let mut pair = piece("pair", 20., 30.);
    pair["cut"]["qty"] = json!(2);
    pair["drills"] = json!([{"p":[5,10],"d":2}]);
    let p = pattern(vec![pair]);
    let mut r = request(vec![stock(
        "hide-001",
        material("hide", "hide"),
        -150.,
        80.,
        100.,
        80.,
    )]);
    r.options.copies = 2;
    let ordinary = r.build(&p).unwrap();
    qualify(&p, &mut r);
    let mut raw = value(r.options.program.as_ref().unwrap());
    raw["machine"]["drilling"] = json!({"model":"coaxial_cold_through_drill/1",
        "tool":"synthetic-2mm-bit","diameter_mm":2.,"maximum_thickness_mm":2.,
        "select_drill_seconds":0.6,"select_knife_seconds":0.4});
    raw["qualifications"][0]["drilling"] = json!({"id":"synthetic-hide-drill",
        "thickness_mm":[0.5,1.5],"plunge_seconds":0.3,"dwell_seconds":0.2,"retract_seconds":0.5,
        "source":{"reference":"test:finite-drill","accessed_on":"2026-11-01",
            "note":"Synthetic single-ply hide and independently declared fixed-bit cycle"},
        "method":"Invented coaxial plunge, dwell and retract applicability; no measured process claim",
        "evidence":"estimate","allow_estimates":true});
    raw["stock_loading"] = json!([{"marker":0,"lower_left_mm":[20.,10.]}]);
    r.options.program = Some(serde_json::from_value(raw).unwrap());
    relock(r.options.program.as_mut().unwrap());
    let plan = r.build(&p).unwrap();
    assert_eq!(value(plan.markers()), value(ordinary.markers()));
    assert_eq!(value(&plan), value(&plan.request().build(&p).unwrap()));
    let report = plan
        .program()
        .expect("qualified finite-stock drill program");
    let marker = &report.markers[0];
    let frame = marker.finite_stock.as_ref().unwrap();
    assert_eq!(frame.marker_to_machine_offset_mm, Vec2::new(170., -70.));
    assert_eq!(
        frame.stock_sha256,
        finite::identities(&ordinary).unwrap()[0].stock_sha256
    );
    let row = value(&marker.selected);
    let drills = row["drills"].as_array().unwrap();
    let placements = &plan.markers()[0].placements;
    assert_eq!(placements.len(), 4);
    assert_eq!(drills.len(), 4);
    let mut identities = BTreeSet::new();
    let mut position = Vec2::ZERO;
    let mut travel = 0.;
    for (i, placement) in placements.iter().enumerate() {
        let instance = p.instance(&placement.instance).unwrap();
        let local = Vec2::new(if instance.mirrored { -5. } else { 5. }, 10.);
        let t = placement.transform;
        let center = Vec2::new(
            t[0][0] * local.x + t[0][1] * local.y + t[0][2],
            t[1][0] * local.x + t[1][1] * local.y + t[1][2],
        ) + frame.marker_to_machine_offset_mm;
        let actual: Vec2 = serde_json::from_value(drills[i]["center_mm"].clone()).unwrap();
        near(actual.dist(center), 0.);
        assert_eq!(drills[i]["placement"], i);
        assert_eq!(drills[i]["drill"], 0);
        assert_eq!(drills[i]["instance"], placement.instance);
        assert_eq!(drills[i]["copy"], placement.copy);
        assert_eq!(drills[i]["diameter_mm"], 2.);
        identities.insert((instance.mirrored, placement.copy));
        travel += position.dist(center);
        position = center;
    }
    assert_eq!(identities.len(), 4);
    assert_eq!(marker.selected.runs[0].travel_mm[0], position);
    for placement in placements {
        let start = placement.cut_mm[0] + frame.marker_to_machine_offset_mm;
        travel += position.dist(start);
        position = start; // Closed exteriors; stock and defect outlines are not knife paths.
    }
    travel += position.norm();
    near(marker.selected.cut_length_mm, 400.);
    near(marker.selected.cutting_seconds, 20.);
    near(marker.selected.lift_lower_seconds, 1.);
    near(row["drilling_seconds"].as_f64().unwrap(), 4.);
    near(row["tool_change_seconds"].as_f64().unwrap(), 1.);
    near(marker.selected.travel_length_mm, travel);
    near(marker.selected.travel_seconds, travel / 100.);
    near(marker.selected.modeled_path_seconds, 26. + travel / 100.);
    assert!(!report.physical_cutting_validated && !report.hardware_execution_authorized);

    let mut raw = value(&r);
    raw["options"]["program"]["qualifications"][0]["drilling"] = Value::Null;
    let unavailable: Request = serde_json::from_value(raw).unwrap();
    let refused = unavailable.build(&p).unwrap();
    assert!(refused.program().is_none() && refused.program_refusal().is_some());
    assert_eq!(value(refused.markers()), value(plan.markers()));
}

#[test]
fn authored_finite_loading_translates_the_whole_inventory_and_preserves_source_identity() {
    let (p, mut r) = inputs();
    r.stocks[0].zones = serde_json::from_value(json!([{
        "id":"review-zone","outline_mm":[[-140,90],[-80,90],[-80,170],[-140,170]],
        "geometry_source":{"reference":"test:loaded-zone","accessed_on":"2026-10-31",
            "note":"Synthetic region retained with the whole stock"}
    }]))
    .unwrap();
    qualify(&p, &mut r);
    let origin = r.build(&p).unwrap();
    assert!(
        value(origin.program().unwrap())
            .get("stock_loading")
            .is_none()
    );
    r.options.program.as_mut().unwrap().stock_loading = vec![program::StockLoading {
        marker: 0,
        lower_left_mm: Vec2::new(20., 10.),
    }];
    let loaded = r.build(&p).unwrap();
    assert_eq!(value(loaded.markers()), value(origin.markers()));
    let marker = &loaded.program().unwrap().markers[0];
    let before = &origin.program().unwrap().markers[0];
    let loading = marker.loading.as_ref().unwrap();
    assert_eq!(loading.lower_left_mm, Vec2::new(20., 10.));
    assert_eq!(loading.marker_to_machine_offset_mm, Vec2::new(170., -70.));
    assert_eq!(loading.stock_bounds_mm, [20., 10., 130., 150.]);
    let frame = marker.finite_stock.as_ref().unwrap();
    assert_eq!(
        frame.marker_to_machine_offset_mm,
        loading.marker_to_machine_offset_mm
    );
    assert_eq!(
        frame.stock_sha256,
        before.finite_stock.as_ref().unwrap().stock_sha256
    );
    assert_ne!(marker.geometry_sha256, before.geometry_sha256);
    near(marker.selected.cut_length_mm, before.selected.cut_length_mm);
    for (run, original) in marker.selected.runs.iter().zip(&before.selected.runs) {
        assert_eq!(
            run.cut_endpoints_mm,
            original
                .cut_endpoints_mm
                .map(|point| point + Vec2::new(20., 10.))
        );
        assert_eq!(value(&run.path), value(&original.path));
    }
    let item = &loaded.markers()[0];
    for point in item
        .outline_mm
        .iter()
        .chain(item.defects_mm.iter().flatten())
        .chain(item.occupied_mm.iter().flatten())
        .chain(item.zones.iter().flat_map(|zone| &zone.outline_mm))
    {
        let point = *point + loading.marker_to_machine_offset_mm;
        assert!((20. ..=130.).contains(&point.x) && (10. ..=150.).contains(&point.y));
    }
    let source = r.stocks[0].outline_mm[0];
    let a = frame.source_to_machine;
    assert_eq!(
        Vec2::new(
            a[0][0] * source.x + a[0][1] * source.y + a[0][2],
            a[1][0] * source.x + a[1][1] * source.y + a[1][2]
        ),
        Vec2::new(20., 10.)
    );
    let svg = ftx_export::cutting::preview::stock_svg(&loaded, 0).unwrap();
    assert!(svg.contains("viewBox=\"0 0 160 160\""));
    assert!(svg.contains("translate(170 -70)"));
    assert!(svg.contains("id=\"stock-defects\"") && svg.contains("id=\"stock-occupied\""));
    assert!(svg.contains("review-zone") && svg.contains("data-phase=\"raised-return\""));
    // The allocated cut subset still fits, but the complete 110 mm stock would cross the bed.
    r.options.program.as_mut().unwrap().stock_loading[0]
        .lower_left_mm
        .x = 60.;
    let refused = r.build(&p).unwrap();
    assert!(refused.program().is_none());
    assert!(
        refused
            .program_refusal()
            .unwrap()
            .message
            .contains("complete finite stock")
    );
    assert_eq!(value(refused.markers()), value(loaded.markers()));
}

#[test]
fn fixtures_are_checked_in_the_loaded_machine_frame_for_translated_hides() {
    let (p, mut r) = inputs();
    qualify(&p, &mut r);
    let clear = r.build(&p).unwrap();
    let marker = &clear.program().unwrap().markers[0];
    let offset = marker
        .finite_stock
        .as_ref()
        .unwrap()
        .marker_to_machine_offset_mm;
    let placement = &clear.markers()[marker.marker].placements[0];
    let midpoint = (placement.cut_mm[0] + placement.cut_mm[1]) * 0.5 + offset;
    assert!(midpoint.x >= 0. && midpoint.y >= 0.);
    let policy = r.options.program.as_mut().unwrap();
    policy.machine.bed_mm = [500., 500.];
    policy.machine.fixture_clearance = Some(
        serde_json::from_value(json!({
            "model":"planar_disk_sweep/1","cutting_radius_mm":0.1,"raised_radius_mm":0.1,
            "fixtures":[{"id":"loaded-hide-clamp","outline_mm":[
                [midpoint.x-0.25,midpoint.y-0.25],[midpoint.x+0.25,midpoint.y-0.25],
                [midpoint.x+0.25,midpoint.y+0.25],[midpoint.x-0.25,midpoint.y+0.25]]}],
            "source":{"reference":"test:translated-fixture","accessed_on":"2026-10-30",
                "note":"Synthetic independent machine-frame geometry, not a measured fixture"},
            "method":"Known translated midpoint with a filled exclusion at both tool elevations",
            "evidence":"estimate","allow_estimates":true
        }))
        .unwrap(),
    );
    relock(policy);
    let blocked = r.build(&p).unwrap();
    assert!(blocked.program().is_none());
    assert!(
        blocked
            .program_refusal()
            .unwrap()
            .message
            .contains("cutting tool envelope")
    );
    assert_eq!(value(&blocked.markers()), value(&clear.markers()));
    r.options.program.as_mut().unwrap().stock_loading = vec![program::StockLoading {
        marker: 0,
        lower_left_mm: Vec2::new(200., 100.),
    }];
    let repositioned = r.build(&p).unwrap();
    assert!(
        repositioned.program().is_some(),
        "{:?}",
        repositioned.program_refusal()
    );
    assert_eq!(value(repositioned.markers()), value(clear.markers()));
    r.options.program.as_mut().unwrap().stock_loading.clear();
    // Original source coordinates are far from the loaded machine-frame cut, so confusing the
    // two frames would miss the obstruction above. Moving that same fixture away admits the cut.
    let policy = r.options.program.as_mut().unwrap();
    for point in &mut policy.machine.fixture_clearance.as_mut().unwrap().fixtures[0].outline_mm {
        point.x += 300.;
        point.y += 300.;
    }
    relock(policy);
    assert!(r.build(&p).unwrap().program().is_some());
}

#[test]
fn translated_hide_detours_and_return_are_visible_beyond_the_stock_in_the_full_bed() {
    let p = pattern(vec![piece("front", 40., 60.)]);
    let mut r = request(vec![stock(
        "hide-001",
        material("hide", "hide"),
        -150.,
        80.,
        110.,
        140.,
    )]);
    qualify(&p, &mut r);
    let original = r.build(&p).unwrap();
    let policy = r.options.program.as_mut().unwrap();
    policy.machine.start_mm = Vec2::new(150., 30.);
    policy.machine.fixture_clearance = Some(
        serde_json::from_value(json!({
            "model":"planar_disk_sweep/1","cutting_radius_mm":0.2,"raised_radius_mm":0.2,
            "fixtures":[{"id":"raised-wall","outline_mm":[[100,0],[110,0],[110,145],[100,145]]}],
            "source":{"reference":"test:finite-detour","accessed_on":"2026-10-30",
                "note":"Synthetic planar wall and tool envelopes"},
            "method":"Known loaded machine-frame geometry at both declared elevations",
            "evidence":"estimate","allow_estimates":true
        }))
        .unwrap(),
    );
    relock(policy);
    assert!(r.build(&p).unwrap().program().is_none());
    r.options.program.as_mut().unwrap().travel_routing = Some(program::TravelRouting {
        model: "fixture_box_visibility/1".into(),
        waypoint_offset_mm: 1.,
    });
    let plan = r.build(&p).unwrap();
    assert_eq!(value(&plan.markers()), value(&original.markers()));
    let report = plan.program().unwrap();
    let marker = &report.markers[0];
    assert_eq!(
        marker
            .finite_stock
            .as_ref()
            .unwrap()
            .marker_to_machine_offset_mm,
        Vec2::new(150., -80.)
    );
    assert_eq!(
        marker.finite_stock.as_ref().unwrap().stock_sha256,
        original.program().unwrap().markers[0]
            .finite_stock
            .as_ref()
            .unwrap()
            .stock_sha256
    );
    assert!(
        marker.selected.runs[0]
            .travel_waypoints_mm
            .iter()
            .all(|p| p.y > 140. && p.y < 160.)
    );
    assert_eq!(marker.selected.runs[0].travel_waypoints_mm.len(), 2);
    assert_eq!(marker.selected.return_waypoints_mm.len(), 2);
    near(
        marker.selected.checked_travel_length().unwrap(),
        marker.selected.travel_length_mm,
    );
    let svg = ftx_export::cutting::preview::stock_svg(&plan, 0).unwrap();
    assert!(svg.contains("viewBox=\"0 0 160 160\""));
    assert!(svg.contains("translate(0 160) scale(1 -1)"));
    assert!(svg.contains("translate(150 -80)"));
    assert!(svg.contains("L111.2 146.2 L98.8 146.2"));
    assert!(svg.contains("data-phase=\"raised-return\""));
    assert_eq!(value(&plan), value(&plan.request().build(&p).unwrap()));

    // Add exact-stop dynamics and a further authored loading translation. The physical-stock
    // identity and nesting stay fixed while every raised detour chord gets its own acceleration.
    let profile = json!({"model":"trapezoidal_exact_stop/1",
        "cutting_acceleration_mm_s2":40.,"travel_acceleration_mm_s2":25.});
    let policy = r.options.program.as_mut().unwrap();
    let mut machine = value(&policy.machine);
    machine["motion_profile"] = profile.clone();
    policy.machine = serde_json::from_value(machine).unwrap();
    relock(policy);
    policy.stock_loading = vec![program::StockLoading {
        marker: 0,
        lower_left_mm: Vec2::new(0., 10.),
    }];
    let accelerated = r.build(&p).unwrap();
    assert_eq!(value(accelerated.markers()), value(plan.markers()));
    assert_eq!(
        value(&accelerated),
        value(&accelerated.request().build(&p).unwrap())
    );
    let report = accelerated.program().unwrap();
    assert_eq!(value(report)["motion_profile"], profile);
    let moved = &report.markers[0];
    assert_eq!(
        moved.finite_stock.as_ref().unwrap().stock_sha256,
        marker.finite_stock.as_ref().unwrap().stock_sha256
    );
    assert_eq!(
        moved
            .finite_stock
            .as_ref()
            .unwrap()
            .marker_to_machine_offset_mm,
        Vec2::new(150., -70.)
    );
    let route = &moved.selected;
    let run = &route.runs[0];
    near(run.cut_endpoints_mm[0].dist(Vec2::new(42., 12.)), 0.);
    assert_eq!(run.cut_endpoints_mm[0], run.cut_endpoints_mm[1]);
    let corners = [Vec2::new(111.2, 146.2), Vec2::new(98.8, 146.2)];
    assert_eq!(run.travel_waypoints_mm.len(), 2);
    assert_eq!(route.return_waypoints_mm.len(), 2);
    for (actual, expected) in run.travel_waypoints_mm.iter().zip(corners) {
        near(actual.dist(expected), 0.);
    }
    for (actual, expected) in route
        .return_waypoints_mm
        .iter()
        .zip(corners.into_iter().rev())
    {
        near(actual.dist(expected), 0.);
    }
    // Each of these three known chords is below v²/a = 400 mm. Returning repeats
    // all three rest-to-rest moves, rather than accelerating just once over their sum.
    let one_way = [
        Vec2::new(150., 30.).dist(corners[0]),
        12.4,
        corners[1].dist(Vec2::new(42., 12.)),
    ];
    let travel = 4.
        * one_way
            .into_iter()
            .map(|length| (length / 25.).sqrt())
            .sum::<f64>();
    near(route.cut_length_mm, 200.);
    near(route.cutting_seconds, 12.);
    near(route.travel_seconds, travel);
    near(route.modeled_path_seconds, 12.25 + travel);
    assert!(!report.physical_cutting_validated && !report.hardware_execution_authorized);
}

#[test]
fn stock_identity_defects_and_rotated_source_frame_remain_bound_to_the_program() {
    let (p, mut r) = inputs();
    qualify(&p, &mut r);
    let original = r.build(&p).unwrap();
    let before = &original.program().unwrap().markers[0];
    r.stocks[0]
        .geometry_source
        .note
        .push_str(" revised acquisition");
    let changed_source = r.build(&p).unwrap();
    let changed = &changed_source.program().unwrap().markers[0];
    assert_ne!(
        before.finite_stock.as_ref().unwrap().stock_sha256,
        changed.finite_stock.as_ref().unwrap().stock_sha256
    );
    assert_eq!(before.geometry_sha256, changed.geometry_sha256);
    for point in &mut r.stocks[0].defects_mm[0] {
        point.x += 1.;
    }
    let changed_defect = r.build(&p).unwrap();
    let changed = &changed_defect.program().unwrap().markers[0];
    assert_ne!(before.geometry_sha256, changed.geometry_sha256);
    assert_eq!(
        value(&original.markers()[0].placements),
        value(&changed_defect.markers()[0].placements)
    );
    r.stocks[0].grain_mm = [Vec2::ZERO, Vec2::new(10., 0.)];
    let turned = r.build(&p).unwrap();
    let frame = turned.program().unwrap().markers[0]
        .finite_stock
        .as_ref()
        .unwrap();
    let a = frame.source_to_machine;
    near(a[0][0] * a[1][1] - a[0][1] * a[1][0], 1.);
    for (&original, &marker) in r.stocks[0]
        .outline_mm
        .iter()
        .zip(&turned.markers()[0].outline_mm)
    {
        let bed = Vec2::new(
            a[0][0] * original.x + a[0][1] * original.y + a[0][2],
            a[1][0] * original.x + a[1][1] * original.y + a[1][2],
        );
        near(
            (bed - marker - frame.marker_to_machine_offset_mm).norm(),
            0.,
        );
    }
    near(
        turned.program().unwrap().markers[0].selected.cut_length_mm,
        305.,
    );
}

#[test]
fn the_entire_hide_must_fit_even_when_every_new_cut_fits_a_smaller_bed() {
    let (p, mut r) = inputs();
    qualify(&p, &mut r);
    let original = r.build(&p).unwrap();
    let program = r.options.program.as_mut().unwrap();
    program.machine.bed_mm[0] = 80.;
    relock(program);
    let result = r.build(&p).unwrap();
    assert_eq!(value(&original.markers()), value(&result.markers()));
    assert!(
        result.markers()[0]
            .placements
            .iter()
            .all(|p| p.bounds_mm[2] + 150. < 80.)
    );
    assert!(result.program().is_none());
    assert!(
        result
            .program_refusal()
            .unwrap()
            .message
            .contains("complete finite stock")
    );
}

#[test]
fn unused_stock_needs_no_process_and_both_work_limits_are_shared_by_all_loaded_items() {
    let p = pattern(vec![piece("front", 40., 60.)]);
    let mut r = request(vec![
        stock(
            "unused-wide-other-material",
            material("unqualified", "hide"),
            0.,
            0.,
            1000.,
            1000.,
        ),
        stock("first-used", material("hide", "hide"), 0., 0., 50., 70.),
        stock("second-used", material("hide", "hide"), 0., 0., 50., 70.),
    ]);
    r.options.copies = 2;
    qualify(&p, &mut r);
    let q = r.options.program.as_mut().unwrap();
    q.ordering = Ordering::NearestPath;
    q.work_limit = 1;
    let plan = r.build(&p).unwrap();
    let report = plan.program().unwrap();
    assert_eq!(
        report.markers.iter().map(|m| m.marker).collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(report.search_work, 1);
    assert!(report.budget_exhausted);
    for row in &report.markers {
        assert_eq!(value(&row.selected), value(&row.baseline));
        assert_eq!(row.selected.runs.len(), 1);
    }
    r.options.program.as_mut().unwrap().stock_loading = vec![
        program::StockLoading {
            marker: 0,
            lower_left_mm: Vec2::new(100., 100.),
        },
        program::StockLoading {
            marker: 2,
            lower_left_mm: Vec2::new(20., 10.),
        },
    ];
    let moved = r.build(&p).unwrap();
    let moved_report = moved.program().unwrap();
    assert_eq!(
        moved_report
            .markers
            .iter()
            .map(|m| m.marker)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    assert!(moved_report.markers[0].loading.is_none());
    assert!(moved_report.markers[1].loading.is_some());
    assert_eq!(value(&moved_report.markers[0]), value(&report.markers[0]));
    assert!(
        !ftx_export::cutting::preview::stock_svg(&moved, 0)
            .unwrap()
            .contains("machine-bed")
    );
    assert_eq!(moved_report.stock_loading.len(), 2);
    r.options.program.as_mut().unwrap().geometry_work_limit = 1;
    let refused = r.build(&p).unwrap();
    assert!(refused.program().is_none());
    assert_eq!(
        refused.program_refusal().unwrap().code,
        RefusalCode::BudgetExceeded
    );
    assert_eq!(value(&plan.markers()), value(&refused.markers()));
}

fn prices(plan: &stock::Plan) -> finite::Request {
    let id = &finite::identities(plan).unwrap()[0];
    serde_json::from_value(json!({"schema":"ftx-finite-stock-pricing/1",
        "quotes":{"schema":"ftx-textile-quotes/1","quotes":[{
            "offer_id":"test:whole-hide","material_id":"hide","material_sha256":id.material_sha256,
            "supplier":"Synthetic supplier","sku":id.stock_id,"price":7,"quantity":id.gross_area_m2,
            "unit":"square_meter","minimum_quantity":id.gross_area_m2,"order_increment":id.gross_area_m2,
            "available_quantity":id.gross_area_m2,"available":true,"currency":"USD","region":"test",
            "quality":"synthetic","terms":"no-shipping","observed_on":"2026-10-30",
            "source":"test:whole-hide-price-not-live","source_sha256":"0".repeat(64)}]},
        "bindings":[{"stock_id":id.stock_id,"stock_sha256":id.stock_sha256,"offer_ids":["test:whole-hide"]}],
        "market":{"currency":"USD","region":"test","quality":"synthetic","terms":"no-shipping",
            "as_of":"2026-10-30","max_age_days":0,"require_observed_quantity":true}})).unwrap()
}

#[test]
fn unavailable_process_preserves_exact_item_procurement_and_missing_values() {
    let (p, mut r) = inputs();
    qualify(&p, &mut r);
    let original = r.build(&p).unwrap();
    let pricing = prices(&original);
    let before = finite::evaluate(&original, &pricing).unwrap();
    r.options.program.as_mut().unwrap().qualifications[0].material_sha256 = "f".repeat(64);
    let refused = r.build(&p).unwrap();
    let after = finite::evaluate(&refused, &pricing).unwrap();
    assert!(refused.program().is_none() && refused.program_refusal().is_some());
    assert_eq!(value(&refused.markers()), value(&original.markers()));
    assert!(refused.markers()[0].stock_mass_kg.is_none());
    assert!(refused.markers()[0].mass_unavailable.is_some());
    assert_eq!(value(&before.stocks), value(&after.stocks));
    assert_eq!(before.least_observed_independent_order_total, Some(7.));
    assert_eq!(
        before.least_observed_independent_order_total,
        after.least_observed_independent_order_total
    );
    assert!(after.complete_offer_coverage && after.complete_observed_quantity_coverage);
    assert!(!after.execution_authorized);
}

#[test]
fn pressure_conditioned_measurements_need_explicit_unloaded_full_range_clearance() {
    let (p, mut r) = inputs();
    let mut raw = p.raw().clone();
    raw["fabric"]["material"]["record"]["properties"]["thickness"]["conditions"] =
        json!({"pressure_pa":"1000"});
    let p = Pattern::new(raw.clone());
    r.stocks[0].material = serde_json::from_value(raw["fabric"]["material"].clone()).unwrap();
    qualify(&p, &mut r);
    let refused = r.build(&p).unwrap();
    assert!(refused.program().is_none() && refused.program_refusal().is_some());
    raw["fabric"]["material"]["record"]["properties"]["unloaded_thickness"] = json!({
        "value":1.8,"unit":"mm","bounds":[1.6,1.9],"evidence":"estimate","source":"test",
        "method":"Synthetic explicitly unloaded coupon","conditions":{"thickness_state":"unloaded",
            "specimen_id":"test-hide-lot","conditioning":"synthetic dry ambient"}});
    let p = Pattern::new(raw.clone());
    r.stocks[0].material = serde_json::from_value(raw["fabric"]["material"].clone()).unwrap();
    r.options.program = None;
    qualify(&p, &mut r);
    let admitted = r.build(&p).unwrap();
    let range = admitted.program().unwrap().markers[0].thickness_mm;
    near(range[0], 1.6);
    near(range[1], 1.9);
    let program = r.options.program.as_mut().unwrap();
    program.machine.maximum_thickness_mm = 1.85;
    relock(program);
    let refused = r.build(&p).unwrap();
    assert!(refused.program().is_none());
    assert!(
        refused
            .program_refusal()
            .unwrap()
            .message
            .contains("full source thickness range")
    );
    assert_eq!(value(&admitted.markers()), value(&refused.markers()));
}

#[test]
fn residual_stock_replans_new_paths_without_cutting_previously_consumed_boundaries() {
    let p = pattern(vec![piece("front", 20., 20.)]);
    let mut r = request(vec![stock(
        "remnant",
        material("hide", "hide"),
        -60.,
        -40.,
        100.,
        80.,
    )]);
    qualify(&p, &mut r);
    let first = r.build(&p).unwrap();
    let second = first.residual_request().unwrap().build(&p).unwrap();
    let a = &first.program().unwrap().markers[0];
    let b = &second.program().unwrap().markers[0];
    assert_eq!(
        a.finite_stock.as_ref().unwrap().stock_id,
        b.finite_stock.as_ref().unwrap().stock_id
    );
    assert_ne!(
        a.finite_stock.as_ref().unwrap().stock_sha256,
        b.finite_stock.as_ref().unwrap().stock_sha256
    );
    assert_ne!(a.geometry_sha256, b.geometry_sha256);
    assert_eq!(second.markers()[0].occupied_mm.len(), 1);
    assert_eq!(b.selected.runs.len(), 1);
    near(a.selected.cut_length_mm, 80.);
    near(b.selected.cut_length_mm, 80.);
    assert_ne!(
        value(&first.markers()[0].placements),
        value(&second.markers()[0].placements)
    );
}

#[test]
fn unsupported_drills_and_nested_cutouts_refuse_only_the_process_after_translation() {
    for features in [
        json!({"drills":[{"p":[10,10],"d":2}]}),
        json!({"internal":[
            {"kind":"cut","closed":true,"pts":[[5,5],[30,5],[30,30],[5,30]]},
            {"kind":"cut","closed":true,"pts":[[10,10],[20,10],[20,20],[10,20]]}]}),
    ] {
        let mut front = piece("front", 40., 60.);
        for (k, v) in features.as_object().unwrap() {
            front[k] = v.clone();
        }
        let p = pattern(vec![front]);
        let mut r = request(vec![stock(
            "hide",
            material("hide", "hide"),
            -150.,
            80.,
            110.,
            140.,
        )]);
        let ordinary = r.build(&p).unwrap();
        qualify(&p, &mut r);
        let refused = r.build(&p).unwrap();
        assert!(refused.program().is_none());
        assert!(refused.program_refusal().is_some());
        assert_eq!(value(&ordinary.markers()), value(&refused.markers()));
    }
}

#[test]
fn mixed_roll_lining_and_finite_hide_share_program_engine_and_independent_refusals() {
    let mut lining_material = material("lining", "woven");
    lining_material["record"]["properties"]["width"] = json!({"value":0.12,"unit":"m",
        "evidence":"estimate","source":"test","method":"Synthetic roll width"});
    let mut lining = piece("lining", 20., 20.);
    lining["fabric"] = json!({"material":lining_material});
    let p = pattern(vec![piece("front", 20., 30.), lining]);
    let finite_request = request(vec![stock(
        "hide",
        material("hide", "hide"),
        -40.,
        20.,
        100.,
        100.,
    )]);
    let mut request: job::Request = serde_json::from_value(json!({"schema":"ftx-cutting-job/1",
        "routes":[{"piece":"front","stock":"finite_stock"},{"piece":"lining","stock":"roll"}],
        "roll":{"copies":1,"gap_mm":2,"edge_margin_mm":2,"end_margin_mm":2},
        "finite_stock":finite_request}))
    .unwrap();
    let baseline = request.build(&p).unwrap();
    request.roll.as_mut().unwrap().program = Some(policy(
        baseline
            .roll
            .as_ref()
            .unwrap()
            .rolls
            .iter()
            .map(|m| &m.material),
    ));
    request.finite_stock.as_mut().unwrap().options.program = Some(policy(
        baseline
            .finite_stock
            .as_ref()
            .unwrap()
            .markers()
            .iter()
            .map(|m| &m.material),
    ));
    let plan = request.build(&p).unwrap();
    assert!(plan.complete_inventory);
    assert_eq!(plan.inventory, baseline.inventory);
    assert_eq!(
        plan.roll.as_ref().unwrap().program.as_ref().unwrap().schema,
        "ftx-knife-cut-program/1"
    );
    assert_eq!(
        plan.finite_stock
            .as_ref()
            .unwrap()
            .program()
            .unwrap()
            .schema,
        "ftx-knife-cut-program/2"
    );
    let replay: job::Request = serde_json::from_str(&plan.replay_request_json).unwrap();
    assert_eq!(
        value(&plan),
        value(
            &replay
                .build(&Pattern::from_json(&plan.pattern_json).unwrap())
                .unwrap()
        )
    );
    let mut raw = p.raw().clone();
    raw["pieces"][0]["drills"] = json!([{"p":[10,10],"d":2}]);
    let refused = request.build(&Pattern::new(raw)).unwrap();
    assert!(refused.complete_inventory);
    assert_eq!(refused.inventory, plan.inventory);
    assert!(
        refused
            .finite_stock
            .as_ref()
            .unwrap()
            .program_refusal()
            .is_some()
    );
    assert!(refused.finite_stock.as_ref().unwrap().program().is_none());
    assert_eq!(
        value(&refused.roll.as_ref().unwrap().program),
        value(&plan.roll.as_ref().unwrap().program)
    );
}
