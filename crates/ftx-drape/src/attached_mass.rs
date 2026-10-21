//! Explicit retained trim/accessory point masses on the existing welded cloth degrees of freedom.
//! Their mass enters gravity and inertia only. No geometry, contact, heat storage or attachment
//! strength is inferred for an accessory from its mass or name.

use ftx_core::{Refusal, RefusalCode, Vec2, receipt::sha256_hex};
use ftx_pattern::{Pattern, material_mass::Attachments};
use serde::Serialize;
use serde_json::Value;

use crate::{ClothModel, GarmentMesh, MembraneElement};

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

/// A declared physical target and the actual meshed/welded vertex which receives its mass share.
#[derive(Clone, Debug, Serialize)]
pub struct Target {
    /// Authored piece that owns this physical instance.
    pub source_piece: String,
    /// Expanded physical instance ID.
    pub instance: String,
    /// Explicit boundary-vertex name in this instance.
    pub vertex: String,
    /// Corner in the original unwelded source mesh.
    pub mesh_vertex: usize,
    /// Actual retained mechanical degree of freedom after welding.
    pub solver_vertex: usize,
    /// Authored share of the group's total mass.
    pub fraction: f64,
    /// Nominal kilograms assigned to this target.
    pub mass_kg: f64,
}

/// One total source group allocated exactly once, including its floating-point allocation residual.
#[derive(Clone, Debug, Serialize)]
pub struct Group {
    /// Unique retained-extra group identity.
    pub id: String,
    /// Source-qualified total group mass.
    pub mass_kg: f64,
    /// Source endpoints, retained separately from nominal dynamics.
    pub reported_bounds_kg: Option<[f64; 2]>,
    /// Sum of the declared target shares after multiplication by group mass.
    pub allocated_mass_kg: f64,
    /// Allocation minus source mass, without hidden renormalization.
    pub allocation_residual_kg: f64,
    /// Every explicit physical target in authored order.
    pub targets: Vec<Target>,
}

/// Mechanical mass accounting and the exact source/target mapping used by the solver.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-attached-mass-report/1`.
    pub schema: &'static str,
    /// Exact original pattern extension, preserving its sources and numeric representation.
    pub input: Value,
    /// SHA-256 of the exact original extension's canonical JSON bytes.
    pub input_sha256: String,
    /// Lumped mass of retained simulated sheet triangles before extras are attached.
    pub cloth_mass_kg: f64,
    /// Sum of independently admitted total group masses, never multiplied by target count.
    pub attached_mass_kg: f64,
    /// Actual sum of the solver's lumped mass vector, used by gravity and implicit inertia.
    pub total_inertial_mass_kg: f64,
    /// Actual total minus cloth and source group masses; numerical, not an uncertainty interval.
    pub mass_balance_residual_kg: f64,
    /// Source groups and their actual sewn-mesh allocations.
    pub groups: Vec<Group>,
    /// Physical scope and explicitly unmodeled accessory properties.
    pub scope: &'static str,
}

/// Private binding catches edits to inertia, reference coordinates or retained element ownership.
#[derive(Clone, Debug)]
pub(crate) struct Binding {
    report: Report,
    mass: Vec<f64>,
    rest: Vec<Vec2>,
    elements: Vec<MembraneElement>,
}

impl Binding {
    pub(crate) fn check(&self, model: &ClothModel) -> Result<(), Refusal> {
        if self.mass != model.mass || self.rest != model.rest || self.elements != model.membranes {
            return Err(invalid(
                "attached mass cannot reuse a changed inertial mass, reference frame or retained topology; rebuild the scene",
            ));
        }
        Ok(())
    }
}

impl ClothModel {
    /// Validate an active attachment binding before consuming its inertial mass.
    ///
    /// # Errors
    /// The mass vector, rest coordinates or retained element topology changed after binding.
    pub fn check_attached_mass(&self) -> Result<(), Refusal> {
        if let Some(bound) = &self.attached_mass {
            bound.check(self)?;
        }
        Ok(())
    }

    /// Exact attachment receipt, absent on the unmodified sheet-only path.
    ///
    /// # Errors
    /// A stale inertial mass or reference topology cannot publish a current receipt.
    pub fn attached_mass_report(&self) -> Result<Option<&Report>, Refusal> {
        self.check_attached_mass()?;
        Ok(self.attached_mass.as_ref().map(|b| &b.report))
    }
}

