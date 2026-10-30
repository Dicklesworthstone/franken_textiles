//! Synthetic process and geometry checks, never physical cutter or textile measurements.
use super::*;
use crate::material_study::{
    Outcome, Request,
    comparison::{Basis, Metric},
    matrix,
};
use ftx_core::RefusalCode;
use ftx_pattern::{material_catalog::Selection, textile_import};
use serde_json::Value;

fn material(id: &str) -> Selection {
    serde_json::from_value(json!({"id":id,"allow_estimates":true,"record":{
        "id":id,"name":"Synthetic knife comparison sheet","form":"woven",
        "properties":{
            "width":{"value":0.1,"unit":"m","source":"test","evidence":"estimate","method":"Synthetic width"},
            "unloaded_thickness":{"value":0.0004,"unit":"m","source":"test","evidence":"estimate",
                "method":"Synthetic unloaded clearance","conditions":{"thickness_state":"unloaded","specimen_id":id,"conditioning":"Synthetic dry reference"}}
        },"sources":{"test":{"reference":format!("test:cutting-objectives/{id}"),"accessed_on":"2026-10-30","note":"Invented software input; no measured performance"}}
    }})).unwrap()
}

fn piece(id: &str) -> Value {
    json!({"id":id,"cut":{"qty":1},"grain":[[0,0],[0,10]],
        "vertices":{"a":[0,0],"b":[20,0],"c":[20,30],"d":[0,30]},
        "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
            {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}],
        "internal":[{"kind":"cut","closed":false,"pts":[[5,10],[10,10]]}]})
}

fn fixture(route: &str) -> (Pattern, Request) {
    let slow = material("synthetic-slow");
    let fast = material("synthetic-fast");
    let missing = material("synthetic-missing");
    let pattern = Pattern::new(
        json!({"schema":"fabric-pattern/1","units":"mm","meta":{"sa_mm":0},
        "fabric":{"material":slow},"pieces":[piece("front"),piece("cuff")],"seams":[]}),
    );
    let machine: program::Machine = serde_json::from_value(
        json!({"id":"synthetic-table","blade":"synthetic-cold-knife",
        "bed_mm":[300,300],"maximum_thickness_mm":2,"maximum_feed_mm_s":100,
        "travel_mm_s":100,"lift_lower_seconds":0.25,"start_mm":[0,0],"return_to_start":true}),
    )
    .unwrap();
    let machine_sha256 = program::machine_sha256(&machine).unwrap();
    let catalog = textile_import::catalog().unwrap();
    let qualifications: Vec<_> = [(&slow,20.),(&fast,40.)].into_iter().map(|(selection,feed)| {
        json!({"id":format!("recipe-{}",selection.id),"material_sha256":catalog.resolve(selection).unwrap().sha256(),
            "machine_sha256":machine_sha256,"thickness_mm":[0.2,0.8],"feed_mm_s":feed,"directions":"all_in_plane",
            "evidence":"estimate","allow_estimates":true,"method":"Synthetic constant-rate cut and raised-travel coupon protocol",
            "conditions":{"specimen_id":selection.id,"workholding":"Synthetic fixed vacuum protocol"},
            "source":{"reference":format!("test:cutting-rate/{}",selection.id),"accessed_on":"2026-10-30","note":"Synthetic source, not physical cutting evidence"}})
    }).collect();
    let process = json!({"model":"qualified_cold_knife_paths/1","machine":machine,"qualifications":qualifications,
        "ordering":"nearest_path","work_limit":1000,"geometry_work_limit":100000});
    let roll =
        json!({"copies":2,"gap_mm":2,"edge_margin_mm":2,"end_margin_mm":2,"program":process});
    let stocks: Vec<_> = [&slow,&fast,&missing].into_iter().enumerate().map(|(i, selection)| json!({
        "id":format!("sheet-{i}"),"material":selection,"outline_mm":[[0,0],[140,0],[140,160],[0,160]],
        "grain_mm":[[0,0],[0,10]],"geometry_source":{"reference":"test:cutting-objective-stock","accessed_on":"2026-10-30","note":"Synthetic physical inventory"}
    })).collect();
    let finite = json!({"schema":"ftx-finite-stock-request/1","stocks":stocks,
        "options":{"copies":2,"gap_mm":2,"boundary_margin_mm":2,"step_mm":2,"work_limit":1000000,"program":process}});
    let mut request = json!({"schema":"ftx-material-study/1","variants":[
        {"key":"slow","changes":{"schema":"ftx-material-bindings/1","default":slow}},
        {"key":"fast","changes":{"schema":"ftx-material-bindings/1","default":fast}},
        {"key":"missing","changes":{"schema":"ftx-material-bindings/1","default":missing}}],
        "comparison":{"basis":"nominal","required_consumers":[format!("{route}_program")],
            "objectives":[{"metric":format!("{route}_modeled_path_seconds"),"direction":"minimize"}],
            "constraints":[{"metric":format!("{route}_cut_length_m"),"minimum":0.1},
                {"metric":format!("{route}_raised_travel_length_m"),"minimum":0.0}]}});
    match route {
        "cutting" => request["cutting"] = roll,
        "finite_stock" => request["finite_stock"] = finite,
        "cutting_job" => {
            request["cutting_job"] = json!({"schema":"ftx-cutting-job/1",
            "routes":[{"piece":"front","stock":"roll"},{"piece":"cuff","stock":"finite_stock"}],
            "roll":roll,"finite_stock":finite})
        }
        _ => unreachable!(),
    }
    (pattern, serde_json::from_value(request).unwrap())
}

