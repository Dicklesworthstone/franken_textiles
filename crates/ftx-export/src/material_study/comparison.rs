//! Declared Pareto objectives over production study outputs; no surrogate solver or weighted score.
use std::collections::{BTreeMap, BTreeSet};

use ftx_core::Refusal;
use ftx_pattern::{
    material_catalog::{Selection, property_unit},
    material_screen::Direction,
    materials::{CATALOG_SCHEMA, Catalog},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Alternative, Request, ScenarioAlternative, cutting_effort, hash, invalid};

mod contact_loads;
#[cfg(test)]
mod heating_tests;
mod material_strain;
mod matrix;
mod motion_bridges;
mod steady_heat;
mod surface_water;
mod wet_film;

/// Uncertainty admission. Source intervals cannot be propagated through planners by relabeling them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// Explicitly compare nominal computed results, without a propagated uncertainty claim.
    Nominal,
    /// Require complete source-reported endpoints for assembly mass, steady heat, vapor or body water.
    /// Geometry, inventory, boundaries, initial liquid, supply and the sampled path stay fixed.
    ReportedBounds,
}

/// Consumer whose successful completion can be required independently of the chosen objectives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Consumer {
    /// Production roll cutting, not hypothetical net pattern area.
    Cutting,
    /// Complete source-qualified selected knife paths on every roll marker.
    CuttingProgram,
    /// Complete allocation on the shared, individually identified hide/remnant inventory.
    FiniteStock,
    /// Complete source-qualified selected knife paths on every used finite-stock item.
    FiniteStockProgram,
    /// Complete, exactly-once physical allocation across explicitly routed roll and finite stock.
    CuttingJob,
    /// Complete qualified knife work on every engaged, disjoint route of the complete job.
    CuttingJobProgram,
    /// Complete observed independent order cost across both stock routes in one declared market.
    CuttingJobPurchasing,
    /// Production seam/stack/recipe admission, not physical seam qualification.
    Sewing,
    /// Complete independently qualified thread channels for the actual selected sewing plan.
    SewingConsumables,
    /// Complete exact-yarn whole orders in the sewing consumer's dated market.
    SewingPurchasing,
    /// Complete declared fabric and selected-plan thread orders in one coherent source market.
    CutAndSewPurchasing,
    /// Complete marker-derived whole-order offer coverage in the request's dated market scope.
    Purchasing,
    /// Complete declared pre-finishing assembly inventory, not a purchased or simulated subset.
    AssemblyMass,
    /// Potential vapor transport through the explicitly declared common-area material stack.
    Vapor,
    /// Fresh, geometrically settled, gating-clear production drape in the retained scenario.
    Drape,
    /// Raw material-frame Green strain on every selected retained triangle of the admitted drape.
    MaterialStrain,
    /// Body barrier forces over an explicit union of welded vertices on the admitted initial drape.
    ContactLoads,
    /// Successful independent sheet-column heat on an admitted drape, not whole-body heat.
    Surface,
    /// Potential vapor transfer over current independent sheets on an admitted initial drape.
    VaporSurface,
    /// Successful declared body-patch/layer coverage heat on an admitted drape.
    Coverage,
    /// Potential vapor transfer on an admitted drape with every declared body patch covered.
    VaporCoverage,
    /// Complete prescribed motion starting at this admitted drape, not independent static poses.
    Motion,
    /// Raw material-frame strain over the initial state and every accepted motion sample.
    MotionMaterialStrain,
    /// Body barrier loads across the initial state and every accepted motion sample.
    MotionContactLoads,
    /// Complete potential vapor transfer on the declared covered body domain at every motion sample.
    MotionVaporCoverage,
    /// Conserved explicitly supplied body liquid over a complete covered-domain motion history.
    MotionWetFilm,
    /// Complete independent-sheet temperature history on every admitted mechanical sample.
    TransientSurface,
    /// Complete moving body-domain/exposed-cloth history, with its explicit layer/film policy.
    TransientCoverage,
}

/// Fixed-dimension output names; values are read only from the actual corresponding consumer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// Gross roll stock including marker waste/margins, in square metres.
    StockAreaM2,
    /// Outer cut area, in square metres; holes are not assumed recovered.
    ExternalCutAreaM2,
    /// Complete gross stock mass, in kilograms, never a partial sum.
    StockMassKg,
    /// Complete selected qualified roll-knife cut/travel/lift time, in seconds.
    CuttingModeledPathSeconds,
    /// Complete selected roll-knife cut length, including internal cuts, in metres.
    CuttingCutLengthM,
    /// Complete selected roll-knife raised travel, including requested return, in metres.
    CuttingRaisedTravelLengthM,
    /// Complete qualified knife time on used finite stock, in seconds.
    FiniteStockModeledPathSeconds,
    /// Complete qualified finite-stock knife cut length, in metres.
    FiniteStockCutLengthM,
    /// Complete qualified finite-stock raised-tool travel, in metres.
    FiniteStockRaisedTravelLengthM,
    /// Sum of selected qualified knife time across all engaged mixed-job routes; not makespan.
    CuttingJobModeledPathSeconds,
    /// Complete exactly-once knife cut length across a mixed job, in metres.
    CuttingJobCutLengthM,
    /// Complete selected raised-tool travel across a mixed job, in metres.
    CuttingJobRaisedTravelLengthM,
    /// All retained physical cut pieces plus sourced extras for ONE declared assembly (kg).
    AssemblyMassKg,
    /// Individually identified finite-stock items engaged by the complete allocation.
    UsedStockCount,
    /// Whole polygon area of used finite stock, including defects and prior occupied regions.
    UsedStockGrossAreaM2,
    /// Complete mass of used finite stock, including unusable/prior-cut regions, in kilograms.
    UsedStockMassKg,
    /// Newly allocated external cutter area on finite stock, in square metres.
    FiniteStockExternalCutAreaM2,
    /// Complete external cutter area across roll and finite-stock routes, in square metres.
    CuttingJobExternalCutAreaM2,
    /// Gross roll stock plus whole used finite-stock items, in square metres.
    CuttingJobStockAreaM2,
    /// Complete gross stock mass across both routes, including waste, in kilograms.
    CuttingJobStockMassKg,
    /// Complete observed independent order cost across both routes in the job's market currency.
    CuttingJobObservedOrderCost,
    /// Complete feasible selected job cost; split roll allocation may retain an unproved incumbent.
    CuttingJobSelectedOrderCost,
    /// Least observed independent whole-order total in the explicit market currency.
    ObservedOrderCost,
    /// Complete feasible selected roll-order total, without requiring an allocation minimum proof.
    SelectedObservedOrderCost,
    /// Traversed stitch intervals, including declared locks and repeated passes.
    StitchIntervals,
    /// Feed-only time in seconds; not a robot cycle-time estimate.
    FeedSeconds,
    /// Source-qualified directed setup time of the selected complete sewing schedule, in seconds.
    SewingSetupSeconds,
    /// Generated feed plus source-qualified setup time, excluding undeclared handling, in seconds.
    SewingFeedAndSetupSeconds,
    /// Qualified serial handling on the selected complete batch schedule, in seconds.
    SewingHandlingSeconds,
    /// Declared serial feed, setup and handling on the selected complete schedule, in seconds.
    SewingModeledProductionSeconds,
    /// Elapsed completion of a source-qualified whole-garment allocation or fixed specialist-cell
    /// route under its explicit millisecond convention; distinct from serial or machine work.
    SewingParallelMakespanSeconds,
    /// Complete thread length across the selected job's channels, including operation allowances.
    ThreadRequiredLengthM,
    /// Complete consumed inventory from the exact yarns' qualified linear densities, in kilograms.
    ThreadRequiredMassKg,
    /// Complete independent thread orders in their own currency; never a cut-and-sew subtotal.
    ThreadObservedOrderCost,
    /// Complete selected fabric plus thread merchandise cost for the same declared garment batch.
    CutAndSewSelectedOrderCost,
    /// Potential latent power (W) through the declared stack; water supply is not modeled.
    VaporPotentialHeatFlowW,
    /// Potential vapor mass flow (kg/s), using the same explicit boundaries and latent heat.
    VaporPotentialMassFlowKgS,
    /// Mass of simulated cloth only (kg); omitted pieces, trims and hardware are not included.
    SimulatedClothMassKg,
    /// Maximum absolute reported edge strain (%) across compression and extension, at report precision.
    MaxAbsoluteEdgeStrainPct,
    /// Magnitude of summed body barrier forces on selected welded vertices, in N.
    BodyContactResultantN,
    /// Largest body barrier-force magnitude at one selected welded vertex, in N.
    MaxBodyContactVertexForceN,
    /// Lowest raw warp Green strain E11 on the selected initial drape, dimensionless.
    MinWarpGreenStrain,
    /// Highest raw warp Green strain E11 on the selected initial drape, dimensionless.
    MaxWarpGreenStrain,
    /// Lowest raw weft Green strain E22 on the selected initial drape, dimensionless.
    MinWeftGreenStrain,
    /// Highest raw weft Green strain E22 on the selected initial drape, dimensionless.
    MaxWeftGreenStrain,
    /// Lowest signed raw engineering Green shear 2E12 on the selected initial drape.
    MinEngineeringShearGreenStrain,
    /// Highest signed raw engineering Green shear 2E12 on the selected initial drape.
    MaxEngineeringShearGreenStrain,
    /// Signed independent sheet-column power (W), not an absolute value or warmth rating.
    IndependentSurfacePowerW,
    /// Potential latent transfer (W) over current independent draped sheets, not wearer heat loss.
    IndependentVaporPotentialHeatFlowW,
    /// Potential vapor transfer (kg/s) over those sheets, with explicit reservoirs and latent heat.
    IndependentVaporPotentialMassFlowKgS,
    /// Signed heat through the declared body coverage domain (W), not assumed whole-wearer heat.
    BodyCoveragePowerW,
    /// Potential latent power over the fully covered declared body domain, in W.
    CoveredVaporPotentialHeatFlowW,
    /// Potential water-vapor flow over the fully covered declared body domain, in kg/s.
    CoveredVaporPotentialMassFlowKgS,
    /// Peak absolute original-panel edge strain (%) over the initial state and accepted motion samples.
    MotionMaxAbsoluteEdgeStrainPct,
    /// Peak selected body-force resultant magnitude over initial and accepted samples, in N.
    MotionPeakBodyContactResultantN,
    /// Largest selected nodal body-force magnitude over initial and accepted samples, in N.
    MotionMaxBodyContactVertexForceN,
    /// Lowest selected raw warp E11 over the initial state and every accepted motion sample.
    MotionMinWarpGreenStrain,
    /// Highest selected raw warp E11 over the initial state and every accepted motion sample.
    MotionMaxWarpGreenStrain,
    /// Lowest selected raw weft E22 over the initial state and every accepted motion sample.
    MotionMinWeftGreenStrain,
    /// Highest selected raw weft E22 over the initial state and every accepted motion sample.
    MotionMaxWeftGreenStrain,
    /// Lowest selected signed engineering Green shear 2E12 over initial and accepted motion states.
    MotionMinEngineeringShearGreenStrain,
    /// Highest selected signed engineering Green shear 2E12 over initial and accepted motion states.
    MotionMaxEngineeringShearGreenStrain,
    /// Declared quadrature of covered vapor potential over the requested motion, in joules.
    MotionCoveredVaporPotentialEnergyJ,
    /// Declared quadrature of covered vapor potential mass flow, in kilograms; no water supply.
    MotionCoveredVaporPotentialMassKg,
    /// Modeled covered evaporation limited by explicit body liquid and supply, in kilograms.
    MotionWetFilmEvaporatedMassKg,
    /// Liquid remaining on the declared body triangles at the end of the motion, in kilograms.
    MotionWetFilmRemainingLiquidMassKg,
    /// Covered potential demand that the declared finite liquid could not supply, in kilograms.
    MotionWetFilmUnmetDemandMassKg,
    /// Prescribed liquid supplied to the declared body triangles over the motion, in kilograms.
    MotionWetFilmSuppliedMassKg,
    /// Latent energy drawn from the prescribed body reservoir, not textile sensible heat, in joules.
    MotionWetFilmBodyReservoirLatentEnergyJ,
    /// Liquid evaporated from the qualified textile film, in kilograms; distinct from body water.
    TextileWaterEvaporatedMassKg,
    /// Liquid remaining on all qualified textile triangles at the final sample, in kilograms.
    TextileWaterRemainingMassKg,
    /// Independently sourced liquid delivered to qualified textile reference areas, in kilograms.
    TextileWaterSuppliedMassKg,
    /// Qualified reverse vapor transfer accumulated as textile free liquid, in kilograms.
    TextileWaterCondensedMassKg,
    /// Latent energy removed from the textile/liquid system over the complete motion, in joules.
    TextileWaterLatentEnergyJ,
    /// Latent energy released by qualified condensation into the textile/liquid system, in joules.
    TextileWaterCondensationLatentEnergyJ,
    /// Signed incoming-side energy (J) for all independent sheets over the whole requested motion.
    TransientSurfaceInsideEnergyJ,
    /// Signed incoming-side energy (J) of declared body paths AND exposed-cloth films, not wearer loss.
    TransientCoverageInsideEnergyJ,
    /// Lowest sheet temperature (C) among the initial state and retained mechanical sample endpoints.
    TransientSurfaceMinSampleTemperatureC,
    /// Highest sheet temperature (C) among the initial state and retained mechanical sample endpoints.
    TransientSurfaceMaxSampleTemperatureC,
    /// Lowest covered/exposed cloth temperature (C) at the initial and retained sample endpoints.
    TransientCoverageMinSampleTemperatureC,
    /// Highest covered/exposed cloth temperature (C) at the initial and retained sample endpoints.
    TransientCoverageMaxSampleTemperatureC,
    /// Signed net boundary energy of independent sheets over the complete motion (J).
    TransientSurfaceNetBoundaryEnergyJ,
    /// Signed physical boundary energy into covered/exposed cloth storage, excluding remap error (J).
    TransientCoverageNetBoundaryEnergyJ,
    /// Lowest independent-sheet cell temperature (C) at the final accepted mechanical sample.
    TransientSurfaceFinalMinTemperatureC,
    /// Highest independent-sheet cell temperature (C) at the final accepted mechanical sample.
    TransientSurfaceFinalMaxTemperatureC,
    /// Lowest covered/exposed cloth-cell temperature (C) at the final accepted mechanical sample.
    TransientCoverageFinalMinTemperatureC,
    /// Highest covered/exposed cloth-cell temperature (C) at the final accepted mechanical sample.
    TransientCoverageFinalMaxTemperatureC,
}

