//! Source-panel topology is retained across physical welds; curvature support never jumps a seam.

use ftx_core::{Refusal, RefusalCode, Vec2, Vec3};
use ftx_pattern::material_bending::BendingTensor;

use super::ClothModel;
use crate::bending::{BendingPatch, build, build_intrinsic, build_local_isometric};
use crate::elements::MembraneElement;
use crate::knit::AsymmetricKnitBending;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PanelField {
    pub patches: Vec<BendingPatch>,
    pub replaced_hinges: Vec<bool>,
    pub triangles: Vec<usize>,
    pub heterogeneous: bool,
    pub knit_laws: Option<Vec<Option<AsymmetricKnitBending>>>,
}

impl ClothModel {
    // Admission to a source-qualified liquid domain must not freeze already substituted bending.
    // Reassemble coefficients on the retained geometry; do not reconstruct grain coordinates or
    // disturb accepted curvature/plastic histories merely to compare their immutable source laws.
    pub(crate) fn check_source_bending(
        &self,
        materials: &[super::SheetMaterial],
        laws: &[Option<BendingTensor>],
    ) -> Result<(), Refusal> {
        let bad = || {
            Refusal::new(
                RefusalCode::InvalidDocument,
                "liquid bending differs from its qualified source materials",
            )
        };
        if materials.len() != self.membranes.len()
            || laws.len() != materials.len()
            || self.hinges.len() != self.hinge_support.len()
        {
            return Err(bad());
        }
        let mut reference = self.clone();
        reference.attached_mass = None;
        reference.panel_bending = None;
        reference.set_materials(materials)?;
        if self.bending != reference.bending
            || (0..self.hinges.len()).any(|h| {
                self.hinge_bending.as_ref().map_or(self.bending, |b| b[h])
                    != reference.hinge_rigidity(h)
            })
        {
            return Err(bad());
        }
        if self
            .panel_bending
            .as_ref()
            .is_some_and(|p| p.knit_laws.is_some())
        {
            return Err(bad());
        }
        let triangles: Vec<_> = laws
            .iter()
            .enumerate()
            .filter_map(|(i, law)| law.map(|_| i))
            .collect();
        if self.panel_bending_triangles() != triangles
            || self.panel_bending_patches().len() != triangles.len()
            || self
                .panel_bending_patches()
                .iter()
                .zip(triangles)
                .any(|(patch, i)| {
                    Some(patch.tensor()) != laws[i]
                        || patch.reference_area() != self.membranes[i].area
                })
        {
            return Err(bad());
        }
        Ok(())
    }

    /// Bind orthotropic curvature on unwelded source panels, then map its support to solver vertices.
    /// Interior scalar hinges are REPLACED; scalar seam hinges remain the existing, separate seam
    /// approximation. None preserves the old model exactly. Build once before simulation; changing
    /// a bound material requires a new model, not stale curvature weights or solver factors.
    ///
    /// # Errors
    /// Mismatched rest geometry/welds, unresolved panels, mixed models inside one connected panel,
    /// or an attempted second binding. A refusal leaves the model unchanged.
    pub fn set_panel_bending(
        &mut self,
        source_rest: &[Vec2],
        source_triangles: &[[usize; 3]],
        weld: &[usize],
        laws: &[Option<BendingTensor>],
    ) -> Result<(), Refusal> {
        let bad = |s| Refusal::new(RefusalCode::InvalidDocument, s);
        if self.panel_bending.is_some() {
            return Err(bad(
                "panel bending is already bound; rebuild for material substitution",
            ));
        }
        if source_rest.len() != weld.len()
            || source_triangles.len() != self.membranes.len()
            || laws.len() != self.membranes.len()
            || weld.iter().any(|&v| v >= self.rest.len())
            || source_triangles
                .iter()
                .flatten()
                .any(|&v| v >= source_rest.len())
        {
            return Err(bad("panel bending source geometry does not match cloth"));
        }
        for (source, e) in source_triangles.iter().zip(&self.membranes) {
            if MembraneElement::new(source.map(|v| weld[v]), source.map(|v| source_rest[v]))
                .as_ref()
                != Some(e)
            {
                return Err(bad(
                    "panel bending rest triangle or weld differs from membrane geometry",
                ));
            }
        }
        if laws.iter().all(Option::is_none) {
            return Ok(());
        }
        let patches = build(source_rest, source_triangles, weld, laws)?;
        let replaced_hinges = self
            .hinge_support
            .iter()
            .map(|&[(a, _), (b, _)]| {
                let common = source_triangles[a]
                    .iter()
                    .filter(|v| source_triangles[b].contains(v))
                    .count();
                common == 2 && laws[a].is_some() && laws[b].is_some()
            })
            .collect();
        self.panel_bending = Some(PanelField {
            patches,
            replaced_hinges,
            triangles: laws
                .iter()
                .enumerate()
                .filter_map(|(i, m)| m.is_some().then_some(i))
                .collect(),
            heterogeneous: laws.windows(2).any(|pair| pair[0] != pair[1]),
            knit_laws: None,
        });
        Ok(())
    }

