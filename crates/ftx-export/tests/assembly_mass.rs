//! Known geometry/source answers for the full declared cut inventory, not measured garments.
use ftx_core::polyline::signed_area;
use ftx_export::{
    Ctx, ExportOptions, Source,
    assembly_mass::Request,
    cutter::cutter_ring,
    cutting::{CutOptions, CutPlan},
};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

fn pattern() -> Value {
    let mut p: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    p["fabric"]["material"]["record"]["properties"]["areal_density"] = json!({"value":200.,"unit":"g/m2","bounds":[180.,220.],"source":"test","evidence":"estimate","method":"Synthetic mass test only"});
    p
}
fn request() -> Request {
    serde_json::from_value(
        json!({"schema":"ftx-assembly-mass/1","inventory_complete":true,"extras":[]}),
    )
    .unwrap()
}
fn extra() -> Value {
    json!({"id":"all-thread-and-closures","mass":{"value":0.03,"unit":"kg","bounds":[0.028,0.032],"evidence":"estimate","source":"test","method":"Synthetic complete retained group, not measured"},
        "sources":{"test":{"reference":"test:extras","accessed_on":"2026-10-16","note":"Synthetic software test"}},"allow_estimates":true})
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-12, "{a} != {b}");
}

#[test]
fn all_cut_pieces_plus_sourced_extras_not_stock_or_simulation_mass() {
    let mut raw = pattern();
    raw["pieces"][0]["sim"] = json!(false);
    raw["pieces"][1]["sim_scale"] = json!([100., 100.]);
    raw["pieces"][1].as_object_mut().unwrap().remove("grain");
    let p = Pattern::new(raw.clone());
    let mut r = request();
    r.extras.push(serde_json::from_value(extra()).unwrap());
    let result = r.evaluate(&p).unwrap();
    // Two 20x100 sew pieces with 10 mm allowances => two 40x120 mm physical cut pieces.
    close(result.sheet_mass.kg.unwrap(), 2. * 0.04 * 0.12 * 0.2);
    close(result.assembly_mass.kg.unwrap(), 0.00192 + 0.03);
    let b = result.assembly_mass.reported_bounds_kg.unwrap();
    close(b[0], 0.0096 * 0.18 + 0.028);
    close(b[1], 0.0096 * 0.22 + 0.032);
    assert_eq!(result.pieces.len(), 2);
    assert_eq!(p.raw(), &raw);
    assert_eq!(
        result.pieces[0]
            .material
            .as_ref()
            .unwrap()
            .material()
            .properties["areal_density"]
            .unit,
        "kg/m2"
    );
    // No width, grain, modulus or body was needed to count these physical pieces.
    let cut: CutOptions = serde_json::from_value(
        json!({"copies":1,"gap_mm":0.,"edge_margin_mm":0.,"end_margin_mm":0.}),
    )
    .unwrap();
    assert!(CutPlan::build(&p, &cut).is_err());
    assert_eq!(
        serde_json::to_value(r.evaluate(&p).unwrap()).unwrap(),
        serde_json::to_value(result).unwrap()
    );
}

#[test]
fn attached_groups_share_accounting_without_duplicate_mass_or_source_authorities() {
    let mut raw = pattern();
    raw["attached_mass"] = json!({"schema":"ftx-attached-mass/1","groups":[{
        "extra":extra(),"targets":[{"instance":"front","vertex":"a","fraction":0.5},
        {"instance":"back","vertex":"a","fraction":0.5}]}]});
    let p = Pattern::new(raw.clone());
    let mut input = request();
    let a = input.evaluate(&p).unwrap();
    close(a.assembly_mass.kg.unwrap(), a.sheet_mass.kg.unwrap() + 0.03);
    assert_eq!(a.extras.len(), 1);
    assert!(a.attached_inventory.is_some());
    input.extras.push(serde_json::from_value(extra()).unwrap());
    let b = input.evaluate(&p).unwrap();
    assert_eq!(a.assembly_mass.kg, b.assembly_mass.kg);
    assert_eq!(
        a.assembly_mass.reported_bounds_kg,
        b.assembly_mass.reported_bounds_kg
    );
    assert_eq!(b.extras.len(), 1);
    input.extras[0].mass.as_mut().unwrap().value = 0.031;
    assert!(
        input
            .evaluate(&p)
            .unwrap_err()
            .message
            .contains("conflicts")
    );
    assert_eq!(p.raw(), &raw);
    let plain = request().evaluate(&Pattern::new(pattern())).unwrap();
    assert!(
        serde_json::to_value(plain)
            .unwrap()
            .get("attached_inventory")
            .is_none()
    );
}

