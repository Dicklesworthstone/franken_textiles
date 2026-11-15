//! Raw material-frame strain on retained production triangles, independent of empirical fit.
//! Existing constitutive domains describe characterized support, never failure or comfort limits.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, Vec2, Vec3};
use ftx_pattern::{
    material_catalog::Selection,
    material_compression::regions::{MAX_REGION_EDGE_TESTS, PreparedSelector, Selector},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{budget, hash, invalid, retained_flat_triangles};
use crate::{elements::MembraneElement, run::DrapeRun, thermal::surface::io::retained_materials};

const MAX_TRIANGLE_SAMPLES: usize = 64_000_000;

/// Optional spatial scope and explicit work allowance. No strain threshold is defaulted here.
#[derive(Clone, Debug, Serialize)]
pub struct Request {
    /// `ftx-material-strain/1` for whole instances, or /2 for fixed flat-material regions.
    pub schema: String,
    /// Version 1 only: exact retained instance IDs; omission selects every retained instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instances: Option<Vec<String>>,
    /// Version 2 only: disjoint nonempty whole-triangle regions, never changing cloth topology.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regions: Option<Vec<Region>>,
    /// Total selected triangle evaluations, including the initial state and all accepted samples.
    pub max_triangle_samples: usize,
}

/// A named strain scope in one physical pattern instance's mirrored, simulation-scaled flat mm.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    /// Unique nonempty region name; independent of the physical instance and material identity.
    pub id: String,
    /// Exact retained physical instance, including the ordinary mirror/copy identity.
    pub instance: String,
    /// Convex polygon selecting whole retained triangles by their exact flat centroids.
    pub selector: Selector,
}

impl<'de> Deserialize<'de> for Request {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        fn instances<'de, D: serde::Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<Option<Vec<String>>>, D::Error> {
            Option::<Vec<String>>::deserialize(deserializer).map(Some)
        }
        fn regions<'de, D: serde::Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<Vec<Region>>, D::Error> {
            Vec::<Region>::deserialize(deserializer).map(Some)
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            schema: String,
            #[serde(default, deserialize_with = "instances")]
            instances: Option<Option<Vec<String>>>,
            #[serde(default, deserialize_with = "regions")]
            regions: Option<Vec<Region>>,
            max_triangle_samples: usize,
        }
        let fields = Fields::deserialize(deserializer)?;
        if (fields.schema == "ftx-material-strain/2"
            && (fields.instances.is_some() || fields.regions.is_none()))
            || (fields.schema != "ftx-material-strain/2" && fields.regions.is_some())
        {
            return Err(serde::de::Error::custom(
                "material strain /2 requires regions without instances; /1 accepts only instance scope",
            ));
        }
        Ok(Self {
            schema: fields.schema,
            instances: fields.instances.flatten(),
            regions: fields.regions,
            max_triangle_samples: fields.max_triangle_samples,
        })
    }
}

