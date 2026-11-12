//! Explicit first-capture model choices shared by native and portable acquisition.
//! These declarations do not infer anatomy, measurements, starting values or a fitted body.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, Vec3};
use serde::{Deserialize, Serialize};

use super::{Configuration, Joint, Parameter};
use crate::{AnnyModel, TAPE_FIELDS};

/// Exact world translation and rotation-vector components exposed by capture authoring.
pub const ROOTS: [&str; 6] = [
    "root_x_m",
    "root_y_m",
    "root_z_m",
    "root_rx_rad",
    "root_ry_rad",
    "root_rz_rad",
];

/// One available shape or root control and its existing model domain.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    /// Exact model parameter name.
    pub name: String,
    /// Inclusive model-domain lower bound, not a fitted result.
    pub lower: f64,
    /// Inclusive model-domain upper bound.
    pub upper: f64,
    /// Existing unfitted model default.
    pub default_value: f64,
    /// Dimensionless, metres or radians.
    pub unit: String,
}

/// One actual non-root model bone available for an explicit joint-axis declaration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bone {
    /// Exact source-model bone name.
    pub name: String,
    /// Source-model bone index.
    pub index: usize,
    /// Source-model parent index.
    pub parent_index: Option<usize>,
    /// Source-model parent name.
    pub parent_name: Option<String>,
}

/// Source-model controls available before anatomical correspondence or fitting.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Catalogue {
    /// Shared phenotype and local shape controls.
    pub shapes: Vec<Control>,
    /// All six world root components, in canonical order.
    pub roots: Vec<Control>,
    /// Actual articulated model bones.
    pub joints: Vec<Bone>,
    /// Supported independent rest-body tape functional names.
    pub measurements: Vec<String>,
}

/// A caller explicitly fixes a control or selects its fitted bounds.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Choice {
    /// Keep one declared value in every frame.
    Fixed {
        /// Explicit fixed value in the control's units.
        value: f64,
    },
    /// Select an existing fitted scalar control.
    Fit {
        /// Exclusive fitted lower bound.
        lower: f64,
        /// Exclusive fitted upper bound.
        upper: f64,
    },
}

/// One explicitly selected shape or root component.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NamedChoice {
    /// Exact catalogue control name.
    pub name: String,
    /// Authored fixed value or fitted bounds.
    pub choice: Choice,
}

/// One explicitly selected model joint and local unit axis.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JointChoice {
    /// Exact non-root model bone name.
    pub bone: String,
    /// Explicit unit axis in the local bone frame; never normalized here.
    pub axis: Vec3,
    /// Authored fixed angle or fitted angular bounds, radians.
    pub choice: Choice,
}

/// Model declarations needed to construct an unfinished multi-frame capture draft.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelChoices {
    /// Authored age; absence refuses rather than inventing wearer evidence.
    pub age_years: Option<f64>,
    /// Authored Anny gender coefficient; absence refuses.
    pub gender: Option<f64>,
    /// Explicit selected shape controls; unselected controls retain model defaults.
    pub shapes: Vec<NamedChoice>,
    /// Exactly one explicit choice for each of the six root components.
    pub roots: Vec<NamedChoice>,
    /// Explicit joint axes; unselected joints retain the rest pose.
    pub joints: Vec<JointChoice>,
}

/// Read available controls from the actual converted model, without posing or fitting it.
#[must_use]
pub fn catalogue(model: &AnnyModel) -> Catalogue {
    let labels = &model.data().header.labels;
    let mut shapes: Vec<_> = ["height", "weight", "muscle", "proportions"]
        .into_iter()
        .map(|name| Control {
            name: name.into(),
            lower: 0.0,
            upper: 1.0,
            default_value: 0.5,
            unit: "dimensionless".into(),
        })
        .collect();
    shapes.extend(labels.local_changes.iter().map(|name| Control {
        name: name.clone(),
        lower: -1.0,
        upper: 1.0,
        default_value: 0.0,
        unit: "dimensionless".into(),
    }));
    let roots = ROOTS
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let limit = if i < 3 { 1e6 } else { std::f64::consts::PI };
            Control {
                name: (*name).into(),
                lower: -limit,
                upper: limit,
                default_value: 0.0,
                unit: if i < 3 { "m" } else { "rad" }.into(),
            }
        })
        .collect();
    let joints = labels
        .bones
        .iter()
        .enumerate()
        .skip(1)
        .map(|(index, name)| {
            let parent_index = usize::try_from(labels.bone_parents[index]).ok();
            Bone {
                name: name.clone(),
                index,
                parent_name: parent_index.and_then(|i| labels.bones.get(i)).cloned(),
                parent_index,
            }
        })
        .collect();
    Catalogue {
        shapes,
        roots,
        joints,
        measurements: TAPE_FIELDS.iter().map(|name| (*name).into()).collect(),
    }
}