impl Metric {
    fn consumer(self) -> Consumer {
        match self {
            Self::StockAreaM2 | Self::ExternalCutAreaM2 | Self::StockMassKg => Consumer::Cutting,
            Self::CuttingModeledPathSeconds
            | Self::CuttingCutLengthM
            | Self::CuttingRaisedTravelLengthM => Consumer::CuttingProgram,
            Self::FiniteStockModeledPathSeconds
            | Self::FiniteStockCutLengthM
            | Self::FiniteStockRaisedTravelLengthM => Consumer::FiniteStockProgram,
            Self::CuttingJobModeledPathSeconds
            | Self::CuttingJobCutLengthM
            | Self::CuttingJobRaisedTravelLengthM => Consumer::CuttingJobProgram,
            Self::AssemblyMassKg => Consumer::AssemblyMass,
            Self::UsedStockCount
            | Self::UsedStockGrossAreaM2
            | Self::UsedStockMassKg
            | Self::FiniteStockExternalCutAreaM2 => Consumer::FiniteStock,
            Self::CuttingJobExternalCutAreaM2
            | Self::CuttingJobStockAreaM2
            | Self::CuttingJobStockMassKg => Consumer::CuttingJob,
            Self::CuttingJobObservedOrderCost | Self::CuttingJobSelectedOrderCost => {
                Consumer::CuttingJobPurchasing
            }
            Self::ObservedOrderCost | Self::SelectedObservedOrderCost => Consumer::Purchasing,
            Self::StitchIntervals
            | Self::FeedSeconds
            | Self::SewingSetupSeconds
            | Self::SewingFeedAndSetupSeconds
            | Self::SewingHandlingSeconds
            | Self::SewingModeledProductionSeconds
            | Self::SewingParallelMakespanSeconds => Consumer::Sewing,
            Self::ThreadRequiredLengthM | Self::ThreadRequiredMassKg => Consumer::SewingConsumables,
            Self::ThreadObservedOrderCost => Consumer::SewingPurchasing,
            Self::CutAndSewSelectedOrderCost => Consumer::CutAndSewPurchasing,
            Self::VaporPotentialHeatFlowW | Self::VaporPotentialMassFlowKgS => Consumer::Vapor,
            Self::SimulatedClothMassKg | Self::MaxAbsoluteEdgeStrainPct => Consumer::Drape,
            Self::BodyContactResultantN | Self::MaxBodyContactVertexForceN => {
                Consumer::ContactLoads
            }
            Self::MinWarpGreenStrain
            | Self::MaxWarpGreenStrain
            | Self::MinWeftGreenStrain
            | Self::MaxWeftGreenStrain
            | Self::MinEngineeringShearGreenStrain
            | Self::MaxEngineeringShearGreenStrain => Consumer::MaterialStrain,
            Self::IndependentSurfacePowerW => Consumer::Surface,
            Self::IndependentVaporPotentialHeatFlowW
            | Self::IndependentVaporPotentialMassFlowKgS => Consumer::VaporSurface,
            Self::BodyCoveragePowerW => Consumer::Coverage,
            Self::CoveredVaporPotentialHeatFlowW | Self::CoveredVaporPotentialMassFlowKgS => {
                Consumer::VaporCoverage
            }
            Self::MotionMaxAbsoluteEdgeStrainPct => Consumer::Motion,
            Self::MotionPeakBodyContactResultantN | Self::MotionMaxBodyContactVertexForceN => {
                Consumer::MotionContactLoads
            }
            Self::MotionMinWarpGreenStrain
            | Self::MotionMaxWarpGreenStrain
            | Self::MotionMinWeftGreenStrain
            | Self::MotionMaxWeftGreenStrain
            | Self::MotionMinEngineeringShearGreenStrain
            | Self::MotionMaxEngineeringShearGreenStrain => Consumer::MotionMaterialStrain,
            Self::MotionCoveredVaporPotentialEnergyJ | Self::MotionCoveredVaporPotentialMassKg => {
                Consumer::MotionVaporCoverage
            }
            Self::MotionWetFilmEvaporatedMassKg
            | Self::MotionWetFilmRemainingLiquidMassKg
            | Self::MotionWetFilmUnmetDemandMassKg
            | Self::MotionWetFilmSuppliedMassKg
            | Self::MotionWetFilmBodyReservoirLatentEnergyJ => Consumer::MotionWetFilm,
            Self::TransientSurfaceInsideEnergyJ
            | Self::TextileWaterEvaporatedMassKg
            | Self::TextileWaterRemainingMassKg
            | Self::TextileWaterSuppliedMassKg
            | Self::TextileWaterCondensedMassKg
            | Self::TextileWaterLatentEnergyJ
            | Self::TextileWaterCondensationLatentEnergyJ
            | Self::TransientSurfaceMinSampleTemperatureC
            | Self::TransientSurfaceMaxSampleTemperatureC
            | Self::TransientSurfaceNetBoundaryEnergyJ
            | Self::TransientSurfaceFinalMinTemperatureC
            | Self::TransientSurfaceFinalMaxTemperatureC => Consumer::TransientSurface,
            Self::TransientCoverageInsideEnergyJ
            | Self::TransientCoverageMinSampleTemperatureC
            | Self::TransientCoverageMaxSampleTemperatureC
            | Self::TransientCoverageNetBoundaryEnergyJ
            | Self::TransientCoverageFinalMinTemperatureC
            | Self::TransientCoverageFinalMaxTemperatureC => Consumer::TransientCoverage,
        }
    }
    fn unit(self, currency: Option<&str>) -> &str {
        match self {
            Self::StockAreaM2
            | Self::ExternalCutAreaM2
            | Self::UsedStockGrossAreaM2
            | Self::FiniteStockExternalCutAreaM2
            | Self::CuttingJobExternalCutAreaM2
            | Self::CuttingJobStockAreaM2 => "m2",
            Self::StockMassKg
            | Self::AssemblyMassKg
            | Self::UsedStockMassKg
            | Self::CuttingJobStockMassKg
            | Self::MotionCoveredVaporPotentialMassKg
            | Self::MotionWetFilmEvaporatedMassKg
            | Self::MotionWetFilmRemainingLiquidMassKg
            | Self::MotionWetFilmUnmetDemandMassKg
            | Self::MotionWetFilmSuppliedMassKg => "kg",
            Self::TextileWaterEvaporatedMassKg
            | Self::TextileWaterRemainingMassKg
            | Self::TextileWaterSuppliedMassKg
            | Self::TextileWaterCondensedMassKg
            | Self::ThreadRequiredMassKg => "kg",
            Self::ObservedOrderCost
            | Self::SelectedObservedOrderCost
            | Self::CuttingJobObservedOrderCost
            | Self::CuttingJobSelectedOrderCost
            | Self::CutAndSewSelectedOrderCost
            | Self::ThreadObservedOrderCost => currency.unwrap_or("unavailable currency"),
            Self::ThreadRequiredLengthM
            | Self::CuttingCutLengthM
            | Self::CuttingRaisedTravelLengthM
            | Self::FiniteStockCutLengthM
            | Self::FiniteStockRaisedTravelLengthM
            | Self::CuttingJobCutLengthM
            | Self::CuttingJobRaisedTravelLengthM => "m",
            Self::StitchIntervals | Self::UsedStockCount => "count",
            Self::FeedSeconds
            | Self::CuttingModeledPathSeconds
            | Self::FiniteStockModeledPathSeconds
            | Self::CuttingJobModeledPathSeconds
            | Self::SewingSetupSeconds
            | Self::SewingFeedAndSetupSeconds
            | Self::SewingHandlingSeconds
            | Self::SewingModeledProductionSeconds
            | Self::SewingParallelMakespanSeconds => "s",
            Self::VaporPotentialHeatFlowW
            | Self::CoveredVaporPotentialHeatFlowW
            | Self::IndependentVaporPotentialHeatFlowW => "W",
            Self::VaporPotentialMassFlowKgS
            | Self::CoveredVaporPotentialMassFlowKgS
            | Self::IndependentVaporPotentialMassFlowKgS => "kg/s",
            Self::SimulatedClothMassKg => "kg",
            Self::MaxAbsoluteEdgeStrainPct | Self::MotionMaxAbsoluteEdgeStrainPct => "%",
            Self::BodyContactResultantN
            | Self::MaxBodyContactVertexForceN
            | Self::MotionPeakBodyContactResultantN
            | Self::MotionMaxBodyContactVertexForceN => "N",
            Self::MinWarpGreenStrain
            | Self::MaxWarpGreenStrain
            | Self::MinWeftGreenStrain
            | Self::MaxWeftGreenStrain
            | Self::MinEngineeringShearGreenStrain
            | Self::MaxEngineeringShearGreenStrain
            | Self::MotionMinWarpGreenStrain
            | Self::MotionMaxWarpGreenStrain
            | Self::MotionMinWeftGreenStrain
            | Self::MotionMaxWeftGreenStrain
            | Self::MotionMinEngineeringShearGreenStrain
            | Self::MotionMaxEngineeringShearGreenStrain => "1",
            Self::IndependentSurfacePowerW | Self::BodyCoveragePowerW => "W",
            Self::TransientSurfaceInsideEnergyJ
            | Self::TransientCoverageInsideEnergyJ
            | Self::TransientSurfaceNetBoundaryEnergyJ
            | Self::TransientCoverageNetBoundaryEnergyJ
            | Self::MotionCoveredVaporPotentialEnergyJ
            | Self::MotionWetFilmBodyReservoirLatentEnergyJ => "J",
            Self::TextileWaterLatentEnergyJ | Self::TextileWaterCondensationLatentEnergyJ => "J",
            Self::TransientSurfaceMinSampleTemperatureC
            | Self::TransientSurfaceMaxSampleTemperatureC
            | Self::TransientCoverageMinSampleTemperatureC
            | Self::TransientCoverageMaxSampleTemperatureC
            | Self::TransientSurfaceFinalMinTemperatureC
            | Self::TransientSurfaceFinalMaxTemperatureC
            | Self::TransientCoverageFinalMinTemperatureC
            | Self::TransientCoverageFinalMaxTemperatureC => "C",
        }
    }
    fn value(self, row: &Alternative, admitted: &Numerical<'_>) -> Option<f64> {
        let simulation = admitted.drape;
        match self {
            Self::StockAreaM2 => row.metrics.stock_area_m2,
            Self::ExternalCutAreaM2 => row.metrics.external_cut_area_m2,
            Self::StockMassKg => row.metrics.stock_mass_kg,
            Self::CuttingModeledPathSeconds => admitted.cutting_program.map(|r| r.totals.seconds),
            Self::CuttingCutLengthM => admitted.cutting_program.map(|r| r.totals.cut_m),
            Self::CuttingRaisedTravelLengthM => admitted.cutting_program.map(|r| r.totals.travel_m),
            Self::FiniteStockModeledPathSeconds => {
                admitted.finite_stock_program.map(|r| r.totals.seconds)
            }
            Self::FiniteStockCutLengthM => admitted.finite_stock_program.map(|r| r.totals.cut_m),
            Self::FiniteStockRaisedTravelLengthM => {
                admitted.finite_stock_program.map(|r| r.totals.travel_m)
            }
            Self::CuttingJobModeledPathSeconds => {
                admitted.cutting_job_program.map(|r| r.totals.seconds)
            }
            Self::CuttingJobCutLengthM => admitted.cutting_job_program.map(|r| r.totals.cut_m),
            Self::CuttingJobRaisedTravelLengthM => {
                admitted.cutting_job_program.map(|r| r.totals.travel_m)
            }
            Self::AssemblyMassKg => row.assembly_mass.value().and_then(|r| r.assembly_mass.kg),
            Self::UsedStockCount => row.metrics.used_stock_count.map(|n| n as f64),
            Self::UsedStockGrossAreaM2 => row.metrics.used_stock_gross_area_m2,
            Self::UsedStockMassKg => row.metrics.used_stock_mass_kg,
            Self::FiniteStockExternalCutAreaM2 => row.metrics.finite_stock_external_cut_area_m2,
            Self::CuttingJobExternalCutAreaM2 => row.metrics.cutting_job_external_cut_area_m2,
            Self::CuttingJobStockAreaM2 => row.metrics.cutting_job_stock_area_m2,
            Self::CuttingJobStockMassKg => row.metrics.cutting_job_stock_mass_kg,
            Self::CuttingJobObservedOrderCost => row.metrics.cutting_job_observed_order_cost,
            Self::CuttingJobSelectedOrderCost => row.metrics.cutting_job_selected_order_cost,
            Self::ObservedOrderCost => row.metrics.least_observed_independent_order_total,
            Self::SelectedObservedOrderCost => row.metrics.selected_observed_order_total,
            Self::StitchIntervals => row.metrics.stitch_intervals.map(|n| n as f64),
            Self::FeedSeconds => row.metrics.feed_seconds,
            Self::SewingSetupSeconds => row.metrics.sewing_setup_seconds,
            Self::SewingFeedAndSetupSeconds => row.metrics.sewing_feed_and_setup_seconds,
            Self::SewingHandlingSeconds => row.metrics.sewing_handling_seconds,
            Self::SewingModeledProductionSeconds => row.metrics.sewing_modeled_production_seconds,
            Self::SewingParallelMakespanSeconds => row.metrics.sewing_parallel_makespan_seconds,
            Self::ThreadRequiredLengthM => row.metrics.thread_required_length_m,
            Self::ThreadRequiredMassKg => row.metrics.thread_required_mass_kg,
            Self::ThreadObservedOrderCost => row.metrics.thread_observed_order_cost,
            Self::CutAndSewSelectedOrderCost => row.metrics.cut_and_sew_selected_order_cost,
            Self::VaporPotentialHeatFlowW => row.vapor.value().map(|r| r.potential_heat_flow_w),
            Self::VaporPotentialMassFlowKgS => {
                row.vapor.value().map(|r| r.potential_mass_flow_kg_s)
            }
            Self::SimulatedClothMassKg => simulation["drape"]["mass_kg"].as_f64(),
            Self::BodyContactResultantN => {
                admitted.contact_loads?["peak_body_resultant_n"].as_f64()
            }
            Self::MaxBodyContactVertexForceN => {
                admitted.contact_loads?["max_body_vertex_force_n"].as_f64()
            }
            Self::MotionPeakBodyContactResultantN => {
                admitted.motion_contact_loads?["peak_body_resultant_n"].as_f64()
            }
            Self::MotionMaxBodyContactVertexForceN => {
                admitted.motion_contact_loads?["max_body_vertex_force_n"].as_f64()
            }
            Self::MaxAbsoluteEdgeStrainPct => {
                let strain = &simulation["drape"]["global_strain_pct"];
                let lo = strain["min"].as_f64()?;
                let hi = strain["max"].as_f64()?;
                (lo.is_finite() && hi.is_finite() && lo <= hi).then(|| lo.abs().max(hi.abs()))
            }
            Self::MinWarpGreenStrain => {
                admitted.material_strain?["green_strain_range"][0][0].as_f64()
            }
            Self::MaxWarpGreenStrain => {
                admitted.material_strain?["green_strain_range"][0][1].as_f64()
            }
            Self::MinWeftGreenStrain => {
                admitted.material_strain?["green_strain_range"][1][0].as_f64()
            }
            Self::MaxWeftGreenStrain => {
                admitted.material_strain?["green_strain_range"][1][1].as_f64()
            }
            Self::MinEngineeringShearGreenStrain => {
                admitted.material_strain?["green_strain_range"][2][0].as_f64()
            }
            Self::MaxEngineeringShearGreenStrain => {
                admitted.material_strain?["green_strain_range"][2][1].as_f64()
            }
            Self::IndependentSurfacePowerW => {
                simulation["surface"]["result"]["steady"]["power_w"].as_f64()
            }
            Self::IndependentVaporPotentialHeatFlowW => {
                admitted.vapor_surface["potential_heat_flow_w"].as_f64()
            }
            Self::IndependentVaporPotentialMassFlowKgS => {
                admitted.vapor_surface["potential_mass_flow_kg_s"].as_f64()
            }
            Self::BodyCoveragePowerW => {
                simulation["coverage"]["result"]["steady"]["power_w"].as_f64()
            }
            Self::CoveredVaporPotentialHeatFlowW => {
                admitted.vapor_coverage?["coverage"]["covered_potential_heat_flow_w"].as_f64()
            }
            Self::CoveredVaporPotentialMassFlowKgS => {
                admitted.vapor_coverage?["coverage"]["covered_potential_mass_flow_kg_s"].as_f64()
            }
            Self::MotionMaxAbsoluteEdgeStrainPct => {
                admitted.motion["metrics"]["max_absolute_edge_strain_pct"].as_f64()
            }
            Self::MotionMinWarpGreenStrain => {
                admitted.motion_material_strain?["green_strain_range"][0][0].as_f64()
            }
            Self::MotionMaxWarpGreenStrain => {
                admitted.motion_material_strain?["green_strain_range"][0][1].as_f64()
            }
            Self::MotionMinWeftGreenStrain => {
                admitted.motion_material_strain?["green_strain_range"][1][0].as_f64()
            }
            Self::MotionMaxWeftGreenStrain => {
                admitted.motion_material_strain?["green_strain_range"][1][1].as_f64()
            }
            Self::MotionMinEngineeringShearGreenStrain => {
                admitted.motion_material_strain?["green_strain_range"][2][0].as_f64()
            }
            Self::MotionMaxEngineeringShearGreenStrain => {
                admitted.motion_material_strain?["green_strain_range"][2][1].as_f64()
            }
            Self::MotionCoveredVaporPotentialEnergyJ => {
                admitted.motion_vapor_coverage?["totals"]["potential_energy_j"].as_f64()
            }
            Self::MotionCoveredVaporPotentialMassKg => {
                admitted.motion_vapor_coverage?["totals"]["potential_mass_kg"].as_f64()
            }
            Self::MotionWetFilmEvaporatedMassKg => {
                admitted.motion_wet_film?["totals"]["evaporated_mass_kg"].as_f64()
            }
            Self::MotionWetFilmRemainingLiquidMassKg => {
                admitted.motion_wet_film?["totals"]["remaining_liquid_mass_kg"].as_f64()
            }
            Self::MotionWetFilmUnmetDemandMassKg => {
                admitted.motion_wet_film?["totals"]["unmet_demand_mass_kg"].as_f64()
            }
            Self::MotionWetFilmSuppliedMassKg => {
                admitted.motion_wet_film?["totals"]["supplied_mass_kg"].as_f64()
            }
            Self::MotionWetFilmBodyReservoirLatentEnergyJ => {
                admitted.motion_wet_film?["totals"]["body_reservoir_latent_energy_j"].as_f64()
            }
            Self::TextileWaterEvaporatedMassKg => {
                admitted.transient_surface.as_ref()?.result["surface_water"]["evaporated_liquid_kg"]
                    .as_f64()
            }
            Self::TextileWaterRemainingMassKg => {
                admitted.transient_surface.as_ref()?.result["surface_water"]["remaining_liquid_kg"]
                    .as_f64()
            }
            Self::TextileWaterSuppliedMassKg => {
                admitted.transient_surface.as_ref()?.result["surface_water"]["supplied_liquid_kg"]
                    .as_f64()
            }
            Self::TextileWaterCondensedMassKg => {
                admitted.transient_surface.as_ref()?.result["surface_water"]["condensed_liquid_kg"]
                    .as_f64()
            }
            Self::TextileWaterLatentEnergyJ => {
                admitted.transient_surface.as_ref()?.result["surface_water"]["latent_energy_j"]
                    .as_f64()
            }
            Self::TextileWaterCondensationLatentEnergyJ => admitted
                .transient_surface
                .as_ref()?
                .result["surface_water"]["condensation_latent_energy_j"]
                .as_f64(),
            Self::TransientSurfaceInsideEnergyJ => {
                admitted.transient_surface.as_ref()?.result["inside_energy_j"].as_f64()
            }
            Self::TransientCoverageInsideEnergyJ => {
                admitted.transient_coverage.as_ref()?.result["inside_energy_j"].as_f64()
            }
            Self::TransientSurfaceMinSampleTemperatureC => {
                Some(admitted.transient_surface.as_ref()?.range_c[0])
            }
            Self::TransientSurfaceMaxSampleTemperatureC => {
                Some(admitted.transient_surface.as_ref()?.range_c[1])
            }
            Self::TransientCoverageMinSampleTemperatureC => {
                Some(admitted.transient_coverage.as_ref()?.range_c[0])
            }
            Self::TransientCoverageMaxSampleTemperatureC => {
                Some(admitted.transient_coverage.as_ref()?.range_c[1])
            }
            Self::TransientSurfaceNetBoundaryEnergyJ => {
                admitted.transient_surface.as_ref()?.result["net_boundary_energy_j"].as_f64()
            }
            Self::TransientCoverageNetBoundaryEnergyJ => {
                admitted.transient_coverage.as_ref()?.result["net_boundary_energy_j"].as_f64()
            }
            Self::TransientSurfaceFinalMinTemperatureC => {
                Some(admitted.transient_surface.as_ref()?.final_range_c[0])
            }
            Self::TransientSurfaceFinalMaxTemperatureC => {
                Some(admitted.transient_surface.as_ref()?.final_range_c[1])
            }
            Self::TransientCoverageFinalMinTemperatureC => {
                Some(admitted.transient_coverage.as_ref()?.final_range_c[0])
            }
            Self::TransientCoverageFinalMaxTemperatureC => {
                Some(admitted.transient_coverage.as_ref()?.final_range_c[1])
            }
        }
    }
    fn currency<'a>(
        self,
        row: &'a Alternative,
        cutting_currency: Option<&'a str>,
    ) -> Option<&'a str> {
        if self == Self::CutAndSewSelectedOrderCost {
            row.cut_and_sew_purchasing
                .value()
                .map(|p| p.market.currency.as_str())
        } else if self == Self::ThreadObservedOrderCost {
            row.sewing
                .value()?
                .options
                .consumables
                .as_ref()?
                .purchasing
                .as_ref()
                .map(|p| p.market.currency.as_str())
        } else {
            cutting_currency
        }
    }

    fn signed(self) -> bool {
        matches!(
            self,
            Self::MinWarpGreenStrain
                | Self::MaxWarpGreenStrain
                | Self::MinWeftGreenStrain
                | Self::MaxWeftGreenStrain
                | Self::MinEngineeringShearGreenStrain
                | Self::MaxEngineeringShearGreenStrain
                | Self::MotionMinWarpGreenStrain
                | Self::MotionMaxWarpGreenStrain
                | Self::MotionMinWeftGreenStrain
                | Self::MotionMaxWeftGreenStrain
                | Self::MotionMinEngineeringShearGreenStrain
                | Self::MotionMaxEngineeringShearGreenStrain
                | Self::IndependentSurfacePowerW
                | Self::BodyCoveragePowerW
                | Self::TransientSurfaceInsideEnergyJ
                | Self::TransientCoverageInsideEnergyJ
                | Self::TransientSurfaceMinSampleTemperatureC
                | Self::TransientSurfaceMaxSampleTemperatureC
                | Self::TransientCoverageMinSampleTemperatureC
                | Self::TransientCoverageMaxSampleTemperatureC
                | Self::TransientSurfaceNetBoundaryEnergyJ
                | Self::TransientCoverageNetBoundaryEnergyJ
                | Self::TransientSurfaceFinalMinTemperatureC
                | Self::TransientSurfaceFinalMaxTemperatureC
                | Self::TransientCoverageFinalMinTemperatureC
                | Self::TransientCoverageFinalMaxTemperatureC
        )
    }

    fn supports_bounds(self) -> bool {
        matches!(
            self,
            Self::AssemblyMassKg
                | Self::IndependentSurfacePowerW
                | Self::BodyCoveragePowerW
                | Self::VaporPotentialHeatFlowW
                | Self::VaporPotentialMassFlowKgS
                | Self::IndependentVaporPotentialHeatFlowW
                | Self::IndependentVaporPotentialMassFlowKgS
                | Self::CoveredVaporPotentialHeatFlowW
                | Self::CoveredVaporPotentialMassFlowKgS
                | Self::MotionCoveredVaporPotentialEnergyJ
                | Self::MotionCoveredVaporPotentialMassKg
                | Self::MotionWetFilmEvaporatedMassKg
                | Self::MotionWetFilmRemainingLiquidMassKg
                | Self::MotionWetFilmUnmetDemandMassKg
                | Self::MotionWetFilmSuppliedMassKg
                | Self::MotionWetFilmBodyReservoirLatentEnergyJ
        )
    }

    fn reported_bounds(
        self,
        row: &Alternative,
        derived: &BTreeMap<Consumer, TransferBounds>,
        water: &BTreeMap<Metric, [f64; 2]>,
        heat: &BTreeMap<Consumer, [f64; 2]>,
    ) -> Option<[f64; 2]> {
        match self {
            Self::AssemblyMassKg => row.assembly_mass.value()?.assembly_mass.reported_bounds_kg,
            Self::IndependentSurfacePowerW | Self::BodyCoveragePowerW => {
                heat.get(&self.consumer()).copied()
            }
            Self::VaporPotentialHeatFlowW => {
                Some(row.vapor.value()?.reported_bounds?.potential_heat_flow_w)
            }
            Self::VaporPotentialMassFlowKgS => {
                Some(row.vapor.value()?.reported_bounds?.potential_mass_flow_kg_s)
            }
            Self::IndependentVaporPotentialHeatFlowW
            | Self::CoveredVaporPotentialHeatFlowW
            | Self::MotionCoveredVaporPotentialEnergyJ => Some(derived.get(&self.consumer())?[0]),
            Self::IndependentVaporPotentialMassFlowKgS
            | Self::CoveredVaporPotentialMassFlowKgS
            | Self::MotionCoveredVaporPotentialMassKg => Some(derived.get(&self.consumer())?[1]),
            Self::MotionWetFilmEvaporatedMassKg
            | Self::MotionWetFilmRemainingLiquidMassKg
            | Self::MotionWetFilmUnmetDemandMassKg
            | Self::MotionWetFilmSuppliedMassKg
            | Self::MotionWetFilmBodyReservoirLatentEnergyJ => water.get(&self).copied(),
            _ => None,
        }
    }
}

/// One independent preference; every objective must be available for a row to be eligible.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Objective {
    /// Computed output and its fixed physical dimension.
    pub metric: Metric,
    /// Smaller or larger, explicitly chosen even for signed quantities.
    pub direction: Direction,
}

/// Inclusive constraints on computed outputs, in the metric's fixed units.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Constraint {
    /// Actual consumer output to constrain.
    pub metric: Metric,
    /// Explicit decision basis for this guard; omission inherits the objective policy's basis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basis: Option<Basis>,
    /// Inclusive lower bound; at least one bound is required.
    pub minimum: Option<f64>,
    /// Inclusive upper bound; no unit inference or clamping is performed.
    pub maximum: Option<f64>,
}

/// An opt-in comparison of alternatives evaluated in ONE study and process context.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// Explicit nominal outputs or supported conditional mass/heat/vapor/body-water source bounds.
    pub basis: Basis,
    /// One to sixteen unique objective metrics. No hidden scalarization or automatic winner.
    pub objectives: Vec<Objective>,
    /// At most 64 unique-metric inclusive constraints.
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    /// Additional consumers required even when they contribute no objective.
    #[serde(default)]
    pub required_consumers: Vec<Consumer>,
    /// False keeps different reported input methods/conditions/evidence incomparable. True is an
    /// explicit relaxation, not evidence of experimental comparability or permission for estimates.
    #[serde(default)]
    pub allow_mixed_protocols: bool,
}

/// One admitted numerical output. The source plans and observations remain in the containing study.
#[derive(Clone, Debug, Serialize)]
pub struct Reading {
    /// Finite computed value, including genuine zero; unknown values have no reading.
    pub value: f64,
    /// Complete conditional endpoints in `unit` when an objective or guard requires them. An
    /// absent required range excludes that candidate; it never implies an exact nominal observation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reported_bounds: Option<[f64; 2]>,
    /// Fixed physical unit or exact currency; no conversions occur in the comparator.
    pub unit: String,
    /// Required production consumer, not a value copied from scalar screening.
    pub consumer: Consumer,
}

/// Explicit eligibility and dominance, independent of the source report's consumer outcomes.
#[derive(Clone, Debug, Serialize)]
pub struct Candidate {
    /// Stable alternative key.
    pub key: String,
    /// Available objective/constraint readings; missing values are not inserted as zero.
    pub readings: BTreeMap<Metric, Reading>,
    /// Required consumer, missing metric or failed constraint reasons.
    pub exclusions: Vec<Refusal>,
    /// All requested objectives, constraints and additional consumers were admitted.
    pub eligible: bool,
    /// Identity of relevant reported input protocols by source piece. A match is not validation.
    pub input_protocol_sha256: Option<String>,
    /// Eligible comparable alternatives no worse in all objectives and strictly better in at least one.
    pub dominated_by: Vec<String>,
}