impl Request {
    fn validate(&self) -> Result<(), Refusal> {
        let regional = self.schema == "ftx-material-strain/2";
        if (!regional && self.schema != "ftx-material-strain/1")
            || (regional && (self.instances.is_some() || self.regions.is_none()))
            || (!regional && self.regions.is_some())
            || !(1..=MAX_TRIANGLE_SAMPLES).contains(&self.max_triangle_samples)
        {
            return Err(invalid(
                "material strain needs schema-qualified instance or region scope and 1..64000000 triangle samples",
            ));
        }
        if let Some(ids) = &self.instances {
            let unique: BTreeSet<_> = ids.iter().collect();
            if ids.is_empty()
                || ids.len() > 4096
                || unique.len() != ids.len()
                || ids.iter().any(|s| s.trim().is_empty() || s.len() > 4096)
            {
                return Err(invalid(
                    "material strain scope needs 1..4096 unique nonempty instance IDs",
                ));
            }
        }
        if let Some(regions) = &self.regions {
            let mut unique = BTreeSet::new();
            if !(1..=256).contains(&regions.len()) {
                return Err(invalid("material strain needs 1..256 declared regions"));
            }
            for region in regions {
                if region.id.trim().is_empty()
                    || region.id.len() > 512
                    || !unique.insert(&region.id)
                    || region.instance.trim().is_empty()
                    || region.instance.len() > 4096
                {
                    return Err(invalid(
                        "material strain regions need unique bounded names and exact physical instances",
                    ));
                }
                PreparedSelector::new(&region.selector)?;
            }
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct Budget {
    reserved: usize,
}

impl Budget {
    pub(super) fn reserve(&mut self, request: &Request) -> Result<(), Refusal> {
        request.validate()?;
        self.reserved = self
            .reserved
            .checked_add(request.max_triangle_samples)
            .filter(|&n| n <= MAX_TRIANGLE_SAMPLES)
            .ok_or_else(|| {
                budget("aggregate material-strain triangle-sample allowance exhausted")
            })?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
struct Triangle {
    source_triangle: usize,
    instance: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    region: Option<String>,
    material_sha256: String,
    vertices: [usize; 3],
    reference_area_m2: f64,
    material_gradients_m_inv: [[f64; 2]; 3],
    fitted_strain_range: Option<[[f64; 2]; 3]>,
    nonlinear_strain_range: Option<[[f64; 2]; 3]>,
    physical_area_ratio_range: Option<[f64; 2]>,
}

#[derive(Clone, Debug, Serialize)]
struct Source {
    vertex_count: usize,
    instances: Vec<String>,
    materials: BTreeMap<String, Selection>,
    triangles: Vec<Triangle>,
    material_report_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    regions: Option<BTreeMap<String, RegionSource>>,
}

#[derive(Clone, Debug, Serialize)]
struct RegionSource {
    instance: String,
    selector: Selector,
    source_triangles: Vec<usize>,
    reference_area_m2: f64,
}

struct Partition {
    triangle_regions: Vec<Option<String>>,
    regions: BTreeMap<String, RegionSource>,
}

fn partition(
    regions: &[Region],
    instances: &[String],
    owners: &[usize],
    triangles: &[[Vec2; 3]],
    areas: &[f64],
) -> Result<Partition, Refusal> {
    if triangles.is_empty()
        || triangles.len() != owners.len()
        || triangles.len() != areas.len()
        || areas.iter().any(|area| !area.is_finite() || *area <= 0.)
    {
        return Err(invalid(
            "regional strain needs complete source triangles and areas",
        ));
    }
    let mut by_instance = BTreeMap::<&str, Vec<usize>>::new();
    for (triangle, &owner) in owners.iter().enumerate() {
        let instance = instances
            .get(owner)
            .ok_or_else(|| invalid("regional strain source owner is unavailable"))?;
        by_instance.entry(instance).or_default().push(triangle);
    }
    let mut edge_tests = 0usize;
    let mut selectors = Vec::with_capacity(regions.len());
    for region in regions {
        let owned = by_instance.get(region.instance.as_str()).ok_or_else(|| {
            invalid("regional strain scope names an unknown or unretained instance")
        })?;
        edge_tests = owned
            .len()
            .checked_mul(region.selector.polygon_mm.len())
            .and_then(|work| edge_tests.checked_add(work))
            .filter(|&work| work <= MAX_REGION_EDGE_TESTS)
            .ok_or_else(|| budget("regional strain selector edge-test budget exhausted"))?;
        selectors.push((region, PreparedSelector::new(&region.selector)?, owned));
    }
    let mut result = Partition {
        triangle_regions: vec![None; triangles.len()],
        regions: BTreeMap::new(),
    };
    for (region, selector, owned) in selectors {
        let mut selected = Vec::new();
        let mut area = 0.;
        for &index in owned {
            if selector.contains_triangle_centroid(triangles[index])? {
                if result.triangle_regions[index].is_some() {
                    return Err(invalid(
                        "material strain regions select the same retained triangle",
                    ));
                }
                result.triangle_regions[index] = Some(region.id.clone());
                selected.push(index);
                area += areas[index];
            }
        }
        if selected.is_empty() || !area.is_finite() {
            return Err(invalid(
                "material strain region needs nonempty selection with finite reference area",
            ));
        }
        result.regions.insert(
            region.id.clone(),
            RegionSource {
                instance: region.instance.clone(),
                selector: region.selector.clone(),
                source_triangles: selected,
                reference_area_m2: area,
            },
        );
    }
    Ok(result)
}

#[derive(Clone, Debug, Serialize)]
struct Witness {
    value: f64,
    source_triangle: usize,
    sample_index: usize,
    time_s: f64,
}

#[derive(Clone, Debug, Serialize)]
struct Summary {
    green_strain_range: [[f64; 2]; 3],
    minimum_witness: [Witness; 3],
    maximum_witness: [Witness; 3],
}

impl Summary {
    fn new(values: [f64; 3], triangle: usize, sample: usize, time: f64) -> Self {
        let witnesses = std::array::from_fn(|i| Witness {
            value: values[i],
            source_triangle: triangle,
            sample_index: sample,
            time_s: time,
        });
        Self {
            green_strain_range: values.map(|x| [x, x]),
            minimum_witness: witnesses.clone(),
            maximum_witness: witnesses,
        }
    }

    fn observe(&mut self, values: [f64; 3], triangle: usize, sample: usize, time: f64) {
        for (i, value) in values.into_iter().enumerate() {
            let witness = Witness {
                value,
                source_triangle: triangle,
                sample_index: sample,
                time_s: time,
            };
            // Strict comparisons retain the earliest sample and lowest source-triangle tie.
            if value < self.green_strain_range[i][0] {
                self.green_strain_range[i][0] = value;
                self.minimum_witness[i] = witness.clone();
            }
            if value > self.green_strain_range[i][1] {
                self.green_strain_range[i][1] = value;
                self.maximum_witness[i] = witness;
            }
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct Frame {
    sample_index: usize,
    time_s: f64,
    positions_sha256: String,
    green_strain_range: [[f64; 2]; 3],
    minimum_triangle: [usize; 3],
    maximum_triangle: [usize; 3],
}

pub(super) struct History {
    request: Request,
    source: Source,
    source_sha256: String,
    elements: Vec<MembraneElement>,
    summary: Option<Summary>,
    instances: BTreeMap<String, Summary>,
    regions: BTreeMap<String, Summary>,
    rest_triangles_mm: Option<Vec<[Vec2; 3]>>,
    frames: Vec<Frame>,
    triangle_samples: usize,
}

impl History {
    pub(super) fn new(request: &Request, run: &DrapeRun) -> Result<Self, Refusal> {
        request.validate()?;
        let scene = &run.scene;
        let model = &scene.solver.model;
        if model.membranes.is_empty()
            || model.membranes.len() > 65_536
            || model.membranes.len() != scene.triangle_owner.len()
        {
            return Err(invalid(
                "material strain needs complete bounded production triangle ownership",
            ));
        }
        let (records, assignment) = retained_materials(
            &run.report["material"],
            &scene.mesh.instances,
            &scene.triangle_owner,
        )?;
        let rest_triangles_mm = request
            .regions
            .as_ref()
            .map(|_| retained_flat_triangles(run))
            .transpose()?;
        let partition = request
            .regions
            .as_ref()
            .map(|regions| {
                partition(
                    regions,
                    &scene.mesh.instances,
                    &scene.triangle_owner,
                    rest_triangles_mm.as_ref().expect("regional flat source"),
                    &model
                        .membranes
                        .iter()
                        .map(|element| element.area)
                        .collect::<Vec<_>>(),
                )
            })
            .transpose()?;
        let requested = request
            .instances
            .as_ref()
            .map(|v| v.iter().collect::<BTreeSet<_>>());
        let mut seen = BTreeSet::new();
        let mut triangles = Vec::new();
        let mut elements = Vec::new();
        let mut materials = BTreeMap::new();
        for (i, element) in model.membranes.iter().enumerate() {
            let instance = &scene.mesh.instances[scene.triangle_owner[i]];
            if requested
                .as_ref()
                .is_some_and(|ids| !ids.contains(instance))
                || partition
                    .as_ref()
                    .is_some_and(|partition| partition.triangle_regions[i].is_none())
            {
                continue;
            }
            seen.insert(instance.clone());
            let material = &records[assignment[i]];
            let material_sha256 = material.sha256().to_string();
            materials
                .entry(material_sha256.clone())
                .or_insert_with(|| material.frozen_selection());
            triangles.push(Triangle {
                source_triangle: i,
                instance: instance.clone(),
                region: partition
                    .as_ref()
                    .and_then(|partition| partition.triangle_regions[i].clone()),
                material_sha256,
                vertices: element.v,
                reference_area_m2: element.area,
                material_gradients_m_inv: element.g,
                fitted_strain_range: model.membrane_domain(i).map(|d| d.ranges()),
                nonlinear_strain_range: model.membrane_response(i).map(|d| d.strain_range()),
                physical_area_ratio_range: model
                    .membrane_response(i)
                    .and_then(|d| d.area_ratio_range()),
            });
            elements.push(*element);
        }
        if triangles.is_empty()
            || seen.len() > 4096
            || requested
                .as_ref()
                .is_some_and(|ids| ids.len() != seen.len())
        {
            return Err(invalid(
                "material strain scope includes an unknown, unplaced or fully collapsed instance",
            ));
        }
        let source = Source {
            vertex_count: scene.solver.x.len(),
            instances: seen.into_iter().collect(),
            materials,
            triangles,
            material_report_sha256: hash(&run.report["material"])?,
            regions: partition.map(|partition| partition.regions),
        };
        // Hash the retained JSON object, whose canonical map order survives serialization.
        let source_sha256 =
            hash(&serde_json::to_value(&source).map_err(|e| invalid(e.to_string()))?)?;
        let mut history = Self {
            request: request.clone(),
            source,
            source_sha256,
            elements,
            summary: None,
            instances: BTreeMap::new(),
            regions: BTreeMap::new(),
            rest_triangles_mm,
            frames: Vec::new(),
            triangle_samples: 0,
        };
        history.advance(&scene.solver.x, 0.)?;
        Ok(history)
    }

    pub(super) fn rest_triangles_mm(&self) -> Option<&[[Vec2; 3]]> {
        self.rest_triangles_mm.as_deref()
    }

    pub(super) fn advance(&mut self, positions: &[Vec3], time_s: f64) -> Result<(), Refusal> {
        let count = self.triangle_samples.checked_add(self.elements.len())
            .filter(|&n| n <= self.request.max_triangle_samples)
            .ok_or_else(|| budget("material-strain triangle-sample budget exhausted; no partial history published"))?;
        if !time_s.is_finite()
            || time_s < 0.
            || self.frames.last().is_some_and(|f| time_s <= f.time_s)
            || (self.frames.is_empty() && time_s != 0.)
            || positions.len() != self.source.vertex_count
            || positions.iter().any(|x| !x.is_finite())
        {
            return Err(invalid(
                "material strain needs ordered accepted times and complete finite production positions",
            ));
        }
        let sample_index = self.frames.len();
        let mut frame: Option<Summary> = None;
        let mut total = self.summary.clone();
        let mut instances = self.instances.clone();
        let mut regions = self.regions.clone();
        for (element, source) in self.elements.iter().zip(&self.source.triangles) {
            let values = element.strain(positions);
            if values.iter().any(|x| !x.is_finite()) {
                return Err(invalid(
                    "material strain is not representable on a retained source triangle",
                ));
            }
            for range in [source.fitted_strain_range, source.nonlinear_strain_range]
                .into_iter()
                .flatten()
            {
                if values
                    .iter()
                    .zip(range)
                    .any(|(&x, [lo, hi])| x < lo || x > hi)
                {
                    return Err(invalid(
                        "accepted material strain leaves its retained constitutive characterization domain",
                    ));
                }
            }
            let first = || Summary::new(values, source.source_triangle, sample_index, time_s);
            frame.get_or_insert_with(first).observe(
                values,
                source.source_triangle,
                sample_index,
                time_s,
            );
            total.get_or_insert_with(first).observe(
                values,
                source.source_triangle,
                sample_index,
                time_s,
            );
            instances
                .entry(source.instance.clone())
                .or_insert_with(first)
                .observe(values, source.source_triangle, sample_index, time_s);
            if let Some(region) = &source.region {
                regions.entry(region.clone()).or_insert_with(first).observe(
                    values,
                    source.source_triangle,
                    sample_index,
                    time_s,
                );
            }
        }
        let frame =
            frame.ok_or_else(|| invalid("material strain needs at least one selected triangle"))?;
        let frame = Frame {
            sample_index,
            time_s,
            positions_sha256: hash(&positions)?,
            green_strain_range: frame.green_strain_range,
            minimum_triangle: frame.minimum_witness.map(|w| w.source_triangle),
            maximum_triangle: frame.maximum_witness.map(|w| w.source_triangle),
        };
        self.summary = total;
        self.instances = instances;
        self.regions = regions;
        self.frames.push(frame);
        self.triangle_samples = count;
        Ok(())
    }

    pub(super) fn report(&self, motion: bool) -> Value {
        let summary = self.summary.as_ref().expect("initial frame was admitted");
        let mut report = json!({"schema":if self.source.regions.is_some() {
                "ftx-material-strain-report/2"
            } else {"ftx-material-strain-report/1"}, "request":self.request,
            "scope":if motion {"accepted_motion_samples"} else {"initial_state"},
            "strain_convention":"material_green_E11_E22_2E12", "unit":"1",
            "source":self.source, "source_sha256":self.source_sha256,
            "initial_positions_sha256":self.frames[0].positions_sha256,
            "final_positions_sha256":self.frames.last().expect("initial frame").positions_sha256,
            "duration_s":self.frames.last().expect("initial frame").time_s,
            "green_strain_range":summary.green_strain_range,
            "minimum_witness":summary.minimum_witness, "maximum_witness":summary.maximum_witness,
            "instances":self.instances, "frames":self.frames,
            "triangle_samples":self.triangle_samples,
            "physical_validation_claim":false,"fit_claim":false,"safe_strain_claim":false,
            "continuous_time_bound":false,
            "note":"Raw production material-frame Green strain before numerical gather/compression softening. Source domains describe constitutive characterization support, not rupture, comfort or safe fit. Comparison thresholds are declared design requirements. Extrema cover only selected retained triangles and accepted sampled states; ties retain the earliest sample then the lowest source-triangle index."});
        if self.source.regions.is_some() {
            report["regions"] = json!(self.regions);
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_core::Vec2;

    fn history(max: usize) -> History {
        let first = MembraneElement::new(
            [0, 1, 2],
            [Vec2::new(0., 0.), Vec2::new(1., 0.), Vec2::new(0., 1.)],
        )
        .unwrap();
        let second = MembraneElement::new(
            [0, 1, 2],
            [Vec2::new(0., 0.), Vec2::new(0., -1.), Vec2::new(1., 0.)],
        )
        .unwrap();
        let elements = vec![first, second];
        let triangles = elements
            .iter()
            .enumerate()
            .map(|(i, e)| Triangle {
                source_triangle: i,
                instance: format!("panel-{i}"),
                region: None,
                material_sha256: format!("synthetic-{i}"),
                vertices: e.v,
                reference_area_m2: e.area,
                material_gradients_m_inv: e.g,
                fitted_strain_range: None,
                nonlinear_strain_range: None,
                physical_area_ratio_range: None,
            })
            .collect();
        let source = Source {
            vertex_count: 3,
            instances: vec!["panel-0".into(), "panel-1".into()],
            materials: BTreeMap::new(),
            triangles,
            material_report_sha256: "synthetic analytic kinematics only".into(),
            regions: None,
        };
        History {
            request: Request {
                schema: "ftx-material-strain/1".into(),
                instances: None,
                regions: None,
                max_triangle_samples: max,
            },
            source_sha256: hash(&serde_json::to_value(&source).unwrap()).unwrap(),
            source,
            elements,
            summary: None,
            instances: BTreeMap::new(),
            regions: BTreeMap::new(),
            rest_triangles_mm: None,
            frames: vec![],
            triangle_samples: 0,
        }
    }

    fn rest() -> [Vec3; 3] {
        [Vec3::ZERO, Vec3::new(1., 0., 0.), Vec3::new(0., 1., 0.)]
    }

    #[test]
    fn raw_material_axes_keep_signed_shear_and_source_triangle_witnesses() {
        let mut h = history(6);
        h.advance(&rest(), 0.).unwrap();
        let x = [Vec3::ZERO, Vec3::new(1.2, 0., 0.), Vec3::new(0.3, 0.8, 0.)];
        h.advance(&x, 0.1).unwrap();
        let expected = [[-0.135, 0.22], [-0.135, 0.22], [-0.36, 0.36]];
        let ranges = h.summary.as_ref().unwrap().green_strain_range;
        for (actual, expected) in ranges.iter().flatten().zip(expected.iter().flatten()) {
            assert!((actual - expected).abs() < 1e-14);
        }
        let r = h.report(true);
        assert_eq!(r["maximum_witness"][2]["source_triangle"], 0);
        assert_eq!(r["minimum_witness"][2]["source_triangle"], 1);
        assert_eq!(r["maximum_witness"][2]["sample_index"], 1);
        assert_eq!(r["triangle_samples"], 4);
        assert_eq!(r["frames"][0]["maximum_triangle"], json!([0, 0, 0]));
        h.advance(&rest(), 0.2).unwrap();
        assert_eq!(
            h.report(true)["green_strain_range"],
            r["green_strain_range"]
        );
        assert_eq!(h.report(true)["maximum_witness"], r["maximum_witness"]);
        assert_eq!(h.report(true)["frames"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn whole_frame_budget_and_late_domain_failures_keep_previous_extrema() {
        let mut h = history(3);
        h.advance(&rest(), 0.).unwrap();
        let before = h.report(true);
        assert!(h.advance(&rest(), 0.1).is_err());
        assert_eq!(h.report(true), before);
        h.request.max_triangle_samples = 6;
        h.source.triangles[1].fitted_strain_range = Some([[-0.01, 0.01]; 3]);
        let before = h.report(true);
        let x = [Vec3::ZERO, Vec3::new(1.2, 0., 0.), Vec3::new(0., 1., 0.)];
        assert!(h.advance(&x, 0.1).is_err());
        assert_eq!(h.report(true), before);
        assert!(h.advance(&rest(), 0.).is_err());
        assert_eq!(h.report(true), before);
    }

    #[test]
    fn scope_and_aggregate_work_are_explicit_without_strain_defaults() {
        let mut b = Budget::default();
        let mut r = Request {
            schema: "ftx-material-strain/1".into(),
            instances: None,
            regions: None,
            max_triangle_samples: 32_000_000,
        };
        b.reserve(&r).unwrap();
        b.reserve(&r).unwrap();
        r.max_triangle_samples = 1;
        assert!(b.reserve(&r).is_err());
        r.instances = Some(vec!["front".into(), "front".into()]);
        assert!(r.validate().is_err());
        r.instances = Some(vec![]);
        assert!(r.validate().is_err());
        r.instances = None;
        r.max_triangle_samples = 0;
        assert!(r.validate().is_err());
    }

    fn region(id: &str, instance: &str, points: &[[f64; 2]]) -> Region {
        Region {
            id: id.into(),
            instance: instance.into(),
            selector: Selector {
                kind: "whole_triangle_centroid".into(),
                polygon_mm: points.iter().copied().map(Vec2::from).collect(),
            },
        }
    }

    #[test]
    fn regional_scope_versions_and_complete_names_are_explicit() {
        let r = json!({"schema":"ftx-material-strain/2","max_triangle_samples":10,
            "regions":[{"id":"elbow","instance":"sleeve","selector":{
                "kind":"whole_triangle_centroid","polygon_mm":[[0.,0.],[3.,0.],[0.,3.]]}}]});
        serde_json::from_value::<Request>(r.clone())
            .unwrap()
            .validate()
            .unwrap();
        for changed in [
            json!({"schema":"ftx-material-strain/2","max_triangle_samples":10}),
            json!({"schema":"ftx-material-strain/2","max_triangle_samples":10,"regions":null}),
            json!({"schema":"ftx-material-strain/1","max_triangle_samples":10,"regions":null}),
        ] {
            assert!(serde_json::from_value::<Request>(changed).is_err());
        }
        for instances in [Value::Null, json!([]), json!(["sleeve"])] {
            let mut changed = r.clone();
            changed["instances"] = instances;
            assert!(serde_json::from_value::<Request>(changed).is_err());
        }
        for regions in [json!([]), json!([r["regions"][0], r["regions"][0]])] {
            let mut changed = r.clone();
            changed["regions"] = regions;
            assert!(
                serde_json::from_value::<Request>(changed)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        let legacy = json!({"schema":"ftx-material-strain/1","max_triangle_samples":10});
        let mut explicit_null = legacy.clone();
        explicit_null["instances"] = Value::Null;
        assert_eq!(
            serde_json::to_value(serde_json::from_value::<Request>(explicit_null).unwrap())
                .unwrap(),
            legacy
        );
        let mut old = history(10);
        old.advance(&rest(), 0.).unwrap();
        assert!(old.report(false).get("regions").is_none());
    }

    #[test]
    fn regional_partition_excludes_remainders_and_rejects_exact_boundary_overlap() {
        let triangles = [
            [[0., 0.], [2., 0.], [0., 6.]].map(Vec2::from),
            [[10., 0.], [12., 0.], [10., 6.]].map(Vec2::from),
            [[20., 0.], [22., 0.], [20., 6.]].map(Vec2::from),
        ];
        let instances = ["panel".into()];
        let owners = [0, 0, 0];
        let areas = [6e-6; 3];
        let left = region("left", "panel", &[[0., 0.], [1., 3.], [-4., 3.], [-4., 0.]]);
        let center = region(
            "center",
            "panel",
            &[[10., 0.], [13., 0.], [13., 4.], [10., 4.]],
        );
        let actual = partition(
            &[left.clone(), center.clone()],
            &instances,
            &owners,
            &triangles,
            &areas,
        )
        .unwrap();
        assert_eq!(
            actual.triangle_regions,
            [Some("left".into()), Some("center".into()), None]
        );
        assert_eq!(actual.regions["left"].source_triangles, [0]);
        assert_eq!(actual.regions["center"].reference_area_m2, 6e-6);
        let mut reverse = center.clone();
        reverse.selector.polygon_mm.reverse();
        assert_eq!(
            partition(&[reverse], &instances, &owners, &triangles, &areas)
                .unwrap()
                .triangle_regions,
            [None, Some("center".into()), None]
        );
        // The mathematical centroid (2/3, 2) belongs to both polygons. Rounding the centroid
        // before its edge predicate would conceal this overlap on one side of y = 3x.
        let right = region("right", "panel", &[[0., 0.], [4., 0.], [4., 3.], [1., 3.]]);
        assert!(partition(&[left, right], &instances, &owners, &triangles, &areas).is_err());
        let unknown = region("unknown", "missing", &[[0., 0.], [3., 0.], [0., 3.]]);
        assert!(partition(&[unknown], &instances, &owners, &triangles, &areas).is_err());
        let empty = region("empty", "panel", &[[100., 0.], [103., 0.], [100., 3.]]);
        assert!(partition(&[empty], &instances, &owners, &triangles, &areas).is_err());
    }

    #[test]
    fn regional_partition_charges_complete_selector_work_before_selection() {
        let regions = (0..256)
            .map(|i| {
                region(
                    &format!("zone-{i}"),
                    "panel",
                    &[[0., 0.], [3., 0.], [3., 3.], [0., 3.]],
                )
            })
            .collect::<Vec<_>>();
        let triangles = vec![[[0., 0.], [1., 0.], [0., 1.]].map(Vec2::from); 65_536];
        let refused = partition(
            &regions,
            &["panel".into()],
            &vec![0; triangles.len()],
            &triangles,
            &vec![0.5e-6; triangles.len()],
        );
        assert_eq!(
            refused.err().unwrap().code,
            ftx_core::RefusalCode::BudgetExceeded
        );
    }

    #[test]
    fn regional_strain_keeps_local_signed_extrema_and_atomic_sample_updates() {
        let mut h = history(6);
        let regions = [
            region("first", "panel", &[[0., 0.], [3., 0.], [0., 3.]]),
            region("second", "panel", &[[3., 0.], [6., 0.], [3., 3.]]),
        ];
        h.request.schema = "ftx-material-strain/2".into();
        h.request.regions = Some(regions.to_vec());
        h.source.instances = vec!["panel".into()];
        h.source.regions = Some(
            regions
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    h.source.triangles[i].instance = r.instance.clone();
                    h.source.triangles[i].region = Some(r.id.clone());
                    (
                        r.id.clone(),
                        RegionSource {
                            instance: r.instance.clone(),
                            selector: r.selector.clone(),
                            source_triangles: vec![i],
                            reference_area_m2: h.source.triangles[i].reference_area_m2,
                        },
                    )
                })
                .collect(),
        );
        h.advance(&rest(), 0.).unwrap();
        let x = [Vec3::ZERO, Vec3::new(1.2, 0., 0.), Vec3::new(0.3, 0.8, 0.)];
        h.advance(&x, 0.1).unwrap();
        let report = h.report(true);
        assert_eq!(report["schema"], "ftx-material-strain-report/2");
        assert!(
            (report["regions"]["first"]["maximum_witness"][2]["value"]
                .as_f64()
                .unwrap()
                - 0.36)
                .abs()
                < 1e-14
        );
        assert!(
            (report["regions"]["second"]["minimum_witness"][2]["value"]
                .as_f64()
                .unwrap()
                + 0.36)
                .abs()
                < 1e-14
        );
        assert_eq!(
            report["regions"]["first"]["maximum_witness"][2]["source_triangle"],
            0
        );
        assert_eq!(
            report["regions"]["second"]["minimum_witness"][2]["source_triangle"],
            1
        );
        h.source.triangles[1].fitted_strain_range = Some([[-0.01, 0.01]; 3]);
        let before = h.report(true);
        assert!(h.advance(&x, 0.2).is_err());
        assert_eq!(
            h.report(true),
            before,
            "a later region refusal cannot publish an earlier region's extrema"
        );
    }
}