    /// Bind twice-integrated `C^3` strictly convex asymmetric knit bending laws (with exact
    /// Cayley–Hamilton `C^{-1/2}` objective physical curvature pullback) on unwelded source
    /// panels, replacing interior scalar hinges while keeping seam hinges intact (`FTX-K09`).
    ///
    /// # Errors
    /// Invalid bending parameters, mismatched rest geometry/welds, unresolved quadratic patches,
    /// or an attempted second binding. A refusal leaves the model unchanged.
    pub fn set_knit_panel_bending(
        &mut self,
        source_rest: &[Vec2],
        source_triangles: &[[usize; 3]],
        weld: &[usize],
        laws: &[Option<AsymmetricKnitBending>],
    ) -> Result<(), Refusal> {
        self.set_mixed_panel_bending(
            source_rest,
            source_triangles,
            weld,
            &vec![None; laws.len()],
            laws,
        )
    }

    /// Bind ordinary tensor and asymmetric knit bending in one source-panel field.
    ///
    /// Both selectors use retained membrane-triangle order. A triangle may select either law;
    /// unselected panels retain scalar hinges, and seam hinges retain their separate scalar law.
    /// The patch-order knit selector includes ordinary tensor patches as explicit empty slots.
    ///
    /// # Errors
    /// Overlapping laws, mismatched selectors, invalid laws/source geometry, or a second binding.
    /// A refusal leaves the model unchanged.
    pub fn set_mixed_panel_bending(
        &mut self,
        source_rest: &[Vec2],
        source_triangles: &[[usize; 3]],
        weld: &[usize],
        ordinary: &[Option<BendingTensor>],
        knit: &[Option<AsymmetricKnitBending>],
    ) -> Result<(), Refusal> {
        if ordinary.len() != self.membranes.len() || knit.len() != self.membranes.len() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "one ordinary and knit bending slot per membrane triangle is required",
            ));
        }
        let mut tensors = Vec::with_capacity(knit.len());
        for (ordinary, knit) in ordinary.iter().zip(knit) {
            if ordinary.is_some() && knit.is_some() {
                return Err(Refusal::new(
                    RefusalCode::InvalidDocument,
                    "a source triangle cannot bind both ordinary and asymmetric knit bending",
                ));
            }
            tensors.push(if let Some(k) = knit {
                k.validate()?;
                let d11 = 0.5 * (k.d_plus[0] + k.d_minus[0]);
                let d22 = 0.5 * (k.d_plus[1] + k.d_minus[1]);
                Some(BendingTensor::new(d11, d22, k.d12, k.d66)?.normal_projected())
            } else {
                *ordinary
            });
        }
        self.set_panel_bending(source_rest, source_triangles, weld, &tensors)?;
        if knit.iter().any(Option::is_some)
            && let Some(field) = &mut self.panel_bending
        {
            field.heterogeneous |= knit.windows(2).any(|pair| pair[0] != pair[1]);
            field.knit_laws = Some(field.triangles.iter().map(|&i| knit[i]).collect());
        }
        Ok(())
    }

    /// Bind source-selected bending to exact intrinsic membrane charts. No global rest embedding
    /// or source stitch geometry is altered to make a curved knit surface appear flat.
    ///
    /// # Errors
    /// Mismatched source triangles/material frames, inconsistent edge lengths, unresolved local
    /// quadratic support, overlapping laws or a second binding. Refusal leaves the model unchanged.
    pub fn set_intrinsic_panel_bending(
        &mut self,
        source_triangles: &[[usize; 3]],
        tri_rest: &[[Vec2; 3]],
        weld: &[usize],
        ordinary: &[Option<BendingTensor>],
        knit: &[Option<AsymmetricKnitBending>],
    ) -> Result<(), Refusal> {
        let bad = |s| Refusal::new(RefusalCode::InvalidDocument, s);
        if self.panel_bending.is_some()
            || source_triangles.len() != self.membranes.len()
            || tri_rest.len() != self.membranes.len()
            || ordinary.len() != self.membranes.len()
            || knit.len() != self.membranes.len()
            || source_triangles.iter().flatten().any(|&v| v >= weld.len())
            || weld.iter().any(|&v| v >= self.rest.len())
        {
            return Err(bad(
                "intrinsic panel bending source geometry does not match unbound cloth",
            ));
        }
        let mut tensors = Vec::with_capacity(knit.len());
        for (i, source) in source_triangles.iter().enumerate() {
            if MembraneElement::new(source.map(|v| weld[v]), tri_rest[i]).as_ref()
                != Some(&self.membranes[i])
            {
                return Err(bad(
                    "intrinsic bending chart or material frame differs from its membrane",
                ));
            }
            if ordinary[i].is_some() && knit[i].is_some() {
                return Err(bad(
                    "an intrinsic triangle cannot select two panel-bending laws",
                ));
            }
            tensors.push(if let Some(k) = knit[i] {
                k.validate()?;
                Some(
                    BendingTensor::new(
                        0.5 * (k.d_plus[0] + k.d_minus[0]),
                        0.5 * (k.d_plus[1] + k.d_minus[1]),
                        k.d12,
                        k.d66,
                    )?
                    .normal_projected(),
                )
            } else {
                ordinary[i]
            });
        }
        if tensors.iter().all(Option::is_none) {
            return Ok(());
        }
        let patches = build_intrinsic(source_triangles, tri_rest, weld, &tensors)?;
        let replaced_hinges = self
            .hinge_support
            .iter()
            .map(|&[(a, _), (b, _)]| {
                source_triangles[a]
                    .iter()
                    .filter(|v| source_triangles[b].contains(v))
                    .count()
                    == 2
                    && tensors[a].is_some()
                    && tensors[b].is_some()
            })
            .collect();
        let triangles: Vec<_> = tensors
            .iter()
            .enumerate()
            .filter_map(|(i, law)| law.map(|_| i))
            .collect();
        let knit_laws = knit
            .iter()
            .any(Option::is_some)
            .then(|| triangles.iter().map(|&i| knit[i]).collect());
        self.panel_bending = Some(PanelField {
            patches,
            replaced_hinges,
            triangles,
            heterogeneous: tensors.windows(2).any(|pair| pair[0] != pair[1])
                || knit.windows(2).any(|pair| pair[0] != pair[1]),
            knit_laws,
        });
        Ok(())
    }

    /// Bind twice-integrated `C^3` strictly convex asymmetric knit bending laws (with exact
    /// Cayley–Hamilton `C^{-1/2}` objective physical curvature pullback) on a curved non-Euclidean
    /// 3D rest surface using local isometric 1-ring tangent charts (`FTX-K09`).
    ///
    /// Unlike [`Self::set_knit_panel_bending`], this validates per-triangle local isometric
    /// `tri_rest` metric equivalence against `self.membranes` (edge lengths and area), enabling
    /// intrinsically curved surfaces (such as spherical caps or shaped seamless knit shells with
    /// non-zero Gaussian curvature) without requiring a global flat 2D `source_rest`.
    ///
    /// # Errors
    /// Invalid bending parameters, mismatched per-triangle rest metrics/welds, unresolved local
    /// isometric patches, or an attempted second binding. A refusal leaves the model unchanged.
    pub fn set_non_euclidean_knit_panel_bending(
        &mut self,
        source_rest_3d: &[Vec3],
        source_triangles: &[[usize; 3]],
        weld: &[usize],
        grain_dirs: Option<&[Vec3]>,
        laws: &[Option<AsymmetricKnitBending>],
    ) -> Result<(), Refusal> {
        let bad = |s| Refusal::new(RefusalCode::InvalidDocument, s);
        if self.panel_bending.is_some() {
            return Err(bad(
                "panel bending is already bound; rebuild for material substitution",
            ));
        }
        if source_rest_3d.len() != weld.len()
            || source_triangles.len() != self.membranes.len()
            || laws.len() != self.membranes.len()
            || grain_dirs.is_some_and(|g| g.len() != self.membranes.len())
            || weld.iter().any(|&v| v >= self.rest.len())
            || source_triangles
                .iter()
                .flatten()
                .any(|&v| v >= source_rest_3d.len())
        {
            return Err(bad(
                "non-Euclidean panel bending source geometry does not match cloth",
            ));
        }
        // Validate per-triangle isometric `tri_rest` metric equivalence (edge lengths + area),
        // rather than global flat `source_rest`.
        for (idx, (source, e)) in source_triangles.iter().zip(&self.membranes).enumerate() {
            let tri_rest = MembraneElement::tri_rest_from_3d(
                source.map(|v| source_rest_3d[v]),
                grain_dirs.map(|g| g[idx]),
            )?;
            let Some(cand) = MembraneElement::new(source.map(|v| weld[v]), tri_rest) else {
                return Err(bad("degenerate non-Euclidean rest triangle"));
            };
            let le = e.rest_edge_lengths();
            let lc = cand.rest_edge_lengths();
            let area_scale = e.area.max(1e-12);
            let edges_match = le
                .iter()
                .zip(&lc)
                .all(|(a, b)| (a - b).abs() <= 1e-8 * a.max(1e-9));
            if cand.v != e.v || !edges_match || (cand.area - e.area).abs() > 1e-8 * area_scale {
                return Err(bad(
                    "non-Euclidean panel bending per-triangle rest metric or weld differs from membrane geometry",
                ));
            }
        }
        if laws.iter().all(Option::is_none) {
            return Ok(());
        }
        let mut tensors = Vec::with_capacity(laws.len());
        for knit in laws {
            tensors.push(if let Some(k) = knit {
                k.validate()?;
                let d11 = 0.5 * (k.d_plus[0] + k.d_minus[0]);
                let d22 = 0.5 * (k.d_plus[1] + k.d_minus[1]);
                Some(BendingTensor::new(d11, d22, k.d12, k.d66)?.normal_projected())
            } else {
                None
            });
        }
        let patches =
            build_local_isometric(source_rest_3d, source_triangles, weld, &tensors, grain_dirs)?;
        let replaced_hinges = self
            .hinge_support
            .iter()
            .map(|&[(a, _), (b, _)]| {
                let common = source_triangles[a]
                    .iter()
                    .filter(|v| source_triangles[b].contains(v))
                    .count();
                common == 2 && laws[a].is_some() && laws[b].is_some()
            })
            .collect();
        let triangles: Vec<usize> = laws
            .iter()
            .enumerate()
            .filter_map(|(i, m)| m.is_some().then_some(i))
            .collect();
        let knit_laws = Some(triangles.iter().map(|&i| laws[i]).collect());
        self.panel_bending = Some(PanelField {
            patches,
            replaced_hinges,
            triangles,
            heterogeneous: laws.windows(2).any(|pair| pair[0] != pair[1]),
            knit_laws,
        });
        Ok(())
    }

    /// Opt-in per-triangle zone flexural rigidity multipliers on bound asymmetric knit panel
    /// bending (`FTX-K09`, lifting the scalar-only restriction of `cloth/materials.rs:208` for
    /// knit hems/cuffs/rib borders/jacquard back-float zones without disturbing legacy scalar
    /// construction mechanics).
    ///
    /// Scales `(d_plus, d_minus, d12, d66)` of each knit patch by `multipliers[tri] > 0`,
    /// preserving strict global convexity `min(D11) * min(D22) > D12^2`.
    ///
    /// # Errors
    /// Missing knit panel bending binding, length mismatch, or non-finite/non-positive multiplier.
    pub fn apply_knit_panel_zone_multipliers(
        &mut self,
        multipliers: &[f64],
    ) -> Result<(), Refusal> {
        if multipliers.len() != self.membranes.len() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "one zone bending multiplier per membrane triangle is required",
            ));
        }
        for (idx, &m) in multipliers.iter().enumerate() {
            if !m.is_finite() || m <= 0.0 {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!(
                        "knit zone bending multiplier[{idx}] = {m} must be finite and positive"
                    ),
                )
                .repair("supply positive finite flexural multipliers per triangle"));
            }
        }
        let Some(field) = self.panel_bending.as_mut() else {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "cannot apply knit zone multipliers before binding knit panel bending",
            ));
        };
        let Some(knit_laws) = field.knit_laws.as_mut() else {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "cannot apply knit zone multipliers to non-knit panel bending",
            ));
        };
        let mut updated = Vec::with_capacity(knit_laws.len());
        for (&tri_idx, law_opt) in field.triangles.iter().zip(knit_laws.iter()) {
            let mult = multipliers[tri_idx];
            if let Some(law) = law_opt {
                let mut scaled = *law;
                scaled.d_plus = [law.d_plus[0] * mult, law.d_plus[1] * mult];
                scaled.d_minus = [law.d_minus[0] * mult, law.d_minus[1] * mult];
                scaled.d12 = law.d12 * mult;
                scaled.d66 = law.d66 * mult;
                scaled.validate()?;
                updated.push(Some(scaled));
            } else {
                updated.push(None);
            }
        }
        field.heterogeneous |= updated.windows(2).any(|pair| pair[0] != pair[1]);
        *knit_laws = updated;
        Ok(())
    }

    /// Asymmetric knit bending law bound to panel patch `patch_index`, if any.
    #[must_use]
    pub fn panel_knit_bending(&self, patch_index: usize) -> Option<&AsymmetricKnitBending> {
        self.panel_bending
            .as_ref()
            .and_then(|p| p.knit_laws.as_ref())
            .and_then(|laws| laws.get(patch_index))
            .and_then(Option::as_ref)
    }

    /// Immutable curvature patches in source-triangle order (unselected scalar panels omitted).
    #[must_use]
    pub fn panel_bending_patches(&self) -> &[BendingPatch] {
        self.panel_bending
            .as_ref()
            .map_or(&[], |p| p.patches.as_slice())
    }

    /// Retained material-triangle index for each patch, preserving source ownership after welding.
    #[must_use]
    pub fn panel_bending_triangles(&self) -> &[usize] {
        self.panel_bending
            .as_ref()
            .map_or(&[], |p| p.triangles.as_slice())
    }

    /// Refuse singular reconstructed tangent frames (or non-SPD knit bending metrics) before
    /// evaluating curvature or derivatives. A line-search trial may fail this gate without
    /// changing the accepted state.
    ///
    /// # Errors
    /// Missing/non-finite positions, a degenerate normal-projected patch frame, or non-SPD metric.
    pub fn check_panel_bending_domains(&self, x: &[Vec3]) -> Result<(), Refusal> {
        for (i, patch) in self.panel_bending_patches().iter().enumerate() {
            patch.check_domain(x)?;
            if self.panel_knit_bending(i).is_some() {
                patch.physical_curvature(x)?;
            }
        }
        Ok(())
    }
}