fn row<'a>(report: &'a super::super::Report, key: &str) -> &'a Alternative {
    report
        .alternatives
        .iter()
        .find(|row| row.key == key)
        .unwrap()
}

fn rerank(request: &Request, rows: &[Alternative]) -> super::super::comparison::Report {
    request.comparison.as_ref().unwrap().evaluate(
        rows,
        "synthetic-source",
        "synthetic-context",
        None,
    )
}

#[test]
fn qualified_rates_change_roll_and_finite_objectives_while_missing_evidence_stays_unknown() {
    for (route, consumer, metric) in [
        (
            "cutting",
            Consumer::CuttingProgram,
            Metric::CuttingModeledPathSeconds,
        ),
        (
            "finite_stock",
            Consumer::FiniteStockProgram,
            Metric::FiniteStockModeledPathSeconds,
        ),
    ] {
        let (pattern, request) = fixture(route);
        let original = pattern.raw().clone();
        let report = request.evaluate(&pattern).unwrap();
        assert_eq!(pattern.raw(), &original);
        let comparison = report.comparison.as_ref().unwrap();
        assert_eq!(comparison.frontier, ["fast"]);
        assert!(
            admit(row(&report, "fast"), consumer)
                .unwrap()
                .totals
                .seconds
                < admit(row(&report, "slow"), consumer)
                    .unwrap()
                    .totals
                    .seconds
        );
        for key in ["fast", "slow"] {
            let reading = &comparison
                .alternatives
                .iter()
                .find(|row| row.key == key)
                .unwrap()
                .readings[&metric];
            assert_eq!(reading.unit, "s");
            assert_eq!(reading.consumer, consumer);
            assert!(reading.reported_bounds.is_none());
        }
        let missing = row(&report, "missing");
        assert!(
            values(&missing.metrics, consumer)
                .iter()
                .all(Option::is_none)
        );
        let excluded = comparison
            .alternatives
            .iter()
            .find(|row| row.key == "missing")
            .unwrap();
        assert!(!excluded.eligible);
        assert!(excluded.readings.is_empty());
        if route == "cutting" {
            assert!(missing.cutting.value().is_some());
            assert!(missing.metrics.stock_area_m2.unwrap() > 0.);
        } else {
            assert!(missing.finite_stock.value().is_some());
            assert_eq!(missing.metrics.used_stock_count, Some(1));
            let used = row(&report, "fast")
                .finite_stock
                .value()
                .unwrap()
                .program()
                .unwrap();
            assert_eq!(used.markers.len(), 1);
            assert_eq!(
                used.markers[0].marker, 1,
                "Unused inventory is not timed or qualified"
            );
        }
        let mut bounds = request.clone();
        bounds.comparison.as_mut().unwrap().basis = Basis::ReportedBounds;
        assert!(
            bounds
                .evaluate(&pattern)
                .unwrap_err()
                .message
                .contains("reported_bounds")
        );
    }
}

