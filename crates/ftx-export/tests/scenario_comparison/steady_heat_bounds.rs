//! Synthetic source-complete receipts exercise decision admission independently of the solver.
//! Production surface/coverage endpoint calculations are covered by the drape integration suites.
use super::*;
use ftx_pattern::material_thermal::{ConductionLaw, ReportedConductionBounds};

fn inputs(coverage: bool, reverse: bool) -> (Request, Value) {
    let mut request = request();
    request.cutting = None;
    for (variant, resistance) in request.variants.iter_mut().zip([0.1, 0.2]) {
        let record = variant
            .changes
            .default
            .as_mut()
            .unwrap()
            .record
            .as_mut()
            .unwrap();
        record.properties.insert(
            "thermal_resistance".into(),
            serde_json::from_value(json!({
            "value":resistance,"unit":"m2*K/W","source":"test","evidence":"estimate",
            "method":"Synthetic constant resistance","bounds":[resistance * 0.9,resistance * 1.1]}))
            .unwrap(),
        );
    }
    let metric = if coverage {
        "body_coverage_power_w"
    } else {
        "independent_surface_power_w"
    };
    request.comparison = Some(
        serde_json::from_value(json!({"basis":"reported_bounds",
        "objectives":[{"metric":metric,"direction":"minimize"}]}))
        .unwrap(),
    );
    let mut descriptor = descriptor();
    let boundary = json!({"inside_c":if reverse {10.} else {30.},"outside_c":if reverse {30.} else {10.},
        "inside_resistance_m2_k_w":0.1,"outside_resistance_m2_k_w":0.2});
    descriptor["scenario"]["surface"] =
        json!({"schema":"ftx-steady-surface-conditions/1","boundaries":[boundary]});
    descriptor["scenario"]["coverage"] = json!({"body_triangle_ids":[0],"instance_layers":{"band":"shell"},
        "settings":{"max_distance_m":2.,"min_normal_cosine":0.5,"separation_tolerance_m":0.00001,"cells_per_layer":1,"interfaces":[]},
        "conditions":{"boundaries":[boundary]}});
    descriptor["coverage_body_mesh_sha256"] = json!(digest(
        &json!({"positions_m":body_positions(),"triangles":[[0,1,2]]})
    ));
    (request, descriptor)
}

fn body_positions() -> Value {
    json!([[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]])
}

fn state(
    laws: &[ConductionLaw],
    boundary: &Value,
    factors: &[f64],
    interfaces: &[f64],
) -> (Value, Option<ReportedConductionBounds>) {
    let temperatures = [
        boundary["inside_c"].as_f64().unwrap(),
        boundary["outside_c"].as_f64().unwrap(),
    ];
    let films = [
        boundary["inside_resistance_m2_k_w"].as_f64().unwrap(),
        boundary["outside_resistance_m2_k_w"].as_f64().unwrap(),
    ];
    let resistance: f64 = laws
        .iter()
        .zip(factors)
        .map(|(law, factor)| factor * law.resistance_at(20.).unwrap())
        .sum();
    let flux = (temperatures[0] - temperatures[1])
        / (films[0] + films[1] + resistance + interfaces.iter().sum::<f64>());
    let bounds =
        ConductionLaw::reported_stack_bounds(laws, factors, interfaces, temperatures, films);
    let (bounds, reasons) = match bounds {
        Ok(bounds) => (Some(bounds), vec![]),
        Err(refusal) => (None, vec![refusal.message]),
    };
    (
        json!({"flux_w_m2":flux,"reported_bounds":bounds,"bounds_unavailable":reasons}),
        bounds,
    )
}

fn power(value: &mut Value, range: Option<[f64; 2]>) {
    value["reported_bounds"] = range.map_or(Value::Null, |range| json!({"power_w":range}));
    value["bounds_unavailable"] = if range.is_some() {
        json!([])
    } else {
        json!(["Synthetic missing source endpoints"])
    };
}

