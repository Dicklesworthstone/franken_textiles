//! Explicit, bounded local approximations of published source-model membrane tangents.
//!
//! Wang et al. (2011) use tensor shear E12, while the production energy uses gamma = 2 E12.
//! Only their origin tangent is transferred. Extending it over an operator-declared strain box
//! is an estimate, never a reconstruction of their varying, direction-dependent stiffness law.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use serde::{Deserialize, Serialize};

use crate::{
    material_catalog::Selection,
    material_response::{Interpolation, StrainDomain, StressCurve, TabulatedMembrane},
    materials::{Evidence, Form, Quantity, Source},
    textile_import,
};

/// Explicit local-linear approximation request. No strain range or missing property is defaulted.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Must be `ftx-wang2011-linearization/1`.
    pub schema: String,
    /// Source observations, with `allow_estimates: true` to admit the requested approximation.
    pub material: Selection,
    /// Closed bounds in `[E11, E22, 2 E12]` order, each strictly spanning zero.
    /// These are the operator's approximation limits, not a source-measured validity interval.
    pub green_strain_domain: [[f64; 2]; 3],
    /// Operator's dated rationale/source for choosing this approximation and its domain.
    pub approximation_source: Source,
}

/// Ordinary frozen selection and exact replay of the bounded approximation decision.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    /// `ftx-origin-membrane-linearization/1`.
    pub schema: String,
    /// Original request with its source material frozen for catalog-independent replay.
    pub input: Input,
    /// SHA-256 of the serialized frozen input.
    pub input_sha256: String,
    /// Exact source snapshot before adding the approximation.
    pub source_sha256: String,
    /// Work-conjugate origin tangent, N/m, in `[E11, E22, 2 E12]` order.
    pub origin_tangent_n_m: [[f64; 3]; 3],
    /// Ordinary source-locked material with an estimate-labelled, bounded membrane response.
    pub selection: Selection,
    /// Exact portable selection text for downstream consumers; do not reserialize display values.
    pub selection_json: String,
    /// Numerical model scope; other required mechanical properties remain independent.
    pub model_scope: String,
}

const SOURCE_KEY: &str = "ftx_origin_linearization";
const ORIGIN_KEYS: [&str; 4] = [
    "wang2011_origin_c11",
    "wang2011_origin_c12",
    "wang2011_origin_c22",
    "wang2011_origin_c33",
];

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

