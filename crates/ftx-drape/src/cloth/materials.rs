//! Per-triangle sheet properties. Material values never alter rest geometry or seam welds.
//! Unlike-neighbor bending integrates the existing scalar-curvature stencil over its two area shares;
//! it is not a measured seam construction, a laminate homogenizer or a directional bending law.

use super::{ClothModel, Membrane};
use ftx_core::{Refusal, RefusalCode, Vec2};

/// Effective sheet law of one material-frame triangle, independent of mesh resolution.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetMaterial {
    /// Orthotropic membrane coefficients in N/m. Compression is the initial per-element factor.
    pub membrane: Membrane,
    /// Scalar flexural rigidity in N·m, not inferred from membrane stiffness or thickness.
    pub bending_nm: f64,
    /// Reference mass per undeformed area in kg/m².
    pub areal_kg_m2: f64,
}

fn bad(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::OutOfRange, message)
}

impl SheetMaterial {
    /// Rejects non-finite, nonphysical and non-positive-definite sheet inputs before assembly.
    ///
    /// # Errors
    /// Invalid density, bending, compression, transition or symmetric elastic stiffness matrix.
    pub fn validate(&self) -> Result<(), Refusal> {
        if !self.areal_kg_m2.is_finite()
            || self.areal_kg_m2 <= 0.0
            || !self.bending_nm.is_finite()
            || self.bending_nm < 0.0
            || !self.membrane.transition.is_finite()
            || self.membrane.transition <= 0.0
            || self
                .membrane
                .compression
                .iter()
                .any(|&c| !c.is_finite() || c <= 0.0 || c > 1.0)
        {
            return Err(bad(
                "sheet needs positive finite mass/transition, nonnegative bending and compression in (0,1]",
            ));
        }
        let q = self.membrane.q;
        if q.iter().flatten().any(|v| !v.is_finite())
            || (0..3).any(|i| (0..i).any(|j| q[i][j] != q[j][i]))
        {
            return Err(bad("sheet stiffness must be finite and symmetric"));
        }
        // Scale before Cholesky so stability checking does not overflow on representable inputs.
        let scale = q.iter().flatten().map(|v| v.abs()).fold(0.0, f64::max);
        if scale == 0.0 {
            return Err(bad("sheet stiffness must be positive definite"));
        }
        let mut l = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..=i {
                let mut a = q[i][j] / scale;
                for k in 0..j {
                    a -= l[i][k] * l[j][k];
                }
                if i == j {
                    if !a.is_finite() || a <= 0.0 {
                        return Err(bad("sheet stiffness must be positive definite"));
                    }
                    l[i][j] = a.sqrt();
                } else {
                    l[i][j] = a / l[j][j];
                }
            }
        }
        Ok(())
    }
}

impl ClothModel {
    /// Builds heterogeneous sheet mechanics on the existing per-triangle rest geometry.
    /// The material array has exactly one entry per triangle; homogeneous entries reproduce the
    /// homogeneous constructor's coefficients and arithmetic. Public global coefficients describe
    /// only the first sheet in a heterogeneous model; use `material` and `hinge_rigidity` to query it.
    ///
    /// # Errors
    /// Empty/mismatched material array, invalid geometry or properties, or mass/bending overflow.
    pub fn from_material_triangles(
        n: usize,
        tris: &[[usize; 3]],
        rest: &[[Vec2; 3]],
        materials: &[SheetMaterial],
    ) -> Result<Self, Refusal> {
        let first = materials
            .first()
            .ok_or_else(|| bad("a material triangle is required"))?;
        if materials.len() != tris.len() {
            return Err(bad("one sheet material per triangle is required"));
        }
        for m in materials {
            m.validate()?;
        }
        let mut model = Self::from_triangles(
            n,
            tris,
            rest,
            first.membrane,
            first.bending_nm,
            first.areal_kg_m2,
        )?;
        model.set_materials(materials)?;
        Ok(model)
    }

    /// Reassembles mass and every constitutive coefficient atomically, leaving rest geometry,
    /// topology, seam welds and elastic segments unchanged. Call before creating a new solver for
    /// a material substitution. Resets compression to the supplied factors and clears any previous
    /// nonlinear membrane responses and fitted domains; reapply gathered zones and the new response field
    /// afterward. No factor or old material coefficient is silently reused.
    ///
    /// # Errors
    /// Invalid/mismatched materials, mass underflow/overflow, or unrepresentable bending coefficients.
    pub fn set_materials(&mut self, materials: &[SheetMaterial]) -> Result<(), Refusal> {
        if self.panel_bending.is_some() {
            return Err(bad(
                "rebuild the cloth model to replace materials bound to panel bending",
            ));
        }
        if materials.len() != self.membranes.len() || materials.is_empty() {
            return Err(bad("one sheet material per triangle is required"));
        }
        for m in materials {
            m.validate()?;
        }
        let mut mass = vec![0.0; self.mass.len()];
        for (e, m) in self.membranes.iter().zip(materials) {
            let share = m.areal_kg_m2 * e.area / 3.0;
            if !share.is_finite() || share <= 0.0 {
                return Err(bad("triangle mass is not representable"));
            }
            for &v in &e.v {
                mass[v] += share;
                if !mass[v].is_finite() {
                    return Err(bad("vertex mass overflows"));
                }
            }
        }
        let mut bending = Vec::with_capacity(self.hinges.len());
        for &[(a, wa), (b, wb)] in &self.hinge_support {
            let (ba, bb) = (materials[a].bending_nm, materials[b].bending_nm);
            let value = if ba == bb {
                ba // preserve the homogeneous coefficient exactly
            } else {
                let scale = wa.max(wb);
                let (u, v) = (wa / scale, wb / scale);
                ba * (u / (u + v)) + bb * (v / (u + v))
            };
            if !value.is_finite() || value < 0.0 {
                return Err(bad("hinge rigidity is not representable"));
            }
            bending.push(value);
        }
        let first = materials[0];
        let heterogeneous = materials.iter().any(|&m| m != first);
        self.mass = mass;
        self.membrane = first.membrane;
        self.bending = first.bending_nm;
        self.compression = materials.iter().map(|m| m.membrane.compression).collect();
        self.element_materials =
            heterogeneous.then(|| materials.iter().map(|m| m.membrane).collect());
        self.element_responses = None;
        self.element_domains = None;
        self.hinge_bending = heterogeneous.then_some(bending);
        Ok(())
    }
}