/// An eligible pair whose protocols or reported ranges do not establish an ordering.
#[derive(Clone, Debug, Serialize)]
pub struct Incomparable {
    /// Stable keys in deterministic order.
    pub alternatives: [String; 2],
    /// Why numerical values alone did not establish dominance.
    pub reason: &'static str,
}

/// Conditional Pareto set among evaluated, eligible alternatives, never a global material optimum.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-material-comparison/1`.
    pub schema: &'static str,
    /// Exact source geometry/material document used by the containing study.
    pub source_pattern_sha256: String,
    /// Entire study/process policy identity, not merely the objective list.
    pub context_sha256: String,
    /// The user's complete comparison policy, including uncertainty and protocol admission.
    pub policy: Policy,
    /// Key-ordered candidate outcomes; excluded alternatives stay visible.
    pub alternatives: Vec<Candidate>,
    /// Eligible non-dominated keys; ties and incomparable candidates remain alternatives.
    pub frontier: Vec<String>,
    /// Explicit protocol incompatibilities or unresolved reported-range overlap.
    pub incomparable: Vec<Incomparable>,
    /// Always false. Conditional source endpoints are not a general uncertainty estimate for the
    /// geometry, inventory, numerical solvers or physical garment.
    pub uncertainty_propagated: bool,
    /// Always false, including when all computed constraints pass.
    pub physical_validation_claim: bool,
    /// True only when the policy requires complete motion, directly or through a motion objective.
    /// False means a static/manufacturing frontier, even if the report also contains a trajectory.
    pub motion_required: bool,
    /// Boundaries of the comparison and its source-data admission.
    pub notes: Vec<&'static str>,
}

impl Policy {
    /// Validate the declared comparison before expensive fitting/planning. Scenario outputs are
    /// admitted only through an explicitly requested additional production consumer.
    ///
    /// # Errors
    /// Invalid bounds, duplicate or missing objectives, or a required unrequested consumer.
    pub fn validate(&self, request: &Request, additional_consumer: bool) -> Result<(), Refusal> {
        if self.objectives.is_empty()
            || self.objectives.len() > 16
            || self.constraints.len() > 64
            || self.required_consumers.len() > 16
        {
            return Err(invalid(
                "comparison needs 1..16 objectives, at most 64 constraints and 16 consumers",
            ));
        }
        let mut metrics = BTreeSet::new();
        if self.objectives.iter().any(|o| !metrics.insert(o.metric)) {
            return Err(invalid("comparison objective metrics must be unique"));
        }
        metrics.clear();
        for c in &self.constraints {
            if !metrics.insert(c.metric)
                || (c.minimum.is_none() && c.maximum.is_none())
                || c.minimum.is_some_and(|x| !x.is_finite())
                || c.maximum.is_some_and(|x| !x.is_finite())
                || matches!((c.minimum, c.maximum), (Some(a), Some(b)) if a > b)
            {
                return Err(invalid(
                    "comparison constraints need unique metrics and finite ordered bounds",
                ));
            }
        }
        if self.bounded_metrics().iter().any(|m| !m.supports_bounds()) {
            return Err(invalid(
                "reported_bounds comparison supports assembly_mass_kg, constant steady heat, vapor potential and final finite-water metrics only; other computed objectives and constraints have no propagated reported bounds",
            ));
        }
        let unique: BTreeSet<_> = self.required_consumers.iter().collect();
        if unique.len() != self.required_consumers.len() {
            return Err(invalid("comparison required consumers must be unique"));
        }
        for consumer in self.consumers() {
            let requested = match consumer {
                Consumer::Cutting => request.cutting.is_some(),
                Consumer::CuttingProgram => request
                    .cutting
                    .as_ref()
                    .is_some_and(|o| o.program.is_some()),
                Consumer::FiniteStock => request.finite_stock.is_some(),
                Consumer::FiniteStockProgram => request
                    .finite_stock
                    .as_ref()
                    .is_some_and(|r| r.options.program.is_some()),
                Consumer::CuttingJob => request.cutting_job.is_some(),
                Consumer::CuttingJobProgram => request.cutting_job.as_ref().is_some_and(|job| {
                    job.roll.as_ref().is_some_and(|o| o.program.is_some())
                        || job
                            .finite_stock
                            .as_ref()
                            .is_some_and(|r| r.options.program.is_some())
                }),
                Consumer::CuttingJobPurchasing => request
                    .cutting_job
                    .as_ref()
                    .is_some_and(|job| job.purchasing.is_some()),
                Consumer::Sewing => request.sewing.is_some() || request.sewing_selection.is_some(),
                Consumer::SewingConsumables => request
                    .sewing_options()
                    .is_some_and(|s| s.consumables.is_some()),
                Consumer::SewingPurchasing => request
                    .sewing_options()
                    .and_then(|s| s.consumables.as_ref())
                    .is_some_and(|c| c.purchasing.is_some()),
                Consumer::CutAndSewPurchasing => request.cut_and_sew_purchasing,
                Consumer::Purchasing => request.purchasing.is_some(),
                Consumer::AssemblyMass => request.assembly_mass.is_some(),
                Consumer::Vapor => request.vapor.is_some(),
                Consumer::Drape
                | Consumer::MaterialStrain
                | Consumer::ContactLoads
                | Consumer::Surface
                | Consumer::VaporSurface
                | Consumer::Coverage
                | Consumer::VaporCoverage
                | Consumer::Motion
                | Consumer::MotionMaterialStrain
                | Consumer::MotionContactLoads
                | Consumer::MotionVaporCoverage
                | Consumer::MotionWetFilm
                | Consumer::TransientSurface
                | Consumer::TransientCoverage => additional_consumer,
            };
            if !requested {
                return Err(invalid(format!(
                    "comparison requires unrequested {consumer:?} consumer"
                )));
            }
        }
        Ok(())
    }

    fn metrics(&self) -> BTreeSet<Metric> {
        self.objectives
            .iter()
            .map(|o| o.metric)
            .chain(self.constraints.iter().map(|c| c.metric))
            .collect()
    }
    fn bounded_metrics(&self) -> BTreeSet<Metric> {
        self.objectives
            .iter()
            .filter(|_| matches!(self.basis, Basis::ReportedBounds))
            .map(|objective| objective.metric)
            .chain(
                self.constraints
                    .iter()
                    .filter(|constraint| {
                        matches!(
                            constraint.basis.unwrap_or(self.basis),
                            Basis::ReportedBounds
                        )
                    })
                    .map(|constraint| constraint.metric),
            )
            .collect()
    }
    fn consumers(&self) -> BTreeSet<Consumer> {
        self.metrics()
            .iter()
            .map(|m| m.consumer())
            .chain(self.required_consumers.iter().copied())
            .collect()
    }
    fn needs_simulation(&self) -> bool {
        self.consumers().iter().any(|c| {
            matches!(
                c,
                Consumer::Drape
                    | Consumer::MaterialStrain
                    | Consumer::ContactLoads
                    | Consumer::Surface
                    | Consumer::VaporSurface
                    | Consumer::Coverage
                    | Consumer::VaporCoverage
                    | Consumer::Motion
                    | Consumer::MotionMaterialStrain
                    | Consumer::MotionContactLoads
                    | Consumer::MotionVaporCoverage
                    | Consumer::MotionWetFilm
                    | Consumer::TransientSurface
                    | Consumer::TransientCoverage
            )
        })
    }

    fn needs_motion(&self) -> bool {
        self.consumers().iter().any(|c| {
            matches!(
                c,
                Consumer::Motion
                    | Consumer::MotionMaterialStrain
                    | Consumer::MotionContactLoads
                    | Consumer::MotionVaporCoverage
                    | Consumer::MotionWetFilm
                    | Consumer::TransientSurface
                    | Consumer::TransientCoverage
            )
        })
    }

    pub(super) fn evaluate(
        &self,
        rows: &[Alternative],
        source: &str,
        context: &str,
        currency: Option<&str>,
    ) -> Report {
        let candidates: Vec<_> = rows
            .iter()
            .map(|row| self.candidate(row, currency, &Value::Null, &Value::Null))
            .collect();
        self.finish(candidates, source, context)
    }

    pub(super) fn evaluate_scenarios<T: Serialize>(
        &self,
        rows: &[Alternative],
        simulations: &[ScenarioAlternative<T>],
        descriptor: &Value,
        source: &str,
        context: &str,
        currency: Option<&str>,
    ) -> Result<Report, Refusal> {
        let by_key: BTreeMap<_, _> = simulations
            .iter()
            .map(|s| (s.key.as_str(), &s.evaluation))
            .collect();
        let mut candidates = Vec::with_capacity(rows.len());
        for row in rows {
            // At most one transient JSON representation; do not duplicate the entire mesh batch.
            let simulation = if self.needs_simulation() {
                by_key
                    .get(row.key.as_str())
                    .and_then(|outcome| outcome.value())
                    .map(serde_json::to_value)
                    .transpose()
                    .map_err(|e| invalid(e.to_string()))?
                    .unwrap_or(Value::Null)
            } else {
                Value::Null
            };
            candidates.push(self.candidate(row, currency, descriptor, &simulation));
        }
        Ok(self.finish(candidates, source, context))
    }

    fn finish(&self, mut candidates: Vec<Candidate>, source: &str, context: &str) -> Report {
        let mut incomparable = Vec::new();
        for i in 0..candidates.len() {
            for j in i + 1..candidates.len() {
                if !candidates[i].eligible || !candidates[j].eligible {
                    continue;
                }
                if !self.allow_mixed_protocols
                    && candidates[i].input_protocol_sha256 != candidates[j].input_protocol_sha256
                {
                    incomparable.push(Incomparable { alternatives: [candidates[i].key.clone(), candidates[j].key.clone()],
                        reason: "Relevant reported methods, conditions or evidence differ; no dominance inferred." });
                    continue;
                }
                if self.dominates(&candidates[i], &candidates[j]) {
                    let key = candidates[i].key.clone();
                    candidates[j].dominated_by.push(key);
                } else if self.dominates(&candidates[j], &candidates[i]) {
                    let key = candidates[j].key.clone();
                    candidates[i].dominated_by.push(key);
                } else if matches!(self.basis, Basis::ReportedBounds) {
                    incomparable.push(Incomparable {
                        alternatives: [candidates[i].key.clone(), candidates[j].key.clone()],
                        reason: "Reported ranges overlap, touch or trade off; no guaranteed strict dominance inferred.",
                    });
                }
            }
        }
        let frontier = candidates
            .iter()
            .filter(|r| r.eligible && r.dominated_by.is_empty())
            .map(|r| r.key.clone())
            .collect();
        let mut report = Report {
            schema: "ftx-material-comparison/1",
            source_pattern_sha256: source.into(),
            context_sha256: context.into(),
            policy: self.clone(),
            alternatives: candidates,
            frontier,
            incomparable,
            uncertainty_propagated: false,
            physical_validation_claim: false,
            motion_required: self.needs_motion(),
            notes: vec![
                match self.basis {
                    Basis::Nominal => {
                        "Nominal computed outputs only; source bounds are not propagated uncertainty intervals. Estimate permission stays with each source selection."
                    }
                    Basis::ReportedBounds => {
                        "Constraints must contain the entire reported range. Dominance requires no worse endpoints with a strict improvement; overlapping or touching ranges remain unresolved. Mass bounds use fixed authored areas and additive inventory. Vapor bounds use complete positive source Ret endpoints through the canonical network with fixed geometry, films, interfaces, pressures and latent heat; motion additionally fixes the retained path and integration rule. No confidence level, independent errors, geometry uncertainty, unsampled-motion bound or measured garment performance is inferred. Estimate permission stays with each source selection."
                    }
                },
                "The frontier is conditional on the evaluated alternatives, constraints, process policy and reported-protocol admission. Scalar pre-screening may have removed better garment alternatives.",
                "Matching reported protocols is a software comparison rule, not proof that specimens or experiments are comparable. Missing or refused required outputs exclude a row without erasing other consumers.",
                "Stock mass includes waste, costs are dated complete independent order observations, and feed time excludes handling. No empirical fit, warmth, hardware authorization or global optimum is claimed.",
                "Scenario objectives require matching pattern/body/scenario identities, geometric settledness and clear gating. Geometric settledness is not viscoelastic stress equilibrium or empirical fit; simulated cloth mass excludes omitted components.",
                "Heat powers keep their signed directions and distinct sheet/domain bases. Edge-strain metrics use the production report's edge extrema and precision, not an invented fit score. Different nonlinear constitutive records are conservatively protocol-incomparable unless explicitly admitted.",
                "Motion is required only when declared by the comparison policy. Motion objectives use a complete source/path/time-matched trajectory from a settled, gating-clear initial drape; the final moving state need not be at rest. Peak edge strain is over original-panel edges and retained samples, not continuous-time rupture or anatomical fit.",
                "Transient objectives require a complete policy/path/geometry/time-matched history at every accepted mechanical sample. Energy retains its incoming-side sign and model domain; coverage includes bare paths and explicitly exposed-cloth films, not certified whole-wearer loss. Sheet and coverage energies must not be added together.",
                "Sample temperature extrema include the declared initial state and retained mechanical sample endpoints; final temperature extrema use only the final accepted sample. Neither is a continuous-time bound or the extrema of unreported thermal substeps. Net boundary energy excludes remap residual and cancelling bare-body paths. Numerical energy/remap residuals remain in source reports; nominal admission is not empirical accuracy, a comfort threshold, or propagated uncertainty.",
            ],
        };
        if self
            .constraints
            .iter()
            .any(|constraint| constraint.basis.is_some_and(|basis| basis != self.basis))
        {
            report.notes[0] = "Objectives retain the declared comparison basis. Every constraint uses its explicit basis or inherits that objective basis: nominal guards test computed values only, while reported-bounds guards must contain the entire supported source range. A nominal guard never supplies or relaxes the ranges required by another decision on the same metric. Source ranges keep geometry, inventory, boundary conditions and sampled history fixed; they are not confidence intervals, uncertainty in unmodeled inputs or measured garment performance. Estimate permission stays with each source selection.";
        }
        if self.consumers().contains(&Consumer::Vapor) {
            report.notes.push("Vapor objectives use the declared common-area stack, pressure boundaries, films and latent heat. They describe potential transfer, not actual evaporation, sweating, condensation or wearer comfort; source protocols must be admitted independently of other thermal consumers.");
        }
        if self.bounded_metrics().iter().any(|metric| {
            matches!(
                metric,
                Metric::IndependentSurfacePowerW | Metric::BodyCoveragePowerW
            )
        }) {
            report.notes.push("Steady heat ranges require complete positive source-reported constant resistance, or both thickness and conductivity endpoints, on the exact selected materials. Canonical column/region and signed power endpoints are reconciled on the retained geometry with fixed reservoirs, films, interfaces and coverage allocation, including explicit bare-body paths. Nonlinear conductivity, coupled exterior exchange, thermal bridges, missing ranges or unrepresentable arithmetic leave nominal heat visible but cannot support a bounded decision. These conditional source ranges exclude geometric and boundary uncertainty, transient behavior and wearer warmth or comfort claims.");
        }
        if self.consumers().contains(&Consumer::MaterialStrain)
            || self.consumers().contains(&Consumer::MotionMaterialStrain)
        {
            report.notes.push("Material-strain objectives use signed, dimensionless raw material-frame Green strains [E11, E22, 2E12] before numerical gather/compression softening, on the explicitly selected retained instances. Motion extrema include the initial state and every accepted mechanical sample; no continuous-time bound is claimed. Canonical source domains describe constitutive characterization support. Authored comparison constraints are design requirements, never measured failure, rupture, comfort or safe-fit limits. Static and motion strain consumers remain independent of thermal qualification; a refused motion history cannot substitute its static result.");
        }
        if self.metrics().iter().any(|metric| {
            matches!(
                metric,
                Metric::TextileWaterEvaporatedMassKg
                    | Metric::TextileWaterRemainingMassKg
                    | Metric::TextileWaterSuppliedMassKg
                    | Metric::TextileWaterCondensedMassKg
                    | Metric::TextileWaterLatentEnergyJ
                    | Metric::TextileWaterCondensationLatentEnergyJ
            )
        }) {
            report.notes.push("Textile-water objectives require the source-qualified free-film history, canonical Ret networks and the complete liquid/vapor energy ledger on the admitted motion. Reference material area owns liquid; current area conducts vapor. Version 2 qualifies incoming-liquid delivery and sensible enthalpy. Version 3 additionally qualifies reverse transfer, dry/wet nucleation and the declared incoming-vapor enthalpy model. Evaporative latent removal and condensation latent release stay separate, including when the same water later re-evaporates. These are nominal conditional computations, not sorption, inferred wet properties, continuous-time temperature bounds, comfort or measured garment validation.");
        }
        if self.consumers().contains(&Consumer::VaporSurface) {
            report.notes.push("Independent draped-sheet vapor objectives use the admitted initial geometry, retained source Ret and declared pressure/film/latent-heat conditions. Overlapping sheets remain independent; these potentials are not covered-body evaporation, water supply, latent cooling, continuous motion totals or comfort. Consumed column protocols require separate cross-alternative admission.");
        }
        if self.consumers().contains(&Consumer::ContactLoads)
            || self.consumers().contains(&Consumer::MotionContactLoads)
        {
            report.notes.push("Body contact objectives use the negative gradient of production barrier energy on an explicit union of welded vertices. They exclude self-contact and Coulomb tangential forces. Opposing forces can cancel in a resultant; nodal peaks depend on mesh and barrier settings. Material and numerical policies, scope and complete sampled motion are retained. No contact area, pressure, comfort, safe-fit threshold or continuous-time force bound is inferred.");
        }
        if self.consumers().contains(&Consumer::VaporCoverage) {
            report.notes.push("Covered vapor objectives require the whole declared body domain to be covered on the admitted drape. Partial covered subtotals stay visible but cannot be ranked as complete-domain transfer. Actual layer areas, explicit interlayer resistance, pressure boundaries and films define the potential network; no sweat supply, condensation, thermal feedback or comfort is inferred.");
        }
        if self.consumers().contains(&Consumer::MotionVaporCoverage) {
            report.notes.push("Moving covered vapor objectives require complete declared body coverage at the initial state and every accepted mechanical sample, with the current material, path and prescribed conditions. Potential energy and mass integrate sampled power and flow over the actual intervals using the declared right-endpoint or composite-trapezoid rule; they do not establish water availability, actual evaporation, latent cooling, continuous-time accuracy or comfort.");
        }
        if self.consumers().contains(&Consumer::MotionWetFilm) {
            report.notes.push("Finite-water objectives require complete declared body coverage throughout the admitted motion and the exact sourced initial liquid and supply on its stable body triangles. Each retained step and cumulative ledger is checked against canonical regional demand, the declared quadrature and finite liquid. Evaporation is modeled covered transfer; latent energy comes from the prescribed body reservoir and does not cool textile storage. No wearer physiology, physical garment performance, unsampled-motion accuracy or physiological supply model is inferred.");
            if self
                .bounded_metrics()
                .iter()
                .any(|metric| metric.consumer() == Consumer::MotionWetFilm)
            {
                report.notes.push("Final finite-water ranges replay complete canonical source Ret endpoints separately on every body triangle through the same liquid reducer, with fixed initial liquid, constant supply, geometry, boundaries, latent heat and integration rule. Cumulative evaporation, unmet demand and prescribed-reservoir latent energy increase with covered demand; remaining liquid reverses endpoint order. Fixed supplied mass has a singleton range independently of Ret bounds. These are conditional final ranges only: nominal per-interval ledgers remain unchanged, and interval evaporation may reverse after prior depletion. Missing or unrepresentable response endpoints stay unavailable rather than becoming nominal singletons.");
            }
        }
        if self.metrics().iter().any(|metric| {
            matches!(
                metric,
                Metric::SewingSetupSeconds
                    | Metric::SewingFeedAndSetupSeconds
                    | Metric::SewingHandlingSeconds
                    | Metric::SewingModeledProductionSeconds
            )
        }) {
            report.notes.push("Sewing time objectives use the selected complete schedule and independently qualified setup and handling evidence. Feed plus setup retains its original scope. Modeled production time additionally requires explicit serial initial, between-operation and final handling, including same-copy versus different-copy transitions. A joint setup-and-handling optimum does not prove either component minimal. A budget-limited complete incumbent remains comparable without an optimality claim; absent timing stays unavailable, and undeclared work or hardware execution is not inferred.");
        }
        if self
            .metrics()
            .contains(&Metric::SewingParallelMakespanSeconds)
        {
            report.notes.push("Parallel sewing makespan is the completion time of the selected whole-garment allocation or fixed specialist-cell route from the declared common origin. The chosen model retains its qualified cell availability, setup, handling, resource independence and conservative feed rounding. A routed model additionally requires its exact transfer-resource and staging policy. This is distinct from summed machine work. A complete incumbent remains comparable without an optimality proof; a fixed-route dispatch proof is conditional on that search scope. No unqualified resource independence, inter-cell transfer, measured throughput or hardware execution is inferred.");
        }
        if self.consumers().contains(&Consumer::CuttingJob)
            || self.consumers().contains(&Consumer::CuttingJobPurchasing)
            || self.consumers().contains(&Consumer::CuttingJobProgram)
        {
            report.notes.push("Cutting-job objectives cover every routed physical piece exactly once. Gross stock includes roll-marker waste and entire used finite-stock items; complete same-market observed costs do not reserve inventory or include undeclared shipping/handling.");
        }
        if self.consumers().iter().any(|consumer| {
            matches!(
                consumer,
                Consumer::CuttingProgram
                    | Consumer::FiniteStockProgram
                    | Consumer::CuttingJobProgram
            )
        }) {
            report.notes.push("Qualified knife objectives use complete selected paths from the actual source-locked producers. Cut length includes internal cuts; raised travel includes requested returns; modeled time includes only declared constant-rate cutting, raised travel and lift/lower phases. Unused finite stock contributes no work, and complete mixed jobs require qualified programs on every engaged disjoint route. Independent roll and finite-stock plans are never added as if they were one garment job. Source-qualified rates may differ under matching machine and observation protocols. A complete selected traversal remains comparable without a shortest-path proof. These nominal values do not establish propagated uncertainty, parallel makespan, loading/acceleration/turning/inspection time, measured cycle time, physical cut quality or hardware authorization.");
        }
        if self.consumers().contains(&Consumer::Purchasing)
            || self.consumers().contains(&Consumer::CuttingJobPurchasing)
        {
            report.notes.push("Selected order cost objectives admit only complete feasible orders. A bounded split-supplier search may retain such an incumbent without proving it cheapest; the observed_order_cost and cutting_job_observed_order_cost objectives require the corresponding allocation minimum. Source quantity and continuous-cut limits apply to each selected aggregate supplier order, without splitting markers or reserving stock.");
        }
        if self.consumers().contains(&Consumer::SewingConsumables)
            || self.consumers().contains(&Consumer::SewingPurchasing)
            || self.consumers().contains(&Consumer::CutAndSewPurchasing)
        {
            report.notes.push("Thread objectives use the selected production plan's complete, independently qualified channel consumption. Exact yarn demands aggregate before supplier rounding and stock admission. Consumed mass requires each exact yarn's qualified linear density and includes the declared allowances; it is not retained garment mass. Kilogram offers retain their original mass minimums, increments, tiers and stock limits, without inventing an uninterrupted length from mass. Missing density preserves length-only capabilities. Costs retain the thread market's own currency and date; missing consumption or any eligible whole order remains unavailable, and time-based recipe selection does not prove a thread-cost optimum.");
        }
        if self.consumers().contains(&Consumer::CutAndSewPurchasing) {
            report.notes.push("Complete declared cut-and-sew purchasing composes actual fabric and selected-plan thread orders for the same pattern and garment count in one exact comparable market and immutable source history. Current transitive supplier stock aliases cannot be counted on both sides. A complete feasible selected cost may remain available without an allocation minimum proof. Labor, equipment, unrepresented trims and costs outside the declared quote terms are excluded; inventory reservations, orders and hardware actions are not authorized.");
        }
        report
    }

