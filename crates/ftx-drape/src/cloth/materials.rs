//! Per-triangle sheet properties. Material values never alter rest geometry or seam welds.
//! Unlike-neighbor bending integrates the existing scalar-curvature stencil over its two area shares;
//! it is not a measured seam construction, a laminate homogenizer or a directional bending law.

use super::{ClothModel, Membrane};
use crate::garment::{ConstructionField, ConstructionMechanicsOptions, compute_construction_field};
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
    /// Invalid/mismatched materials, existing panel/attachment bindings, mass underflow/overflow,
    /// or unrepresentable bending coefficients.
    pub fn set_materials(&mut self, materials: &[SheetMaterial]) -> Result<(), Refusal> {
        if self.attached_mass.is_some() {
            return Err(bad(
                "rebuild the scene to replace sheet materials with source-bound attached mass",
            ));
        }
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

    /// Applies opt-in folded-hem and sewn seam-allowance mass and bending multipliers (FTX-27d)
    /// derived from flat rest geometry, piece ownership, and welded seam topology.
    ///
    /// When `!options.is_active()` (all multipliers `== 1.0`), this validates inputs and returns
    /// without touching `self.mass` or `self.hinge_bending`, preserving bit-identical legacy
    /// arithmetic. Must be called once before binding attached masses; refuses combining scalar
    /// construction multipliers with characterized orthotropic panel bending.
    ///
    /// # Errors
    /// Invalid `options`, existing attached-mass or panel-bending bindings, mismatched source
    /// topology/densities, or mass/bending overflow.
    pub fn apply_construction_mechanics(
        &mut self,
        source_rest_m: &[Vec2],
        source_triangles: &[[usize; 3]],
        triangle_owner: &[usize],
        weld: &[usize],
        base_areal_kg_m2: &[f64],
        options: ConstructionMechanicsOptions,
    ) -> Result<ConstructionField, Refusal> {
        options.validate()?;
        if self.attached_mass.is_some() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "apply construction mechanics before binding attached mass",
            ));
        }
        if self.panel_bending.is_some() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "construction bending multipliers cannot be combined with characterized panel bending",
            ));
        }
        if source_triangles.len() != self.membranes.len()
            || base_areal_kg_m2.len() != self.membranes.len()
            || source_rest_m.len() != weld.len()
        {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "construction mechanics source triangles, densities, and weld map must match cloth",
            ));
        }
        for (t, (&tri, (&rho, elem))) in source_triangles
            .iter()
            .zip(base_areal_kg_m2.iter().zip(&self.membranes))
            .enumerate()
        {
            if !rho.is_finite() || rho <= 0.0 {
                return Err(bad(format!(
                    "triangle {t} base areal density {rho} kg/m² must be positive and finite"
                )));
            }
            if tri.iter().any(|&v| v >= weld.len()) || tri.map(|v| weld[v]) != elem.v {
                return Err(Refusal::new(
                    RefusalCode::DegenerateGeometry,
                    format!("construction source triangle {t} does not match welded membrane"),
                ));
            }
        }
        let field = compute_construction_field(
            source_rest_m,
            source_triangles,
            triangle_owner,
            weld,
            &self.hinge_support,
            options,
        )?;
        if !options.is_active() {
            return Ok(field);
        }

        let mass_changed = field.triangle_mass_multiplier.iter().any(|&m| m != 1.0);
        let new_mass = if mass_changed {
            let mut mass = vec![0.0; self.mass.len()];
            for ((e, &rho), &mult) in self
                .membranes
                .iter()
                .zip(base_areal_kg_m2)
                .zip(&field.triangle_mass_multiplier)
            {
                let share = (rho * mult) * e.area / 3.0;
                if !share.is_finite() || share <= 0.0 {
                    return Err(bad(
                        "construction-scaled triangle mass is not representable",
                    ));
                }
                for &v in &e.v {
                    mass[v] += share;
                    if !mass[v].is_finite() {
                        return Err(bad("construction-scaled vertex mass overflows"));
                    }
                }
            }
            Some(mass)
        } else {
            None
        };

        let bend_changed = field.hinge_bending_multiplier.iter().any(|&m| m != 1.0);
        let new_hinge_bending = if bend_changed {
            let mut bending = Vec::with_capacity(self.hinges.len());
            for (h, &hm) in field.hinge_bending_multiplier.iter().enumerate() {
                let base = self.hinge_bending.as_ref().map_or(self.bending, |b| b[h]);
                let value = base * hm;
                if !value.is_finite() || value < 0.0 {
                    return Err(bad(
                        "construction-scaled hinge rigidity is not representable",
                    ));
                }
                bending.push(value);
            }
            Some(bending)
        } else {
            None
        };

        if let Some(mass) = new_mass {
            self.mass = mass;
        }
        if let Some(bending) = new_hinge_bending {
            self.hinge_bending = Some(bending);
        }
        Ok(field)
    }

    /// Convenience form of [`ClothModel::apply_construction_mechanics`] for standalone uniform-density
    /// meshes where unwelded connected components of `source_triangles` define each flat panel's
    /// local 2D frame.
    ///
    /// # Errors
    /// As [`ClothModel::apply_construction_mechanics`].
    pub fn apply_uniform_construction_mechanics(
        &mut self,
        source_rest_m: &[Vec2],
        source_triangles: &[[usize; 3]],
        weld: &[usize],
        base_areal_kg_m2: f64,
        options: ConstructionMechanicsOptions,
    ) -> Result<ConstructionField, Refusal> {
        let n_tris = source_triangles.len();
        let mut parent: Vec<usize> = (0..n_tris).collect();
        let find = |parent: &mut [usize], mut x: usize| {
            while parent[x] != x {
                parent[x] = parent[parent[x]];
                x = parent[x];
            }
            x
        };
        let mut src_edges: std::collections::BTreeMap<(usize, usize), usize> =
            std::collections::BTreeMap::new();
        for (t, tri) in source_triangles.iter().enumerate() {
            for k in 0..3 {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                let key = (a.min(b), a.max(b));
                if let Some(&prev) = src_edges.get(&key) {
                    let (ra, rb) = (find(&mut parent, prev), find(&mut parent, t));
                    if ra != rb {
                        parent[ra.max(rb)] = ra.min(rb);
                    }
                } else {
                    src_edges.insert(key, t);
                }
            }
        }
        let owners: Vec<usize> = (0..n_tris).map(|t| find(&mut parent, t)).collect();
        let densities = vec![base_areal_kg_m2; n_tris];
        self.apply_construction_mechanics(
            source_rest_m,
            source_triangles,
            &owners,
            weld,
            &densities,
            options,
        )
    }
}