#[test]
fn cached_producer_receipts_reject_changed_metrics_sources_and_partial_or_retargeted_programs() {
    let (pattern, request) = fixture("cutting");
    let mut plain = request.clone();
    plain.comparison = None;
    let produced = plain.evaluate(&pattern).unwrap();
    let original = row(&produced, "fast").clone();
    assert!(rerank(&request, std::slice::from_ref(&original)).alternatives[0].eligible);
    assert!(
        serde_json::to_value(&original)
            .unwrap()
            .get("cutting_effort")
            .is_none()
    );
    for defect in 0..9 {
        let mut changed = original.clone();
        match defect {
            0 => changed.metrics.cutting_modeled_path_seconds = Some(0.),
            1 => changed.metrics.cutting_cut_length_m = Some(0.),
            2 => changed.cutting_effort = Receipts::default(),
            3 => {
                if let Outcome::Available { result } = &mut changed.binding {
                    result["pattern"]["meta"]["changed"] = json!(true);
                }
            }
            4 => {
                changed
                    .frozen_changes
                    .as_mut()
                    .unwrap()
                    .default
                    .as_mut()
                    .unwrap()
                    .record
                    .as_mut()
                    .unwrap()
                    .sources
                    .get_mut("test")
                    .unwrap()
                    .reference = "test:changed-source".into();
            }
            _ => {
                let Outcome::Available { result: plan } = &mut changed.cutting else {
                    unreachable!()
                };
                match defect {
                    5 => plan.program.as_mut().unwrap().markers[0]
                        .selected
                        .runs
                        .pop()
                        .map(|_| ())
                        .unwrap(),
                    6 => plan.program.as_mut().unwrap().markers[0].material_sha256 = "0".repeat(64),
                    7 => plan.options.program.as_mut().unwrap().machine.travel_mm_s *= 2.,
                    _ => {
                        plan.program.as_mut().unwrap().markers[0]
                            .selected
                            .modeled_path_seconds = 0.
                    }
                }
            }
        }
        let result = rerank(&request, &[changed]);
        assert!(!result.alternatives[0].eligible, "tamper {defect}");
        assert!(
            result.alternatives[0].readings.is_empty(),
            "tamper {defect}"
        );
    }
}

#[test]
fn complete_mixed_jobs_add_each_engaged_route_once_and_never_admit_a_partial_total() {
    let (pattern, request) = fixture("cutting_job");
    let report = request.evaluate(&pattern).unwrap();
    assert_eq!(report.comparison.as_ref().unwrap().frontier, ["fast"]);
    let qualified = row(&report, "fast");
    let plan = qualified.cutting_job.value().unwrap();
    let expected: f64 = plan
        .roll
        .as_ref()
        .unwrap()
        .program
        .as_ref()
        .unwrap()
        .markers
        .iter()
        .chain(
            &plan
                .finite_stock
                .as_ref()
                .unwrap()
                .program()
                .unwrap()
                .markers,
        )
        .map(|marker| marker.selected.modeled_path_seconds)
        .sum();
    assert!(near(
        qualified.metrics.cutting_job_modeled_path_seconds.unwrap(),
        expected
    ));
    assert!(qualified.metrics.cutting_modeled_path_seconds.is_none());
    assert!(
        qualified
            .metrics
            .finite_stock_modeled_path_seconds
            .is_none()
    );
    assert_eq!(plan.inventory.len(), 4);
    let missing = row(&report, "missing");
    assert!(missing.cutting_job.value().unwrap().complete_inventory);
    assert!(missing.metrics.cutting_job_modeled_path_seconds.is_none());
    for defect in 0..3 {
        let mut changed = qualified.clone();
        let Outcome::Available { result } = &mut changed.cutting_job else {
            unreachable!()
        };
        match defect {
            0 => result.inventory.push(result.inventory[0].clone()),
            1 => result.finite_stock = None,
            _ => result.roll.as_mut().unwrap().program = None,
        }
        assert!(!rerank(&request, &[changed]).alternatives[0].eligible);
    }
    let mut partial = request.clone();
    partial
        .cutting_job
        .as_mut()
        .unwrap()
        .finite_stock
        .as_mut()
        .unwrap()
        .options
        .program = None;
    let partial = partial.evaluate(&pattern).unwrap();
    assert!(partial.comparison.as_ref().unwrap().frontier.is_empty());
    assert!(
        row(&partial, "fast")
            .cutting_job
            .value()
            .unwrap()
            .complete_inventory
    );
    assert_eq!(
        row(&partial, "fast").metrics.cutting_job_stock_area_m2,
        qualified.metrics.cutting_job_stock_area_m2
    );
    assert!(
        row(&partial, "fast")
            .metrics
            .cutting_job_modeled_path_seconds
            .is_none()
    );
}