    fn candidate(
        &self,
        row: &Alternative,
        currency: Option<&str>,
        descriptor: &Value,
        simulation: &Value,
    ) -> Candidate {
        let mut result = Candidate {
            key: row.key.clone(),
            readings: BTreeMap::new(),
            exclusions: Vec::new(),
            eligible: false,
            input_protocol_sha256: None,
            dominated_by: Vec::new(),
        };
        if row.binding.value().is_none() {
            result.exclusions.push(invalid(
                "comparison requires successful source-locked binding",
            ));
        }
        let simulation = if self.needs_simulation() {
            match admit_simulation(row, descriptor, simulation) {
                Ok(()) => simulation,
                Err(e) => {
                    result.exclusions.push(e);
                    &Value::Null
                }
            }
        } else {
            &Value::Null
        };
        let mut admitted = Numerical {
            cutting_program: None,
            finite_stock_program: None,
            cutting_job_program: None,
            drape: simulation,
            material_strain: None,
            contact_loads: None,
            vapor_surface: &Value::Null,
            vapor_coverage: None,
            motion: &Value::Null,
            motion_material_strain: None,
            motion_contact_loads: None,
            motion_vapor_coverage: None,
            motion_wet_film: None,
            transient_surface: None,
            transient_coverage: None,
        };
        for (consumer, target) in [
            (Consumer::CuttingProgram, &mut admitted.cutting_program),
            (
                Consumer::FiniteStockProgram,
                &mut admitted.finite_stock_program,
            ),
            (
                Consumer::CuttingJobProgram,
                &mut admitted.cutting_job_program,
            ),
        ] {
            if self.consumers().contains(&consumer) {
                match cutting_effort::admit(row, consumer) {
                    Ok(receipt) => *target = Some(receipt),
                    Err(refusal) => result.exclusions.push(refusal),
                }
            }
        }
        if self.consumers().contains(&Consumer::MaterialStrain) && !simulation.is_null() {
            match material_strain::admit(row, descriptor, simulation, None) {
                Ok(strain) => admitted.material_strain = Some(strain),
                Err(e) => result.exclusions.push(e),
            }
        }
        if self.consumers().contains(&Consumer::ContactLoads) && !simulation.is_null() {
            match contact_loads::admit(row, descriptor, simulation, None) {
                Ok(loads) => admitted.contact_loads = Some(loads),
                Err(e) => result.exclusions.push(e),
            }
        }
        if self.consumers().contains(&Consumer::VaporSurface) && !simulation.is_null() {
            match admit_vapor_surface(row, descriptor, simulation) {
                Ok(vapor) => admitted.vapor_surface = vapor,
                Err(e) => result.exclusions.push(e),
            }
        }
        if self.consumers().contains(&Consumer::VaporCoverage) && !simulation.is_null() {
            match admit_vapor_coverage(row, descriptor, simulation) {
                Ok(vapor) => admitted.vapor_coverage = Some(vapor),
                Err(e) => result.exclusions.push(e),
            }
        }
        if self.needs_motion() && !simulation.is_null() {
            match admit_motion(descriptor, simulation) {
                Ok(motion) => admitted.motion = motion,
                Err(e) => result.exclusions.push(e),
            }
        }
        if !admitted.motion.is_null() {
            if self.consumers().contains(&Consumer::MotionContactLoads) {
                match contact_loads::admit(row, descriptor, simulation, Some(admitted.motion)) {
                    Ok(loads) => admitted.motion_contact_loads = Some(loads),
                    Err(e) => result.exclusions.push(e),
                }
            }
            if self.consumers().contains(&Consumer::MotionMaterialStrain) {
                match material_strain::admit(row, descriptor, simulation, Some(admitted.motion)) {
                    Ok(strain) => admitted.motion_material_strain = Some(strain),
                    Err(e) => result.exclusions.push(e),
                }
            }
            if self.consumers().contains(&Consumer::MotionVaporCoverage)
                || self.consumers().contains(&Consumer::MotionWetFilm)
            {
                match admit_motion_vapor_coverage(row, descriptor, simulation, admitted.motion) {
                    Ok(history) => admitted.motion_vapor_coverage = Some(history),
                    Err(e) => result.exclusions.push(e),
                }
            }
            if self.consumers().contains(&Consumer::MotionWetFilm)
                && let Some(history) = admitted.motion_vapor_coverage
            {
                match wet_film::admit(
                    history,
                    &descriptor["scenario"]["motion"]["vapor_coverage_history"]["wet_film"],
                ) {
                    Ok(water) => admitted.motion_wet_film = Some(water),
                    Err(e) => result.exclusions.push(e),
                }
            }
            for (consumer, target) in [
                (Consumer::TransientSurface, &mut admitted.transient_surface),
                (
                    Consumer::TransientCoverage,
                    &mut admitted.transient_coverage,
                ),
            ] {
                if self.consumers().contains(&consumer) {
                    match admit_heat(row, descriptor, simulation, admitted.motion, consumer) {
                        Ok(heat) => *target = Some(heat),
                        Err(e) => result.exclusions.push(e),
                    }
                }
            }
        }
        for consumer in self.consumers() {
            if !consumer_available(row, consumer, &admitted) {
                result.exclusions.push(invalid(format!(
                    "comparison requires available {consumer:?} with complete required coverage"
                )));
            }
        }
        let mut derived_bounds = BTreeMap::new();
        let mut water_bounds = BTreeMap::new();
        let mut heat_bounds = BTreeMap::new();
        let bounded_metrics = self.bounded_metrics();
        if !bounded_metrics.is_empty() {
            for consumer in [Consumer::Surface, Consumer::Coverage] {
                if bounded_metrics
                    .iter()
                    .any(|metric| metric.consumer() == consumer)
                    && consumer_available(row, consumer, &admitted)
                {
                    match steady_heat::bounds(row, descriptor, simulation, consumer) {
                        Ok(range) => {
                            heat_bounds.insert(consumer, range);
                        }
                        Err(refusal) => result.exclusions.push(refusal),
                    }
                }
            }
            for consumer in [
                Consumer::VaporSurface,
                Consumer::VaporCoverage,
                Consumer::MotionVaporCoverage,
            ] {
                if !bounded_metrics.iter().any(|m| m.consumer() == consumer)
                    || !consumer_available(row, consumer, &admitted)
                {
                    continue;
                }
                let bounds = match consumer {
                    Consumer::VaporSurface => sheet_vapor_bounds(admitted.vapor_surface),
                    Consumer::VaporCoverage => {
                        let result = admitted.vapor_coverage.expect("admitted covered vapor");
                        covered_vapor_bounds(result)
                    }
                    Consumer::MotionVaporCoverage => motion_vapor_bounds(
                        admitted
                            .motion_vapor_coverage
                            .expect("admitted moving vapor"),
                    ),
                    _ => unreachable!(),
                };
                match bounds {
                    Ok(Some(range)) => {
                        derived_bounds.insert(consumer, range);
                    }
                    Ok(None) => {}
                    Err(e) => result.exclusions.push(e),
                }
            }
            if bounded_metrics
                .iter()
                .any(|metric| metric.consumer() == Consumer::MotionWetFilm)
                && let Some(water) = admitted.motion_wet_film
            {
                if let Some(supplied) =
                    finite(&water["totals"]["supplied_mass_kg"]).filter(|value| *value >= 0.)
                {
                    water_bounds.insert(Metric::MotionWetFilmSuppliedMassKg, [supplied, supplied]);
                }
                if bounded_metrics.iter().any(|metric| {
                    metric.consumer() == Consumer::MotionWetFilm
                        && *metric != Metric::MotionWetFilmSuppliedMassKg
                }) {
                    match wet_film::reported_bounds(
                        admitted
                            .motion_vapor_coverage
                            .expect("admitted moving water"),
                    ) {
                        Ok(Some(ranges)) => water_bounds.extend(ranges),
                        Ok(None) => {}
                        Err(e) => result.exclusions.push(e),
                    }
                }
            }
        }
        for metric in self.metrics() {
            let currency = metric.currency(row, currency);
            match metric
                .value(row, &admitted)
                .filter(|v| v.is_finite() && (metric.signed() || *v >= 0.))
            {
                Some(value) if consumer_available(row, metric.consumer(), &admitted) => {
                    let needs_bounds = bounded_metrics.contains(&metric);
                    let reported_bounds = if needs_bounds {
                        metric
                            .reported_bounds(row, &derived_bounds, &water_bounds, &heat_bounds)
                            .filter(|[lo, hi]| {
                                lo.is_finite()
                                    && hi.is_finite()
                                    && (metric.signed() || *lo >= 0.)
                                    && *lo <= value
                                    && value <= *hi
                            })
                    } else {
                        None
                    };
                    result.readings.insert(
                        metric,
                        Reading {
                            value,
                            reported_bounds,
                            unit: metric.unit(currency).into(),
                            consumer: metric.consumer(),
                        },
                    );
                    if needs_bounds && reported_bounds.is_none() {
                        result.exclusions.push(invalid(format!(
                            "{metric:?} has no complete valid reported bounds; its nominal value remains visible but no exact range is substituted"
                        )));
                    }
                    for c in self.constraints.iter().filter(|c| c.metric == metric) {
                        let basis = c.basis.unwrap_or(self.basis);
                        let interval = match basis {
                            Basis::Nominal => Some([value, value]),
                            Basis::ReportedBounds => reported_bounds,
                        };
                        let Some([lo, hi]) = interval else {
                            continue;
                        };
                        if c.minimum.is_some_and(|min| lo < min)
                            || c.maximum.is_some_and(|max| hi > max)
                        {
                            let observed = match basis {
                                Basis::Nominal => value.to_string(),
                                Basis::ReportedBounds => format!("reported range [{lo}, {hi}]"),
                            };
                            result.exclusions.push(invalid(format!("{metric:?} = {observed} {} violates the inclusive comparison constraint", metric.unit(currency))));
                        }
                    }
                }
                _ => result.exclusions.push(invalid(format!(
                    "{metric:?} is unavailable; no zero or partial value substituted"
                ))),
            }
        }
        match self.protocol(row, descriptor, &admitted) {
            Ok(protocol) => result.input_protocol_sha256 = Some(protocol),
            Err(e) => result.exclusions.push(e),
        }
        result.eligible = result.exclusions.is_empty();
        result
    }

    fn dominates(&self, a: &Candidate, b: &Candidate) -> bool {
        self.relation(a, b) == Some(true)
    }

    /// Reuse the same admitted, protocol-qualified dominance rule for adaptive consumer polling.
    pub(super) fn prefers(&self, a: &Candidate, b: &Candidate) -> bool {
        a.eligible
            && b.eligible
            && (self.allow_mixed_protocols || a.input_protocol_sha256 == b.input_protocol_sha256)
            && self.dominates(a, b)
    }

    // Some(false) is no worse with no strict improvement; None is not no-worse. Keeping that
    // distinction lets named-case matrices combine ties and improvements using this same rule.
    fn relation(&self, a: &Candidate, b: &Candidate) -> Option<bool> {
        let mut strictly = false;
        for objective in &self.objectives {
            let interval = |candidate: &Candidate| {
                let reading = &candidate.readings[&objective.metric];
                match self.basis {
                    Basis::Nominal => [reading.value, reading.value],
                    Basis::ReportedBounds => {
                        reading.reported_bounds.expect("admitted source range")
                    }
                }
            };
            let [al, ah] = interval(a);
            let [bl, bh] = interval(b);
            match objective.direction {
                Direction::Minimize => {
                    if ah > bl {
                        return None;
                    }
                    strictly |= ah < bl;
                }
                Direction::Maximize => {
                    if al < bh {
                        return None;
                    }
                    strictly |= al > bh;
                }
            }
        }
        Some(strictly)
    }