#[cfg(test)]
mod construction_tests {
    use super::*;
    use crate::cloth::Solver;
    use ftx_core::Vec3;

    /// Builds a uniform rectangular grid `[0, lx] x [0, ly]` split into `nx x ny` cells (2 tris/cell).
    fn rect_strip(nx: usize, ny: usize, lx: f64, ly: f64) -> (Vec<Vec2>, Vec<[usize; 3]>) {
        let mut rest = Vec::with_capacity((nx + 1) * (ny + 1));
        for j in 0..=ny {
            for i in 0..=nx {
                rest.push(Vec2::new(
                    lx * i as f64 / nx as f64,
                    ly * j as f64 / ny as f64,
                ));
            }
        }
        let idx = |i: usize, j: usize| j * (nx + 1) + i;
        let mut tris = Vec::with_capacity(2 * nx * ny);
        for j in 0..ny {
            for i in 0..nx {
                let (v00, v10, v01, v11) =
                    (idx(i, j), idx(i + 1, j), idx(i, j + 1), idx(i + 1, j + 1));
                tris.push([v00, v10, v11]);
                tris.push([v00, v11, v01]);
            }
        }
        (rest, tris)
    }

    struct WeldedTwoPanel {
        source_rest: Vec<Vec2>,
        source_tris: Vec<[usize; 3]>,
        owner: Vec<usize>,
        weld: Vec<usize>,
        x0: Vec<Vec3>,
        tris: Vec<[usize; 3]>,
        tri_rest: Vec<[Vec2; 3]>,
    }

