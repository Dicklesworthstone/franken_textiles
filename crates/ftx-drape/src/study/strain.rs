//! Raw material-frame strain on retained production triangles, independent of empirical fit.
//! Existing constitutive domains describe characterized support, never failure or comfort limits.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, Vec3};
use ftx_pattern::material_catalog::Selection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{budget, hash, invalid};
use crate::{elements::MembraneElement, run::DrapeRun, thermal::surface::io::retained_materials};

const MAX_TRIANGLE_SAMPLES: usize = 64_000_000;

/// Optional spatial scope and explicit work allowance. No strain threshold is defaulted here.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Must be `ftx-material-strain/1`.
    pub schema: String,
    /// Exact retained instance IDs; omission selects every retained instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instances: Option<Vec<String>>,
    /// Total selected triangle evaluations, including the initial state and all accepted samples.
    pub max_triangle_samples: usize,
}

impl Request {
    fn validate(&self) -> Result<(), Refusal> {
        if self.schema != "ftx-material-strain/1"
            || !(1..=MAX_TRIANGLE_SAMPLES).contains(&self.max_triangle_samples)
        {
            return Err(invalid(
                "material strain needs its explicit schema and 1..64000000 triangle samples",
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
            frames: Vec::new(),
            triangle_samples: 0,
        };
        history.advance(&scene.solver.x, 0.)?;
        Ok(history)
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
        self.frames.push(frame);
        self.triangle_samples = count;
        Ok(())
    }

    pub(super) fn report(&self, motion: bool) -> Value {
        let summary = self.summary.as_ref().expect("initial frame was admitted");
        json!({"schema":"ftx-material-strain-report/1", "request":self.request,
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
            "note":"Raw production material-frame Green strain before numerical gather/compression softening. Source domains describe constitutive characterization support, not rupture, comfort or safe fit. Comparison thresholds are declared design requirements. Extrema cover only selected retained triangles and accepted sampled states; ties retain the earliest sample then the lowest source-triangle index."})
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
        };
        History {
            request: Request {
                schema: "ftx-material-strain/1".into(),
                instances: None,
                max_triangle_samples: max,
            },
            source_sha256: hash(&serde_json::to_value(&source).unwrap()).unwrap(),
            source,
            elements,
            summary: None,
            instances: BTreeMap::new(),
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
}