    fn protocol(
        &self,
        row: &Alternative,
        descriptor: &Value,
        admitted: &Numerical<'_>,
    ) -> Result<String, Refusal> {
        let bound = row
            .binding
            .value()
            .ok_or_else(|| invalid("binding protocol unavailable"))?;
        let frozen = row
            .frozen_changes
            .as_ref()
            .ok_or_else(|| invalid("frozen source selections unavailable"))?;
        let consumers = self.consumers();
        let combined = consumers.contains(&Consumer::CutAndSewPurchasing);
        let mut keys = BTreeSet::new();
        if consumers.contains(&Consumer::Cutting)
            || consumers.contains(&Consumer::CuttingProgram)
            || consumers.contains(&Consumer::Purchasing)
            || (combined && row.cutting.value().is_some())
        {
            keys.insert("width");
        }
        let thread = consumers.contains(&Consumer::SewingConsumables)
            || consumers.contains(&Consumer::SewingPurchasing)
            || combined;
        if consumers.contains(&Consumer::Sewing) || thread {
            keys.insert("thickness");
        }
        if self.metrics().iter().any(|m| {
            matches!(
                m,
                Metric::StockMassKg | Metric::UsedStockMassKg | Metric::CuttingJobStockMassKg
            )
        }) || consumers.contains(&Consumer::AssemblyMass)
        {
            keys.insert("areal_density");
        }
        let mechanics = self.needs_simulation();
        let transient = consumers.contains(&Consumer::TransientSurface)
            || consumers.contains(&Consumer::TransientCoverage);
        let thermal = transient
            || consumers.contains(&Consumer::Surface)
            || consumers.contains(&Consumer::Coverage);
        if mechanics {
            keys.extend(
                ftx_pattern::materials::MECHANICAL_PROPERTIES
                    .iter()
                    .map(|(key, _)| *key),
            );
        }
        if thermal {
            keys.extend(["thermal_conductivity", "thermal_resistance", "emissivity"]);
        }
        if transient {
            keys.extend(["areal_density", "specific_heat", "thickness"]);
        }
        let mass = keys.contains("areal_density");
        let textile_water = consumers.contains(&Consumer::TransientSurface)
            && !descriptor["scenario"]["motion"]["transient_surface"]["surface_water"].is_null();
        if textile_water {
            keys.insert("vapor_resistance");
        }
        if [
            (Consumer::TransientSurface, "transient_surface"),
            (Consumer::TransientCoverage, "transient_coverage"),
        ]
        .into_iter()
        .any(|(consumer, name)| {
            consumers.contains(&consumer)
                && descriptor["scenario"]["motion"][name]["plastic_dissipation_heat"]
                    == "uniform_reference_thickness"
        }) {
            keys.insert("membrane_plastic_heat_fraction");
        }
        if [
            (Consumer::TransientSurface, "transient_surface"),
            (Consumer::TransientCoverage, "transient_coverage"),
        ]
        .into_iter()
        .any(|(consumer, name)| {
            consumers.contains(&consumer)
                && descriptor["scenario"]["motion"][name]["relaxation_dissipation_heat"]
                    == "uniform_reference_thickness"
        }) {
            keys.insert("membrane_relaxation_heat_fraction");
        }
        if [
            (Consumer::TransientSurface, "transient_surface"),
            (Consumer::TransientCoverage, "transient_coverage"),
        ]
        .into_iter()
        .any(|(consumer, name)| {
            consumers.contains(&consumer)
                && descriptor["scenario"]["motion"][name]["bending_plastic_dissipation_heat"]
                    == "uniform_reference_thickness"
        }) {
            keys.insert("bending_plastic_heat_fraction");
        }
        let pieces = bound["pattern"]["pieces"]
            .as_array()
            .ok_or_else(|| invalid("bound source pieces unavailable"))?;
        let mut protocols = BTreeMap::new();
        for piece in pieces {
            let id = piece["id"]
                .as_str()
                .ok_or_else(|| invalid("bound source piece identity unavailable"))?;
            let selection = frozen
                .pieces
                .get(id)
                .or(frozen.default.as_ref())
                .ok_or_else(|| invalid("frozen piece selection unavailable"))?;
            let material = selection
                .record
                .as_ref()
                .ok_or_else(|| invalid("frozen source record unavailable"))?;
            let mut relevant = keys.clone();
            if mass && material.structure.contains_key("mechanical_mass_model") {
                relevant.insert("retained_liquid_areal_density");
            }
            if (consumers.contains(&Consumer::CuttingJob)
                || consumers.contains(&Consumer::CuttingJobPurchasing)
                || consumers.contains(&Consumer::CuttingJobProgram)
                || combined)
                && row.cutting_job.value().is_some_and(|job| {
                    job.inventory.iter().any(|entry| {
                        entry.piece == id && entry.stock == crate::cutting::job::StockKind::Roll
                    })
                })
            {
                relevant.insert("width");
            }
            if mechanics {
                relevant.extend(
                    material
                        .properties
                        .keys()
                        .filter(|k| {
                            k.starts_with("bending_")
                                && k.as_str() != "bending_plastic_heat_fraction"
                        })
                        .map(String::as_str),
                );
            }
            let profile: BTreeMap<_, _> = relevant
                .iter()
                .map(|&key| {
                    let p = material.properties.get(key).map_or(Value::Null, |q| {
                        json!({
                    "unit":property_unit(key).unwrap_or(&q.unit), "method":q.method,
                    "conditions":q.conditions, "evidence":q.evidence })
                    });
                    (key, p)
                })
                .collect();
            // Scalar protocols compare qualifiers, not their values. Complex response records use
            // conservative whole-record equality rather than guessing which curve fields are metadata.
            let models: BTreeMap<_, _> = material
                .structure
                .iter()
                .filter(|(key, _)| {
                    (mechanics && (key.starts_with("bending_") || key.starts_with("membrane_")))
                        || (mass && key.as_str() == "mechanical_mass_model")
                })
                .collect();
            protocols.insert(id, json!({"properties":profile, "constitutive_models":models,
                "membrane_response":if mechanics { serde_json::to_value(&material.membrane_response).map_err(|e| invalid(e.to_string()))? } else { Value::Null },
                "membrane_relaxation":if mechanics { serde_json::to_value(&material.membrane_relaxation).map_err(|e| invalid(e.to_string()))? } else { Value::Null },
                "thermal_response":if thermal { serde_json::to_value(&material.thermal_response).map_err(|e| invalid(e.to_string()))? } else { Value::Null }}));
        }
        let mut base = hash(&(
            protocols,
            if mechanics {
                bound["pattern"]["fabric"]["interfaces"].clone()
            } else {
                Value::Null
            },
        ))?;
        for (consumer, receipt) in [
            (Consumer::CuttingProgram, admitted.cutting_program),
            (Consumer::FiniteStockProgram, admitted.finite_stock_program),
            (Consumer::CuttingJobProgram, admitted.cutting_job_program),
        ] {
            if consumers.contains(&consumer) {
                let receipt =
                    receipt.ok_or_else(|| invalid("qualified cutting protocol unavailable"))?;
                base = hash(&(base, consumer, &receipt.protocol))?;
            }
        }
        if combined {
            let receipt = row
                .cut_and_sew_purchasing
                .value()
                .ok_or_else(|| invalid("complete declared purchasing protocol unavailable"))?;
            base = hash(&(
                base,
                (
                    receipt.schema,
                    &receipt.market,
                    &receipt.quote_book_sha256,
                    receipt.production_copies,
                    receipt.fabric_consumer,
                ),
            ))?;
        }
        for (consumer, strain) in [
            (Consumer::MaterialStrain, admitted.material_strain),
            (
                Consumer::MotionMaterialStrain,
                admitted.motion_material_strain,
            ),
        ] {
            if consumers.contains(&consumer) {
                let strain =
                    strain.ok_or_else(|| invalid("admitted material-strain scope unavailable"))?;
                // Compare the same physical instance scope and raw convention. Geometry and
                // source-triangle count may legitimately change across declared design alternatives.
                base = hash(&(
                    base,
                    json!({"consumer":consumer,
                    "instances":strain["source"]["instances"],"scope":strain["scope"],
                    "strain_convention":strain["strain_convention"],"unit":strain["unit"]}),
                ))?;
            }
        }
        for (consumer, loads) in [
            (Consumer::ContactLoads, admitted.contact_loads),
            (Consumer::MotionContactLoads, admitted.motion_contact_loads),
        ] {
            if consumers.contains(&consumer) {
                let loads =
                    loads.ok_or_else(|| invalid("admitted contact-load scope unavailable"))?;
                base = hash(&(
                    base,
                    json!({"consumer":consumer,"scope":loads["scope"],
                    "instances":loads["source"]["instances"].as_object().map(|ids| ids.keys().collect::<Vec<_>>()),
                    "selected_vertices":loads["source"]["selected_vertices"],
                    "triangles":admitted.drape["triangles"],"unit":"N"}),
                ))?;
            }
        }
        if textile_water {
            base = hash(&(
                &base,
                &descriptor["scenario"]["motion"]["transient_surface"]["surface_water"],
            ))?;
        }
        if self.needs_motion() {
            let coupling =
                &descriptor["scenario"]["motion"]["transient_surface"]["mechanical_coupling"];
            if !coupling.is_null() {
                // Source velocity changes the actual trajectory even when water inventory matches.
                // Static and manufacturing-only consumers retain their independent protocols.
                base = hash(&(&base, coupling))?;
            }
        }
        if consumers.contains(&Consumer::Coverage) {
            let expected = &descriptor["scenario"]["coverage"]["thermal_bridges"];
            let result = &admitted.drape["coverage"]["result"];
            if !expected.is_null()
                || !result["thermal_bridges"].is_null()
                || !result["request"]["thermal_bridges"].is_null()
            {
                if expected.is_null()
                    || result["thermal_bridges"].is_null()
                    || result["request"]["thermal_bridges"] != *expected
                {
                    return Err(invalid(
                        "coverage comparison requires the requested thermal bridges and their exact frozen source policy; missing, injected or changed bridge outcomes refuse",
                    ));
                }
                base = hash(&(base, thermal_bridge_protocol(result)?))?;
            }
        }
        if consumers.contains(&Consumer::TransientCoverage)
            && let Some(history) = &admitted.transient_coverage
            && !history.result["thermal_bridges"].is_null()
        {
            base = hash(&(base, motion_bridges::protocol(history.result)?))?;
        }
        if thread {
            let plan = row
                .sewing
                .value()
                .ok_or_else(|| invalid("selected sewing plan for thread protocols unavailable"))?;
            let mut consumption_protocols = BTreeMap::new();
            let mass_protocol = self.metrics().contains(&Metric::ThreadRequiredMassKg)
                || ((consumers.contains(&Consumer::SewingPurchasing) || combined)
                    && plan
                        .consumables
                        .as_ref()
                        .and_then(|c| c.purchasing.as_ref())
                        .is_some_and(|p| {
                            p.selected_offers().any(|offer| {
                                offer.quote.unit == ftx_pattern::material_cost::SaleUnit::Kilogram
                            })
                        }));
            let mut mass_protocols = BTreeMap::new();
            let mut sources = Vec::new();
            let mut routed_operations = None;
            if let Some(routing) = plan
                .production
                .as_ref()
                .and_then(|production| production.routing.as_ref())
            {
                let used: BTreeSet<_> = routing
                    .selected
                    .operations
                    .iter()
                    .map(|operation| operation.cell.as_str())
                    .collect();
                routed_operations = Some(
                    routing
                        .selected
                        .operations
                        .iter()
                        .map(|operation| (operation.cell.as_str(), operation.operation_id.as_str()))
                        .collect::<BTreeSet<_>>(),
                );
                for cell in &routing.cell_plans {
                    if used.contains(cell.cell.as_str()) {
                        sources.push((Some(cell.cell.as_str()), cell.plan.as_ref()));
                    }
                }
            } else if let Some(parallel) = plan
                .production
                .as_ref()
                .and_then(|production| production.parallel.as_ref())
                .filter(|parallel| !parallel.cell_plans.is_empty())
            {
                for cell in &parallel.selected.cells {
                    if !cell.copies.is_empty() {
                        sources.push((
                            Some(cell.cell.as_str()),
                            plan.parallel_cell_plan(&cell.cell).ok_or_else(|| {
                                invalid("selected physical cell lost its qualified sewing plan")
                            })?,
                        ));
                    }
                }
            } else {
                sources.push((None, plan));
            }
            for (cell, source_plan) in sources {
                for seam in &source_plan.seams {
                    if routed_operations.as_ref().is_some_and(|used| {
                        !used.contains(&(
                            cell.expect("routed source cell"),
                            seam.pass.operation_id(),
                        ))
                    }) {
                        continue;
                    }
                    let q = source_plan
                        .options
                        .qualifications
                        .iter()
                        .find(|q| Some(q.id.as_str()) == seam.qualification_id.as_deref())
                        .ok_or_else(|| {
                            invalid(
                                "admitted sewing qualification for thread protocols unavailable",
                            )
                        })?;
                    let model = q.thread_consumption.as_ref().ok_or_else(|| {
                        invalid("independent thread consumption protocol unavailable")
                    })?;
                    let protocol_key = cell.map_or_else(
                        || Ok(seam.pass.operation_id().to_owned()),
                        |cell| {
                            serde_json::to_string(&(cell, seam.pass.operation_id()))
                                .map_err(|error| invalid(error.to_string()))
                        },
                    )?;
                    let channels: BTreeSet<_> =
                        model.channels.iter().map(|c| c.id.as_str()).collect();
                    consumption_protocols.insert(protocol_key.clone(), json!({
                    "process":{"method":q.method,"conditions":q.conditions,"evidence":q.evidence},
                    "consumption":{"method":model.method,"conditions":model.conditions,"evidence":model.evidence},
                    "channels":channels,
                }));
                    if mass_protocol {
                        let materials = &plan
                            .consumables
                            .as_ref()
                            .ok_or_else(|| invalid("resolved thread materials unavailable"))?
                            .materials;
                        let channels: BTreeMap<_, _> = model
                            .channels
                            .iter()
                            .map(|channel| {
                                // Selection overrides participate in the consumed snapshot. Inspect its
                                // resolved frozen record, never the original pre-override embedded record.
                                let material = channel
                                    .material
                                    .expected_sha256
                                    .as_deref()
                                    .and_then(|sha| {
                                        materials
                                            .iter()
                                            .find(|(hash, _)| hash.eq_ignore_ascii_case(sha))
                                            .and_then(|(_, selection)| selection.record.as_ref())
                                    })
                                    .ok_or_else(|| {
                                        invalid("exact resolved thread material unavailable")
                                    })?;
                                let quantity = material.properties.get("yarn_linear_density");
                                Ok((channel.id.as_str(), quantity.map_or(Value::Null, |q| json!({
                            "unit":"kg/m", "method":q.method, "conditions":q.conditions,
                            "evidence":q.evidence
                        }))))
                            })
                            .collect::<Result<_, Refusal>>()?;
                        mass_protocols.insert(protocol_key, channels);
                    }
                }
            }
            base = hash(&(base, consumption_protocols))?;
            if mass_protocol {
                base = hash(&(base, mass_protocols))?;
            }
        }
        if self.metrics().iter().any(|metric| {
            matches!(
                metric,
                Metric::SewingSetupSeconds
                    | Metric::SewingFeedAndSetupSeconds
                    | Metric::SewingHandlingSeconds
                    | Metric::SewingModeledProductionSeconds
            )
        }) {
            let times = row
                .sewing
                .value()
                .and_then(|plan| plan.options.ordering_policy())
                .and_then(|ordering| ordering.setup_times.as_ref())
                .ok_or_else(|| invalid("source-qualified sewing setup times unavailable"))?;
            base = hash(&(
                base,
                json!({"machine":times.machine,"method":times.method,
                "conditions":times.conditions,"evidence":times.evidence}),
            ))?;
        }
        if self.metrics().iter().any(|metric| {
            matches!(
                metric,
                Metric::SewingHandlingSeconds | Metric::SewingModeledProductionSeconds
            )
        }) {
            let handling = row
                .sewing
                .value()
                .and_then(|plan| plan.options.production.as_ref())
                .and_then(|production| production.handling.as_ref())
                .ok_or_else(|| invalid("source-qualified sewing handling unavailable"))?;
            // The declared operation/copy timing domain is part of this model's protocol;
            // do not silently compare different transition tables or source qualifications.
            base = hash(&(base, handling))?;
        }
        if (consumers.contains(&Consumer::Sewing) || thread)
            && let Some(selection) = row.sewing_selection.value()
            && selection.request.completion_deadline_milliseconds.is_some()
        {
            // The deadline and complete candidate domain determine which otherwise usable
            // recipe can supply these readings. A changed search must not inherit its protocol.
            base = hash(&(
                base,
                ("deadline_constrained_sewing_selection", &selection.request),
            ))?;
        }
        if (consumers.contains(&Consumer::Sewing) || thread)
            && let Some(selection) = row.sewing_selection.value()
            && selection.request.schema == "ftx-sewing-selection/6"
            && selection.request.objective
                == crate::sewing_selection::Objective::MinimizeThreadObservedOrderCost
        {
            // Fixed routes have order-independent quantities, but the complete candidate domain
            // and work cap still determine which qualified plan supplies these cost readings.
            base = hash(&(base, ("fixed_route_cost_selection", &selection.request)))?;
        }
        if (consumers.contains(&Consumer::Sewing) || thread)
            && let Some(selection) = row.sewing_selection.value()
            && selection.request.schema == "ftx-sewing-selection/6"
            && (selection
                .request
                .options
                .production
                .as_ref()
                .and_then(|production| production.routing.as_ref())
                .is_some_and(crate::sewing::production::routing::Policy::eligible)
                || selection
                    .request
                    .route_candidates
                    .iter()
                    .any(|candidate| candidate.routing.eligible()))
        {
            // The winner may be fixed even though an eligible route participated in its search.
            // Preserve every outer candidate, recipe and work cap that determined that decision.
            base = hash(&(base, ("eligible_route_selection", &selection.request)))?;
        }
        if self
            .metrics()
            .contains(&Metric::SewingParallelMakespanSeconds)
            || ((consumers.contains(&Consumer::Sewing) || thread)
                && row
                    .sewing
                    .value()
                    .and_then(|plan| plan.options.production.as_ref())
                    .and_then(|production| production.routing.as_ref())
                    .is_some_and(crate::sewing::production::routing::Policy::eligible))
        {
            let production = row
                .sewing
                .value()
                .and_then(|plan| plan.options.production.as_ref())
                .ok_or_else(|| {
                    invalid("source-qualified multi-cell sewing production unavailable")
                })?;
            // Resource independence, availability, time resolution and every cell's source-bound
            // timing/fixture policy define this experiment, independent of textile protocol consent.
            // Eligible routes optimize makespan before reporting feed/yarn. Their complete search
            // domain therefore remains relevant even when no makespan metric was requested.
            if let Some(routing) = &production.routing {
                // The complete routing domain, including unused eligible cells, qualifies unloaded
                // movement, staging and the assignment search. Never hash only selected resources.
                base = hash(&(base, ("routed", routing)))?;
            } else {
                let parallel = production
                    .parallel
                    .as_ref()
                    .ok_or_else(|| invalid("source-qualified parallel sewing cells unavailable"))?;
                base = hash(&(base, parallel))?;
            }
        }
        if consumers.contains(&Consumer::VaporCoverage) {
            let vapor = admitted
                .vapor_coverage
                .ok_or_else(|| invalid("covered vapor observation protocols unavailable"))?;
            base = hash(&(base, covered_vapor_protocols(vapor)?))?;
        }
        if consumers.contains(&Consumer::MotionVaporCoverage)
            || consumers.contains(&Consumer::MotionWetFilm)
        {
            let history = admitted
                .motion_vapor_coverage
                .ok_or_else(|| invalid("moving covered vapor observation protocols unavailable"))?;
            let mut protocols = covered_vapor_protocols(&history["initial"]["result"])?;
            for sample in vapor_array(history, "samples")? {
                protocols.extend(covered_vapor_protocols(&sample["result"])?);
            }
            base = hash(&(base, protocols))?;
        }
        if consumers.contains(&Consumer::MotionWetFilm) {
            // Explicit liquid/supply/source identity is part of this experiment, never inferred
            // from the material or admitted by the mixed-textile-protocol option.
            admitted
                .motion_wet_film
                .ok_or_else(|| invalid("conserved body wet-film history unavailable"))?;
            base = hash(&(
                base,
                &descriptor["scenario"]["motion"]["vapor_coverage_history"]["wet_film"],
            ))?;
        }
        // Only the explicitly consumed layer occurrences participate. A cuff or trim with
        // unrelated vapor observations must not change a shell/lining comparison's protocol.
        if consumers.contains(&Consumer::Vapor) {
            let vapor = row
                .vapor
                .value()
                .ok_or_else(|| invalid("vapor observation protocols unavailable"))?;
            let layers: Vec<_> = vapor
                .layers
                .iter()
                .map(|layer| {
                    let q = &layer.observation;
                    json!({"key":layer.key,"unit":q.unit,"method":q.method,
                        "conditions":q.conditions,"evidence":q.evidence})
                })
                .collect();
            base = hash(&(base, layers))?;
        }
        if consumers.contains(&Consumer::VaporSurface) {
            let vapor = admitted.vapor_surface;
            let patches = vapor["patch_topology"]
                .as_array()
                .ok_or_else(|| invalid("admitted draped vapor protocols unavailable"))?;
            let mut owners: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
            for (i, patch) in patches.iter().enumerate() {
                // Column order/deduplication can change with substitution. Protocols belong to
                // stable physical instances; unconsumed records and triangle counts do not rank.
                let column = patch["column"].as_u64().unwrap_or(u64::MAX) as usize;
                let owner = vapor["triangle_instance"][i].as_u64().unwrap_or(u64::MAX) as usize;
                let instance = vapor["instances"][owner]
                    .as_str()
                    .ok_or_else(|| invalid("admitted draped vapor owner unavailable"))?;
                let q = &vapor["columns"][column]["layers"][0]["observation"];
                owners.entry(instance).or_default().insert(hash(&json!({
                    "unit":q["unit"],"method":q["method"],
                    "conditions":q["conditions"],"evidence":q["evidence"]}))?);
            }
            base = hash(&(base, owners))?;
        }
        Ok(base)
    }
}

// Each reference is either independently admitted or null. A failed motion must not make a valid
// static reading vanish, and an available status alone must not admit stale numerical results.
struct Numerical<'a> {
    cutting_program: Option<&'a cutting_effort::Receipt>,
    finite_stock_program: Option<&'a cutting_effort::Receipt>,
    cutting_job_program: Option<&'a cutting_effort::Receipt>,
    drape: &'a Value,
    material_strain: Option<&'a Value>,
    contact_loads: Option<&'a Value>,
    vapor_surface: &'a Value,
    vapor_coverage: Option<&'a Value>,
    motion: &'a Value,
    motion_material_strain: Option<&'a Value>,
    motion_contact_loads: Option<&'a Value>,
    motion_vapor_coverage: Option<&'a Value>,
    motion_wet_film: Option<&'a Value>,
    transient_surface: Option<Heat<'a>>,
    transient_coverage: Option<Heat<'a>>,
}

struct Heat<'a> {
    result: &'a Value,
    range_c: [f64; 2],
    final_range_c: [f64; 2],
}

fn consumer_available(row: &Alternative, consumer: Consumer, admitted: &Numerical<'_>) -> bool {
    match consumer {
        Consumer::Cutting => row.cutting.value().is_some(),
        Consumer::CuttingProgram => admitted.cutting_program.is_some(),
        Consumer::FiniteStock => row.finite_stock.value().is_some(),
        Consumer::FiniteStockProgram => admitted.finite_stock_program.is_some(),
        Consumer::CuttingJobProgram => admitted.cutting_job_program.is_some(),
        Consumer::CuttingJob => row
            .cutting_job
            .value()
            .is_some_and(|r| r.complete_inventory),
        Consumer::CuttingJobPurchasing => row.cutting_job.value().is_some_and(|r| {
            r.complete_inventory && row.metrics.cutting_job_selected_order_cost.is_some()
        }),
        Consumer::Sewing => row.sewing.value().is_some(),
        Consumer::SewingConsumables => row
            .sewing
            .value()
            .and_then(|p| p.consumables.as_ref())
            .is_some_and(|c| {
                c.complete_consumption_coverage && c.total_required_length_m.is_some()
            }),
        Consumer::SewingPurchasing => row
            .sewing
            .value()
            .and_then(|p| p.consumables.as_ref())
            .filter(|c| c.complete_consumption_coverage)
            .and_then(|c| c.purchasing.as_ref())
            .is_some_and(|p| p.complete_offer_coverage && p.selected_total().is_some()),
        Consumer::CutAndSewPurchasing => row.cut_and_sew_purchasing.value().is_some_and(|r| {
            row.metrics.cut_and_sew_selected_order_cost == Some(r.selected_order_cost)
                && row.binding.value().is_some_and(|b| {
                    b["pattern_sha256"].as_str() == Some(r.pattern_sha256.as_str())
                })
        }),
        Consumer::Purchasing => row
            .purchasing
            .value()
            .is_some_and(|p| p["complete_offer_coverage"] == true),
        Consumer::AssemblyMass => row.assembly_mass.value().is_some_and(|r| {
            r.assembly_mass.kg.is_some()
                && row.binding.value().is_some_and(|b| {
                    b["pattern_sha256"].as_str() == Some(r.pattern_sha256.as_str())
                })
        }),
        Consumer::Vapor => row.vapor.value().is_some(),
        Consumer::Drape => !admitted.drape.is_null(),
        Consumer::MaterialStrain => admitted.material_strain.is_some(),
        Consumer::ContactLoads => admitted.contact_loads.is_some(),
        Consumer::Surface => admitted.drape["surface"]["status"] == "available",
        Consumer::VaporSurface => !admitted.vapor_surface.is_null(),
        Consumer::Coverage => admitted.drape["coverage"]["status"] == "available",
        Consumer::VaporCoverage => admitted.vapor_coverage.is_some(),
        Consumer::Motion => !admitted.motion.is_null(),
        Consumer::MotionMaterialStrain => admitted.motion_material_strain.is_some(),
        Consumer::MotionContactLoads => admitted.motion_contact_loads.is_some(),
        Consumer::MotionVaporCoverage => admitted.motion_vapor_coverage.is_some(),
        Consumer::MotionWetFilm => admitted.motion_wet_film.is_some(),
        Consumer::TransientSurface => admitted.transient_surface.is_some(),
        Consumer::TransientCoverage => admitted.transient_coverage.is_some(),
    }
}

// Consume only the observations actually selected by the production bridge solver. Unused source
// alternatives do not change a comparison protocol. Whole-observation equality is conservative:
// different source/material/domain qualifications need the existing explicit mixed-protocol consent.
fn thermal_bridge_protocol(result: &Value) -> Result<Value, Refusal> {
    let bridges = &result["thermal_bridges"];
    let input = &result["request"]["thermal_bridges"];
    let bad = || {
        invalid(
            "thermal bridge comparison needs the complete frozen request and exact consumed source qualifications",
        )
    };
    if bridges["schema"] != "ftx-steady-thermal-bridge-result/1"
        || input["schema"] != "ftx-steady-thermal-bridges/1"
        || bridges["model"] != input["model"]
        || bridges["model"] != "qualified_excess_centerline_conductance/1"
    {
        return Err(bad());
    }
    let paths = input["paths"].as_array().ok_or_else(bad)?;
    let reported = bridges["paths"].as_array().ok_or_else(bad)?;
    if paths.len() != reported.len() || reported.is_empty() {
        return Err(bad());
    }
    let mut protocols = BTreeMap::new();
    for path in reported {
        let id = path["id"].as_str().ok_or_else(bad)?;
        let sources: Vec<_> = paths.iter().filter(|p| p["id"] == id).collect();
        if sources.len() != 1 || sources[0]["instance"] != path["instance"] {
            return Err(bad());
        }
        let qualified = sources[0]["qualifications"].as_array().ok_or_else(bad)?;
        let pieces = path["contributions"].as_array().ok_or_else(bad)?;
        if pieces.is_empty() {
            return Err(bad());
        }
        let mut consumed = BTreeSet::new();
        for piece in pieces {
            let matching: Vec<_> = qualified
                .iter()
                .filter(|q| q["id"] == piece["qualification_id"])
                .collect();
            if matching.len() != 1 {
                return Err(bad());
            }
            let identity = hash(matching[0])?;
            if piece["qualification_sha256"] != identity {
                return Err(bad());
            }
            consumed.insert(identity);
        }
        if protocols
            .insert(
                id,
                json!({"instance":path["instance"],"consumed_qualifications":consumed}),
            )
            .is_some()
        {
            return Err(bad());
        }
    }
    Ok(
        json!({"model":bridges["model"],"reference_basis":bridges["reference_basis"],"paths":protocols}),
    )
}