    /// Builds two flat rectangular panels `[0, lx] x [0, ly]` (left) and `[0, lx] x [0, ly]` (right)
    /// welded along the seam `left(x = lx) == right(x = 0)` so their combined world span is
    /// `[-lx, +lx] x [0, ly]` with the sewn seam on `x = 0`.
    fn welded_two_panel_strip(nx_half: usize, ny: usize, lx_half: f64, ly: f64) -> WeldedTwoPanel {
        let (r_l, t_l) = rect_strip(nx_half, ny, lx_half, ly);
        let (r_r, t_r) = rect_strip(nx_half, ny, lx_half, ly);
        let nv_half = r_l.len();
        let mut source_rest = r_l.clone();
        source_rest.extend_from_slice(&r_r);
        let mut source_tris = t_l.clone();
        source_tris.extend(t_r.iter().map(|t| t.map(|v| v + nv_half)));
        let mut owner = vec![0usize; t_l.len()];
        owner.extend(std::iter::repeat_n(1usize, t_r.len()));

        let mut weld = vec![0usize; 2 * nv_half];
        let mut next = 0usize;
        for slot in weld.iter_mut().take(nv_half) {
            *slot = next;
            next += 1;
        }
        for j in 0..=ny {
            for i in 0..=nx_half {
                let src = nv_half + j * (nx_half + 1) + i;
                if i == 0 {
                    // Weld right panel's left edge (i = 0) onto left panel's right edge (i = nx_half).
                    weld[src] = weld[j * (nx_half + 1) + nx_half];
                } else {
                    weld[src] = next;
                    next += 1;
                }
            }
        }
        let mut x0 = vec![Vec3::ZERO; next];
        for j in 0..=ny {
            for i in 0..=nx_half {
                let lv = j * (nx_half + 1) + i;
                x0[weld[lv]] = Vec3::new(r_l[lv].x - lx_half, r_l[lv].y, 0.0);
                let rv = nv_half + j * (nx_half + 1) + i;
                if i > 0 {
                    x0[weld[rv]] = Vec3::new(r_r[rv - nv_half].x, r_r[rv - nv_half].y, 0.0);
                }
            }
        }
        let tris: Vec<[usize; 3]> = source_tris.iter().map(|t| t.map(|v| weld[v])).collect();
        let tri_rest: Vec<[Vec2; 3]> = source_tris
            .iter()
            .map(|t| t.map(|v| source_rest[v]))
            .collect();
        WeldedTwoPanel {
            source_rest,
            source_tris,
            owner,
            weld,
            x0,
            tris,
            tri_rest,
        }
    }