fn invalid(message: &str) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

impl ModelChoices {
    /// Construct the ordinary correspondence configuration with empty anatomical targets.
    /// Call the existing correspondence-model and capture-authoring admission before using it:
    /// those authorities validate model-specific names, domains and fitted parameter counts.
    ///
    /// # Errors
    /// Missing wearer declarations, duplicate controls or joints, unknown/missing root choices,
    /// or invalid fixed root values.
    pub fn configuration(
        &self,
        model_sha256: &str,
        measurements: &[String],
    ) -> Result<Configuration, Refusal> {
        let age_years = self
            .age_years
            .ok_or_else(|| invalid("choose age_years before constructing a capture"))?;
        let gender = self
            .gender
            .ok_or_else(|| invalid("choose gender before constructing a capture"))?;
        let mut fixed_shape = BTreeMap::new();
        let mut shape_parameters = Vec::new();
        let mut names = BTreeSet::new();
        for row in &self.shapes {
            if !names.insert(&row.name) {
                return Err(invalid("duplicate shape choice"));
            }
            match row.choice {
                Choice::Fixed { value } => {
                    fixed_shape.insert(row.name.clone(), value);
                }
                Choice::Fit { lower, upper } => shape_parameters.push(Parameter {
                    name: row.name.clone(),
                    lower,
                    upper,
                }),
            }
        }
        let mut translation = [0.0; 3];
        let mut rotation = [0.0; 3];
        let mut pose_parameters = Vec::new();
        let mut names = BTreeSet::new();
        if self.roots.len() != ROOTS.len() {
            return Err(invalid(
                "declare exactly one fixed or fitted choice for each of the six root components",
            ));
        }
        for row in &self.roots {
            let index = ROOTS
                .iter()
                .position(|name| *name == row.name)
                .ok_or_else(|| invalid("unknown root component"))?;
            if !names.insert(&row.name) {
                return Err(invalid("duplicate root choice"));
            }
            match row.choice {
                Choice::Fixed { value } => {
                    let limit = if index < 3 { 1e6 } else { std::f64::consts::PI };
                    if !value.is_finite() || !(-limit..=limit).contains(&value) {
                        return Err(invalid(
                            "fixed root value is outside its declared model domain",
                        ));
                    }
                    if index < 3 {
                        translation[index] = value;
                    } else {
                        rotation[index - 3] = value;
                    }
                }
                Choice::Fit { lower, upper } => pose_parameters.push(Parameter {
                    name: row.name.clone(),
                    lower,
                    upper,
                }),
            }
        }
        let mut joints = BTreeMap::new();
        for row in &self.joints {
            let angle_rad = match row.choice {
                Choice::Fixed { value } => value,
                Choice::Fit { lower, upper } => {
                    pose_parameters.push(Parameter {
                        name: format!("joint:{}", row.bone),
                        lower,
                        upper,
                    });
                    0.0
                }
            };
            if joints
                .insert(
                    row.bone.clone(),
                    Joint {
                        axis: row.axis,
                        angle_rad,
                    },
                )
                .is_some()
            {
                return Err(invalid("duplicate joint choice"));
            }
        }
        Ok(Configuration {
            model_sha256: model_sha256.into(),
            age_years,
            gender,
            fixed_shape,
            shape_parameters,
            root_translation_m: Vec3::new(translation[0], translation[1], translation[2]),
            root_rotation_rad: Vec3::new(rotation[0], rotation[1], rotation[2]),
            joints,
            pose_parameters,
            keypoints: BTreeMap::new(),
            measurements: measurements.to_vec(),
        })
    }
}