fn admit_simulation(
    row: &Alternative,
    descriptor: &Value,
    simulation: &Value,
) -> Result<(), Refusal> {
    let bound = row
        .binding
        .value()
        .ok_or_else(|| invalid("scenario comparison requires successful binding"))?;
    let same = |key: &str, expected: &Value| {
        expected
            .as_str()
            .is_some_and(|s| !s.is_empty() && simulation[key].as_str() == Some(s))
    };
    if simulation["schema"] != "ftx-drape-scenario-result/1"
        || !same("pattern_sha256", &bound["pattern_sha256"])
        || !same("scenario_sha256", &descriptor["scenario_sha256"])
        || !same("body_geometry_sha256", &descriptor["body_geometry_sha256"])
    {
        return Err(invalid(
            "scenario comparison needs fresh production results matching this bound pattern, body and scenario; missing/stale identities refuse",
        ));
    }
    if simulation["drape"]["settled"] != true
        || simulation["metrics"]["simulation_gating_clear"] != true
        || !simulation["drape"]["gating"]
            .as_array()
            .is_some_and(Vec::is_empty)
    {
        return Err(invalid(
            "scenario comparison requires a geometrically settled, gating-clear drape; review retained flags rather than ranking an unfinished frame",
        ));
    }
    Ok(())
}

// These endpoints are conditional on the already admitted geometry and canonical source network.
// Summing monotone endpoints does not assume independent specimens or turn SD/CV into limits.
type TransferBounds = [[f64; 2]; 2];

fn read_transfer_bounds(
    value: &Value,
    names: [&str; 2],
) -> Result<Option<TransferBounds>, Refusal> {
    if value.is_null() {
        return Ok(None);
    }
    let mut result: TransferBounds = [[0.; 2]; 2];
    for (range, name) in result.iter_mut().zip(names) {
        *range = serde_json::from_value(value[name].clone())
            .map_err(|_| invalid("vapor reported bounds need two complete endpoints per metric"))?;
        if range.iter().any(|v| !v.is_finite() || *v < 0.) || range[0] > range[1] {
            return Err(invalid(
                "vapor reported bounds must be finite, nonnegative and ordered",
            ));
        }
    }
    Ok(Some(result))
}

fn accumulate_transfer_bounds(
    total: &mut Option<TransferBounds>,
    term: Option<TransferBounds>,
    scale: f64,
) {
    let (Some(mut sum), Some(term)) = (*total, term) else {
        *total = None;
        return;
    };
    for (sum, range) in sum.iter_mut().zip(term) {
        for (sum, endpoint) in sum.iter_mut().zip(range) {
            let value = endpoint * scale;
            let next = *sum + value;
            if !scale.is_finite()
                || scale <= 0.
                || !value.is_finite()
                || (endpoint > 0. && value <= 0.)
                || !next.is_finite()
                || (value > 0. && next <= *sum)
                || (*sum > 0. && value > 0. && next <= value)
            {
                *total = None;
                return;
            }
            *sum = next;
        }
    }
    *total = Some(sum);
}

fn match_transfer_bounds(
    expected: Option<TransferBounds>,
    actual: &Value,
    names: [&str; 2],
) -> Result<Option<TransferBounds>, Refusal> {
    if read_transfer_bounds(actual, names)? != expected {
        return Err(invalid(
            "vapor reported bounds differ from complete canonical source endpoints at the admitted geometry and sample times",
        ));
    }
    Ok(expected)
}

fn sheet_vapor_bounds(result: &Value) -> Result<Option<TransferBounds>, Refusal> {
    let mut total = Some([[0.; 2]; 2]);
    for patch in vapor_array(result, "patches")? {
        let column = vapor_index(&patch["column"])?;
        let bounds = read_transfer_bounds(
            &result["columns"][column]["reported_bounds"],
            ["potential_heat_flux_w_m2", "potential_mass_flux_kg_m2_s"],
        )?;
        let area = finite(&patch["area_m2"])
            .ok_or_else(|| invalid("vapor bounds require the admitted patch area"))?;
        accumulate_transfer_bounds(&mut total, bounds, area);
    }
    match_transfer_bounds(
        total,
        &result["reported_bounds"],
        ["potential_heat_flow_w", "potential_mass_flow_kg_s"],
    )
}

fn covered_vapor_bounds(result: &Value) -> Result<Option<TransferBounds>, Refusal> {
    let mut total = Some([[0.; 2]; 2]);
    for region in vapor_array(&result["coverage"], "regions")? {
        let bounds = read_transfer_bounds(
            &region["vapor"]["reported_bounds"],
            ["potential_heat_flow_w", "potential_mass_flow_kg_s"],
        )?;
        accumulate_transfer_bounds(&mut total, bounds, 1.);
    }
    match_transfer_bounds(
        total,
        &result["coverage"]["reported_bounds"],
        [
            "covered_potential_heat_flow_w",
            "covered_potential_mass_flow_kg_s",
        ],
    )
}

fn motion_vapor_bounds(history: &Value) -> Result<Option<TransferBounds>, Refusal> {
    let trapezoid = match history["integration"].as_str() {
        Some("right_endpoint") => false,
        Some("composite_trapezoid") => true,
        _ => {
            return Err(invalid(
                "vapor bounds require the admitted integration rule",
            ));
        }
    };
    let mut previous = if trapezoid {
        covered_vapor_bounds(&history["initial"]["result"])?
    } else {
        None
    };
    let mut total = Some([[0.; 2]; 2]);
    for sample in vapor_array(history, "samples")? {
        // The existing motion admission has already replayed each canonical regional network.
        let bounds = covered_vapor_bounds(&sample["result"])?;
        let dt = finite(&sample["dt_s"])
            .ok_or_else(|| invalid("vapor bounds require the admitted sample interval"))?;
        let mut interval = Some([[0.; 2]; 2]);
        if trapezoid {
            // Match the canonical producer's two weighted endpoints, including time zero.
            accumulate_transfer_bounds(&mut interval, previous, 0.5 * dt);
            accumulate_transfer_bounds(&mut interval, bounds, 0.5 * dt);
        } else {
            accumulate_transfer_bounds(&mut interval, bounds, dt);
        }
        match_transfer_bounds(
            interval,
            &sample["reported_bounds"],
            ["potential_energy_j", "potential_mass_kg"],
        )?;
        accumulate_transfer_bounds(&mut total, interval, 1.);
        previous = bounds;
    }
    match_transfer_bounds(
        total,
        &history["totals"]["reported_bounds"],
        ["potential_energy_j", "potential_mass_kg"],
    )
}

fn admit_vapor_surface<'a>(
    alternative: &Alternative,
    descriptor: &Value,
    simulation: &'a Value,
) -> Result<&'a Value, Refusal> {
    let fail = || {
        invalid(
            "draped vapor comparison needs the requested source/geometry-matched complete initial-sheet result",
        )
    };
    let requested = &descriptor["scenario"]["vapor_surface"];
    let vapor = &simulation["vapor_surface"]["result"];
    let frame = &simulation["drape"]["vapor_surface"];
    let model = &frame["model"];
    if requested["schema"] != "ftx-vapor-surface-conditions/1"
        || simulation["vapor_surface"]["status"] != "available"
        || vapor["schema"] != "ftx-vapor-surface-result/1"
        || frame["schema"] != "ftx-drape-vapor-surface/1"
        || frame["available"] != true
        || model["schema"] != "ftx-vapor-surface/1"
        || !same_identity(&vapor["model_sha256"], &model["sha256"])
        || !same_identity(&vapor["drape_vapor_frame_sha256"], &frame["sha256"])
        || (!requested["model_sha256"].is_null() && requested["model_sha256"] != model["sha256"])
    {
        return Err(fail());
    }
    // The evaluator freezes the model identity even for uniform caller boundaries. Other
    // conditions must remain exactly the same; mixed-column consent is not comparison consent.
    let mut expected_conditions = requested.clone();
    expected_conditions["model_sha256"] = model["sha256"].clone();
    if vapor["request"]["conditions"] != expected_conditions
        || vapor["request"]["report"]
            != json!({"material":simulation["drape"]["material"],"vapor_surface":frame})
        || !same_identity(&vapor["request_sha256"], &json!(hash(&vapor["request"])?))
        || frame["material_report_sha256"] != json!(hash(&simulation["drape"]["material"])?)
        || vapor["positions_m"] != simulation["positions_m"]
        || frame["positions_m"] != simulation["positions_m"]
        || !same_identity(
            &simulation["positions_sha256"],
            &json!(hash(&simulation["positions_m"])?),
        )
        || vapor["patch_topology"] != model["patches"]
        || vapor["triangle_instance"] != frame["triangle_instance"]
        || vapor["instances"] != frame["instances"]
    {
        return Err(fail());
    }
    for retained in [model, frame] {
        let mut unhashed = retained.clone();
        unhashed.as_object_mut().ok_or_else(fail)?.remove("sha256");
        if !same_identity(&retained["sha256"], &json!(hash(&unhashed)?)) {
            return Err(fail());
        }
    }
    let patches = model["patches"]
        .as_array()
        .filter(|p| !p.is_empty())
        .ok_or_else(fail)?;
    let triangles = simulation["triangles"].as_array().ok_or_else(fail)?;
    let columns = vapor["columns"].as_array().ok_or_else(fail)?;
    let selections = model["columns"].as_array().ok_or_else(fail)?;
    let readings = vapor["patches"].as_array().ok_or_else(fail)?;
    let owners = vapor["triangle_instance"].as_array().ok_or_else(fail)?;
    let instances = vapor["instances"].as_array().ok_or_else(fail)?;
    let positions: Vec<ftx_core::Vec3> =
        serde_json::from_value(simulation["positions_m"].clone()).map_err(|_| fail())?;
    let mut instance_ids = BTreeSet::new();
    if patches.len() != triangles.len()
        || readings.len() != patches.len()
        || owners.len() != patches.len()
        || columns.len() != selections.len()
        || instances.iter().any(|id| {
            id.as_str()
                .is_none_or(|id| id.is_empty() || !instance_ids.insert(id))
        })
        || model["vertex_count"].as_u64() != Some(positions.len() as u64)
        || positions.iter().any(|p| !p.is_finite())
    {
        return Err(fail());
    }
    admit_vapor_assignments(alternative, instances, owners, patches, selections)?;
    let mut consumed = BTreeMap::new();
    let mut totals = [0.0; 3];
    for (((patch, triangle), reading), owner) in
        patches.iter().zip(triangles).zip(readings).zip(owners)
    {
        let column = patch["column"]
            .as_u64()
            .and_then(|i| usize::try_from(i).ok())
            .filter(|&i| i < columns.len())
            .ok_or_else(fail)?;
        let vertices: [usize; 3] = serde_json::from_value(triangle.clone()).map_err(|_| fail())?;
        if patch["vertices"] != *triangle
            || reading["column"] != patch["column"]
            || reading["instance"] != *owner
            || !owner.as_u64().is_some_and(|i| i < instances.len() as u64)
            || vertices.iter().any(|&v| v >= positions.len())
        {
            return Err(fail());
        }
        let owner = owner
            .as_u64()
            .and_then(|i| usize::try_from(i).ok())
            .ok_or_else(fail)?;
        let material = &simulation["drape"]["material"];
        let (identity, record) = match material["basis"].as_str() {
            Some("characterized_record") => (&material["record_sha256"], &material["record"]),
            Some("per_piece_characterization") => {
                let receipt = &material["instances"][owner];
                if receipt["instance"] != instances[owner]
                    || !material["instances"]
                        .as_array()
                        .is_some_and(|r| r.len() == instances.len())
                {
                    return Err(fail());
                }
                let identity = receipt["record_sha256"].as_str().ok_or_else(fail)?;
                (
                    &receipt["record_sha256"],
                    &material["records"][identity]["record"],
                )
            }
            _ => return Err(fail()),
        };
        if !same_identity(&columns[column]["layers"][0]["material_sha256"], identity)
            || consumed
                .insert(column, record)
                .is_some_and(|previous| previous != record)
        {
            return Err(fail());
        }
        let [a, b, c] = vertices.map(|v| positions[v]);
        let area = 0.5 * (b - a).cross(c - a).norm();
        if finite(&reading["area_m2"]) != Some(area) {
            return Err(fail());
        }
        for (flux, flow) in [
            ("potential_heat_flux_w_m2", "potential_heat_flow_w"),
            ("potential_mass_flux_kg_m2_s", "potential_mass_flow_kg_s"),
        ] {
            let value = finite(&columns[column][flux]).ok_or_else(fail)?;
            if reading[flux] != columns[column][flux]
                || finite(&reading[flow]) != Some(value * area)
            {
                return Err(fail());
            }
        }
        for (i, key) in [
            "area_m2",
            "potential_heat_flow_w",
            "potential_mass_flow_kg_s",
        ]
        .iter()
        .enumerate()
        {
            let value = finite(&reading[*key])
                .filter(|&v| if i == 0 { v > 0.0 } else { v >= 0.0 })
                .ok_or_else(fail)?;
            totals[i] += value;
        }
    }
    for (total, key) in totals.into_iter().zip([
        "area_m2",
        "potential_heat_flow_w",
        "potential_mass_flow_kg_s",
    ]) {
        if !total.is_finite() || finite(&vapor[key]) != Some(total) {
            return Err(fail());
        }
    }
    // Reuse the existing source-qualified column consumer. This checks frozen source policy and
    // declared boundary arithmetic without adding another transport model or requiring dry heat.
    let catalog = ftx_pattern::materials::Catalog {
        schema: ftx_pattern::materials::CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    };
    let boundaries = requested["boundaries"].as_array().ok_or_else(fail)?;
    if boundaries.len() != 1
        && (boundaries.len() != columns.len() || requested["model_sha256"].is_null())
    {
        return Err(fail());
    }
    let mut protocols = BTreeSet::new();
    for (index, retained_record) in consumed {
        let mut request = boundaries[if boundaries.len() == 1 { 0 } else { index }].clone();
        request.as_object_mut().ok_or_else(fail)?.extend(
            json!({"schema":"ftx-vapor-stack/1","area_m2":1.0,
                "layers":[{"key":"sheet","selection":selections[index]}],
                "required_conditions":requested["required_conditions"],
                "required_method":requested["required_method"],
                "allow_mixed_protocols":requested["allow_mixed_protocols"]})
            .as_object()
            .ok_or_else(fail)?
            .clone(),
        );
        let request: ftx_pattern::material_vapor::Request =
            serde_json::from_value(request).map_err(|_| fail())?;
        if serde_json::to_value(catalog.resolve(&request.layers[0].selection)?)
            .map_err(|_| fail())?
            != *retained_record
            || columns[index]
                != serde_json::to_value(request.evaluate(&catalog)?).map_err(|_| fail())?
        {
            return Err(fail());
        }
        let q = &columns[index]["layers"][0]["observation"];
        protocols.insert(hash(&(&q["method"], &q["conditions"]))?);
    }
    if protocols.len() > 1 && requested["allow_mixed_protocols"] != true {
        return Err(fail());
    }
    Ok(vapor)
}

fn same_identity(actual: &Value, expected: &Value) -> bool {
    expected
        .as_str()
        .is_some_and(|s| !s.is_empty() && actual.as_str() == Some(s))
}

fn vapor_array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], Refusal> {
    value[key]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| invalid(format!("covered vapor needs retained {key}")))
}

fn vapor_index(value: &Value) -> Result<usize, Refusal> {
    value
        .as_u64()
        .and_then(|i| usize::try_from(i).ok())
        .ok_or_else(|| invalid("covered vapor needs an integer retained index"))
}

// Self-consistent callback receipts do not establish which material belongs to an alternative.
// Resolve each original physical instance through the final source-locked binding, independently
// of column order/deduplication and of the material receipt carried by the numerical result.
fn admit_vapor_assignments(
    alternative: &Alternative,
    instances: &[Value],
    owners: &[Value],
    patches: &[Value],
    materials: &[Value],
) -> Result<(), Refusal> {
    let fail =
        || invalid("vapor source differs from this alternative's final material assignments");
    if owners.len() != patches.len() {
        return Err(fail());
    }
    let bound = alternative.binding.value().ok_or_else(fail)?;
    let pattern = ftx_pattern::Pattern::new(bound["pattern"].clone());
    let changes = alternative.frozen_changes.as_ref().ok_or_else(fail)?;
    let catalog = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    let selections = instances
        .iter()
        .map(|instance| {
            let piece = &pattern
                .instance(instance.as_str().ok_or_else(fail)?)
                .ok_or_else(fail)?
                .piece;
            let selection = changes
                .pieces
                .get(piece)
                .or(changes.default.as_ref())
                .ok_or_else(fail)?;
            serde_json::to_value(catalog.resolve(selection)?.frozen_selection()).map_err(|_| fail())
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (patch, owner) in patches.iter().zip(owners) {
        let expected = selections.get(vapor_index(owner)?).ok_or_else(fail)?;
        let actual = materials
            .get(vapor_index(&patch["column"])?)
            .ok_or_else(fail)?;
        if expected != actual {
            return Err(fail());
        }
    }
    Ok(())
}

// A caller-supplied material-report digest is not evidence that the embedded source still uses
// that report's selections. Resolve only the frozen inputs and compare the complete canonical
// receipts, including estimate policy, before accepting each original instance's assignment.
fn admit_vapor_material(source: &Value, report: &Value) -> Result<(), Refusal> {
    let instances = vapor_array(source, "instances")?;
    let owners = vapor_array(source, "triangle_instance")?;
    let patches = vapor_array(source, "patches")?;
    let selections = vapor_array(source, "materials")?;
    if source["schema"] != "ftx-vapor-coverage-source/1"
        || instances.is_empty()
        || instances.len() > 65_536
        || patches.is_empty()
        || patches.len() > 65_536
        || owners.len() != patches.len()
        || selections.is_empty()
        || selections.len() > patches.len()
    {
        return Err(invalid("covered vapor source ownership is incomplete"));
    }
    let names: BTreeSet<_> = instances.iter().filter_map(Value::as_str).collect();
    if names.len() != instances.len() || names.iter().any(|name| name.trim().is_empty()) {
        return Err(invalid("covered vapor source instances must be distinct"));
    }
    let instance_hashes: Vec<_> = match report["basis"].as_str() {
        Some("characterized_record") => {
            let id = report["record_sha256"]
                .as_str()
                .ok_or_else(|| invalid("covered vapor material receipt has no identity"))?;
            vec![id; instances.len()]
        }
        Some("per_piece_characterization") => {
            let receipts = vapor_array(report, "instances")?;
            if receipts.len() != instances.len()
                || report["triangle_instance"] != source["triangle_instance"]
                || receipts
                    .iter()
                    .zip(instances)
                    .any(|(r, id)| r["instance"] != *id)
            {
                return Err(invalid(
                    "covered vapor source ownership differs from the retained drape",
                ));
            }
            receipts
                .iter()
                .map(|r| {
                    r["record_sha256"].as_str().ok_or_else(|| {
                        invalid("covered vapor instance receipt has no material identity")
                    })
                })
                .collect::<Result<_, _>>()?
        }
        _ => {
            return Err(invalid(
                "covered vapor needs characterized drape material receipts",
            ));
        }
    };
    let catalog = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    let mut hashes = Vec::with_capacity(selections.len());
    for value in selections {
        let selection: Selection = serde_json::from_value(value.clone())
            .map_err(|e| invalid(format!("covered vapor frozen material: {e}")))?;
        if selection.record.is_none() || selection.expected_sha256.is_none() {
            return Err(invalid(
                "covered vapor material must be frozen and digest-locked",
            ));
        }
        let resolved = catalog.resolve(&selection)?;
        let receipt = if report["basis"] == "characterized_record" {
            &report["record"]
        } else {
            &report["records"][resolved.sha256()]["record"]
        };
        if serde_json::to_value(&resolved).map_err(|e| invalid(e.to_string()))? != *receipt
            || serde_json::to_value(resolved.frozen_selection())
                .map_err(|e| invalid(e.to_string()))?
                != *value
        {
            return Err(invalid(
                "covered vapor frozen selection differs from its canonical drape receipt",
            ));
        }
        hashes.push(resolved.sha256().to_string());
    }
    let mut used = BTreeSet::new();
    for (patch, owner) in patches.iter().zip(owners) {
        let column = vapor_index(&patch["column"])?;
        let owner = vapor_index(owner)?;
        let actual = hashes.get(column);
        let expected = instance_hashes.get(owner);
        if actual.is_none() || expected.is_none() || actual.map(String::as_str) != expected.copied()
        {
            return Err(invalid(
                "covered vapor patch material differs from its retained instance receipt",
            ));
        }
        used.insert(column);
    }
    if used.len() != selections.len() {
        return Err(invalid(
            "covered vapor source has unassigned material columns",
        ));
    }
    Ok(())
}

fn covered_vapor_protocols(vapor: &Value) -> Result<BTreeSet<String>, Refusal> {
    let regions = vapor_array(&vapor["coverage"], "regions")?;
    let projected = vapor_array(&vapor["projection"], "regions")?;
    let source = &vapor["request"]["source"];
    let instances = vapor_array(source, "instances")?;
    let owners = vapor_array(source, "triangle_instance")?;
    if regions.len() != projected.len() {
        return Err(invalid(
            "covered vapor regions differ from their retained projection",
        ));
    }
    let mut protocols = BTreeSet::new();
    for (region, geometry) in regions.iter().zip(projected) {
        let layers = vapor_array(&region["vapor"], "layers")?;
        let portions = vapor_array(geometry, "layers")?;
        if region["id"].as_str().is_none()
            || region["id"] != geometry["id"]
            || region["body_triangle"] != geometry["body_triangle"]
            || layers.len() != portions.len()
        {
            return Err(invalid(
                "covered vapor layer occurrences differ from their projection",
            ));
        }
        for (layer, portion) in layers.iter().zip(portions) {
            let triangle = vapor_index(&portion["cloth_triangle"])?;
            let owner = owners
                .get(triangle)
                .ok_or_else(|| invalid("covered vapor occurrence has no retained triangle"))?;
            let instance = instances
                .get(vapor_index(owner)?)
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("covered vapor occurrence has no retained instance"))?;
            if layer["key"] != portion["layer"]
                || layer["key"] != vapor["request"]["map"]["instance_layers"][instance]
            {
                return Err(invalid(
                    "covered vapor occurrence differs from its physical layer",
                ));
            }
            let q = &layer["observation"];
            // Triangulation and region count may change, but swapping protocols between two
            // original instances in the same physical layer must not become protocol equality.
            protocols.insert(
                serde_json::to_string(&json!({"instance":instance,"key":layer["key"],
                "unit":q["unit"],"method":q["method"],"conditions":q["conditions"],
                "evidence":q["evidence"]}))
                .map_err(|e| invalid(e.to_string()))?,
            );
        }
    }
    Ok(protocols)
}

fn admit_vapor_coverage<'a>(
    alternative: &Alternative,
    descriptor: &Value,
    simulation: &'a Value,
) -> Result<&'a Value, Refusal> {
    let policy = &descriptor["scenario"]["vapor_coverage"];
    let result = &simulation["vapor_coverage"]["result"];
    let mut expected_conditions = policy["conditions"].clone();
    if let Some(conditions) = expected_conditions.as_object_mut() {
        conditions.insert(
            "body_mesh_sha256".into(),
            descriptor["vapor_coverage_body_mesh_sha256"].clone(),
        );
    }
    if simulation["vapor_coverage"]["status"] != "available" {
        return Err(invalid("covered vapor result unavailable"));
    }
    let source = &result["request"]["source"];
    admit_vapor_assignments(
        alternative,
        vapor_array(source, "instances")?,
        vapor_array(source, "triangle_instance")?,
        vapor_array(source, "patches")?,
        vapor_array(source, "materials")?,
    )?;
    admit_vapor_frame(
        policy,
        result,
        &descriptor["vapor_coverage_body_mesh_sha256"],
        &expected_conditions,
        VaporFrame {
            positions: &simulation["positions_m"],
            triangles: &simulation["triangles"],
            material: &simulation["drape"]["material"],
        },
    )?;
    // Geometry/source receipts do not establish the reported transport. Static objectives need
    // the same canonical regional networks and complete area/rate accounting as moving ones.
    covered_vapor_potential(result)?;
    Ok(result)
}