    #[test]
    fn construction_mechanics_mass_conservation_validation_and_legacy_parity_kat() {
        // Serde round-trip and fail-closed validation.
        let std_opts = ConstructionMechanicsOptions::standard();
        std_opts.validate().unwrap();
        let json = serde_json::to_value(std_opts).unwrap();
        assert_eq!(
            serde_json::from_value::<ConstructionMechanicsOptions>(json).unwrap(),
            std_opts
        );
        assert!(
            serde_json::from_value::<ConstructionMechanicsOptions>(serde_json::json!({
                "unknown_field": 2.0
            }))
            .is_err()
        );
        assert!(ConstructionMechanicsOptions::default().is_default());
        assert!(!ConstructionMechanicsOptions::default().is_active());
        assert_eq!(
            ConstructionMechanicsOptions {
                hem_mass_multiplier: 2.0,
                hem_width_mm: 0.0,
                ..ConstructionMechanicsOptions::default()
            }
            .validate()
            .unwrap_err()
            .code,
            RefusalCode::OutOfRange
        );
        assert_eq!(
            ConstructionMechanicsOptions {
                seam_mass_multiplier: 2.0,
                seam_width_mm: 0.0,
                ..ConstructionMechanicsOptions::default()
            }
            .validate()
            .unwrap_err()
            .code,
            RefusalCode::OutOfRange
        );
        assert_eq!(
            ConstructionMechanicsOptions {
                hem_bending_multiplier: f64::NAN,
                hem_width_mm: 10.0,
                ..ConstructionMechanicsOptions::default()
            }
            .validate()
            .unwrap_err()
            .code,
            RefusalCode::NonFinite
        );
        // seam_bending_multiplier > 1 with seam_width_mm == 0 IS valid (welded seam line only).
        ConstructionMechanicsOptions {
            seam_bending_multiplier: 3.0,
            ..ConstructionMechanicsOptions::default()
        }
        .validate()
        .unwrap();

        // Two heterogeneous welded panels: left 0.15 kg/m², right 0.30 kg/m², each 60 mm x 40 mm
        // on an exact 10 mm grid (6 x 4 cells per panel).
        let WeldedTwoPanel {
            source_rest: src_rest,
            source_tris: src_tris,
            owner,
            weld,
            x0,
            tris,
            tri_rest,
        } = welded_two_panel_strip(6, 4, 0.06, 0.04);
        let mem = Membrane::orthotropic(1200.0, 1200.0, 0.2, 400.0, 0.05);
        let sheets: Vec<SheetMaterial> = owner
            .iter()
            .map(|&o| SheetMaterial {
                membrane: mem,
                bending_nm: if o == 0 { 2e-4 } else { 4e-4 },
                areal_kg_m2: if o == 0 { 0.15 } else { 0.30 },
            })
            .collect();
        let base_areal: Vec<f64> = sheets.iter().map(|s| s.areal_kg_m2).collect();
        let baseline =
            ClothModel::from_material_triangles(x0.len(), &tris, &tri_rest, &sheets).unwrap();

        // Default 1.0x is a bit-identical no-op.
        let mut noop = baseline.clone();
        let noop_field = noop
            .apply_construction_mechanics(
                &src_rest,
                &src_tris,
                &owner,
                &weld,
                &base_areal,
                ConstructionMechanicsOptions::default(),
            )
            .unwrap();
        assert_eq!(noop.mass, baseline.mass);
        assert_eq!(noop.hinge_bending, baseline.hinge_bending);
        assert_eq!(noop_field.seam_hinges, 4);
        assert_eq!(noop_field.hem_edges, 2 * 6 + 2 * 6 + 4 + 4); // 32 open outer edges; 4 seam edges excluded!

        // Apply 10 mm hem band (2.5x mass, 5x bending) + 10 mm seam allowance (2x mass, 3x bending).
        let opts = ConstructionMechanicsOptions {
            hem_width_mm: 10.0,
            hem_mass_multiplier: 2.5,
            hem_bending_multiplier: 5.0,
            seam_width_mm: 10.0,
            seam_mass_multiplier: 2.0,
            seam_bending_multiplier: 3.0,
        };
        let mut constructed = baseline.clone();
        let field = constructed
            .apply_construction_mechanics(&src_rest, &src_tris, &owner, &weld, &base_areal, opts)
            .unwrap();
        assert_eq!(field.seam_hinges, 4);
        // Exact analytical mass conservation: sum(model.mass) == sum_t(rho_t * area_t * mult_t).
        let expected_mass: f64 = constructed
            .membranes
            .iter()
            .zip(&base_areal)
            .zip(&field.triangle_mass_multiplier)
            .map(|((e, &rho), &mult)| rho * mult * e.area)
            .sum();
        let actual_mass: f64 = constructed.mass.iter().sum();
        assert!(
            (actual_mass - expected_mass).abs() <= 1e-15 * expected_mass,
            "mass conservation violated: actual={actual_mass} expected={expected_mass}"
        );
        // Check exact analytical hem & seam-allowance triangle counts on the 10 mm grid:
        // Each panel is 6x4 cells = 24 cells (48 tris).
        // Open edges on each panel are top (j=3), bottom (j=0), and outer side (i=0 for left, i=5 for right) ->
        // 6 + 6 + 2 = 14 hem cells per panel -> 28 hem tris per panel -> 56 hem tris total.
        // Seam cells on each panel are the 4 cells touching the seam (2 of which also touch top/bottom hem) ->
        // 8 seam-band tris per panel -> 16 seam-band tris total.
        assert_eq!(field.hem_triangles, 56);
        assert_eq!(field.seam_band_triangles, 16);
        // Interior non-hem, non-seam cells: columns i=1..=4 (excluding outer hem i=0 and seam i=5)
        // x inner rows j=1..=2 (excluding top/bottom hems j=0,3) = 8 cells = 16 tris per panel =
        // 32 single-ply tris total.
        let single_ply_tris = field
            .triangle_mass_multiplier
            .iter()
            .filter(|&&m| m == 1.0)
            .count();
        assert_eq!(single_ply_tris, 32);
    }