/// Resolve named physical corners through the actual source mesh and weld map, then publish one
/// atomic mass-vector update. Called after all sheet material masses and seam welds are established.
pub(crate) fn bind(
    pattern: &Pattern,
    mesh: &GarmentMesh,
    weld: &[usize],
    model: &mut ClothModel,
) -> Result<(), Refusal> {
    let Some(request) = Attachments::from_pattern(pattern)? else {
        return Ok(());
    };
    if request.groups.is_empty() {
        return Ok(());
    }
    if model.attached_mass.is_some()
        || weld.len() != mesh.verts2d.len()
        || model.mass.len() != model.rest.len()
        || model.mass.iter().any(|m| !m.is_finite() || *m < 0.0)
    {
        return Err(invalid(
            "attached mass requires a pristine, finite sheet mass and complete weld map",
        ));
    }
    let cloth_mass_kg: f64 = model.mass.iter().sum();
    let mut mass = model.mass.clone();
    let mut groups = Vec::with_capacity(request.groups.len());
    let mut attached_mass_kg = 0.0;
    for group in &request.groups {
        let (group_mass, bounds) = group.extra.evaluate()?;
        let mut targets = Vec::with_capacity(group.targets.len());
        let mut allocated_mass = 0.0;
        for target in &group.targets {
            let key = (target.instance.clone(), target.vertex.clone());
            let mesh_vertex = *mesh.corner.get(&key).ok_or_else(|| {
                invalid(format!(
                    "attached mass target {}/{} is not a retained simulated corner",
                    target.instance, target.vertex
                ))
            })?;
            let solver_vertex = *weld
                .get(mesh_vertex)
                .filter(|&&v| v < mass.len())
                .ok_or_else(|| invalid("attached mass target has no valid welded vertex"))?;
            // A collapsed/orphaned point is not a load-bearing sheet degree of freedom.
            if model.mass[solver_vertex] <= 0.0 {
                return Err(invalid(
                    "attached mass target has no retained sheet support",
                ));
            }
            let share = group_mass * target.fraction;
            let updated = mass[solver_vertex] + share;
            if !share.is_finite()
                || share <= 0.0
                || !updated.is_finite()
                || updated <= mass[solver_vertex]
            {
                return Err(invalid("attached mass allocation is not representable"));
            }
            mass[solver_vertex] = updated;
            allocated_mass += share;
            let instance = pattern
                .instance(&target.instance)
                .expect("validated target");
            targets.push(Target {
                source_piece: instance.piece.clone(),
                instance: target.instance.clone(),
                vertex: target.vertex.clone(),
                mesh_vertex,
                solver_vertex,
                fraction: target.fraction,
                mass_kg: share,
            });
        }
        if !allocated_mass.is_finite()
            || (allocated_mass - group_mass).abs()
                > 64.0 * f64::EPSILON * group_mass * group.targets.len() as f64
        {
            return Err(invalid(
                "attached mass group allocation does not conserve its source mass",
            ));
        }
        attached_mass_kg += group_mass;
        groups.push(Group {
            id: group.extra.id.clone(),
            mass_kg: group_mass,
            reported_bounds_kg: bounds,
            allocated_mass_kg: allocated_mass,
            allocation_residual_kg: allocated_mass - group_mass,
            targets,
        });
    }
    let total_inertial_mass_kg: f64 = mass.iter().sum();
    let residual = total_inertial_mass_kg - cloth_mass_kg - attached_mass_kg;
    if !cloth_mass_kg.is_finite()
        || !attached_mass_kg.is_finite()
        || !total_inertial_mass_kg.is_finite()
        || !residual.is_finite()
    {
        return Err(invalid("attached or total inertial mass overflows"));
    }
    let input = pattern.raw()["attached_mass"].clone();
    let input_sha256 = sha256_hex(&serde_json::to_vec(&input).map_err(|e| invalid(e.to_string()))?);
    let binding = Binding {
        report: Report {
            schema: "ftx-attached-mass-report/1",
            input,
            input_sha256,
            cloth_mass_kg,
            attached_mass_kg,
            total_inertial_mass_kg,
            mass_balance_residual_kg: residual,
            groups,
            scope: "Explicit retained extra mass on existing welded cloth vertices supplies gravity and inertia. It supplies no accessory geometry, contact, thermal mass, rigidity, attachment strength or empirical garment validation. Reported source bounds remain accounting metadata; dynamics use admitted nominal masses.",
        },
        mass: mass.clone(),
        rest: model.rest.clone(),
        elements: model.membranes.clone(),
    };
    model.mass = mass;
    model.attached_mass = Some(binding);
    Ok(())
}