fn thermal(pattern: &Pattern, descriptor: &Value, covered: f64) -> Value {
    let mut simulation = synthetic(pattern, 0., 0.2);
    let layered = descriptor["scenario"]["coverage"]["instance_layers"]["shell"].is_string();
    let (positions, triangles, owners, instances, areas) = if layered {
        (
            json!([
                [0., 0., 0.1],
                [1., 0., 0.1],
                [0., 1., 0.1],
                [0., 0., 0.2],
                [1., 0., 0.2],
                [0., 1., 1.2]
            ]),
            json!([[0, 1, 2], [3, 4, 5]]),
            json!([0, 1]),
            json!(["band", "shell"]),
            vec![0.5, 0.5 * 2_f64.sqrt()],
        )
    } else {
        (
            json!([[0., 0., 0.1], [1., 0., 0.1], [0., 1., 1.1]]),
            json!([[0, 1, 2]]),
            json!([0]),
            json!(["band"]),
            vec![0.5 * 2_f64.sqrt()],
        )
    };
    simulation["positions_m"] = positions;
    simulation["positions_sha256"] = json!(digest(&simulation["positions_m"]));
    simulation["triangles"] = triangles;
    let catalog = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    let selection: Selection =
        serde_json::from_value(pattern.raw()["fabric"]["material"].clone()).unwrap();
    let material = catalog.resolve(&selection).unwrap();
    let law =
        ConductionLaw::from_material(material.material(), material.allows_estimates()).unwrap();
    simulation["drape"]["material"] =
        json!({"basis":"characterized_record","record_sha256":material.sha256(),"record":material});
    let source_patches: Vec<_> = simulation["triangles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|triangle| json!({"vertices":triangle,"column":0,"reference_area_m2":0.5}))
        .collect();
    let mut model = json!({"schema":"ftx-steady-thermal-surface/1",
        "columns":[{"schema":"ftx-steady-conduction-model/1","layers":[material.frozen_selection()],"interface_resistance_m2_k_w":[]}],
        "vertex_count":simulation["positions_m"].as_array().unwrap().len(),"patches":source_patches});
    model["sha256"] = json!(digest(&model));
    let mut frame = json!({"schema":"ftx-drape-steady-thermal-surface/1","available":true,"model":model,
        "positions_m":simulation["positions_m"],"material_report_sha256":digest(&simulation["drape"]["material"]),
        "triangle_instance":owners,"instances":instances});
    frame["sha256"] = json!(digest(&frame));
    simulation["drape"]["steady_thermal_surface"] = frame.clone();
    let report = json!({"material":simulation["drape"]["material"],"steady_thermal_surface":frame});
    let mut conditions = descriptor["scenario"]["surface"].clone();
    conditions["model_sha256"] = model["sha256"].clone();
    let environment = &conditions["exterior_exchange"][0];
    let coefficient = environment["convection_w_m2_k"]
        .as_f64()
        .filter(|h| *h > 0.);
    let mut effective = conditions["boundaries"][0].clone();
    if let Some(h) = coefficient {
        effective["outside_resistance_m2_k_w"] = json!(1. / h);
    }
    let (mut column, ranges) = state(std::slice::from_ref(&law), &effective, &[1.], &[]);
    let exchange = coefficient.map(|h| {
        let air = environment["air_c"].as_f64().unwrap();
        let surface = air + column["flux_w_m2"].as_f64().unwrap() / h;
        let flux = h * (surface - air);
        column["flux_w_m2"] = json!(flux);
        column["layer_faces_c"] = json!([[
            effective["inside_c"].as_f64().unwrap()
                - flux * effective["inside_resistance_m2_k_w"].as_f64().unwrap(),
            surface
        ]]);
        json!({"conduction":column,"surface_c":surface,"convective_flux_w_m2":flux,
            "radiative_flux_w_m2":0.,"radiation_enabled":false,
            "outer_area_per_model_area":1.,"balance_residual_w_m2":0.})
    });
    let patches: Vec<_> = areas
        .iter()
        .map(|&area| {
            let mut patch = json!({"column":0,"area_m2":area,
            "power_w":area * column["flux_w_m2"].as_f64().unwrap()});
            power(
                &mut patch,
                ranges.map(|bounds| bounds.power_bounds(area).unwrap()),
            );
            patch
        })
        .collect();
    let range = ranges.map(|bounds| {
        let mut total = [0.; 2];
        for &area in &areas {
            ReportedConductionBounds::add_power_bounds(
                &mut total,
                bounds.power_bounds(area).unwrap(),
            )
            .unwrap();
        }
        total
    });
    let mut steady = json!({"columns":[column],"patches":patches,"area_m2":areas.iter().sum::<f64>(),
        "power_w":patches.iter().map(|p| p["power_w"].as_f64().unwrap()).sum::<f64>()});
    power(&mut steady, range);
    let request = json!({"report":report,"conditions":conditions});
    simulation["surface"] = json!({"status":"available","result":{"schema":"ftx-steady-surface-result/1",
        "request":request,"request_sha256":digest(&request),"drape_steady_frame_sha256":frame["sha256"],
        "model_sha256":model["sha256"],"positions_m":simulation["positions_m"],"patch_topology":model["patches"],
        "instances":frame["instances"],"triangle_instance":frame["triangle_instance"],"steady":steady}});
    if let Some(exchange) = exchange {
        simulation["surface"]["result"]["exterior_exchange"] = json!({"columns":[exchange],
            "convective_power_w":steady["power_w"],"radiative_power_w":0.});
    }

    let policy = &descriptor["scenario"]["coverage"];
    let mut conditions = policy["conditions"].clone();
    conditions["body_mesh_sha256"] = descriptor["coverage_body_mesh_sha256"].clone();
    let layers: Vec<_> = frame["instances"].as_array().unwrap().iter().enumerate().map(|(i, id)|
        json!({"layer":policy["instance_layers"][id.as_str().unwrap()],"cloth_triangle":i,"fraction":covered})).collect();
    let interfaces = if layered { vec![0.05] } else { vec![] };
    let layout = json!({"schema":"ftx-thermal-coverage-layout/1","mapping_basis":"synthetic admission test",
        "body_vertex_count":3,"body_triangles":[[0,1,2]],"cells_per_layer":1,
        "regions":[{"id":"region-0","body_triangle":0,"body_fraction":covered,
            "layers":layers,"interface_resistance_m2_k_w":interfaces}]});
    let mut assembly =
        json!({"schema":"ftx-steady-thermal-coverage/1","source":model,"layout":layout});
    assembly["sha256"] = json!(digest(&assembly));
    let region_area = 0.5 * covered;
    let layer_areas: Vec<_> = areas.iter().map(|area| area * covered).collect();
    let factors: Vec<_> = layer_areas.iter().map(|area| region_area / area).collect();
    let laws = vec![law.clone(); layer_areas.len()];
    let environment = &conditions["exterior_exchange"][0];
    let coefficient = environment["convection_w_m2_k"]
        .as_f64()
        .filter(|h| *h > 0.);
    let mut effective = conditions["boundaries"][0].clone();
    if let Some(h) = coefficient {
        effective["outside_resistance_m2_k_w"] = json!(factors.last().unwrap() / h);
    }
    let (mut conduction, ranges) = state(&laws, &effective, &factors, &interfaces);
    let exchange = coefficient.map(|h| {
        let factor = *factors.last().unwrap();
        let air = environment["air_c"].as_f64().unwrap();
        let surface = air + conduction["flux_w_m2"].as_f64().unwrap() * (factor / h);
        let convective = h * (surface - air);
        let flux = convective / factor;
        conduction["flux_w_m2"] = json!(flux);
        let mut faces = Vec::new();
        let mut inside = effective["inside_c"].as_f64().unwrap()
            - flux * effective["inside_resistance_m2_k_w"].as_f64().unwrap();
        for (i, factor) in factors.iter().enumerate() {
            let outside = if i + 1 == factors.len() {
                surface
            } else {
                inside - flux * factor * law.resistance_at(20.).unwrap()
            };
            faces.push([inside, outside]);
            if i < interfaces.len() {
                inside = outside - flux * interfaces[i];
            }
        }
        conduction["layer_faces_c"] = json!(faces);
        json!({"conduction":conduction,"surface_c":surface,"convective_flux_w_m2":convective,
            "radiative_flux_w_m2":0.,"radiation_enabled":false,
            "outer_area_per_model_area":1./factor,"balance_residual_w_m2":0.})
    });
    let mut region = json!({"id":"region-0","body_triangle":0,"body_area_m2":region_area,"layer_area_m2":layer_areas,
        "power_w":region_area * conduction["flux_w_m2"].as_f64().unwrap(),"conduction":conduction});
    let range = ranges.map(|bounds| bounds.power_bounds(region_area).unwrap());
    power(&mut region, range);
    let boundary = if exchange.is_some() && conditions["bare_boundaries"].is_array() {
        &conditions["bare_boundaries"][0]
    } else {
        &conditions["boundaries"][0]
    };
    let bare = (0.5 * (1. - covered))
        * ((boundary["inside_c"].as_f64().unwrap() - boundary["outside_c"].as_f64().unwrap())
            / (boundary["inside_resistance_m2_k_w"].as_f64().unwrap()
                + boundary["outside_resistance_m2_k_w"].as_f64().unwrap()));
    let range = range.map(|range| {
        let mut total = [bare; 2];
        ReportedConductionBounds::add_power_bounds(&mut total, range).unwrap();
        total
    });
    let mut body = json!({"area_m2":0.5,"covered_area_m2":region_area,"bare_power_w":bare,"power_w":bare + region["power_w"].as_f64().unwrap()});
    power(&mut body, range);
    let mut steady = json!({"regions":[region],"body":[body],"body_area_m2":0.5,"covered_area_m2":region_area,
        "bare_power_w":bare,"power_w":body["power_w"],"unassigned_cloth_reference_area_m2":areas.iter().map(|_| 0.5 * (1. - covered)).sum::<f64>()});
    power(&mut steady, range);
    let request = json!({"schema":"ftx-steady-coverage-request/1","report":report,
        "map":{"body_positions_m":body_positions(),"body_triangles":[[0,1,2]],
            "instance_layers":policy["instance_layers"],"settings":policy["settings"]},"conditions":conditions});
    simulation["coverage"] = json!({"status":"available","result":{"schema":"ftx-steady-coverage-result/1",
        "request":request,"request_sha256":digest(&request),"drape_steady_frame_sha256":frame["sha256"],
        "body_mesh_sha256":descriptor["coverage_body_mesh_sha256"],"assembly":assembly,"projection":{"layout":layout},"steady":steady}});
    if let Some(exchange) = exchange {
        let convective =
            layer_areas.last().unwrap() * exchange["convective_flux_w_m2"].as_f64().unwrap();
        let residual = (region["power_w"].as_f64().unwrap() - convective) - 0.;
        simulation["coverage"]["result"]["exterior_exchange"] = json!({"regions":[exchange],
            "convective_power_w":convective,"radiative_power_w":0.,"balance_residual_w":residual,
            "bare_policy":"separate explicitly prescribed films; no inferred skin emissivity"});
    }
    simulation
}

fn evaluate(request: &Request, descriptor: &Value, change: impl Fn(&str, &mut Value)) -> Value {
    let mut pattern = serde_json::from_str::<Value>(PATTERN).unwrap();
    if descriptor["scenario"]["coverage"]["instance_layers"]["shell"].is_string() {
        let mut shell = pattern["pieces"][0].clone();
        shell["id"] = json!("shell");
        pattern["pieces"].as_array_mut().unwrap().push(shell);
    }
    json!(
        request
            .evaluate_with(&Pattern::new(pattern), descriptor, |key, pattern| {
                let mut result = thermal(pattern, descriptor, 0.5);
                change(key, &mut result);
                Ok(result)
            })
            .unwrap()
    )
}

#[test]
fn reported_steady_heat_uses_signed_source_endpoints_and_current_geometry() {
    for coverage in [false, true] {
        for reverse in [false, true] {
            let (mut request, descriptor) = inputs(coverage, reverse);
            let report = evaluate(&request, &descriptor, |_, _| {});
            let metric = if coverage {
                "body_coverage_power_w"
            } else {
                "independent_surface_power_w"
            };
            assert_eq!(
                report["comparison"]["frontier"],
                json!([if reverse { "a" } else { "b" }]),
                "{coverage} {reverse}: {}",
                report["comparison"]
            );
            let selected = &report["comparison"]["alternatives"][0]["readings"][metric];
            let ranges: [f64; 2] =
                serde_json::from_value(selected["reported_bounds"].clone()).unwrap();
            let nominal = selected["value"].as_f64().unwrap();
            assert!(ranges[0] < nominal && nominal < ranges[1]);
            assert_eq!(nominal < 0., reverse);
            let replay: Request = serde_json::from_value(json!(request)).unwrap();
            assert_eq!(report, evaluate(&replay, &descriptor, |_, _| {}));
            request.comparison.as_mut().unwrap().constraints = serde_json::from_value(json!([
                {"metric":metric,"minimum":ranges[0],"maximum":ranges[1]}]))
            .unwrap();
            let constrained = evaluate(&request, &descriptor, |_, _| {});
            assert_eq!(
                constrained["comparison"]["frontier"],
                json!(["a"]),
                "inclusive endpoints: {}",
                constrained["comparison"]
            );
            assert_eq!(report["simulations"], constrained["simulations"]);
            assert_eq!(report["comparison"]["physical_validation_claim"], false);
        }
    }
}

fn prescribe_convection(descriptor: &mut Value) {
    let surface = &mut descriptor["scenario"]["surface"];
    surface["boundaries"][0]["outside_resistance_m2_k_w"] = json!(0.);
    surface["exterior_exchange"] = json!([{"air_c":surface["boundaries"][0]["outside_c"],
        "convection_w_m2_k":5.,"surface_temperature_range_c":[-20.,80.],
        "convection_source":{"reference":"test:convection-ranges","accessed_on":"2026-11-04",
            "note":"Synthetic prescribed boundary; no empirical claim"},
        "convection_evidence":"estimate","convection_conditions":"Synthetic fixed coefficient",
        "allow_estimates":true}]);
}

fn prescribe_covered_convection(descriptor: &mut Value) {
    prescribe_convection(descriptor);
    let exterior = descriptor["scenario"]["surface"]["exterior_exchange"].clone();
    let policy = &mut descriptor["scenario"]["coverage"];
    policy["instance_layers"] = json!({"band":"lining","shell":"shell"});
    policy["settings"]["interfaces"] = json!([{"inner":"lining","outer":"shell",
        "resistance_m2_k_w":0.05,"basis":"Synthetic fixed interface, not a measured garment"}]);
    let mut bare = policy["conditions"]["boundaries"][0].clone();
    bare["inside_resistance_m2_k_w"] = json!(0.2);
    bare["outside_resistance_m2_k_w"] = json!(0.4);
    policy["conditions"]["boundaries"][0]["outside_resistance_m2_k_w"] = json!(0.);
    policy["conditions"]["exterior_exchange"] = exterior;
    policy["conditions"]["bare_boundaries"] = json!([bare]);
}

#[test]
fn layered_coverage_convection_ranges_drive_signed_decisions_with_independent_bare_films() {
    for (reverse, equal) in [(false, false), (true, false), (false, true)] {
        let (mut request, mut descriptor) = inputs(true, reverse);
        if equal {
            for path in [
                "/scenario/surface/boundaries/0",
                "/scenario/coverage/conditions/boundaries/0",
            ] {
                descriptor.pointer_mut(path).unwrap()["inside_c"] = json!(10.);
            }
        }
        prescribe_covered_convection(&mut descriptor);
        if !equal {
            // The bare path has its own outside reservoir; in the first case it opposes
            // covered-cloth heat flow. It cannot inherit the covered convection film.
            descriptor["scenario"]["coverage"]["conditions"]["bare_boundaries"][0]["outside_c"] =
                json!(45.);
        }
        let report = evaluate(&request, &descriptor, |_, _| {});
        assert_eq!(
            report["comparison"]["frontier"],
            if equal {
                json!(["a", "b"])
            } else {
                json!([if reverse { "a" } else { "b" }])
            },
            "{}",
            report["comparison"]
        );
        let inside = if reverse || equal { 10. } else { 30. };
        let outside = if reverse { 30. } else { 10. };
        let bare_outside = if equal { 10. } else { 45. };
        let bare = 0.25 * (inside - bare_outside) / (0.2 + 0.4);
        let factor = 0.25 / (0.25 * 2_f64.sqrt());
        let fixed = 0.1 + 0.05 + factor / 5.;
        for (candidate, resistance) in report["comparison"]["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .zip([0.1, 0.2])
        {
            assert_eq!(candidate["eligible"], true, "{candidate}");
            let endpoints = [0.9, 1.1].map(|scale| {
                bare + 0.25 * (inside - outside) / (fixed + resistance * scale * (1. + factor))
            });
            let expected = [
                endpoints[0].min(endpoints[1]),
                endpoints[0].max(endpoints[1]),
            ];
            let reading = &candidate["readings"]["body_coverage_power_w"];
            let actual: [f64; 2] =
                serde_json::from_value(reading["reported_bounds"].clone()).unwrap();
            for i in 0..2 {
                assert!(
                    (actual[i] - expected[i]).abs() < 1e-12,
                    "{actual:?} != {expected:?}"
                );
            }
            assert_eq!(reading["value"].as_f64().unwrap() < 0., reverse);
        }
        let replay: Request = serde_json::from_value(json!(request)).unwrap();
        assert_eq!(report, evaluate(&replay, &descriptor, |_, _| {}));
        if !equal && !reverse {
            let range = &report["comparison"]["alternatives"][0]["readings"]["body_coverage_power_w"]
                ["reported_bounds"];
            request.comparison = Some(
                serde_json::from_value(json!({"basis":"nominal",
                "objectives":[{"metric":"simulated_cloth_mass_kg","direction":"minimize"}],
                "constraints":[{"metric":"body_coverage_power_w","basis":"reported_bounds",
                    "minimum":range[0],"maximum":range[1]}]}))
                .unwrap(),
            );
            let constrained = evaluate(&request, &descriptor, |_, _| {});
            assert_eq!(constrained["comparison"]["frontier"], json!(["a"]));
            assert_eq!(constrained["simulations"], report["simulations"]);
        }
    }
}

#[test]
fn layered_convection_ranges_reject_corrupted_exterior_and_bare_receipts() {
    let (request, mut descriptor) = inputs(true, false);
    prescribe_covered_convection(&mut descriptor);
    for fault in [
        "source",
        "bounds",
        "copy",
        "flux",
        "ratio",
        "power",
        "balance",
        "bare",
        "bare-source",
        "bare-policy",
        "outer-face",
        "layer-area",
        "outside-film",
        "extra-region",
    ] {
        let report = evaluate(&request, &descriptor, |key, simulation| {
            if key != "a" {
                return;
            }
            let result = &mut simulation["coverage"]["result"];
            match fault {
                "source" => {
                    result["request"]["conditions"]["exterior_exchange"][0]["convection_source"]["reference"] =
                        json!("test:changed")
                }
                "bounds" => {
                    result["steady"]["regions"][0]["conduction"]["reported_bounds"]["flux_w_m2"]
                        [0] = json!(0.);
                    result["exterior_exchange"]["regions"][0]["conduction"] =
                        result["steady"]["regions"][0]["conduction"].clone();
                }
                "copy" => {
                    result["exterior_exchange"]["regions"][0]["conduction"]["reported_bounds"]["flux_w_m2"]
                        [0] = json!(0.)
                }
                "flux" => {
                    result["exterior_exchange"]["regions"][0]["convective_flux_w_m2"] = json!(0.)
                }
                "ratio" => {
                    result["exterior_exchange"]["regions"][0]["outer_area_per_model_area"] =
                        json!(1.)
                }
                "power" => result["exterior_exchange"]["convective_power_w"] = json!(0.),
                "balance" => result["exterior_exchange"]["balance_residual_w"] = json!(1.),
                "bare" => result["steady"]["body"][0]["bare_power_w"] = json!(0.),
                "bare-source" => {
                    result["request"]["conditions"]["bare_boundaries"][0]["outside_c"] = json!(15.)
                }
                "bare-policy" => {
                    result["exterior_exchange"]["bare_policy"] = json!("inherited cloth convection")
                }
                "outer-face" => {
                    result["steady"]["regions"][0]["conduction"]["layer_faces_c"][1][1] = json!(0.);
                    result["exterior_exchange"]["regions"][0]["conduction"] =
                        result["steady"]["regions"][0]["conduction"].clone();
                }
                "layer-area" => result["steady"]["regions"][0]["layer_area_m2"][1] = json!(0.25),
                "outside-film" => {
                    result["request"]["conditions"]["boundaries"][0]["outside_resistance_m2_k_w"] =
                        json!(0.2)
                }
                "extra-region" => {
                    let extra = result["exterior_exchange"]["regions"][0].clone();
                    result["exterior_exchange"]["regions"]
                        .as_array_mut()
                        .unwrap()
                        .push(extra);
                }
                _ => unreachable!(),
            }
            result["request_sha256"] = json!(digest(&result["request"]));
        });
        assert_eq!(
            report["comparison"]["frontier"],
            json!(["b"]),
            "{fault}: {}",
            report["comparison"]
        );
        let candidate = &report["comparison"]["alternatives"][0];
        assert_eq!(candidate["eligible"], false);
        assert!(candidate["readings"]["body_coverage_power_w"]["value"].is_number());
        assert!(candidate["readings"]["body_coverage_power_w"]["reported_bounds"].is_null());
    }
}

#[test]
fn layered_convection_requires_complete_qualified_paths_and_explicit_bare_reservoirs() {
    for fault in [
        "missing",
        "domain",
        "radiation",
        "zero-h",
        "qualification",
        "bare-temperature",
        "missing-bare",
        "nonuniform-environment",
        "nonuniform-bare",
    ] {
        let (mut request, mut descriptor) = inputs(true, false);
        prescribe_covered_convection(&mut descriptor);
        let conditions = &mut descriptor["scenario"]["coverage"]["conditions"];
        match fault {
            "missing" => {
                request.variants[0]
                    .changes
                    .default
                    .as_mut()
                    .unwrap()
                    .record
                    .as_mut()
                    .unwrap()
                    .properties
                    .get_mut("thermal_resistance")
                    .unwrap()
                    .bounds = None
            }
            "domain" => {
                let factor = 1. / 2_f64.sqrt();
                let surface =
                    10. + 20. / (0.1 + 0.05 + 0.1 * (1. + factor) + factor / 5.) * (factor / 5.);
                conditions["exterior_exchange"][0]["surface_temperature_range_c"] =
                    json!([surface - 0.01, surface + 0.01]);
            }
            "radiation" => {
                conditions["exterior_exchange"][0]["radiation"] = json!({"ambient_c":10.,
                "emissivity_property":"emissivity","exposed_face":"public","allow_opaque_gray_approximation":true})
            }
            "zero-h" => conditions["exterior_exchange"][0]["convection_w_m2_k"] = json!(0.),
            "qualification" => {
                conditions["exterior_exchange"][0]["convection_source"]["reference"] = json!("")
            }
            "bare-temperature" => conditions["bare_boundaries"][0]["inside_c"] = json!(31.),
            "missing-bare" => {
                conditions
                    .as_object_mut()
                    .unwrap()
                    .remove("bare_boundaries");
            }
            "nonuniform-environment" => {
                let extra = conditions["exterior_exchange"][0].clone();
                conditions["exterior_exchange"]
                    .as_array_mut()
                    .unwrap()
                    .push(extra);
            }
            "nonuniform-bare" => {
                let extra = conditions["bare_boundaries"][0].clone();
                conditions["bare_boundaries"]
                    .as_array_mut()
                    .unwrap()
                    .push(extra);
            }
            _ => unreachable!(),
        }
        let report = evaluate(&request, &descriptor, |_, _| {});
        let candidate = &report["comparison"]["alternatives"][0];
        assert_eq!(
            candidate["eligible"], false,
            "{fault}: {}",
            report["comparison"]
        );
        assert!(candidate["readings"]["body_coverage_power_w"]["value"].is_number());
        if fault == "domain" {
            assert!(candidate["exclusions"].to_string().contains("endpoints"));
        }
    }
}

#[test]
fn coverage_convection_nonuniform_environments_and_bare_films_bind_the_original_body() {
    for bound in [true, false] {
        let (request, mut descriptor) = inputs(true, false);
        prescribe_covered_convection(&mut descriptor);
        let positions = json!([[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [1., 1., 0.]]);
        let triangles = json!([[0, 1, 2], [1, 3, 2]]);
        let identity = digest(&json!({"positions_m":positions,"triangles":triangles}));
        descriptor["coverage_body_mesh_sha256"] = json!(identity);
        let policy = &mut descriptor["scenario"]["coverage"];
        policy["body_triangle_ids"] = json!([0, 1]);
        if bound {
            policy["conditions"]["body_mesh_sha256"] = json!(identity);
        }
        // Covered films still broadcast. Only the exterior and bare arrays need this identity.
        // The second body triangle is bare, so its textile environment is unused.
        let mut environment = policy["conditions"]["exterior_exchange"][0].clone();
        environment["air_c"] = json!(20.);
        environment["convection_w_m2_k"] = json!(17.);
        policy["conditions"]["exterior_exchange"]
            .as_array_mut()
            .unwrap()
            .push(environment);
        let mut bare = policy["conditions"]["bare_boundaries"][0].clone();
        bare["outside_c"] = json!(80.);
        policy["conditions"]["bare_boundaries"]
            .as_array_mut()
            .unwrap()
            .push(bare);
        let report = evaluate(&request, &descriptor, |_, simulation| {
            let result = &mut simulation["coverage"]["result"];
            result["request"]["map"]["body_positions_m"] = positions.clone();
            result["request"]["map"]["body_triangles"] = triangles.clone();
            result["request_sha256"] = json!(digest(&result["request"]));
            result["assembly"]["layout"]["body_vertex_count"] = json!(4);
            result["assembly"]["layout"]["body_triangles"] = triangles.clone();
            result["projection"]["layout"] = result["assembly"]["layout"].clone();
            let mut assembly = result["assembly"].clone();
            assembly.as_object_mut().unwrap().remove("sha256");
            result["assembly"]["sha256"] = json!(digest(&assembly));
            let bare = 0.5 * ((30. - 80.) / (0.2 + 0.4));
            let mut field =
                json!({"area_m2":0.5,"covered_area_m2":0.,"bare_power_w":bare,"power_w":bare});
            power(&mut field, Some([bare; 2]));
            let steady = &mut result["steady"];
            steady["body"].as_array_mut().unwrap().push(field);
            let mut range: [f64; 2] =
                serde_json::from_value(steady["reported_bounds"]["power_w"].clone()).unwrap();
            ReportedConductionBounds::add_power_bounds(&mut range, [bare; 2]).unwrap();
            power(steady, Some(range));
            steady["body_area_m2"] = json!(1.);
            steady["power_w"] = json!(steady["power_w"].as_f64().unwrap() + bare);
            steady["bare_power_w"] = json!(steady["bare_power_w"].as_f64().unwrap() + bare);
        });
        assert_eq!(
            report["comparison"]["frontier"],
            if bound { json!(["b"]) } else { json!([]) },
            "body bound {bound}: {}",
            report["comparison"]
        );
        for candidate in report["comparison"]["alternatives"].as_array().unwrap() {
            assert_eq!(candidate["eligible"], bound);
            let reading = &candidate["readings"]["body_coverage_power_w"];
            assert!(reading["value"].as_f64().unwrap() < 0.);
            assert_eq!(reading["reported_bounds"].is_array(), bound);
        }
    }
}

#[test]
fn prescribed_convection_ranges_drive_signed_dominance_and_explicit_bounded_guards() {
    for (reverse, equal) in [(false, false), (true, false), (false, true)] {
        let (mut request, mut descriptor) = inputs(false, reverse);
        if equal {
            descriptor["scenario"]["surface"]["boundaries"][0]["inside_c"] = json!(10.);
        }
        prescribe_convection(&mut descriptor);
        let report = evaluate(&request, &descriptor, |_, _| {});
        let frontier = if equal {
            json!(["a", "b"])
        } else {
            json!([if reverse { "a" } else { "b" }])
        };
        assert_eq!(
            report["comparison"]["frontier"], frontier,
            "{}",
            report["comparison"]
        );
        assert!(report["comparison"]["alternatives"][0]["readings"]
            ["independent_surface_power_w"]["reported_bounds"].is_array());
        if !reverse && !equal {
            request.comparison = Some(serde_json::from_value(json!({"basis":"nominal",
                "objectives":[{"metric":"simulated_cloth_mass_kg","direction":"minimize"}],
                "constraints":[{"metric":"independent_surface_power_w","basis":"reported_bounds","maximum":32.}]})).unwrap());
            let guarded = evaluate(&request, &descriptor, |_, _| {});
            assert_eq!(guarded["comparison"]["frontier"], json!(["b"]));
            assert_eq!(guarded["comparison"]["alternatives"][0]["eligible"], false);
        }
    }
}

#[test]
fn convection_ranges_refuse_changed_receipts_and_unqualified_endpoints_without_erasing_nominal() {
    let (request, mut descriptor) = inputs(false, false);
    prescribe_convection(&mut descriptor);
    for fault in ["source", "bounds", "flux", "ratio", "power", "outside-film"] {
        let report = evaluate(&request, &descriptor, |key, simulation| {
            if key != "a" {
                return;
            }
            let result = &mut simulation["surface"]["result"];
            match fault {
                "source" => {
                    result["request"]["conditions"]["exterior_exchange"][0]["convection_source"]
                        ["reference"] = json!("test:changed-convection-source");
                }
                "bounds" => {
                    result["steady"]["columns"][0]["reported_bounds"]["flux_w_m2"][0] = json!(0.);
                }
                "flux" => {
                    result["exterior_exchange"]["columns"][0]["convective_flux_w_m2"] = json!(0.);
                }
                "ratio" => {
                    result["exterior_exchange"]["columns"][0]["outer_area_per_model_area"] =
                        json!(2.);
                }
                "power" => result["exterior_exchange"]["convective_power_w"] = json!(0.),
                "outside-film" => {
                    result["request"]["conditions"]["boundaries"][0]["outside_resistance_m2_k_w"] =
                        json!(0.2);
                }
                _ => unreachable!(),
            }
            result["request_sha256"] = json!(digest(&result["request"]));
        });
        assert_eq!(
            report["comparison"]["frontier"],
            json!(["b"]),
            "{fault}: {}",
            report["comparison"]
        );
        assert!(report["comparison"]["alternatives"][0]["readings"]
            ["independent_surface_power_w"]["value"].is_number());
    }
    for fault in ["missing", "domain", "radiation", "zero-h", "qualification"] {
        let (mut request, mut descriptor) = inputs(false, false);
        prescribe_convection(&mut descriptor);
        let environment = &mut descriptor["scenario"]["surface"]["exterior_exchange"][0];
        match fault {
            "missing" => {
                request.variants[0]
                    .changes
                    .default
                    .as_mut()
                    .unwrap()
                    .record
                    .as_mut()
                    .unwrap()
                    .properties
                    .get_mut("thermal_resistance")
                    .unwrap()
                    .bounds = None;
            }
            "domain" => environment["surface_temperature_range_c"] = json!([19.9, 20.1]),
            "radiation" => environment["radiation"] = json!({"ambient_c":10.}),
            "zero-h" => environment["convection_w_m2_k"] = json!(0.),
            "qualification" => environment["convection_source"]["reference"] = json!(""),
            _ => unreachable!(),
        }
        let report = evaluate(&request, &descriptor, |_, _| {});
        let row = &report["comparison"]["alternatives"][0];
        assert_eq!(row["eligible"], false, "{fault}: {}", report["comparison"]);
        assert!(row["readings"]["independent_surface_power_w"]["value"].is_number());
        if fault == "domain" {
            assert!(row["exclusions"].to_string().contains("endpoints"));
        }
    }
}

#[test]
fn steady_heat_missing_sources_and_unsupported_coupling_keep_nominal_visible() {
    for coverage in [false, true] {
        let (mut request, mut descriptor) = inputs(coverage, false);
        let name = if coverage { "coverage" } else { "surface" };
        let metric = if coverage {
            "body_coverage_power_w"
        } else {
            "independent_surface_power_w"
        };
        request.variants[0]
            .changes
            .default
            .as_mut()
            .unwrap()
            .record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("thermal_resistance")
            .unwrap()
            .bounds = None;
        let missing = evaluate(&request, &descriptor, |_, _| {});
        assert_eq!(missing["comparison"]["frontier"], json!(["b"]));
        let a = &missing["comparison"]["alternatives"][0];
        assert!(a["readings"][metric]["value"].is_number());
        assert!(a["readings"][metric]["reported_bounds"].is_null());
        assert!(
            a["exclusions"]
                .to_string()
                .contains("complete positive source-reported endpoints")
        );
        request.comparison.as_mut().unwrap().basis =
            ftx_export::material_study::comparison::Basis::Nominal;
        assert_eq!(
            evaluate(&request, &descriptor, |_, _| {})["comparison"]["alternatives"][0]["eligible"],
            true
        );
        request.comparison.as_mut().unwrap().basis =
            ftx_export::material_study::comparison::Basis::ReportedBounds;
        if coverage {
            descriptor["scenario"][name]["conditions"]["exterior_exchange"] = json!([{}]);
        } else {
            descriptor["scenario"][name]["exterior_exchange"] = json!([{}]);
        }
        let unsupported = evaluate(&request, &descriptor, |_, _| {});
        assert!(
            unsupported["comparison"]["frontier"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            unsupported["comparison"]["alternatives"][1]["exclusions"]
                .to_string()
                .contains("steady heat bounds")
        );
        assert!(
            unsupported["comparison"]["alternatives"][1]["readings"][metric]["value"].is_number()
        );
    }
}

#[test]
fn static_heat_ranges_reject_self_consistent_sources_stale_geometry_and_changed_components() {
    for coverage in [false, true] {
        let (request, descriptor) = inputs(coverage, false);
        let name = if coverage { "coverage" } else { "surface" };
        let faults: &[&str] = if coverage {
            &[
                "bounds",
                "source",
                "request",
                "geometry",
                "body",
                "layer_area",
                "bare",
                "region",
                "component_bounds",
                "gap",
            ]
        } else {
            &[
                "bounds",
                "source",
                "request",
                "geometry",
                "area",
                "patch",
                "component_bounds",
            ]
        };
        for fault in faults {
            let report = evaluate(&request, &descriptor, |key, simulation| {
                if key != "a" {
                    return;
                }
                let result = &mut simulation[name]["result"];
                match *fault {
                    "bounds" => result["steady"]["reported_bounds"]["power_w"][1] = json!(1e9),
                    "request" => {
                        result["request"]["conditions"]["boundaries"][0]["inside_c"] = json!(31.);
                        result["request_sha256"] = json!(digest(&result["request"]));
                    }
                    "source" => {
                        result["request"]["report"]["steady_thermal_surface"]["model"]["columns"]
                            [0]["layers"][0]["allow_estimates"] = json!(false);
                        result["request_sha256"] = json!(digest(&result["request"]));
                    }
                    "geometry" => simulation["positions_m"][2][2] = json!(1.2),
                    "area" => result["steady"]["patches"][0]["area_m2"] = json!(0.5),
                    "patch" => result["steady"]["patches"][0]["power_w"] = json!(0.),
                    "body" => {
                        result["request"]["map"]["body_positions_m"][1][0] = json!(2.);
                        result["body_mesh_sha256"] = json!(digest(
                            &json!({"positions_m":result["request"]["map"]["body_positions_m"],"triangles":result["request"]["map"]["body_triangles"]})
                        ));
                        result["request"]["conditions"]["body_mesh_sha256"] =
                            result["body_mesh_sha256"].clone();
                        result["request_sha256"] = json!(digest(&result["request"]));
                    }
                    "layer_area" => {
                        result["steady"]["regions"][0]["layer_area_m2"][0] = json!(0.25)
                    }
                    "bare" => result["steady"]["body"][0]["bare_power_w"] = json!(0.),
                    "region" => result["steady"]["regions"][0]["power_w"] = json!(0.),
                    "component_bounds" => {
                        let component = if coverage {
                            &mut result["steady"]["regions"][0]["conduction"]
                        } else {
                            &mut result["steady"]["columns"][0]
                        };
                        component["reported_bounds"]["flux_w_m2"][1] = json!(1e9);
                    }
                    "gap" => {
                        result["projection"]["gap_interfaces"] = json!([{"region":"region-0"}])
                    }
                    _ => unreachable!(),
                }
            });
            assert_eq!(
                report["comparison"]["frontier"],
                json!(["b"]),
                "{name} {fault}: {}",
                report["comparison"]
            );
            assert_eq!(report["comparison"]["alternatives"][0]["eligible"], false);
        }
    }
}

#[test]
fn coverage_bounds_keep_opposing_bare_power_and_reject_a_cancelling_bad_component() {
    let (request, mut descriptor) = inputs(true, false);
    let positions = json!([[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [1., 1., 0.]]);
    let triangles = json!([[0, 1, 2], [1, 3, 2]]);
    let identity = digest(&json!({"positions_m":positions,"triangles":triangles}));
    descriptor["coverage_body_mesh_sha256"] = json!(identity);
    let policy = &mut descriptor["scenario"]["coverage"];
    policy["body_triangle_ids"] = json!([0, 1]);
    policy["conditions"]["body_mesh_sha256"] = json!(identity);
    let mut opposite = policy["conditions"]["boundaries"][0].clone();
    opposite["inside_c"] = json!(10.);
    opposite["outside_c"] = json!(30.);
    policy["conditions"]["boundaries"]
        .as_array_mut()
        .unwrap()
        .push(opposite);
    for bad in [false, true] {
        let report = evaluate(&request, &descriptor, |key, simulation| {
            let result = &mut simulation["coverage"]["result"];
            result["request"]["map"]["body_positions_m"] = positions.clone();
            result["request"]["map"]["body_triangles"] = triangles.clone();
            result["request_sha256"] = json!(digest(&result["request"]));
            result["assembly"]["layout"]["body_vertex_count"] = json!(4);
            result["assembly"]["layout"]["body_triangles"] = triangles.clone();
            result["projection"]["layout"] = result["assembly"]["layout"].clone();
            let mut content = result["assembly"].clone();
            content.as_object_mut().unwrap().remove("sha256");
            result["assembly"]["sha256"] = json!(digest(&content));
            let bare = 0.5 * (-20. / (0.1 + 0.2));
            let field = json!({"area_m2":0.5,"covered_area_m2":0.,"bare_power_w":bare,"power_w":bare,
                "reported_bounds":{"power_w":[bare,bare]},"bounds_unavailable":[]});
            let steady = &mut result["steady"];
            steady["body"].as_array_mut().unwrap().push(field);
            let mut range: [f64; 2] =
                serde_json::from_value(steady["reported_bounds"]["power_w"].clone()).unwrap();
            ReportedConductionBounds::add_power_bounds(&mut range, [bare; 2]).unwrap();
            steady["reported_bounds"]["power_w"] = json!(range);
            steady["body_area_m2"] = json!(1.);
            steady["power_w"] = json!(steady["power_w"].as_f64().unwrap() + bare);
            steady["bare_power_w"] = json!(steady["bare_power_w"].as_f64().unwrap() + bare);
            if bad && key == "a" {
                // Both nominal total and aggregate range remain unchanged: corrupt opposite
                // local ranges would cancel if admission looked only at their sum.
                for endpoint in 0..2 {
                    steady["body"][0]["reported_bounds"]["power_w"][endpoint] = json!(
                        steady["body"][0]["reported_bounds"]["power_w"][endpoint]
                            .as_f64()
                            .unwrap()
                            + 1.
                    );
                    steady["body"][1]["reported_bounds"]["power_w"][endpoint] = json!(bare - 1.);
                }
            }
        });
        assert_eq!(
            report["comparison"]["frontier"],
            json!(["b"]),
            "{}",
            report["comparison"]
        );
        let a = &report["comparison"]["alternatives"][0];
        assert_eq!(a["eligible"], !bad);
        assert!(
            a["readings"]["body_coverage_power_w"]["value"]
                .as_f64()
                .unwrap()
                < 0.
        );
        if !bad {
            assert!(
                a["readings"]["body_coverage_power_w"]["reported_bounds"][1]
                    .as_f64()
                    .unwrap()
                    < 0.
            );
        }
    }
}

#[test]
fn heat_ranges_bind_final_material_assignments_and_explicit_bounded_guards() {
    let (mut request, descriptor) = inputs(false, false);
    request.comparison = Some(serde_json::from_value(json!({"basis":"nominal",
        "objectives":[{"metric":"simulated_cloth_mass_kg","direction":"minimize"}],
        "constraints":[{"metric":"independent_surface_power_w","basis":"reported_bounds","maximum":32.}]})).unwrap());
    let guarded = evaluate(&request, &descriptor, |_, _| {});
    assert_eq!(guarded["comparison"]["frontier"], json!(["b"]));
    assert!(guarded["comparison"]["alternatives"][1]["readings"]["independent_surface_power_w"]["reported_bounds"].is_array());
    assert!(guarded["comparison"]["alternatives"][1]["readings"]["simulated_cloth_mass_kg"]["reported_bounds"].is_null());
    let (request, _) = inputs(false, false);
    let report = json!(
        request
            .evaluate_with(
                &Pattern::from_json(PATTERN).unwrap(),
                &descriptor,
                |key, pattern| {
                    let mut changed = pattern.raw().clone();
                    if key == "a" {
                        let material = &mut changed["fabric"]["material"];
                        material["expected_sha256"] = Value::Null;
                        material["record"]["properties"]["thermal_resistance"]["value"] =
                            json!(0.3);
                        material["record"]["properties"]["thermal_resistance"]["bounds"] =
                            json!([0.29, 0.31]);
                    }
                    let mut result = thermal(&Pattern::new(changed), &descriptor, 0.5);
                    // Every substituted column, source digest, frame and output is internally consistent;
                    // it still must match the selected alternative's material rather than self-attest.
                    result["pattern_sha256"] = json!(digest(pattern.raw()));
                    Ok(result)
                }
            )
            .unwrap()
    );
    assert_eq!(report["comparison"]["frontier"], json!(["b"]));
    assert!(
        report["comparison"]["alternatives"][0]["exclusions"]
            .to_string()
            .contains("final material assignments")
    );
}