    #[test]
    fn folded_hem_cantilever_droop_reduction_kat() {
        // Horizontal clamped cantilever strip [0, 0.11] x [0, 0.06] m clamped at x <= 0.01 m
        // (two root columns pinned so root slope dz/dx = 0, leaving a 0.10 m free overhang).
        // Adding a 20 mm folded hem (2.0x mass, 6.0x bending) leaves a single-ply center strip
        // while raising width-averaged B / m along the hemmed perimeter, measurably reducing
        // vertical tip droop under gravity.
        let (nx, ny) = (11, 6);
        let (lx, ly) = (0.11, 0.06);
        let dx = lx / nx as f64;
        let (rest, tris) = rect_strip(nx, ny, lx, ly);
        let weld: Vec<usize> = (0..rest.len()).collect();
        let rho = 0.16;
        let bending = 2e-4;
        let mem = Membrane::orthotropic(1200.0, 1200.0, 0.2, 400.0, 0.05);

        let run_cantilever = |opts: ConstructionMechanicsOptions| -> (f64, Vec<Vec3>) {
            let mut model = ClothModel::new(rest.clone(), &tris, mem, bending, rho).unwrap();
            model
                .apply_uniform_construction_mechanics(&rest, &tris, &weld, rho, opts)
                .unwrap();
            let x0: Vec<Vec3> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
            let pinned: Vec<[bool; 3]> = rest
                .iter()
                .map(|p| {
                    if p.x <= dx + 1e-12 {
                        [true; 3]
                    } else {
                        [false; 3]
                    }
                })
                .collect();
            let mut solver = Solver::new(model, x0, &pinned, Vec3::new(0.0, 0.0, -9.81));
            solver.tol = 1e-9;
            for _ in 0..40 {
                let rep = solver.step(0.02).unwrap();
                assert!(rep.converged, "cantilever step failed to converge: {rep:?}");
            }
            // Mean vertical droop at the free tip x = lx.
            let mut tip_z = 0.0;
            let mut n_tip = 0usize;
            for (v, p) in rest.iter().enumerate() {
                if (p.x - lx).abs() <= 1e-12 {
                    tip_z += solver.x[v].z;
                    n_tip += 1;
                }
            }
            (-tip_z / n_tip as f64, solver.x)
        };

        let (droop_raw, x_raw) = run_cantilever(ConstructionMechanicsOptions::default());
        let hem_opts = ConstructionMechanicsOptions {
            hem_width_mm: 20.0,
            hem_mass_multiplier: 2.0,
            hem_bending_multiplier: 6.0,
            ..ConstructionMechanicsOptions::default()
        };
        let (droop_hem, x_hem) = run_cantilever(hem_opts);
        let (droop_hem_repeat, x_hem_repeat) = run_cantilever(hem_opts);
        assert_eq!(droop_hem, droop_hem_repeat);
        assert_eq!(
            x_hem, x_hem_repeat,
            "folded-hem drape must be deterministic"
        );
        assert!(
            droop_raw > 0.015,
            "raw single-ply cantilever should droop meaningfully: {droop_raw}"
        );
        assert!(
            droop_hem < 0.75 * droop_raw,
            "folded-hem cantilever should significantly reduce droop: raw={droop_raw:.5} m hem={droop_hem:.5} m"
        );
        assert_ne!(x_raw, x_hem);
    }