#[test]
fn independent_roll_and_finite_plans_keep_separate_totals_and_share_reserved_program_work() {
    let (pattern, mut request) = fixture("cutting");
    let (_, finite) = fixture("finite_stock");
    request.finite_stock = finite.finite_stock;
    request.variants.retain(|variant| variant.key == "fast");
    let report = request.evaluate(&pattern).unwrap();
    let metrics = &report.alternatives[0].metrics;
    assert!(metrics.cutting_modeled_path_seconds.unwrap() > 0.);
    assert!(metrics.finite_stock_modeled_path_seconds.unwrap() > 0.);
    assert!(metrics.cutting_job_modeled_path_seconds.is_none());
    for geometry in [false, true] {
        let mut too_much = request.clone();
        for policy in [
            too_much.cutting.as_mut().unwrap().program.as_mut().unwrap(),
            too_much
                .finite_stock
                .as_mut()
                .unwrap()
                .options
                .program
                .as_mut()
                .unwrap(),
        ] {
            if geometry {
                policy.geometry_work_limit = 10_000_001;
            } else {
                policy.work_limit = 1_000_001;
            }
        }
        let refusal = too_much.evaluate(&pattern).unwrap_err();
        assert_eq!(refusal.code, RefusalCode::BudgetExceeded);
        assert!(refusal.message.contains("aggregate knife-program"));
    }
    let (_, mut mixed) = fixture("cutting_job");
    mixed.variants.truncate(1);
    let job = mixed.cutting_job.as_mut().unwrap();
    job.roll
        .as_mut()
        .unwrap()
        .program
        .as_mut()
        .unwrap()
        .work_limit = 1_000_001;
    job.finite_stock
        .as_mut()
        .unwrap()
        .options
        .program
        .as_mut()
        .unwrap()
        .work_limit = 1_000_001;
    assert_eq!(
        mixed.evaluate(&pattern).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
}

#[test]
fn process_protocols_remain_explicit_and_shared_scenario_comparisons_reuse_admitted_manufacturing()
{
    let (pattern, mut request) = fixture("cutting");
    request.variants.retain(|variant| variant.key != "missing");
    let cases = [
        matrix::Case {
            key: "first".into(),
            scenario: json!({"synthetic":1}),
            comparison: None,
        },
        matrix::Case {
            key: "second".into(),
            scenario: json!({"synthetic":2}),
            comparison: None,
        },
    ];
    let mut calls = 0;
    let matrix = request
        .evaluate_matrix_with(&pattern, &cases, 4, |_, _, _| {
            calls += 1;
            Ok(json!({"synthetic":"independent scenario"}))
        })
        .unwrap();
    assert_eq!(calls, 4);
    assert_eq!(matrix.comparison.as_ref().unwrap().frontier, ["fast"]);
    for case in &matrix.cases {
        assert_eq!(case.comparison.as_ref().unwrap().frontier, ["fast"]);
    }
    for row in &matrix.manufacturing.alternatives {
        assert!(admit(row, Consumer::CuttingProgram).is_ok());
    }
    request
        .cutting
        .as_mut()
        .unwrap()
        .program
        .as_mut()
        .unwrap()
        .qualifications
        .iter_mut()
        .find(|q| q.id == "recipe-synthetic-fast")
        .unwrap()
        .conditions
        .insert(
            "conditioning".into(),
            "Synthetic different conditioning".into(),
        );
    let different = request.evaluate(&pattern).unwrap();
    assert_eq!(different.comparison.as_ref().unwrap().frontier.len(), 2);
    assert_eq!(different.comparison.as_ref().unwrap().incomparable.len(), 1);
    request.comparison.as_mut().unwrap().allow_mixed_protocols = true;
    assert_eq!(
        request
            .evaluate(&pattern)
            .unwrap()
            .comparison
            .unwrap()
            .frontier,
        ["fast"]
    );
}