struct VaporFrame<'a> {
    positions: &'a Value,
    triangles: &'a Value,
    material: &'a Value,
}

// Projection partitions barycentric triangles to 1e-9 area and allocation admission already
// uses this relative accuracy. Completeness is scale-free at that accuracy, per body triangle
// as well as in total; it does not rewrite the producer's covered/uncovered diagnostics.
const VAPOR_COVERAGE_RELATIVE_TOLERANCE: f64 = 1e-9;

fn complete_vapor_area(area: f64, row: &Value) -> bool {
    let Some(covered) = finite(&row["covered_area_m2"]).filter(|a| *a >= 0.) else {
        return false;
    };
    let Some(uncovered) = finite(&row["uncovered_area_m2"]).filter(|a| *a >= 0.) else {
        return false;
    };
    area.is_finite()
        && area > 0.
        && uncovered / area <= VAPOR_COVERAGE_RELATIVE_TOLERANCE
        && (covered / area - 1.).abs() <= VAPOR_COVERAGE_RELATIVE_TOLERANCE
        && ((covered + uncovered) / area - 1.).abs() <= VAPOR_COVERAGE_RELATIVE_TOLERANCE
}

fn admit_vapor_frame<'a>(
    policy: &Value,
    result: &'a Value,
    body_mesh_sha256: &Value,
    expected_conditions: &Value,
    frame: VaporFrame<'_>,
) -> Result<&'a Value, Refusal> {
    let request = &result["request"];
    let source = &request["source"];
    if !policy.is_object()
        || result["schema"] != "ftx-vapor-coverage-report/1"
        || request["schema"] != "ftx-vapor-coverage/1"
        || !same_identity(&result["body_mesh_sha256"], body_mesh_sha256)
        || request["map"]["instance_layers"] != policy["instance_layers"]
        || request["map"]["settings"] != policy["settings"]
        || request["conditions"] != *expected_conditions
        || result["request_sha256"].as_str() != Some(hash(request)?.as_str())
        || result["body_mesh_sha256"].as_str()
            != Some(
                hash(&json!({"positions_m":request["map"]["body_positions_m"],
                "triangles":request["map"]["body_triangles"]}))?
                .as_str(),
            )
        || source["positions_m"] != *frame.positions
        || result["source_sha256"].as_str() != Some(hash(source)?.as_str())
        || source["material_report_sha256"].as_str() != Some(hash(frame.material)?.as_str())
    {
        return Err(invalid(
            "covered vapor comparison needs the current material/geometry and exact requested body domain, projection and pressure conditions",
        ));
    }
    let patches = source["patches"]
        .as_array()
        .ok_or_else(|| invalid("covered vapor source patches unavailable"))?;
    let triangles = frame
        .triangles
        .as_array()
        .ok_or_else(|| invalid("current drape triangles unavailable"))?;
    if patches.len() != triangles.len()
        || patches
            .iter()
            .zip(triangles)
            .any(|(p, t)| p["vertices"] != *t)
    {
        return Err(invalid(
            "covered vapor topology differs from the current drape",
        ));
    }
    admit_vapor_material(source, frame.material)?;
    let coverage = &result["coverage"];
    let body_area = coverage["body_area_m2"]
        .as_f64()
        .filter(|a| a.is_finite() && *a > 0.)
        .ok_or_else(|| invalid("covered vapor body area unavailable"))?;
    if !complete_vapor_area(body_area, coverage) {
        return Err(invalid(
            "covered vapor objective requires complete declared body coverage; a partial subtotal cannot stand in for unavailable bare-body transfer",
        ));
    }
    Ok(result)
}

fn admit_motion_vapor_coverage<'a>(
    alternative: &Alternative,
    descriptor: &Value,
    simulation: &Value,
    motion: &'a Value,
) -> Result<&'a Value, Refusal> {
    let fail = || {
        invalid(
            "moving covered vapor comparison needs the complete current material/path/geometry history, fully covered body domain and exact potential integrals",
        )
    };
    let policy = &descriptor["scenario"]["motion"]["vapor_coverage_history"];
    let trapezoid = match policy["integration"].as_str() {
        Some("right_endpoint") => false,
        Some("composite_trapezoid") => true,
        _ => return Err(fail()),
    };
    let coverage_policy = &descriptor["scenario"]["vapor_coverage"];
    let history = &motion["vapor_coverage_history"]["result"];
    let request = &history["request"];
    let initial = &history["initial"];
    let initial_result = &initial["result"];
    let initial_request = &request["initial"];
    let samples = vapor_array(motion, "samples")?;
    let rows = vapor_array(history, "samples")?;
    let retained = vapor_array(request, "samples")?;
    if motion["vapor_coverage_history"]["status"] != "available"
        || policy["schema"] != "ftx-motion-vapor-coverage/1"
        || history["schema"] != "ftx-vapor-coverage-history-report/1"
        || request["schema"] != "ftx-vapor-coverage-history/1"
        || request["policy"] != *policy
        || history["integration"] != policy["integration"]
        || rows.len() != samples.len()
        || retained.len() != samples.len()
        || initial["time_s"].as_f64() != Some(0.)
        || history["duration_s"] != motion["duration_s"]
        || !same_identity(&history["path_sha256"], &motion["path_sha256"])
        || request["path_sha256"] != motion["path_sha256"]
        || !same_identity(
            &history["initial_positions_sha256"],
            &simulation["positions_sha256"],
        )
        || history["initial_positions_sha256"] != initial["positions_sha256"]
        || motion["initial_sample"]["time_s"].as_f64() != Some(0.)
        || (!coverage_policy["conditions"]["body_mesh_sha256"].is_null()
            && coverage_policy["conditions"]["body_mesh_sha256"]
                != descriptor["vapor_coverage_body_mesh_sha256"])
        || initial_request != &initial_result["request"]
        || history["request_sha256"].as_str() != Some(hash(request)?.as_str())
        || serde_json::from_str::<Value>(history["replay_request_json"].as_str().ok_or_else(fail)?)
            .map_err(|_| fail())?
            != *request
    {
        return Err(fail());
    }
    let body_triangles = vapor_array(motion, "body_triangles")?;
    let body_ids = vapor_array(coverage_policy, "body_triangle_ids")?;
    if body_ids.is_empty() || body_ids.len() > 4096 {
        return Err(fail());
    }
    let selected: Vec<_> = body_ids
        .iter()
        .map(|id| {
            body_triangles
                .get(vapor_index(id)?)
                .cloned()
                .ok_or_else(fail)
        })
        .collect::<Result<_, _>>()?;
    if body_ids
        .iter()
        .map(vapor_index)
        .collect::<Result<BTreeSet<_>, _>>()?
        .len()
        != body_ids.len()
        || initial_request["map"]["body_triangles"] != json!(selected)
        || motion["triangles"] != simulation["triangles"]
        || motion["initial_sample"]["positions_m"] != simulation["positions_m"]
        || initial_request["map"]["body_positions_m"]
            != motion["initial_sample"]["body_positions_m"]
    {
        return Err(fail());
    }
    let source = &initial_request["source"];
    admit_vapor_assignments(
        alternative,
        vapor_array(source, "instances")?,
        vapor_array(source, "triangle_instance")?,
        vapor_array(source, "patches")?,
        vapor_array(source, "materials")?,
    )?;
    let mut time = 0.;
    let mut energy = 0.;
    let mut mass = 0.;
    let mut work = 0_u64;
    let mut previous = (0., 0.);
    let interval = |previous: f64, current: f64, dt: f64| -> Result<f64, Refusal> {
        if !trapezoid {
            return Ok(current * dt);
        }
        // Preserve the production history's operation order: two half-interval products,
        // then checked addition. Averaging the rates first can overflow a finite integral.
        let first = previous * (0.5 * dt);
        let second = current * (0.5 * dt);
        let total = first + second;
        if !first.is_finite()
            || !second.is_finite()
            || !total.is_finite()
            || (previous > 0. && first <= 0.)
            || (current > 0. && second <= 0.)
            || (second > 0. && total <= first)
        {
            return Err(fail());
        }
        Ok(total)
    };
    for (index, (row, sample)) in std::iter::once((initial, &motion["initial_sample"]))
        .chain(rows.iter().zip(samples))
        .enumerate()
    {
        let result = &row["result"];
        let current = &result["request"];
        let body_hash = json!(hash(&json!({"positions_m":sample["body_positions_m"],
            "triangles":selected}))?);
        let mut conditions = coverage_policy["conditions"].clone();
        conditions
            .as_object_mut()
            .ok_or_else(fail)?
            .insert("body_mesh_sha256".into(), body_hash.clone());
        if row["positions_sha256"].as_str() != Some(hash(&sample["positions_m"])?.as_str())
            || serde_json::from_str::<Value>(
                result["replay_request_json"].as_str().ok_or_else(fail)?,
            )
            .map_err(|_| fail())?
                != *current
            || row["body_positions_sha256"].as_str()
                != Some(hash(&sample["body_positions_m"])?.as_str())
            || current["map"]["body_positions_m"] != sample["body_positions_m"]
            || current["map"]["body_triangles"] != initial_request["map"]["body_triangles"]
            || [
                "patches",
                "materials",
                "instances",
                "triangle_instance",
                "material_report_sha256",
            ]
            .iter()
            .any(|key| current["source"][key] != initial_request["source"][key])
            || (index == 0
                && !same_identity(&body_hash, &descriptor["vapor_coverage_body_mesh_sha256"]))
        {
            return Err(fail());
        }
        admit_vapor_frame(
            coverage_policy,
            result,
            &body_hash,
            &conditions,
            VaporFrame {
                positions: &sample["positions_m"],
                triangles: &simulation["triangles"],
                material: &simulation["drape"]["material"],
            },
        )?;
        let (power, flow) = covered_vapor_potential(result)?;
        work = work
            .checked_add(
                result["projection"]["geometry_operations"]
                    .as_u64()
                    .ok_or_else(fail)?,
            )
            .ok_or_else(fail)?;
        if index != 0 {
            let t = finite(&sample["time_s"]).ok_or_else(fail)?;
            let dt = t - time;
            if dt < 1e-9 {
                return Err(fail());
            }
            let delta_energy = interval(previous.0, power, dt)?;
            let delta_mass = interval(previous.1, flow, dt)?;
            if (power > 0. && delta_energy <= 0.)
                || (flow > 0. && delta_mass <= 0.)
                || (delta_energy > 0. && energy + delta_energy <= energy)
                || (delta_mass > 0. && mass + delta_mass <= mass)
                || row["sample_index"].as_u64() != Some((index - 1) as u64)
                || row["time_s"] != sample["time_s"]
                || row["dt_s"].as_f64() != Some(dt)
                || retained[index - 1]
                    != json!({"time_s":sample["time_s"],
                    "positions_m":sample["positions_m"],"body_positions_m":sample["body_positions_m"]})
                || row["potential_energy_j"].as_f64() != Some(delta_energy)
                || row["potential_mass_kg"].as_f64() != Some(delta_mass)
            {
                return Err(fail());
            }
            energy += delta_energy;
            mass += delta_mass;
            time = t;
        }
        previous = (power, flow);
    }
    let limit = policy["max_geometry_operations"]
        .as_u64()
        .ok_or_else(fail)?;
    if !(1..=20_000_000).contains(&limit)
        || work > limit
        || history["geometry_operations"].as_u64() != Some(work)
        || !energy.is_finite()
        || !mass.is_finite()
        || history["totals"]["potential_energy_j"].as_f64() != Some(energy)
        || history["totals"]["potential_mass_kg"].as_f64() != Some(mass)
    {
        return Err(fail());
    }
    Ok(history)
}