    #[test]
    fn structured_seam_ridge_profile_kat() {
        // Two 60 mm x 40 mm panels welded along a central seam at x = 0, pinned along the seam
        // line x = 0 so both wings hang down symmetrically under gravity (an inverted-V ridge).
        // Stiffening the welded seam hinges (4x bending) reduces the crease sharpness directly
        // across the sewn seam while leaving unsewn interior panel hinges at exact 1.0x rigidity.
        let WeldedTwoPanel {
            source_rest: src_rest,
            source_tris: src_tris,
            owner,
            weld,
            x0,
            tris,
            tri_rest,
        } = welded_two_panel_strip(6, 4, 0.06, 0.04);
        let rho = 0.18;
        let bending = 6e-5;
        let mem = Membrane::orthotropic(1200.0, 1200.0, 0.2, 400.0, 0.05);

        let seam_dihedral_rad = |model: &ClothModel, x: &[Vec3], is_seam: &[bool]| -> f64 {
            let mut sum = 0.0;
            let mut count = 0usize;
            for (h, &seam) in is_seam.iter().enumerate() {
                if !seam {
                    continue;
                }
                let [a, b, o1, o2] = model.hinges[h].v;
                let e = x[b] - x[a];
                let n1 = e.cross(x[o1] - x[a]).normalized();
                let n2 = (x[o2] - x[a]).cross(e).normalized();
                let cos_t = n1.dot(n2).clamp(-1.0, 1.0);
                let sin_t = n1.cross(n2).norm();
                sum += ftx_core::det::atan2(sin_t, cos_t);
                count += 1;
            }
            assert!(count > 0);
            sum / count as f64
        };

        let run_ridge = |opts: ConstructionMechanicsOptions| -> (f64, f64, Vec<f64>) {
            let mut model =
                ClothModel::from_triangles(x0.len(), &tris, &tri_rest, mem, bending, rho).unwrap();
            let field = model
                .apply_construction_mechanics(
                    &src_rest,
                    &src_tris,
                    &owner,
                    &weld,
                    &vec![rho; tris.len()],
                    opts,
                )
                .unwrap();
            assert_eq!(field.seam_hinges, 4);
            let is_seam: Vec<bool> = model
                .hinge_support
                .iter()
                .map(|&[(t1, _), (t2, _)]| {
                    src_tris[t1]
                        .iter()
                        .filter(|v| src_tris[t2].contains(v))
                        .count()
                        < 2
                })
                .collect();
            let rigidities: Vec<f64> = (0..model.hinges.len())
                .map(|h| model.hinge_rigidity(h))
                .collect();
            // Pin only the central seam line (x == 0); both wings hang freely under gravity.
            let pinned: Vec<[bool; 3]> = x0
                .iter()
                .map(|p| {
                    if p.x.abs() <= 1e-12 {
                        [true; 3]
                    } else {
                        [false; 3]
                    }
                })
                .collect();
            let mut solver = Solver::new(model, x0.clone(), &pinned, Vec3::new(0.0, 0.0, -9.81));
            solver.tol = 1e-9;
            for _ in 0..40 {
                let rep = solver.step(0.02).unwrap();
                assert!(rep.converged, "seam ridge step failed: {rep:?}");
            }
            let mut tip_z = 0.0;
            let mut n_tip = 0usize;
            for (v, p) in x0.iter().enumerate() {
                if (p.x.abs() - 0.06).abs() <= 1e-12 {
                    tip_z += solver.x[v].z;
                    n_tip += 1;
                }
            }
            (
                seam_dihedral_rad(&solver.model, &solver.x, &is_seam),
                -tip_z / n_tip as f64,
                rigidities,
            )
        };

        let (angle_raw, droop_raw, rig_raw) = run_ridge(ConstructionMechanicsOptions::default());
        let (angle_seam_line, droop_seam_line, rig_seam_line) =
            run_ridge(ConstructionMechanicsOptions {
                seam_bending_multiplier: 4.0,
                ..ConstructionMechanicsOptions::default()
            });
        let (angle_seam_band, droop_seam_band, _) = run_ridge(ConstructionMechanicsOptions {
            seam_width_mm: 10.0,
            seam_bending_multiplier: 4.0,
            ..ConstructionMechanicsOptions::default()
        });

        // Seam-line-only mode stiffens ONLY the 4 welded seam hinges by 4x and leaves every
        // interior panel hinge at exact single-ply bending rigidity.
        let mut stiffened_count = 0usize;
        for (&r0, &r1) in rig_raw.iter().zip(&rig_seam_line) {
            if (r1 - 4.0 * r0).abs() <= 1e-15 {
                stiffened_count += 1;
            } else {
                assert_eq!(r1, r0, "interior hinge rigidity must stay exact 1.0x");
            }
        }
        assert_eq!(stiffened_count, 4);
        assert!(
            angle_raw > 0.25,
            "raw seam should crease meaningfully under wing gravity: {angle_raw} rad"
        );
        assert!(
            angle_seam_line < 0.85 * angle_raw,
            "4x seam hinge should produce a gentler seam-ridge dihedral: raw={angle_raw:.4} seam={angle_seam_line:.4}"
        );
        assert!(
            angle_seam_band < 0.85 * angle_raw,
            "10 mm seam allowance band should maintain a gentler seam-ridge dihedral vs raw: raw={angle_raw:.4} band={angle_seam_band:.4}"
        );
        assert!(
            droop_seam_line < droop_raw,
            "4x seam hinge should lift the hanging wings relative to raw: raw={droop_raw:.5} m line={droop_seam_line:.5} m"
        );
        assert!(
            droop_seam_band < droop_seam_line,
            "10 mm seam allowance band should hold the wing profile flatter than stiffening only the seam line: line={droop_seam_line:.5} m band={droop_seam_band:.5} m"
        );
    }
}
