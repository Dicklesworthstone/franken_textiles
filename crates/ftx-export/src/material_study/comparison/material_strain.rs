//! Admit the trusted production strain consumer's source, sampled scope and extrema arithmetic.
//! This is receipt consistency, not a second deformation solve or an imported-report certificate.

use super::*;
use ftx_core::Vec2;
use ftx_pattern::{material_calibration, material_compression::regions};

type Ranges = [[f64; 2]; 3];

fn fail() -> Refusal {
    invalid(
        "material strain comparison needs exact source-qualified ownership, complete accepted samples and consistent raw Green-strain extrema/witnesses; no failure or safe-fit limit is inferred",
    )
}

fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], Refusal> {
    vapor_array(value, key).map_err(|_| fail())
}

fn index(value: &Value) -> Result<usize, Refusal> {
    vapor_index(value).map_err(|_| fail())
}

fn valid_ranges(ranges: &Ranges) -> bool {
    ranges
        .iter()
        .all(|[lo, hi]| lo.is_finite() && hi.is_finite() && lo <= hi)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Witness {
    value: f64,
    source_triangle: usize,
    sample_index: usize,
    time_s: f64,
}

impl Witness {
    fn order(&self) -> (usize, usize) {
        (self.sample_index, self.source_triangle)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Summary {
    green_strain_range: Ranges,
    minimum_witness: [Witness; 3],
    maximum_witness: [Witness; 3],
}

impl Summary {
    fn read(value: &Value) -> Result<Self, Refusal> {
        let result = Self::deserialize(value).map_err(|_| fail())?;
        if !valid_ranges(&result.green_strain_range) {
            return Err(fail());
        }
        Ok(result)
    }

    // Only reconcile the published extrema. Material-frame kinematics remain owned by ftx-drape.
    fn include(&mut self, other: &Self) {
        for i in 0..3 {
            for (side, mine, theirs) in [
                (0, &mut self.minimum_witness[i], &other.minimum_witness[i]),
                (1, &mut self.maximum_witness[i], &other.maximum_witness[i]),
            ] {
                let more_extreme = if side == 0 {
                    theirs.value < mine.value
                } else {
                    theirs.value > mine.value
                };
                if more_extreme || (theirs.value == mine.value && theirs.order() < mine.order()) {
                    *mine = theirs.clone();
                    self.green_strain_range[i][side] = theirs.value;
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct Frame {
    sample_index: usize,
    time_s: f64,
    positions_sha256: String,
    green_strain_range: Ranges,
    minimum_triangle: [usize; 3],
    maximum_triangle: [usize; 3],
}

impl Frame {
    fn summary(&self) -> Summary {
        let witnesses = |side, triangles: [usize; 3]| {
            std::array::from_fn(|i| Witness {
                value: self.green_strain_range[i][side],
                source_triangle: triangles[i],
                sample_index: self.sample_index,
                time_s: self.time_s,
            })
        };
        Summary {
            green_strain_range: self.green_strain_range,
            minimum_witness: witnesses(0, self.minimum_triangle),
            maximum_witness: witnesses(1, self.maximum_triangle),
        }
    }
}

struct Material<'a> {
    selection: &'a Value,
    fitted: Option<Ranges>,
    nonlinear: Option<Ranges>,
    area_ratio: Option<[f64; 2]>,
}

struct Source<'a> {
    vertex_count: usize,
    triangles: BTreeMap<usize, &'a Value>,
    domains: BTreeMap<&'a str, [Option<Ranges>; 2]>,
    regions: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Region {
    id: String,
    instance: String,
    selector: regions::Selector,
}

struct RegionalScope {
    instances: BTreeSet<String>,
    triangle_regions: BTreeMap<usize, String>,
    regions: BTreeMap<String, String>,
    flat_areas_m2: Vec<f64>,
}

// Reconstruct the fixed source-space union. Hashes alone cannot establish that an omitted or
// reassigned retained triangle lies outside an authored region. The kinematics remain in drape.
fn regional_scope(
    policy: &Value,
    source: &Value,
    ownership: &Value,
    instances: &[Value],
    owners: &[Value],
) -> Result<RegionalScope, Refusal> {
    let requested = array(policy, "regions")?;
    let flat = array(ownership, "rest_triangles_mm")?;
    let published = source["regions"].as_object().ok_or_else(fail)?;
    if !(1..=256).contains(&requested.len())
        || published.len() != requested.len()
        || flat.len() != owners.len()
    {
        return Err(fail());
    }
    let flat = flat
        .iter()
        .map(|triangle| <[Vec2; 3]>::deserialize(triangle).map_err(|_| fail()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut flat_areas_m2 = Vec::with_capacity(flat.len());
    let mut by_instance = BTreeMap::<&str, Vec<usize>>::new();
    for (i, (triangle, owner)) in flat.iter().zip(owners).enumerate() {
        regions::triangle_centroid_mm(*triangle)?;
        let [a, b, c] = *triangle;
        let area = 0.5 * (b - a).cross(c - a).abs() * 1e-6;
        if !area.is_finite() || area <= 0. {
            return Err(fail());
        }
        flat_areas_m2.push(area);
        let instance = instances
            .get(index(owner)?)
            .and_then(Value::as_str)
            .ok_or_else(fail)?;
        by_instance.entry(instance).or_default().push(i);
    }
    let requested = requested
        .iter()
        .map(|value| Region::deserialize(value).map_err(|_| fail()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut edge_tests = 0usize;
    let mut ids = BTreeSet::new();
    let mut selectors = Vec::with_capacity(requested.len());
    for region in &requested {
        if region.id.trim().is_empty()
            || region.id.len() > 512
            || !ids.insert(region.id.as_str())
            || region.instance.trim().is_empty()
            || region.instance.len() > 4096
        {
            return Err(fail());
        }
        let owned = by_instance.get(region.instance.as_str()).ok_or_else(fail)?;
        edge_tests = owned
            .len()
            .checked_mul(region.selector.polygon_mm.len())
            .and_then(|count| edge_tests.checked_add(count))
            .filter(|&count| count <= regions::MAX_REGION_EDGE_TESTS)
            .ok_or_else(fail)?;
        selectors.push((
            region,
            regions::PreparedSelector::new(&region.selector)?,
            owned,
        ));
    }
    let mut result = RegionalScope {
        instances: BTreeSet::new(),
        triangle_regions: BTreeMap::new(),
        regions: BTreeMap::new(),
        flat_areas_m2,
    };
    for (region, selector, owned) in selectors {
        let mut selected = Vec::new();
        for &i in owned {
            if selector.contains_triangle_centroid(flat[i])? {
                if result
                    .triangle_regions
                    .insert(i, region.id.clone())
                    .is_some()
                {
                    return Err(fail());
                }
                selected.push(i);
            }
        }
        let published = published.get(&region.id).ok_or_else(fail)?;
        if selected.is_empty()
            || published["instance"] != region.instance
            || published["selector"] != json!(region.selector)
            || published["source_triangles"] != json!(selected)
            || !finite(&published["reference_area_m2"]).is_some_and(|area| area > 0.)
        {
            return Err(fail());
        }
        result.instances.insert(region.instance.clone());
        result
            .regions
            .insert(region.id.clone(), region.instance.clone());
    }
    Ok(result)
}

fn source<'a>(
    alternative: &Alternative,
    policy: &Value,
    simulation: &'a Value,
    report: &'a Value,
) -> Result<Source<'a>, Refusal> {
    let source = &report["source"];
    let material_report = &simulation["drape"]["material"];
    let ownership = &simulation["material_strain_ownership"];
    let all_instances = array(ownership, "instances")?;
    let owners = array(ownership, "triangle_instance")?;
    let faces = array(simulation, "triangles")?;
    let vertex_count = index(&source["vertex_count"])?;
    let bound = alternative.binding.value().ok_or_else(fail)?;
    let pattern = ftx_pattern::Pattern::new(bound["pattern"].clone());
    let changes = alternative.frozen_changes.as_ref().ok_or_else(fail)?;
    if !(3..=196_608).contains(&vertex_count)
        || vertex_count != array(simulation, "positions_m")?.len()
        || faces.is_empty()
        || faces.len() > 65_536
        || owners.len() != faces.len()
        || all_instances.is_empty()
        || all_instances.len() > 196_608
        || source["material_report_sha256"] != hash(material_report)?
        || report["source_sha256"] != hash(source)?
    {
        return Err(fail());
    }
    let mut names = BTreeMap::new();
    for (i, instance) in all_instances.iter().enumerate() {
        let id = instance.as_str().ok_or_else(fail)?;
        if id.trim().is_empty()
            || id.len() > 4096
            || names.insert(id, i).is_some()
            || pattern.instance(id).is_none()
        {
            return Err(fail());
        }
    }
    let mut retained = BTreeSet::new();
    for owner in owners {
        let name = all_instances
            .get(index(owner)?)
            .and_then(Value::as_str)
            .ok_or_else(fail)?;
        retained.insert(name.to_owned());
    }
    let regional = if policy["schema"] == "ftx-material-strain/2" {
        Some(regional_scope(
            policy,
            source,
            ownership,
            all_instances,
            owners,
        )?)
    } else {
        if source.get("regions").is_some() || report.get("regions").is_some() {
            return Err(fail());
        }
        None
    };
    let selected = if let Some(regional) = &regional {
        regional.instances.clone()
    } else if let Some(requested) = policy.get("instances") {
        let ids = requested.as_array().ok_or_else(fail)?;
        let mut selected = BTreeSet::new();
        for id in ids {
            let id = id.as_str().ok_or_else(fail)?;
            if !retained.contains(id) || !selected.insert(id.to_owned()) {
                return Err(fail());
            }
        }
        selected
    } else {
        retained
    };
    if selected.is_empty() || selected.len() > 4096 || source["instances"] != json!(selected) {
        return Err(fail());
    }
    let per_piece = match material_report["basis"].as_str() {
        Some("characterized_record") => false,
        Some("per_piece_characterization") => true,
        _ => return Err(fail()),
    };
    if per_piece {
        let receipts = array(material_report, "instances")?;
        if receipts.len() != all_instances.len()
            || material_report["triangle_instance"] != ownership["triangle_instance"]
            || receipts
                .iter()
                .zip(all_instances)
                .any(|(receipt, name)| receipt["instance"] != *name)
        {
            return Err(fail());
        }
    }
    let catalog = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    let raw_materials = source["materials"].as_object().ok_or_else(fail)?;
    if raw_materials.is_empty() || raw_materials.len() > selected.len() {
        return Err(fail());
    }
    let mut materials = BTreeMap::new();
    for (identity, value) in raw_materials {
        let selection = Selection::deserialize(value).map_err(|_| fail())?;
        if selection.record.is_none() || selection.expected_sha256.as_deref() != Some(identity) {
            return Err(fail());
        }
        let resolved = catalog.resolve(&selection)?;
        let receipt = if per_piece {
            &material_report["records"][identity]
        } else {
            material_report
        };
        if resolved.sha256() != identity
            || receipt["record_sha256"] != *identity
            || serde_json::to_value(&resolved).map_err(|_| fail())? != receipt["record"]
            || serde_json::to_value(resolved.frozen_selection()).map_err(|_| fail())? != *value
        {
            return Err(fail());
        }
        let nonlinear = resolved.membrane_response()?;
        materials.insert(
            identity.as_str(),
            Material {
                selection: value,
                fitted: material_calibration::fitted_domain(resolved.material())?
                    .map(|domain| domain.ranges()),
                nonlinear: nonlinear.as_ref().map(|law| law.strain_range()),
                area_ratio: nonlinear.as_ref().and_then(|law| law.area_ratio_range()),
            },
        );
    }
    let mut domains = BTreeMap::new();
    let mut instance_materials = BTreeMap::new();
    let mut used_materials = BTreeSet::new();
    for name in array(source, "instances")? {
        let name = name.as_str().ok_or_else(fail)?;
        let piece = &pattern.instance(name).ok_or_else(fail)?.piece;
        let selection = changes
            .pieces
            .get(piece)
            .or(changes.default.as_ref())
            .ok_or_else(fail)?;
        let identity = if per_piece {
            &material_report["instances"][names[name]]["record_sha256"]
        } else {
            &material_report["record_sha256"]
        };
        let identity = identity.as_str().ok_or_else(fail)?;
        let material = materials.get(identity).ok_or_else(fail)?;
        if serde_json::to_value(catalog.resolve(selection)?.frozen_selection())
            .map_err(|_| fail())?
            != *material.selection
        {
            return Err(fail());
        }
        domains.insert(name, [material.fitted, material.nonlinear]);
        instance_materials.insert(name, identity);
        used_materials.insert(identity);
    }
    if used_materials.len() != materials.len() {
        return Err(fail());
    }
    let expected = owners
        .iter()
        .enumerate()
        .filter_map(|(i, owner)| {
            let name = all_instances[owner.as_u64().expect("checked owner") as usize]
                .as_str()
                .expect("checked name");
            regional
                .as_ref()
                .map_or_else(
                    || selected.contains(name),
                    |scope| scope.triangle_regions.contains_key(&i),
                )
                .then_some((i, name))
        })
        .collect::<Vec<_>>();
    let rows = array(source, "triangles")?;
    if rows.len() != expected.len() {
        return Err(fail());
    }
    let mut triangles = BTreeMap::new();
    let mut area_by_instance = BTreeMap::<&str, f64>::new();
    let mut area_by_region = BTreeMap::<&str, f64>::new();
    for (row, (i, name)) in rows.iter().zip(expected) {
        let vertices: [usize; 3] =
            Deserialize::deserialize(&row["vertices"]).map_err(|_| fail())?;
        let gradients: [[f64; 2]; 3] =
            Deserialize::deserialize(&row["material_gradients_m_inv"]).map_err(|_| fail())?;
        let area = finite(&row["reference_area_m2"])
            .filter(|area| *area > 0.)
            .ok_or_else(fail)?;
        let identity = instance_materials[name];
        let material = &materials[identity];
        let determinant = gradients[1][0] * gradients[2][1] - gradients[1][1] * gradients[2][0];
        if index(&row["source_triangle"])? != i
            || row["instance"] != name
            || row["material_sha256"] != identity
            || row["vertices"] != faces[i]
            || vertices.iter().any(|&v| v >= vertex_count)
            || vertices[0] == vertices[1]
            || vertices[1] == vertices[2]
            || vertices[2] == vertices[0]
            || gradients.iter().flatten().any(|v| !v.is_finite())
            || !determinant.is_finite()
            || determinant == 0.
            || gradients[0]
                != [
                    -gradients[1][0] - gradients[2][0],
                    -gradients[1][1] - gradients[2][1],
                ]
            || row["fitted_strain_range"] != json!(material.fitted)
            || row["nonlinear_strain_range"] != json!(material.nonlinear)
            || row["physical_area_ratio_range"] != json!(material.area_ratio)
        {
            return Err(fail());
        }
        if let Some(regional) = &regional {
            let id = &regional.triangle_regions[&i];
            let flat_area = regional.flat_areas_m2[i];
            if row["region"] != *id || (flat_area - area).abs() > 1e-10 * flat_area.max(area) {
                return Err(fail());
            }
            *area_by_region.entry(id).or_default() += area;
        } else if row.get("region").is_some() {
            return Err(fail());
        }
        *area_by_instance.entry(name).or_default() += area;
        triangles.insert(i, row);
    }
    if area_by_instance.values().any(|area| !area.is_finite())
        || area_by_region.iter().any(|(id, area)| {
            !area.is_finite() || finite(&source["regions"][*id]["reference_area_m2"]) != Some(*area)
        })
        || (per_piece
            && regional.is_none()
            && area_by_instance.iter().any(|(name, area)| {
                finite(&material_report["instances"][names[name]]["rest_area_m2"]) != Some(*area)
            }))
    {
        return Err(fail());
    }
    Ok(Source {
        vertex_count,
        triangles,
        domains,
        regions: regional.map_or_else(BTreeMap::new, |scope| scope.regions),
    })
}

fn check_summary(
    summary: &Summary,
    frames: &[Frame],
    source: &Source<'_>,
    instance: Option<&str>,
    region: Option<&str>,
) -> Result<(), Refusal> {
    for i in 0..3 {
        for (side, witness) in [
            (0, &summary.minimum_witness[i]),
            (1, &summary.maximum_witness[i]),
        ] {
            let frame = frames.get(witness.sample_index).ok_or_else(fail)?;
            let triangle = source
                .triangles
                .get(&witness.source_triangle)
                .ok_or_else(fail)?;
            let [lo, hi] = frame.green_strain_range[i];
            if !witness.value.is_finite()
                || witness.value != summary.green_strain_range[i][side]
                || witness.time_s != frame.time_s
                || witness.value < lo
                || witness.value > hi
                || instance.is_some_and(|id| triangle["instance"] != id)
                || region.is_some_and(|id| triangle["region"] != id)
                || (witness.value == lo && witness.source_triangle < frame.minimum_triangle[i])
                || (witness.value == hi && witness.source_triangle < frame.maximum_triangle[i])
            {
                return Err(fail());
            }
        }
    }
    if let Some(id) = instance {
        // Each local envelope must include every published frame extremum owned by this instance.
        // Equal extrema must retain the earliest sample and then the lowest original triangle ID.
        for frame in frames {
            let observed = frame.summary();
            for i in 0..3 {
                for witness in [&observed.minimum_witness[i], &observed.maximum_witness[i]] {
                    let triangle = source.triangles[&witness.source_triangle];
                    if triangle["instance"] != id
                        || region.is_some_and(|id| triangle["region"] != id)
                    {
                        continue;
                    }
                    if witness.value < summary.green_strain_range[i][0]
                        || witness.value > summary.green_strain_range[i][1]
                        || (witness.value == summary.green_strain_range[i][0]
                            && witness.order() < summary.minimum_witness[i].order())
                        || (witness.value == summary.green_strain_range[i][1]
                            && witness.order() < summary.maximum_witness[i].order())
                    {
                        return Err(fail());
                    }
                }
            }
        }
        for domain in source.domains[id].into_iter().flatten() {
            for ([lo, hi], [domain_lo, domain_hi]) in
                summary.green_strain_range.into_iter().zip(domain)
            {
                if lo < domain_lo || hi > domain_hi {
                    return Err(fail());
                }
            }
        }
    }
    if let Some(region) = region {
        let first = source
            .triangles
            .iter()
            .find_map(|(index, triangle)| (triangle["region"] == region).then_some(*index))
            .ok_or_else(fail)?;
        for (axis, [lo, hi]) in summary.green_strain_range.iter().enumerate() {
            if lo == hi
                && (summary.minimum_witness[axis].order() != (0, first)
                    || summary.maximum_witness[axis].order() != (0, first))
            {
                return Err(fail());
            }
        }
    }
    Ok(())
}

/// The caller first admits the drape and, when present, the complete prescribed motion.
pub(super) fn admit<'a>(
    alternative: &Alternative,
    descriptor: &Value,
    simulation: &'a Value,
    motion: Option<&'a Value>,
) -> Result<&'a Value, Refusal> {
    let policy = &descriptor["scenario"]["material_strain"];
    let request = policy.as_object().ok_or_else(fail)?;
    let allowance = index(&policy["max_triangle_samples"])?;
    let outcome = &motion.unwrap_or(simulation)["material_strain"];
    let report = &outcome["result"];
    let regional = policy["schema"] == "ftx-material-strain/2";
    if (!regional && policy["schema"] != "ftx-material-strain/1")
        || !(1..=64_000_000).contains(&allowance)
        || request.keys().any(|key| {
            ![
                "schema",
                if regional { "regions" } else { "instances" },
                "max_triangle_samples",
            ]
            .contains(&key.as_str())
        })
        || outcome["status"] != "available"
        || report["schema"]
            != if regional {
                "ftx-material-strain-report/2"
            } else {
                "ftx-material-strain-report/1"
            }
        || report["request"] != *policy
        || report["scope"]
            != if motion.is_some() {
                "accepted_motion_samples"
            } else {
                "initial_state"
            }
        || report["strain_convention"] != "material_green_E11_E22_2E12"
        || report["unit"] != "1"
        || [
            "physical_validation_claim",
            "fit_claim",
            "safe_strain_claim",
            "continuous_time_bound",
        ]
        .iter()
        .any(|key| report[*key] != false)
    {
        return Err(fail());
    }
    let initial = motion
        .map(|_| admit(alternative, descriptor, simulation, None))
        .transpose()?;
    let source = source(alternative, policy, simulation, report)?;
    let rows = array(report, "frames")?;
    let samples = motion.map_or(Ok(&[][..]), |m| array(m, "samples"))?;
    if rows.len() != samples.len() + 1
        || rows.len() > 513
        || source.vertex_count.saturating_mul(rows.len()) > 2_000_000
        || source.triangles.len().saturating_mul(rows.len()) > allowance
        || index(&report["triangle_samples"])? != source.triangles.len().saturating_mul(rows.len())
        || initial.is_some_and(|initial| {
            report["source"] != initial["source"]
                || report["source_sha256"] != initial["source_sha256"]
                || rows[0] != initial["frames"][0]
        })
    {
        return Err(fail());
    }
    if let Some(motion) = motion
        && (samples.is_empty()
            || simulation["triangles"] != motion["triangles"]
            || motion["initial_positions_sha256"] != simulation["positions_sha256"]
            || motion["initial_sample"]["positions_m"] != simulation["positions_m"]
            || motion["initial_sample"]["time_s"].as_f64() != Some(0.))
    {
        return Err(fail());
    }
    let mut frames = Vec::with_capacity(rows.len());
    let mut aggregate: Option<Summary> = None;
    let mut previous = 0.;
    let first_triangle = *source.triangles.keys().next().ok_or_else(fail)?;
    for (i, row) in rows.iter().enumerate() {
        let frame = Frame::deserialize(row).map_err(|_| fail())?;
        let positions = if i == 0 {
            &simulation["positions_m"]
        } else {
            &samples[i - 1]["positions_m"]
        };
        let time = if i == 0 {
            0.
        } else {
            finite(&samples[i - 1]["time_s"]).ok_or_else(fail)?
        };
        let coordinates = positions.as_array().ok_or_else(fail)?;
        if coordinates.len() != source.vertex_count
            || coordinates.iter().any(|p| {
                !p.as_array()
                    .is_some_and(|v| v.len() == 3 && v.iter().all(|x| finite(x).is_some()))
            })
            || frame.sample_index != i
            || frame.time_s != time
            || (i > 0 && (time <= previous || samples[i - 1]["solve"]["converged"] != true))
            || frame.positions_sha256 != hash(positions)?
            || !valid_ranges(&frame.green_strain_range)
            || frame
                .minimum_triangle
                .iter()
                .chain(&frame.maximum_triangle)
                .any(|id| !source.triangles.contains_key(id))
            || (0..3).any(|axis| {
                frame.green_strain_range[axis][0] == frame.green_strain_range[axis][1]
                    && (frame.minimum_triangle[axis] != first_triangle
                        || frame.maximum_triangle[axis] != first_triangle)
            })
        {
            return Err(fail());
        }
        previous = time;
        let summary = frame.summary();
        aggregate
            .get_or_insert_with(|| summary.clone())
            .include(&summary);
        frames.push(frame);
    }
    if report["initial_positions_sha256"] != simulation["positions_sha256"]
        || report["initial_positions_sha256"] != frames[0].positions_sha256
        || report["final_positions_sha256"] != frames.last().ok_or_else(fail)?.positions_sha256
        || finite(&report["duration_s"]) != Some(previous)
        || motion.is_some_and(|m| {
            m["final_positions_sha256"] != report["final_positions_sha256"]
                || finite(&m["duration_s"]) != Some(previous)
        })
    {
        return Err(fail());
    }
    let reported = Summary::read(report)?;
    check_summary(&reported, &frames, &source, None, None)?;
    if aggregate.as_ref() != Some(&reported) {
        return Err(fail());
    }
    let instances = report["instances"].as_object().ok_or_else(fail)?;
    if instances.len() != source.domains.len() {
        return Err(fail());
    }
    let mut aggregate: Option<Summary> = None;
    for (id, value) in instances {
        if !source.domains.contains_key(id.as_str()) {
            return Err(fail());
        }
        let summary = Summary::read(value)?;
        check_summary(&summary, &frames, &source, Some(id), None)?;
        if let Some(initial) = initial {
            let before = Summary::read(&initial["instances"][id])?;
            let mut including_initial = summary.clone();
            including_initial.include(&before);
            if including_initial != summary {
                return Err(fail());
            }
        }
        aggregate
            .get_or_insert_with(|| summary.clone())
            .include(&summary);
    }
    if aggregate.as_ref() != Some(&reported) {
        return Err(fail());
    }
    if regional {
        let regions = report["regions"].as_object().ok_or_else(fail)?;
        if regions.len() != source.regions.len() {
            return Err(fail());
        }
        let mut by_instance = BTreeMap::<&str, Summary>::new();
        for (id, value) in regions {
            let instance = source.regions.get(id).ok_or_else(fail)?;
            let summary = Summary::read(value)?;
            check_summary(&summary, &frames, &source, Some(instance), Some(id))?;
            if let Some(initial) = initial {
                let before = Summary::read(&initial["regions"][id])?;
                let mut including_initial = summary.clone();
                including_initial.include(&before);
                if including_initial != summary {
                    return Err(fail());
                }
            }
            by_instance
                .entry(instance)
                .or_insert_with(|| summary.clone())
                .include(&summary);
        }
        if by_instance.len() != instances.len()
            || by_instance.iter().any(|(instance, summary)| {
                !Summary::read(&report["instances"][*instance])
                    .is_ok_and(|actual| actual == *summary)
            })
        {
            return Err(fail());
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material_study::{Metrics, Outcome};
    use ftx_pattern::{Pattern, material_binding::Changes};

    // Synthetic receipt arithmetic only. Native producer tests establish the actual material-frame
    // kinematics; these tests make no physical claim about the supplied comparison callback.
    fn case(per_piece: bool, scoped: bool) -> (Alternative, Value, Value, Value) {
        let raw: Value = serde_json::from_str(include_str!(
            "../../../../../tests/fixtures/materials/synthetic-response.json"
        ))
        .unwrap();
        let catalog = Catalog {
            schema: CATALOG_SCHEMA.into(),
            materials: vec![],
        };
        let first = catalog
            .resolve(&serde_json::from_value::<Selection>(raw.clone()).unwrap())
            .unwrap();
        let mut other = raw;
        other["id"] = json!("synthetic-second-sheet");
        other["record"]["id"] = other["id"].clone();
        let second = catalog
            .resolve(&serde_json::from_value::<Selection>(other).unwrap())
            .unwrap();
        let records = [&first, if per_piece { &second } else { &first }];
        let changes = Changes {
            schema: "ftx-material-bindings/1".into(),
            default: Some(first.frozen_selection()),
            pieces: if per_piece {
                BTreeMap::from([("back".into(), second.frozen_selection())])
            } else {
                BTreeMap::new()
            },
            ..Changes::default()
        };
        let pattern = Pattern::from_json(include_str!(
            "../../../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
        ))
        .unwrap();
        let alternative = Alternative {
            key: "synthetic".into(),
            choices: BTreeMap::new(),
            design: None,
            binding: Outcome::Available {
                result: changes.apply(&pattern).unwrap(),
            },
            frozen_changes: Some(changes),
            cutting: Outcome::NotRequested,
            finite_stock: Outcome::NotRequested,
            cutting_job: Outcome::NotRequested,
            sewing: Outcome::NotRequested,
            sewing_selection: Outcome::NotRequested,
            purchasing: Outcome::NotRequested,
            cut_and_sew_purchasing: Outcome::NotRequested,
            assembly_mass: Outcome::NotRequested,
            vapor: Outcome::NotRequested,
            metrics: Metrics::default(),
            cutting_effort: Default::default(),
        };
        let ownership = json!({"instances":["front","back"], "triangle_instance":[0,1,0]});
        let material = if per_piece {
            let receipts = records
                .iter()
                .map(|r| {
                    (
                        r.sha256().to_owned(),
                        json!({"record":r,"record_sha256":r.sha256()}),
                    )
                })
                .collect::<serde_json::Map<String, Value>>();
            json!({"basis":"per_piece_characterization","records":receipts,
                "triangle_instance":ownership["triangle_instance"],
                "instances":[{"instance":"front","record_sha256":first.sha256(),"rest_area_m2":1.},
                {"instance":"back","record_sha256":second.sha256(),"rest_area_m2":0.5}]})
        } else {
            json!({"basis":"characterized_record","record":first,"record_sha256":first.sha256()})
        };
        let faces = json!([[0, 1, 2], [3, 4, 5], [0, 2, 6]]);
        let mut policy = json!({"schema":"ftx-material-strain/1","max_triangle_samples":100});
        if scoped {
            policy["instances"] = json!(["front"]);
        }
        let chosen = if scoped { vec![0, 2] } else { vec![0, 1, 2] };
        let source_faces: Vec<_> = chosen.iter().map(|&i| {
            let owner = usize::from(i == 1);
            let record = records[owner];
            json!({"source_triangle":i,"instance":if owner == 0 {"front"} else {"back"},
                "material_sha256":record.sha256(),"vertices":faces[i],"reference_area_m2":0.5,
                "material_gradients_m_inv":[[-1.,-1.],[1.,0.],[0.,1.]],
                "fitted_strain_range":null,"physical_area_ratio_range":null,
                "nonlinear_strain_range":record.membrane_response().unwrap().unwrap().strain_range()})
        }).collect();
        let mut materials = serde_json::Map::new();
        for &i in &chosen {
            let record = records[usize::from(i == 1)];
            materials.insert(record.sha256().into(), json!(record.frozen_selection()));
        }
        let source = json!({"vertex_count":7,
            "instances":if scoped {json!(["front"])} else {json!(["back","front"])},
            "materials":materials,"triangles":source_faces,
            "material_report_sha256":hash(&material).unwrap()});
        let positions: Vec<_> = (0..3)
            .map(|step| {
                json!([
                    [0., 0., 0.],
                    [1., 0., 0.],
                    [0., 1., 0.],
                    [2., 0., 0.],
                    [3., 0., 0.],
                    [2., 1., 0.],
                    [step as f64, 2., 0.]
                ])
            })
            .collect();
        let readings = [
            [[0.2, -0.1, 0.3], [-0.2, 0.4, -0.1], [0.2, 0.1, 0.15]],
            [[0.3, -0.1, 0.2], [-0.2, 0.5, -0.2], [0.3, 0.2, 0.1]],
            [[0.2, -0.1, 0.3], [-0.2, 0.4, -0.1], [0.2, 0.1, 0.15]],
        ];
        let report = |moving: bool| {
            let mut overall: Option<Summary> = None;
            let mut instances: BTreeMap<&str, Summary> = BTreeMap::new();
            let mut frames = Vec::new();
            for step in 0..if moving { 3 } else { 1 } {
                let mut frame: Option<Summary> = None;
                for &id in &chosen {
                    let values = readings[step][id];
                    let witnesses = std::array::from_fn(|axis| Witness {
                        value: values[axis],
                        source_triangle: id,
                        sample_index: step,
                        time_s: step as f64,
                    });
                    let single = Summary {
                        green_strain_range: values.map(|v| [v, v]),
                        minimum_witness: witnesses.clone(),
                        maximum_witness: witnesses,
                    };
                    overall
                        .get_or_insert_with(|| single.clone())
                        .include(&single);
                    frame.get_or_insert_with(|| single.clone()).include(&single);
                    instances
                        .entry(if id == 1 { "back" } else { "front" })
                        .or_insert_with(|| single.clone())
                        .include(&single);
                }
                let frame = frame.unwrap();
                frames.push(json!({"sample_index":step,"time_s":step as f64,
                    "positions_sha256":hash(&positions[step]).unwrap(),
                    "green_strain_range":frame.green_strain_range,
                    "minimum_triangle":frame.minimum_witness.map(|w| w.source_triangle),
                    "maximum_triangle":frame.maximum_witness.map(|w| w.source_triangle)}));
            }
            let overall = overall.unwrap();
            json!({"schema":"ftx-material-strain-report/1","request":policy,
                "scope":if moving {"accepted_motion_samples"} else {"initial_state"},
                "strain_convention":"material_green_E11_E22_2E12","unit":"1",
                "source":source,"source_sha256":hash(&source).unwrap(),
                "initial_positions_sha256":frames[0]["positions_sha256"],
                "final_positions_sha256":frames.last().unwrap()["positions_sha256"],
                "duration_s":if moving {2.} else {0.},
                "green_strain_range":overall.green_strain_range,
                "minimum_witness":overall.minimum_witness,"maximum_witness":overall.maximum_witness,
                "instances":instances,"triangle_samples":chosen.len()*frames.len(),"frames":frames,
                "physical_validation_claim":false,"fit_claim":false,"safe_strain_claim":false,
                "continuous_time_bound":false})
        };
        let simulation = json!({"drape":{"material":material,"thermal_surface":{"available":false}},
            "positions_m":positions[0],"positions_sha256":hash(&positions[0]).unwrap(),
            "triangles":faces,"material_strain_ownership":ownership,
            "material_strain":{"status":"available","result":report(false)}});
        let motion = json!({"triangles":faces,"duration_s":2.,
            "initial_positions_sha256":hash(&positions[0]).unwrap(),
            "final_positions_sha256":hash(&positions[2]).unwrap(),
            "initial_sample":{"time_s":0.,"positions_m":positions[0]},
            "samples":[{"time_s":1.,"positions_m":positions[1],"solve":{"converged":true}},
                {"time_s":2.,"positions_m":positions[2],"solve":{"converged":true}}],
            "material_strain":{"status":"available","result":report(true)}});
        (
            alternative,
            json!({"scenario":{"material_strain":policy}}),
            simulation,
            motion,
        )
    }

    fn comparison_case(scoped: bool) -> (Alternative, Value, Value) {
        let (a, mut d, mut s, mut m) = case(true, scoped);
        d["scenario_sha256"] = json!("a".repeat(64));
        d["body_geometry_sha256"] = json!("b".repeat(64));
        d["motion_path"] = json!({"sha256":"c".repeat(64)});
        d["scenario"]["motion"] = json!({"schema":"ftx-drape-motion/1","max_trials":2,
            "keyframes":[{"time_s":1.,"arm_abduction_deg":0.},{"time_s":2.,"arm_abduction_deg":0.}]});
        s["schema"] = json!("ftx-drape-scenario-result/1");
        s["pattern_sha256"] = a.binding.value().unwrap()["pattern_sha256"].clone();
        s["scenario_sha256"] = d["scenario_sha256"].clone();
        s["body_geometry_sha256"] = d["body_geometry_sha256"].clone();
        s["drape"]["settled"] = json!(true);
        s["drape"]["gating"] = json!([]);
        s["metrics"] = json!({"simulation_gating_clear":true});
        m["schema"] = json!("ftx-drape-motion-result/1");
        m["path_sha256"] = d["motion_path"]["sha256"].clone();
        m["continuous_contact_checked"] = json!(true);
        m["initial_drape_settled"] = json!(true);
        m["initial_drape_gating"] = json!([]);
        m["trials"] = json!(2);
        m["body_triangles"] = json!([[0, 1, 2]]);
        let body = json!([[0., 0., -1.], [1., 0., -1.], [0., 1., -1.]]);
        let mut endpoints = Vec::new();
        for i in 0..2 {
            m["samples"][i]["body_positions_m"] = body.clone();
            endpoints.push(json!({"sample_index":i,"time_s":(i+1) as f64,
                "arm_abduction_deg":0.,"positions_sha256":hash(&m["samples"][i]["positions_m"]).unwrap(),
                "body_collision_sha256":hash(&(&body,&m["body_triangles"])).unwrap()}));
        }
        m["keyframes"] = json!(endpoints);
        s["motion"] = json!({"status":"available","result":m});
        (a, d, s)
    }

    fn regional_case(per_piece: bool, selected: &[usize]) -> (Alternative, Value, Value, Value) {
        let (a, mut d, mut s, mut m) = case(per_piece, true);
        let regions: Vec<_> = selected
            .iter()
            .map(|&index| {
                let (id, low, high) = if index == 0 {
                    ("right", 100., 1100.)
                } else {
                    assert_eq!(index, 2);
                    ("left", -1100., -100.)
                };
                json!({"id":id,"instance":"front","selector":{
                    "kind":"whole_triangle_centroid",
                    "polygon_mm":[[low,-100.],[high,-100.],[high,1100.],[low,1100.]]}})
            })
            .collect();
        let policy = json!({"schema":"ftx-material-strain/2","regions":regions,
            "max_triangle_samples":100});
        d["scenario"]["material_strain"] = policy.clone();
        s["material_strain_ownership"]["rest_triangles_mm"] = json!([
            [[0., 0.], [1000., 0.], [0., 1000.]],
            [[0., 0.], [1000., 0.], [0., 1000.]],
            [[0., 0.], [0., 1000.], [-1000., 0.]]
        ]);
        let mut source = s["material_strain"]["result"]["source"].clone();
        source["triangles"]
            .as_array_mut()
            .unwrap()
            .retain(|row| selected.contains(&(row["source_triangle"].as_u64().unwrap() as usize)));
        source["regions"] = json!({});
        for (region, &index) in regions.iter().zip(selected) {
            let id = region["id"].as_str().unwrap();
            source["regions"][id] = json!({"instance":"front","selector":region["selector"],
                "source_triangles":[index],"reference_area_m2":0.5});
            for row in source["triangles"].as_array_mut().unwrap() {
                if row["source_triangle"] == index {
                    row["region"] = json!(id);
                }
            }
        }
        let positions = [
            s["positions_m"].clone(),
            m["samples"][0]["positions_m"].clone(),
            m["samples"][1]["positions_m"].clone(),
        ];
        let readings = [
            [[0.2, -0.1, 0.3], [0., 0., 0.], [0.2, 0.1, 0.15]],
            [[0.3, -0.1, 0.2], [0., 0., 0.], [0.3, 0.2, 0.1]],
            [[0.2, -0.1, 0.3], [0., 0., 0.], [0.2, 0.1, 0.15]],
        ];
        for (moving, container) in [(false, &mut s), (true, &mut m)] {
            let mut summaries = BTreeMap::<&str, Summary>::new();
            let mut frames = Vec::new();
            let mut overall: Option<Summary> = None;
            for step in 0..if moving { 3 } else { 1 } {
                let mut frame: Option<Summary> = None;
                for &index in selected {
                    let values = readings[step][index];
                    let witnesses = std::array::from_fn(|axis| Witness {
                        value: values[axis],
                        source_triangle: index,
                        sample_index: step,
                        time_s: step as f64,
                    });
                    let summary = Summary {
                        green_strain_range: values.map(|value| [value, value]),
                        minimum_witness: witnesses.clone(),
                        maximum_witness: witnesses,
                    };
                    summaries
                        .entry(if index == 0 { "right" } else { "left" })
                        .or_insert_with(|| summary.clone())
                        .include(&summary);
                    frame
                        .get_or_insert_with(|| summary.clone())
                        .include(&summary);
                    overall
                        .get_or_insert_with(|| summary.clone())
                        .include(&summary);
                }
                let frame = frame.unwrap();
                frames.push(json!({"sample_index":step,"time_s":step as f64,
                    "positions_sha256":hash(&positions[step]).unwrap(),
                    "green_strain_range":frame.green_strain_range,
                    "minimum_triangle":frame.minimum_witness.map(|w|w.source_triangle),
                    "maximum_triangle":frame.maximum_witness.map(|w|w.source_triangle)}));
            }
            let report = &mut container["material_strain"]["result"];
            let overall = overall.unwrap();
            report["schema"] = json!("ftx-material-strain-report/2");
            report["request"] = policy.clone();
            report["source"] = source.clone();
            report["source_sha256"] = json!(hash(&source).unwrap());
            report["regions"] = json!(summaries);
            report["instances"] = json!({"front":overall});
            report["green_strain_range"] = json!(overall.green_strain_range);
            report["minimum_witness"] = json!(overall.minimum_witness);
            report["maximum_witness"] = json!(overall.maximum_witness);
            report["triangle_samples"] = json!(selected.len() * frames.len());
            report["frames"] = json!(frames);
        }
        (a, d, s, m)
    }

    fn regional_comparison_case(selected: &[usize]) -> (Alternative, Value, Value) {
        let (a, mut d, mut s) = comparison_case(true);
        let (_, regional_d, regional_s, regional_m) = regional_case(true, selected);
        d["scenario"]["material_strain"] = regional_d["scenario"]["material_strain"].clone();
        s["material_strain_ownership"] = regional_s["material_strain_ownership"].clone();
        s["material_strain"] = regional_s["material_strain"].clone();
        s["motion"]["result"]["material_strain"] = regional_m["material_strain"].clone();
        (a, d, s)
    }

    #[test]
    fn comparison_constraints_use_all_twelve_signed_raw_metrics_and_keep_static_refusal_isolation()
    {
        let (a, d, mut s) = comparison_case(false);
        let values = [
            (Metric::MinWarpGreenStrain, -0.2),
            (Metric::MaxWarpGreenStrain, 0.2),
            (Metric::MinWeftGreenStrain, -0.1),
            (Metric::MaxWeftGreenStrain, 0.4),
            (Metric::MinEngineeringShearGreenStrain, -0.1),
            (Metric::MaxEngineeringShearGreenStrain, 0.3),
            (Metric::MotionMinWarpGreenStrain, -0.2),
            (Metric::MotionMaxWarpGreenStrain, 0.3),
            (Metric::MotionMinWeftGreenStrain, -0.1),
            (Metric::MotionMaxWeftGreenStrain, 0.5),
            (Metric::MotionMinEngineeringShearGreenStrain, -0.2),
            (Metric::MotionMaxEngineeringShearGreenStrain, 0.3),
        ];
        let mut policy = Policy {
            basis: Basis::Nominal,
            objectives: values
                .iter()
                .map(|&(metric, _)| Objective {
                    metric,
                    direction: Direction::Minimize,
                })
                .collect(),
            constraints: values
                .iter()
                .map(|&(metric, value)| Constraint {
                    metric,
                    basis: None,
                    minimum: Some(value),
                    maximum: Some(value),
                })
                .collect(),
            required_consumers: vec![],
            allow_mixed_protocols: false,
        };
        let admitted = policy.candidate(&a, None, &d, &s);
        assert!(admitted.eligible, "{}", json!(admitted));
        for (metric, value) in values {
            assert_eq!(admitted.readings[&metric].value, value);
            assert_eq!(admitted.readings[&metric].unit, "1");
            assert!(!metric.supports_bounds());
        }
        let report = policy.finish(vec![admitted], "synthetic source", "synthetic context");
        assert_eq!(report.frontier, vec![a.key.clone()]);
        assert!(report.motion_required);
        assert!(!report.physical_validation_claim);
        for (i, (_, value)) in values.iter().enumerate() {
            policy.constraints[i].minimum = None;
            policy.constraints[i].maximum = Some(value - 0.001);
            let excluded = policy.candidate(&a, None, &d, &s);
            assert!(!excluded.eligible);
            assert_eq!(excluded.readings.len(), 12);
            assert!(
                policy
                    .finish(vec![excluded], "source", "context")
                    .frontier
                    .is_empty()
            );
            policy.constraints[i].minimum = Some(*value);
            policy.constraints[i].maximum = Some(*value);
        }
        s["motion"]["result"]["material_strain"]["status"] = json!("refused");
        let refused = policy.candidate(&a, None, &d, &s);
        assert!(!refused.eligible);
        assert_eq!(refused.readings.len(), 6);
        assert_eq!(refused.readings[&Metric::MinWeftGreenStrain].value, -0.1);
        policy.objectives.truncate(6);
        policy.constraints.truncate(6);
        assert!(!policy.needs_motion());
        assert!(policy.candidate(&a, None, &d, &s).eligible);
        policy
            .required_consumers
            .push(Consumer::MotionMaterialStrain);
        assert!(policy.needs_motion());
        assert!(!policy.candidate(&a, None, &d, &s).eligible);
    }

    #[test]
    fn comparison_protocol_keeps_explicit_material_strain_scope() {
        let policy: Policy = serde_json::from_value(json!({"basis":"nominal",
            "objectives":[{"metric":"max_warp_green_strain","direction":"minimize"}]}))
        .unwrap();
        let (a, d, s) = comparison_case(false);
        let all = policy.candidate(&a, None, &d, &s);
        let (a, d, s) = comparison_case(true);
        let front = policy.candidate(&a, None, &d, &s);
        assert!(all.eligible && front.eligible);
        assert_eq!(
            all.readings[&Metric::MaxWarpGreenStrain].value,
            front.readings[&Metric::MaxWarpGreenStrain].value
        );
        assert_ne!(all.input_protocol_sha256, front.input_protocol_sha256);
    }

    #[test]
    fn regional_strain_admits_partial_panels_and_complete_disjoint_sample_histories() {
        for per_piece in [false, true] {
            for selected in [&[2][..], &[0, 2][..]] {
                let (a, d, s, m) = regional_case(per_piece, selected);
                let initial = admit(&a, &d, &s, None).unwrap();
                let moving = admit(&a, &d, &s, Some(&m)).unwrap();
                assert_eq!(initial["triangle_samples"], json!(selected.len()));
                assert_eq!(moving["triangle_samples"], json!(3 * selected.len()));
                assert_eq!(moving["source"]["instances"], json!(["front"]));
                assert_eq!(
                    moving["regions"]["left"]["green_strain_range"][2],
                    json!([0.1, 0.15])
                );
                assert_eq!(
                    moving["regions"]["left"]["maximum_witness"][2]["sample_index"],
                    0
                );
                assert_eq!(
                    moving["regions"]["left"]["minimum_witness"][2]["sample_index"],
                    1
                );
                assert_eq!(
                    initial["green_strain_range"][2][1],
                    if selected.len() == 1 { 0.15 } else { 0.3 }
                );
                let mut capped_d = d.clone();
                let mut capped_s = s.clone();
                let mut capped_m = m.clone();
                capped_d["scenario"]["material_strain"]["max_triangle_samples"] =
                    json!(selected.len());
                capped_s["material_strain"]["result"]["request"] =
                    capped_d["scenario"]["material_strain"].clone();
                capped_m["material_strain"]["result"]["request"] =
                    capped_d["scenario"]["material_strain"].clone();
                assert!(admit(&a, &capped_d, &capped_s, None).is_ok());
                assert!(admit(&a, &capped_d, &capped_s, Some(&capped_m)).is_err());
            }
        }
    }

    #[test]
    fn regional_strain_reconstructs_membership_and_refuses_rehashed_source_tampering() {
        let (a, d, s, _) = regional_case(true, &[0, 2]);
        for mutation in 0..12 {
            let mut changed = s.clone();
            match mutation {
                0 => {
                    changed["material_strain"]["result"]["source"]["triangles"]
                        .as_array_mut()
                        .unwrap()
                        .pop();
                }
                1 => {
                    changed["material_strain"]["result"]["source"]["regions"]["left"]["source_triangles"] =
                        json!([0])
                }
                2 => {
                    changed["material_strain"]["result"]["source"]["regions"]["left"]["instance"] =
                        json!("back")
                }
                3 => {
                    changed["material_strain"]["result"]["source"]["regions"]["left"]["selector"]
                        ["polygon_mm"][0][0] = json!(-1200.)
                }
                4 => {
                    changed["material_strain"]["result"]["source"]["regions"]["left"]["reference_area_m2"] =
                        json!(1.)
                }
                5 => {
                    changed["material_strain"]["result"]["source"]["triangles"][1]["region"] =
                        json!("right")
                }
                6 => {
                    changed["material_strain_ownership"]["rest_triangles_mm"][2] =
                        changed["material_strain_ownership"]["rest_triangles_mm"][0].clone()
                }
                7 => {
                    changed["material_strain_ownership"]["rest_triangles_mm"]
                        .as_array_mut()
                        .unwrap()
                        .pop();
                }
                8 => {
                    changed["material_strain_ownership"]["rest_triangles_mm"][1] =
                        json!([[0., 0.], [0., 0.], [0., 0.]])
                }
                9 => {
                    changed["material_strain"]["result"]["source"]["triangles"][0]["reference_area_m2"] =
                        json!(0.25)
                }
                10 => {
                    changed["material_strain"]["result"]["source"]["regions"]
                        .as_object_mut()
                        .unwrap()
                        .remove("left");
                }
                _ => {
                    changed["material_strain"]["result"]["source"]["regions"]["extra"] =
                        changed["material_strain"]["result"]["source"]["regions"]["left"].clone()
                }
            }
            let report = &mut changed["material_strain"]["result"];
            report["source_sha256"] = json!(hash(&report["source"]).unwrap());
            assert!(
                admit(&a, &d, &changed, None).is_err(),
                "mutation {mutation}"
            );
        }
    }

    #[test]
    fn regional_strain_rejects_empty_overlap_unknown_and_mixed_schema_requests() {
        let (a, d, s, _) = regional_case(true, &[0, 2]);
        for mutation in 0..8 {
            let mut changed_d = d.clone();
            let mut changed_s = s.clone();
            let policy = &mut changed_d["scenario"]["material_strain"];
            match mutation {
                0 => policy["instances"] = json!(["front"]),
                1 => policy["regions"][1]["id"] = policy["regions"][0]["id"].clone(),
                2 => policy["regions"][1]["selector"] = policy["regions"][0]["selector"].clone(),
                3 => {
                    policy["regions"][0]["selector"]["polygon_mm"] =
                        json!([[5000., 0.], [6000., 0.], [6000., 1000.], [5000., 1000.]])
                }
                4 => policy["regions"][0]["instance"] = json!("missing"),
                5 => policy["regions"][0]["unexpected"] = json!(true),
                6 => policy["regions"] = json!([]),
                _ => policy["schema"] = json!("ftx-material-strain/1"),
            }
            changed_s["material_strain"]["result"]["request"] = policy.clone();
            assert!(
                admit(&a, &changed_d, &changed_s, None).is_err(),
                "mutation {mutation}"
            );
        }
    }

    #[test]
    fn regional_strain_checks_local_witnesses_and_region_to_instance_reductions() {
        let (a, d, s, m) = regional_case(true, &[0, 2]);
        for mutation in 0..7 {
            let mut changed = m.clone();
            let report = &mut changed["material_strain"]["result"];
            match mutation {
                0 => report["regions"]["left"]["maximum_witness"][2]["source_triangle"] = json!(0),
                1 => report["regions"]["left"]["maximum_witness"][2]["time_s"] = json!(1.),
                2 => report["regions"]["left"]["green_strain_range"][1][1] = json!(0.3),
                3 => {
                    report["regions"].as_object_mut().unwrap().remove("right");
                }
                4 => report["regions"]["extra"] = report["regions"]["right"].clone(),
                5 => {
                    report["regions"]["right"]["minimum_witness"][1]["sample_index"] = json!(1);
                    report["regions"]["right"]["minimum_witness"][1]["time_s"] = json!(1.);
                }
                _ => {
                    report["regions"]["left"]["green_strain_range"][2][1] = json!(0.1);
                    report["regions"]["left"]["maximum_witness"][2] =
                        report["regions"]["left"]["minimum_witness"][2].clone();
                }
            }
            assert!(
                admit(&a, &d, &s, Some(&changed)).is_err(),
                "mutation {mutation}"
            );
        }
    }

    #[test]
    fn regional_comparison_guards_use_selected_union_and_preserve_polygon_protocols() {
        let policy: Policy = serde_json::from_value(json!({"basis":"nominal",
            "objectives":[{"metric":"max_engineering_shear_green_strain","direction":"minimize"}],
            "constraints":[{"metric":"max_engineering_shear_green_strain","maximum":0.2}]}))
        .unwrap();
        let (a, d, s) = regional_comparison_case(&[0, 2]);
        let union = policy.candidate(&a, None, &d, &s);
        assert!(!union.eligible);
        assert_eq!(
            union.readings[&Metric::MaxEngineeringShearGreenStrain].value,
            0.3
        );
        let (a, d, s) = regional_comparison_case(&[2]);
        let selected = policy.candidate(&a, None, &d, &s);
        assert!(selected.eligible, "{}", json!(selected));
        assert_eq!(
            selected.readings[&Metric::MaxEngineeringShearGreenStrain].value,
            0.15
        );

        let mut changed_d = d.clone();
        let mut changed_s = s.clone();
        changed_d["scenario"]["material_strain"]["regions"][0]["selector"]["polygon_mm"][0][0] =
            json!(-1200.);
        let selector = changed_d["scenario"]["material_strain"]["regions"][0]["selector"].clone();
        let report = &mut changed_s["material_strain"]["result"];
        report["request"] = changed_d["scenario"]["material_strain"].clone();
        report["source"]["regions"]["left"]["selector"] = selector;
        report["source_sha256"] = json!(hash(&report["source"]).unwrap());
        let different_polygon = policy.candidate(&a, None, &changed_d, &changed_s);
        assert!(different_polygon.eligible, "{}", json!(different_polygon));
        assert_eq!(
            selected.readings[&Metric::MaxEngineeringShearGreenStrain].value,
            different_polygon.readings[&Metric::MaxEngineeringShearGreenStrain].value
        );
        assert_ne!(
            selected.input_protocol_sha256,
            different_polygon.input_protocol_sha256
        );
    }

    #[test]
    fn source_qualified_scope_and_full_history_do_not_require_thermal_properties() {
        for per_piece in [false, true] {
            for scoped in [false, true] {
                let (a, d, s, m) = case(per_piece, scoped);
                assert!(admit(&a, &d, &s, None).is_ok());
                let result = admit(&a, &d, &s, Some(&m)).unwrap();
                assert_eq!(result["triangle_samples"], if scoped { 6 } else { 9 });
                assert_eq!(result["maximum_witness"][0]["sample_index"], 1);
                assert_eq!(result["maximum_witness"][0]["source_triangle"], 0);
                assert_eq!(result["maximum_witness"][2]["sample_index"], 0);
                assert_eq!(result["maximum_witness"][2]["value"], 0.3);
            }
        }
    }

    #[test]
    fn rehashed_source_cannot_drop_scoped_triangles_or_reassign_materials_and_domains() {
        let (a, d, s, _) = case(true, true);
        for mutation in 0..6 {
            let mut changed = s.clone();
            let report = &mut changed["material_strain"]["result"];
            match mutation {
                0 => {
                    report["source"]["triangles"].as_array_mut().unwrap().pop();
                }
                1 => report["source"]["triangles"][1]["source_triangle"] = json!(1),
                2 => report["source"]["triangles"][0]["instance"] = json!("back"),
                3 => report["source"]["triangles"][0]["vertices"] = json!([0, 1, 6]),
                4 => report["source"]["triangles"][0]["nonlinear_strain_range"][0][1] = json!(100.),
                _ => report["source"]["triangles"][0]["material_sha256"] = json!("0".repeat(64)),
            }
            report["source_sha256"] = json!(hash(&report["source"]).unwrap());
            assert!(
                admit(&a, &d, &changed, None).is_err(),
                "mutation {mutation}"
            );
        }
        let mut reassigned = a.clone();
        let changes = reassigned.frozen_changes.as_mut().unwrap();
        changes.default = changes.pieces.get("back").cloned();
        assert!(admit(&reassigned, &d, &s, None).is_err());
    }

    #[test]
    fn partial_stale_or_miscounted_history_cannot_supply_motion_extrema() {
        let (a, d, s, m) = case(true, false);
        for mutation in 0..8 {
            let mut changed = m.clone();
            let report = &mut changed["material_strain"]["result"];
            match mutation {
                0 => {
                    report["frames"].as_array_mut().unwrap().remove(1);
                }
                1 => report["triangle_samples"] = json!(6),
                2 => report["frames"][1]["positions_sha256"] = json!("0".repeat(64)),
                3 => report["frames"][1]["time_s"] = json!(0.5),
                4 => report["final_positions_sha256"] = report["initial_positions_sha256"].clone(),
                5 => report["source"]["triangles"][0]["material_gradients_m_inv"][0][0] = json!(3.),
                6 => report["duration_s"] = json!(1.),
                _ => report["scope"] = json!("initial_state"),
            }
            assert!(
                admit(&a, &d, &s, Some(&changed)).is_err(),
                "mutation {mutation}"
            );
        }
        let mut d = d;
        let mut s = s;
        let mut m = m;
        d["scenario"]["material_strain"]["max_triangle_samples"] = json!(8);
        s["material_strain"]["result"]["request"] = d["scenario"]["material_strain"].clone();
        m["material_strain"]["result"]["request"] = d["scenario"]["material_strain"].clone();
        assert!(admit(&a, &d, &s, None).is_ok());
        assert!(admit(&a, &d, &s, Some(&m)).is_err());
    }

    #[test]
    fn witnesses_preserve_signed_axes_local_ownership_and_earliest_ties() {
        let (a, d, s, m) = case(true, false);
        for mutation in 0..7 {
            let mut changed = m.clone();
            let report = &mut changed["material_strain"]["result"];
            match mutation {
                0 => report["minimum_witness"][2]["value"] = json!(0.2),
                1 => report["minimum_witness"][0]["time_s"] = json!(0.5),
                2 => {
                    report["minimum_witness"][0]["sample_index"] = json!(1);
                    report["minimum_witness"][0]["time_s"] = json!(1.);
                }
                3 => report["maximum_witness"][0]["source_triangle"] = json!(2),
                4 => {
                    report["instances"]["back"]["minimum_witness"][0]["source_triangle"] = json!(0)
                }
                5 => report["instances"]["front"]["green_strain_range"][2][1] = json!(0.2),
                _ => report["strain_convention"] = json!("material_green_E11_E22_E12"),
            }
            assert!(
                admit(&a, &d, &s, Some(&changed)).is_err(),
                "mutation {mutation}"
            );
        }
    }
}