#[test]
fn unknown_attached_mass_remains_unknown_and_inventory_completeness_is_not_inferred() {
    let mut raw = pattern();
    let mut group = extra();
    group["mass"] = Value::Null;
    raw["attached_mass"] = json!({"schema":"ftx-attached-mass/1","groups":[{
        "extra":group,"targets":[{"instance":"front","vertex":"a","fraction":1.0}]}]});
    let a = request().evaluate(&Pattern::new(raw.clone())).unwrap();
    assert!(a.sheet_mass.kg.is_some());
    assert!(a.assembly_mass.kg.is_none());
    assert!(a.extras[0].1.kg.is_none());
    raw["attached_mass"]["groups"][0]["extra"] = extra();
    let mut incomplete = request();
    incomplete.inventory_complete = false;
    let b = incomplete.evaluate(&Pattern::new(raw)).unwrap();
    close(b.extras[0].1.kg.unwrap(), 0.03);
    assert!(b.assembly_mass.kg.is_none());
}

#[test]
fn mirror_fold_and_notches_use_the_actual_physical_export_geometry() {
    let mut raw = pattern();
    raw["seams"] = json!([]);
    raw["pieces"][0]["cut"] = json!({"qty":2});
    raw["pieces"][0]["notches"] = json!([{"id":"n","edge":"right","s":0.5}]);
    raw["pieces"][1]["cut"] = json!({"qty":1,"on_fold":"left"});
    let p = Pattern::new(raw);
    let r = request().evaluate(&p).unwrap();
    assert_eq!(r.pieces.len(), 3);
    let ctx = Ctx::new(&p, ExportOptions::default()).unwrap();
    for (instance, mass) in p.instances.iter().zip(&r.pieces) {
        let g = ctx.geom(Source::Instance(instance), 6.).unwrap();
        close(
            mass.external_cut_area_m2,
            signed_area(&cutter_ring(&g).unwrap()).abs() * 1e-6,
        );
        close(mass.mass.kg.unwrap(), mass.retained_area_m2 * 0.2);
    }
    close(r.pieces[0].mass.kg.unwrap(), r.pieces[1].mass.kg.unwrap());
    assert!(r.pieces[0].external_cut_area_m2 > 0.0048); // outward notch tabs are retained
    assert!(r.pieces[2].external_cut_area_m2 > 0.0048); // folded source is physically unfolded
}

#[test]
fn closed_internal_cutouts_are_removed_once_and_slits_remove_no_invented_kerf() {
    let mut raw = pattern();
    raw["pieces"][0]["internal"] = json!([
        {"kind":"cut","closed":true,"pts":[[1,1],[11,1],[11,21],[1,21],[1,1]]},
        {"kind":"cut","closed":false,"pts":[[1,30],[11,30]]}]);
    let r = request().evaluate(&Pattern::new(raw)).unwrap();
    close(r.pieces[0].removed_cutout_area_m2, 0.0002);
    close(r.sheet_mass.kg.unwrap(), (0.0096 - 0.0002) * 0.2);
}

#[test]
fn non_simple_touching_nested_overlapping_or_outside_cutouts_refuse() {
    let a = json!({"kind":"cut","closed":true,"pts":[[0,0],[15,0],[15,20],[0,20]]});
    for pts in [
        json!([[5, 5], [10, 5], [10, 10], [5, 10]]),     // nested
        json!([[15, 0], [18, 0], [18, 10], [15, 10]]),   // touching
        json!([[10, 10], [18, 10], [18, 25], [10, 25]]), // overlapping
        json!([[80, 0], [90, 0], [90, 10], [80, 10]]),   // outside
        json!([[0, 30], [10, 40], [0, 40], [10, 30]]),
    ] {
        // self crossing
        let mut raw = pattern();
        raw["pieces"][0]["internal"] = json!([a,{"kind":"cut","closed":true,"pts":pts}]);
        assert!(request().evaluate(&Pattern::new(raw)).is_err());
    }
}