/// Transfer the source origin tangent into the existing bounded hyperelastic membrane authority.
/// The result runs in ordinary strip/cloth consumers once their independent inputs are supplied.
///
/// # Errors
/// Missing estimate consent, source/unit/specimen/basis mismatches, an existing membrane/history
/// law, invalid physical strain box, nonpositive-definite origin or any canonical source refusal.
pub fn linearize(input: &Input) -> Result<Report, Refusal> {
    if input.schema != "ftx-wang2011-linearization/1" {
        return Err(invalid("unsupported origin-membrane linearization schema"));
    }
    if !input.material.allow_estimates {
        return Err(invalid(
            "extending a source origin tangent is an estimate; material.allow_estimates must be true",
        ));
    }
    let domain = StrainDomain::new(input.green_strain_domain)?;
    let ranges = domain.ranges();
    if ranges.iter().any(|[lo, hi]| *lo >= 0.0 || *hi <= 0.0) {
        return Err(invalid(
            "each approximation interval must strictly span zero",
        ));
    }
    // The entire authored box must describe a positive-definite in-plane metric 2 E + I.
    let min_u = 1.0 + 2.0 * ranges[0][0];
    let min_v = 1.0 + 2.0 * ranges[1][0];
    let max_shear = ranges[2][0].abs().max(ranges[2][1].abs());
    let metric_determinant = min_u * min_v - max_shear * max_shear;
    if min_u <= 0.0 || min_v <= 0.0 || !metric_determinant.is_finite() || metric_determinant <= 0.0
    {
        return Err(invalid(
            "approximation strain box includes a nonphysical surface metric",
        ));
    }
    if [
        &input.approximation_source.reference,
        &input.approximation_source.accessed_on,
        &input.approximation_source.note,
    ]
    .iter()
    .any(|text| text.trim().is_empty() || text.len() > 4096)
    {
        return Err(invalid(
            "approximation needs a bounded, dated operator source and rationale",
        ));
    }
    let original =
        textile_import::resolve_fabric(&serde_json::json!({"material": input.material}))?
            .ok_or_else(|| invalid("missing source material selection"))?;
    let material = original.material();
    if matches!(material.form, Form::Fiber | Form::Yarn | Form::Fill) {
        return Err(invalid(
            "an origin sheet tangent cannot describe loose fiber, yarn or fill",
        ));
    }
    if material.membrane_response.is_some()
        || material.membrane_relaxation.is_some()
        || material.structure.contains_key("membrane_plasticity_model")
        || material
            .structure
            .contains_key("membrane_linearization_model")
        || material
            .structure
            .contains_key("membrane_linearization_parent_sha256")
        || [
            "warp_stiffness",
            "weft_stiffness",
            "poisson_warp_weft",
            "shear_stiffness",
        ]
        .iter()
        .any(|key| material.properties.contains_key(*key))
        || material.sources.contains_key(SOURCE_KEY)
    {
        return Err(invalid(
            "origin linearization cannot replace an existing membrane law or history",
        ));
    }
    let mut values = [0.0; 4];
    let mut common: Option<(&str, &BTreeMap<String, String>)> = None;
    for (i, key) in ORIGIN_KEYS.iter().enumerate() {
        values[i] = material.require(key, "N/m", original.allows_estimates())?;
        let observation = &material.properties[*key];
        let conditions = &observation.conditions;
        if conditions.get("strain_basis").map(String::as_str) != Some("green_lagrange_tensor_shear")
            || conditions.get("source_node").map(String::as_str) != Some("lambda_max=0")
            || ["specimen", "conditioning"].iter().any(|key| {
                conditions
                    .get(*key)
                    .is_none_or(|value| value.trim().is_empty())
            })
        {
            return Err(invalid(format!(
                "{key} needs exact origin, tensor-shear and specimen qualification"
            )));
        }
        if let Some((source, previous)) = common {
            if source != observation.source
                || ["specimen", "conditioning", "strain_basis", "source_node"]
                    .iter()
                    .any(|key| previous.get(*key) != conditions.get(*key))
            {
                return Err(invalid(
                    "origin coefficients belong to different sources, specimens or conditions",
                ));
            }
        } else {
            common = Some((&observation.source, conditions));
        }
    }
    let [q11, q12, q22, tensor_shear] = values;
    // S12 = c33 E12 = (c33 / 2) gamma. Copying c33 into G12 would double shear forces.
    let g12 = tensor_shear / 2.0;
    let tangent = [[q11, q12, 0.0], [q12, q22, 0.0], [0.0, 0.0, g12]];
    let mut conditions = material.properties[ORIGIN_KEYS[0]].conditions.clone();
    conditions.insert(
        "source_strain_basis".into(),
        "green_lagrange_tensor_shear".into(),
    );
    conditions.insert(
        "strain_basis".into(),
        "green_lagrange_engineering_shear".into(),
    );
    conditions.insert("approximation".into(), "Constant origin tangent over operator-authored Green-strain box; not the source's nonlinear law or a measured validity domain".into());
    conditions.insert("source_material_sha256".into(), original.sha256().into());
    conditions.insert(
        "green_strain_domain".into(),
        serde_json::to_string(&ranges).map_err(|e| invalid(e.to_string()))?,
    );
    let curve = |axis: usize, modulus: f64| {
        StressCurve {
        points: vec![
            [ranges[axis][0], modulus * ranges[axis][0]],
            [0.0, 0.0],
            [ranges[axis][1], modulus * ranges[axis][1]],
        ],
        source: SOURCE_KEY.into(),
        evidence: Evidence::Estimate,
        method: "Local origin linearization of Wang2011 Eq. 1-3; production work-conjugate stress versus [E11,E22,2E12]; G12=C33/2".into(),
        conditions: conditions.clone(),
    }
    };
    let response = TabulatedMembrane {
        model: crate::material_response::MODEL.into(),
        interpolation: Interpolation::PiecewiseLinear,
        warp: curve(0, q11),
        weft: curve(1, q22),
        shear: curve(2, g12),
        coupling: Quantity {
            value: q12,
            unit: "N/m".into(),
            evidence: Evidence::Estimate,
            source: SOURCE_KEY.into(),
            method: "Constant source-origin C12 coupling on the declared approximation box".into(),
            conditions,
            bounds: None,
        },
        area_response: None,
    };
    let mut authored = original.authored().clone();
    authored
        .sources
        .insert(SOURCE_KEY.into(), input.approximation_source.clone());
    // This is the production compiler's positive-definiteness gate, never an SPD projection.
    response.compile(&authored.sources, true)?;
    authored.membrane_response = Some(response);
    authored.structure.insert(
        "membrane_linearization_parent_sha256".into(),
        original.sha256().into(),
    );
    authored
        .structure
        .insert("membrane_linearization_model".into(), input.schema.clone());
    let derived = textile_import::resolve_fabric(&serde_json::json!({"material": Selection {
        id: authored.id.clone(), record: Some(authored), allow_estimates: true,
        ..Selection::default()
    }}))?
    .ok_or_else(|| invalid("missing derived selection"))?;
    let selection = derived.frozen_selection();
    let selection_json = serde_json::to_string(&selection).map_err(|e| invalid(e.to_string()))?;
    let mut frozen_input = input.clone();
    frozen_input.material = original.frozen_selection();
    let bytes = serde_json::to_vec(&frozen_input).map_err(|e| invalid(e.to_string()))?;
    Ok(Report {
        schema: "ftx-origin-membrane-linearization/1".into(),
        input: frozen_input, input_sha256: sha256_hex(&bytes),
        source_sha256: original.sha256().into(), origin_tangent_n_m: tangent,
        selection, selection_json,
        model_scope: "Estimated constant origin tangent on an explicit Green-strain box. Source nonlinear angular/strain dependence and bending model are not reproduced. No thickness, contact friction, damping, heat transport, garment fit or empirical validation is inferred.".into(),
    })
}