fn covered_vapor_potential(result: &Value) -> Result<(f64, f64), Refusal> {
    let fail = || {
        invalid(
            "covered vapor needs canonical source-qualified regional transport and complete potential totals",
        )
    };
    let regions = vapor_array(&result["coverage"], "regions")?;
    let projected = vapor_array(&result["projection"], "regions")?;
    let source = &result["request"]["source"];
    let map = &result["request"]["map"];
    let conditions = &result["request"]["conditions"];
    let boundaries = vapor_array(conditions, "boundaries")?;
    let body_triangles = vapor_array(map, "body_triangles")?;
    let body_rows = vapor_array(&result["coverage"], "body")?;
    let patches = vapor_array(source, "patches")?;
    if body_triangles.len() != body_rows.len() {
        return Err(fail());
    }
    let mut body_allocations = vec![0_f64; body_triangles.len()];
    let mut cloth_allocations = vec![0_f64; patches.len()];
    let mut ids = BTreeSet::new();
    let mut protocols = BTreeSet::new();
    let mut allocated_body_area = 0.;
    let mut power = 0.;
    let mut mass = 0.;
    if regions.is_empty() || regions.len() != projected.len() {
        return Err(fail());
    }
    for (region, geometry) in regions.iter().zip(projected) {
        let vapor = &region["vapor"];
        let request: ftx_pattern::material_vapor::Request =
            serde_json::from_value(vapor["request"].clone()).map_err(|_| fail())?;
        let portions = vapor_array(geometry, "layers")?;
        if request.layers.len() != portions.len() {
            return Err(fail());
        }
        let body = vapor_index(&geometry["body_triangle"])?;
        *body_allocations.get_mut(body).ok_or_else(fail)? +=
            finite(&geometry["body_fraction"]).ok_or_else(fail)?;
        let id = geometry["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(fail)?;
        if !ids.insert(id)
            || region["id"] != geometry["id"]
            || region["body_triangle"] != geometry["body_triangle"]
        {
            return Err(fail());
        }
        let boundary = boundaries
            .get(if boundaries.len() == 1 { 0 } else { body })
            .ok_or_else(fail)?;
        let body_area =
            vapor_triangle_area(&map["body_positions_m"], &map["body_triangles"][body])?
                * finite(&geometry["body_fraction"])
                    .filter(|f| *f > 0. && *f <= 1.)
                    .ok_or_else(fail)?;
        if request.area_m2 != body_area
            || region["body_area_m2"].as_f64() != Some(body_area)
            || [
                "inside_vapor_pressure_pa",
                "outside_vapor_pressure_pa",
                "inside_film_resistance_m2_pa_w",
                "outside_film_resistance_m2_pa_w",
                "latent_heat_j_kg",
            ]
            .iter()
            .any(|key| vapor["request"][*key] != boundary[*key])
            || [
                "required_conditions",
                "required_method",
                "allow_mixed_protocols",
            ]
            .iter()
            .any(|key| vapor["request"][*key] != conditions[*key])
        {
            return Err(fail());
        }
        allocated_body_area += body_area;
        for (index, (layer, portion)) in request.layers.iter().zip(portions).enumerate() {
            let patch = vapor_index(&portion["cloth_triangle"])?;
            let column = vapor_index(&source["patches"][patch]["column"])?;
            *cloth_allocations.get_mut(patch).ok_or_else(fail)? +=
                finite(&portion["fraction"]).ok_or_else(fail)?;
            let area = vapor_triangle_area(
                &source["positions_m"],
                &source["patches"][patch]["vertices"],
            )? * finite(&portion["fraction"])
                .filter(|f| *f > 0. && *f <= 1.)
                .ok_or_else(fail)?;
            if json!(layer.selection) != source["materials"][column]
                || layer.key != portion["layer"].as_str().ok_or_else(fail)?
                || layer.area_m2 != Some(area)
                || region["layer_area_m2"][index].as_f64() != Some(area)
            {
                return Err(fail());
            }
        }
        let interfaces = vapor_array(&map["settings"], "interfaces")?;
        let expected_interfaces = request
            .layers
            .windows(2)
            .map(|pair| {
                let matching: Vec<_> = interfaces
                    .iter()
                    .filter(|interface| {
                        interface["inner"] == pair[0].key && interface["outer"] == pair[1].key
                    })
                    .collect();
                if matching.len() != 1 {
                    return Err(fail());
                }
                finite(&matching[0]["resistance_m2_pa_w"])
                    .filter(|r| *r >= 0.)
                    .ok_or_else(fail)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if request.interface_resistance_m2_pa_w != expected_interfaces {
            return Err(fail());
        }
        let canonical = request.evaluate(&Catalog {
            schema: CATALOG_SCHEMA.into(),
            materials: vec![],
        })?;
        if serde_json::to_value(&canonical).map_err(|_| fail())? != *vapor {
            return Err(fail());
        }
        for layer in &canonical.layers {
            protocols.insert(hash(&(
                &layer.observation.method,
                &layer.observation.conditions,
            ))?);
        }
        if (canonical.potential_heat_flow_w > 0.
            && power + canonical.potential_heat_flow_w <= power)
            || (canonical.potential_mass_flow_kg_s > 0.
                && mass + canonical.potential_mass_flow_kg_s <= mass)
        {
            return Err(fail());
        }
        power += canonical.potential_heat_flow_w;
        mass += canonical.potential_mass_flow_kg_s;
    }
    let mut body_area = 0.;
    for (triangle, row) in body_triangles.iter().zip(body_rows) {
        let area = vapor_triangle_area(&map["body_positions_m"], triangle)?;
        if row["area_m2"].as_f64() != Some(area) || !complete_vapor_area(area, row) {
            return Err(fail());
        }
        body_area += area;
    }
    let mixed = protocols.len() > 1;
    if (mixed && conditions["allow_mixed_protocols"] != true)
        || result["mixed_protocols"] != json!(mixed)
        || body_allocations
            .iter()
            .any(|f| !f.is_finite() || (f - 1.).abs() > VAPOR_COVERAGE_RELATIVE_TOLERANCE)
        || cloth_allocations
            .iter()
            .any(|f| !f.is_finite() || *f > 1. + VAPOR_COVERAGE_RELATIVE_TOLERANCE)
        || !body_area.is_finite()
        || !allocated_body_area.is_finite()
        || (allocated_body_area - body_area).abs() > VAPOR_COVERAGE_RELATIVE_TOLERANCE * body_area
        || result["coverage"]["body_area_m2"].as_f64() != Some(body_area)
        || !power.is_finite()
        || !mass.is_finite()
        || result["coverage"]["covered_potential_heat_flow_w"].as_f64() != Some(power)
        || result["coverage"]["covered_potential_mass_flow_kg_s"].as_f64() != Some(mass)
    {
        return Err(fail());
    }
    Ok((power, mass))
}

fn vapor_triangle_area(positions: &Value, vertices: &Value) -> Result<f64, Refusal> {
    let fail = || invalid("covered vapor needs finite retained triangle geometry");
    let vertices = vertices
        .as_array()
        .filter(|v| v.len() == 3)
        .ok_or_else(fail)?;
    let point = |index: &Value| -> Result<ftx_core::Vec3, Refusal> {
        let point = &positions[vapor_index(index)?];
        let values = point.as_array().filter(|p| p.len() == 3).ok_or_else(fail)?;
        Ok(ftx_core::Vec3::new(
            finite(&values[0]).ok_or_else(fail)?,
            finite(&values[1]).ok_or_else(fail)?,
            finite(&values[2]).ok_or_else(fail)?,
        ))
    };
    let a = point(&vertices[0])?;
    let area = 0.5
        * (point(&vertices[1])? - a)
            .cross(point(&vertices[2])? - a)
            .norm();
    if !area.is_finite() || area <= 0. {
        return Err(fail());
    }
    Ok(area)
}

fn admit_motion<'a>(descriptor: &Value, simulation: &'a Value) -> Result<&'a Value, Refusal> {
    let request = &descriptor["scenario"]["motion"];
    let motion = &simulation["motion"]["result"];
    if request["schema"] != "ftx-drape-motion/1"
        || simulation["motion"]["status"] != "available"
        || motion["schema"] != "ftx-drape-motion-result/1"
        || !same_identity(&motion["path_sha256"], &descriptor["motion_path"]["sha256"])
        || !same_identity(
            &motion["initial_positions_sha256"],
            &simulation["positions_sha256"],
        )
        || motion["continuous_contact_checked"] != true
        || motion["initial_drape_settled"] != true
        || !motion["initial_drape_gating"]
            .as_array()
            .is_some_and(Vec::is_empty)
    {
        return Err(invalid(
            "motion comparison needs successful checked motion matching the requested path and admitted initial drape",
        ));
    }
    let frames = request["keyframes"]
        .as_array()
        .filter(|f| !f.is_empty() && f.len() <= 16)
        .ok_or_else(|| invalid("motion comparison has no bounded requested keyframes"))?;
    let samples = motion["samples"]
        .as_array()
        .filter(|s| !s.is_empty() && s.len() <= 512)
        .ok_or_else(|| invalid("motion comparison needs the complete accepted sample sequence"))?;
    let endpoints = motion["keyframes"]
        .as_array()
        .filter(|f| f.len() == frames.len())
        .ok_or_else(|| invalid("motion comparison needs every requested endpoint"))?;
    let duration = motion["duration_s"]
        .as_f64()
        .filter(|t| t.is_finite() && *t > 0.)
        .ok_or_else(|| invalid("motion comparison has no finite physical duration"))?;
    if frames.last().and_then(|f| f["time_s"].as_f64()) != Some(duration)
        || samples.last().and_then(|s| s["time_s"].as_f64()) != Some(duration)
        || !motion["trials"].as_u64().is_some_and(|n| {
            n >= samples.len() as u64 && request["max_trials"].as_u64().is_some_and(|max| n <= max)
        })
    {
        return Err(invalid(
            "motion comparison refuses partial or mismatched-duration trajectories",
        ));
    }
    let mut previous = 0.;
    for sample in samples {
        let time = sample["time_s"]
            .as_f64()
            .filter(|t| t.is_finite() && *t > previous && *t <= duration)
            .ok_or_else(|| invalid("motion comparison requires ordered physical sample times"))?;
        if sample["solve"]["converged"] != true {
            return Err(invalid(
                "motion comparison refuses an unconverged accepted substep",
            ));
        }
        previous = time;
    }
    let mut next_sample = 0;
    for (frame, endpoint) in frames.iter().zip(endpoints) {
        let index = endpoint["sample_index"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n >= next_sample && n < samples.len())
            .ok_or_else(|| {
                invalid("motion comparison requires ordered, retained endpoint samples")
            })?;
        let sample = &samples[index];
        if endpoint["time_s"] != frame["time_s"]
            || endpoint["time_s"] != sample["time_s"]
            || endpoint["arm_abduction_deg"] != frame["arm_abduction_deg"]
            || !sample["positions_m"].is_array()
            || !sample["body_positions_m"].is_array()
            || endpoint["positions_sha256"].as_str() != Some(hash(&sample["positions_m"])?.as_str())
            || endpoint["body_collision_sha256"].as_str()
                != Some(hash(&(&sample["body_positions_m"], &motion["body_triangles"]))?.as_str())
        {
            return Err(invalid(
                "motion comparison refuses stale endpoint pose, time or geometry",
            ));
        }
        next_sample = index + 1;
    }
    if next_sample != samples.len() {
        return Err(invalid(
            "motion comparison needs the final requested endpoint, without extra samples",
        ));
    }
    Ok(motion)
}

fn finite(value: &Value) -> Option<f64> {
    value.as_f64().filter(|x| x.is_finite())
}

fn admit_heat<'a>(
    alternative: &Alternative,
    descriptor: &Value,
    simulation: &Value,
    motion: &'a Value,
    consumer: Consumer,
) -> Result<Heat<'a>, Refusal> {
    let (name, schema, request_schema, coverage) = match consumer {
        Consumer::TransientSurface => (
            "transient_surface",
            "ftx-motion-surface-heat-result/1",
            "ftx-motion-surface-heat/1",
            false,
        ),
        Consumer::TransientCoverage => (
            "transient_coverage",
            "ftx-motion-coverage-heat-result/1",
            "ftx-motion-coverage-heat/1",
            true,
        ),
        _ => return Err(invalid("not a transient comparison consumer")),
    };
    let request = &descriptor["scenario"]["motion"][name];
    let h = &motion[name]["result"];
    let fail = |message: &str| invalid(format!("{consumer:?}: {message}"));
    if request["schema"] != request_schema
        || motion[name]["status"] != "available"
        || h["schema"] != schema
        || &h["request"] != request
        || h["duration_s"] != motion["duration_s"]
        || !same_identity(&h["path_sha256"], &motion["path_sha256"])
        || !same_identity(
            &h["initial_positions_sha256"],
            &motion["initial_positions_sha256"],
        )
    {
        return Err(fail(
            "complete thermal history must match the requested policy and admitted motion",
        ));
    }
    // Admission is deliberately tied to the actual accepted sequence, not an old scalar summary.
    // This is consistency checking of trusted production output, not an authenticity certificate.
    let samples = motion["samples"]
        .as_array()
        .ok_or_else(|| fail("missing admitted mechanical sequence"))?;
    let endpoints = motion["keyframes"]
        .as_array()
        .ok_or_else(|| fail("missing admitted mechanical endpoints"))?;
    let trace = h["trace"]
        .as_array()
        .filter(|t| !t.is_empty() && t.len() == samples.len())
        .ok_or_else(|| fail("thermal history must cover every accepted mechanical sample"))?;
    if !request["boundaries"]
        .as_array()
        .is_some_and(|b| b.len() == endpoints.len())
        || (coverage
            && !request["exposed_boundaries"]
                .as_array()
                .is_some_and(|b| b.len() == endpoints.len()))
    {
        return Err(fail("missing explicit interval boundary policy"));
    }
    let initial = finite(&request["initial_temperature_c"])
        .filter(|t| *t >= -273.15)
        .ok_or_else(|| fail("initial temperature must be finite and above absolute zero"))?;
    let mut range = [initial, initial];
    let mut final_range = None;
    let mut steps = 0_u64;
    let mut interval = 0;
    for (i, (row, sample)) in trace.iter().zip(samples).enumerate() {
        while endpoints[interval]["sample_index"]
            .as_u64()
            .is_some_and(|last| last < i as u64)
        {
            interval += 1;
            if interval >= endpoints.len() {
                return Err(fail("thermal history extends beyond the requested motion"));
            }
        }
        if row["sample_index"].as_u64() != Some(i as u64)
            || row["interval_index"].as_u64() != Some(interval as u64)
            || row["time_s"] != sample["time_s"]
            || row["positions_sha256"].as_str() != Some(hash(&sample["positions_m"])?.as_str())
            || (coverage
                && row["body_positions_sha256"].as_str()
                    != Some(hash(&sample["body_positions_m"])?.as_str()))
        {
            return Err(fail(
                "stale or missing thermal sample time, interval or geometry",
            ));
        }
        let temperatures = row["temperature_range_c"]
            .as_array()
            .filter(|v| v.len() == 2)
            .ok_or_else(|| fail("missing sampled temperature range"))?;
        let lo = finite(&temperatures[0]);
        let hi = finite(&temperatures[1]);
        match (lo, hi) {
            (Some(lo), Some(hi)) if lo >= -273.15 && lo <= hi => {
                range[0] = range[0].min(lo);
                range[1] = range[1].max(hi);
                final_range = Some([lo, hi]);
            }
            _ => {
                return Err(fail(
                    "sampled temperatures must be finite, ordered and physical",
                ));
            }
        }
        steps = row["thermal_substeps"]
            .as_u64()
            .filter(|&n| n > 0)
            .and_then(|n| steps.checked_add(n))
            .ok_or_else(|| fail("missing or overflowing thermal substep count"))?;
    }
    if h["thermal_steps"].as_u64() != Some(steps)
        || !h["cell_steps"].as_u64().is_some_and(|n| {
            n >= steps
                && request["max_cell_steps"]
                    .as_u64()
                    .is_some_and(|max| n <= max)
        })
    {
        return Err(fail(
            "thermal work counts do not describe the complete admitted history",
        ));
    }
    // Keep physical storage separate from remap error. Neither residual becomes incoming heat.
    let stored = if coverage {
        "physical_stored_change_j"
    } else {
        "stored_change_j"
    };
    for field in [
        "inside_energy_j",
        "outside_energy_j",
        "net_boundary_energy_j",
        stored,
        "balance_error_j",
        "absolute_step_balance_error_j",
    ] {
        if finite(&h[field]).is_none() {
            return Err(fail("missing finite full-history energy accounting"));
        }
    }
    if coverage {
        for field in [
            "bare_energy_j",
            "signed_remap_residual_j",
            "absolute_remap_residual_j",
        ] {
            if finite(&h[field]).is_none() {
                return Err(fail("missing separate bare-body and remap accounting"));
            }
        }
    }
    let bridged = motion_bridges::admit(simulation, motion, h, request, coverage)?;
    // Reconcile each source independently, then check storage against their SUM once. Neither
    // source may borrow the other's evidence or make its own full-temperature energy claim.
    let mut supplied = vec![0.; trace.len() + 1];
    let mut heated = false;
    for channel in [HeatWork::Plastic, HeatWork::Relaxation, HeatWork::Bending] {
        let field = channel.policy();
        match request.get(field).filter(|v| !v.is_null()) {
            Some(policy) if policy.as_str() == Some("uniform_reference_thickness") => {
                let source = admit_dissipation_heat(alternative, motion, h, consumer, channel)?;
                for (sum, value) in supplied.iter_mut().zip(source) {
                    *sum += value;
                    if !sum.is_finite() {
                        return Err(fail("combined internal-source heat overflows"));
                    }
                }
                heated = true;
            }
            Some(_) => {
                return Err(fail(
                    "unsupported dissipation heat policy for this thermal domain",
                ));
            }
            None if !h[field].is_null() || trace.iter().any(|r| !r[field].is_null()) => {
                return Err(fail(
                    "dissipation heating was not requested for this thermal domain",
                ));
            }
            None => {}
        }
    }
    let liquid = surface_water::admit(
        alternative,
        simulation,
        motion,
        h,
        request,
        coverage,
        &supplied,
    )?;
    if heated || liquid.is_some() || bridged {
        for (i, (row, supplied)) in trace
            .iter()
            .chain(std::iter::once(h))
            .zip(supplied)
            .enumerate()
        {
            // Equal bridge additions can erase a small stored-heat difference when large totals
            // are rounded. Reconcile storage against the retained baseline boundary ledger.
            let boundary_ledger = if bridged {
                &row["thermal_bridges"]
            } else {
                row
            };
            let inside_field = if bridged {
                "baseline_inside_energy_j"
            } else {
                "inside_energy_j"
            };
            let outside_field = if bridged {
                "baseline_outside_energy_j"
            } else {
                "outside_energy_j"
            };
            let inside = finite(&boundary_ledger[inside_field])
                .ok_or_else(|| fail("missing source/boundary balance"))?;
            let outside = finite(&boundary_ledger[outside_field])
                .ok_or_else(|| fail("missing source/boundary balance"))?;
            let net = finite(&row["net_boundary_energy_j"])
                .ok_or_else(|| fail("missing source/boundary balance"))?;
            let stored =
                finite(&row[stored]).ok_or_else(|| fail("missing source/boundary balance"))?;
            let residual = finite(&row["balance_error_j"])
                .ok_or_else(|| fail("missing source/boundary balance"))?;
            let liquid_energy = liquid.as_ref().map_or(0., |terms| terms[i]);
            if !heat_near(net, inside - outside)
                || !heat_near(residual, stored + liquid_energy - net - supplied)
            {
                return Err(fail(
                    "stored heat, liquid and vapor enthalpy ledger do not reconcile boundary energy and all qualified internal sources",
                ));
            }
        }
    }
    Ok(Heat {
        result: h,
        range_c: range,
        final_range_c: final_range
            .ok_or_else(|| fail("missing final sampled temperature range"))?,
    })
}

// Reconcile the published source terms with accepted work. This does not solve mechanics or heat
// again, nor certify an imported report: the existing consumer owns constitutive qualification.
#[derive(Clone, Copy, PartialEq)]
enum HeatWork {
    Plastic,
    Relaxation,
    Bending,
}

impl HeatWork {
    fn policy(self) -> &'static str {
        match self {
            Self::Plastic => "plastic_dissipation_heat",
            Self::Relaxation => "relaxation_dissipation_heat",
            Self::Bending => "bending_plastic_dissipation_heat",
        }
    }
}

fn heat_near(a: f64, b: f64) -> bool {
    a.is_finite() && b.is_finite() && (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.)
}

fn admit_dissipation_heat(
    alternative: &Alternative,
    motion: &Value,
    heat: &Value,
    consumer: Consumer,
    channel: HeatWork,
) -> Result<Vec<f64>, Refusal> {
    use ftx_pattern::materials::{CATALOG_SCHEMA, Catalog};
    let policy = channel.policy();
    let (property, array, initial_history, final_history, history_total, work_total, initial_work) =
        match channel {
            HeatWork::Plastic => (
                "membrane_plastic_heat_fraction",
                "membrane_plastic_dissipation_j",
                "initial_membrane_plasticity_history",
                "membrane_plasticity_history",
                "plastic_dissipation_j",
                "plastic_dissipation_j",
                "initial_plastic_dissipation_j",
            ),
            HeatWork::Relaxation => (
                "membrane_relaxation_heat_fraction",
                "membrane_relaxation_dissipation_j",
                "initial_membrane_history",
                "membrane_history",
                "viscous_step_work_j",
                "viscous_dissipation_j",
                "initial_viscous_dissipation_j",
            ),
            HeatWork::Bending => (
                "bending_plastic_heat_fraction",
                "bending_plastic_dissipation_j",
                "initial_bending_history",
                "bending_history",
                "plastic_dissipation_j",
                "plastic_dissipation_j",
                "initial_plastic_dissipation_j",
            ),
        };
    let fail = || {
        invalid(format!(
            "{consumer:?}: {policy} needs source-matched accepted work and complete supplied-energy accounting"
        ))
    };
    let frozen = alternative.frozen_changes.as_ref().ok_or_else(fail)?;
    let receipts = heat[policy]["material_fractions"]
        .as_array()
        .filter(|v| !v.is_empty())
        .ok_or_else(fail)?;
    let catalog = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    let mut laws = BTreeMap::new();
    for receipt in receipts {
        let identity = receipt["material_sha256"].as_str().ok_or_else(fail)?;
        let selection = frozen
            .pieces
            .values()
            .chain(frozen.default.iter())
            .find(|s| s.expected_sha256.as_deref() == Some(identity))
            .ok_or_else(fail)?;
        let material = catalog.resolve(selection)?;
        let (fraction, temperature_range_c, law) = match channel {
            HeatWork::Plastic => {
                let f = material
                    .membrane_plastic_heat_fraction()?
                    .ok_or_else(fail)?;
                (
                    f.fraction,
                    f.temperature_range_c,
                    material
                        .membrane_plasticity()?
                        .ok_or_else(fail)?
                        .fingerprint(),
                )
            }
            HeatWork::Relaxation => {
                let f = material
                    .membrane_relaxation_heat_fraction()?
                    .ok_or_else(fail)?;
                (
                    f.fraction,
                    f.temperature_range_c,
                    material
                        .membrane_relaxation()?
                        .ok_or_else(fail)?
                        .fingerprint()
                        .to_owned(),
                )
            }
            HeatWork::Bending => {
                let f = material.bending_plastic_heat_fraction()?.ok_or_else(fail)?;
                (
                    f.fraction,
                    f.temperature_range_c,
                    material
                        .bending_plasticity()?
                        .ok_or_else(fail)?
                        .fingerprint(),
                )
            }
        };
        let expected = json!({"material_sha256":material.sha256(),"law_sha256":law,
            "fraction":fraction,"temperature_range_c":temperature_range_c,
            "observation":material.authored().properties[property]});
        if receipt != &expected || laws.insert(law, fraction).is_some() {
            return Err(fail());
        }
    }
    let triangles = motion["triangles"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 65_536)
        .ok_or_else(fail)?;
    let values = |sample: &Value| -> Result<Vec<f64>, Refusal> {
        sample[array]
            .as_array()
            .filter(|v| v.len() == triangles.len())
            .ok_or_else(fail)?
            .iter()
            .map(|v| finite(v).filter(|&v| v >= 0.).ok_or_else(fail))
            .collect()
    };
    let mut previous = values(&motion["initial_sample"])?;
    let initial: f64 = previous.iter().sum();
    let near = heat_near;
    if finite(&motion["initial_sample"]["time_s"]) != Some(0.)
        || !finite(&motion[initial_history][history_total]).is_some_and(|v| near(v, initial))
    {
        return Err(fail());
    }
    let mut fractions = vec![None; triangles.len()];
    let mut bending_patches = vec![None; triangles.len()];
    let mut used = BTreeSet::new();
    let entries = if channel == HeatWork::Bending {
        "patches"
    } else {
        "elements"
    };
    for element in motion[initial_history][entries]
        .as_array()
        .ok_or_else(fail)?
    {
        let index = element["triangle"]
            .as_u64()
            .and_then(|i| usize::try_from(i).ok())
            .filter(|&i| i < triangles.len())
            .ok_or_else(fail)?;
        let law = element["law_sha256"].as_str().ok_or_else(fail)?;
        let fraction = *laws.get(law).ok_or_else(fail)?;
        if fractions[index].replace(fraction).is_some()
            || (channel == HeatWork::Plastic
                && (element["vertices"] != triangles[index]
                    || !finite(&element["plastic_dissipation_j"])
                        .is_some_and(|v| near(v, previous[index]))))
        {
            return Err(fail());
        }
        if channel == HeatWork::Bending {
            let vertices = element["vertices"]
                .as_array()
                .filter(|v| v.len() == 6)
                .ok_or_else(fail)?;
            if triangles[index].as_array().map(Vec::as_slice) != Some(&vertices[..3])
                || !finite(&element["plastic_dissipation_j"])
                    .is_some_and(|v| near(v, previous[index]))
                || !finite(&element["reference_area_m2"]).is_some_and(|v| v > 0.)
            {
                return Err(fail());
            }
            bending_patches[index] = Some(element);
        }
        used.insert(law);
    }
    if used.len() != laws.len()
        || fractions
            .iter()
            .zip(&previous)
            .any(|(f, &j)| f.is_none() && j != 0.)
    {
        return Err(fail());
    }
    let check = |row: &Value, dissipated: f64, supplied: f64| -> Result<(), Refusal> {
        let work = &row[policy];
        if work["distribution"] != "uniform_reference_thickness"
            || work["temporal_distribution"] != "uniform_over_accepted_interval"
        {
            return Err(fail());
        }
        for (name, expected) in [
            (work_total, dissipated),
            ("supplied_heat_j", supplied),
            ("unconverted_dissipation_j", dissipated - supplied),
            (initial_work, initial),
        ] {
            if !finite(&work[name]).is_some_and(|v| v >= 0. && near(v, expected)) {
                return Err(fail());
            }
        }
        Ok(())
    };
    let mut supplied_history = Vec::new();
    let mut dissipated = 0.;
    let mut supplied = 0.;
    for (sample, row) in motion["samples"]
        .as_array()
        .ok_or_else(fail)?
        .iter()
        .zip(heat["trace"].as_array().ok_or_else(fail)?)
    {
        let current = values(sample)?;
        let mut step_work = 0.;
        let mut step_heat = 0.;
        for ((&value, &old), fraction) in current.iter().zip(&previous).zip(&fractions) {
            if value < old || (fraction.is_none() && value != 0.) {
                return Err(fail());
            }
            step_work += value - old;
            step_heat += fraction.unwrap_or(0.) * (value - old);
        }
        dissipated += step_work;
        supplied += step_heat;
        check(row, dissipated, supplied)?;
        supplied_history.push(supplied);
        previous = current;
    }
    if !finite(&motion[final_history][history_total]).is_some_and(|v| near(v, initial + dissipated))
    {
        return Err(fail());
    }
    if channel == HeatWork::Bending {
        let mut seen = BTreeSet::new();
        for patch in motion[final_history]["patches"]
            .as_array()
            .ok_or_else(fail)?
        {
            let index = patch["triangle"]
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .filter(|&i| i < triangles.len())
                .ok_or_else(fail)?;
            let initial_patch = bending_patches[index].ok_or_else(fail)?;
            if !seen.insert(index)
                || patch["law_sha256"] != initial_patch["law_sha256"]
                || patch["vertices"] != initial_patch["vertices"]
                || patch["reference_area_m2"] != initial_patch["reference_area_m2"]
                || !finite(&patch["plastic_dissipation_j"])
                    .is_some_and(|v| near(v, previous[index]))
            {
                return Err(fail());
            }
        }
        if seen.len() != bending_patches.iter().flatten().count() {
            return Err(fail());
        }
    }
    check(heat, dissipated, supplied)?;
    supplied_history.push(supplied);
    Ok(supplied_history)
}
