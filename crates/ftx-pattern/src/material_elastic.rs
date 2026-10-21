//! Characterized elastic tape/cord consumed by casing rings, distinct from the casing fabric.
//! EA is force per unit engineering strain for the finished band, not a sheet modulus or strength.

use ftx_core::{Refusal, RefusalCode};
use serde::Serialize;

use crate::material_catalog::{ResolvedMaterial, Selection};
use crate::materials::{Catalog, Form};

/// Explicit admission of the existing regularized, tension-only linear casing model.
pub const MODEL: &str = "linear_tension_only/1";

/// One source-complete band selection and its positive, normalized axial stiffness.
/// No band mass, contact surface, nonlinear stretch or recovery history is inferred from EA.
#[derive(Clone, Debug, Serialize)]
pub struct ElasticBand {
    snapshot: ResolvedMaterial,
    axial_stiffness_n: f64,
}

impl PartialEq for ElasticBand {
    fn eq(&self, other: &Self) -> bool {
        self.snapshot.sha256() == other.snapshot.sha256()
            && self.axial_stiffness_n == other.axial_stiffness_n
    }
}

impl ElasticBand {
    /// Resolve through the same catalog/override/normalization/hash authority as sheet materials.
    /// A yarn/cord or finished band may be characterized without sheet bending and skin friction.
    ///
    /// # Errors
    /// Missing/unsupported model, wrong units, stale identity, invalid material form, nonpositive
    /// stiffness or unapproved estimates. No width, cross-section or composition mixing is inferred.
    pub fn resolve(catalog: &Catalog, selection: &Selection) -> Result<Self, Refusal> {
        let snapshot = catalog.resolve(selection)?;
        let material = snapshot.material();
        if material.structure.get("elastic_model").map(String::as_str) != Some(MODEL)
            || matches!(material.form, Form::Fiber | Form::Fill)
        {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "casing band needs an explicit linear_tension_only/1 elastic_model and a finished band/cord, not fiber or loose fill",
            ));
        }
        let axial_stiffness_n =
            material.require("axial_stiffness", "N", snapshot.allows_estimates())?;
        if axial_stiffness_n <= 0.0
            || material.properties["axial_stiffness"]
                .bounds
                .is_some_and(|[lo, _]| lo < 0.0)
        {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "elastic axial_stiffness must be positive with nonnegative reported bounds",
            ));
        }
        Ok(Self {
            snapshot,
            axial_stiffness_n,
        })
    }

    /// EA in N for the entire selected band, before splitting it among numerical casing rings.
    #[must_use]
    pub fn axial_stiffness_n(&self) -> f64 {
        self.axial_stiffness_n
    }

    /// Frozen source record, original units, normalized observations and independent evidence policy.
    #[must_use]
    pub fn snapshot(&self) -> &ResolvedMaterial {
        &self.snapshot
    }

    /// Portable catalog-independent input; any changed observation or evidence policy breaks its lock.
    #[must_use]
    pub fn frozen_selection(&self) -> Selection {
        self.snapshot.frozen_selection()
    }
}