#[test]
fn incomplete_inventory_unknown_density_or_unadmitted_extra_never_becomes_a_total() {
    let mut raw = pattern();
    let mut req = request();
    req.inventory_complete = false;
    let r = req.evaluate(&Pattern::new(raw.clone())).unwrap();
    assert!(r.sheet_mass.kg.is_some());
    assert!(r.assembly_mass.kg.is_none());
    req.inventory_complete = true;
    req.extras.push(serde_json::from_value(extra()).unwrap());
    req.extras[0].allow_estimates = false;
    let r = req.evaluate(&Pattern::new(raw.clone())).unwrap();
    assert!(r.sheet_mass.kg.is_some());
    assert!(r.assembly_mass.kg.is_none());
    assert!(
        r.extras[0]
            .1
            .unavailable
            .as_ref()
            .unwrap()
            .message
            .contains("consent")
    );
    req.extras[0].mass = None;
    assert!(
        req.evaluate(&Pattern::new(raw.clone()))
            .unwrap()
            .assembly_mass
            .kg
            .is_none()
    );
    raw["pieces"][1]["fabric"] = raw["fabric"].clone();
    raw["pieces"][1]["fabric"]["material"]["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("areal_density");
    let r = request().evaluate(&Pattern::new(raw)).unwrap();
    assert!(r.pieces[0].mass.kg.is_some());
    assert!(r.pieces[1].mass.kg.is_none());
    assert!(r.sheet_mass.kg.is_none());
    assert!(r.assembly_mass.kg.is_none());
}

#[test]
fn missing_bounds_do_not_become_zero_width_and_source_policies_remain_independent() {
    let mut raw = pattern();
    raw["fabric"]["material"]["record"]["properties"]["areal_density"]
        .as_object_mut()
        .unwrap()
        .remove("bounds");
    let r = request().evaluate(&Pattern::new(raw.clone())).unwrap();
    assert!(r.assembly_mass.kg.is_some());
    assert!(r.assembly_mass.reported_bounds_kg.is_none());
    raw["fabric"]["material"]["allow_estimates"] = json!(false);
    let mut req = request();
    req.extras.push(serde_json::from_value(extra()).unwrap());
    let r = req.evaluate(&Pattern::new(raw)).unwrap();
    assert!(r.extras[0].1.kg.is_some());
    assert!(r.sheet_mass.kg.is_none());
    assert!(r.assembly_mass.kg.is_none()); // group consent cannot override sheet policy
}

#[test]
fn geometry_inventory_and_numeric_refusals_do_not_invent_physical_assumptions() {
    let mut req = request();
    req.extras = vec![serde_json::from_value(extra()).unwrap(); 2];
    assert!(req.validate().is_err());
    req = request();
    req.seam_allowance_mm = Some(f64::NAN);
    assert!(req.validate().is_err());
    for edit in [json!({"scale_status":"UNSCALED"}), json!({"sa_mm":-1})] {
        let mut raw = pattern();
        raw["meta"] = edit;
        assert!(request().evaluate(&Pattern::new(raw)).is_err());
    }
    let mut raw = pattern();
    raw["fabric"]["layers"] = json!([]);
    assert!(request().evaluate(&Pattern::new(raw)).is_err());
    let mut raw = pattern();
    raw["fabric"]["material"]["record"]["properties"]["areal_density"]["value"] = json!(-0.2);
    assert!(
        request()
            .evaluate(&Pattern::new(raw))
            .unwrap()
            .assembly_mass
            .kg
            .is_none()
    );
    let mut req = request();
    let mut e = extra();
    e["mass"]["bounds"] = json!([-0.01, 0.1]);
    req.extras.push(serde_json::from_value(e).unwrap());
    assert!(
        req.evaluate(&Pattern::new(pattern()))
            .unwrap()
            .assembly_mass
            .kg
            .is_none()
    );
}

#[test]
fn piece_specific_hide_density_changes_mass_not_geometry_and_bad_cut_points_refuse() {
    let raw = pattern();
    let base = request().evaluate(&Pattern::new(raw.clone())).unwrap();
    let mut changed = raw.clone();
    changed["pieces"][1]["fabric"] = changed["fabric"].clone();
    changed["pieces"][1]["fabric"]["material"]["record"]["form"] = json!("hide");
    changed["pieces"][1]["fabric"]["material"]["record"]["properties"]["areal_density"] = json!({"value":0.6,"unit":"kg/m2","evidence":"estimate","source":"test","method":"Synthetic hide for software test"});
    let second = request().evaluate(&Pattern::new(changed)).unwrap();
    assert_eq!(
        second.pieces[1].retained_area_m2,
        base.pieces[1].retained_area_m2
    );
    close(
        second.pieces[1].mass.kg.unwrap(),
        3. * base.pieces[1].mass.kg.unwrap(),
    );
    assert_eq!(second.pieces[0].mass.kg, base.pieces[0].mass.kg);
    for pts in [
        json!([[0, 0], ["bad", 5], [5, 10], [0, 10]]),
        json!([[0, 0], [5, 0, 1], [5, 10], [0, 10]]),
    ] {
        let mut bad = raw.clone();
        bad["pieces"][0]["internal"] = json!([{"kind":"cut","closed":true,"pts":pts}]);
        assert!(request().evaluate(&Pattern::new(bad)).is_err());
    }
    let mut bad = request();
    bad.extras = vec![serde_json::from_value(extra()).unwrap(); 1025];
    assert_eq!(
        bad.validate().unwrap_err().code,
        ftx_core::RefusalCode::BudgetExceeded
    );
}
