// State is local to this tab. Epoch/request checks cover worker responses AND delayed file reads.
import { buildMaterialStudyRequest, buildMaterialMotionRequest, buildMaterialScenarioRequest, buildMaterialScenarioMatrixRequest,
  buildGuidedDesignAfterOperations, hasTextileWaterPolicy, buildMaterialSurfaceConditions,
  buildMaterialVaporSurfaceConditions } from "./materials-study-builder.js";
import { buildMaterialScreenRequest } from "./materials-screen-builder.js";

const $ = id => document.getElementById(id);
const commands = [...document.querySelectorAll("[data-command]")];
const boundOperations = ["exports", "fold", "heat", "vaporSurface", "coverage", "cut", "stock", "stockIdentities", "stockPrice", "cutJob", "price", "sew", "sewPreview"];
const operations = [...boundOperations, "screen", "study", "scenario", "pressure", "linearizeOrigin", "treatment", "vapor", "calibrate",
  "stripAcquisition", "stripExperiment", "stripHistory", "stripFit", "outfit", "thermalPoint", "thermalHistory", "garmentcode", "bodyCheck"];
const selectionOperations = ["inspect", "pressure", "linearizeOrigin", "calibrate", "stripFit"];
let worker, epoch = 0, sequence = 0, pending = null, ready = false;
let source = null, inspected = null, bound = null, drape = null;
let patternTransfer = null;
let preparedBy = null;
let originSource = null;
const originDomainFields = [["originWarpMin", "originWarpMax"], ["originWeftMin", "originWeftMax"], ["originShearMin", "originShearMax"]];
const originRationaleFields = ["originReference", "originAccessedOn", "originNote"];
let preparedTreatment = null;
let preparedImport = null, adoptedImport = false, selectedImportFit = null;
let sourceText = null, outfit = null;
let defaultChange = null;
let selectionRead = 0, patternRead = 0;
const sourceQuery = new URL(window.location.href).searchParams;
const traceImport = !sourceQuery.has("roundImport") && sourceQuery.has("traceImport");
let roundRead = 0, roundToken = sourceQuery.get(traceImport ? "traceImport" : "roundImport");
const roundFiles = new Map();
let photoEvidence = null, photoTransfer = null, photoProblem = null;
let interfaceChange = null, interfaceRead = 0, interfaceReady = true;
let calibratedPair = null;
let sewingPreviewSource = null;
let selectedStockRequest = null;
const pieceChanges = new Map(), urls = new Map();
const combinationGroups = new Map();
const studyCandidates = [];
const guidedScreenAttributes = [];
const screenProperties = {
  areal_density: "kg/m2", thickness: "m", width: "m", bulk_density: "kg/m3",
  thermal_resistance: "m2*K/W", thermal_conductivity: "W/(m*K)", specific_heat: "J/(kg*K)",
  vapor_resistance: "m2*Pa/W", warp_stiffness: "N/m", weft_stiffness: "N/m", shear_stiffness: "N/m",
  bending_rigidity: "N*m", friction_skin: "1", friction_self: "1",
};
const guidedDesignVariables = [];
const guidedDesignButtons = [];
const guidedDesignAfter = [], guidedDesignAfterButtons = [];
let guidedDesignAfterSource = null, guidedDesignAfterProblem = "";
let guidedDesignCurrent = 0;
let guidedDesignVertexSource = null, guidedDesignVertexProblem = "";
let guidedDesignNomination = null;
let guidedScenarioCases = null, guidedScenarioCasesRead = 0;
let guidedDesignCaseNomination = null;
const guidedScenarioDrafts = [], guidedScenarioButtons = [];
const guidedMotionKeyframes = [], guidedMotionButtons = [];
let guidedScenarioDraftSource = {};
const guidedDesignFields = { key: "guidedDesignKey", kind: "guidedDesignKind", axis: "guidedDesignAxis",
  at: "guidedDesignAt", multiplier: "guidedDesignMultiplier", values: "guidedDesignValues", dx: "guidedDesignDx", dy: "guidedDesignDy",
  lower: "guidedDesignLower", upper: "guidedDesignUpper", initial: "guidedDesignInitial",
  initialStep: "guidedDesignInitialStep", minimumStep: "guidedDesignMinimumStep" };
const guidedMetrics = [
  ["guidedArea", "stock_area_m2", "roll"], ["guidedStockMass", "stock_mass_kg", "roll"], ["guidedMass", "assembly_mass_kg"],
  ["guidedCutTime", "cutting_modeled_path_seconds", "roll"], ["guidedSewTime", "sewing_modeled_production_seconds"],
  ["guidedMakespan", "sewing_parallel_makespan_seconds"], ["guidedOrderCost", "selected_observed_order_cost", "roll"],
  ["guidedThreadLength", "thread_required_length_m"], ["guidedThreadCost", "thread_observed_order_cost"],
  ["guidedCombinedCost", "cut_and_sew_selected_order_cost", "priced"],
  ["guidedFiniteCount", "used_stock_count", "finite"], ["guidedFiniteArea", "used_stock_gross_area_m2", "finite"],
  ["guidedFiniteMass", "used_stock_mass_kg", "finite"], ["guidedFiniteTime", "finite_stock_modeled_path_seconds", "finite"],
  ["guidedJobArea", "cutting_job_stock_area_m2", "job"], ["guidedJobMass", "cutting_job_stock_mass_kg", "job"],
  ["guidedJobTime", "cutting_job_modeled_path_seconds", "job"], ["guidedJobCost", "cutting_job_selected_order_cost", "job"],
  ["guidedDrapeMass", "simulated_cloth_mass_kg"], ["guidedStrain", "max_absolute_edge_strain_pct"],
  ["guidedMotionStrain", "motion_max_absolute_edge_strain_pct"],
  ["guidedContactResultant", "body_contact_resultant_n"], ["guidedContactVertex", "max_body_contact_vertex_force_n"],
  ["guidedMotionContactResultant", "motion_peak_body_contact_resultant_n"],
  ["guidedMotionContactVertex", "motion_max_body_contact_vertex_force_n"],
  ["guidedHeatPower", "independent_surface_power_w"],
];
// The lower extrema are raised, upper extrema lowered. Limits stay signed Green strain.
const guidedDirectionalMetrics = [
  ["guidedWarpMin", "min_warp_green_strain", "maximize", "minimum"],
  ["guidedWarpMax", "max_warp_green_strain", "minimize", "maximum"],
  ["guidedWeftMin", "min_weft_green_strain", "maximize", "minimum"],
  ["guidedWeftMax", "max_weft_green_strain", "minimize", "maximum"],
  ["guidedShearMin", "min_engineering_shear_green_strain", "maximize", "minimum"],
  ["guidedShearMax", "max_engineering_shear_green_strain", "minimize", "maximum"],
  ["guidedMotionWarpMin", "motion_min_warp_green_strain", "maximize", "minimum"],
  ["guidedMotionWarpMax", "motion_max_warp_green_strain", "minimize", "maximum"],
  ["guidedMotionWeftMin", "motion_min_weft_green_strain", "maximize", "minimum"],
  ["guidedMotionWeftMax", "motion_max_weft_green_strain", "minimize", "maximum"],
  ["guidedMotionShearMin", "motion_min_engineering_shear_green_strain", "maximize", "minimum"],
  ["guidedMotionShearMax", "motion_max_engineering_shear_green_strain", "minimize", "maximum"],
];
const guidedTransportMetrics = [
  ["guidedCoveragePower", "body_coverage_power_w"],
  ["guidedVaporSurfaceHeat", "independent_vapor_potential_heat_flow_w"],
  ["guidedVaporSurfaceMass", "independent_vapor_potential_mass_flow_kg_s"],
  ["guidedVaporCoverageHeat", "covered_vapor_potential_heat_flow_w"],
  ["guidedVaporCoverageMass", "covered_vapor_potential_mass_flow_kg_s"],
  ["guidedMotionVaporEnergy", "motion_covered_vapor_potential_energy_j"],
  ["guidedMotionVaporMass", "motion_covered_vapor_potential_mass_kg"],
  ["guidedBodyWaterEvaporated", "motion_wet_film_evaporated_mass_kg"],
  ["guidedBodyWaterRemaining", "motion_wet_film_remaining_liquid_mass_kg"],
  ["guidedBodyWaterUnmet", "motion_wet_film_unmet_demand_mass_kg"],
  ["guidedBodyWaterSupplied", "motion_wet_film_supplied_mass_kg"],
  ["guidedBodyWaterLatent", "motion_wet_film_body_reservoir_latent_energy_j"],
  ["guidedTextileWaterEvaporated", "textile_water_evaporated_mass_kg"],
  ["guidedTextileWaterRemaining", "textile_water_remaining_mass_kg"],
  ["guidedTextileWaterSupplied", "textile_water_supplied_mass_kg"],
  ["guidedTextileWaterCondensed", "textile_water_condensed_mass_kg"],
  ["guidedTextileWaterDrained", "textile_water_drained_mass_kg"],
  ["guidedTextileWaterLatent", "textile_water_latent_energy_j"],
  ["guidedTextileWaterCondensationLatent", "textile_water_condensation_latent_energy_j"],
  ["guidedTransientSurfaceInsideEnergy", "transient_surface_inside_energy_j"],
  ["guidedTransientSurfaceNetEnergy", "transient_surface_net_boundary_energy_j"],
  ["guidedTransientSurfaceSampleMin", "transient_surface_min_sample_temperature_c"],
  ["guidedTransientSurfaceSampleMax", "transient_surface_max_sample_temperature_c"],
  ["guidedTransientSurfaceFinalMin", "transient_surface_final_min_temperature_c"],
  ["guidedTransientSurfaceFinalMax", "transient_surface_final_max_temperature_c"],
  ["guidedTransientCoverageInsideEnergy", "transient_coverage_inside_energy_j"],
  ["guidedTransientCoverageNetEnergy", "transient_coverage_net_boundary_energy_j"],
  ["guidedTransientCoverageSampleMin", "transient_coverage_min_sample_temperature_c"],
  ["guidedTransientCoverageSampleMax", "transient_coverage_max_sample_temperature_c"],
  ["guidedTransientCoverageFinalMin", "transient_coverage_final_min_temperature_c"],
  ["guidedTransientCoverageFinalMax", "transient_coverage_final_max_temperature_c"],
];
const guidedTransportSources = [
  ["coverage", "guidedCoverage", "guidedCoverageFile", "the original scenario body/layer heat policy"],
  ["vaporSurface", "guidedVaporSurface", "guidedVaporSurfaceFile", "the original independent-sheet vapor conditions"],
  ["vaporCoverage", "guidedVaporCoverage", "guidedVaporCoverageFile", "the original scenario covered-body vapor policy"],
];
const sheetConditionForms = {
  heatConditions: { operation: "heat", build: buildMaterialSurfaceConditions, fields: [
    ["heatInside", "inside_c", "inside reservoir temperature"], ["heatOutside", "outside_c", "outside reservoir temperature"],
    ["heatInsideFilm", "inside_resistance_m2_k_w", "inside heat film resistance"], ["heatOutsideFilm", "outside_resistance_m2_k_w", "outside heat film resistance"],
  ] },
  vaporSurfaceConditions: { operation: "vaporSurface", build: buildMaterialVaporSurfaceConditions, fields: [
    ["vaporInside", "inside_vapor_pressure_pa", "inside vapor pressure"], ["vaporOutside", "outside_vapor_pressure_pa", "outside vapor pressure"],
    ["vaporInsideFilm", "inside_film_resistance_m2_pa_w", "inside vapor film resistance"], ["vaporOutsideFilm", "outside_film_resistance_m2_pa_w", "outside vapor film resistance"],
    ["vaporLatentHeat", "latent_heat_j_kg", "latent heat"],
  ] },
};
const guidedRequired = [
  ["guidedRequireCutting", "cutting", "roll"], ["guidedRequireCutProgram", "cutting_program", "roll"],
  ["guidedRequireSewing", "sewing"], ["guidedRequirePurchasing", "purchasing", "roll"],
  ["guidedRequireThread", "sewing_consumables"], ["guidedRequireThreadPurchasing", "sewing_purchasing"],
  ["guidedRequireCombined", "cut_and_sew_purchasing", "priced"], ["guidedRequireMass", "assembly_mass"],
  ["guidedRequireFinite", "finite_stock", "finite"], ["guidedRequireFiniteProgram", "finite_stock_program", "finite"],
  ["guidedRequireJob", "cutting_job", "job"], ["guidedRequireJobProgram", "cutting_job_program", "job"],
  ["guidedRequireJobPurchasing", "cutting_job_purchasing", "job"],
  ["guidedRequireDrape", "drape"], ["guidedRequireSurface", "surface"],
  ["guidedRequireMaterialStrain", "material_strain"], ["guidedRequireMotion", "motion"],
  ["guidedRequireMotionMaterialStrain", "motion_material_strain"],
  ["guidedRequireContactLoads", "contact_loads"], ["guidedRequireMotionContactLoads", "motion_contact_loads"],
  ["guidedRequireCoverage", "coverage"], ["guidedRequireVaporSurface", "vapor_surface"],
  ["guidedRequireVaporCoverage", "vapor_coverage"], ["guidedRequireMotionVaporCoverage", "motion_vapor_coverage"],
  ["guidedRequireBodyWater", "motion_wet_film"],
  ["guidedRequireTransientSurface", "transient_surface"],
  ["guidedRequireTransientCoverage", "transient_coverage"],
];
let studyCandidateSequence = 0;
const outfitTargets = new Map();
const sections = new Map();
for (const op of operations) {
  const section = document.createElement("div"); section.hidden = true;
  const title = document.createElement("h3"); title.textContent = ({ screen: "Source-qualified screening",
    study: "Fixed-pattern manufacturing alternatives", scenario: "Fresh drape and heat alternatives",
    pressure: "Pressure-conditioned material", linearizeOrigin: "Bounded origin membrane approximation",
    treatment: "Laundering and dimensional treatment", vapor: "Potential evaporative transfer", calibrate: "Identified material behavior",
    thermalPoint: "Material at the stated temperature", thermalHistory: "Dry heating and cooling history",
    stripAcquisition: "Complete clamped-strip acquisitions", stripExperiment: "Clamped-strip experiment",
    stripHistory: "Strip loading and recovery", vaporSurface: "Vapor transfer on this drape",
    stripFit: "Clamped-strip identification", outfit: "Imported joint outfit", cutJob: "Complete mixed cutting job",
    stockIdentities: "Exact physical-stock identities", stockPrice: "Whole-item finite-stock prices", sewPreview: "Sewing operation preview",
    garmentcode: "Imported GarmentCode pattern — review required", bodyCheck: "Body target comparison" })[op] || op;
  const summary = document.createElement("pre"), links = document.createElement("div"); links.className = "links";
  const comparison = document.createElement("div"); comparison.className = "comparison";
  section.append(title, summary, comparison, links); $("outputs").append(section);
  sections.set(op, { section, summary, comparison, links, study: null, adoptions: [] });
}
const text = value => JSON.stringify(value, null, 2);
const has = id => (id === "stockRequest" && selectedStockRequest !== null) || roundFiles.has(id) || !!$(id).files.length;
const guidedStudyMode = () => $("studyMode").value === "guided";
const guidedScreenMode = () => $("screenMode").value === "guided";
const guidedScenarioMode = () => ["guided", "guided_matrix"].includes($("scenarioMode").value);
const guidedMatrixMode = () => $("scenarioMode").value === "guided_matrix";
const guidedMotionMode = () => $("guidedScenarioMotion").checked ? ($("guidedMotionSource").value || "form") : "none";
const sheetConditionsMode = id => $(id + "Source").value || "file";
const guidedVaporSurfaceMode = () => $("guidedVaporSurfaceSource").value || "file";
const guidedStockMode = () => $("guidedStockMode").value || "roll";
const guidedStockScope = (scope, mode) => !scope || scope === mode || (scope === "priced" && ["roll", "job"].includes(mode));
const studyUsesBody = () => guidedStudyMode() ? $("guidedDesign").checked : $("studyOnBody").checked;
const studyBodyMode = () => $("studyBodySource").value || "measurements";
function studyBodyReady() {
  const mode = studyBodyMode();
  if (mode === "accepted") return has("bodyFitFile") && $("bodyFitConsent").checked;
  if (mode === "request") return has("scenarioBody");
  return ["measurements", "preview"].includes(mode) && has("measurements");
}
function bodyDependentStudies(id) {
  const mode = studyBodyMode();
  const used = id === "body" || (id === "scenarioBody" && mode === "request") ||
    (id === "measurements" && ["measurements", "accepted", "preview"].includes(mode)) ||
    (["bodyArmDeg", "drapeLegs", "drapeLegFraction"].includes(id) && mode === "preview") ||
    (["bodyFitFile", "bodyFitConsent", "bodyArmDeg"].includes(id) && mode === "accepted");
  return used ? ["scenario", ...(studyUsesBody() ? ["study"] : [])] : [];
}
function guidedSourceStudies(id) {
  const scenarioSource = guidedScenarioMode() &&
    ((id === "fit" && $("guidedScenarioFitSource").value === "file") ||
      (id === "heatConditions" && $("guidedScenarioHeat").checked) ||
      (id === "vaporSurfaceConditions" && $("guidedVaporSurface").checked && guidedVaporSurfaceMode() === "workbench"));
  const scenarioKeys = scenarioSource ? [guidedMatrixMode() ? "scenarioDraft" : "scenario"] : [];
  if (!guidedStudyMode()) return scenarioKeys;
  const mode = guidedStockMode(), cutting = $("guidedCutting").checked;
  const used = (id === "fit" && $("guidedDesign").checked) ||
    (id === "cutOptions" && cutting && mode === "roll" && $("guidedCuttingSource").value === "file") ||
    (id === "stockRequest" && cutting && mode === "finite") || (id === "cutJobRequest" && cutting && mode === "job") ||
    (["sewOptions", "sewOptimize"].includes(id) && $("guidedSewing").checked) ||
    (["quotes", "market"].includes(id) && mode === "roll" && $("guidedPurchasing").checked);
  return used ? ["study", "scenario", ...scenarioKeys] : scenarioKeys;
}
function controls() {
  const guided = guidedStudyMode();
  for (const el of commands) el.disabled = !ready || !!pending;
  for (const section of sections.values()) {
    for (const button of section.adoptions) button.disabled = !ready || !!pending ||
      (typeof button.screenSelection === "string" && (!source || studyCandidates.length >= 7 || hasStudySelection(button.screenSelection)));
  }
  $("guidedScreen").hidden = !guidedScreenMode(); $("screenFileControls").hidden = guidedScreenMode();
  const customScreenProperty = $("guidedScreenProperty").value === "custom";
  $("guidedScreenCustom").hidden = !customScreenProperty;
  $("guidedScreenUnit").disabled = !customScreenProperty;
  if (!customScreenProperty) $("guidedScreenUnit").value = screenProperties[$("guidedScreenProperty").value] || "";
  $("addScreenAttribute").disabled = guidedScreenAttributes.length >= 64;
  $("clearScreenAttributes").disabled = !guidedScreenAttributes.length;
  $("bind").disabled ||= !source || !interfaceReady;
  $("openPatternWorkbench").disabled = !ready || !!pending || !bound;
  $("sew").textContent = $("sewOptimize").checked ? "Choose recipes & evaluate stitch paths" : "Evaluate sewing & stitch paths";
  $("assign").disabled = !!pending || !source || !inspected;
  $("reset").disabled = !!pending || !source;
  $("addCombination").disabled = !!pending || !source || !inspected;
  $("clearCombinations").disabled = !!pending || !combinationGroups.size;
  $("addStudyCandidate").disabled = !ready || !!pending || !source || !inspected || studyCandidates.length >= 7;
  $("clearStudyCandidates").disabled = !studyCandidates.length;
  $("guidedStudy").hidden = !guided;
  $("studyFileControls").hidden = guided;
  $("guidedScenario").hidden = !guidedScenarioMode();
  $("scenarioFileControls").hidden = guidedScenarioMode();
  $("guidedScenarioMatrix").hidden = !guidedMatrixMode();
  $("guidedScenarioCasePolicy").hidden = !guidedMatrixMode();
  $("guidedScenarioComparison").disabled = !guidedMatrixMode() || $("guidedScenarioPolicySource").value !== "file";
  $("addScenarioCase").disabled = !ready || !!pending || !source || !guidedMatrixMode() || guidedScenarioDrafts.length >= 8;
  $("clearScenarioCases").disabled = !guidedScenarioDrafts.length;
  for (const { button, unavailable, replace } of guidedScenarioButtons) button.disabled = unavailable || (replace && (!ready || !!pending));
  const motion = $("guidedScenarioMotion").checked, motionMode = guidedMotionMode();
  $("guidedMotionControls").hidden = !motion;
  $("guidedMotionLegFraction").disabled = $("guidedMotionLegs").value === "standing";
  $("addMotionKeyframe").disabled = !source || !guidedScenarioMode() || motionMode !== "form" || guidedMotionKeyframes.length >= 16;
  $("clearMotionKeyframes").disabled = motionMode !== "form" || !guidedMotionKeyframes.length;
  for (const button of guidedMotionButtons) button.disabled = !source || !guidedScenarioMode() || motionMode !== "form";
  $("addScenarioCase").disabled ||= motionMode === "form" && !guidedMotionKeyframes.length;
  $("guidedScenarioGoals").hidden = !guided;
  $("guidedMaterialStrainControls").hidden = !$("guidedMaterialStrain").checked;
  $("guidedMaterialStrainInstances").disabled = !$("guidedMaterialStrain").checked || $("guidedMaterialStrainScope").value !== "instances";
  const contactLoads = $("guidedContactLoads").checked;
  $("guidedContactLoadsControls").hidden = !contactLoads;
  for (const id of ["guidedContactLoadsScope", "guidedContactVertexBudget", "guidedContactPairBudget"]) $(id).disabled = !contactLoads;
  $("guidedContactLoadsInstances").disabled = !contactLoads || $("guidedContactLoadsScope").value !== "instances";
  for (const [, id, file] of guidedTransportSources) $(file).disabled = !$(id).checked;
  $("guidedVaporSurfaceSource").disabled = !$("guidedVaporSurface").checked;
  $("guidedVaporSurfaceFileField").hidden = guidedVaporSurfaceMode() !== "file";
  $("guidedVaporSurfaceFile").disabled ||= guidedVaporSurfaceMode() !== "file";
  $("guidedMotionForm").hidden = motionMode !== "form";
  $("guidedMotionFileField").hidden = motionMode !== "file";
  const motionHeat = motionMode === "form" && $("guidedMotionHeat").checked;
  $("guidedMotionHeatSettings").hidden = !motionHeat; $("guidedMotionHeatBoundary").hidden = !motionHeat;
  for (const id of ["guidedMotionHeatInitial", "guidedMotionHeatCells", "guidedMotionHeatStep", "guidedMotionHeatBudget",
    "guidedMotionHeatInside", "guidedMotionHeatOutside", "guidedMotionHeatInsideFilm", "guidedMotionHeatOutsideFilm"]) $(id).disabled = !motionHeat;
  $("studyPieces").disabled = !source || $("studyTarget").value !== "pieces";
  $("guidedDesign").disabled = !source;
  $("guidedDesignControls").hidden = !$("guidedDesign").checked;
  for (const id of ["guidedDesignPieces", "guidedDesignVertices", ...Object.values(guidedDesignFields), "guidedDesignMax", "guidedDesignGridBudget",
    "guidedDesignMode", "guidedDesignObjective", "guidedDesignMaterial", "guidedDesignCase", "guidedDesignEvaluations"]) {
    $(id).disabled = !source || !$("guidedDesign").checked;
  }
  const adaptiveDesign = $("guidedDesignMode").value === "adaptive";
  $("guidedDesignGridDomain").hidden = adaptiveDesign; $("guidedDesignAdaptiveDomain").hidden = !adaptiveDesign;
  $("guidedDesignGridBudgetField").hidden = adaptiveDesign; $("guidedDesignGridNote").hidden = adaptiveDesign;
  $("guidedDesignAdaptivePolicy").hidden = !adaptiveDesign;
  for (const id of ["guidedDesignValues", "guidedDesignGridBudget"]) $(id).disabled ||= adaptiveDesign;
  for (const id of ["guidedDesignLower", "guidedDesignUpper", "guidedDesignInitial", "guidedDesignInitialStep", "guidedDesignMinimumStep",
    "guidedDesignObjective", "guidedDesignMaterial", "guidedDesignCase", "guidedDesignEvaluations"]) $(id).disabled ||= !adaptiveDesign;
  const objective = $("guidedDesignObjective").value;
  const consumerDesign = ["consumer_comparison", "consumer_pareto", "consumer_worst_case", "consumer_matrix_pareto"].includes(objective);
  $("guidedDesignMaterialField").hidden = !consumerDesign; $("guidedDesignMaterial").disabled ||= !consumerDesign;
  $("guidedDesignMaterialStatus").hidden = !consumerDesign || !guidedDesignNomination?.invalidated;
  const nominatedCase = ["consumer_comparison", "consumer_pareto"].includes(objective) && (!guidedScenarioMode() || guidedMatrixMode());
  $("guidedDesignCaseField").hidden = !nominatedCase;
  $("guidedDesignCaseStatus").hidden = !nominatedCase;
  $("guidedDesignCase").disabled ||= !nominatedCase || !currentGuidedScenarioCases()?.keys?.length;
  const movingVertices = $("guidedDesignKind").value === "move";
  if (source && $("guidedDesign").checked && movingVertices) ensureGuidedDesignVertices();
  $("guidedDesignSlash").hidden = movingVertices; $("guidedDesignMove").hidden = !movingVertices;
  for (const id of ["guidedDesignPieces", "guidedDesignAxis", "guidedDesignAt", "guidedDesignMultiplier"]) $(id).disabled ||= movingVertices;
  for (const id of ["guidedDesignVertices", "guidedDesignDx", "guidedDesignDy"]) $(id).disabled ||= !movingVertices;
  $("guidedDesignVertices").disabled ||= !!guidedDesignVertexProblem;
  $("guidedDesignAdd").disabled = !source || !$("guidedDesign").checked || guidedDesignVariables.length >= 3;
  for (const { button, unavailable } of guidedDesignButtons) button.disabled = !source || !$("guidedDesign").checked || unavailable;
  const afterActive = !!source && guided && $("guidedDesign").checked;
  if (afterActive) ensureGuidedDesignAfterSources();
  const capEase = $("guidedDesignAfterKind").value === "cap_ease";
  $("guidedDesignAfterTrueFields").hidden = capEase;
  $("guidedDesignAfterCapFields").hidden = !capEase;
  for (const id of ["guidedDesignAfterKind", "guidedDesignAfterSeam", "guidedDesignAfterEnd", "guidedDesignAfterPiece",
    "guidedDesignAfterSeams", "guidedDesignAfterRatio"]) $(id).disabled = !afterActive || !!guidedDesignAfterProblem;
  $("guidedDesignAfterAdd").disabled = !afterActive || !!guidedDesignAfterProblem || guidedDesignAfter.length >= 16;
  $("guidedDesignAfterClear").disabled = !guidedDesignAfter.length;
  for (const { button, unavailable } of guidedDesignAfterButtons) button.disabled = !afterActive || unavailable;
  const stockMode = guidedStockMode(), cutting = $("guidedCutting").checked;
  for (const [mode, prefix] of [["roll", "guidedRoll"], ["finite", "guidedFinite"], ["job", "guidedJob"]]) {
    for (const suffix of ["Sources", "Goals", "Required"]) $(prefix + suffix).hidden = stockMode !== mode;
  }
  $("guidedRollPurchasing").hidden = stockMode !== "roll";
  $("guidedPurchasing").disabled = stockMode !== "roll";
  for (const id of ["guidedCombinedPurchasing", "guidedCombinedGoals", "guidedCombinedRequired"]) $(id).hidden = stockMode === "finite";
  $("guidedCombinedCost").disabled = stockMode === "finite";
  for (const [prefix, , scope] of guidedMetrics) {
    if (!scope) continue;
    const active = guidedStockScope(scope, stockMode);
    $(`${prefix}Goal`).disabled = !active || (scope !== "priced" && prefix !== "guidedOrderCost" && !cutting);
    $(`${prefix}Limit`).disabled = !active;
  }
  for (const [id, , scope] of guidedRequired) if (scope) $(id).disabled = !guidedStockScope(scope, stockMode);
  const rollFile = $("guidedCuttingSource").value === "file";
  $("guidedCuttingSource").disabled = !cutting || stockMode !== "roll";
  $("guidedRollForm").hidden = rollFile;
  for (const id of ["guidedCopies", "guidedGap", "guidedEdge", "guidedEnd"]) $(id).disabled = !cutting || stockMode !== "roll" || rollFile;
  $("guidedCutTimeGoal").disabled ||= !rollFile;
  for (const id of ["guidedSewTimeGoal", "guidedMakespanGoal", "guidedThreadLengthGoal", "guidedThreadCostGoal"]) $(id).disabled = !$("guidedSewing").checked;
  $("guidedOrderCostGoal").disabled ||= !$("guidedPurchasing").checked;
  $("guidedCombinedCostGoal").disabled ||= !$("guidedCombinedCost").checked;
  $("guidedInventory").disabled = !$("guidedMass").checked;
  $("clearGuidedInventory").disabled = !has("guidedInventory");
  $("guidedInventoryComplete").disabled = !$("guidedMass").checked || has("guidedInventory");
  $("guidedMassGoal").disabled = !$("guidedMass").checked;
  $("studyOnBody").disabled = guided;
  $("clearInterfaces").disabled = interfaceChange === null && interfaceReady;
  for (const op of boundOperations) $(op).disabled ||= !bound;
  $("screen").disabled ||= guidedScreenMode() ? !$("guidedScreenBasis").value : !has("screenRequest");
  $("linearizeOrigin").disabled ||= !originSource?.allowEstimates || !$("originEstimates").checked ||
    [...originDomainFields.flat(), ...originRationaleFields].some(id => !$(id).value.trim());
  $("pressure").disabled ||= !has("compressionRequest");
  $("treatment").disabled ||= !has("treatmentRequest");
  $("useTreatment").disabled = !ready || !!pending || !preparedTreatment;
  $("garmentcode").disabled ||= !has("garmentcodeSource");
  $("useGarmentcode").disabled = !!pending || !preparedImport;
  $("useGarmentcodeFit").disabled = !!pending || typeof preparedImport?.fit_spec_json !== "string";
  $("clearGarmentcodeAnnotations").disabled = !has("garmentcodeAnnotations");
  $("garmentcodeArcs").disabled = has("garmentcodeAnnotations");
  for (const id of ["garmentcodeArcSagitta", "garmentcodeArcLength"]) {
    $(id).disabled = has("garmentcodeAnnotations") || !$("garmentcodeArcs").checked;
  }
  $("vapor").disabled ||= !has("vaporRequest");
  $("thermalPoint").disabled ||= !$("selection").value.trim() || !$("thermalPointTemperature").value.trim() ||
    !Number.isFinite(Number($("thermalPointTemperature").value));
  $("thermalHistory").disabled ||= !has("thermalRequest");
  $("calibrate").disabled ||= !has("calibrationRequest");
  $("stageInterface").disabled ||= !source || !interfaceReady || !calibratedPair;
  $("replaceCalibratedInterface").disabled = !!pending || !calibratedPair;
  for (const op of ["stripAcquisition", "stripExperiment", "stripHistory", "stripFit"]) $(op).disabled ||= !has(`${op}Request`);
  const studyReady = guided ? studyCandidates.length > 0 && (!$("guidedDesign").checked || has("fit")) : has("studyRequest");
  $("study").disabled ||= !source || !studyReady;
  if (studyUsesBody()) $("study").disabled ||= !has("body") || !studyBodyReady();
  $("scenario").disabled ||= !source || !studyReady || (!guidedScenarioMode() && !has("scenarioRequest")) || !has("body") || !studyBodyReady();
  $("scenario").disabled ||= guidedMatrixMode() && !guidedScenarioDrafts.length;
  $("scenario").disabled ||= guidedScenarioMode() && !guidedMatrixMode() && motionMode === "form" && !guidedMotionKeyframes.length;
  if (!guided && $("combinationMode").checked && !combinationGroups.size) {
    $("study").disabled = true; $("scenario").disabled = true;
  }
  $("exports").disabled ||= !has("measurements") || !has("fit");
  $("bodyCheck").disabled ||= !has("body") || (!has("measurements") && !has("bodyFitFile")) ||
    (has("bodyFitFile") && !$("bodyFitConsent").checked);
  $("fold").disabled ||= !has("measurements") || !has("fit") || !has("body");
  $("fold").disabled ||= has("bodyFitFile") && !$("bodyFitConsent").checked;
  $("clearBodyFit").disabled = !has("bodyFitFile");
  $("selfContact").disabled = !!outfit;
  $("cut").disabled ||= !has("cutOptions");
  $("stock").disabled ||= !has("stockRequest");
  $("stockIdentities").disabled ||= !has("stockRequest");
  $("stockPrice").disabled ||= !has("stockRequest") || !has("stockPricing");
  $("clearStockRequest").disabled = !has("stockRequest");
  $("cutJob").disabled ||= !has("cutJobRequest");
  $("price").disabled ||= !has("cutOptions") || !has("quotes") || !has("market");
  $("sew").disabled ||= !has("sewOptions");
  $("sewPreview").disabled ||= !currentSewingPreviewOperation();
  const previewBlocked = !!pending && pending.op !== "sewPreview";
  $("sewPreviewCell").disabled = !sewingPreviewSource?.requiresCell || previewBlocked;
  $("sewPreviewOperation").disabled = !currentSewingPreviewCell() || previewBlocked;
  for (const [id, form] of Object.entries(sheetConditionForms)) {
    const authored = sheetConditionsMode(id) === "form";
    $(id + "Form").hidden = !authored; $(id + "FileField").hidden = authored; $(id).disabled = authored;
    for (const [field] of form.fields) $(field).disabled = !authored;
    $(form.operation).disabled ||= !drape || (!authored && !has(id));
  }
  $("coverage").disabled ||= !drape || !has("map") || !has("coverageConditions");
  $("clearCoverageBridges").disabled = !has("coverageBridges");
  $("cancel").disabled = !pending;
}
function clear(keys) {
  if (keys.includes("bind") && !keys.includes("treatment")) keys = [...keys, "treatment"];
  if (keys.includes("sew") && !keys.includes("sewPreview")) keys = [...keys, "sewPreview"];
  for (const key of keys) {
    for (const url of urls.get(key) || []) URL.revokeObjectURL(url);
    urls.delete(key);
    if (sections.has(key)) {
      const s = sections.get(key); s.section.hidden = true; s.links.replaceChildren();
      s.comparison.replaceChildren(); s.summary.textContent = ""; s.study = null; s.adoptions = [];
    }
    if (key === "fold") { drape = null; $("viewer").hidden = true; $("viewer").removeAttribute("src"); }
    if (key === "cut") { $("marker").hidden = true; $("marker").removeAttribute("src"); }
    if (key === "stock") { $("stockMarker").hidden = true; $("stockMarker").removeAttribute("src"); }
    if (key === "sewPreview") { $("sewingPreview").hidden = true; $("sewingPreview").removeAttribute("src"); }
    if (key === "sew") {
      sewingPreviewSource = null; $("sewPreviewControls").hidden = true;
      $("sewPreviewCell").replaceChildren(); $("sewPreviewOperation").replaceChildren();
      $("sewPreviewSummary").textContent = "";
    }
    if (key === "bind") {
      if (patternTransfer) patternTransfer.pattern = null;
      bound = null; $("boundSummary").textContent = "No current bound pattern."; $("bindFiles").replaceChildren();
    }
    if (key === "treatment") preparedTreatment = null;
    if (key === "garmentcode") { preparedImport = null; $("useGarmentcodeFit").checked = false; }
    if (key === "calibrate") {
      calibratedPair = null;
      $("calibratedInterfaceSummary").textContent = "No current calibrated contact pair.";
      $("replaceCalibratedInterface").checked = false;
    }
  }
}
function error(message) { $("error").textContent = message; $("error").hidden = false; }
function invalidateThermalPoint() {
  if (pending?.op === "thermalPoint") start();
  clear(["thermalPoint"]); controls();
}
function start() {
  if (worker) worker.terminate();
  const generation = ++epoch;
  ready = false; pending = null;
  $("status").textContent = "Loading local material engine…"; controls();
  try { worker = new Worker(new URL("./materials-worker.js", import.meta.url), { type: "module" }); }
  catch (e) { error(String(e.message || e)); return; }
  worker.onmessage = ({ data: msg }) => {
    if (generation !== epoch) return;
    if (msg.type === "ready") { ready = true; $("status").textContent = "Ready. Inspect a material or load a pattern."; controls(); return; }
    if (msg.type === "loadError") { ready = false; error(`Cannot load web/pkg: ${msg.message}. Build the browser bundle first.`); controls(); return; }
    if (msg.request !== pending?.id) return;
    if (msg.type === "progress") { $("progress").textContent = msg.text; return; }
    const request = pending, op = request.op; pending = null;
    if (msg.type === "error") {
      error(msg.message); $("status").textContent = `${op} refused; no current result published.`;
      if (msg.fatal) { ready = false; error(`${msg.message}. Reload this page to restart the engine.`); }
    } else if (msg.type === "result") {
      show(op, msg, request); $("status").textContent = `${op} complete. Results describe only the retained inputs.`;
    }
    $("progress").textContent = ""; controls();
  };
  worker.onerror = e => {
    if (generation !== epoch) return;
    pending = null; ready = false; clear(["bind", ...operations]);
    error(e.message || "The worker failed; reload the page."); controls();
  };
}
function invalidate(keys, dependentOnly = false) {
  if (keys.includes("sew") && !keys.includes("sewPreview")) keys = [...keys, "sewPreview"];
  if (pending && (!dependentOnly || keys.includes(pending.op))) start(); // terminate affected Rust work; never wait for its stale success
  clear(keys); $("error").hidden = true; $("progress").textContent = ""; controls();
}
function selectStockRequest(raw) {
  selectedStockRequest = raw; $("stockRequest").value = "";
  clear(["selectedStock"]); $("selectedStockFiles").replaceChildren();
  $("selectedStockNote").hidden = raw === null;
  if (raw !== null) link("selectedStock", $("selectedStockFiles"),
    { name: "selected-stock-request.json", text: raw, mime: "application/json" });
  invalidate(["stock", "stockIdentities", "stockPrice", ...guidedSourceStudies("stockRequest")], true);
}
function link(key, parent, f) {
  const url = URL.createObjectURL(new Blob([f.bytes || f.text], { type: f.mime }));
  if (!urls.has(key)) urls.set(key, []); urls.get(key).push(url);
  const a = document.createElement("a"); a.href = url; a.download = f.name; a.textContent = f.name; parent.append(a);
  return url;
}
// These are displays of retained production outcomes, not a second scorer or physics engine.
const available = outcome => outcome?.status === "available" ? outcome.result : null;
const state = outcome => outcome?.status === "available" ? "Available" :
  outcome?.status === "not_requested" ? "Not requested" : outcome?.status === "refused" ?
  `Refused: ${outcome.refusal?.message || "See report"}` : "Unavailable";
const metric = value => typeof value === "number" && Number.isFinite(value) ? String(value) : "Unknown";
function bodyCheckResult(parent, report) {
  const body = report.body, imported = body.shape_source?.kind === "accepted_multi_frame_fit";
  const note = document.createElement("p");
  note.textContent = (imported ? "Accepted image-fit shape reused without refitting. " : "Body fitted to the supplied targets. ") +
    "Predictions are MODEL_FIT. Targets may include priors. No cloth was simulated; these differences do not change measurement readiness or establish physical fit.";
  parent.append(note);
  const rows = imported ? body.measurement_comparison || [] :
    Object.entries(body.fit?.tape || {}).map(([name, row]) => ({ name, ...row }));
  const t = table(parent, "Model predictions compared with supplied targets", ["Target", "Target mm", "Body mm (MODEL_FIT)",
    "Body − target mm", "Supplied σ mm", ...(imported ? [] : ["Fit scale mm", "Residual / fit scale"])]);
  const signed = value => typeof value === "number" && Number.isFinite(value) ? (value > 0 ? "+" : "") + value : "Unavailable";
  for (const row of rows) cells(t, [row.name, metric(row.target_mm), metric(row.body_mm), signed(row.residual_mm),
    row.sigma_mm == null ? "Not supplied" : metric(row.sigma_mm),
    ...(imported ? [] : [metric(row.effective_sigma_mm), signed(row.normalized_residual)])]);
  if (!rows.length) { const empty = document.createElement("p"); empty.textContent = "No supplied tape targets to compare."; parent.append(empty); }
  const unavailable = body.tape_evaluation?.unavailable_fields;
  if (Array.isArray(unavailable) && unavailable.length) {
    const missing = document.createElement("p"); missing.textContent = "Unavailable model predictions: " + unavailable.join(", ") + ". No missing section is inferred."; parent.append(missing);
  }
  if (body.tape_evaluation?.status === "UNAVAILABLE") {
    const failed = document.createElement("p"); failed.textContent = "Model tape evaluation unavailable: " +
      (body.tape_evaluation.reason || "See the original report."); parent.append(failed);
  }
  if (!imported) {
    const scale = document.createElement("p"); scale.textContent = "Fit scale is the larger of supplied uncertainty and the existing fitting tolerance. Scaled residuals are diagnostics, not confidence intervals."; parent.append(scale);
  }
  return { body_source: imported ? "Accepted shape, no refit" : "Tape-fitted MODEL_FIT shape",
    model_mass_kg: body.mass_kg, fit_iterations: body.fit?.iterations, cloth_simulated: report.cloth_simulated };
}
const vaporMetric = (result, name) => metric(result?.[name]) +
  (result?.reported_bounds?.[name] ? `; reported range [${result.reported_bounds[name].map(metric).join(", ")}]` : "");
function vaporBoundsNote(parent, result, label = "Vapor source bounds") {
  if (!result || !Object.hasOwn(result, "reported_bounds")) return;
  const note = document.createElement("p");
  note.textContent = `${label}: ` + (result.reported_bounds ?
    "complete reported resistance endpoints propagated through the fixed geometry, pressures, films, interfaces and latent-heat conversion. " +
    "These conditional ranges are not confidence intervals or uncertain-geometry, moisture or empirical garment predictions." :
    `unavailable${result.bounds_unavailable?.length ? ` — ${result.bounds_unavailable.join("; ")}` : ""}. ` +
    "Nominal results remain visible; missing bounds are not replaced with an exact range.");
  parent.append(note);
}
function table(parent, caption, headings) {
  const t = document.createElement("table"), c = document.createElement("caption"), head = document.createElement("tr");
  c.textContent = caption; t.append(c);
  for (const label of headings) { const th = document.createElement("th"); th.scope = "col"; th.textContent = label; head.append(th); }
  t.append(head); parent.append(t); return t;
}
function cells(t, values) {
  const row = document.createElement("tr");
  for (const value of values) { const td = document.createElement("td"); td.textContent = value; row.append(td); }
  t.append(row);
}
function detail(parent, title, value) {
  const d = document.createElement("details"), s = document.createElement("summary"), pre = document.createElement("pre");
  s.textContent = title; d.append(s, pre); parent.append(d);
  // Large material snapshots are formatted only when inspected. Downloaded reports stay raw.
  let shown = false;
  d.addEventListener("toggle", () => { if (d.open && !shown) { pre.textContent = text(value); shown = true; } });
}
function stockIdentitiesResult(parent, identities) {
  const t = table(parent, "Physical stock for exact offer binding",
    ["Inventory item", "Stock ID", "Used by this layout", "Gross area (m²)", "Stock SHA-256", "Material SHA-256"]);
  for (const row of identities) cells(t, [String(row.stock_index + 1), row.stock_id,
    row.used ? "Used" : "Unused", metric(row.gross_area_m2), row.stock_sha256, row.material_sha256]);
  return { stock_items: identities.length, scope: "Exact inventory identities; no price, reservation or metrology claim" };
}
function stockPriceResult(parent, r) {
  const total = table(parent, "Complete finite-stock order",
    ["Complete offer coverage", "Observed capacity coverage", `Complete whole-item total (${r.currency})`]);
  cells(total, [r.complete_offer_coverage ? "Complete" : "Incomplete; required prices unknown",
    r.complete_observed_quantity_coverage ? "Observed sufficient" : "Not fully observed",
    metric(r.least_observed_independent_order_total)]);
  stockIdentitiesResult(parent, r.stocks.map(row => row.identity));
  const t = table(parent, "Whole-item prices for the current finite-stock layout",
    ["Physical stock", "Required by layout", "Chosen offer", `Whole-item cost (${r.currency})`, "Binding diagnostic"]);
  for (const row of r.stocks) {
    cells(t, [row.identity.stock_id, row.identity.used ? "Yes" : "No",
      row.least_observed_offer_id || (row.identity.used ? "No admitted offer" : "Unused; no price required"),
      metric(row.least_observed_order_cost), row.binding_refusal?.message || "None"]);
    const offers = table(parent, `${row.identity.stock_id}: dated whole-item offers`,
      ["Offer", "Observation", "Status", "Order quantity", `Whole-item cost (${r.currency})`, "Observed capacity"]);
    for (const offer of row.offers) cells(offers, [offer.quote.offer_id,
      offer.quote.observed_at || `${offer.quote.observed_on} (date only)`,
      offer.quote.offer_id === row.least_observed_offer_id ? "Selected" : "Eligible alternative",
      `${metric(offer.order_quantity)} ${offer.quote.unit}`, metric(offer.order_cost),
      offer.quantity_sufficiency_observed ? "Observed sufficient" : "Not fully observed"]);
    for (const excluded of [...row.source_exclusions, ...row.stock_exclusions]) cells(offers, [excluded.quote.offer_id,
      excluded.quote.observed_at || `${excluded.quote.observed_on} (date only)`, `Excluded: ${excluded.reason}`,
      "Not ordered", "Unknown", "Not admitted"]);
    for (const source of row.source_refusals) cells(offers, [source.offer_id, "Unknown",
      `Refused: ${source.refusal.message}`, "Not ordered", "Unknown", "Not admitted"]);
    detail(parent, `${row.identity.stock_id}: original offers, source exclusions and binding diagnostics`, row);
  }
  detail(parent, "Comparable market and original pricing request", r.request);
  const note = document.createElement("p");
  const market = r.request.market;
  note.textContent = `Market: ${r.currency}; ${market.region}; ${market.quality}; ${market.terms}. Evaluated at ${market.as_of_at || market.as_of} UTC. ` +
    "Only used physical items contribute to the complete total, once per item. An unknown required price keeps the total unknown. " +
    "Current pricing does not change the stock request's layout objective or prove the cheapest layout. " +
    "Dates, capacity, source exclusions and whole-item terms come from the shared quote engine. No reservation, checkout or hardware execution occurs.";
  parent.append(note);
  return { schema: r.schema, complete_offer_coverage: r.complete_offer_coverage,
    complete_observed_quantity_coverage: r.complete_observed_quantity_coverage,
    currency: r.currency, least_observed_independent_order_total: r.least_observed_independent_order_total,
    execution_authorized: r.execution_authorized };
}
function procurementResult(parent, r) {
  if (r.schema === "ftx-cut-procurement/3") return splitProcurementResult(parent, r);
  const grouped = Array.isArray(r.material_orders);
  for (const row of (grouped ? r.material_orders : r.roll_prices) || []) {
    const sample = row.sample, scope = sample.scope;
    const label = grouped ? `Material order ${row.material_order_index + 1}` : `Roll ${row.roll_index + 1}`;
    const note = document.createElement("p");
    note.textContent = `${label}: ${metric(row.required_stock_area_m2)} m² of stock. ` +
      (grouped ? `Markers ${row.roll_indices.map(i => i + 1).join(", ")} require ${row.marker_lengths_m.map(metric).join(", ")} m respectively; ` +
        `total ${metric(row.required_length_m)} m, longest continuous segment ${metric(row.required_continuous_length_m)} m. ` +
        "Minimum quantity, order increments and available stock apply once to the combined material order. " :
        (row.required_length_m == null ? "" : `The marker requires one continuous ${metric(row.required_length_m)} m cut. `)) +
      `Evaluated at ${scope.as_of_at || `${scope.as_of} (date only)`} UTC. ` +
      `Market: ${scope.currency}; ${scope.region}; ${scope.quality}; ${scope.terms}. ` +
      "The report retains the latest observed offer and its exclusion; an older price is not substituted. " +
      "A source continuous-cut limit is an upper limit, not a guarantee of continuous inventory.";
    parent.append(note);
    const entries = [...sample.offers.map(offer => ({ offer, quote: offer.quote })),
      ...(sample.excluded_offers || []).map(excluded => ({ excluded, quote: excluded.quote }))];
    const hasPriceBreaks = entries.some(({ quote, offer }) => quote.price_breaks?.length || offer?.applied_price_break);
    const t = table(parent, `${label} — latest supplier observations`,
      ["Offer", "Observed (UTC)", "Valid through (UTC)", "Admission", "Order quantity", `Merchandise cost (${scope.currency})`, "Source continuous-cut limit",
        ...(hasPriceBreaks ? ["Applied source price"] : [])]);
    for (const { quote: q, offer, excluded } of entries) {
      const expiry = [q.valid_through_at, q.valid_through ? `${q.valid_through} (date limit)` : null].filter(Boolean).join("; ");
      const quantity = offer?.order_quantity ?? excluded?.required_order_quantity;
      const continuity = q.maximum_continuous_quantity == null ? "Not reported" :
        `${metric(q.maximum_continuous_quantity)} ${q.unit}` +
        (offer?.within_observed_continuous_limit === true ? "; within observed limit" : "");
      const tier = offer?.applied_price_break;
      const appliedPrice = excluded ? "Not applied: excluded" : tier ?
        `From ${metric(tier.minimum_quantity)} ${q.unit}: ${metric(tier.price)} ${scope.currency} per ${metric(q.quantity)} ${q.unit}` +
          (tier.sku == null ? "" : `; SKU ${tier.sku}`) : "Base quote";
      cells(t, [q.offer_id, q.observed_at || `${q.observed_on} (date only)`, expiry || "Not reported",
        excluded ? `Excluded: ${excluded.reason.replaceAll("_", " ")}` : "Eligible",
        typeof quantity === "number" ? `${metric(quantity)} ${q.unit}` : "Unknown", metric(offer?.order_cost), continuity,
        ...(hasPriceBreaks ? [appliedPrice] : [])]);
    }
    if (!entries.length) cells(t, ["No observation in the requested market at this time", "Unknown", "Unknown", "No eligible offer", "Unknown", "Unknown", "Not reported"]);
    detail(parent, `${label}: selected order, source evidence and stock exclusions`, row);
  }
  return { complete_offer_coverage: r.complete_offer_coverage,
    complete_observed_quantity_coverage: r.complete_observed_quantity_coverage, currency: r.currency,
    complete_continuous_length_limit_coverage: r.complete_continuous_length_limit_coverage,
    least_observed_independent_order_total: r.least_observed_independent_order_total, limits: r.limits };
}
function splitProcurementResult(parent, r, label = "Whole-marker supplier allocation") {
  const summary = table(parent, label, ["Complete offer coverage", "Selected complete cost", "Proven least cost",
    "Search complete", "Budget exhausted", "Optimality proven", "Search nodes", "Quote evaluations"]);
  const cost = value => typeof value === "number" ? `${metric(value)} ${r.currency}` : "Unknown";
  cells(summary, [r.complete_offer_coverage ? "Yes" : "No complete allocation found",
    cost(r.selected_observed_order_total), cost(r.least_observed_independent_order_total),
    String(r.search_complete), String(r.budget_exhausted), String(r.order_allocation_optimality_proven),
    metric(r.search_nodes), metric(r.quote_evaluations)]);
  const note = document.createElement("p");
  note.textContent = "Each marker stays whole and is assigned once. Minimum quantities, increments and observed capacity apply once to each selected supplier order. " +
    "A complete feasible allocation remains usable when the work limit prevents proof of the least cost; no partial allocation is priced as a complete job. " +
    "Only source-locked offers for the exact material revision are considered. Requested cuts and dated stock observations do not reserve stock or guarantee fulfillment.";
  parent.append(note);
  for (const row of r.material_orders || []) {
    const allocation = row.allocation, selected = allocation?.selected_orders;
    const title = `${label}: material order ${row.material_order_index + 1}`;
    const t = table(parent, title, ["Supplier / offer", "Assigned markers", "Required stock (m²)", "Rounded order quantity",
      `Selected cost (${r.currency})`, "Observed available quantity", "Longest marker (m)", "Continuous-cut limit", "Order SKU / price tier"]);
    for (const order of selected || []) {
      const offer = order.offer, q = offer.quote;
      const tier = offer.applied_price_break;
      const orderSku = (tier?.sku ?? q.sku ?? "Not reported") +
        (tier ? `; tier from ${metric(tier.minimum_quantity)} ${q.unit}` : "");
      const continuity = q.maximum_continuous_quantity == null ? "Not reported" :
        `${metric(q.maximum_continuous_quantity)} ${q.unit}` +
        (offer.within_observed_continuous_limit === true ? "; within observed limit" : "");
      cells(t, [`${q.supplier || "Supplier not named"} / ${q.offer_id}`, order.roll_indices.map(i => i + 1).join(", "),
        metric(order.required_stock_area_m2), `${metric(offer.order_quantity)} ${q.unit}`, metric(offer.order_cost),
        q.available_quantity == null ? "Not reported" : `${metric(q.available_quantity)} ${q.unit}`,
        metric(order.required_continuous_length_m), continuity, orderSku]);
    }
    if (!selected?.length) cells(t, [allocation?.search_complete ? "No feasible allocation among admitted offers" :
      "No complete allocation found within the work limit", "Unassigned", "Unknown", "Unknown", "Unknown", "Unknown", "Unknown", "Unknown", "Unknown"]);
    const status = document.createElement("p");
    status.textContent = `Markers ${(row.roll_indices || []).map(i => i + 1).join(", ")}; ` +
      `total required length ${metric(row.required_length_m)} m. ` +
      (allocation?.optimality_proven ? "Least cost proved for these whole markers and admitted offers. " :
        "Least cost has not been proved. ") +
      (allocation?.budget_exhausted ? "Search reached its declared work limit. " : "") +
      `Candidate offers: ${(allocation?.candidate_offer_ids || []).join(", ") || "None"}.`;
    parent.append(status);
    detail(parent, `${title}: selected orders, market scopes, source evidence and search result`, row);
  }
  detail(parent, `${label}: dated market and retained quote book`, { market: r.market, quotes: r.quotes });
  return { complete_offer_coverage: r.complete_offer_coverage,
    complete_observed_quantity_coverage: r.complete_observed_quantity_coverage,
    complete_continuous_length_limit_coverage: r.complete_continuous_length_limit_coverage, currency: r.currency,
    selected_observed_order_total: r.selected_observed_order_total,
    least_observed_independent_order_total: r.least_observed_independent_order_total,
    order_allocation_optimality_proven: r.order_allocation_optimality_proven,
    search_complete: r.search_complete, budget_exhausted: r.budget_exhausted,
    search_nodes: r.search_nodes, quote_evaluations: r.quote_evaluations, limits: r.limits };
}
function screening(parent, r) {
  const screen = parent === sections.get("screen").comparison ? sections.get("screen") : null;
  if (screen) screen.study = r;
  const properties = [...new Set(r.alternatives.flatMap(row => Object.keys(row.readings)))].sort();
  const t = table(parent, "Scalar-property screening only — not garment-level ranking", ["Alternative", "Admission", "Dominated by", ...properties]);
  for (const row of r.alternatives) {
    cells(t, [row.key, row.eligible ? "Eligible" : `Excluded: ${row.exclusions.map(x => x.message).join("; ")}`,
      row.dominated_by.length ? row.dominated_by.join(", ") : "None reported", ...properties.map(key => {
        const q = row.readings[key];
        return q ? `${metric(q.value)} ${q.unit}; ${q.bounds ? `[${q.bounds.map(metric).join(", ")}] reported bounds` : "bounds not reported"}` : "Unknown";
      })]);
    detail(parent, `${row.key}: observations, protocols and frozen source (display only)`, {
      material_id: row.material_id, readings: row.readings, exclusions: row.exclusions, selection: row.selection });
    const selection = screen ? screenedSelection(row) : null;
    if (selection !== null) {
      const actions = document.createElement("div"); actions.className = "study-choice";
      const inspect = document.createElement("button"); inspect.type = "button";
      inspect.textContent = `Inspect ${row.key} source`;
      inspect.onclick = () => {
        if (!ready || pending || screen.study !== r || !screen.adoptions.includes(inspect)) return;
        invalidateThermalPoint(); resetOriginSource(); ++selectionRead; preparedBy = null; inspected = null;
        $("detail").textContent = ""; $("capabilities").replaceChildren();
        $("selectionFile").value = ""; $("selection").value = selection;
        execute("inspect");
      };
      actions.append(inspect); screen.adoptions.push(inspect);
      if (row.eligible === true) {
        const add = document.createElement("button"); add.type = "button";
        add.textContent = `Add ${row.key} to comparisons`; add.screenSelection = selection;
        add.onclick = () => {
          if (!ready || pending || screen.study !== r || !screen.adoptions.includes(add)) return;
          try { addStudySelection(selection, row.key); } catch (e) { error(String(e.message || e)); }
        };
        actions.append(add); screen.adoptions.push(add);
      }
      link("screen", actions, { name: `screened-material-${r.alternatives.indexOf(row) + 1}.json`, text: selection, mime: "application/json" });
      parent.append(actions);
    }
  }
}
function screenedSelection(row) {
  if (typeof row.selection_json !== "string" || !row.selection_json.trim()) return null;
  try {
    const selection = JSON.parse(row.selection_json);
    if (selection.id !== row.material_id || selection.record?.id !== selection.id ||
        !/^[0-9a-f]{64}$/.test(selection.expected_sha256) || typeof selection.allow_estimates !== "boolean" ||
        row.selection?.expected_sha256 !== selection.expected_sha256 || row.selection?.id !== selection.id ||
        row.selection?.allow_estimates !== selection.allow_estimates) return null;
    return row.selection_json;
  } catch { return null; }
}
function metricUsesBounds(policy, name) {
  return (policy?.basis === "reported_bounds" && policy.objectives.some(o => o.metric === name)) ||
    (policy?.constraints || []).some(c => c.metric === name && (c.basis ?? policy.basis) === "reported_bounds");
}
function objectiveComparison(parent, r) {
  const motionQualified = r.motion_required === true;
  const reportedBounds = r.policy.basis === "reported_bounds";
  const anyBounds = reportedBounds || r.policy.constraints.some(c => c.basis === "reported_bounds");
  const note = document.createElement("p");
  note.textContent = `Computed ${r.policy.basis} frontier: ${r.frontier.length ? r.frontier.join(", ") : "No eligible non-dominated alternatives"}. ` +
    (r.policy.allow_mixed_protocols ? "Mixed source protocols explicitly admitted. " : "Reported input protocols must match. ") +
    "Each constraint uses its explicit basis or inherits the objective basis. Nominal constraints retain nominal admission. " +
    (anyBounds ? "Requested reported-range decisions use complete source-reported ranges: mass at fixed authored geometry and inventory, " +
      "constant steady dry heat at fixed geometry, reservoirs, films and interfaces, " +
      "and vapor potential at fixed geometry, boundaries and retained sampling. " +
      "Final cumulative finite-water ranges, when requested, additionally hold the declared initial liquid and supply fixed. " +
      "Conditional on these inputs and evaluated alternatives; no general scenario uncertainty, confidence level, empirical fit or global optimum claim." :
      "Conditional on these inputs and evaluated alternatives; no propagated uncertainty, empirical fit or global optimum claim.");
  parent.append(note);
  const scope = document.createElement("p");
  scope.textContent = motionQualified ?
    "Motion-qualified comparison: a complete admitted trajectory is required. Each declared motion heat objective also requires its complete thermal history. " +
      "Initial static objectives retain their initial-frame meaning; signed energy uses the requested sheet or body-coverage domain." :
    "This comparison does not require motion completion. Static drape or heat objectives, when requested, describe the initial state. " +
      "Any separately requested motion or transient heat must be inspected in its own result.";
  parent.append(scope);
  const objectives = r.policy.objectives;
  const constraints = r.policy.constraints.filter(c => !objectives.some(o => o.metric === c.metric));
  const metrics = [...objectives, ...constraints];
  const wetFilmLabels = {
    motion_wet_film_evaporated_mass_kg: "Modeled covered evaporation",
    motion_wet_film_remaining_liquid_mass_kg: "Remaining body liquid",
    motion_wet_film_unmet_demand_mass_kg: "Unmet covered vapor demand",
    motion_wet_film_supplied_mass_kg: "Declared supplied body water",
    motion_wet_film_body_reservoir_latent_energy_j: "Prescribed-body latent energy"
  };
  const threadLabels = {
    thread_required_length_m: "Required thread length",
    thread_required_mass_kg: "Consumed thread mass",
    thread_observed_order_cost: "Complete thread order cost",
    cut_and_sew_selected_order_cost: "Complete fabric + thread order cost"
  };
  const cuttingLabels = {};
  for (const [prefix, scope] of [["cutting", "Roll"], ["finite_stock", "Finite stock"], ["cutting_job", "Complete cutting job"]]) {
    cuttingLabels[`${prefix}_modeled_path_seconds`] = `${scope} modeled knife time`;
    cuttingLabels[`${prefix}_cut_length_m`] = `${scope} cut length`;
    cuttingLabels[`${prefix}_raised_travel_length_m`] = `${scope} raised travel`;
  }
  const contactLabels = {
    body_contact_resultant_n: "Initial body-contact resultant",
    max_body_contact_vertex_force_n: "Initial maximum nodal body-contact force",
    motion_peak_body_contact_resultant_n: "Motion peak body-contact resultant",
    motion_max_body_contact_vertex_force_n: "Motion maximum nodal body-contact force"
  };
  const textileWaterLabels = {
    textile_water_evaporated_mass_kg: "Evaporated textile water",
    textile_water_remaining_mass_kg: "Remaining textile liquid",
    textile_water_supplied_mass_kg: "Supplied textile liquid",
    textile_water_condensed_mass_kg: "Condensed textile water",
    textile_water_drained_mass_kg: "Drained textile water",
    textile_water_latent_energy_j: "Textile evaporation latent energy",
    textile_water_condensation_latent_energy_j: "Textile condensation latent release"
  };
  const strainLabels = {};
  for (const [prefix, scope] of [["", "Initial"], ["motion_", "Motion"]]) {
    for (const [axis, label] of [["warp", "warp E11"], ["weft", "weft E22"], ["engineering_shear", "shear 2E12"]]) {
      strainLabels[`${prefix}min_${axis}_green_strain`] = `${scope} minimum ${label}`;
      strainLabels[`${prefix}max_${axis}_green_strain`] = `${scope} maximum ${label}`;
    }
  }
  if (metrics.some(o => Object.hasOwn(strainLabels, o.metric)) ||
      r.policy.required_consumers?.some(name => ["material_strain", "motion_material_strain"].includes(name))) {
    const strain = document.createElement("p");
    strain.textContent = "Material-direction objectives use signed, dimensionless Green strains in the retained warp and weft frame. " +
      "The requested instances define the spatial scope; motion extrema include the initial state and every accepted sample. " +
      "Inclusive constraints are declared design requirements. They do not establish fabric rupture limits, continuous-time bounds, wearer comfort or measured fit.";
    parent.append(strain);
  }
  if (metrics.some(o => Object.hasOwn(contactLabels, o.metric)) ||
      r.policy.required_consumers?.some(name => ["contact_loads", "motion_contact_loads"].includes(name))) {
    const contact = document.createElement("p");
    contact.textContent = "Body-contact objectives use normal barrier forces in newtons over the selected garment vertices. " +
      "Shared seam vertices count once, and opposite forces can cancel in a resultant. Nodal maxima depend on mesh resolution. " +
      "Motion extrema cover the initial state and every accepted sample; these declared limits do not establish contact pressure, friction loads, comfort or measured fit.";
    parent.append(contact);
  }
  if (metrics.some(o => Object.hasOwn(textileWaterLabels, o.metric))) {
    const textileWater = document.createElement("p");
    textileWater.textContent = "Textile water objectives use the complete admitted coupled temperature history. " +
      "The Rust comparison checks exact material and geometry ownership, canonical vapor paths, initial and supplied water inventory, incoming sensible heat and the combined sensible/latent energy balance. " +
      "These nominal quantities account for textile-owned liquid; body-reservoir water has separate objectives. " +
      "Source uncertainty, physical garment validation and a globally optimal design are not established.";
    parent.append(textileWater);
  }
  if (metrics.some(o => Object.hasOwn(threadLabels, o.metric)) ||
      r.policy.required_consumers?.some(name => ["sewing_consumables", "sewing_purchasing", "cut_and_sew_purchasing"].includes(name))) {
    const thread = document.createElement("p");
    thread.textContent = "Thread comparison uses the selected sewing plan's complete, source-qualified channels and exact yarn orders. " +
      "Thread purchasing retains its own currency and dated market. A requested complete fabric + thread cost additionally requires one coherent market and quote history with distinct physical stock. " +
      "Missing channels or offers leave the corresponding complete metric unknown. Recipe selection by time does not prove a thread-cost optimum.";
    parent.append(thread);
  }
  if (metrics.some(o => Object.hasOwn(cuttingLabels, o.metric)) ||
      r.policy.required_consumers?.some(name => ["cutting_program", "finite_stock_program", "cutting_job_program"].includes(name))) {
    const cutting = document.createElement("p");
    cutting.textContent = "Cutting objectives use complete selected knife traversals qualified for the exact textile and machine. " +
      "Roll and finite-stock objectives retain their separate physical scope; only a complete cutting job combines its disjoint routes. " +
      "Modeled time uses each program's retained constant-speed or exact-stop acceleration model, together with lift/lower phases. " +
      "Exact-stop timing includes acceleration and deceleration at every retained vertex, including detours and requested returns. Loading, blade turning and inspection remain outside these models. " +
      "An incomplete or refused process leaves these nominal metrics unknown while independent geometry and purchasing remain available. " +
      "A complete selected traversal need not be shortest; these objectives do not establish production cycle time, physical cut quality or machine authorization.";
    parent.append(cutting);
  }
  const waterMetrics = metrics.filter(o => Object.hasOwn(wetFilmLabels, o.metric));
  if (waterMetrics.length || r.policy.required_consumers?.includes("motion_wet_film")) {
    const water = document.createElement("p");
    water.textContent = "Finite body water comparison requires complete covered motion and the exact declared film policy. " +
      "Modeled evaporation is limited by each triangle's initial liquid and prescribed supply; water is not pooled between triangles. " +
      (waterMetrics.some(o => metricUsesBounds(r.policy, o.metric)) ?
        (waterMetrics.some(o => o.metric !== "motion_wet_film_supplied_mass_kg") ?
          "Final cumulative source ranges replay low- and high-demand Ret endpoints with geometry, boundaries, initial liquid, supply and integration fixed. " +
          "Remaining liquid reverses the demand endpoint order; evaporation, unmet demand and latent energy follow low to high demand. " +
          "Missing Ret bounds exclude these bounded water metrics while retaining nominal readings. " : "") +
        "Declared supplied water, when requested, has a fixed singleton range and needs no Ret bounds. " +
        "Accepted-sample water ledgers and replay remain nominal, without per-interval water bounds. " +
        "These conditional ranges do not establish general water-state uncertainty or confidence intervals. " :
        "These nominal results do not propagate water-state uncertainty. ") +
      "Prescribed-body latent energy does not include textile-temperature feedback or establish wearer cooling. " +
      "Neither basis describes bare-body transfer or physiological sweating.";
    parent.append(water);
  }
  const t = table(parent, "Declared computed objectives — authoritative Rust comparison", ["Alternative", "Admission", "Dominated by",
    ...metrics.map(o => `${wetFilmLabels[o.metric] || textileWaterLabels[o.metric] || strainLabels[o.metric] || contactLabels[o.metric] || threadLabels[o.metric] || cuttingLabels[o.metric] || o.metric} (${o.direction || "constraint"})`)]);
  for (const row of r.alternatives) {
    cells(t, [row.key, row.eligible ? "Eligible" : `Excluded: ${row.exclusions.map(e => e.message).join("; ")}`,
      row.dominated_by.length ? row.dominated_by.join(", ") : "None reported",
      ...metrics.map(o => {
        const q = row.readings[o.metric];
        if (!q) return "Unknown";
        const range = q.reported_bounds ? `; reported range [${q.reported_bounds.map(metric).join(", ")}] ${q.unit}` :
          metricUsesBounds(r.policy, o.metric) ? "; reported range unknown" : "";
        return `${metric(q.value)} ${q.unit}${range}`;
      })]);
  }
  for (const pair of r.incomparable) {
    const p = document.createElement("p"); p.textContent = `${pair.alternatives.join(" / ")}: ${pair.reason}`; parent.append(p);
  }
  if (r.policy.constraints.length) {
    const limits = table(parent, "Declared constraints — basis and inclusive limits",
      ["Metric", "Basis", "Minimum", "Maximum"]);
    for (const c of r.policy.constraints) cells(limits, [c.metric,
      `${c.basis ?? r.policy.basis}${c.basis ? " (explicit)" : " (inherited)"}`,
      c.minimum == null ? "None" : metric(c.minimum), c.maximum == null ? "None" : metric(c.maximum)]);
  }
  detail(parent, "Computed comparison policy, context and exclusions (display only)", r);
}
function assemblyMassComparison(parent, rows) {
  if (!rows.some(row => row.assembly_mass)) return;
  const note = document.createElement("p");
  note.textContent = "One declared pre-finishing assembly, not purchased stock or simulated cloth. All physical cut pieces and declared extras are counted. " +
    "Reported mass bounds use fixed authored geometry and inventory; they are not confidence intervals or an allowance for missing components.";
  parent.append(note);
  const t = table(parent, "Declared assembly mass — complete or unknown", ["Alternative", "Mass consumer", "All cut sheets (kg)",
    "Declared assembly (kg)", "Reported assembly bounds (kg)", "Extra inventory"]);
  for (const row of rows) {
    const report = available(row.assembly_mass), mass = report?.assembly_mass;
    cells(t, [row.key, report ? (mass?.kg == null ? "Report available; total unknown" : "Complete declared inventory") :
      row.assembly_mass ? state(row.assembly_mass) : "Not requested",
      metric(report?.sheet_mass?.kg), metric(mass?.kg),
      mass?.reported_bounds_kg ? `[${mass.reported_bounds_kg.map(metric).join(", ")}]` : "Not reported",
      report?.request?.inventory_complete === true ? "Declared complete" :
        report?.request?.inventory_complete === false ? "Incomplete" : "Unknown"]);
    detail(parent, `${row.key}: cut-piece mass, retained extras and source inventory (display only)`,
      report || { refusal: row.assembly_mass?.refusal });
  }
}
function attachedMassComparison(parent, rows, simulations) {
  if (!rows.some(row => available(simulations.get(row.key))?.drape?.attached_mass)) return;
  const note = document.createElement("p");
  note.textContent = "Source-qualified accessory groups contribute to gravity and inertia at their declared physical attachment points. " +
    "Cloth thermal storage continues to use the characterized sheet materials and reference areas.";
  parent.append(note);
  const t = table(parent, "Attached accessory mass — gravity and inertia", ["Alternative", "Drape", "Simulated cloth (kg)",
    "Attached groups (kg)", "Total inertial mass (kg)", "Mass balance residual (kg)"]);
  for (const row of rows) {
    const outcome = simulations.get(row.key), report = available(outcome)?.drape?.attached_mass;
    cells(t, [row.key, state(outcome), metric(report?.cloth_mass_kg), metric(report?.attached_mass_kg),
      metric(report?.total_inertial_mass_kg), metric(report?.mass_balance_residual_kg)]);
    detail(parent, `${row.key}: attached mass sources and welded physical targets (display only)`,
      report || { refusal: outcome?.refusal });
  }
}
function materialStrainComparison(parent, key, output, dependency, requested, moving = false) {
  if (!requested && (!output || output.status === "not_requested")) return;
  const result = available(output), label = moving ? "accepted-motion" : "initial";
  const status = document.createElement("p");
  status.textContent = `${key}: ${label} material-direction strain — ${output ? state(output) : `Unavailable: ${state(dependency)}`}. ` +
    (result ? `${metric(result.triangle_samples)} triangle evaluations; sampled duration ${metric(result.duration_s)} s.` : "");
  parent.append(status);
  if (result) {
    const t = table(parent, `${key}: ${label} material-direction strain`,
      ["Scope", "Direction", "Minimum (1)", "Minimum location", "Maximum (1)", "Maximum location"]);
    const witness = w => w ? `Triangle ${metric(w.source_triangle)}, sample ${metric(w.sample_index)} at ${metric(w.time_s)} s` : "Unknown";
    for (const [scope, values] of [["Selected scope", result], ...Object.entries(result.instances || {})]) {
      ["Warp E11", "Weft E22", "Engineering shear 2E12"].forEach((axis, i) => {
        cells(t, [scope, axis, metric(values.green_strain_range?.[i]?.[0]), witness(values.minimum_witness?.[i]),
          metric(values.green_strain_range?.[i]?.[1]), witness(values.maximum_witness?.[i])]);
      });
    }
    const note = document.createElement("p");
    note.textContent = "Raw material Green strain [E11, E22, 2E12] is dimensionless and retains its sign; it is not engineering extension percent. " +
      "Values use the production material frame before numerical compression or gather softening. Witnesses retain the earliest sampled extreme and its source triangle. " +
      "Characterization domains describe supported material observations; declared comparison limits do not establish rupture, safe strain, continuous-time bounds or physical garment fit.";
    parent.append(note);
  }
  detail(parent, `${key}: ${label} strain scope, sources and sampled witnesses (display only)`, result || {refusal: output?.refusal || dependency?.refusal});
}
function contactLoadsComparison(parent, key, output, dependency, requested, moving = false) {
  if (!requested && (!output || output.status === "not_requested")) return;
  const result = available(output), label = moving ? "accepted-motion" : "initial";
  const status = document.createElement("p");
  status.textContent = `${key}: ${label} body-contact loads — ${output ? state(output) : `Unavailable: ${state(dependency)}`}. ` +
    (result ? `${metric(result.vertex_samples)} vertex evaluations; ${metric(result.pair_evaluations)} contact-pair evaluations.` : "");
  parent.append(status);
  if (result) {
    const frames = result.frames || [];
    const witness = index => {
      const frame = frames.find(f => f.sample_index === index);
      return frame ? `Sample ${metric(frame.sample_index)} at ${metric(frame.time_s)} s` : "Unknown";
    };
    const summary = table(parent, `${key}: ${label} body-contact loads`,
      ["Peak resultant (N)", "Resultant witness", "Maximum nodal force (N)", "Nodal-force witness"]);
    cells(summary, [metric(result.peak_body_resultant_n), witness(result.peak_body_resultant_sample),
      metric(result.max_body_vertex_force_n), witness(result.max_body_vertex_force_sample)]);
    const samples = table(parent, `${key}: ${label} sampled body-contact forces`,
      ["Sample", "Time (s)", "Resultant x (N)", "Resultant y (N)", "Resultant z (N)", "Resultant magnitude (N)", "Maximum nodal force (N)", "Source vertex"]);
    for (const frame of frames) {
      const body = frame.body;
      cells(samples, [metric(frame.sample_index), metric(frame.time_s),
        ...[0, 1, 2].map(i => metric(body?.resultant_n?.[i])), metric(body?.resultant_magnitude_n),
        metric(body?.max_vertex_force_n), metric(body?.max_force_vertex)]);
    }
    const scope = document.createElement("p");
    scope.textContent = `Selected garment instances: ${Object.keys(result.source?.instances || {}).join(", ") || "Unknown"}. ` +
      "The shared vertices of sewn pieces are counted once. Resultants can hide opposing local loads; compare nodal maxima at the same mesh resolution. " +
      "These normal barrier forces exclude tangential friction and do not establish pressure, safe wear or continuous-time load bounds.";
    parent.append(scope);
  }
  detail(parent, `${key}: ${label} contact scope, materials and sampled force witnesses (display only)`,
    result || { refusal: output?.refusal || dependency?.refusal });
}
function transientHeatComparison(parent, key, result, dependency, policy) {
  const consumers = [["transient_surface", "Independent sheets"], ["transient_coverage", "Body coverage + exposed cloth"]]
    .filter(([name]) => policy?.[name] || result?.[name]);
  if (!consumers.length) return;
  const t = table(parent, `${key}: transient heat — complete time history or refusal`, ["Thermal domain", "Status", "Duration (s)",
    "Thermal substeps", "Final temperature range (°C)", "Net boundary energy (J)", "Textile sensible change (J)",
    "Energy residual (J)", "Signed remap residual (J)"]);
  for (const [name, label] of consumers) {
    const output = result?.[name], h = available(output), range = h?.trace?.at(-1)?.temperature_range_c;
    cells(t, [label, output ? state(output) : `Unavailable: ${state(dependency)}`, metric(h?.duration_s), metric(h?.thermal_steps),
      range ? `[${range.map(metric).join(", ")}]` : "Unknown", metric(h?.net_boundary_energy_j),
      metric(h?.physical_stored_change_j ?? h?.stored_change_j), metric(h?.balance_error_j),
      name === "transient_surface" ? "Not applicable" : metric(h?.signed_remap_residual_j)]);
    detail(parent, `${key}: ${label} temperature history, source and energy accounting (display only)`, h || { refusal: output?.refusal || dependency?.refusal });
    if (h?.thermal_bridges) {
      const bridges = h.thermal_bridges;
      const energy = table(parent, `${key}: seam and closure heat during motion`,
        ["Baseline inside energy (J)", "Signed bridge excess (J)", "Total inside energy (J)",
          "Baseline outside energy (J)", "Total outside energy (J)"]);
      cells(energy, [metric(bridges.baseline_inside_energy_j), metric(bridges.bridge_energy_j),
        metric(bridges.total_inside_energy_j), metric(bridges.baseline_outside_energy_j), metric(bridges.total_outside_energy_j)]);
      const scope = document.createElement("p");
      scope.textContent = "These seam and closure paths use an independently qualified zero-storage exchange law. " +
        "Their actual length and body ownership are checked at each accepted motion endpoint, with live textile temperatures and the full interval inside the source domain. " +
        "The same signed excess energy is added to the inside and outside boundary ledgers; baseline textile temperature, sensible storage and net boundary energy remain unchanged. " +
        "The ordinary inside-energy comparison objective uses the total. This model does not infer seam heat capacity or a time-dependent law from a steady measurement. " +
        "A later source or geometry failure refuses the complete affected heat history.";
      parent.append(scope);
      detail(parent, `${key}: moving bridge qualifications, geometry and signed energy history (display only)`, bridges);
    }
    if (h?.surface_water) {
      const water = h.surface_water;
      if (h.surface_water_selection) {
        const selection = h.surface_water_selection;
        const source = table(parent, `${key}: selected water qualification`,
          ["Qualification", "Source", "Evidence", "Selected request SHA-256"]);
        cells(source, [Number.isSafeInteger(selection.index) ? selection.index + 1 : "Unknown",
          water.request?.source?.reference ?? "Unknown", water.request?.evidence ?? "Unknown",
          selection.request_sha256 ?? "Unknown"]);
      }
      if (h.coupled_water_selection) {
        const selection = h.coupled_water_selection;
        const source = table(parent, `${key}: selected water and mechanics qualification`,
          ["Qualification", "Water source", "Mechanical source", "Selected pair SHA-256"]);
        cells(source, [Number.isSafeInteger(selection.index) ? selection.index + 1 : "Unknown",
          water.request?.source?.reference ?? "Unknown",
          h.mechanical_coupling?.qualification?.source?.reference ?? "Unknown",
          selection.pair_sha256 ?? "Unknown"]);
      }
      const finiteVapor = water.schema === "ftx-textile-surface-water-result/5";
      const draining = water.schema === "ftx-textile-surface-water-result/4" ||
        finiteVapor && typeof water.drained_liquid_kg === "number";
      const condensing = finiteVapor || water.schema === "ftx-textile-surface-water-result/3" ||
        draining && typeof water.condensed_liquid_kg === "number";
      const supplied = draining || condensing || water.schema === "ftx-textile-surface-water-result/2";
      const mass = table(parent, `${key}: textile surface liquid — conserved ${condensing ? "initial, supplied and condensed" : supplied ? "initial and supplied" : "initial"} inventory`,
        ["Initial liquid (kg)", ...(supplied ? ["Supplied liquid (kg)"] : []),
          ...(condensing ? ["Condensed liquid (kg)"] : []), "Evaporated liquid (kg)",
          ...(draining ? ["Drained liquid (kg)"] : []), "Remaining liquid (kg)", "Mass residual (kg)", "Dryout events"]);
      cells(mass, [metric(water.initial_liquid_kg), ...(supplied ? [metric(water.supplied_liquid_kg)] : []),
        ...(condensing ? [metric(water.condensed_liquid_kg)] : []), metric(water.evaporated_liquid_kg),
        ...(draining ? [metric(water.drained_liquid_kg)] : []), metric(water.remaining_liquid_kg),
        metric(water.mass_balance_error_kg), metric(water.dryout_events)]);
      const energy = table(parent, `${key}: textile evaporation and sensible energy`,
        ["Latent energy (J)", "Exported vapor sensible energy (J)",
          ...(draining ? ["Exported liquid sensible energy (J)"] : []), "Liquid sensible change (J)",
          ...(supplied ? ["Incoming liquid sensible energy (J)"] : []),
          ...(condensing ? ["Condensation latent release (J)", "Incoming vapor sensible energy (J)"] : []), "Combined energy residual (J)"]);
      cells(energy, [metric(water.latent_energy_j), metric(water.exported_vapor_sensible_energy_j),
        ...(draining ? [metric(water.exported_liquid_sensible_energy_j)] : []),
        metric(water.liquid_stored_change_j), ...(supplied ? [metric(water.supplied_liquid_sensible_energy_j)] : []),
        ...(condensing ? [metric(water.condensation_latent_energy_j), metric(water.incoming_vapor_sensible_energy_j)] : []), metric(water.balance_error_j)]);
      if (finiteVapor) {
        const vapor = water.vapor_reservoir;
        const inventory = table(parent, `${key}: finite shared vapor inventory and pressure`,
          ["Initial vapor (kg)", "Remaining vapor (kg)", "Net vapor received (kg)", "Mass residual (kg)",
            "Initial vapor pressure (Pa)", "Vapor pressure (Pa)"]);
        cells(inventory, [metric(vapor?.initial_vapor_mass_kg), metric(vapor?.remaining_vapor_mass_kg),
          metric(vapor?.net_vapor_received_kg), metric(vapor?.mass_balance_error_kg),
          metric(vapor?.initial_vapor_pressure_pa), metric(vapor?.vapor_pressure_pa)]);
        const reservoirEnergy = table(parent, `${key}: finite vapor energy and fixed-temperature heat exchange`,
          ["Vapor enthalpy change (J)", "Net vapor enthalpy received (J)",
            "Isothermal heat removed (J)", "Enthalpy residual (J)"]);
        cells(reservoirEnergy, [metric(vapor?.vapor_enthalpy_change_j), metric(vapor?.net_vapor_enthalpy_received_j),
          metric(vapor?.isothermal_heat_removed_j), metric(vapor?.enthalpy_balance_error_j)]);
      }
      const scope = document.createElement("p");
      scope.textContent = "This explicitly sourced free liquid shares the inner textile cell temperature. Evaporation cools that cell, " +
        "and the temperature feeds the declared vapor-pressure table. " +
        (supplied ? "Each mechanical interval has independently sourced delivery rates and incoming temperatures per instance. Supply uses reference material area, can support evaporation on a dry patch and can rewet it. " :
          "Dryout ends evaporation; the remaining interval uses the dry heat model. ") +
        "The combined balance includes dry-textile and liquid sensible storage, exported vapor sensible energy and latent energy, " +
        "against boundary energy plus qualified material heat" + (supplied ? " and incoming liquid sensible energy. Loading beyond the qualified cap refuses the interval. " : ". The initial-only model has zero replenishment. ") +
        (condensing ? "Independently qualified reverse vapor transfer can condense on dry or wet cloth and later re-evaporate. Condensation latent release and incoming vapor sensible enthalpy are separate energy inputs; the effective enthalpy convention remains explicit in the source policy. " : "") +
        (finiteVapor ? "All textile patches share one finite vapor inventory: evaporation raises its pressure and condensation consumes available vapor. The source explicitly fixes vapor capacitance and temperature. Isothermal heat removed is the signed heat exchange needed to hold that temperature; it is separate from textile boundary heat. No gas volume, heat capacity or body moisture source is inferred. " : "") +
        (draining ? "Independently sourced drainage demand uses reference material area and is limited by available liquid. Drained liquid exports sensible enthalpy at the old inner-cell temperature with no latent energy; empty cloth shares available inflow proportionally between evaporation and drainage demands. No overflow or runoff law is inferred. " : "") +
        (h.mechanical_coupling?.exchange ? `Qualified supplied liquid, condensed vapor, evaporation${draining ? " and drainage" : ""} update cloth inertia and gravity after every accepted interval. ` :
          h.mechanical_coupling ? "Qualified evaporation updates cloth inertia and gravity after every accepted interval; co-moving vapor removes its own momentum without accelerating the remaining cloth. " :
          "The liquid adds no mechanical water mass. ") +
        (["ftx-coupled-liquid-mechanics-result/2", "ftx-coupled-liquid-mechanics-result/3", "ftx-coupled-liquid-mechanics-result/4", "ftx-coupled-liquid-mechanics-result/5", "ftx-coupled-liquid-mechanics-result/6"].includes(h.mechanical_coupling?.schema) ?
          "Incoming water also transfers its explicitly prescribed world-frame momentum. Inelastic mixing loss is reported separately from sensible and latent heat. " : "") +
        "It does not infer absorbed moisture, wicking, body sweat or wet material properties. Comparison admission checks the complete liquid and vapor ledger, " +
        "including exact material bindings and canonical vapor paths, before temperature, energy or textile-water objectives can qualify.";
      parent.append(scope);
      detail(parent, `${key}: textile liquid source, exact patches and canonical vapor paths (display only)`, water);
      if (h.mechanical_coupling) {
        const m = h.mechanical_coupling, exchange = m.exchange;
        const prescribed = ["ftx-coupled-liquid-mechanics-result/2", "ftx-coupled-liquid-mechanics-result/3", "ftx-coupled-liquid-mechanics-result/4", "ftx-coupled-liquid-mechanics-result/5", "ftx-coupled-liquid-mechanics-result/6"].includes(m.schema);
        const recoil = m.schema === "ftx-coupled-liquid-mechanics-result/6";
        const phaseScheduled = recoil || m.schema === "ftx-coupled-liquid-mechanics-result/5";
        const inertia = table(parent, `${key}: ${exchange ? "liquid exchange" : "evaporation"} changes moving cloth mass`,
          ["Dry cloth + accessories (kg)", "Initial water (kg)", "Remaining water (kg)", "Current inertial mass (kg)", "Mass residual (kg)"]);
        cells(inertia, [metric(m.substrate_and_attached_mass_kg), metric(m.initial_liquid_mass_kg),
          metric(m.remaining_liquid_mass_kg), metric(m.total_inertial_mass_kg), metric(m.mass_balance_residual_kg)]);
        const vector = value => Array.isArray(value) ? value.map(metric).join(", ") : "Unknown";
        if (exchange) {
          const transfer = table(parent, `${key}: ${prescribed ? "prescribed incoming" : "co-moving"} liquid transfer — separate from thermal energy`,
            ["Transfer", "Mass (kg)", "Momentum x, y, z (kg m/s)", "Kinetic energy (J)", "Gravitational energy (J)"]);
          cells(transfer, ["Initial admission", metric(m.initial_liquid_mass_kg), vector(exchange.initial_momentum_kg_m_s),
            metric(exchange.initial_kinetic_energy_j), metric(exchange.initial_gravitational_energy_j)]);
          cells(transfer, ["Incoming liquid and condensed vapor", metric(exchange.incoming_mass_kg), vector(exchange.incoming_momentum_kg_m_s),
            metric(exchange.incoming_kinetic_energy_j), metric(exchange.incoming_gravitational_energy_j)]);
          cells(transfer, [draining ? "Outgoing vapor and drained liquid" : "Outgoing vapor", metric(exchange.outgoing_mass_kg), vector(m.exported_momentum_kg_m_s),
            metric(m.exported_kinetic_energy_j), metric(m.exported_gravitational_energy_j)]);
          const residual = table(parent, `${key}: liquid exchange momentum and energy residuals`,
            ["Momentum x, y, z (kg m/s)", "Kinetic energy (J)", "Gravitational energy (J)",
              ...(prescribed ? ["Inelastic mixing loss (J)"] : []),
              ...(recoil ? ["Outgoing separation work (J)"] : [])]);
          cells(residual, [vector(m.transfer_momentum_residual_kg_m_s), metric(m.transfer_kinetic_residual_j),
            metric(m.transfer_gravitational_residual_j), ...(prescribed ? [metric(exchange.mixing_dissipation_j)] : []),
            ...(recoil ? [metric(exchange.outgoing_separation_work_j)] : [])]);
        } else {
          const transfer = table(parent, `${key}: mechanical energy carried away by vapor`,
            ["Momentum x, y, z (kg m/s)", "Kinetic energy (J)", "Gravitational energy (J)", "Kinetic transfer residual (J)"]);
          cells(transfer, [vector(m.exported_momentum_kg_m_s), metric(m.exported_kinetic_energy_j),
            metric(m.exported_gravitational_energy_j), metric(m.transfer_kinetic_residual_j)]);
        }
        const couplingScope = document.createElement("p");
        couplingScope.textContent = "Initial liquid is added co-moving after the dry drape; it is not a wet equilibrium. Each mechanical step uses its starting water mass, then accepted heat and liquid exchange update the next step. " +
          "Refine the mechanical time step to assess this first-order coupling. The thermal latent/sensible ledger remains separate from transferred bulk mechanical energy. " +
          (exchange ? (prescribed ?
            `Incoming liquid and condensed vapor use ${phaseScheduled ? "their independently scheduled" : "the prescribed"} world-frame velocities. ` +
            (recoil ? `Outgoing vapor${draining ? " and drained liquid use" : " uses"} scheduled velocities relative to the post-mixing cloth; the retained cloth recoils to conserve momentum. Inelastic mixing loss and outgoing separation work are mechanical boundary terms, separate from thermal heat. ` :
              `The outgoing vapor${draining ? " and drained liquid leave" : " leaves"} at the mixed cloth velocity. Inelastic mixing loss is separate from thermal heat. `) :
            `Incoming liquid, condensed vapor and outgoing vapor${draining ? " and drained liquid" : ""} share the local endpoint cloth velocity under the independent source qualification. `) +
            "Equal incoming and outgoing mass retain both transfer ledgers. Signed gravitational energies keep the declared world frame. These residuals check mass transfer, not the complete motion energy balance. " :
            "Supply and condensation are unsupported in this evaporation-only mode. ") +
          "A coupled heat or source-domain failure refuses the motion. " +
          (draining ? "Liquid drainage and evaporation retain separate mass totals; their combined outgoing mass carries bulk momentum and energy. " : "") +
          "Resolved fluid impact and inferred wet material properties remain unsupported.";
        parent.append(couplingScope);
        detail(parent, `${key}: ${exchange ? "liquid exchange" : "evaporation"} mechanics qualification and complete transfer ledger (display only)`, m);
      }
    }
    const channels = [
      ["plastic_dissipation_heat", "Plastic yield", "plastic_dissipation_j", "initial_plastic_dissipation_j"],
      ["relaxation_dissipation_heat", "Maxwell dashpots", "viscous_dissipation_j", "initial_viscous_dissipation_j"],
      ["bending_plastic_dissipation_heat", "Bending yield", "plastic_dissipation_j", "initial_plastic_dissipation_j"]
    ].filter(([field]) => h?.[field]);
    if (channels.length) {
      const domain = name === "transient_surface" ? "independent-sheet heat" : "coverage and exposed-cloth heat";
      const work = table(parent, `${key}: material work supplied to ${domain}`, ["Physical source", "Work since motion start (J)",
        "Supplied material heat (J)", "Unconverted work (J)", "Initial work excluded (J)"]);
      for (const [field, label, total, initial] of channels) {
        const source = h[field];
        cells(work, [label, metric(source[total]), metric(source.supplied_heat_j),
          metric(source.unconverted_dissipation_j), metric(source[initial])]);
        detail(parent, `${key}: ${label} conversion observations for ${domain} (display only)`, {
          distribution: source.distribution, temporal_distribution: source.temporal_distribution,
          material_fractions: source.material_fractions ?? null
        });
      }
      const scope = document.createElement("p");
      scope.textContent = (h.surface_water ?
        "The energy residual includes dry-textile and liquid sensible storage, exported vapor and drainage sensible energy and evaporative latent energy, minus net boundary energy, qualified source heat and declared incoming liquid/vapor sensible energy and condensation latent release. " :
        "The energy residual is physical stored change minus net boundary energy minus the sum of qualified source heat. ") +
        "Each source needs its own material conversion evidence. Work already accumulated in the initial drape, reversible storage and numerical loss are excluded. " +
        (channels.every(([field]) => h[field].distribution === "uniform_reference_thickness" && h[field].temporal_distribution === "uniform_over_accepted_interval") ?
          "Converted heat is distributed uniformly through each sheet's reference thickness and over its accepted motion interval. " :
          "Inspect the retained report for the declared heat distribution. ") +
        (name === "transient_coverage" ? "Source heat follows covered and exposed reference material; bare body receives none. " : "") +
        (channels.some(([field]) => field === "bending_plastic_dissipation_heat") ?
          "Bending work uses each source triangle's reference area; overlapping reconstruction stencils do not multiply deposited heat. " : "") +
        (h.mechanical_coupling ? "Temperature controls qualified vapor exchange, which updates mechanical mass; the material laws remain frozen within their qualified domains. " : "Temperatures do not feed back into mechanics. ") +
        "The temperature-history report retains the material fractions and their source evidence.";
      parent.append(scope);
    }
  }
  const note = document.createElement("p");
  note.textContent = (consumers.some(([name]) => available(result?.[name])?.mechanical_coupling) ?
    "Only the explicitly qualified liquid domain feeds mechanical mass; the thermal domains retain separate initial temperatures, interval films and energy totals. Do not add their energy totals. " :
    "These are separate one-way thermal models with declared initial temperatures and interval films. Do not add their energy totals. ") +
    "Remapping transfers stored heat without elapsed time; its numerical residual is not environmental heat. Bare body paths store no cloth energy. " +
    "Geometry uses accepted motion samples; smaller thermal substeps do not refine the body motion. Small energy residuals are not empirical accuracy.";
  parent.append(note);
}
function motionSamples(parent, key, motion) {
  const d = document.createElement("details"), title = document.createElement("summary");
  title.textContent = `${key}: inspect retained motion samples`; d.append(title); parent.append(d);
  let initialized = false;
  d.addEventListener("toggle", () => {
    if (!d.open || initialized) return;
    initialized = true;
    try {
      const accepted = motion.samples, hasInitial = motion.initial_sample != null;
      const samples = Array.isArray(accepted) && accepted.length ? (hasInitial ? [motion.initial_sample, ...accepted] : accepted) : null;
      const counts = [samples?.[0]?.body_positions_m?.length, samples?.[0]?.positions_m?.length];
      const tracks = ["body_positions_m", "positions_m"], topology = [motion.body_triangles, motion.triangles];
      if (!samples || counts.some(n => !n) || topology.some((triangles, i) =>
        !Array.isArray(triangles) || !triangles.length || triangles.some(t => !Array.isArray(t) || t.length !== 3 ||
          t.some(v => !Number.isInteger(v) || v < 0 || v >= counts[i])))) {
        throw Error("Retained body/cloth geometry is incomplete; inspect the downloaded report.");
      }
      const low = [Infinity, Infinity, Infinity], high = [-Infinity, -Infinity, -Infinity];
      let previous = hasInitial ? -1 : 0;
      for (const sample of samples) {
        if (!Number.isFinite(sample.time_s) || sample.time_s <= previous || (hasInitial && previous === -1 && sample.time_s !== 0)) {
          throw Error("Retained physical sample times are invalid; no intermediate motion is inferred.");
        }
        previous = sample.time_s;
        for (const [i, track] of tracks.entries()) {
          if (!Array.isArray(sample[track]) || sample[track].length !== counts[i]) {
            throw Error("Retained samples changed topology; inspect the downloaded report.");
          }
          for (const p of sample[track]) {
            if (!Array.isArray(p) || p.length !== 3 || p.some(v => !Number.isFinite(v))) {
              throw Error("Retained sample coordinates are invalid; inspect the downloaded report.");
            }
            for (let axis = 0; axis < 3; ++axis) { low[axis] = Math.min(low[axis], p[axis]); high[axis] = Math.max(high[axis], p[axis]); }
          }
        }
      }
      const span = Math.max(...high.map((v, i) => v - low[i]), 1e-6);
      if (!Number.isFinite(span)) throw Error("Retained sample coordinates exceed the display range.");
      const center = high.map((v, i) => low[i] + (v - low[i]) / 2), scale = 372 / span;
      const note = document.createElement("p"); note.className = "note";
      note.textContent = "Select a retained state at its exact physical time. " + (hasInitial ?
        "The first state precedes motion; later states are accepted solver steps. " :
        "This report retains accepted solver steps; its initial geometry was not retained. ") +
        "Grey body and teal cloth use one fixed metre scale across both views and every sample. Wireframe projections do not resolve depth.";
      const label = document.createElement("label"), slider = document.createElement("input"), stamp = document.createElement("output");
      label.textContent = "Retained sample"; slider.type = "range"; slider.min = "0"; slider.max = String(samples.length - 1);
      slider.step = "1"; slider.value = "0"; slider.ariaLabel = `${key}: retained motion sample`; label.append(slider);
      const views = document.createElement("div"); views.className = "motion-views";
      const contexts = ["Front · X / Z", "Side · Y / Z"].map(name => {
        const figure = document.createElement("figure"), caption = document.createElement("figcaption"), canvas = document.createElement("canvas");
        caption.textContent = name; canvas.width = 420; canvas.height = 420;
        canvas.ariaLabel = `${key}: ${name} at the selected physical sample`; canvas.textContent = canvas.ariaLabel;
        const context = canvas.getContext("2d");
        if (!context) throw Error("Canvas preview unavailable; retained sample geometry remains in the downloaded report.");
        figure.append(caption, canvas); views.append(figure); return context;
      });
      function render() {
        const index = Number(slider.value);
        if (!Number.isInteger(index) || index < 0 || index >= samples.length) return;
        const sample = samples[index];
        stamp.textContent = `${hasInitial && index === 0 ? "Initial state" :
          `Accepted step ${index + 1 - Number(hasInitial)} of ${accepted.length}`} · t = ${metric(sample.time_s)} s`;
        for (const [axis, ctx] of contexts.entries()) {
          ctx.clearRect(0, 0, 420, 420);
          for (const [i, track] of tracks.entries()) {
            ctx.strokeStyle = i === 0 ? "#a0adb2" : "#145b68"; ctx.lineWidth = i === 0 ? 0.5 : 1;
            ctx.beginPath();
            for (const triangle of topology[i]) {
              for (const [j, vertex] of triangle.entries()) {
                const p = sample[track][vertex], x = 210 + (p[axis] - center[axis]) * scale, y = 210 - (p[2] - center[2]) * scale;
                if (j === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
              }
              ctx.closePath();
            }
            ctx.stroke();
          }
        }
      }
      slider.addEventListener("input", render); d.append(note, label, stamp, views); render();
    } catch (e) {
      const message = document.createElement("p"); message.textContent = e.message; d.append(message);
    }
  });
}
function wetFilmResult(parent, key, history) {
  const film = history.wet_film;
  if (!film) return;
  const totals = table(parent, `${key}: finite body water and latent energy`, ["Quantity", "Value", "Unit"]);
  for (const [label, field, unit] of [
    ["Initial body-side liquid", "starting_liquid_mass_kg", "kg"],
    ["Prescribed liquid supplied", "supplied_mass_kg", "kg"],
    ["Covered-path potential demand", "potential_evaporation_mass_kg", "kg"],
    ["Modeled evaporation from available liquid", "evaporated_mass_kg", "kg"],
    ["Unmet covered-path demand", "unmet_demand_mass_kg", "kg"],
    ["Remaining body-side liquid", "remaining_liquid_mass_kg", "kg"],
    ["Latent energy supplied by prescribed body reservoir", "body_reservoir_latent_energy_j", "J"],
    ["Liquid mass balance residual", "mass_balance_error_kg", "kg"]
  ]) cells(totals, [label, metric(film.totals?.[field]), unit]);
  const note = document.createElement("p");
  note.textContent = "Liquid belongs to the same body triangles throughout motion. Each interval uses its declared average covered-path demand and constant liquid supply; " +
    "evaporation is limited by the liquid available to each triangle. Uncovered transfer remains unknown. " +
    "The prescribed body reservoir supplies the reported latent energy. Textile temperatures, garment mass and wet-material properties are unchanged by this body-film model. " +
    "The source and model assumptions are retained below; this is not a sweat-production or wearer-comfort prediction.";
  parent.append(note);
  const samples = table(parent, `${key}: body water at accepted motion samples`, ["Time (s)", "Supplied liquid (kg)",
    "Modeled evaporation (kg)", "Unmet demand (kg)", "Remaining liquid (kg)", "Body latent energy (J)", "Mass balance residual (kg)"]);
  for (const sample of history.samples || []) {
    const water = sample.wet_film?.totals;
    cells(samples, [metric(sample.time_s), metric(water?.supplied_mass_kg), metric(water?.evaporated_mass_kg),
      metric(water?.unmet_demand_mass_kg), metric(water?.remaining_liquid_mass_kg),
      metric(water?.body_reservoir_latent_energy_j), metric(water?.mass_balance_error_kg)]);
  }
  detail(parent, `${key}: body water source, model assumptions and per-triangle ledger`, film);
}
function vaporHistoryResult(parent, key, history, statusLabel, refusal) {
  const summary = table(parent, `${key}: moving layered vapor potential`, ["History", "Duration (s)",
    "Accepted samples", "Potential energy (J)", "Potential water mass (kg)", "Integration rule"]);
  cells(summary, [statusLabel, metric(history?.duration_s),
    metric(history?.samples?.length), vaporMetric(history?.totals, "potential_energy_j"),
    vaporMetric(history?.totals, "potential_mass_kg"), history?.integration || "Unknown"]);
  const note = document.createElement("p");
  const integration = history?.integration === "right_endpoint" ?
    "The right-endpoint rule holds each accepted sample's potential flow over its preceding interval; " :
    history?.integration === "composite_trapezoid" ?
      "The composite trapezoidal rule averages each interval's retained start and end flows; " :
      "The integration rule is retained in the report; ";
  note.textContent = "Each accepted body/cloth sample recomputes the covered layer paths under the fixed declared vapor boundaries. " +
    integration + "refining motion samples can change the integral. " +
    "Uncovered body transfer remains unknown, and complete-domain comparisons require coverage at every sample. " +
    (history?.wet_film ?
      "Potential transfer and the finite body-water ledger below retain separate meanings; the latter limits modeled evaporation by the supplied liquid." :
      "These potentials do not model water supply, actual evaporation, moisture storage or cooling supplied to dry heat.");
  parent.append(note);
  vaporBoundsNote(parent, history?.totals, `${key}: moving vapor source bounds`);
  detail(parent, `${key}: moving vapor sources, sample coverage and exact replay (display only)`,
    history || { refusal });
  if (!history) return;
  const samples = table(parent, `${key}: vapor coverage at accepted motion samples`, ["Time (s)", "Interval (s)",
    "Covered body (m²)", "Uncovered body (m²)", "Unassigned reference cloth (m²)",
    "Potential power (W)", "Potential water flow (kg/s)", "Interval energy (J)", "Interval water mass (kg)"]);
  for (const sample of [history.initial, ...(history.samples || [])].filter(Boolean)) {
    const coverage = sample.result?.coverage, initial = sample.time_s === 0;
    cells(samples, [metric(sample.time_s), initial ? "Initial only" : metric(sample.dt_s),
      metric(coverage?.covered_area_m2), metric(coverage?.uncovered_area_m2), metric(coverage?.unassigned_cloth_reference_area_m2),
      vaporMetric(coverage, "covered_potential_heat_flow_w"), vaporMetric(coverage, "covered_potential_mass_flow_kg_s"),
      initial ? "Initial only" : vaporMetric(sample, "potential_energy_j"),
      initial ? "Initial only" : vaporMetric(sample, "potential_mass_kg")]);
  }
  wetFilmResult(parent, key, history);
}
function motionVaporComparison(op, parent, key, index, motion, dependency, requested, filePrefix = "") {
  const output = motion?.vapor_coverage_history;
  if (!requested?.vapor_coverage_history && !output) return;
  const history = available(output);
  vaporHistoryResult(parent, key, history, output ? state(output) : `Unavailable: ${state(dependency)}`,
    output?.refusal || dependency?.refusal);
  if (typeof history?.replay_request_json === "string") {
    const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
    link(op, actions, { name: `${filePrefix}alternative-${index + 1}-motion-vapor-coverage-replay.json`,
      text: history.replay_request_json, mime: "application/json" });
  }
}
function steadyHeatBounds(parent, label, result) {
  const steady = result?.steady;
  if (!steady || (!Object.hasOwn(steady, "reported_bounds") && !steady.bounds_unavailable?.length)) return;
  const bounds = steady.reported_bounds?.power_w;
  const t = table(parent, `${label}: steady source-reported heat range`,
    ["Signed nominal power (W)", "Signed reported power range (W)", "Range availability"]);
  cells(t, [metric(steady.power_w), bounds ? `[${bounds.map(metric).join(", ")}]` : "Unknown",
    bounds ? "Complete selected source endpoints" :
      (steady.bounds_unavailable || []).map(reason => reason.message || reason).join("; ") || "No complete source range"]);
  const note = document.createElement("p");
  note.textContent = "These conditional steady ranges retain the exact evaluated geometry, reservoir temperatures, films and fixed interfaces. " +
    "Each consumed constant material law needs complete reported resistance endpoints or thickness and conductivity endpoints. " +
    "Nominal power remains available when ranges are unsupported or incomplete. The ranges do not describe geometric uncertainty, transient heat or measured garment warmth.";
  parent.append(note);
}
function thermalBridges(parent, label, result) {
  const bridges = result?.thermal_bridges;
  if (!bridges) return;
  const totals = table(parent, `${label}: qualified seam and closure heat`,
    ["Baseline power (W)", "Seam/closure excess (W)", "Combined power (W)"]);
  cells(totals, [metric(bridges.baseline_power_w), metric(bridges.bridge_power_w), metric(bridges.total_power_w)]);
  const paths = table(parent, `${label}: current qualified paths`, ["Path", "Instance", "Current length (m)", "Excess power (W)"]);
  for (const path of bridges.paths || []) cells(paths, [path.id, path.instance, metric(path.length_m), metric(path.power_w)]);
  const note = document.createElement("p");
  note.textContent = "Combined power includes each source-qualified seam or closure excess once. " +
    "The unbridged stack remains visible as the baseline. These signed steady rates do not supply transient heat or establish measured garment warmth.";
  parent.append(note);
  detail(parent, `${label}: path qualifications, projected ownership and replay inputs (display only)`, {
    surface_model_sha256: bridges.surface_model_sha256, bridges,
    qualifications: result.request?.thermal_bridges });
}
function coverageResult(parent, result) {
  steadyHeatBounds(parent, "Steady heat", result);
  thermalBridges(parent, "Coverage", result);
  return { steady: result.steady,
    ...(Object.hasOwn(result, "thermal_bridges") ? { thermal_bridges: result.thermal_bridges } : {}),
    comfort_claim: result.comfort_claim, empirical_validation_claim: result.empirical_validation_claim };
}
function motionComparison(op, parent, rows, simulations, requested, filePrefix = "") {
  if (!requested && ![...simulations.values()].some(o => available(o)?.motion)) return;
  const note = document.createElement("p");
  note.textContent = "Motion continues the initial sewn cloth; it does not re-drape each pose. Motion enters the computed frontier through explicitly declared motion objectives or required consumers. " +
    "Endpoint steady snapshots are separate from explicitly requested transient histories. Successful motion is not a settled-garment or empirical fit claim.";
  parent.append(note);
  const t = table(parent, "Prescribed motion — complete trajectory or refusal", ["Alternative", "Motion", "Initial drape settled",
    "Duration (s)", "Accepted substeps", "Attempted substeps", "Peak absolute panel-edge strain (%)"]);
  for (const [index, row] of rows.entries()) {
    const evaluation = simulations.get(row.key), initial = available(evaluation), result = available(initial?.motion);
    cells(t, [row.key, initial ? state(initial.motion) : state(evaluation),
      initial?.drape?.settled === true ? "Yes — also review initial flags" : initial?.drape?.settled === false ? "NO — review initial state" : "Unknown",
      metric(result?.duration_s), metric(result?.samples?.length), metric(result?.trials), metric(result?.metrics?.max_absolute_edge_strain_pct)]);
    materialStrainComparison(parent, row.key, result?.material_strain, initial?.motion || evaluation,
      initial?.material_strain && initial.material_strain.status !== "not_requested", true);
    contactLoadsComparison(parent, row.key, result?.contact_loads, initial?.motion || evaluation,
      initial?.contact_loads && initial.contact_loads.status !== "not_requested", true);
    transientHeatComparison(parent, row.key, result, initial?.motion || evaluation, requested);
    motionVaporComparison(op, parent, row.key, index, result, initial?.motion || evaluation, requested, filePrefix);
    if (!result) continue;
    const endpoints = table(parent, `${row.key}: endpoint steady snapshots — not transient heat`, ["Time (s)", "Arm angle (degrees)", "Leg posture", "Leg fraction",
      "Surface heat", "Signed surface power (W)", "Body coverage", "Signed coverage power (W)"]);
    for (const endpoint of result.keyframes) {
      cells(endpoints, [metric(endpoint.time_s), metric(endpoint.arm_abduction_deg), endpoint.leg_pose?.posture || "standing", endpoint.leg_pose ? metric(endpoint.leg_pose.fraction) : "—", state(endpoint.surface),
        metric(available(endpoint.surface)?.steady?.power_w), state(endpoint.coverage), metric(available(endpoint.coverage)?.steady?.power_w)]);
      steadyHeatBounds(parent, `${row.key}: endpoint ${metric(endpoint.time_s)} s independent sheets`, available(endpoint.surface));
      steadyHeatBounds(parent, `${row.key}: endpoint ${metric(endpoint.time_s)} s body coverage`, available(endpoint.coverage));
      thermalBridges(parent, `${row.key}: endpoint ${metric(endpoint.time_s)} s`, available(endpoint.coverage));
    }
    if (result.keyframes.some(e => e.vapor_surface && e.vapor_surface.status !== "not_requested")) {
      const vapor = table(parent, `${row.key}: endpoint vapor potentials — independent sheets`,
        ["Time (s)", "Vapor transfer", "Current sheet area (m²)", "Potential heat transfer (W)", "Potential mass transfer (kg/s)"]);
      for (const endpoint of result.keyframes) {
        const transfer = available(endpoint.vapor_surface);
        cells(vapor, [metric(endpoint.time_s), state(endpoint.vapor_surface), metric(transfer?.area_m2),
          vaporMetric(transfer, "potential_heat_flow_w"), vaporMetric(transfer, "potential_mass_flow_kg_s")]);
      }
    }
    if (result.keyframes.some(e => e.vapor_coverage && e.vapor_coverage.status !== "not_requested")) {
      const note = document.createElement("p");
      note.textContent = "Layered vapor transfer follows the cloth and body at each endpoint. These are steady potential rates for covered regions; " +
        "endpoint rates are separate from the optional history integral. Neither result models sweat supply or cooling supplied to the dry heat simulation.";
      parent.append(note);
      const vapor = table(parent, `${row.key}: endpoint vapor potentials — covered layers`,
        ["Time (s)", "Vapor coverage", "Covered body (m²)", "Uncovered body (m²)",
          "Potential power (W)", "Potential water flow (kg/s)"]);
      for (const [j, endpoint] of result.keyframes.entries()) {
        const transfer = available(endpoint.vapor_coverage), coverage = transfer?.coverage;
        cells(vapor, [metric(endpoint.time_s), state(endpoint.vapor_coverage), metric(coverage?.covered_area_m2),
          metric(coverage?.uncovered_area_m2), vaporMetric(coverage, "covered_potential_heat_flow_w"),
          vaporMetric(coverage, "covered_potential_mass_flow_kg_s")]);
        detail(parent, `${row.key}: endpoint ${j + 1} vapor layers, sources and boundary conditions (display only)`,
          transfer || { refusal: endpoint.vapor_coverage?.refusal });
        if (typeof transfer?.replay_request_json === "string") {
          const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
          link(op, actions, { name: `${filePrefix}alternative-${index + 1}-motion-endpoint-${j + 1}-vapor-coverage-replay.json`,
            text: transfer.replay_request_json, mime: "application/json" });
        }
      }
    }
    detail(parent, `${row.key}: motion identities, material history and endpoint refusals (display only)`, {
      path_sha256: result.path_sha256, initial_positions_sha256: result.initial_positions_sha256,
      initial_body_collision_sha256: result.initial_body_collision_sha256, initial_drape_gating: result.initial_drape_gating,
      retained_vertex_samples: result.retained_vertex_samples, pairs_tested: result.pairs_tested,
      membrane_history: result.membrane_history, bending_history: result.bending_history,
      membrane_plasticity_history: result.membrane_plasticity_history,
      endpoints: result.keyframes.map(e => ({ time_s: e.time_s, positions_sha256: e.positions_sha256,
        body_collision_sha256: e.body_collision_sha256, surface_refusal: e.surface?.refusal, coverage_refusal: e.coverage?.refusal,
        vapor_refusal: e.vapor_surface?.refusal, vapor_coverage_refusal: e.vapor_coverage?.refusal })),
      scope: result.scope, thermal_basis: result.thermal_basis, vapor_basis: result.vapor_basis });
    motionSamples(parent, row.key, result);
  }
}
function nappyOccupancyResult(parent, occupancy) {
  if (!occupancy) return;
  const t = table(parent, "Nappy geometry proxy — MODEL_FIT",
    ["Target seat over nappy (mm)", "Bare seat (mm)", "Occupied seat (mm)", "Occupied − target (mm)", "Supplied σ (mm)"]);
  cells(t, [metric(occupancy.target_mm), metric(occupancy.bare_seat_mm), metric(occupancy.achieved_seat_mm),
    metric(occupancy.residual_mm), occupancy.sigma_mm == null ? "Not supplied" : metric(occupancy.sigma_mm)]);
  const flags = Array.isArray(occupancy.target_source?.flags)
    ? occupancy.target_source.flags.filter(flag => typeof flag === "string") : [];
  const evidence = document.createElement("p");
  evidence.textContent = `Target evidence: ${occupancy.target_source?.primary_provenance || "Not supplied"}` +
    (flags.length ? `; ${flags.join(", ")}` : "") + ".";
  parent.append(evidence);
  const note = document.createElement("p");
  note.textContent = "Calibrated once in the fixed reference pose and reused for contact, display and movement. " +
    "Bare body parameters, model mass and fitting evidence stay unchanged. The target retains its source flags; " +
    "a chart target remains a prior. This geometry proxy supplies no nappy mass, material properties or physical-fit evidence.";
  parent.append(note);
  detail(parent, "Nappy target evidence and occupied geometry identity (display only)", occupancy);
}
function comparison(op, r, parent = sections.get(op).comparison, { skipManufacturing = false, filePrefix = "", casePolicies = {} } = {}) {
  if (op === "screen") {
    screening(parent, r);
    return { schema: r.schema, basis: r.basis, allow_mixed_protocols: r.allow_mixed_protocols,
      scalar_frontier: r.frontier, request_sha256: r.request_sha256, notes: r.notes };
  }
  if (["ftx-material-scenario-matrix-study/1", "ftx-design-scenario-matrix-study/1"].includes(r.schema)) {
    return scenarioMatrixComparison(op, parent, r);
  }
  if (r.comparison) objectiveComparison(parent, r.comparison);
  const study = r.manufacturing || r;
  if (!skipManufacturing) manufacturingComparison(op, parent, study, r.comparison, casePolicies);
  if (r.manufacturing) {
    nappyOccupancyResult(parent, r.scenario?.body_fit?.nappy_occupancy);
    const simulations = new Map(r.simulations.map(row => [row.key, row.evaluation]));
    const sim = table(parent, "Initial static-drape results — review drape flags before interpreting heat", ["Alternative", "Drape",
      "Gating clear", "Cloth mass (kg)", "Surface heat", "Signed surface power (W)", "Body coverage", "Signed coverage power (W)"]);
    for (const row of study.alternatives) {
      const outcome = simulations.get(row.key), result = available(outcome), m = result?.metrics || {};
      cells(sim, [row.key, state(outcome), m.simulation_gating_clear === true ? "Yes" :
        m.simulation_gating_clear === false ? "NO — review flags" : "Unknown", metric(m.cloth_mass_kg),
        state(result?.surface), metric(m.independent_surface_power_w), state(result?.coverage), metric(m.body_coverage_power_w)]);
      detail(parent, `${row.key}: simulation flags, identities and thermal refusals`, {
        refusal: outcome?.refusal, settled: result?.drape?.settled, flags: result?.drape?.flags,
        scenario_sha256: result?.scenario_sha256, body_geometry_sha256: result?.body_geometry_sha256,
        placements_sha256: result?.placements_sha256, positions_sha256: result?.positions_sha256,
        surface_refusal: result?.surface?.refusal, coverage_refusal: result?.coverage?.refusal, note: result?.note });
      steadyHeatBounds(parent, `${row.key}: initial independent sheets`, available(result?.surface));
      steadyHeatBounds(parent, `${row.key}: initial body coverage`, available(result?.coverage));
      thermalBridges(parent, `${row.key}: initial coverage`, available(result?.coverage));
      materialStrainComparison(parent, row.key, result?.material_strain, outcome, r.scenario?.scenario?.material_strain);
      contactLoadsComparison(parent, row.key, result?.contact_loads, outcome, r.scenario?.scenario?.contact_loads);
    }
    vaporCoverageComparison(op, parent, study.alternatives, simulations, r.scenario?.scenario?.vapor_coverage, filePrefix);
    if ([...simulations.values()].some(o => {
      const vapor = available(o)?.vapor_surface;
      return vapor && vapor.status !== "not_requested";
    })) {
      const vapor = table(parent, "Initial drape vapor potentials — independent sheets",
        ["Alternative", "Vapor transfer", "Current sheet area (m²)", "Potential heat transfer (W)", "Potential mass transfer (kg/s)"]);
      for (const row of study.alternatives) {
        const outcome = simulations.get(row.key), initial = available(outcome), transfer = available(initial?.vapor_surface);
        cells(vapor, [row.key, initial ? state(initial.vapor_surface) : state(outcome), metric(transfer?.area_m2),
          vaporMetric(transfer, "potential_heat_flow_w"), vaporMetric(transfer, "potential_mass_flow_kg_s")]);
        vaporBoundsNote(parent, transfer, `${row.key}: independent-sheet vapor bounds`);
        detail(parent, `${row.key}: vapor conditions, source columns and refusal (display only)`,
          transfer || { refusal: initial?.vapor_surface?.refusal || outcome?.refusal });
      }
    }
    attachedMassComparison(parent, study.alternatives, simulations);
    motionComparison(op, parent, study.alternatives, simulations, r.scenario?.scenario?.motion, filePrefix);
    detail(parent, "Retained body fit, scenario options and environment (display only)", r.scenario);
  }
  return { schema: r.schema, source_pattern_sha256: study.source_pattern_sha256, request_sha256: r.request_sha256,
    geometry_changed: study.geometry_changed, physical_validation_claim: study.physical_validation_claim,
    hardware_execution_authorized: study.hardware_execution_authorized, notes: study.notes,
    computed_frontier: r.comparison?.frontier ?? null,
    comparison_basis: r.comparison ? (r.comparison.policy.basis === "reported_bounds" ?
      "Complete source-reported ranges evaluated by Rust at fixed declared inputs; unresolved overlap and unknown ranges remain explicit." :
      "Declared nominal objectives evaluated by Rust; conditional Pareto set, not an empirical fit/warmth claim or global optimum.") :
      "Actual consumer results; no computed objectives requested. Unknowns are not zero, and no ranking is inferred by this page." };
}
function scenarioMatrixComparison(op, parent, r) {
  const note = document.createElement("p");
  note.textContent = "Named scenarios share the same original pattern, declared design and material alternatives, and body source. " +
    "Manufacturing is evaluated once; each case starts a fresh drape with its own conditions and optional motion. " +
    "Case decisions and physical units remain separate. No browser score or average replaces the native comparison.";
  parent.append(note);
  if (r.comparison) {
    const result = r.comparison, frontier = document.createElement("p");
    frontier.textContent = `Computed cross-case frontier: ${result.frontier.length ? result.frontier.join(", ") : "No eligible non-dominated alternatives"}. ` +
      "Admission requires every case's declared gates and limits. Review each case's retained readings and exclusions below.";
    parent.append(frontier);
    const t = table(parent, "Named scenario decisions — authoritative Rust comparison",
      ["Alternative", "Across cases", "Dominated by", "Case", "Case admission", "Retained readings"]);
    for (const row of result.alternatives) {
      for (const key of result.case_order) {
        const candidate = row.cases[key], policy = result.policies[key];
        const quantities = [...(policy?.objectives || []), ...(policy?.constraints || [])]
          .filter((q, i, all) => all.findIndex(other => other.metric === q.metric) === i);
        const readings = quantities.map(({ metric: name }) => {
          const q = candidate?.readings?.[name];
          if (!q) return `${name}: Unknown`;
          const bounds = q.reported_bounds ? `; reported range [${q.reported_bounds.map(metric).join(", ")}] ${q.unit}` :
            metricUsesBounds(policy, name) ? "; reported range unknown" : "";
          return `${name}: ${metric(q.value)} ${q.unit}${bounds}`;
        });
        cells(t, [row.key, row.eligible ? "Eligible" : "Excluded", row.dominated_by.length ? row.dominated_by.join(", ") : "None reported",
          key, candidate ? (candidate.eligible ? "Eligible" : `Excluded: ${candidate.exclusions.map(e => e.message).join("; ")}`) : "Unknown",
          readings.length ? readings.join(" | ") : "No declared quantities"]);
      }
    }
    for (const pair of result.incomparable) {
      const p = document.createElement("p");
      p.textContent = `${pair.alternatives.join(" / ")}: ${pair.reason} (cases: ${pair.cases.join(", ")}).`;
      parent.append(p);
    }
    detail(parent, "Cross-case policies, identities and decisions (display only)", result);
  } else {
    const unavailable = document.createElement("p");
    unavailable.textContent = "No cross-case comparison is available. Comparison policies are required for every case; " +
      "available per-case comparisons and independent outcomes remain below.";
    parent.append(unavailable);
  }
  for (const message of r.notes || []) {
    const note = document.createElement("p"); note.textContent = message; parent.append(note);
  }
  const shared = document.createElement("section"), heading = document.createElement("h4");
  heading.textContent = "Shared manufacturing — evaluated once"; shared.append(heading); parent.append(shared);
  const casePolicies = r.comparison?.policies || Object.fromEntries(r.cases
    .filter(item => item.comparison?.policy).map(item => [item.key, item.comparison.policy]));
  comparison(op, r.manufacturing, shared, { casePolicies });
  for (const [index, item] of r.cases.entries()) {
    const group = document.createElement("section"), heading = document.createElement("h4");
    heading.textContent = `Case ${index + 1}: ${item.key}`; group.append(heading); parent.append(group);
    comparison(op, { ...item, schema: r.schema === "ftx-design-scenario-matrix-study/1" ?
      "ftx-design-scenario-study/1" : "ftx-material-scenario-study/1", manufacturing: r.manufacturing }, group,
      { skipManufacturing: true, filePrefix: `case-${index + 1}-` });
    detail(group, "Case request identity (display only)", { key: item.key, request_sha256: item.request_sha256 });
  }
  return { schema: r.schema, request_sha256: r.request_sha256, source_pattern_sha256: r.manufacturing.source_pattern_sha256,
    cases: r.cases.map(item => item.key), max_evaluations: r.max_evaluations,
    planned_evaluations: r.planned_evaluations, evaluations: r.evaluations,
    computed_cross_case_frontier: r.comparison?.frontier ?? null,
    physical_validation_claim: r.manufacturing.physical_validation_claim,
    scope: "Fixed-input named cases, with their native outcomes, refusals and policies retained independently." };
}
function designTreatments(op, parent, design, prefix, label) {
  if (!Array.isArray(design?.treatments) || !design.treatments.length) return;
  const note = document.createElement("p");
  note.textContent = "These are exact intermediate treatment stages. Review final geometry, fit and material binding admission before using a design. " +
    "Completed stages can remain available for inspection if a later operation refuses.";
  parent.append(note);
  for (const stage of design.treatments) {
    const step = stage.operation_index + 1, report = stage.report;
    const heading = document.createElement("h4");
    heading.textContent = `${label}: operation ${step} — ${report.mode === "compensate" ? "Cutting compensation" : "Material treatment"}`;
    parent.append(heading);
    treatmentResult(parent, report);
    const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
    for (const [suffix, field] of [["before-pattern", "source_pattern_json"], ["after-pattern", "pattern_json"], ["request", "frozen_request_json"]]) {
      if (typeof report[field] === "string") link(op, actions, { name: `${prefix}-treatment-${step}-${suffix}.json`,
        text: report[field], mime: "application/json" });
    }
  }
}
function designComparison(op, parent, study, casePolicies = {}) {
  if (!study.alternatives.some(row => row.design)) return;
  const note = document.createElement("p");
  note.textContent = "Each pattern design starts from the original source, then applies its explicit alterations and material choices. " +
    "The baseline retains the source geometry. Cutting, sewing and requested simulations use each candidate's actual pattern. " +
    "Review alteration validation and consumer refusals before interpreting the computed comparison.";
  parent.append(note);
  const grid = study.design_search;
  if (grid) {
    if (grid.schema === "ftx-design-adaptive-search/1") {
      const guidance = grid.consumer_guidance;
      const pareto = guidance?.pareto_frontier;
      const matrixPareto = pareto && guidance.case_aggregation === "pareto";
      const policies = matrixPareto ? (guidance.case_order || []).map(key =>
        ({ key, policy: guidance.case_policies?.[key] })) : [];
      const objectives = pareto ? guidance.objectives || [] : guidance?.objective ? [guidance.objective] : [];
      const policyFor = key => policies.find(item => item.key === key)?.policy || casePolicies[key];
      const basisText = basis => basis === "reported_bounds" ? "Source-reported ranges" :
        basis === "nominal" ? "Nominal values" : "Unknown";
      const bounded = guidance?.basis === "reported_bounds";
      const worstCase = guidance?.case_aggregation === "worst_case";
      const limitingCases = (nominal, endpoints) => nominal ?
        `; nominal limiting case: ${nominal}` + (endpoints ?
          `; lower endpoint case: ${endpoints[0]}; upper endpoint case: ${endpoints[1]}` : "") : "";
      const readingText = (reading, needsBounds = bounded) => {
        if (!reading) return "Unknown";
        const range = reading.reported_bounds ?
          `; reported range [${reading.reported_bounds.map(metric).join(", ")}] ${reading.unit || ""}` :
          needsBounds ? "; reported range unknown" : "";
        return `${metric(reading.value)} ${reading.unit || ""}${range}`;
      };
      if (guidance) {
        const search = table(parent, matrixPareto ? "Multi-case Pareto adaptive alteration search" :
          pareto ? "Pareto-guided adaptive alteration search" : "Consumer-guided adaptive alteration search", ["Fit evaluations", "Consumer probes",
          "Planned callback slots (upper bound)", "Selected geometries", "Stop reason",
          ...(pareto ? ["Evaluated nondominated trials", "Retained trials", "Omitted by capacity"] :
            [bounded ? "Retained trial" : "Best trial", bounded ? "Retained objective" : "Best objective"])]);
        const reading = guidance.best_reading;
        cells(search, [metric(grid.evaluations), metric(guidance.evaluations), metric(guidance.reserved_evaluations),
          metric(grid.selected_designs), grid.stop_reason?.replaceAll("_", " ") ?? "Unknown",
          ...(pareto ? [(pareto.evaluated_trial_keys || []).join(", ") || "None admitted",
            (pareto.retained_trial_keys || []).join(", ") || "None selected",
            (pareto.unretained_trial_keys || []).join(", ") || "None"] :
            [guidance.best_trial_key ?? "Unknown", readingText(reading) +
              (worstCase && reading ? limitingCases(guidance.best_case_key, guidance.best_reported_bound_case_keys) : "")])]);
        const objective = document.createElement("p");
        objective.textContent = `Guidance material: ${guidance.material_key ?? "Unknown"}. ` +
          (guidance.case_key ? `Guidance case: ${guidance.case_key}. Every declared case must admit a selected trial: ${(guidance.case_order || []).join(", ")}. ` : "") +
          (worstCase ? `Case aggregation: worst case. Every declared case must admit a selected trial: ${(guidance.case_order || []).join(", ")}. ` +
            (bounded ? "Reported lower and upper endpoints each retain their own limiting case; they may differ from the nominal limiting case. " : "") : "") +
          (matrixPareto ? `Guidance covers every named case: ${(guidance.case_order || []).join(", ")}. ` +
            "Every declared case must admit a selected trial. Each case keeps its own objectives, directions, comparison basis and source protocols. " +
            "Dominance requires no worse outcomes in every case and a strict improvement in at least one case. " :
            pareto ? `Declared objectives: ${objectives.map(item => `${item.metric} (${item.direction})`).join("; ") || "Unknown"}. ` :
              `Declared objective: ${guidance.objective?.metric ?? "Unknown"} (${guidance.objective?.direction ?? "Unknown"}). `) +
          (pareto ? `Retention capacity: ${metric(pareto.retention_capacity)} geometries. ` +
            "Polling also explores incomparable admitted designs, including those omitted from the output cache. " +
            "The displayed frontier covers evaluated trials only; the retained subset is not a complete frontier or a single winner. " +
            "No weighted score is introduced. " : "") +
          (matrixPareto ? "Objective bases are declared separately for each case below. Missing required ranges cannot qualify; nominal readings do not supply missing endpoints. " :
            bounded ? "Objective basis: source-reported ranges. Bounded constraints admit the entire reported range; missing required ranges cannot qualify. " +
              (pareto ? "Range comparisons retain every declared objective and direction; incomparable ranges do not acquire a total ranking. " :
                "Search advances only when one range is strictly preferred. Overlapping, touching or equal ranges remain unresolved; retained trials do not form a total ranking. ") :
              "Objective basis: nominal values. ") +
          "Each constraint uses its explicit basis or inherits its case policy; nominal guards do not acquire uncertainty bounds. " +
          "Rust evaluates the nominated material with the ordinary manufacturing and requested scenario consumers. " +
          "Existing source admission, required consumers, protocol rules and inclusive constraints govern each probe. " +
          "Callback slots are an upper bound on attempted work, not a preapproved simulation allowance; cumulative consumer budgets still apply.";
        parent.append(objective);
        if (matrixPareto) {
          const cases = table(parent, "Adaptive guidance policies by operating case",
            ["Case", "Objective basis", "Objectives and directions", "Inclusive constraints", "Required consumers", "Protocol comparison"]);
          for (const { key, policy } of policies) {
            cells(cases, [key, basisText(policy.basis),
              policy.objectives.map(item => `${item.metric} (${item.direction})`).join("; "),
              (policy.constraints || []).map(item => `${item.metric}: minimum ${item.minimum == null ? "Unbounded" : metric(item.minimum)}; ` +
                `maximum ${item.maximum == null ? "Unbounded" : metric(item.maximum)} (${basisText(item.basis || policy.basis)})`).join("; ") || "None",
              (policy.required_consumers || []).join(", ") || "Objective consumers",
              policy.allow_mixed_protocols ? "Mixed protocols explicitly admitted" : "Matching case protocols required"]);
            detail(parent, `${key}: complete adaptive guidance policy (display only)`, policy);
          }
        }
      } else {
        const search = table(parent, "Fit-directed adaptive alteration search", ["Fit evaluations", "Passed flat gates",
          "Selected for consumers", "Stop reason", "Best maximum violation (mm)", "Best total violation (mm)"]);
        cells(search, [metric(grid.evaluations), metric(grid.qualified_designs), metric(grid.selected_designs),
          grid.stop_reason?.replaceAll("_", " ") ?? "Unknown", metric(grid.best_max_fit_band_violation_mm),
          metric(grid.best_total_fit_band_violation_mm)]);
      }
      const scope = document.createElement("p");
      scope.textContent = (guidance ?
        "Flat geometry and selected-body fit gates precede every consumer probe. The search keeps better admitted geometries while polling the declared domain; " +
        "all trials remain inspectable, including displaced candidates and failed probes. Selected geometries are then crossed with the authored material choices. " :
        "The adaptive search uses unrounded distances outside your declared fit bands to choose the next alteration values. ") +
        "Its bounds and step sizes define where it can search; the stopping reason does not prove that unvisited values are infeasible " +
        "or that a global optimum was found. Selected designs use the ordinary manufacturing and scenario consumers, and the original baseline remains visible. " +
        "Computed target compliance does not establish physical garment fit.";
      parent.append(scope);
      const trials = table(parent, "Adaptive alteration trials", ["Pattern design", "Variables (mm)",
        "Maximum violation (mm)", "Total violation (mm)", "Flat admission",
        ...(guidance ? ["Consumer admission", pareto ? "Declared objectives" : "Declared objective",
          ...(pareto ? ["Evaluated frontier"] : [])] : []), "Downstream selection", "Reason"]);
      for (const [i, row] of (grid.rows || []).entries()) {
        const design = row.design || {}, fit = available(design.fit);
        const passed = design.validation?.passed === true && fit?.passed === true;
        const candidate = row.consumer_comparison;
        const reading = worstCase && bounded ? row.consumer_worst_case?.reading :
          candidate?.readings?.[guidance?.objective?.metric];
        const caseDecisions = row.consumer_cases || [];
        const attempted = matrixPareto ? caseDecisions.length > 0 : !!candidate;
        const admitted = !row.refusal && (matrixPareto ?
          policies.length > 0 && policies.every(policy => caseDecisions.some(item =>
            item.key === policy.key && item.comparison?.eligible === true)) :
          candidate?.eligible === true && caseDecisions.every(item => item.comparison?.eligible === true));
        const reasons = [row.refusal?.message, row.consumer_refusal?.message,
          ...(candidate?.exclusions || []).map(e => e.message),
          ...(matrixPareto ? caseDecisions.flatMap(item => [
            ...(item.comparison?.exclusions || []).map(e => `${item.key}: ${e.message}`),
            item.refusal?.message ? `${item.key}: ${item.refusal.message}` : null]) : [])]
          .filter((message, index, all) => message && all.indexOf(message) === index).join("; ");
        cells(trials, [design.key ?? "Unknown",
          design.variables_mm ? Object.entries(design.variables_mm).map(([key, value]) => `${key}: ${metric(value)}`).join("; ") : "Unknown",
          metric(row.max_fit_band_violation_mm), metric(row.total_fit_band_violation_mm),
          passed ? "Passed declared flat gates" : "Refused",
          ...(guidance ? [attempted ? (admitted ? "Eligible" : "Excluded") : "Not attempted",
            matrixPareto ? policies.map(({ key, policy }) => {
              const decision = caseDecisions.find(item => item.key === key);
              return `${key} [${basisText(policy.basis)}]: ` + policy.objectives.map(item =>
                `${item.metric} (${item.direction}): ${readingText(decision?.comparison?.readings?.[item.metric], policy.basis === "reported_bounds")}`).join("; ");
            }).join(" | ") || "Unknown" :
              pareto ? objectives.map(item => `${item.metric}: ${readingText(candidate?.readings?.[item.metric])}`).join("; ") || "Unknown" :
                readingText(reading) + (worstCase && reading ?
                limitingCases(row.consumer_case_key, row.consumer_worst_case?.reported_bound_case_keys) : ""),
            ...(pareto ? [admitted ? ((pareto.evaluated_trial_keys || []).includes(design.key) ?
              "Nondominated among evaluated trials" : "Dominated among evaluated trials") : "Not admitted"] : [])] : []),
          row.selected === true ? "Selected" : "Not selected",
          reasons || (passed ? "Passed declared targets" : "Inspect retained validation")]);
        if (row.consumer_cases?.length) {
          const cases = table(parent, `${design.key ?? "Unknown design"}: operating-case admission`,
            ["Case", "Admission", "Readings", "Exclusions or consumer refusal"]);
          for (const item of row.consumer_cases) cells(cases, [item.key,
            item.comparison?.eligible === true ? "Eligible under declared policy" : "Excluded",
            Object.entries(item.comparison?.readings || {}).map(([key, value]) =>
              `${key}: ${readingText(value, policyFor(item.key) ? metricUsesBounds(policyFor(item.key), key) :
                bounded && objectives.some(item => item.metric === key))}`).join("; ") || "Unavailable",
            [...(item.comparison?.exclusions || []).map(reason => reason.message), item.refusal?.message]
              .filter(Boolean).join("; ") || "None"]);
        }
        if (typeof design.altered_pattern_json === "string") {
          const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
          link(op, actions, {name: `adaptive-design-${i + 1}-pattern.json`, text: design.altered_pattern_json, mime: "application/json"});
        }
        designTreatments(op, parent, design, `adaptive-design-${i + 1}`, design.key ?? "Unknown design");
        detail(parent, `${design.key ?? "Unknown design"}: adaptive trial, unrounded fit and body identity (display only)`, row);
      }
    } else if (grid.fit_screening) {
      const screening = grid.fit_screening;
      const search = table(parent, "Fit screening before manufacturing and simulation",
        ["Declared grid points", "Flat screened", "Passed flat gates", "Rejected", "Selected for consumers",
          "Passing designs not selected", "Unvisited grid points"]);
      cells(search, [metric(grid.total_grid_points), metric(screening.screened_designs), metric(screening.qualified_designs),
        metric(screening.rejected_designs), metric(screening.selected_designs), metric(screening.qualified_designs_omitted),
        metric(screening.unvisited_designs)]);
      const scope = document.createElement("p");
      scope.textContent = (screening.complete_fit_screening === true ?
        "Every declared grid point received flat geometry and fit screening. " :
        "The flat-screening allowance left unvisited grid points; their feasibility remains unknown. ") +
        (screening.all_qualified_designs_selected === true ?
          "Every passing screened design was selected for the requested consumers. " :
          "Some passing screened designs were not selected for manufacturing or simulation; their downstream results remain unknown. ") +
        "The original baseline remains visible. Flat fit targets do not establish physical garment fit or a continuous or global optimum.";
      parent.append(scope);
      const screened = table(parent, "Screened garment designs",
        ["Pattern design", "Variables (mm)", "Flat admission", "Downstream selection", "Reason"]);
      for (const [i, row] of (screening.rows || []).entries()) {
        const design = row.design || {}, fit = available(design.fit);
        const passed = design.validation?.passed === true && fit?.passed === true;
        cells(screened, [design.key ?? "Unknown",
          design.variables_mm ? Object.entries(design.variables_mm).map(([key, value]) => `${key}: ${metric(value)}`).join("; ") : "Unknown",
          passed ? "Passed declared flat gates" : "Refused",
          row.selected === true ? "Selected" : "Not selected",
          row.refusal?.message || (passed ? "Passed declared targets" : "Inspect retained validation")]);
        if (typeof design.altered_pattern_json === "string") {
          const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
          link(op, actions, {name: `screened-design-${i + 1}-pattern.json`, text: design.altered_pattern_json, mime: "application/json"});
        }
        designTreatments(op, parent, design, `screened-design-${i + 1}`, design.key ?? "Unknown design");
        detail(parent, `${design.key ?? "Unknown design"}: screened operations, body identity and fit evidence (display only)`, row);
      }
    } else {
      const search = table(parent, "Declared alteration search",
        ["Generated designs", "Declared grid points", "Omitted designs", "Enumeration"]);
      cells(search, [metric(grid.generated_designs), metric(grid.total_grid_points), metric(grid.omitted_designs),
        grid.complete_grid_enumeration === true ? "Complete declared grid" :
          grid.budget_exhausted === true ? "Stopped at design budget" : "Incomplete"]);
      const scope = document.createElement("p");
      scope.textContent = grid.complete_grid_enumeration === true ?
        "Every declared grid point was generated. Candidate validation and consumer results still govern eligibility; this is not a continuous or global optimum." :
        "The frontier is conditional on visited designs. Omitted grid points remain unknown and may contain better feasible designs.";
      parent.append(scope);
    }
    detail(parent, grid.schema === "ftx-design-adaptive-search/1" ?
      "Exact adaptive bounds, linked sites, steps and search allowance (display only)" :
      "Exact alteration grid, linked sites and search allowance (display only)", grid);
  }
  const t = table(parent, "Pattern designs and material choices",
    ["Alternative", "Pattern design", "Material choice", "Alteration operations", "Geometry"]);
  const fitTable = study.alternatives.some(row => row.design?.fit || row.design?.variables_mm) ?
    table(parent, "Design variables and declared fit targets",
      ["Alternative", "Variables (mm)", "Body measurement basis", "Fit admission"]) : null;
  for (const [i, row] of study.alternatives.entries()) {
    const design = row.design, fit = available(design?.fit);
    cells(t, [row.key, design?.key ?? "Unknown", design?.material_key ?? "Unknown",
      Array.isArray(design?.operations) ? String(design.operations.length) : "Unknown",
      design?.geometry_changed === true ? "Changed" : design?.geometry_changed === false ? "Unchanged" : "Unavailable"]);
    if (!design) continue;
    if (fitTable) cells(fitTable, [row.key,
      design.variables_mm ? Object.entries(design.variables_mm).map(([key, value]) => key + ": " + metric(value)).join("; ") :
        design.key === "baseline" ? "Original baseline" : "Not parameterized",
      fit?.body_identity?.provenance || (design.fit ? "Unavailable" : "Not requested"),
      !design.fit ? "Not requested" : !fit ? state(design.fit) :
        fit.passed === true ? "Passed declared targets" : "Targets not met"]);
    if (fit) {
      const points = table(parent, row.key + ": canonical flat fit measurements",
        ["Point", "Body (mm)", "Finished (mm)", "Ease (mm)", "Target min / nominal / max (mm)", "Status", "Residual (mm)"]);
      for (const point of Array.isArray(fit.validation?.points) ? fit.validation.points : []) {
        cells(points, [point.label ?? point.id ?? "Unknown", metric(point.body_mm), metric(point.finished_mm),
          metric(point.ease_mm), Array.isArray(point.target_mm) ? point.target_mm.map(value => metric(value)).join(" / ") : "Unknown",
          point.status ?? "Unknown", metric(point.residual_mm)]);
      }
      detail(parent, row.key + ": selected body identity and complete flat fit validation (display only)", fit);
    }
    if (typeof design.altered_pattern_json === "string") {
      const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
      link(op, actions, { name: `alternative-${i + 1}-design-pattern.json`,
        text: design.altered_pattern_json, mime: "application/json" });
    }
    designTreatments(op, parent, design, `alternative-${i + 1}`, row.key);
    detail(parent, `${row.key}: exact alteration inputs, geometry identities and validation (display only)`, {
      key: design.key, material_key: design.material_key, operations: design.operations, variables_mm: design.variables_mm,
      source_pattern_sha256: design.source_pattern_sha256, altered_pattern_sha256: design.altered_pattern_sha256,
      geometry_changed: design.geometry_changed, validation: design.validation, fit: design.fit, treatments: design.treatments,
      refusal: row.binding?.refusal });
  }
}
function useStudyPattern(op, study, row) {
  if (!ready || pending || sections.get(op).study !== study) return;
  const raw = available(row.binding)?.pattern_json;
  if (typeof raw !== "string") return;
  try {
    installDerivedPattern(raw, `Study candidate ${row.key}`);
    $("status").textContent = "Candidate pattern loaded with its exact material selections. Review the current body, fit and process inputs, then Freeze selections and bind to recompute consumers. Study alterations now start from this candidate.";
    controls();
  } catch (e) { error(String(e.message || e)); controls(); }
}
function manufacturingComparison(op, parent, study, computed, casePolicies = {}) {
  sections.get(op).study = study;
  designComparison(op, parent, study, casePolicies);
  if (study.alternatives.some(row => typeof available(row.binding)?.pattern_json === "string")) {
    const note = document.createElement("p");
    note.textContent = "Use a candidate pattern to continue with its exact evaluated geometry and material assignments. " +
      "Review its refusals and current body, fit and process inputs before binding and recomputing consumers. Selection does not carry forward a fit verdict or choose a comparison winner.";
    parent.append(note);
  }
  if (study.alternatives.some(row => row.choices)) {
    const choices = table(parent, "Material combinations evaluated by the study", ["Alternative", "Group choices"]);
    for (const row of study.alternatives) cells(choices, [row.key,
      Object.entries(row.choices || {}).map(([group, choice]) => `${group}: ${choice}`).join("; ")]);
  }
  const t = table(parent, "Independent manufacturing outcomes", ["Alternative", "Binding", "Cutting", "Sewing", "Purchasing",
    "Stock area (m²)", "Cut area (m²)", "Complete stock mass (kg)", "Proven least order cost", "Stitch intervals", "Feed-only seconds"]);
  for (const [i, row] of study.alternatives.entries()) {
    const m = row.metrics || {}, purchase = available(row.purchasing);
    const cost = metric(m.least_observed_independent_order_total);
    cells(t, [row.key, state(row.binding), state(row.cutting), state(row.sewing),
      purchase?.complete_offer_coverage === false ? "Incomplete offer coverage" : state(row.purchasing),
      metric(m.stock_area_m2), metric(m.external_cut_area_m2), metric(m.stock_mass_kg),
      cost === "Unknown" ? cost : `${cost} ${purchase?.currency || "(currency unavailable)"}`,
      metric(m.stitch_intervals), metric(m.feed_seconds)]);
    const binding = available(row.binding);
    const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
    if (typeof binding?.pattern_json === "string") {
      link(op, actions, { name: `alternative-${i + 1}-pattern.json`, text: binding.pattern_json, mime: "application/json" });
      const button = document.createElement("button"); button.type = "button";
      button.textContent = `Use ${row.key} pattern`;
      button.onclick = () => useStudyPattern(op, study, row);
      sections.get(op).adoptions.push(button); actions.append(button);
    }
    detail(parent, `${row.key}: frozen assignments and source observations (display only)`, {
      pattern_sha256: binding?.pattern_sha256, choices: row.choices, frozen_changes: row.frozen_changes, pieces: binding?.pieces,
      refusals: { binding: row.binding.refusal, cutting: row.cutting.refusal, finite_stock: row.finite_stock?.refusal,
        cutting_job: row.cutting_job?.refusal, sewing: row.sewing.refusal, purchasing: row.purchasing.refusal } });
    if (purchase?.schema === "ftx-cut-procurement/3") {
      splitProcurementResult(parent, purchase, `${row.key}: whole-marker supplier allocation`);
    }
    sewingWorkholding(parent, available(row.sewing), row.key);
    sewingHandling(parent, available(row.sewing), row.key);
    sewingParallel(parent, available(row.sewing), row.key);
    sewingRouting(parent, available(row.sewing), row.key);
    sewingAccess(parent, available(row.sewing), row.key);
    knifeProgramResult(parent, available(row.cutting)?.program, row.key, available(row.cutting)?.program_refusal);
  }
  if (study.alternatives.some(row => row.finite_stock && row.finite_stock.status !== "not_requested")) {
    const stock = table(parent, "Finite hides and remnants — used physical inventory", ["Alternative", "Finite stock",
      "Used items", "Used gross area (m²)", "Allocated cut area (m²)", "Complete used-stock mass (kg)"]);
    for (const row of study.alternatives) {
      const m = row.metrics || {};
      cells(stock, [row.key, state(row.finite_stock), metric(m.used_stock_count), metric(m.used_stock_gross_area_m2),
        metric(m.finite_stock_external_cut_area_m2), metric(m.used_stock_mass_kg)]);
      const plan = available(row.finite_stock);
      knifeProgramResult(parent, plan?.program, `${row.key}: finite stock`, plan?.program_refusal);
      if (plan) detail(parent, `${row.key}: physical stock, defects and allocated pieces (display only)`, plan);
    }
  }
  sewingSelection(parent, study.alternatives);
  manufacturingSewingMetrics(op, parent, study, computed);
}
function sewingSelection(parent, alternatives) {
  if (alternatives.some(row => row.sewing_selection && row.sewing_selection.status !== "not_requested")) {
    const routeSearch = alternatives.some(row => {
      const result = available(row.sewing_selection);
      return result?.request?.schema === "ftx-sewing-selection/6" || result?.selected_route != null;
    });
    const selection = table(parent, routeSearch ? "Material-specific sewing recipe and route search" : "Material-specific sewing recipe search",
      ["Alternative", "Status", ...(routeSearch ? ["Chosen route"] : []), "Chosen recipes",
      "Trials evaluated", "Search complete", "Budget exhausted", "Listed optimum proved", "Unresolved trials"]);
    for (const row of alternatives) {
      const plan = available(row.sewing_selection);
      const choices = plan?.selected_choices ? Object.entries(plan.selected_choices)
        .map(([operation, recipe]) => `${operation}: ${recipe}`).join("; ") :
        plan ? "No listed recipe selected" : "Unavailable";
      cells(selection, [row.key, state(row.sewing_selection),
        ...(routeSearch ? [plan?.selected_route ?? (plan ? "No route selected" : "Unavailable")] : []),
        choices || "Original recipes", metric(plan?.trials?.length),
        plan ? String(plan.search_complete) : "Unknown", plan ? String(plan.budget_exhausted) : "Unknown",
        plan ? String(plan.optimality_proven) : "Unknown", metric(plan?.unresolved_trials)]);
      detail(parent, `${row.key}: sewing recipe choices, work limits and refusals (display only)`,
        plan || { refusal: row.sewing_selection?.refusal });
    }
    const costSearches = alternatives.filter(row => {
      const result = available(row.sewing_selection);
      return result?.request?.objective === "minimize_thread_observed_order_cost" ||
        result?.objective_currency != null;
    });
    if (costSearches.length) {
      const costs = table(parent, "Thread-cost-directed recipe search", ["Alternative",
        "Selected complete thread order cost", "Currency", "Listed cost optimum proved", "Unresolved trials"]);
      for (const row of costSearches) {
        const result = available(row.sewing_selection);
        cells(costs, [row.key, metric(result.selected_thread_order_cost), result.objective_currency ?? "Unknown",
          String(result.optimality_proven), metric(result.unresolved_trials)]);
      }
      const routedCosts = costSearches.filter(row => {
        const result = available(row.sewing_selection);
        return result.request?.schema === "ftx-sewing-selection/6" || result.selected_route != null;
      });
      if (routedCosts.length) {
        const trials = table(parent, "Complete thread order costs by recipe and route",
          ["Alternative", "Route", "Recipes", "Complete order cost", "Currency", "Cost availability", "Deadline admission", "Reason"]);
        for (const row of routedCosts) {
          const result = available(row.sewing_selection);
          for (const trial of result.trials || []) {
            const deadline = trial.completion_deadline;
            const cost = trial.refusal ? "Plan refused" :
              trial.objective_unavailable || trial.thread_order_cost == null ? "Unresolved" : "Complete order";
            const admission = deadline ?
              ({ feasible: "Meets deadline", excluded: "Misses deadline (proved)", unresolved: "Unresolved" }[deadline.status] || "Unknown") :
              result.request?.completion_deadline_milliseconds != null ? "Unknown" : "Not requested";
            const reasons = [trial.refusal?.message, trial.objective_unavailable?.message, deadline?.reason]
              .filter((message, index, all) => message && all.indexOf(message) === index).join("; ");
            cells(trials, [row.key, trial.route_choice ?? "Unknown",
              Object.entries(trial.choices || {}).map(([operation, recipe]) => `${operation}: ${recipe}`).join("; ") || "Original recipes",
              metric(trial.thread_order_cost), result.objective_currency ?? "Unknown", cost, admission,
              reasons || "Inspect retained source-qualified order"]);
          }
        }
        const scope = document.createElement("p");
        scope.textContent = "Route costs use each authored fixed assignment's actual qualified recipe and complete thread order. " +
          "Eligible-cell assignment searches are not admitted for this cost objective. " +
          "A complete fixed-route order does not require proof of the shortest makespan. " +
          "Any declared deadline is checked independently; a cheap excluded or unresolved trial does not become the selected route.";
        parent.append(scope);
      }
      const scope = document.createElement("p");
      scope.textContent = "Recipe selection uses each complete job's source-qualified thread orders, including supplier minimums, " +
        "increments and applicable price tiers. Trials with incomplete consumption or prices remain unresolved. " +
        "A retained order can be usable before the search is complete; the proof applies only to the declared recipes and dated observations. " +
        "Cutting costs, setup time and undeclared supplier bundles are outside this objective.";
      parent.append(scope);
    }
    const deadlineSearches = alternatives.filter(row => {
      const result = available(row.sewing_selection);
      return result?.request?.completion_deadline_milliseconds != null ||
        result?.trials?.some(trial => trial.completion_deadline);
    });
    if (deadlineSearches.length) {
      const deadlines = table(parent, "Deadline-constrained sewing recipe search", ["Alternative", "Recipe and route",
        "Inclusive deadline (s)", "Complete schedule (s)", "Completion lower bound (s)", "Deadline admission",
        "Minimum completion proved", "Reason"]);
      const seconds = (milliseconds, fallback) => {
        if (!Number.isSafeInteger(milliseconds) || milliseconds < 0) return metric(fallback);
        // Preserve the exact integer clock even where adjacent milliseconds have identical f64 seconds.
        const digits = String(milliseconds).padStart(4, "0");
        const fraction = digits.slice(-3).replace(/0+$/, "");
        return digits.slice(0, -3) + (fraction ? `.${fraction}` : "");
      };
      for (const row of deadlineSearches) {
        const result = available(row.sewing_selection);
        for (const trial of result.trials || []) {
          const deadline = trial.completion_deadline;
          const recipe = Object.entries(trial.choices || {}).map(([operation, choice]) => `${operation}: ${choice}`).join("; ");
          const admission = trial.refusal ? "Plan refused" :
            ({ feasible: "Meets deadline", excluded: "Misses deadline (proved)", unresolved: "Unresolved" }[deadline?.status] || "Unknown");
          const reasons = [deadline?.reason, trial.refusal?.message, trial.objective_unavailable?.message]
            .filter((message, index, all) => message && all.indexOf(message) === index).join("; ");
          cells(deadlines, [row.key,
            [trial.route_choice, recipe || "Original recipes"].filter(Boolean).join("; "),
            seconds(deadline?.limit_milliseconds ?? result.request?.completion_deadline_milliseconds),
            seconds(deadline?.selected_makespan_milliseconds, deadline?.selected_completion_seconds),
            seconds(deadline?.lower_bound_makespan_milliseconds, deadline?.lower_bound_completion_seconds),
            admission, deadline?.timing_optimality_proven == null ? "Unknown" : String(deadline.timing_optimality_proven),
            reasons || "Inspect retained completion timing"]);
        }
      }
      const scope = document.createElement("p");
      scope.textContent = "Only complete schedules meeting the inclusive deadline compete for recipe selection. " +
        "A feasible schedule can qualify before its minimum time is proved. A late incumbent remains unresolved unless a valid lower bound " +
        "or proved minimum rules out the deadline. Feed and setup alone do not establish complete production time.";
      parent.append(scope);
    }
  }
}
function manufacturingSewingMetrics(op, parent, study, computed) {
  if (study.alternatives.some(row => row.metrics?.sewing_setup_seconds != null ||
      row.metrics?.sewing_feed_and_setup_seconds != null) ||
      [...(computed?.policy?.objectives || []), ...(computed?.policy?.constraints || [])]
        .some(item => ["sewing_setup_seconds", "sewing_feed_and_setup_seconds"].includes(item.metric))) {
    const timing = table(parent, "Source-qualified sewing durations", ["Alternative", "Sewing",
      "Feed-only seconds", "Setup seconds", "Feed + setup seconds"]);
    for (const row of study.alternatives) {
      const m = row.metrics || {};
      cells(timing, [row.key, state(row.sewing), metric(m.feed_seconds),
        metric(m.sewing_setup_seconds), metric(m.sewing_feed_and_setup_seconds)]);
      const sewing = available(row.sewing);
      const source = sewing?.production?.parallel || sewing?.production?.routing ? null :
        (sewing?.production?.ordering || sewing?.ordering)?.timing;
      if (source) detail(parent, `${row.key}: setup timing evidence and selected schedule (display only)`, source);
    }
    const note = document.createElement("p");
    note.textContent = "Setup and feed + setup durations come from the selected qualified sewing plan and its declared timing evidence. " +
      "Missing timing stays unknown. These durations exclude undeclared handling, workholding and other machine-cycle work. " +
      "A feasible bounded-search result does not prove an optimal schedule.";
    parent.append(note);
  }
  sewingConsumablesComparison(parent, study.alternatives, computed);
  cuttingJobComparison(op, parent, study.alternatives);
  cutAndSewPurchasingComparison(parent, study.alternatives, computed);
  assemblyMassComparison(parent, study.alternatives);
  vaporComparison(op, parent, study.alternatives);
  if (study.screening) screening(parent, study.screening);
}
function sewingConsumablesComparison(parent, alternatives, computed) {
  const metrics = ["thread_required_length_m", "thread_required_mass_kg", "thread_observed_order_cost"];
  const requested = [...(computed?.policy?.objectives || []), ...(computed?.policy?.constraints || [])]
    .some(item => metrics.includes(item.metric)) ||
    computed?.policy?.required_consumers?.some(name => ["sewing_consumables", "sewing_purchasing"].includes(name));
  if (!requested && !alternatives.some(row => available(row.sewing)?.consumables ||
      metrics.some(name => row.metrics?.[name] != null))) return;
  const totals = table(parent, "Thread for the selected sewing plan — complete or unknown",
    ["Alternative", "Production copies", "Thread quantities", "Required length (m)", "Thread purchasing", "Thread currency", "Complete order cost"]);
  for (const row of alternatives) {
    const plan = available(row.sewing), report = plan?.consumables, purchase = report?.purchasing;
    const complete = report?.complete_consumption_coverage === true;
    const quantity = report ? (complete ? "Complete channel coverage" : "Incomplete channel coverage") :
      plan ? "Not requested" : state(row.sewing);
    const pricing = purchase ? (purchase.complete_offer_coverage === true ? "Complete offer coverage" :
      "Incomplete offer coverage") : report || plan ? "Not requested" : state(row.sewing);
    const cost = metric(complete && purchase?.complete_offer_coverage === true ?
      (purchase.selected_observed_order_total ?? purchase.least_observed_independent_order_total) : null);
    const copies = report?.schema === "ftx-sewing-consumables/1" ? 1 : report?.production_copies;
    cells(totals, [row.key, metric(copies), quantity, metric(complete ? report.total_required_length_m : null), pricing,
      purchase?.currency || "Unknown", cost === "Unknown" ? cost : `${cost} ${purchase?.currency || "(currency unavailable)"}`]);
    if (report?.complete_mass_coverage != null || row.metrics?.thread_required_mass_kg != null ||
        [...(computed?.policy?.objectives || []), ...(computed?.policy?.constraints || [])]
          .some(item => item.metric === "thread_required_mass_kg")) {
      const mass = table(parent, `${row.key}: consumed thread mass`,
        ["Mass coverage", "Complete consumed mass (kg)"]);
      cells(mass, [report?.complete_mass_coverage === true ? "Complete qualified yarn density" : "Incomplete or unavailable density",
        metric(complete && report?.complete_mass_coverage === true ? report.total_required_mass_kg : null)]);
      const yarns = table(parent, `${row.key}: exact yarn length and mass requirements`,
        ["Exact yarn snapshot", "Required length (m)", "Qualified linear density (kg/m)", "Consumed mass (kg)", "Missing mass evidence"]);
      for (const yarn of report?.material_requirements || []) cells(yarns, [yarn.material_sha256,
        metric(yarn.required_length_m), metric(yarn.mass?.linear_density_kg_m),
        metric(yarn.mass?.required_mass_kg), yarn.mass_unavailable_reason || (yarn.mass ? "None" : "Not supplied")]);
    }
    if (report?.operation_channels?.length) {
      const physicalCells = report.operation_channels.some(channel => typeof channel.cell === "string");
      const channels = table(parent, `${row.key}: selected operation and thread channel requirements`,
        [...(physicalCells ? ["Cell"] : []), "Operation", "Qualification", "Channel", "Exact yarn snapshot", "Traversed intervals", "Required length (m)"]);
      for (const channel of report.operation_channels) cells(channels, [...(physicalCells ? [channel.cell || "Unknown"] : []),
        channel.operation_id, channel.qualification_id,
        channel.channel_id, channel.material_sha256, metric(channel.stitch_intervals), metric(channel.required_length_m)]);
      if (report.operation_channels.some(channel => channel.per_copy)) {
        const perCopy = table(parent, `${row.key}: thread consumption per garment before production aggregation`,
          [...(physicalCells ? ["Cell"] : []), "Operation", "Channel", "Traversed intervals", "Traversal length (m)", "Start allowance (m)", "End allowance (m)", "Required length (m)"]);
        for (const channel of report.operation_channels) {
          const one = channel.per_copy;
          cells(perCopy, [...(physicalCells ? [channel.cell || "Unknown"] : []), channel.operation_id, channel.channel_id, metric(one?.stitch_intervals),
            metric(one?.traversal_length_m), metric(one?.start_allowance_m), metric(one?.end_allowance_m), metric(one?.required_length_m)]);
        }
      }
    }
    if (purchase?.order_allocation_optimality_proven != null) {
      const allocation = document.createElement("p");
      allocation.textContent = `${row.key}: ` + (purchase.order_allocation_optimality_proven ?
        "The least complete thread allocation among the listed offers is proved. " :
        purchase.complete_offer_coverage ? "A complete thread allocation is available; its minimum cost is unproved. " :
        "No complete thread allocation is available. ") +
        "Each operation/channel on each garment stays with one supplier; no mid-operation thread splice is inferred.";
      parent.append(allocation);
      const orders = table(parent, `${row.key}: complete thread orders across suppliers`,
        ["Exact yarn", "Offer", "Assigned operation / channel / garment", "Required length (m)",
          "Supplier order quantity", "Sale unit", "Merchandise cost", "Allocation search"]);
      for (const material of purchase.material_orders || []) {
        const result = material.allocation;
        for (const order of result?.selected_orders || []) {
          const assigned = (order.segment_indices || []).map(index => {
            const segment = purchase.segments?.[index], channel = report.operation_channels?.[segment?.operation_channel];
            return channel && segment ? `${channel.cell ? `${channel.cell} / ` : ""}${channel.operation_id} / ${channel.channel_id} / garment ${segment.copy + 1}` : "Unknown segment";
          }).join("; ");
          cells(orders, [material.requirement?.material_sha256 ?? "Unknown", order.offer?.quote?.offer_id ?? "Unknown",
            assigned, metric(order.required_length_m), metric(order.offer?.order_quantity), order.offer?.quote?.unit ?? "Unknown",
            `${metric(order.offer?.order_cost)} ${purchase.currency || "Unknown currency"}`,
            result.optimality_proven ? "Listed minimum proved" : result.budget_exhausted ? "Complete incumbent; budget ended" : "Unproved"]);
        }
      }
    }
    if (report?.missing_operations?.length || purchase?.unavailable_reason) {
      const missing = document.createElement("p");
      missing.textContent = `${row.key}: ` + (report.missing_operations || [])
        .map(item => `${item.cell ? `${item.cell} / ` : ""}${item.operation_id}: ${item.reason}`).join("; ") +
        (purchase?.unavailable_reason ? ` ${purchase.unavailable_reason}` : "");
      parent.append(missing);
    }
    detail(parent, `${row.key}: selected thread channels, exact yarns and order evidence (display only)`, {
      selected_recipe_choices: available(row.sewing_selection)?.selected_choices,
      selected_route: available(row.sewing_selection)?.selected_route,
      consumables: report || null, qualifications: plan?.options?.qualifications,
      cell_qualifications: (plan?.production?.routing?.cell_plans || plan?.production?.parallel?.cell_plans)?.map(source =>
        ({ cell: source.cell, qualifications: source.plan.options.qualifications })),
      request: plan?.options?.consumables, sewing_refusal: row.sewing?.refusal
    });
  }
  const note = document.createElement("p");
  note.textContent = "Thread demand follows every traversal in the selected sewing plan, with each declared start/end allowance applied once per operation and channel in each garment. " +
    "Partial channel rows are diagnostic; they are never a complete bill of materials or a priced subtotal. " +
    "Exact yarn requirements aggregate before supplier minimums, increments and observed stock are applied. " +
    "Study thread inventories use the same production copies as cutting; standalone sewing uses its explicitly declared production copies or one garment. " +
    "One continuous operation/channel requirement is retained without multiplying its splice-free length by the production count. " +
    "Qualified yarn linear density can supply consumed mass and convert kilogram offers while keeping supplier rounding in the original sale units. " +
    "Missing density leaves length-only results usable; mass never establishes an uninterrupted thread length. " +
    "Consumed thread includes the declared allowances and is not retained assembly mass. Explicit production schedules report their complete batch timing. " +
    "Thread prices retain their own currency and market; an explicitly requested cut-and-sew total separately checks complete fabric orders and a common source market. " +
    "Time-based recipe selection does not prove the least thread cost.";
  parent.append(note);
}
function cutAndSewPurchasingComparison(parent, alternatives, computed) {
  const requested = [...(computed?.policy?.objectives || []), ...(computed?.policy?.constraints || [])]
    .some(item => item.metric === "cut_and_sew_selected_order_cost") ||
    computed?.policy?.required_consumers?.includes("cut_and_sew_purchasing");
  if (!requested && !alternatives.some(row => row.cut_and_sew_purchasing &&
      row.cut_and_sew_purchasing.status !== "not_requested")) return;
  const t = table(parent, "Complete fabric + thread merchandise orders",
    ["Alternative", "Combined purchasing", "Garment copies", "Fabric cost", "Thread cost", "Complete selected cost",
      "Currency", "Order allocation proof", "Observed quantity coverage"]);
  for (const row of alternatives) {
    const r = available(row.cut_and_sew_purchasing);
    cells(t, [row.key, state(row.cut_and_sew_purchasing), metric(r?.production_copies),
      metric(r?.fabric_selected_order_cost), metric(r?.thread_selected_order_cost), metric(r?.selected_order_cost),
      r?.market?.currency || "Unknown", r ? (r.order_allocation_optimality_proven ? "Proved for fixed demands" :
        "Complete feasible orders; optimum unproved") : "Unknown",
      r ? (r.complete_observed_quantity_coverage ? "Complete observed capacity" : "Not fully observed") : "Unknown"]);
    detail(parent, `${row.key}: complete fabric and thread orders, common market and source history (display only)`,
      r || { refusal: row.cut_and_sew_purchasing?.refusal });
  }
  const note = document.createElement("p");
  note.textContent = "The total uses complete actual fabric orders and the selected sewing plan's complete thread orders for the same garment count. " +
    "Both sides share one comparable market and immutable quote history; current supplier stock aliases cannot supply both independently. " +
    "Supplier minimums, increments, tiers and declared capacity rules are already included by Rust. " +
    "A complete feasible total can remain available without a proved allocation minimum. Its proof applies to the fixed demands, not all designs, recipes or cell assignments. " +
    "Labor, equipment, unrepresented trims and costs outside the declared quote terms are outside this merchandise total. " +
    "A refused combined total leaves the independent fabric and thread results available for review.";
  parent.append(note);
}
function knifeProgramResult(parent, program, label = "Cutting plan", refusal) {
  if (!program) {
    if (refusal) {
      const note = document.createElement("p");
      note.textContent = `${label}: knife process unavailable — ${refusal.message}. Valid marker geometry and independent purchasing remain available.`;
      parent.append(note);
      detail(parent, `${label}: knife process refusal (display only)`, refusal);
    }
    return;
  }
  const profile = program.motion_profile;
  const exactStop = profile?.model === "trapezoidal_exact_stop/1";
  const drilling = program.drilling;
  if (profile) {
    const timing = table(parent, `${label}: retained knife timing model`,
      ["Timing model", "Cutting acceleration (mm/s²)", "Raised-travel acceleration (mm/s²)"]);
    cells(timing, [profile.model, metric(profile.cutting_acceleration_mm_s2), metric(profile.travel_acceleration_mm_s2)]);
  }
  const t = table(parent, `${label}: material-qualified knife paths`,
    ["Marker", "Process observation", "Cut paths", "Cut length (mm)", "Raised travel (mm)",
      "Cutting (s)", "Travel (s)", "Lift / lower (s)", drilling ? "Modeled process time (s)" : "Modeled path time (s)", "Travel ordering"]);
  for (const marker of program.markers || []) {
    const route = marker.selected;
    cells(t, [String(marker.marker + 1), marker.qualification, metric(route?.runs?.length),
      metric(route?.cut_length_mm), metric(route?.travel_length_mm), metric(route?.cutting_seconds),
      metric(route?.travel_seconds), metric(route?.lift_lower_seconds), metric(route?.modeled_path_seconds),
      marker.improved ? (exactStop ? "Faster complete traversal" : "Shorter complete traversal") :
        program.travel_routing ? "Complete authored cut order" : "Complete authored traversal"]);
  }
  if (drilling) {
    const tool = table(parent, `${label}: retained drill configuration`,
      ["Drill model", "Tool", "Diameter (mm)", "Maximum sheet thickness (mm)", "Select drill (s)", "Select knife (s)"]);
    cells(tool, [drilling.model, drilling.tool, metric(drilling.diameter_mm), metric(drilling.maximum_thickness_mm),
      metric(drilling.select_drill_seconds), metric(drilling.select_knife_seconds)]);
    const phases = table(parent, `${label}: qualified drilling and tool selection`,
      ["Marker", "Drill observation", "Holes", "Drill cycles (s)", "Tool selection (s)"]);
    for (const marker of program.markers || []) {
      const route = marker.selected, count = route?.drills?.length || 0;
      cells(phases, [String(marker.marker + 1), count ? marker.drill_qualification : "Not used", metric(count),
        metric(route?.drilling_seconds ?? 0), metric(route?.tool_change_seconds ?? 0)]);
    }
    const drillingNote = document.createElement("p");
    drillingNote.textContent = "All admitted holes are drilled before knife cutting. Each hole retains its exact source center and diameter, " +
      "physical piece and copy, with a separate drill observation for the material and complete machine configuration. " +
      "Drill cycles include the qualified plunge, dwell and retract phases; selecting the drill and returning to the knife are counted separately. " +
      "Complete modeled process time includes these phases and raised travel between drills and knife paths. The cut-path count and cut length cover knife paths only. " +
      "The SVG distinguishes qualified drilled holes from the original drawing marks. Hole quality and machine execution require separate physical qualification.";
    parent.append(drillingNote);
  }
  if (program.travel_routing) {
    const routes = table(parent, `${label}: raised-tool fixture routing`,
      ["Marker", "Waypoint offset (mm)", drilling ? "Detoured moves before drills / cuts" : "Detoured moves before cuts", "Intermediate travel points", "Return points"]);
    for (const marker of program.markers || []) {
      const route = marker.selected;
      const waypoints = [...(route?.drills || []), ...(route?.runs || [])].map(run => run.travel_waypoints_mm?.length || 0);
      cells(routes, [String(marker.marker + 1), metric(program.travel_routing.waypoint_offset_mm),
        metric(waypoints.filter(count => count > 0).length), metric(waypoints.reduce((sum, count) => sum + count, 0)),
        metric(route?.return_waypoints_mm?.length || 0)]);
    }
    const routing = document.createElement("p");
    routing.textContent = "Raised-tool routes retain every intermediate point in the machine frame and use the qualified tool and fixture geometry. " +
      "The SVG preview shows the complete bed and selected travel routes. Reported travel distances and times include every detour and requested return. " +
      "The waypoint offset controls the bounded candidate search; it does not add measured clearance evidence or establish a globally shortest route. " +
      "Cutting paths, their directions and the internal-before-exterior phases remain unchanged.";
    parent.append(routing);
  }
  const loaded = (program.markers || []).filter(marker => marker.loading);
  if (loaded.length) {
    const placements = table(parent, `${label}: explicit stock loading`,
      ["Marker", "Stock lower-left on bed (mm)", "Marker-to-machine translation (mm)", "Whole stock bounds on bed (mm)"]);
    for (const marker of loaded) {
      cells(placements, [String(marker.marker + 1),
        marker.loading.lower_left_mm.map(metric).join(", "),
        marker.loading.marker_to_machine_offset_mm.map(metric).join(", "),
        marker.loading.stock_bounds_mm.map(metric).join(", ")]);
    }
  }
  const finite = (program.markers || []).filter(marker => marker.finite_stock);
  if (finite.length) {
    const frames = table(parent, `${label}: physical stock and machine placement`,
      ["Inventory marker", "Physical stock", "Frozen stock SHA-256", "Planning-frame translation to machine (mm)"]);
    for (const marker of finite) {
      const stock = marker.finite_stock;
      cells(frames, [String(marker.marker + 1), stock.stock_id, stock.stock_sha256,
        (stock.marker_to_machine_offset_mm || []).map(metric).join(", ")]);
    }
    const placement = document.createElement("p");
    placement.textContent = "Each used hide or remnant keeps its original inventory identity, outline, defects and occupied regions. " +
      "The whole stock bounding box is translated onto the declared bed without rotation; path coordinates use that machine frame. " +
      "The exact source-to-machine transform is retained below. Unused inventory is not a cutting program.";
    parent.append(placement);
  }
  const note = document.createElement("p");
  note.textContent = "Internal cuts finish before exterior pieces are released. Every path retains its authored geometry and direction; " +
    "the process observation is locked to the exact textile and full machine configuration. " +
    (exactStop ?
      "Times use the retained acceleration limits with a complete stop at every cutting and raised-travel vertex, including collinear vertices, detours and requested returns. " +
      "Each segment has a triangular or trapezoidal speed profile; the qualified feed and travel rates are speed limits. No vertices are merged and no corner blending is inferred. " +
      "Lift/lower phases remain separate. Loading, blade turning and inspection are outside this model. " :
      profile ? "Timing assumptions are retained in the exact source report below. " :
      "Times cover declared constant-rate cutting, raised travel and lift/lower phases. Loading, acceleration, blade turning and inspection are outside this model. ") +
    (program.budget_exhausted ? "The ordering budget ended; complete authored traversals remain available. " : "") +
    "This reviewable plan does not establish physical cut quality, an optimal route or permission to run a machine.";
  parent.append(note);
  detail(parent, `${label}: exact knife paths, material locks and process limits (display only)`, program);
}
function cuttingJobComparison(op, parent, alternatives, downloadReplay = true) {
  if (!alternatives.some(row => row.cutting_job && row.cutting_job.status !== "not_requested")) return;
  const note = document.createElement("p");
  note.textContent = "A complete cutting job allocates each physical instance and copy once, using its declared roll or finite-stock route. " +
    "Each alternative starts from the same original inventory. Purchasing is separate: an allocated job can retain incomplete or refused offers. " +
    "Download both frozen job files to replay with cut-job; the result does not authorize machine execution.";
  parent.append(note);
  const t = table(parent, "Complete cutting jobs — rolls, hides and remnants", ["Alternative", "Cutting job", "Complete inventory",
    "Allocated instances / copies", "Roll markers", "Used finite items", "Engaged stock area (m²)",
    "External cut area (m²)", "Complete engaged-stock mass (kg)", "Purchasing", "Proven least complete cost"]);
  for (const [i, row] of alternatives.entries()) {
    const r = available(row.cutting_job), purchase = r?.purchasing;
    const cost = metric(purchase?.least_observed_independent_order_total);
    cells(t, [row.key, state(row.cutting_job), r ? (r.complete_inventory ? "Yes" : "Incomplete") : "Unknown",
      metric(r?.inventory?.length), r ? (r.roll ? metric(r.roll.rolls?.length) : "0") : "Unknown",
      r ? (r.finite_stock ? metric(r.finite_stock.used_stock_count) : "0") : "Unknown",
      metric(r?.stock_area_m2), metric(r?.external_cut_area_m2), metric(r?.stock_mass_kg),
      purchase ? (purchase.complete_offer_coverage ? "Complete offer coverage" : "Incomplete offer coverage") : r ? "Not requested" : "Unknown",
      cost === "Unknown" ? cost : `${cost} ${purchase.currency}`]);
    detail(parent, `${row.key}: routed physical inventory, source materials and purchasing outcomes (display only)`,
      r || { refusal: row.cutting_job?.refusal });
    if (!r) continue;
    knifeProgramResult(parent, r.roll?.program, `${row.key}: roll route`, r.roll?.program_refusal);
    knifeProgramResult(parent, r.finite_stock?.program, `${row.key}: finite-stock route`, r.finite_stock?.program_refusal);
    if (r.resolved_routes?.length) {
      const routes = table(parent, `${row.key}: cutting routes for the selected materials`,
        ["Source piece", "Stock route", "Material snapshot"]);
      for (const route of r.resolved_routes) cells(routes, [route.piece,
        route.stock === "roll" ? "Roll" : route.stock === "finite_stock" ? "Finite hide, sheet or remnant" : "Unknown",
        route.material_sha256]);
    }
    const rollPurchase = available(purchase?.roll);
    if (rollPurchase?.schema === "ftx-cut-procurement/3") {
      const selected = table(parent, `${row.key}: complete mixed-stock purchasing`,
        ["Complete offer coverage", "Selected complete cost", "Proven least complete cost", "Allocation optimum proved"]);
      const showCost = value => value == null ? "Unknown" : `${metric(value)} ${purchase.currency}`;
      cells(selected, [purchase.complete_offer_coverage ? "Yes" : "No",
        showCost(purchase.selected_observed_order_total), showCost(purchase.least_observed_independent_order_total),
        String(purchase.order_allocation_optimality_proven)]);
      splitProcurementResult(parent, rollPurchase, `${row.key}: roll-route supplier allocation`);
    }
    if (!downloadReplay) continue;
    const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
    if (typeof r.pattern_json === "string") link(op, actions,
      { name: `alternative-${i + 1}-cut-job-pattern.json`, text: r.pattern_json, mime: "application/json" });
    if (typeof r.replay_request_json === "string") link(op, actions,
      { name: `alternative-${i + 1}-cut-job-request.json`, text: r.replay_request_json, mime: "application/json" });
  }
}
function vaporCoverageComparison(op, parent, alternatives, simulations, requested, filePrefix = "") {
  if (!requested && ![...simulations.values()].some(value => available(value)?.vapor_coverage)) return;
  const note = document.createElement("p");
  note.textContent = "Vapor paths use this alternative's draped geometry, declared physical layers and source-qualified vapor resistance. " +
    "Only covered regions contribute to the reported potential transfer. Uncovered body fractions and unassigned cloth remain explicit; " +
    "the covered subtotal is not whole-body transfer, actual sweat evaporation or cooling supplied to the dry heat simulation.";
  parent.append(note);
  const t = table(parent, "Draped layered vapor coverage — covered-region potentials", ["Alternative", "Vapor coverage", "Body area (m²)",
    "Covered area (m²)", "Uncovered area (m²)", "Unassigned cloth reference area (m²)",
    "Covered potential latent power (W)", "Covered potential water flow (kg/s)"]);
  for (const [i, row] of alternatives.entries()) {
    const evaluation = simulations.get(row.key), initial = available(evaluation), r = available(initial?.vapor_coverage), c = r?.coverage;
    cells(t, [row.key, initial ? state(initial.vapor_coverage) : state(evaluation), metric(c?.body_area_m2),
      metric(c?.covered_area_m2), metric(c?.uncovered_area_m2), metric(c?.unassigned_cloth_reference_area_m2),
      vaporMetric(c, "covered_potential_heat_flow_w"), vaporMetric(c, "covered_potential_mass_flow_kg_s")]);
    vaporBoundsNote(parent, c, `${row.key}: covered vapor source bounds`);
    detail(parent, `${row.key}: vapor coverage regions, source observations and boundary assumptions (display only)`,
      r || { refusal: initial?.vapor_coverage?.refusal || evaluation?.refusal });
    if (typeof r?.replay_request_json === "string") {
      const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
      link(op, actions, { name: `${filePrefix}alternative-${i + 1}-vapor-coverage-replay.json`, text: r.replay_request_json, mime: "application/json" });
    }
  }
}
function vaporComparison(op, parent, alternatives) {
  if (!alternatives.some(row => row.vapor && row.vapor.status !== "not_requested")) return;
  const note = document.createElement("p");
  note.textContent = "Each alternative resolves the declared vapor-stack layers from its final pattern assignments. " +
    "The common area and boundary pressures, films and latent heat are supplied assumptions. " +
    "These are potential transfer rates, not actual sweat evaporation, garment comfort or heat supplied to the separate thermal simulation. " +
    "Missing vapor observations do not erase independent manufacturing or simulation results.";
  parent.append(note);
  const t = table(parent, "Material-following potential vapor transfer", ["Alternative", "Vapor evaluation", "Declared area (m²)",
    "Total Ret (m²·Pa/W)", "Potential latent heat flow (W)", "Potential water mass flow (kg/s)", "Layer protocols"]);
  for (const [i, row] of alternatives.entries()) {
    const r = available(row.vapor);
    cells(t, [row.key, state(row.vapor), metric(r?.request.area_m2), vaporMetric(r, "total_resistance_m2_pa_w"),
      vaporMetric(r, "potential_heat_flow_w"), vaporMetric(r, "potential_mass_flow_kg_s"),
      r ? (r.mixed_protocols ? "Mixed; explicitly admitted" : "Reported protocols match") : "Unknown"]);
    vaporBoundsNote(parent, r, `${row.key}: vapor source bounds`);
    detail(parent, `${row.key}: vapor layers, source observations and boundary assumptions (display only)`,
      r || { refusal: row.vapor?.refusal });
    if (typeof r?.replay_request_json === "string") {
      const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
      link(op, actions, { name: `alternative-${i + 1}-vapor-replay.json`, text: r.replay_request_json, mime: "application/json" });
    }
  }
}
function resetOriginSource() {
  originSource = null; $("originEstimates").checked = false;
  $("originSourceSummary").textContent = "Inspect an exact source selection before preparing an approximation.";
  clear(["linearizeOrigin"]);
}
function originRequest() {
  if (!originSource) throw Error("Inspect an exact source selection before preparing an approximation.");
  if (!originSource.allowEstimates) throw Error("Author and inspect a new selection with allow_estimates:true before preparing an estimated law. Do not edit an existing digest in place.");
  if (!$("originEstimates").checked) throw Error("Explicitly admit the estimated local law over your authored strain domain.");
  const domain = originDomainFields.map(pair => pair.map(id => {
    const value = $(id).value.trim();
    if (!value || !Number.isFinite(Number(value))) throw Error("Supply all six finite Green strain limits; no domain is inferred.");
    return Number(value);
  }));
  const [reference, accessed_on, note] = originRationaleFields.map(id => $(id).value.trim());
  if (!reference || !accessed_on || !note) throw Error("Supply an approximation reference, access date and rationale for this domain.");
  // Only the operator's new fields are serialized here. Preserve the source's exact Rust JSON,
  // including its evidence policy and digest; all constitutive admission stays in the core.
  return `{"schema":"ftx-wang2011-linearization/1","material":${originSource.selectionJson},` +
    `"green_strain_domain":${JSON.stringify(domain)},"approximation_source":${JSON.stringify({ reference, accessed_on, note })}}`;
}
function inspectResult(r, selectionJson) {
  clear(["thermalPoint"]);
  inspected = selectionJson || r.selection_json || text(r.selection);
  if (preparedBy !== "linearizeOrigin") {
    resetOriginSource();
    const exact = selectionJson || r.selection_json;
    if (typeof exact === "string") {
      const selection = JSON.parse(exact); // Display and policy only; never reserialize the retained source.
      originSource = { selectionJson: exact, allowEstimates: selection.allow_estimates === true };
      $("originSourceSummary").textContent = `Retained source: ${selection.id}. SHA-256: ${selection.expected_sha256 || "not supplied"}. ` +
        (originSource.allowEstimates ? "Estimate policy admitted; choose and confirm the local domain below." :
          "Estimates are disabled. Author and inspect a new selection with allow_estimates:true to use this approximation.");
    }
  }
  $("detail").textContent = text(r.snapshot);
  $("capabilities").replaceChildren();
  const table = document.createElement("table");
  for (const [name, state] of Object.entries(r.capabilities)) {
    const row = document.createElement("tr"), a = document.createElement("th"), b = document.createElement("td");
    a.textContent = name.replaceAll("_", " ");
    b.textContent = state.ready ? "Coefficient inputs available" : state.refusal.message;
    row.append(a, b); table.append(row);
  }
  $("capabilities").append(table);
}
function originResult(parent, r) {
  const domain = table(parent, "Authored approximation domain", ["Green strain component", "Lower", "Upper"]);
  for (const [i, label] of ["Warp E11", "Weft E22", "Engineering Green shear γ = 2E12"].entries()) {
    cells(domain, [label, ...r.input.green_strain_domain[i].map(metric)]);
  }
  const scope = document.createElement("p"); scope.textContent = r.model_scope; parent.append(scope);
  detail(parent, "Dated approximation rationale", r.input.approximation_source);
  detail(parent, "Source origin tangent in N/m (display only)", r.origin_tangent_n_m);
  return { schema: r.schema, source_material_id: r.input.material.id, source_sha256: r.source_sha256,
    input_sha256: r.input_sha256, prepared_material_sha256: r.selection.expected_sha256,
    next: "Review the prepared material's capabilities, then assign it or add it to a comparison. The original source remains retained for an explicit domain revision; this estimate does not establish garment fit or fill missing properties." };
}
function thermalPointResult(parent, r) {
  const properties = table(parent, "Source-qualified thermal properties", ["Quantity", "Value", "Unit"]);
  cells(properties, ["Material temperature", metric(r.temperature_c), "°C"]);
  cells(properties, ["Effective sheet resistance", metric(r.resistance_m2_k_w), "m² K/W"]);
  cells(properties, ["Areal heat capacity", metric(r.areal_heat_capacity_j_m2_k), "J/(m² K)"]);
  cells(properties, ["Common source temperature interval", Array.isArray(r.temperature_domain_c) ?
    r.temperature_domain_c.map(metric).join(" to ") : "Not reported for this model", "°C"]);
  const note = document.createElement("p");
  note.textContent = "These properties describe the selected dry material at the stated temperature. " +
    "A source-qualified property value does not establish garment warmth or comfort. Keep the frozen selection with the reported temperature to replay this point.";
  parent.append(note);
  detail(parent, "Material observations, thermal laws and source conditions", r.material);
  return { schema: r.schema, material_id: r.selection?.id, material_sha256: r.material?.sha256,
    allow_estimates: r.selection?.allow_estimates, model: r.model,
    uncertainty_propagated: r.uncertainty_propagated, comfort_claim: r.comfort_claim,
    empirical_validation_claim: r.empirical_validation_claim };
}
function thermalHistoryResult(parent, r) {
  const energy = table(parent, "Signed heat and stored energy", ["Quantity", "Value", "Unit"]);
  for (const [label, key, unit] of [
    ["Incoming energy at the inside boundary", "incoming_energy_j", "J"],
    ["Outgoing energy at the outside boundary", "outgoing_energy_j", "J"],
    ["Stored energy change", "stored_energy_change_j", "J"],
    ["Largest absolute step energy-balance error", "largest_step_energy_balance_error_j", "J"],
    ["Steady heat through the same network", "steady_heat_w", "W"]
  ]) cells(energy, [label, metric(r[key]), unit]);
  if (r.exterior_exchange) {
    const exterior = table(parent, "Qualified exterior convection and radiation", ["Quantity", "Value", "Unit"]);
    for (const [label, key, unit] of [["Convective energy", "convective_energy_j", "J"],
      ["Radiative energy", "radiative_energy_j", "J"], ["Steady convective heat", "steady_convective_heat_w", "W"],
      ["Steady radiative heat", "steady_radiative_heat_w", "W"]]) cells(exterior, [label, metric(r.exterior_exchange[key]), unit]);
    detail(parent, "Exterior observations and boundary conditions", r.exterior_exchange);
  }
  const note = document.createElement("p");
  note.textContent = "Boundary heat keeps its native sign; cooling can produce negative stored energy and reversed heat flow. " +
    "The initial sample has no reported boundary power or accumulated energy. A small energy residual checks numerical balance, not measured material accuracy. " +
    (r.accuracy_note || "");
  parent.append(note);
  const trace = r.trace || [], count = r.steady_temperature_c?.length || 0;
  if (trace.length && count) {
    const label = document.createElement("label"), cell = document.createElement("input"), state = document.createElement("p");
    label.textContent = `Inspect thermal cell (1–${count}, ordered from inside to outside)`;
    cell.type = "number"; cell.min = "1"; cell.max = String(count); cell.step = "1"; cell.value = "1";
    label.append(cell); parent.append(label, state);
    const samples = table(parent, "Retained heating / cooling samples", ["Time (s)", "Selected cell temperature (°C)",
      "Inside boundary heat (W)", "Outside boundary heat (W)", "Stored energy change (J)"]);
    const body = document.createElement("tbody"); samples.append(body);
    const navigation = document.createElement("div"), position = document.createElement("p");
    const buttons = ["First samples", "Previous samples", "Next samples", "Final samples"].map(title => {
      const button = document.createElement("button"); button.type = "button"; button.textContent = title;
      navigation.append(button); return button;
    });
    parent.append(navigation, position);
    const pageSize = 50, lastPage = Math.floor((trace.length - 1) / pageSize); let page = 0;
    const render = () => {
      body.replaceChildren();
      const selected = Number(cell.value) - 1;
      if (!cell.value.trim() || !Number.isInteger(selected) || selected < 0 || selected >= count) {
        state.textContent = `Choose a whole cell number from 1 to ${count}.`; position.textContent = ""; return;
      }
      const layer = Math.floor(selected / r.model.cells_per_layer), material = r.model.layers[layer];
      state.textContent = `Cell ${selected + 1}; layer ${layer + 1}: ${material?.authored?.id || "Unknown"}. ` +
        `Initial ${metric(trace[0].temperature_c?.[selected])} °C; final ${metric(trace.at(-1).temperature_c?.[selected])} °C; ` +
        `steady ${metric(r.steady_temperature_c[selected])} °C.`;
      const start = page * pageSize, end = Math.min(trace.length, start + pageSize);
      for (let i = start; i < end; i++) {
        const sample = trace[i];
        cells(body, [metric(sample.time_seconds), metric(sample.temperature_c?.[selected]), metric(sample.inside_heat_w),
          metric(sample.outside_heat_w), metric(sample.stored_energy_change_j)]);
      }
      position.textContent = `Samples ${start + 1}–${end} of ${trace.length}. Every retained sample remains in the exact report download.`;
      buttons[0].disabled = buttons[1].disabled = page === 0;
      buttons[2].disabled = buttons[3].disabled = page === lastPage;
    };
    cell.addEventListener("input", render);
    buttons[0].onclick = () => { page = 0; render(); };
    buttons[1].onclick = () => { page = Math.max(0, page - 1); render(); };
    buttons[2].onclick = () => { page = Math.min(lastPage, page + 1); render(); };
    buttons[3].onclick = () => { page = lastPage; render(); };
    render();
  }
  detail(parent, "Frozen layers, thermal laws and source conditions", r.model);
  return { schema: r.schema, area_m2: r.area_m2, inside_c: r.inside_c, outside_c: r.outside_c,
    inside_resistance_m2_k_w: r.inside_resistance_m2_k_w, outside_resistance_m2_k_w: r.outside_resistance_m2_k_w,
    cells_per_layer: r.model?.cells_per_layer, temperature_domain_c: r.model?.temperature_domain_c,
    step_seconds: r.step_seconds, steps: r.steps, record_every: r.record_every,
    final_time_seconds: trace.at(-1)?.time_seconds, retained_samples: trace.length,
    comfort_claim: r.comfort_claim, empirical_validation_claim: r.empirical_validation_claim,
    scope: "Dry one-dimensional layers with prescribed boundaries; no garment comfort, moisture or transient seam storage." };
}
function compressionHistoryResult(parent, r) {
  const controlled = r.schema === "ftx-compressed-thermal-history-report/3";
  const controls = controlled ? r.input.legs.flatMap(leg => leg.holds.map(hold => hold.control)) : [];
  const t = table(parent, "Compression and heat history — chronological holds", ["Time (s)",
    ...(controlled ? ["Prescribed boundary"] : []), "Pressure (Pa)",
    "Gap (m)", "Cell temperatures (°C)", "Cumulative external work (J)", "Cumulative entering heat (J)",
    "Cumulative leaving heat (J)", "Stored energy change (J)", "Heat balance error (J)"]);
  for (const [index, step] of r.steps.entries()) {
    const control = controls[index];
    const boundary = control?.kind === "pressure" ? `Pressure: ${metric(control.pressure_pa)} Pa` :
      control?.kind === "gap" ? `Gap: ${metric(control.gap_m)} m` : "Unavailable";
    cells(t, [metric(step.elapsed_s), ...(controlled ? [boundary] : []), metric(step.pressure_pa), metric(step.thickness_m),
      step.temperature_c.map(metric).join(", "), metric(step.cumulative_external_work_j),
      metric(step.cumulative_inside_heat_j), metric(step.cumulative_outside_heat_j),
      metric(step.cumulative_stored_change_j), metric(step.cumulative_heat_balance_error_j)]);
  }
  const note = document.createElement("p");
  note.textContent = "External mechanical work is separate from sensible heat; no work-to-heat conversion is inferred. " +
    (controlled ? "Each hold prescribes either pressure or gap; the other value is computed from the same qualified material branches. " : "") +
    "Temperatures follow the same material cells across qualified branch changes. A final material selection does not save this temperature state; keep the full history report.";
  parent.append(note);
  if (r.transitions) detail(parent, "Source-qualified branch transitions", r.transitions);
  if (r.input?.adaptive) {
    const integration = table(parent, "Adaptive thermal integration — local estimates and charged work",
      ["Time (s)", "Accepted half steps", "Rejected trials", "Charged cell solves", "Cumulative cell solves", "Largest local error ratio"]);
    for (const step of r.steps) {
      const d = step.integration;
      cells(integration, [metric(step.elapsed_s), metric(d.accepted_substeps), metric(d.rejected_trials),
        metric(d.cell_solves), metric(d.cumulative_cell_solves), metric(d.max_accepted_error_ratio)]);
    }
    const scope = document.createElement("p");
    scope.textContent = "Boundary heat sums actual accepted half steps. Local temperature and signed heat error estimates are not global accuracy or confidence bounds. " +
      "Charged work includes rejected trials and continues across holds and branch changes. Spatial error and measured material qualification remain separate.";
    parent.append(scope);
    detail(parent, "Declared adaptive thermal policy", r.input.adaptive);
  }
  return { selected_layer: r.chosen_layer_id, holds: r.steps.length, branch_transitions: r.transitions?.length || 0,
    input_sha256: r.input_sha256, reference_mass_kg: r.final_compression.reference_mass_kg,
    initial_preparation_work_j: r.initial_compression.external_work_j, no_claim: r.no_claim,
    next: "Review the final selected layer's capabilities, then assign it above. Download all final layers and the complete history below." };
}
function vaporResult(parent, r) {
  if (r.schema === "ftx-vapor-coverage-history-report/1") {
    vaporHistoryResult(parent, "Replay", r, "Available");
    return { schema: r.schema, request_sha256: r.request_sha256, path_sha256: r.path_sha256,
      integration: r.integration, latent_heat_coupled: r.latent_heat_coupled,
      ...(r.wet_film ? { body_wet_film: "Available", textile_temperature_coupled: r.wet_film.textile_temperature_coupled } : {}),
      physical_validation_claim: r.physical_validation_claim,
      next: "Download the exact report, original input and frozen replay request. Replay it in this panel without a body model." };
  }
  if (r.schema === "ftx-vapor-coverage-report/1") {
    const totals = table(parent, "Layered vapor coverage", ["Quantity", "Value", "Unit"]);
    for (const [label, name, unit] of [
      ["Covered body area", "covered_area_m2", "m²"],
      ["Uncovered body area — transfer unknown", "uncovered_area_m2", "m²"],
      ["Covered potential heat flow", "covered_potential_heat_flow_w", "W"],
      ["Covered potential water flow", "covered_potential_mass_flow_kg_s", "kg/s"]
    ]) cells(totals, [label, vaporMetric(r.coverage, name), unit]);
    vaporBoundsNote(parent, r.coverage);
    detail(parent, "Ordered layers, pressure profiles and source conditions", r.coverage.regions);
    detail(parent, "Projection and boundary assumptions", { projection: r.projection, request: r.request });
    return { schema: r.schema, request_sha256: r.request_sha256, physical_validation_claim: r.physical_validation_claim,
      scope: "Potential transfer through covered regions only; uncovered transfer and moisture availability remain unknown." };
  }
  const note = document.createElement("p");
  note.textContent = "Potential steady transfer through the declared series of vapor resistances. " +
    "Boundary vapor pressures, film resistances, area and latent heat are supplied assumptions. " +
    "This does not establish available sweat, actual evaporative cooling, condensation, garment comfort or a coupled dry/wet heat history. " +
    (r.mixed_protocols ? "Different reported source protocols were explicitly admitted. " : "Reported layer protocols match. ") +
    "Source measurement conditions remain attached to each layer; they are not an operating range for this model.";
  parent.append(note);
  const totals = table(parent, "Potential vapor and latent heat exchange", ["Quantity", "Value", "Unit"]);
  for (const [label, name, unit] of [
    ["Total vapor resistance", "total_resistance_m2_pa_w", "m²·Pa/W"],
    ["Boundary vapor pressure difference", "pressure_difference_pa", "Pa"],
    ["Potential latent heat flux", "potential_heat_flux_w_m2", "W/m²"],
    ["Potential latent heat flow", "potential_heat_flow_w", "W"],
    ["Potential water mass flux", "potential_mass_flux_kg_m2_s", "kg/(m²·s)"],
    ["Potential water mass flow", "potential_mass_flow_kg_s", "kg/s"],
    ["Inside film pressure drop", "inside_film_pressure_drop_pa", "Pa"],
    ["Outside film pressure drop", "outside_film_pressure_drop_pa", "Pa"]
  ]) cells(totals, [label, vaporMetric(r, name), unit]);
  vaporBoundsNote(parent, r);
  const layers = table(parent, "Layer resistances and vapor pressure drops — inside to outside",
    ["Layer", "Material", "Ret (m²·Pa/W)", "Inside pressure (Pa)", "Outside pressure (Pa)", "Drop (Pa)", "Evidence"]);
  for (const row of r.layers) {
    cells(layers, [row.key, row.selection.id, metric(row.observation.value) +
      (row.observation.bounds ? `; source bounds [${row.observation.bounds.map(metric).join(", ")}]` : ""), metric(row.inside_pressure_pa),
      metric(row.outside_pressure_pa), metric(row.pressure_drop_pa), row.observation.evidence]);
    detail(parent, `${row.key}: measurement method, conditions and exact source`, row);
  }
  detail(parent, "Declared boundary conditions and effective model", { request: r.request, effective_model: r.effective_model });
  return { schema: r.schema, request_sha256: r.request_sha256, mixed_protocols: r.mixed_protocols,
    physical_validation_claim: r.physical_validation_claim, notes: r.notes,
    next: "Download the exact report, original input and frozen replay request to repeat this calculation." };
}
function vaporSurfaceResult(parent, r) {
  const note = document.createElement("p");
  note.textContent = "Independent sheet potentials use each piece's current draped area and retained vapor resistance. " +
    "Overlapping sheets are evaluated independently. These totals do not establish body coverage, available sweat, " +
    "actual wearer evaporation or latent cooling of the dry-heat model.";
  parent.append(note);
  const totals = table(parent, "Potential transfer on current cloth geometry", ["Quantity", "Value", "Unit"]);
  for (const [label, name, unit] of [
    ["Current cloth area", "area_m2", "m²"],
    ["Potential latent heat flow", "potential_heat_flow_w", "W"],
    ["Potential water mass flow", "potential_mass_flow_kg_s", "kg/s"]
  ]) cells(totals, [label, vaporMetric(r, name), unit]);
  vaporBoundsNote(parent, r);
  const pieces = table(parent, "Independent piece potentials", ["Piece", "Area (m²)", "Potential heat (W)", "Potential water (kg/s)"]);
  for (const row of r.pieces) cells(pieces, [row.instance, metric(row.area_m2),
    vaporMetric(row, "potential_heat_flow_w"), vaporMetric(row, "potential_mass_flow_kg_s")]);
  detail(parent, "Original resistance observations, boundaries and source protocols", r.columns);
  return { schema: r.schema, model_sha256: r.model_sha256, request_sha256: r.request_sha256,
    scope: r.scope, physical_validation_claim: r.physical_validation_claim,
    next: "Download the complete report, frozen replay, original conditions and retained drape below." };
}
function treatmentResult(parent, r) {
  const transitions = r.schema === "ftx-pattern-treatment-report/1" ? Object.entries(r.transitions) : [[r.input.specimen, r]];
  const rows = table(parent, "Source-qualified dimensional change", ["Specimen / transition", "Cycles",
    "Warp / wale remaining length", "Weft / course remaining length", "Area ratio", "Unsupported post-treatment properties"]);
  for (const [name, transition] of transitions) {
    cells(rows, [name, metric(transition.input.cycles), metric(transition.stretch[0]),
      metric(transition.stretch[1]), metric(transition.area_scale),
      [...transition.properties_no_longer_available, ...transition.responses_no_longer_available].join(", ") || "No original field lost"]);
    detail(parent, `${name}: exact process, conditioning, observations and before/after materials`, transition);
  }
  if (r.seams_after) {
    const seams = table(parent, "Seam compatibility after dimensional change", ["Seam", "Before", "After", "Length mismatch (mm)"]);
    for (const [i, row] of r.seams_after.entries()) cells(seams,
      [row.seam, r.seams_before[i]?.status ?? "Unknown", row.status, metric(row.residual_mm)]);
  }
  return { schema: r.schema, mode: r.mode, source_pattern_sha256: r.source_pattern_sha256,
    pattern_sha256: r.pattern_sha256, input_sha256: r.input_sha256,
    scope: r.scope || "Measured free dimensional change between two explicit material states; no old property is inherited.",
    next: r.pattern_json ? "Review the seam checks and downloads, then use the derived pattern to recompute fit, drape and manufacturing." :
      "Download the exact material selections or use this transition in a piece-treatment request." };
}
function sewingWorkholding(parent, plan, label = "Selected sewing plan") {
  if (plan?.production?.parallel || plan?.production?.routing) return;
  const report = plan?.production?.workholding;
  if (!report) return;
  const replaced = plan.production.ordering?.baseline_replaced === true;
  const resources = table(parent, `${label}: finite fixture inventory`,
    ["Resource", "Available units", replaced ? "Feasible baseline peak" : "Original order peak", "Selected order peak"]);
  for (const resource of report.resources) cells(resources, [resource.id, metric(resource.capacity),
    metric(report.baseline.peak_occupancy[resource.id]), metric(report.selected.peak_occupancy[resource.id])]);
  const occupancy = values => Object.entries(values).map(([id, units]) => `${id}: ${metric(units)}`).join("; ");
  const timeline = table(parent, `${label}: selected fixture occupancy`,
    ["Operation", "Occupied before", "Acquired before start", "Occupied during", "Released after completion", "Occupied after"]);
  for (const step of report.selected.operations) cells(timeline, [step.operation,
    occupancy(step.occupied_before), step.acquired.join(", ") || "None", occupancy(step.occupied_during),
    step.released.join(", ") || "None", occupancy(step.occupied_after)]);
  const note = document.createElement("p");
  note.textContent = "The complete selected order respects the declared finite fixture inventory. " +
    (replaced ? "The original copy order could not satisfy the declared fixtures or lifecycle prerequisites; a complete feasible replacement baseline was found within the same search budget. " : "") +
    "Each garment acquires its units before the named operation starts and releases them after the named operation completes. " +
    "The inventory and holds are tied to the exact machine configuration and admitted material/process qualifications. " +
    "Fixture geometry, seam access, handling duration and hardware execution remain unvalidated.";
  parent.append(note);
  detail(parent, `${label}: fixture source, qualification locks and both complete timelines`, {
    policy: plan.options?.production?.workholding, report });
}
function sewingHandling(parent, plan, label = "Selected sewing plan") {
  if (plan?.production?.parallel || plan?.production?.routing) return;
  const report = plan?.production?.handling;
  if (!report) return;
  const timing = plan.production.ordering?.handling_timing;
  const totals = table(parent, `${label}: serial production durations`,
    ["Schedule", "Feed (s)", "Setup (s)", "Handling (s)", "Modeled production (s)"]);
  const baselineLabel = plan.production.ordering?.baseline_replaced ? "Feasible replacement baseline" : "Original dependency order";
  for (const [name, prefix] of [[baselineLabel, "baseline"], ["Selected order", "selected"]]) {
    const setup = plan.production.ordering?.timing?.[`${prefix}_setup_milliseconds`];
    const handling = timing?.[`${prefix}_handling_milliseconds`];
    cells(totals, [name, metric(plan.production.feed_seconds),
      metric(typeof setup === "number" ? setup / 1000 : null),
      metric(typeof handling === "number" ? handling / 1000 : null),
      metric(timing?.[`${prefix}_modeled_production_seconds`])]);
  }
  const transitions = table(parent, `${label}: selected garment handling`,
    ["Previous operation", "Next operation", "Handling (ms)"]);
  for (const step of report.selected?.steps || []) cells(transitions,
    [step.from ?? "Initial loading", step.to ?? "Final unloading", metric(step.milliseconds)]);
  const note = document.createElement("p");
  note.textContent = "The selected order includes independently qualified initial loading, operation transitions and final unloading. " +
    "Same-garment and different-garment transitions have explicit durations; retaining a needle/thread setup does not imply zero handling. " +
    "Feed, setup and handling are serial, nonoverlapping phases in this declared model. A joint optimum does not prove the setup-only minimum. " +
    "The result remains conditional on the supplied timings and constraints; it does not qualify undeclared work, concurrent machines or hardware execution.";
  parent.append(note);
  detail(parent, `${label}: handling evidence, exact qualifications and complete timelines`,
    { policy: plan.options?.production?.handling, report, timing });
}
function sewingSharedWorkholding(parent, plan, label) {
  const report = plan.production.parallel.shared_workholding;
  if (!report) return;
  const inventory = table(parent, `${label}: shared fixture pools`,
    ["Pool", "Available units", "Baseline peak", "Selected peak", "Held after completion"]);
  for (const resource of report.resources) cells(inventory, [resource.id, metric(resource.capacity),
    metric(report.baseline?.peak_occupancy?.[resource.id]), metric(report.selected?.peak_occupancy?.[resource.id]),
    metric(report.selected?.final_occupancy?.[resource.id])]);
  const bindings = table(parent, `${label}: cell fixtures using shared inventory`,
    ["Cell", "Local fixture resource", "Shared pool"]);
  for (const binding of report.bindings) cells(bindings, [binding.cell, binding.resource, binding.pool]);
  const holds = table(parent, `${label}: selected shared fixture reservations`,
    ["Pool", "Cell", "Global garment copy", "Local hold", "Units", "Acquire operation", "Release operation",
      "Held from (s)", "Available again (s)"]);
  for (const hold of report.selected?.holds || []) cells(holds, [hold.pool, hold.cell, metric(hold.copy),
    hold.hold, metric(hold.units), hold.acquire, hold.release,
    metric(typeof hold.acquired_at_milliseconds === "number" ? hold.acquired_at_milliseconds / 1000 : null),
    metric(typeof hold.released_at_milliseconds === "number" ? hold.released_at_milliseconds / 1000 : null)]);
  const note = document.createElement("p");
  note.textContent = "These cells share the declared finite fixture inventory. Units are reserved before handling starts, " +
    "remain held through intervening operations and waits, and become available at the release operation's feed completion. " +
    "A release can supply an acquisition at the same instant. The qualified protocol includes pool access and return in cell handling. " +
    "The computed allocation respects pool capacities; physical pool access, garment transfers and hardware execution still require their own qualification.";
  parent.append(note);
  detail(parent, `${label}: shared pool source, exact cell locks and both reservation histories`, {
    policy: plan.options?.production?.parallel?.shared_workholding, report });
}
function sewingRoutedWorkholding(parent, routing, label) {
  const report = routing.workholding;
  if (!report) return;
  const seconds = value => typeof value === "number" ? value / 1000 : null;
  const local = table(parent, `${label}: routed cell fixture inventory`,
    ["Cell", "Resource", "Available units", "Baseline peak", "Selected peak"]);
  for (const cell of report.cells || []) for (const resource of cell.resources || []) cells(local, [
    cell.cell, resource.id, metric(resource.capacity),
    metric(report.baseline?.local_peak_occupancy?.[cell.cell]?.[resource.id]),
    metric(report.selected?.local_peak_occupancy?.[cell.cell]?.[resource.id]),
  ]);
  if (report.shared_workholding) {
    const pools = table(parent, `${label}: routed shared fixture pools`,
      ["Pool", "Available units", "Baseline peak", "Selected peak"]);
    for (const pool of report.shared_workholding.resources || []) cells(pools, [pool.id, metric(pool.capacity),
      metric(report.baseline?.shared_peak_occupancy?.[pool.id]), metric(report.selected?.shared_peak_occupancy?.[pool.id])]);
  }
  const retained = (report.selected?.holds || []).some(hold =>
    typeof hold.release_operation === "string" && hold.release_operation !== hold.operation);
  const holds = table(parent, `${label}: selected routed fixture reservations`,
    ["Cell", "Garment copy", retained ? "Acquire operation" : "Operation", ...(retained ? ["Release operation"] : []),
      "Hold", "Cell resource", "Shared pool", "Units", "Acquired (s)", "Released after unloading (s)"]);
  for (const hold of report.selected?.holds || []) cells(holds, [hold.cell, metric(hold.copy), hold.operation,
    ...(retained ? [hold.release_operation ?? hold.operation] : []),
    hold.hold, hold.resource, hold.pool || "Cell local", metric(hold.units),
    metric(seconds(hold.acquired_at_milliseconds)), metric(seconds(hold.released_at_milliseconds))]);
  const note = document.createElement("p");
  note.textContent = "Routed fixtures are reserved before loading and remain occupied through setup, feed and unloading. " +
    "A release can supply an acquisition at the same instant. Shared pools constrain overlapping operations across specialist cells, including reservations already placed later in time. " +
    (retained ? "Retained fixtures stay reserved at their owning cell until the declared release operation finishes unloading, including intervening waits and unloaded garment handoffs; " :
      "These holds end at their operation; ") +
    "occupied fixtures are not transferred with garments. Physical fixture access, pool access and machine execution require separate qualification.";
  parent.append(note);
  detail(parent, `${label}: routed fixture sources, cell locks and complete reservation histories`, report);
}
function sewingRoutedLabor(parent, routing, label) {
  const report = routing.attending_labor;
  if (!report) return;
  const transfer = report.transfer;
  const seconds = value => typeof value === "number" ? value / 1000 : null;
  const totals = table(parent, `${label}: routed attending labor`,
    ["Schedule", "Travel (s)", "Waiting (s)", "Full attendance (s)", ...(transfer ? ["Garment carry (s)"] : [])]);
  for (const [name, timeline] of [["Baseline", report.baseline], ["Selected", report.selected]]) cells(totals, [name,
    metric(seconds(timeline?.travel_milliseconds)), metric(seconds(timeline?.waiting_milliseconds)),
    metric(seconds(timeline?.attended_milliseconds)), ...(transfer ? [metric(seconds(timeline?.transfer_milliseconds))] : [])]);
  const workers = table(parent, `${label}: qualified attending workers`,
    ["Worker", "Cells", "Available (s)", "Inclusive finish limit (s)"]);
  for (const worker of report.policy?.workers || []) cells(workers, [worker.id, (worker.cells || []).join(", "),
    metric(seconds(worker.available_at_milliseconds)), typeof worker.available_until_milliseconds === "number" ?
      metric(seconds(worker.available_until_milliseconds)) : "Open ended"]);
  const steps = table(parent, `${label}: selected attending worker timeline`,
    ["Worker", "Garment copy", "Operation", "From", "Cell", "Departure (s)", "Arrival (s)",
      "Loading (s)", "Feed (s)", "Unloading complete (s)"]);
  for (const step of report.selected?.steps || []) cells(steps, [step.worker, metric(step.copy), step.operation,
    step.from_cell ?? "Declared initial location", step.cell, metric(seconds(step.departure_milliseconds)),
    metric(seconds(step.arrival_milliseconds)), metric(seconds(step.loading_start_milliseconds)),
    metric(seconds(step.feed_start_milliseconds)), metric(seconds(step.finish_milliseconds))]);
  if (transfer) {
    const transfers = table(parent, `${label}: selected worker-owned garment transfers`,
      ["Worker", "Resource", "Garment copy", "After operation", "Before operation", "Worker from", "Pickup cell", "Deposit cell",
        "Approach starts (s)", "At pickup cell (s)", "Pickup and carry starts (s)", "Deposit complete (s)"]);
    for (const step of report.selected?.transfers || []) cells(transfers, [step.worker, step.resource, metric(step.copy),
      step.from_operation, step.to_operation, step.from_cell ?? "Declared initial location", step.source_cell, step.destination_cell,
      metric(seconds(step.departure_milliseconds)), metric(seconds(step.arrival_milliseconds)),
      metric(seconds(step.start_milliseconds)), metric(seconds(step.finish_milliseconds))]);
  }
  const note = document.createElement("p");
  note.textContent = "Each named worker attends the complete operation, including feed and final unloading. " +
    "Travel and waiting also occupy that worker; cells assigned to the same person cannot use them concurrently. " +
    (transfer ? `Worker ${transfer.attending_worker} performs the garment handoffs using resource ${transfer.id}. ` +
      "Empty approach is charged to worker travel; pickup, carrying and deposit occupy the same worker and end at the destination cell. " +
      "The worker's machine attendance and handoffs cannot overlap. " : "Garment handoffs retain their separate qualified resource. ") +
    "These source-qualified modeled times do not establish measured labor performance or hardware authorization.";
  parent.append(note);
  detail(parent, `${label}: attending worker sources, cell locks and complete replay`, report);
}
function sewingRouting(parent, plan, label = "Selected sewing plan") {
  const report = plan?.production?.routing;
  if (!report) return;
  const policy = plan.options?.production?.routing;
  const eligible = policy?.model === "eligible_operation_route/1";
  const seconds = value => typeof value === "number" ? value / 1000 : null;
  const totals = table(parent, `${label}: routed completion and aggregate work`,
    ["Schedule", "Completion (s)", "Total feed work (s)", "Scheduled feed work (s)",
      "Total setup work (s)", "Total loading/unloading (s)", "Total transfer work (s)"]);
  for (const [name, timeline] of [[report.baseline_replaced ? "Feasible replacement baseline" : "Copy-order baseline", report.baseline],
      [eligible ? "Selected route and cell dispatch" : "Selected route dispatch", report.selected]]) cells(totals, [name,
    metric(seconds(timeline?.makespan_milliseconds)), metric(timeline?.feed_seconds),
    metric(seconds(timeline?.scheduled_feed_milliseconds)), metric(seconds(timeline?.setup_milliseconds)),
    metric(seconds(timeline?.handling_milliseconds)), metric(seconds(timeline?.transfer_milliseconds))]);
  if (report.thread_order_search) {
    const search = report.thread_order_search;
    const cost = table(parent, `${label}: routed thread-order cost and proof`,
      ["Currency", "Selected order cost", "Completion deadline (s)", "Order evaluations",
        "Unresolved assignments", "Order budget exhausted", "Cost minimum proved"]);
    cells(cost, [search.currency || "Unavailable",
      typeof search.selected_order_cost === "number" ? metric(search.selected_order_cost) : "Unavailable",
      typeof search.completion_deadline_milliseconds === "number" ? metric(seconds(search.completion_deadline_milliseconds)) : "None declared",
      metric(search.order_evaluations), metric(search.unresolved_assignments),
      search.order_budget_exhausted ? "Yes" : "No", search.optimality_proven ? "Yes" : "No"]);
    const scope = document.createElement("p");
    scope.textContent = (search.optimality_proven ?
      "Minimum selected thread order cost proved within the declared eligible-cell assignments and order search. " :
      "The minimum selected thread order cost is not proved. ") +
      "This proof concerns the qualified discrete thread purchase, including assigned stitch demand and declared setup waste. " +
      "A completion deadline constrains feasible assignments when declared. Cost proof and minimum completion-time proof are independent. " +
      (search.order_budget_exhausted ? "The order search reached its declared evaluation limit. " : "") +
      (search.unresolved_assignments ? "Unresolved assignments prevent a global cost proof." : "");
    parent.append(scope);
  }
  const route = table(parent, `${label}: ${eligible ? "eligible cells by garment operation" : "fixed garment route"}`,
    ["Step", "Operation", "Physical cell", ...(eligible ? ["Authored seed", "Selected copies"] : []),
      "Qualified machine", "Needle", "Thread", "Maximum stitch (mm)", "Stitches/min"]);
  for (const [index, step] of (report.route || []).entries()) {
    for (const cell of eligible ? step.eligible_cells || [] : [step.cell]) {
      const source = report.cell_plans?.find(item => item.cell === cell);
      const pass = source?.plan?.seams?.find(item => (item.pass.id || item.pass.seam) === step.operation)?.pass;
      const copies = (report.selected?.operations || []).filter(item => item.operation_id === step.operation && item.cell === cell).length;
      cells(route, [metric(index + 1), step.operation, cell,
        ...(eligible ? [cell === step.cell ? "Yes" : "No", metric(copies)] : []),
        source?.plan?.options?.machine?.id || "Unavailable", pass?.needle || "Unavailable", pass?.thread || "Unavailable",
        metric(pass?.max_stitch_mm), metric(pass?.stitches_per_minute)]);
    }
  }
  const operations = table(parent, `${label}: selected routed operation timeline`,
    ["Global garment copy", "Operation", "Cell", "Loading starts (s)", "Setup starts (s)",
      "Feed starts (s)", "Feed finishes (s)", "Unloaded and cell free (s)"]);
  for (const step of report.selected?.operations || []) cells(operations, [metric(step.copy), step.operation_id, step.cell,
    metric(seconds(step.loading_start_milliseconds)), metric(seconds(step.setup_start_milliseconds)),
    metric(seconds(step.feed_start_milliseconds)), metric(seconds(step.feed_finish_milliseconds)),
    metric(seconds(step.finish_milliseconds))]);
  const transfers = table(parent, `${label}: selected garment transfers`,
    ["Global garment copy", "After operation", "Before operation", "From cell", "To cell", "Starts (s)", "Arrives (s)"]);
  for (const step of report.selected?.transfers || []) cells(transfers, [metric(step.copy), step.from_operation,
    step.to_operation, step.from, step.to, metric(seconds(step.start_milliseconds)), metric(seconds(step.finish_milliseconds))]);
  const resources = table(parent, `${label}: qualified routing availability`,
    ["Resource", "Available (s)", "Inclusive finish limit (s)"]);
  for (const cell of policy?.cells || []) cells(resources, [cell.id, metric(seconds(cell.available_at_milliseconds)),
    typeof cell.available_until_milliseconds === "number" ? metric(seconds(cell.available_until_milliseconds)) : "Open ended"]);
  if (policy?.transfer) cells(resources, [policy.transfer.id, metric(seconds(policy.transfer.available_at_milliseconds)),
    typeof policy.transfer.available_until_milliseconds === "number" ? metric(seconds(policy.transfer.available_until_milliseconds)) : "Open ended"]);
  if (report.availability_calendars) {
    const calendars = report.availability_calendars;
    const windows = table(parent, `${label}: nonpreemptive operating windows`,
      ["Resource", "Window", "Opens (s)", "Inclusive finish (s)"]);
    for (const [resource, intervals] of Object.entries(calendars.resources)) {
      for (const [index, interval] of intervals.entries()) cells(windows, [resource, metric(index + 1),
        metric(seconds(interval.start_milliseconds)), metric(seconds(interval.end_milliseconds))]);
    }
    const scope = document.createElement("p");
    scope.textContent = "Each listed window contains a complete occupied interval. Machine loading, setup, feed and unloading stay together; " +
      "worker windows include empty travel, waiting and full attendance or carrying. Fixture delays recheck the windows. " +
      "Gaps preserve machine setup and worker location. Resources without a listed calendar retain their outer availability. " +
      "These qualified planning inputs do not reserve live resources or authorize hardware.";
    parent.append(scope);
    detail(parent, `${label}: operating calendar source and exact windows`, calendars);
  }
  sewingRoutedWorkholding(parent, report, label);
  sewingRoutedLabor(parent, report, label);
  for (const source of report.cell_plans || []) {
    sewingAccess(parent, source.plan, `${label}: routed cell ${source.cell}`);
    detail(parent, `${label}: routed cell ${source.cell} admitted operations and exact geometry`, source);
  }
  const note = document.createElement("p");
  note.textContent = (report.optimality_proven ?
    (eligible ? "Minimum completion time proved by the declared eligible-cell model's independent lower bound. " :
      "Minimum completion time proved within the declared fixed-route dispatch search. ") :
    "Complete feasible routed schedule retained; minimum completion time is not proved. ") +
    `Lower bound: ${metric(seconds(report.lower_bound_makespan_milliseconds))} s; search work: ${metric(report.search_nodes)}. ` +
    (report.budget_exhausted ? "The dispatch search reached its work limit. " : "") +
    (report.baseline_replaced ? "The copy-order seed could not satisfy the qualified routing constraints; a complete feasible replacement is shown. " : "") +
    (eligible ?
      "Each garment follows the same explicit operation sequence and can choose a different qualified cell at each step. " +
      "Every eligible operation/cell pair is qualified; selected copy counts distinguish used recipes from unused alternatives. " +
      "The outer one-garment seam table is the authored seed reference; the selected timeline, stitch stream and batch totals use actual cell assignments. " :
      "Each garment follows the same explicit operation route. Each specialist cell qualifies only its assigned operations. ") +
    (report.attending_labor ? "Explicit worker ownership and availability constrain the complete loading, setup, feed and unloading interval. " :
      "Each dedicated worker attends loading, setup, feed and unloading before the cell becomes free. ") +
    (policy?.transfer?.attending_worker ?
      `Worker ${policy.transfer.attending_worker} and the qualified transfer resource are both reserved from pickup through deposit; ` +
      "empty approach uses the worker's travel qualification. " :
      "One separately qualified transfer resource serializes unloaded garment handoffs; its source duration includes positioning, pickup, movement and deposit. ") +
    `Qualified staging has ${metric(policy?.staging_capacity)} garment slots. ` +
    "Stitch geometry and thread demand come from the actual assigned cell. Elapsed completion is distinct from summed machine and transfer work. " +
    "The proof covers the supplied route and dispatch model; it does not establish unrestricted factory optimality, measured seam quality or hardware authorization.";
  parent.append(note);
  detail(parent, `${label}: route sources, cell locks and complete baseline/selected timelines`, { policy, report });
}
function sewingParallel(parent, plan, label = "Selected sewing plan") {
  const report = plan?.production?.parallel;
  if (!report) return;
  const heterogeneous = Boolean(report.cell_plans?.length);
  const windowed = Boolean(report.availability);
  const operator = report.shared_operator;
  const pools = report.shared_workholding;
  const seconds = value => typeof value === "number" ? value / 1000 : null;
  const totals = table(parent, `${label}: parallel completion and aggregate work`,
    ["Allocation", "Completion (s)", "Total feed work (s)", "Scheduled feed work (s)", "Total setup work (s)", "Total handling work (s)"]);
  const baselineLabel = report.availability?.baseline_replaced || operator?.baseline_replaced ? "Feasible replacement baseline" : "Single-cell baseline";
  for (const [name, allocation] of [[baselineLabel, report.baseline], ["Selected cells", report.selected]]) {
    cells(totals, [name, metric(seconds(allocation?.makespan_milliseconds)), metric(allocation?.feed_seconds),
      metric(seconds(allocation?.scheduled_feed_milliseconds)), metric(seconds(allocation?.setup_milliseconds)),
      metric(seconds(allocation?.handling_milliseconds))]);
  }
  const machines = table(parent, `${label}: selected physical cells`,
    ["Cell", ...(heterogeneous ? ["Qualified machine"] : []), "Global garment copies (zero-based)", "Available (s)",
      ...(windowed ? ["Finish limit (s)"] : []), "Complete (s)", "Raw feed (s)", "Feed rounding (ms)", "Cell order minimum proved"]);
  const operations = table(parent, `${label}: cell-local sewing order`,
    ["Cell", "Position in cell", "Global garment copy", "Operation", "Needle", "Thread",
      ...(heterogeneous ? ["Maximum stitch (mm)", "Stitches/min"] : []), "Required local predecessors"]);
  for (const cell of report.selected?.cells || []) {
    const schedule = cell.schedule;
    const source = report.cell_plans?.find(item => item.cell === cell.cell);
    const sourcePlan = source?.plan || plan;
    cells(machines, [cell.cell, ...(heterogeneous ? [sourcePlan.options?.machine?.id || "Unavailable"] : []),
      cell.copies.join(", ") || "Idle", metric(seconds(cell.available_at_milliseconds)),
      ...(windowed ? [typeof cell.available_until_milliseconds === "number" ? metric(seconds(cell.available_until_milliseconds)) : "Open ended"] : []),
      schedule ? metric(seconds(cell.finish_milliseconds)) : "Idle", metric(schedule?.feed_seconds),
      metric(typeof cell.feed_padding_seconds === "number" ? cell.feed_padding_seconds * 1000 : null),
      schedule ? String(schedule.ordering.optimality_proven) : "No work"]);
    if (!schedule) continue;
    for (const [i, id] of schedule.ordering.selected_order.entries()) {
      const operation = schedule.operations.find(item => item.id === id);
      const pass = sourcePlan.seams[operation.seam_index].pass;
      cells(operations, [cell.cell, metric(i + 1), metric(cell.copies[operation.copy]),
        operation.operation_id, pass.needle, pass.thread,
        ...(heterogeneous ? [metric(pass.max_stitch_mm), metric(pass.stitches_per_minute)] : []),
        operation.after.join(", ") || "None"]);
    }
    const policy = plan.options?.production?.parallel?.cells?.find(item => item.id === cell.cell);
    const local = { production: schedule, options: { production: policy } };
    sewingWorkholding(parent, local, `${label}: cell ${cell.cell} (local copy IDs)`);
    sewingHandling(parent, local, `${label}: cell ${cell.cell} (local copy IDs)`);
    if (heterogeneous) {
      sewingAccess(parent, sourcePlan, `${label}: cell ${cell.cell}`);
      detail(parent, `${label}: cell ${cell.cell} qualified machine, recipes and exact station geometry`, source);
    }
  }
  if (windowed) {
    const windows = table(parent, `${label}: ${operator ? "cell-only window relaxation" : "qualified cell availability"}`,
      ["Cell", "Garment count", "Finish limit (s)", "Complete incumbent finish (s)", "Finish lower bound (s)", "Window admission"]);
    for (const cell of report.availability.cells || []) {
      for (const count of cell.counts || []) cells(windows, [cell.cell, metric(count.copies),
        typeof cell.available_until_milliseconds === "number" ? metric(seconds(cell.available_until_milliseconds)) : "Open ended",
        metric(seconds(count.incumbent_finish_milliseconds)), metric(seconds(count.lower_bound_finish_milliseconds)),
        count.feasible ? (pools ? "Fits without shared-resource contention" : operator ? "Fits without operator contention" : "Fits") : count.infeasibility_proven ? "Cannot fit: proved" : "Unresolved search"]);
    }
  }
  if (operator) {
    const labor = table(parent, `${label}: shared operator work`,
      ["Operator", "Busy (s)", "Travel (s)", "Waiting (s)", "Restricted search complete", "Operator work limit reached"]);
    cells(labor, [operator.operator, metric(seconds(operator.selected?.busy_milliseconds)),
      metric(seconds(operator.selected?.transfer_milliseconds)), metric(seconds(operator.selected?.waiting_milliseconds)),
      String(operator.search_complete), String(operator.budget_exhausted)]);
    const timeline = table(parent, `${label}: selected shared operator timeline`,
      ["Cell", "Local operation", "Global copy", "Departure (s)", "Arrival (s)", "Handling starts (s)",
        "Setup starts (s)", "Operator free (s)", "Feed complete (s)"]);
    for (const step of operator.selected?.steps || []) cells(timeline,
      [step.cell, step.operation ?? "Final unloading", typeof step.copy === "number" ? metric(step.copy) : "Batch end",
        metric(seconds(step.departure_milliseconds)), metric(seconds(step.arrival_milliseconds)),
        metric(seconds(step.handling_start_milliseconds)), metric(seconds(step.setup_start_milliseconds)),
        metric(seconds(step.operator_release_milliseconds)), metric(seconds(step.feed_finish_milliseconds))]);
  }
  sewingSharedWorkholding(parent, plan, label);
  const note = document.createElement("p");
  note.textContent = (report.optimality_proven ?
    "Minimum parallel completion time proved under the declared millisecond planning model. " :
    "Complete feasible cell allocation retained; the minimum parallel completion time is not proved. ") +
    `Lower bound: ${metric(seconds(report.lower_bound_makespan_milliseconds))} s; search work: ${metric(report.search_nodes)}. ` +
    (report.budget_exhausted ? (operator ? "At least one serial or operator search reached its work limit. " : "At least one inner schedule reached its work limit. ") : "") +
    (windowed ? "Every used cell finishes inside its inclusive qualified availability window, including final unloading. " +
      "A lower bound that still fits leaves an over-limit incumbent unresolved. " : "") +
    (report.availability?.baseline_replaced || operator?.baseline_replaced ?
      `The first-cell baseline failed a qualified cell or operator window${pools ? " or shared fixture constraint" : ""}; the displayed replacement is the complete selected feasible assignment. ` : "") +
    "Every garment stays in one physical cell. " +
    (operator ? "One qualified operator performs exclusive travel, handling, setup and final unloading while explicitly qualified autonomous machine feed may overlap. " +
      "The operator search considers the retained serial operation orders; completing that search alone does not prove an unrestricted joint optimum. " +
      (pools ? "Cell-only window receipts ignore shared operator and fixture contention; the selected timeline enforces cell, operator and pool limits. " :
        "Cell-only window receipts ignore operator contention; the selected timeline enforces both cell and operator limits. ") +
      "Each positive feed operation keeps its own upward-rounded time slot, including after attended work or waiting. " :
      "Concurrent operation requires independently qualified machines, fixtures and handling labor. Feed is rounded upward once per used cell. ") +
    "Each used cell includes its own availability, initial setup and final unloading; raw feed and padding remain separate. " +
    "Elapsed completion is distinct from summed machine work. The retained single-machine reference is not the selected concurrent order. " +
    (heterogeneous ? "Stitch pitch, feed, qualifications and thread consumption come from each selected cell's own admitted plan. " +
      "The allocation minimizes elapsed completion; its thread purchasing cost and total feed are reported consequences, not separately optimized quantities. " : "") +
    (operator ? "This calculation does not qualify garment transfers, unmodeled shared resources or hardware execution." :
      "This calculation does not qualify garment transfers, shared operators or hardware execution.");
  parent.append(note);
  detail(parent, `${label}: cell independence, timing sources and complete allocations`, {
    policy: plan.options?.production?.parallel, report });
}
function sewingAccess(parent, plan, label = "Selected sewing plan") {
  if (plan?.production?.parallel?.cell_plans?.length || plan?.production?.routing) return;
  const seams = plan?.seams?.filter(row => row.access || row.pass?.access) || [];
  if (!seams.length) return;
  const clearances = table(parent, `${label}: planar tool and fixture clearance`,
    ["Operation", "Result", "Tool radius (mm)", "Required gap (mm)", "Minimum gap (mm)", "Fixtures", "Complete path segments"]);
  const witnesses = table(parent, `${label}: limiting planar clearance witnesses`,
    ["Operation", "Fixture", "Needle segment", "Fixture edge", "Needle point (mm)", "Fixture point (mm)"]);
  const point = value => Array.isArray(value) ? `[${value.map(metric).join(", ")}]` : "Unknown";
  for (const seam of seams) {
    const report = seam.access, operation = seam.pass.id || seam.pass.seam;
    cells(clearances, [operation, report ? "Declared planar model checked" : "No retained clearance result",
      metric(report?.tool_radius_mm), metric(report?.required_clearance_mm), metric(report?.minimum_clearance_mm),
      metric(report?.fixture_count), metric(report?.checked_segments)]);
    if (report?.witness) {
      const w = report.witness;
      cells(witnesses, [operation, w.fixture, point(w.needle_segment), point(w.fixture_edge),
        point(w.needle_mm), point(w.fixture_mm)]);
    }
    detail(parent, `${label}: ${operation} tool geometry, assembly and qualification sources`, {
      policy: seam.pass.access, assembly: seam.assembly, report });
  }
  const note = document.createElement("p");
  note.textContent = "Clearance uses the declared circular tool envelope over every complete needle-path segment in the operation-local assembly frame. " +
    "Reverse and backtack runs revisit the same swept geometry. The source policy is tied to the exact machine and admitted material/process qualification. " +
    "These retained planar calculations do not establish fixture grip, three-dimensional approach clearance, machine registration or hardware authorization. " +
    "Operations without a retained result remain unchecked.";
  parent.append(note);
}
// Choices reference retained producer plans; preview geometry is always rendered by Rust.
function sewingPreviewCells(plan) {
  const routing = plan.production?.routing, parallel = plan.production?.parallel;
  if (routing) return (routing.cell_plans || []).map(source => ({ cell: source.cell, plan: source.plan,
    copies: [...new Set((routing.selected?.operations || []).filter(row => row.cell === source.cell).map(row => row.copy))],
    routedOperations: (routing.selected?.operations || []).filter(row => row.cell === source.cell) }));
  if (parallel) {
    const configured = !!parallel.cell_plans?.length;
    return (parallel.selected?.cells || []).map(cell => ({ cell: cell.cell, copies: cell.copies,
      plan: configured ? parallel.cell_plans.find(source => source.cell === cell.cell)?.plan : plan }));
  }
  return [{ cell: null, plan }];
}
function currentSewingPreviewCell() {
  if (!sewingPreviewSource) return null;
  return sewingPreviewSource.cells.find(cell => cell.cell ===
    (sewingPreviewSource.requiresCell ? $("sewPreviewCell").value : null)) || null;
}
function currentSewingPreviewOperation() {
  return currentSewingPreviewCell()?.plan?.seams?.find(row =>
    (row.pass.id ?? row.pass.seam) === $("sewPreviewOperation").value) || null;
}
function describeSewingPreviewSelection() {
  const cell = currentSewingPreviewCell(), operation = currentSewingPreviewOperation();
  if (!cell) {
    $("sewPreviewSummary").textContent = "Choose an explicit production cell to inspect its own retained source operations.";
    return;
  }
  if (!operation) {
    $("sewPreviewSummary").textContent = "Choose a retained operation. Separate passes over the same seam keep their own operation identities.";
    return;
  }
  const id = operation.pass.id ?? operation.pass.seam;
  const copies = cell.routedOperations ? [...new Set(cell.routedOperations.filter(row => row.operation_id === id).map(row => row.copy))] : cell.copies;
  $("sewPreviewSummary").textContent = `${id} on source seam ${operation.pass.seam}. ` +
    (copies ? (copies.length ? `Selected garment copy IDs: [${copies.join(", ")}]. ` : "No selected garment copies use this cell and operation; its retained source geometry is available for inspection. ") : "Reference geometry for this operation. ") +
    (sewingPreviewSource.selected ? "The preview uses the exact selected recipe options." : "The preview uses the exact evaluated sewing options.");
}
function refreshSewingPreviewOperations() {
  const select = $("sewPreviewOperation"); select.replaceChildren();
  const placeholder = document.createElement("option"); placeholder.value = "";
  placeholder.textContent = "Choose a sewing operation"; select.append(placeholder);
  for (const row of currentSewingPreviewCell()?.plan?.seams || []) {
    const option = document.createElement("option"), id = row.pass.id ?? row.pass.seam;
    option.value = id; option.textContent = `${id} — source seam ${row.pass.seam}`; select.append(option);
  }
  select.value = ""; describeSewingPreviewSelection();
}
function retainSewingPreview(result, request) {
  const inputs = request?.sewingInputs, selected = !!result.selection;
  const program = selected ? result.program : result;
  const options = selected ? result.selection.selected_options_json : inputs?.options;
  if (!inputs || typeof inputs.pattern !== "string" || typeof options !== "string" || !program?.plan) return;
  const plan = program.plan;
  sewingPreviewSource = { pattern: inputs.pattern, options, selected,
    requiresCell: !!(plan.production?.routing || plan.production?.parallel), cells: sewingPreviewCells(plan) };
  $("sewPreviewControls").hidden = false;
  $("sewPreviewCellLabel").hidden = !sewingPreviewSource.requiresCell;
  const select = $("sewPreviewCell"); select.replaceChildren();
  const placeholder = document.createElement("option"); placeholder.value = "";
  placeholder.textContent = "Choose a production cell"; select.append(placeholder);
  for (const cell of sewingPreviewSource.cells) {
    if (cell.cell === null) continue;
    const option = document.createElement("option"); option.value = cell.cell;
    option.textContent = `${cell.cell} — ${cell.copies?.length ? `garment copy IDs [${cell.copies.join(", ")}]` : "no selected garment copies"}`;
    select.append(option);
  }
  select.value = ""; refreshSewingPreviewOperations();
}
function sewingResult(parent, r) {
  if (r.selection) {
    sewingSelection(parent, [{ key: "Current bound pattern", sewing_selection: { status: "available", result: r.selection.report } }]);
    if (!r.program) {
      const note = document.createElement("p");
      note.textContent = "No complete sewing candidate was selected. Review the retained trials and source-qualified repairs; no stitch program was produced.";
      parent.append(note);
      return { schema: r.selection.report.schema, selected: false,
        search_complete: r.selection.report.search_complete, budget_exhausted: r.selection.report.budget_exhausted,
        unresolved_trials: r.selection.report.unresolved_trials, hardware_execution_authorized: false };
    }
    r = r.program;
  }
  const plan = r.plan, production = plan?.production, parallel = production?.parallel, routing = production?.routing;
  const order = parallel || routing ? null : production?.ordering || plan?.ordering;
  if (order) {
    const counts = table(parent, "Sewing setup changes — declared needle and thread identities",
      ["Schedule", "Needle changes", "Thread changes", "Total changes"]);
    for (const [label, changes] of [[order.baseline_replaced ? "Feasible replacement baseline" : "Original dependency order", order.baseline_changes],
      ["Selected order", order.selected_changes]]) {
      cells(counts, [label, metric(changes.needle_changes), metric(changes.thread_changes), metric(changes.total_changes)]);
    }
    const operations = table(parent, production ? "Selected cross-garment sewing order" : "Selected sewing operation order",
      ["Position", "Operation", "Source seam", "Needle", "Thread", "Required predecessors"]);
    const selected = production ? order.selected_order.map(id => {
      const operation = production.operations.find(o => o.id === id);
      return { ...plan.seams[operation.seam_index], productionOperation: operation };
    }) : plan.seams;
    for (const [i, row] of selected.entries()) {
      const pass = row.pass;
      cells(operations, [metric(i + 1), row.productionOperation?.id || pass.id || pass.seam,
        pass.seam, pass.needle, pass.thread, (row.productionOperation?.after || pass.after).join(", ") || "None declared"]);
    }
    const note = document.createElement("p");
    const objective = order.handling_timing ? "declared setup plus handling duration" :
      order.timing ? "declared setup duration" : "setup-change count";
    note.textContent = (order.optimality_proven ?
      `Minimum ${objective} proven for the declared operations, dependencies, initial setup${production?.workholding ? " and finite fixtures" : ""}. ` :
      `Best complete feasible schedule found; minimum ${objective} has not been proven. `) +
      (order.budget_exhausted ? "The search reached its declared work limit. " : "") +
      (order.handling_timing ? "Needle and thread changes remain separate counts; declared handling is included in the joint duration objective. " :
        "Needle and thread changes each count once. Handling, rethreading duration, assembly access and total manufacturing time are not inferred. ") +
      ((plan.options.production?.ordering || plan.options.ordering).initial_setup ? "The supplied initial setup is included in the counts." :
        "No initial setup was supplied, so installing the first setup is not counted.");
    parent.append(note);
    detail(parent, "Baseline order, selected order and bounded search result", order);
  }
  sewingWorkholding(parent, plan);
  sewingHandling(parent, plan);
  sewingParallel(parent, plan);
  sewingRouting(parent, plan);
  if (!parallel?.cell_plans?.length && !routing) sewingAccess(parent, plan);
  if (plan?.seams.some(row => row.source_span)) {
    const spans = table(parent, routing?.reference_scope ? "Seed-route partial-seam reference — one garment" :
      "Partial-seam operations — original source correspondence",
      ["Operation", "Source seam", "Side A range", "Side B range", "Full source lengths (mm)", "Traveled lengths (mm)", "Direction"]);
    for (const row of plan.seams) {
      const span = row.source_span, range = side => span ? `[${span.source_ranges[side].map(metric).join(", ")}]` : "Whole seam";
      cells(spans, [row.pass.id || row.pass.seam, row.pass.seam, range(0), range(1),
        span ? span.source_lengths_mm.map(metric).join(" / ") : "See full seam stations",
        span ? span.sewn_lengths_mm.map(metric).join(" / ") : "See full seam stations", row.pass.direction]);
      if (span) detail(parent, `${row.pass.id || row.pass.seam}: retained source span`, span);
    }
    const note = document.createElement("p");
    note.textContent = "Ranges are fractions of each original seam side in paired traversal order; side B includes the seam's reverse setting. " +
      "The planner checks complete coverage on both source sides. Overlapping passes can reinforce a join. " +
      "These ranges preserve the declared notch correspondence and do not authorize a physical fold transition or machine execution.";
    parent.append(note);
  }
  if (plan?.consumables) sewingConsumablesComparison(parent,
    [{ key: "Selected sewing plan", sewing: { status: "available", result: plan } }]);
  return { schema: r.schema, stitches: r.stitches?.length, production_copies: production?.production_copies,
    feed_seconds: production?.feed_seconds ?? plan?.feed_seconds,
    setup_seconds: order?.timing ? order.timing.selected_setup_milliseconds / 1000 : undefined,
    feed_and_setup_seconds: order?.timing?.feed_and_setup_seconds,
    handling_seconds: order?.handling_timing ? order.handling_timing.selected_handling_milliseconds / 1000 : undefined,
    modeled_production_seconds: order?.handling_timing?.selected_modeled_production_seconds,
    parallel_makespan_seconds: parallel ? parallel.selected.makespan_milliseconds / 1000 : undefined,
    parallel_lower_bound_seconds: parallel ? parallel.lower_bound_makespan_milliseconds / 1000 : undefined,
    parallel_optimality_proven: parallel?.optimality_proven,
    parallel_search_work: parallel?.search_nodes,
    routed_makespan_seconds: routing ? routing.selected.makespan_milliseconds / 1000 : undefined,
    routed_lower_bound_seconds: routing ? routing.lower_bound_makespan_milliseconds / 1000 : undefined,
    routed_optimality_proven: routing?.optimality_proven,
    routed_search_work: routing?.search_nodes,
    routed_calendar_resources: routing?.availability_calendars ? Object.keys(routing.availability_calendars.resources).length : undefined,
    garment_transfers: routing?.selected?.transfers?.length,
    transfer_seconds: routing ? routing.selected.transfer_milliseconds / 1000 : undefined,
    setup_changes: order?.selected_changes,
    ordering_objective: order ? (order.handling_timing ? "minimize_production_time" :
      order.timing ? "minimize_setup_time" : "minimize_setup_changes") : undefined,
    ordering_optimality_proven: order?.optimality_proven,
    finite_workholding_enforced: (routing ? routing.workholding : parallel ? parallel.selected.cells.some(cell => cell.schedule?.workholding) : production?.workholding) ? true : undefined,
    shared_fixture_pools_enforced: (routing?.workholding?.shared_workholding || parallel?.shared_workholding) ? true : undefined,
    planar_access_checked_operations: (routing ? new Set(routing.selected.operations
      .filter(operation => routing.cell_plans.find(source => source.cell === operation.cell)?.plan.seams[operation.seam_index]?.access)
      .map(operation => JSON.stringify([operation.cell, operation.seam_index]))).size : parallel?.cell_plans?.length ? parallel.selected.cells
      .filter(cell => cell.copies.length).reduce((count, cell) => count +
        (parallel.cell_plans.find(source => source.cell === cell.cell)?.plan.seams.filter(row => row.access).length || 0), 0) :
      plan?.seams?.filter(row => row.access).length) || undefined,
    search_nodes: order?.search_nodes, hardware_execution_authorized: plan?.hardware_execution_authorized };
}
function permanentStretchResult(parent, r) {
  const c = r.calibration, d = c.diagnostics;
  const recovery = c.input.material.membrane_relaxation;
  const identifiedRecovery = c.input.joint_relaxation;
  const joint = Boolean(identifiedRecovery || recovery?.parallel_plasticity);
  const decision = document.createElement("p");
  decision.textContent = `Permanent-stretch identification: ${c.status}. Held-out replay: ${c.check_status}. ` +
    c.decision_reasons.join(" ") + (c.check_refusal ? ` ${c.check_refusal.message}` : "");
  parent.append(decision);
  const errors = table(parent, "Cyclic load errors — independent whole histories",
    ["Partition", "Observed components", "Scaled RMS", "Scaled peak"]);
  for (const [name, quality] of [["Fit", c.fit], [`Held out: ${c.check_status}`, c.check]]) {
    cells(errors, [name, metric(quality?.components), metric(quality?.rms_scaled), metric(quality?.peak_scaled)]);
  }
  const parameters = table(parent, "Selected FIT coefficients and declared search bounds",
    ["Material direction", "Yield (N/m)", "Yield search range (N/m)", "Hardening (N/m)", "Hardening search range (N/m)"]);
  const domain = table(parent, "Source-declared total and accumulated Green-strain limits",
    ["Material direction", "Total strain limit", "Accumulated plastic-strain limit"]);
  const sourceQuantity = q => q ? `${metric(q.value)} ${q.unit}` : "Unknown";
  for (const [i, axis] of ["warp", "weft", "shear"].entries()) {
    const name = ["Warp / wale", "Weft / course", "Engineering shear"][i];
    const bounds = values => `[${values.map(metric).join(", ")}]`;
    cells(parameters, [name, metric(c.parameters?.yield_n_m[i]), bounds(c.input.yield_bounds_n_m[i]),
      metric(c.parameters?.hardening_n_m[i]), bounds(c.input.hardening_bounds_n_m[i])]);
    const properties = c.input.material.properties;
    cells(domain, [name, sourceQuantity(properties[`membrane_strain_limit_${axis}`]),
      sourceQuantity(properties[`membrane_accumulation_limit_${axis}`])]);
  }
  if (joint) {
    if (identifiedRecovery) {
      const spectrum = table(parent, "Jointly identified recovery — selected FIT coefficients",
        ["Branch / coefficient", "Selected value", "Search range", "FIT sensitivity per unit", "At search bound"]);
      const selected = c.parameters?.relaxation, diagnostics = d.joint_relaxation;
      for (const [i, label, value, bounds] of [
        [0, "Equilibrium-tensor stiffness ratio", selected?.stiffness_ratio, identifiedRecovery.stiffness_ratio_bounds],
        [1, "Relaxation time (s)", selected?.relaxation_time_seconds, identifiedRecovery.relaxation_time_bounds_seconds]
      ]) cells(spectrum, [`${identifiedRecovery.branch_id}: ${label}`, metric(value),
        `[${bounds.map(metric).join(", ")}]`,
        d.sensitivity_evaluated ? metric(diagnostics?.parameter_sensitivity[i]) : "Not evaluated",
        d.sensitivity_evaluated ? (diagnostics?.at_bounds[i] ? "Yes" : "No") : "Not evaluated"]);
      detail(parent, "Joint model, fixed material properties and original sources",
        { joint_relaxation: identifiedRecovery, fixed_properties: c.input.material.properties,
          replaced_recovery: recovery, sources: c.input.material.sources });
    } else {
      const spectrum = table(parent, "Delayed recovery — independently supplied, held fixed during identification",
      ["Branch", "Stiffness definition", "Relaxation time", "Time evidence", "Time source"]);
      for (const branch of recovery.branches || []) cells(spectrum,
        [branch.id, `Equilibrium-tensor ratio: ${sourceQuantity(branch.stiffness_ratio)}`,
          sourceQuantity(branch.relaxation_time), branch.relaxation_time.evidence, branch.relaxation_time.source]);
      for (const branch of recovery.orthotropic_branches || []) cells(spectrum,
        [branch.id, ["warp", "weft", "coupling", "shear"].map(axis =>
          `${axis}: ${sourceQuantity(branch.stiffness[axis])}`).join("; "),
          sourceQuantity(branch.relaxation_time), branch.relaxation_time.evidence, branch.relaxation_time.source]);
      detail(parent, "Fixed recovery spectrum, joint-network qualification and original sources",
        { recovery, sources: c.input.material.sources });
    }
    const endings = new Map();
    for (const prediction of c.predictions || []) if (prediction.parallel_history) endings.set(prediction.experiment, prediction);
    if (endings.size) {
      const histories = table(parent, "Final recorded state of each independent history — work per reference area",
        ["History / partition", "Equilibrium storage (J/m²)", "Maxwell storage (J/m²)", "Total storage (J/m²)",
          "Plastic loss (J/m²)", "Dashpot work (J/m²)"]);
      for (const prediction of endings.values()) {
        const state = prediction.parallel_history;
        cells(histories, [`${prediction.experiment} / ${prediction.partition}`, metric(prediction.stored_j_m2),
          metric(state.maxwell_stored_j_m2), metric(state.total_stored_j_m2),
          metric(prediction.plastic_dissipation_j_m2), metric(state.maxwell_dissipation_j_m2)]);
      }
    }
  }
  const work = table(parent, "Identification work and FIT excitation", ["Diagnostic", "Value"]);
  for (const row of [
    ["Completed / attempted starts", `${metric(d.starts_completed)} / ${metric(d.starts_attempted)}`],
    ["Forward evaluations / limit", `${metric(d.evaluations)} / ${metric(d.evaluation_limit)}`],
    ["Reserved history nodes / limit", `${metric(d.history_steps)} / ${metric(d.history_step_limit)}`],
    ["Refused forward trials", metric(d.trials_refused)],
    ["FIT excitation pivot", d.sensitivity_evaluated ? metric(d.excitation_pivot) : "Not evaluated"]
  ]) cells(work, row);
  if (identifiedRecovery) cells(work, ["Joint Maxwell branch-node advances (included above)", metric(d.joint_relaxation?.branch_steps)]);
  else if (joint) cells(work, ["Fixed Maxwell branch-node advances (included above)", metric(d.fixed_maxwell_branch_steps)]);
  const note = document.createElement("p");
  note.textContent = (identifiedRecovery ?
    "Six permanent-stretch and hardening coefficients and one Maxwell stiffness ratio and recovery time are identified together. " +
    "The FIT excitation check includes all eight unknowns; a locally unresolved combination cannot publish a material. " +
    "Every trial advances both histories at every recorded node, including nodes with unobserved forces. " +
    "The supplied equilibrium elasticity, domains and network interpretation remain fixed. " : joint ?
    "Only the six permanent-stretch and hardening coefficients are identified. The independently qualified Maxwell spectrum stays fixed. " +
    "Recorded elapsed times advance its delayed-recovery state, including nodes with unobserved forces; residuals compare the total parallel-network load. " :
    "The shared rate-independent law follows every recorded loading, unloading and reversal node, including nodes with unobserved forces. ") +
    "Strains are raw material-frame Green strain; loads are second-Piola sheet resultants in N/m. " +
    "FIT chooses the coefficients before CHECK replay. Search ranges, force scales and local sensitivity are not confidence intervals or a proof of global uniqueness. " +
    "Applicability limits remain supplied source observations; the fitter does not infer them from the largest load. Fitted coefficients remain estimates." +
    (joint ? " Equilibrium and Maxwell storage remain separate from plastic loss and physical dashpot work; none is automatically injected as heat." : "");
  parent.append(note);
  detail(parent, "Complete source histories, permanent strain, stored energy, dissipation and residuals", c);
  return { response: identifiedRecovery ? "Joint permanent stretch and delayed-recovery identification" :
    joint ? "Permanent stretch with independently characterized delayed recovery" :
    "Permanent stretch and directional hardening", status: c.status,
    held_out: c.check_status, input_sha256: c.input_sha256, material_id: r.selection?.id,
    allow_estimates: r.selection?.allow_estimates,
    mechanical_coefficients_available: r.material?.capabilities.mechanical_coefficients?.ready === true,
    mechanics_refusal: r.material?.capabilities.mechanical_coefficients?.refusal,
    next: r.selection ? "Review capabilities, then assign the identified selection above and bind the pattern." :
      "Review the retained diagnostics and source histories. No identified selection is available to assign." };
}
function areaCalibrationResult(parent, c) {
  const area = c.material?.membrane_response?.area_response, fittedRange = c.fitted_area_ratio_range;
  const declared = c.input.area_response?.area_ratio_range;
  const inherited = c.input.material?.membrane_response?.area_response?.area_ratio_range;
  if (!area && !fittedRange && !declared && !inherited) return;
  const preserved = c.input.area_response?.mode === "preserve";
  if (area) {
    const parameters = table(parent, preserved ? "Area coefficients — preserved source observations" :
      "Area coefficients — joint identification", ["Energy term", "Coefficient", "Unit", "Evidence", "Source"]);
    for (const [label, quantity] of [["(J − 1)²", area.quadratic], ["(ln J)²", area.log_squared]]) {
      cells(parameters, [label, metric(quantity.value), quantity.unit, quantity.evidence, quantity.source]);
    }
    const conditions = document.createElement("p");
    conditions.textContent = `Area specimen: ${area.conditions.specimen}. Conditioning: ${area.conditions.conditioning}.`;
    parent.append(conditions);
    detail(parent, "Area response, complete applicability conditions and source evidence", {
      response: area,
      sources: Object.fromEntries([...new Set([area.quadratic.source, area.log_squared.source])]
        .map(source => [source, c.material.sources?.[source] ?? null]))
    });
  }
  const domain = table(parent, "Physical area ratio J — applicability", ["Domain", "Minimum J", "Maximum J"]);
  if (declared) cells(domain, ["Admissible input domain", ...declared.map(metric)]);
  if (inherited) cells(domain, [declared ? "Inherited material domain" : "Admissible input domain", ...inherited.map(metric)]);
  if (fittedRange) cells(domain, ["FIT observations and undeformed reference", ...fittedRange.map(metric)]);
  if (area) cells(domain, ["Published material domain", ...area.area_ratio_range.map(metric)]);
  if (c.parameter_count != null) {
    const excitation = table(parent, "Joint identification diagnostics", ["Diagnostic", "Value"]);
    cells(excitation, ["Identified coefficients", metric(c.parameter_count)]);
    cells(excitation, ["Normalized FIT excitation pivot", metric(c.excitation_pivot)]);
  }
  const note = document.createElement("p");
  note.textContent = "J is deformed area divided by reference area. " + (preserved ?
    "The area coefficients, source evidence and area-ratio interval are retained from the supplied material. " :
    fittedRange ? "FIT observations determine the area coefficients and their supported J range, including the undeformed reference. CHECK cannot widen that range. " :
      "Only an accepted result publishes area coefficients and a supported J range. ") +
    (declared && inherited ? "Trials must satisfy both the declared and inherited J intervals. " : "") +
    "The area domain also intersects the directional Green-strain limits. An area-energy response does not impose constant area.";
  parent.append(note);
}
function stripCalibrationResult(parent, r) {
  const c = r.calibration, family = c.input.response_family, nonlinear = c.input.nonlinear;
  const quality = table(parent, "Clamped strip observations — independent partitions",
    ["Partition", "Observed channels", "Scaled RMS", "Scaled peak"]);
  for (const [name, q] of [["FIT", c.fit], ["CHECK", c.check]]) {
    cells(quality, [name, metric(q?.components), metric(q?.rms_scaled), metric(q?.peak_scaled)]);
  }
  const area = c.input.area_response, scalarArea = area && !family && !nonlinear;
  const parameterTitle = area?.mode === "identify" ? scalarArea ? "scalar base and physical-area coefficients" :
    "directional and physical-area coefficients" :
    scalarArea ? "scalar orthotropic base coefficients" :
    nonlinear ? "nonlinear sheet tangents and coupling" :
    family ? "nonlinear shape scales and coupling" : "orthotropic sheet coefficients";
  const parameters = table(parent, `${c.accepted ? "Identified" : "Candidate"} ${parameterTitle}`,
    ["Coefficient", "Value", "Unit", "Search minimum", "Search maximum"]);
  const descriptors = c.parameter_descriptors ?? [
    {label:"Warp / wale stiffness",unit:"N/m"},
    {label:"Weft / course stiffness",unit:"N/m"},
    {label:"Major Poisson ratio",unit:"1"},
    {label:"Shear stiffness",unit:"N/m"}
  ];
  const parameterBounds = c.parameter_bounds?.length ? c.parameter_bounds : c.input.parameter_bounds;
  for (const [i, descriptor] of descriptors.entries()) {
    cells(parameters, [descriptor.label, metric(c.parameters[i]), descriptor.unit,
      ...parameterBounds[i].map(metric)]);
  }
  if (scalarArea) {
    const support = table(parent, "Scalar base — declared trial strain support",
      ["Material direction", "Minimum Green strain", "Maximum Green strain"]);
    for (const [i, axis] of ["Warp / wale (E11)", "Weft / course (E22)", "Engineering shear (2 E12)"].entries()) {
      cells(support, [axis, ...area.scalar_green_strain_range[i].map(metric)]);
    }
    const scope = document.createElement("p");
    scope.textContent = "The four scalar coefficients describe the orthotropic base; physical-area energy is a separate contribution. " +
      "The base Poisson ratio is not the transverse response of the combined law. " +
      "These signed intervals constrain trials. Published support comes from FIT element strains and production replay.";
    parent.append(scope);
  }
  if (nonlinear) {
    const knots = table(parent, "Declared nonlinear Green-strain intervals",
      ["Material direction", "Fixed strain knots", "Independent tangent intervals"]);
    for (const [i, axis] of ["Warp / wale (E11)", "Weft / course (E22)", "Engineering shear (2 E12)"].entries()) {
      cells(knots, [axis, nonlinear.strain_knots[i].map(metric).join(", "),
        metric(nonlinear.tangent_bounds[i].length)]);
    }
    const scope = document.createElement("p");
    scope.textContent = "Every declared signed interval has its own fitted positive tangent. " +
      "The fixed knots are caller-supplied model choices; no compression branch or symmetry is inferred. " +
      "Accepted curves retain only the strain support established by FIT and verified by production replay.";
    parent.append(scope);
  }
  if (family) {
    const scope = document.createElement("p");
    scope.textContent = "The warp, weft and shear curve shapes and strain knots were supplied by the request. " +
      "The fit identifies three dimensionless shape scales and signed normal coupling in N/m" +
      (area?.mode === "identify" ? ", jointly with the two physical-area coefficients. " : ". ") +
      "It does not identify the curve shapes or impose area preservation. Shape evidence, acquisition observations and fitted outputs require their separate permissions.";
    parent.append(scope);
    detail(parent, "Declared nonlinear response shapes and source evidence", family);
  }
  if (area) {
    const scope = document.createElement("p");
    scope.textContent = area.mode === "identify" ?
      "The physical-area coefficients participate in the same joint fit, excitation and work limits. " +
        "Their declared J domain and FIT-only strain support remain authoritative; search bounds are not confidence intervals. " +
        (!c.input.material?.membrane_response?.area_response ? "No prior area coefficients were supplied." : "") :
      "The physical-area coefficients, J domain and source evidence remain fixed while directionality is identified.";
    parent.append(scope);
    const model = c.input.material?.membrane_response?.area_response;
    if (model) detail(parent, "Declared physical-area model and source conditions", model);
  }
  const geometry = table(parent, "Actual strip geometry and declared width comparison",
    ["Experiment", "Partition", "Width (m)", "Free gauge (m)", "Warp axis [across, along]", "Authored mesh cells", "Width observation"]);
  for (const e of c.input.experiments) {
    const observed = [];
    if (e.observations.some(o => o.mid_width != null)) observed.push("Chord at half the reference gauge");
    if (e.observations.some(o => o.extensometers?.length)) observed.push("Declared extensometers (see below)");
    cells(geometry, [e.id, e.partition, metric(e.fixture.width_m), metric(e.fixture.gauge_length_m),
      e.fixture.warp_axis.map(metric).join(", "), e.fixture.cells.join(" × "),
      observed.join("; ") || "Not observed"]);
  }
  if (c.input.experiments.some(e => e.observations.some(o => o.extensometers?.length))) {
    const measured = table(parent, "Declared transverse extensometer acquisitions",
      ["Experiment / observation", "Partition", "Channel", "Operator", "Reference station fraction", "Projection axis [across, along]",
        "Observed value (m)", "Residual scale (m)"]);
    for (const e of c.input.experiments) for (const o of e.observations) for (const channel of o.extensometers || []) {
      const operator = channel.operator;
      cells(measured, [`${e.id} / ${o.id}`, e.partition, channel.id,
        operator.kind === "edge_chord" ? "Euclidean edge chord" : "Signed edge projection",
        metric(operator.station_fraction), operator.axis?.map(metric).join(", ") || "Not applicable",
        metric(channel.value_m), metric(channel.scale_m)]);
    }
  }
  const method = document.createElement("p"); method.textContent = c.input.method; parent.append(method);
  if (c.fitted_green_strain_range) {
    const domain = table(parent, "Observed material strain range in FIT element fields",
      ["Material direction", "Minimum Green strain", "Maximum Green strain"]);
    for (const [i, axis] of ["Warp / wale (E11)", "Weft / course (E22)", "Engineering shear (2 E12)"].entries()) {
      cells(domain, [axis, ...c.fitted_green_strain_range[i].map(metric)]);
    }
  }
  areaCalibrationResult(parent, c);
  const work = table(parent, "Inverse solve and excitation diagnostics", ["Diagnostic", "Value"]);
  for (const row of [
    ["Identification outcome", c.accepted ? "ACCEPTED" : "NOT ACCEPTED"],
    ["Stationary / attempted starts", `${c.starts.filter(s => s.converged).length} / ${c.starts.length}`],
    ["Normalized excitation pivot", metric(c.excitation_pivot)],
    ["Forward solves / limit", `${metric(c.forward_solves)} / ${metric(c.input.policy.max_forward_solves)}`],
    ["Doubled-mesh change / limit", `${metric(c.mesh_change_peak_scaled)} / ${metric(c.input.policy.max_mesh_scaled)}`],
    ["Enforced-domain replay", c.domain_replay_verified ? "Verified" : "Not verified"]
  ]) cells(work, row);
  const predictions = table(parent, "Actual refined strip predictions and observation residuals",
    ["Experiment / observation", "Partition", "Grip extension", "Force (N)", "Force residual (N)", "Mid-width (m)", "Width residual (m)"]);
  for (const p of c.predictions) cells(predictions, [`${p.experiment} / ${p.observation}`, p.partition,
    metric(p.state.extension_fraction), metric(p.state.force_n), metric(p.force_residual_n),
    metric(p.state.mid_width_m), p.width_residual_m == null ? "Not observed" : metric(p.width_residual_m)]);
  if (c.predictions.some(p => p.extensometers?.length)) {
    const measured = table(parent, "Refined extensometer predictions and residuals",
      ["Experiment / observation", "Partition", "Channel", "Operator", "Reference station fraction", "Projection axis [across, along]",
        "Predicted value (m)", "Residual (m)"]);
    for (const p of c.predictions) for (const channel of p.extensometers || []) {
      const operator = channel.operator;
      cells(measured, [`${p.experiment} / ${p.observation}`, p.partition, channel.id,
        operator.kind === "edge_chord" ? "Euclidean edge chord" : "Signed edge projection",
        metric(operator.station_fraction), operator.axis?.map(metric).join(", ") || "Not applicable",
        metric(channel.predicted_m), metric(channel.residual_m)]);
    }
  }
  if (c.reasons.length) {
    const reasons = document.createElement("p"); reasons.textContent = c.reasons.join(" "); parent.append(reasons);
  }
  const note = document.createElement("p");
  note.textContent = "Each prediction solves the finite strip with clamped ends, free in-plane interior motion, and a prescribed planar quasistatic boundary. " +
    "Grip extension is not treated as uniform fabric strain. Transverse measurements need the declared reference station and chord or signed-projection definition; no source contraction is mapped automatically. " +
    "This existing reversible law cannot supply hysteresis, damage or rate dependence. FIT determines the parameters before CHECK replay. " +
    "Search bounds, force scales and local sensitivity are not confidence intervals or a global uniqueness proof.";
  parent.append(note);
  detail(parent, "Decision, complete source acquisitions, physical grip reactions, deformed widths and residuals", c);
  return { response: nonlinear ? "Nonlinear interval clamped strip" : "Finite clamped strip", status: c.accepted ? "ACCEPTED" : "NOT ACCEPTED",
    held_out: c.check ? "Evaluated; inspect independent residuals and acceptance reasons" : "Not supplied",
    input_sha256: c.input_sha256, material_id: r.selection?.id,
    allow_estimates: r.selection?.allow_estimates,
    mechanical_coefficients_available: r.material?.capabilities.mechanical_coefficients?.ready === true,
    mechanics_refusal: r.material?.capabilities.mechanical_coefficients?.refusal,
    next: r.selection ? "Review capabilities and model assumptions, then assign the identified selection and bind the pattern." :
      "Review the retained acquisition and fit diagnostics. No identified selection is available to assign." };
}
function measuredMechanicsResult(parent, r) {
  const c = r.calibration, friction = c.schema === "ftx-friction-calibration-report/1";
  const bending = c.schema === "ftx-bending-calibration-report/1";
  const pair = r.binding?.kind === "interface";
  const response = friction ? ({ pair: "Kinetic friction between two materials", self_contact: "Same-material kinetic friction",
    body: "Kinetic friction against a declared body counterface" })[c.input.target.kind] :
    bending ? "Directional bending and preferred curvature" : "Linear orthotropic stretch and shear";
  const errors = table(parent, friction ? "Observed sliding-force errors — independent acquisitions" :
    bending ? "Observed moment errors — independent partitions" : "Observed force errors — independent partitions",
  ["Partition", "Measured components", "Scaled RMS", "Scaled peak"]);
  for (const [name, quality] of [["Fit", c.fit], ["Held out", c.check]]) {
    cells(errors, [quality ? name : `${name}: not supplied`, metric(quality?.components), metric(quality?.rms_scaled), metric(quality?.peak_scaled)]);
  }
  if (friction) {
    const fitted = table(parent, "Effective isotropic kinetic coefficient", ["Target", "Coefficient", "Evidence", "Source"]);
    cells(fitted, [response, metric(c.coefficient.value), c.coefficient.evidence, c.coefficient.source]);
    const range = table(parent, "Observed FIT load and speed ranges", ["Observation", "Minimum", "Maximum"]);
    cells(range, ["Normal force (N)", ...c.fit_normal_n_range.map(metric)]);
    cells(range, ["Sliding speed (m/s)", ...c.fit_speed_m_s_range.map(metric)]);
    if (pair) {
      const admission = table(parent, "Exact contact pair and separate material readiness",
        ["Scope", "Source identity", "Coefficients admitted", "Reason"]);
      cells(admission, ["Contact interface", r.binding.interface.materials.join(" / "), r.readiness.ready ? "Yes" : "No",
        r.readiness.refusal?.message || "See the retained interface evidence"]);
      for (const sheet of r.readiness.sheets || []) cells(admission,
        ["Material mechanics", sheet.material_sha256, sheet.ready ? "Yes" : "No", sheet.refusal?.message || "Available"]);
    }
    const scope = document.createElement("p");
    scope.textContent = "These ranges describe the fitting observations; they do not enforce a runtime load or speed domain. " +
      "The fitted law describes constant isotropic kinetic friction, with no static-friction or condition-dependent response inferred. " +
      (pair ? "The pair has its own estimate admission. Its readiness does not establish either sheet's mechanical completeness or a successful garment simulation. " :
        c.input.target.kind === "body" ? "The declared counterface remains in the receipt; a tested surrogate does not establish human-skin behavior. " : "") +
      "Review the original acquisition sources and corrected forces before using the result.";
    parent.append(scope);
    detail(parent, "Exact contact binding, counterfaces and independent readiness", { binding: r.binding, readiness: r.readiness });
  } else {
    const directions = bending ? ["Warp / wale (κuu)", "Weft / course (κvv)", "Twist (2 κuv)"] :
      ["Warp / wale (E11)", "Weft / course (E22)", "Engineering shear (2 E12)"];
    const matrix = table(parent, bending ? "Directional bending tensor (N·m)" : "Linear membrane stiffness tensor (N/m)",
      ["Response direction", ...directions]);
    for (const [i, row] of (bending ? c.bending_nm : c.stiffness_n_m).entries()) cells(matrix, [directions[i], ...row.map(metric)]);
    if (bending) {
      const preferred = table(parent, c.input.fit_preferred_curvature ? "Identified preferred curvature" : "Preferred curvature used by the fit",
        ["Material direction", "Preferred curvature (1/m)"]);
      for (const [i, direction] of directions.entries()) cells(preferred, [direction, metric(c.preferred_curvature_m_inv[i])]);
    } else {
      const domain = table(parent, "Strain domain from fitting observations", ["Material direction", "Minimum Green strain", "Maximum Green strain"]);
      for (const [i, direction] of directions.entries()) cells(domain, [direction, ...c.fitted_strain_range[i].map(metric)]);
    }
    const excitation = document.createElement("p");
    excitation.textContent = `FIT excitation pivot: ${metric(c.excitation_pivot)}. ` + (bending ?
      "The fitted panel response retains independent seam bending and all unrelated material properties; missing properties remain unavailable." :
      "The fitted strain domain comes from FIT observations. CHECK observations do not widen it.");
    parent.append(excitation);
  }
  const note = document.createElement("p");
  note.textContent = "Scaled residuals use the source request's declared measurement scales, not confidence intervals. " +
    "An absent held-out partition is not a passed check. Fitted coefficients remain estimates; source identity does not authenticate the observations or establish measured garment fit.";
  parent.append(note);
  detail(parent, "Complete source observations, fitted response and independent residuals", c);
  return { response, material_id: r.selection?.id, allow_estimates: pair ? r.binding.interface.allow_estimates : r.selection?.allow_estimates,
    readiness: r.readiness, mechanical_coefficients_available: pair ? undefined : r.material?.capabilities.mechanical_coefficients?.ready === true,
    mechanics_refusal: pair ? undefined : r.material?.capabilities.mechanical_coefficients?.refusal,
    missing_mechanics: c.missing_mechanics, input_sha256: c.input_sha256,
    error_budgets: { max_rms_scaled: c.input.max_rms_scaled, max_peak_scaled: c.input.max_peak_scaled },
    next: pair ? "Assign the exact source materials to the garment, then stage this calibrated contact pair and bind. The inspected material selection is unchanged." :
      "Review capabilities, then assign the identified material selection and bind the pattern." };
}
function calibrationResult(parent, r) {
  if (r.calibration?.schema === "ftx-strip-calibration-report/1") return stripCalibrationResult(parent, r);
  if (r.calibration?.schema === "ftx-membrane-plasticity-calibration-report/1") return permanentStretchResult(parent, r);
  if (["ftx-swatch-calibration-report/1", "ftx-bending-calibration-report/1", "ftx-friction-calibration-report/1"].includes(r.calibration?.schema)) {
    return measuredMechanicsResult(parent, r);
  }
  if (r.comparison) {
    const comparison = r.comparison, decision = comparison.fit_decision;
    const models = table(parent, "Recovery model comparison — FIT choice before held-out checks",
      ["Model", "Declared parameters", "FIT outcome", "FIT RMS", "FIT peak", "FIT equivalent", "Held-out outcome"]);
    for (const candidate of comparison.candidates) {
      const quality = candidate.calibration?.fit;
      cells(models, [candidate.id, metric(candidate.declared_parameters), candidate.fit_status,
        metric(quality?.rms_scaled), metric(quality?.peak_scaled),
        decision.equivalent_candidate_ids.includes(candidate.id) ? "Yes" : "No", candidate.check_status]);
    }
    const explanation = document.createElement("p");
    explanation.textContent = `Comparison outcome: ${comparison.status}. ` +
      `FIT-equivalent alternatives: ${decision.equivalent_candidate_ids.join(", ") || "none"}. ` +
      `Choice before held-out replay: ${decision.chosen_candidate_id || "unresolved"}. ` +
      "Fewer declared parameters is a reported preference and does not resolve equivalent spectra automatically. " +
      "A failed held-out check rejects the same chosen candidate; it never chooses another model. " +
      "Force scales and the RMS equivalence tolerance are not confidence intervals.";
    parent.append(explanation);
    detail(parent, "Alternative spectra, prediction differences, source observations and shared numerical work", comparison);
    if (!r.calibration) return { response: "Directional recovery model comparison", status: comparison.status,
      fit_decision: decision, objective_evaluations: comparison.objective_evaluations,
      projected_iterations: comparison.projected_iterations,
      next: decision.ambiguous ? "Review the retained alternatives and supply select_candidate_id in a new comparison request to choose one explicitly. It must still pass held-out checks." :
        "Review the candidate refusals and acquisition errors. This comparison has no material selection to assign." };
  }
  const c = r.calibration, nonlinear = c.schema === "ftx-nonlinear-swatch-calibration-report/1";
  const timeSearch = c.orthotropic_fit?.time_search;
  const method = (c.input.plastic_equilibrium ? "Fixed permanent stretch with " : "") +
    (nonlinear ? c.material?.membrane_response?.area_response ? "Nonlinear stretch, shear and area response" :
    "Nonlinear stretch and shear" : c.input.orthotropic ?
    timeSearch ? "Directional relaxation with times identified within declared bounds" : "Directional relaxation at the declared times" :
    "Proportional relaxation ratios and times");
  const t = table(parent, "Observed force errors — independent partitions", ["Partition", "Measured components", "Scaled RMS", "Scaled peak"]);
  for (const [name, quality] of [["Fit", c.fit], ["Held out", c.check]]) {
    cells(t, [quality ? name : `${name}: not supplied`, metric(quality?.components), metric(quality?.rms_scaled), metric(quality?.peak_scaled)]);
  }
  const range = nonlinear ? c.fitted_strain_range : c.input.strain_range;
  const domain = table(parent, nonlinear ? "Strain domain from fitting observations" : "Source-declared strain applicability",
    ["Material direction", "Minimum Green strain", "Maximum Green strain"]);
  for (const [i, axis] of ["Warp / wale (E11)", "Weft / course (E22)", "Engineering shear (2 E12)"].entries()) {
    cells(domain, [axis, metric(range?.[i]?.[0]), metric(range?.[i]?.[1])]);
  }
  if (nonlinear) areaCalibrationResult(parent, c);
  if (c.input.plastic_equilibrium) {
    const known = table(parent, "Known permanent-stretch branch at every acquisition node",
      ["Acquisition", "Partition", "Time (s)", "Permanent Green strain [warp, weft, shear]",
        "Accumulated permanent strain", "Known nominal force (N/m)", "Step loss (J/m²)", "Cumulative loss (J/m²)"]);
    for (const p of c.predictions || []) {
      const fixed = p.plastic_equilibrium;
      if (!fixed) continue;
      cells(known, [p.experiment, p.partition, metric(p.time_seconds), fixed.plastic_strain.map(metric).join(", "),
        fixed.accumulated_strain.map(metric).join(", "),
        fixed.nominal_n_m.map(row => row.map(metric).join(", ")).join(" / "),
        metric(fixed.step_dissipated_j_m2), metric(fixed.cumulative_dissipated_j_m2)]);
    }
    const scope = document.createElement("p");
    scope.textContent = "The permanent-stretch coefficients and their evidence are independently supplied and remain fixed. " +
      "Each acquisition starts at its declared pristine reference and advances the plastic state at every recorded node, including nodes without observed force. " +
      "Only the Maxwell recovery branches are identified. Their convolution integrates the declared piecewise-linear Green strain analytically; " +
      "the plastic response is a recorded-node return map, without a continuous-path or refinement claim. " +
      "FIT chooses recovery parameters before CHECK replay. The combined network requires its own source qualification.";
    parent.append(scope);
    detail(parent, "Fixed permanent-stretch observations and combined-network qualification", {
      plastic_equilibrium: c.input.plastic_equilibrium,
      material_id: c.input.material?.id,
      fixed_properties: c.input.material?.properties,
      sources: c.input.material?.sources,
      time_integration: r.time_integration });
  }
  if (c.orthotropic_fit) {
    const recovery = table(parent, timeSearch ? "Directional recovery — published stiffness at identified times" :
      "Directional recovery — published stiffness at supplied times", ["Time (s)", "Retained branch",
      "Warp / wale (N/m)", "Weft / course (N/m)", "Normal coupling (N/m)", "Shear (N/m)"]);
    for (const row of c.orthotropic_fit.candidates) {
      cells(recovery, [metric(row.time_seconds), row.active ? "Active" : "Inactive", ...row.stiffness_n_m.map(metric)]);
    }
    if (timeSearch) {
      const windows = table(parent, "Directional time search — declared windows and local sensitivity",
        ["Minimum time (s)", "Identified time (s)", "Maximum time (s)", "Time sensitivity", "Log-window boundary distance"]);
      for (const [i, row] of c.orthotropic_fit.candidates.entries()) {
        const bounds = c.input.orthotropic.time_bounds_seconds[i];
        cells(windows, [metric(bounds[0]), metric(row.time_seconds), metric(bounds[1]),
          metric(timeSearch.time_sensitivity[i]), metric(timeSearch.boundary_distance_fraction[i])]);
      }
      const diagnostics = table(parent, "Directional time search — local optimizer diagnostics", ["Diagnostic", "Value"]);
      for (const row of [
        ["Converged starts / attempted", `${metric(timeSearch.starts_converged)} / ${metric(timeSearch.starts_attempted)}`],
        ["Iterations in selected solve", metric(timeSearch.iterations)],
        ["Objective evaluations across starts and trials", metric(timeSearch.objective_evaluations)],
        ["Projected tensor iterations across all solves", metric(timeSearch.projected_iterations)],
        ["Maximum scaled gradient", metric(timeSearch.gradient_scaled_max)],
        ["Joint stiffness / time excitation pivot", metric(timeSearch.joint_excitation_pivot)]
      ]) cells(diagnostics, row);
      const interpretation = document.createElement("p");
      interpretation.textContent = "Times were identified within the request's declared windows. Boundary distance is a fraction of the log-time window: 0 is an endpoint and 0.5 is its center. " +
        "Time sensitivity is the scaled force response to log-time changes with fitted tensors held fixed. The gradient measures the remaining objective slope after fitting the tensors; the joint excitation pivot uses column-normalized fit data. " +
        "Review local convergence and excitation alongside the held-out errors; these diagnostics do not establish a global optimum, a unique spectrum or confidence intervals.";
      parent.append(interpretation);
    }
    const scope = document.createElement("p");
    scope.textContent = (timeSearch ?
      "Every identified time must retain an observable recovery branch; individual directional coefficients may still be zero. " :
      "Candidate times are supplied by the request, not optimized from the histories. Inactive candidates remain visible; an entirely inactive spectrum retains only the supplied equilibrium. ") +
      "The table shows the published tensors after any explicit zero threshold. " +
      "The source-declared strain domain is not inferred from small residuals.";
    parent.append(scope);
    detail(parent, "Unconstrained, physically constrained and published candidate tensors (display only)", c.orthotropic_fit);
  }
  const note = document.createElement("p");
  note.textContent = "Scaled errors use the input's declared force scales; they are not confidence intervals. " +
    "An absent held-out partition is not a passed check. Fitted coefficients remain estimates, and coefficient availability does not establish a successful garment simulation or measured fit.";
  parent.append(note);
  if (!nonlinear) detail(parent, "Identified recovery branches and initial-state declaration (display only)",
    c.material.membrane_relaxation || { response: "Supplied equilibrium only; no active recovery branches." });
  detail(parent, "Complete observations, predictions, residuals and identification diagnostics (display only)", c);
  return { response: method, material_id: r.selection.id, allow_estimates: r.selection.allow_estimates,
    mechanical_coefficients_available: r.material.capabilities.mechanical_coefficients?.ready === true,
    mechanics_refusal: r.material.capabilities.mechanical_coefficients?.refusal,
    missing_mechanics: c.missing_mechanics, input_sha256: c.input_sha256,
    error_budgets: { max_rms_scaled: c.input.max_rms_scaled, max_peak_scaled: c.input.max_peak_scaled },
    next: "Review capabilities, then assign the identified selection above and bind the pattern." };
}
function stripHistoryResult(parent, r) {
  const note = document.createElement("p");
  note.textContent = "The prescribed grips load, hold and unload one material specimen. Accepted material history " +
    "persists throughout the program. Stored energy, free-vertex motion and dissipated work remain separate; " +
    "these values do not establish total actuator work, out-of-plane buckling or measured garment behavior.";
  parent.append(note);
  const parallel = r.input?.material?.record?.membrane_relaxation?.parallel_plasticity;
  if (parallel?.model === "parallel_green_maxwell_plastic/1") {
    const network = document.createElement("p");
    network.textContent = "Declared parallel network: the equilibrium branch retains permanent strain, while Maxwell branches retain their own viscous strains. " +
      "Plastic flow uses equilibrium-branch stress; Maxwell arms share the total material strain. " +
      "The report keeps plastic and Maxwell dissipation separate. Matched specimen and garment validation remain independent requirements.";
    parent.append(network);
    detail(parent, "Declared permanent-set and recovery network", {
      declaration: parallel, material_sha256: r.input.material.expected_sha256, model_scope: r.model_scope });
  }
  const states = table(parent, "Loading and recovery history", ["Time (s)", "Grip extension fraction",
    "Moving-grip force (N)", "Mid-gauge width (m)", "Dynamic residual (N)", "Stored energy (J)",
    "Free kinetic energy (J)", "Maxwell dissipation (J)", "Plastic dissipation (J)"]);
  for (const state of r.states) cells(states, [state.time_s, state.extension_fraction, state.force_n,
    state.mid_width_m, state.free_dynamic_residual_n, state.stored_membrane_energy_j,
    state.free_kinetic_energy_j, state.maxwell_dissipation_j, state.plastic_dissipation_j].map(metric));
  detail(parent, "Frozen material, fixture and complete loading program", r.input);
  return { schema: r.schema, input_sha256: r.input_sha256, steps: r.states.length,
    model_scope: r.model_scope,
    next: "Download the original request, complete report and exact frozen replay below." };
}
function stripResult(parent, op, r) {
  if (op === "stripAcquisition") {
    const curves = table(parent, "Original complete force acquisitions", ["Specimen", "Cut direction", "Original rows", "Archive entry", "Source SHA-256"]);
    for (const curve of r.curves) {
      cells(curves, [curve.specimen, curve.direction, metric(curve.observations.length), curve.entry, curve.sha256]);
      detail(parent, `${curve.specimen} / ${curve.direction}: original loading and unloading observations`, curve);
    }
    if (r.transverse_curves?.length) {
      const transverse = table(parent, "Original transverse acquisitions", ["Specimen", "Cut direction", "Original rows", "Archive entry", "Source SHA-256"]);
      for (const curve of r.transverse_curves) {
        cells(transverse, [curve.specimen, curve.direction, metric(curve.observations.length), curve.entry, curve.sha256]);
        detail(parent, `${curve.specimen} / ${curve.direction}: original transverse observations and measurement scope`, curve);
      }
      const note = document.createElement("p");
      note.textContent = "Transverse compression fractions retain the original source channels. Exact spatial sampling and loading/unloading tags are not reported; equivalence to the solver's mid-gauge width is unverified. These values are not converted to widths, local strains or Poisson ratios.";
      parent.append(note);
    }
    if (r.transverse_readme) detail(parent, "Original acquisition README, source identity and fractional units", r.transverse_readme);
    detail(parent, "Source protocol, declared conditions and unverified model assumptions", r.protocol);
    detail(parent, "Original specimen metadata and license", { metadata_json: r.metadata_json, license: r.license });
    return { acquisition_sha256: r.acquisition_sha256, archive_url: r.archive_url, retrieved_on: r.retrieved_on,
      free_gauge_length_m: r.protocol.free_gauge_length_m, reference_width_m: r.protocol.reference_width_m,
      next: "Review the complete source curves and protocol before authoring a separate experiment or calibration request. Grip extension does not establish local membrane strain or transverse deformation." };
  }
  if (op === "stripExperiment") {
    const states = table(parent, "Production-solver grip reactions", ["Grip extension fraction", "Moving-grip force (N)",
      "Fixed-grip force (N)", "Free-force residual (N)", "Grip imbalance (N)", "Mid-gauge width (m)", "Strain energy (J)"]);
    for (const state of r.states) cells(states, [state.extension_fraction, state.force_n, state.fixed_grip_force_n,
      state.free_force_residual_n, state.grip_balance_residual_n, state.mid_width_m, state.energy_j].map(metric));
    detail(parent, "Frozen request, nonuniform element strains and equilibrium positions", r);
    return { input_sha256: r.input_sha256, fixture: r.input.fixture, controls: r.input.controls, model_scope: r.model_scope };
  }
  return stripCalibrationResult(parent, r);
}
function show(op, msg, request) {
  const r = msg.result;
  if (op === "catalog") {
    $("catalog").replaceChildren();
    for (const record of r.materials) {
      const option = document.createElement("option"); option.value = record.id;
      option.textContent = `${record.name} · ${record.form} · ${record.property_count} observations`;
      $("catalog").append(option);
    }
  } else if (op === "inspect") {
    preparedBy = null; inspectResult(r);
  } else if (op === "garmentcode") {
    preparedImport = { pattern_json: r.pattern_json, fit_spec_json: r.fit_spec_json };
    $("useGarmentcodeFit").checked = false;
    const report = r.report, s = sections.get(op); s.section.hidden = false;
    s.summary.textContent = text({ status: report.status, scope: report.scope,
      source_units_per_meter: report.source_units_per_meter, counts: report.counts,
      source_sha256: report.source_sha256, annotations_sha256: report.annotations_sha256,
      circular_arc_approximations: report.circular_arcs?.curves.length,
      missing_semantics: report.missing_semantics, limitations: report.limitations });
    const note = document.createElement("p");
    note.textContent = "REVIEW_REQUIRED: conversion preserves supported evaluated geometry; it does not establish garment completeness or fit. " +
      "Review the missing construction information, then choose Use imported pattern to replace the current workbench pattern. " +
      (r.fit_spec_json ? "You can also select the supplied fit specification when adopting this pattern, or keep your own choice." : "No fit specification was supplied.");
    s.comparison.append(note);
    const panels = table(s.comparison, "Original panels and native physical pieces", ["Source panel", "Native piece", "Winding reversed"]);
    for (const panel of report.panels) cells(panels, [panel.source_panel, panel.piece, panel.winding_reversed ? "Yes" : "No"]);
    if (report.circular_arcs) {
      const arcs = report.circular_arcs, explanation = document.createElement("p");
      explanation.textContent = "Circular edges are imported as polylines under your explicit limits. " + arcs.scope;
      s.comparison.append(explanation);
      const t = table(s.comparison, "Circular-arc approximation: admitted bound / requested limit", [
        "Source panel / edge", "Native edge", "Radius (mm)", "Source arc", "Segments",
        "Distance bound / limit (mm)", "Length error bound / limit (mm)", "Polyline / circular length (mm)",
      ]);
      for (const arc of arcs.curves) cells(t, [
        `${arc.source_panel} / ${arc.source_edge}`, `${arc.piece}/${arc.edge}`, metric(arc.radius_mm),
        `${arc.large_arc ? "Major" : "Minor"}; ${arc.right ? "right" : "left"}`, metric(arc.segments),
        `${metric(arc.max_deviation_bound_mm)} / ${metric(arcs.policy.max_sagitta_mm)}`,
        `${metric(arc.length_error_bound_mm)} / ${metric(arcs.policy.max_length_error_mm)}`,
        `${metric(arc.polyline_length_mm)} / ${metric(arc.arc_length_mm)}`,
      ]);
    }
    detail(s.comparison, "Complete panel, edge, stitch, geometry and source review", report);
    for (const f of msg.files) link(op, s.links, f);
  } else if (op === "outfit") {
    outfit = { request_json: r.request_json, composition_json: r.composition_json };
    installPattern(r.pattern_json, `${r.composition.garments.length} garments in one composed outfit`, r.composition.garments);
    $("selfContact").checked = true;
    const s = sections.get(op); s.section.hidden = false;
    s.summary.textContent = text({ request_sha256: r.request_sha256,
      composed_pattern_sha256: r.composition.composed_pattern_sha256,
      garments: r.composition.garments.map(g => ({ id: g.id, pieces: g.pieces, attached_mass: g.attached_mass })),
      scope: r.scope });
    detail(s.comparison, "Original garment, piece, seam and accessory correspondence", r.composition);
    for (const f of msg.files) link(op, s.links, f);
  } else if (op === "bind") {
    bound = r;
    const pre = document.createElement("pre"); pre.textContent = text({ pattern_sha256: r.pattern_sha256, geometry_changed: r.geometry_changed,
      pieces: r.pieces, material_resolution: { without_self_contact: r.drape_materials_without_self_contact, with_self_contact: r.drape_materials_with_self_contact } });
    $("boundSummary").replaceChildren(pre);
    for (const f of msg.files) link(op, $("bindFiles"), f);
  } else if (op === "stageInterface") {
    ++interfaceRead; interfaceChange = r.interfaces_json; interfaceReady = true;
    $("interfacesFile").value = "";
    $("replaceCalibratedInterface").checked = false;
    edits(); clear(["bind", ...boundOperations]);
    $("interfacesSummary").textContent = `Staged calibrated pair in ${JSON.parse(r.interfaces_json).length} retained contact pair(s). Freeze selections & bind to apply.`;
  } else {
    const pairCalibration = op === "calibrate" && r.binding?.kind === "interface";
    if (op === "treatment") preparedTreatment = typeof r.pattern_json === "string" ? r.pattern_json : null;
    if (pairCalibration) {
      calibratedPair = r.interface_json;
      $("calibratedInterfaceSummary").textContent = `Calibrated pair: ${r.binding.interface.materials.join(" / ")}. Stage this pair after assigning those exact materials.`;
    }
    if (op === "pressure" || op === "linearizeOrigin" || (op === "calibrate" && !pairCalibration) || op === "stripFit") {
      clear(["thermalPoint"]);
      ++selectionRead; preparedBy = op;
      if (typeof r.selection_json === "string" && (op !== "stripFit" || r.calibration?.accepted === true)) {
        $("selection").value = r.selection_json;
        inspectResult(["pressure", "linearizeOrigin"].includes(op) ? r.inspection : r.material, r.selection_json);
      } else {
        $("selection").value = "";
        inspected = null;
        $("detail").textContent = "";
        $("capabilities").replaceChildren();
      }
    }
    const s = sections.get(op); s.section.hidden = false;
    if (op === "cut" || op === "stock") {
      knifeProgramResult(s.comparison, r.program, op === "stock" ? "Finite-stock plan" : "Cutting plan", r.program_refusal);
    }
    if (op === "fold") nappyOccupancyResult(s.comparison, r.body?.nappy_occupancy);
    if (op === "cutJob") cuttingJobComparison(op, s.comparison,
      [{ key: "Current bound pattern", cutting_job: { status: "available", result: r } }], false);
    const summary = ["screen", "study", "scenario"].includes(op) ? comparison(op, r) :
      op === "bodyCheck" ? bodyCheckResult(s.comparison, r) :
      op === "linearizeOrigin" ? originResult(s.comparison, r) :
      op === "calibrate" ? calibrationResult(s.comparison, r) :
      op === "stripHistory" ? stripHistoryResult(s.comparison, r) :
      ["stripAcquisition", "stripExperiment", "stripFit"].includes(op) ? stripResult(s.comparison, op, r) :
      op === "fold" ? { settled: r.settled, flags: r.flags, material: r.material?.basis,
        legs: r.body?.legs || "standing", leg_fraction: r.body?.leg_fraction ?? 1, settling: r.numerical?.settling,
        body_source: r.body?.shape_source || "Tape-fitted MODEL_FIT shape", body_fit: r.body?.fit,
        measurement_comparison: r.body?.measurement_comparison } :
      op === "cut" ? { pattern_sha256: r.pattern_sha256, maximum_marker_length_m: r.options?.maximum_marker_length_m,
        rolls: r.rolls.map((x, i) => ({ marker_number: i + 1, material: x.material?.authored?.id,
        width_m: x.width_m, ...(x.layout_width ? { layout_width: x.layout_width } : {}),
        length_m: x.length_m, stock_mass_kg: x.stock_mass_kg, utilization: x.utilization })), limitations: r.limitations } :
      op === "price" ? procurementResult(s.comparison, r) :
      op === "stockIdentities" ? stockIdentitiesResult(s.comparison, r) :
      op === "stockPrice" ? stockPriceResult(s.comparison, r) :
      op === "cutJob" ? { schema: r.schema, complete_inventory: r.complete_inventory,
        allocated_instances: r.inventory?.length, stock_area_m2: r.stock_area_m2,
        external_cut_area_m2: r.external_cut_area_m2, stock_mass_kg: r.stock_mass_kg,
        hardware_execution_authorized: r.hardware_execution_authorized } :
      op === "stock" ? { used_stock_count: r.used_stock_count, used_gross_area_m2: r.used_gross_area_m2,
        external_cut_area_m2: r.external_cut_area_m2, markers: r.markers.map(m => ({ id: m.id, pieces: m.placements.length,
          gross_area_m2: m.gross_area_m2, defect_free_area_m2: m.defect_free_area_m2, utilization: m.utilization,
          stock_mass_kg: m.stock_mass_kg, mass_unavailable: m.mass_unavailable })), limitations: r.limitations } :
      op === "sew" ? sewingResult(s.comparison, r) :
      op === "treatment" ? treatmentResult(s.comparison, r) :
      op === "vapor" ? vaporResult(s.comparison, r) :
      op === "thermalPoint" ? thermalPointResult(s.comparison, r) :
      op === "thermalHistory" ? thermalHistoryResult(s.comparison, r) :
      op === "vaporSurface" ? vaporSurfaceResult(s.comparison, r) :
      op === "heat" || op === "coverage" ? coverageResult(s.comparison, r) :
      op === "pressure" && ["ftx-compressed-thermal-history-report/1", "ftx-compressed-thermal-history-report/2", "ftx-compressed-thermal-history-report/3"].includes(r.schema) ? compressionHistoryResult(s.comparison, r) :
      op === "pressure" && r.schema === "ftx-compression-stack-report/1" ? {
        selected_layer: r.chosen_layer_id, pressure_pa: r.pressure_pa,
        common_pressure_range_pa: r.common_pressure_range_pa, requested_gap_m: r.input.gap_m,
        thickness_m: r.thickness_m, closure_residual_m: r.closure_residual_m,
        normal_force_n: r.normal_force_n, external_work_j: r.external_work_j,
        reference_mass_kg: r.reference_mass_kg, material_resistance_m2_k_w: r.material_resistance_m2_k_w,
        layers: r.layers.map(l => ({ id: l.id, thickness_m: l.thickness_m, closure_m: l.closure_m,
          external_work_j_m2: l.external_work_j_m2, unavailable: l.unavailable,
          discarded_properties: l.conditioned.discarded_properties })),
        no_claim: r.no_claim, next: "Assign the explicitly selected layer above. Change the layer ID and prepare again to select another layer." } :
      op === "pressure" ? { material_id: r.selection.id, pressure_pa: r.input.pressure_pa,
        pressure_range_pa: r.pressure_range_pa, bracket_pa: r.bracket_pa, interpolated: r.interpolated,
        discarded_properties: r.discarded_properties, discarded_responses: r.discarded_responses,
        discarded_structure: r.discarded_structure,
        next: "Review the prepared selection's capabilities, then assign it to the pattern above." } : r;
    s.summary.textContent = text(summary);
    for (const f of msg.files) {
      const url = link(op, s.links, f);
      if (op === "fold" && f.mime === "text/html") { $("viewer").src = url; $("viewer").hidden = false; }
      if (op === "cut" && f.mime === "image/svg+xml") { $("marker").src = url; $("marker").hidden = false; }
      if (op === "stock" && f.mime === "image/svg+xml") { $("stockMarker").src = url; $("stockMarker").hidden = false; }
      if (op === "sewPreview" && f.name === "sewing-preview.svg" && f.mime === "image/svg+xml") {
        $("sewingPreview").src = url; $("sewingPreview").hidden = false;
        $("sewingPreview").alt = `Independent flat source frames for sewing operation ${r.operation}${r.cell ? ` on cell ${r.cell}` : ""}`;
      }
      if (["screen", "study", "scenario"].includes(op) && ["screen-request.json", "study-request.json"].includes(f.name)) {
        detail(s.comparison, `${f.name}: declared objectives, evidence and process policy (display only)`, JSON.parse(f.text));
      }
    }
    // The parsed result is for display only: JSON.stringify changes 1.0/-0.0 and breaks locks.
    if (op === "fold") drape = msg.reportText;
    if (op === "sew") retainSewingPreview(r, request);
    if (op === "stock") {
      const residual = msg.files.find(f => f.name === "residual-stock-request.json")?.text;
      if (typeof residual === "string") {
        const adopt = document.createElement("button"); adopt.textContent = "Use planned remaining stock";
        adopt.onclick = () => {
          if (!ready || pending || !s.adoptions.includes(adopt)) return;
          selectStockRequest(residual);
        };
        const note = document.createElement("p");
        note.textContent = "Select this only when treating the displayed layout as consumed for your next planned job. " +
          "Consumed regions stay excluded. Existing price locks are retained and may need requalification; no physical cutting is recorded.";
        s.adoptions.push(adopt); s.comparison.append(note, adopt);
      }
    }
  }
}
async function read(id, binary = false) {
  if (id === "stockRequest" && selectedStockRequest !== null) return selectedStockRequest;
  if (!binary && roundFiles.has(id)) return roundFiles.get(id);
  const f = binary && roundFiles.has(id) ? roundFiles.get(id) : $(id).files[0];
  if (!f) throw Error(`Choose ${id} first.`);
  const limit = (id === "bodyFitFile" ? 16 : binary ? 256 : 64) * 1024 * 1024;
  const budgetMessage = id === "bodyFitFile" ? "Accepted body-fit report exceeds 16 MiB." : `${id} exceeds its input budget.`;
  if (f.size > limit) throw Error(budgetMessage);
  const bytes = await f.arrayBuffer();
  if (bytes.byteLength > limit) throw Error(budgetMessage);
  if (binary) return bytes;
  const raw = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes);
  JSON.parse(raw);
  return raw;
}
function sheetConditionsSource(id) {
  const mode = sheetConditionsMode(id);
  if (mode === "file") return roundFiles.has(id) ? roundFiles.get(id) : $(id).files[0];
  if (mode !== "form") throw Error("Choose authored uniform conditions or an original conditions file.");
  const form = sheetConditionForms[id];
  return form.build(Object.fromEntries(form.fields.map(([field, key, label]) => {
    const raw = $(field).value.trim();
    if (!raw || !Number.isFinite(Number(raw))) throw Error(`Supply a finite ${label}.`);
    return [key, Number(raw)];
  })));
}
function drapeArmAngle() {
  const angle = $("bodyArmDeg").value.trim();
  if (!angle || !Number.isFinite(Number(angle)) || Number(angle) < 0 || Number(angle) > 180) {
    throw Error("Supply a finite drape arm angle from 0 to 180 degrees.");
  }
  return Number(angle);
}
async function measurementInput(msg) {
  if (photoTransfer) throw Error("Finish the photo return or clear additional measurements before checking.");
  if (photoProblem) throw Error(photoProblem);
  const files = Array.from($("supplementalFiles").files || []);
  if (photoEvidence) files.unshift(photoEvidence);
  if (files.length && !$("supplementalConsent").checked) throw Error("Confirm that every added file describes this same wearer.");
  if (files.length > 8 || files.reduce((sum, f) => sum + f.size, 0) > 16 * 1024 * 1024) {
    throw Error("Choose at most 8 additional measurement files, 16 MiB combined.");
  }
  const primary = await read("measurements");
  if (files.length) {
    const sources = []; let bytes = 0;
    for (const f of files) {
      const buffer = await f.arrayBuffer(); bytes += buffer.byteLength;
      if (bytes > 16 * 1024 * 1024) throw Error("Additional measurements exceed 16 MiB.");
      const raw = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(buffer);
      JSON.parse(raw); sources.push(raw);
    }
    msg.measurementSources = JSON.stringify(sources); msg.measurementConsent = true;
  }
  return primary;
}
async function studyBodyInput(msg) {
  const mode = studyBodyMode();
  if (mode === "measurements") return measurementInput(msg);
  if (mode === "preview") {
    const fraction = $("drapeLegFraction").value.trim();
    const pose = { arm_abduction_deg: drapeArmAngle(), legs: $("drapeLegs").value, leg_fraction: Number(fraction) };
    if (!fraction || !["standing", "seated", "squat"].includes(pose.legs) ||
        !Number.isFinite(pose.leg_fraction) || pose.leg_fraction < 0 || pose.leg_fraction > 1) {
      throw Error("Choose standing, seated or squat legs and a finite leg fraction from 0 to 1.");
    }
    const measurements = await measurementInput(msg);
    if (msg.measurementSources !== undefined) { msg.measurementPrimary = measurements; msg.measurementPose = pose; }
    return '{"schema":"ftx-scenario-body/1","measurements":' + measurements + ',' + JSON.stringify(pose).slice(1);
  }
  if (mode === "request") return read("scenarioBody");
  if (mode !== "accepted") throw Error("Choose a supported study body source.");
  if (!has("bodyFitFile")) throw Error("Choose an accepted body-fit report for the selected study body source.");
  if (!$("bodyFitConsent").checked) throw Error("Associate the accepted fit with this wearer before running body-dependent studies.");
  const angle = drapeArmAngle(), fit = await read("bodyFitFile");
  const measurements = has("measurements") ? await measurementInput(msg) : "{}";
  if (!has("measurements") && (photoEvidence || photoTransfer || photoProblem || $("supplementalFiles").files.length)) throw Error("Choose primary measurements before adding evidence.");
  if (msg.measurementSources !== undefined) {
    msg.measurementPrimary = measurements; msg.measurementAcceptedFit = fit; msg.measurementArmDeg = angle;
  }
  // The existing Rust request is the authority. Embed validated source text without rounding its numbers.
  return '{"schema":"ftx-scenario-body/1","measurements":' + measurements +
    ',"accepted_body_fit":' + fit + ',"arm_abduction_deg":' + angle + '}';
}
async function readGarmentcode(id) {
  const f = $(id).files[0]; if (!f) throw Error(`Choose ${id} first.`);
  if (f.size > 4 * 1024 * 1024) throw Error("Each GarmentCode input is limited to 4 MiB.");
  const bytes = await f.arrayBuffer();
  if (bytes.byteLength > 4 * 1024 * 1024) throw Error("Each GarmentCode input is limited to 4 MiB.");
  let raw;
  try { raw = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes); }
  catch { throw Error(`${id} must be valid UTF-8; no replacement characters were substituted.`); }
  // Preserve every valid source byte. Retaining a BOM also preserves the native JSON refusal.
  JSON.parse(raw);
  return raw;
}
function circularArcAnnotations() {
  if (!$("garmentcodeArcs").checked) return undefined;
  const positive = (id, label) => {
    const raw = $(id).value.trim(), value = Number(raw);
    if (!raw || !Number.isFinite(value) || value <= 0) throw Error(`Supply a positive finite ${label} in millimetres.`);
    return value;
  };
  return text({ schema: "fabric-garmentcode-annotations/1", circular_arcs: {
    model: "bounded_chord_polylines/1",
    max_sagitta_mm: positive("garmentcodeArcSagitta", "circular-arc distance tolerance"),
    max_length_error_mm: positive("garmentcodeArcLength", "circular-edge length tolerance"),
  } }) + "\n";
}
function changes() {
  const pieces = [...pieceChanges].map(([id, selection]) => `${JSON.stringify(id)}:${selection}`).join(",");
  const interfaces = interfaceChange === null ? "" : `,"interfaces":${interfaceChange}`;
  return `{"schema":"ftx-material-bindings/1","default":${defaultChange || "null"},"pieces":{${pieces}}${interfaces}}`;
}
function edits() { $("edits").textContent = text(JSON.parse(changes())); }
function hasStudySelection(raw) {
  const selection = JSON.parse(raw);
  return studyCandidates.some(candidate => {
    if (candidate.selection === raw) return true;
    const current = JSON.parse(candidate.selection);
    return typeof selection.expected_sha256 === "string" && selection.id === current.id &&
      selection.expected_sha256 === current.expected_sha256;
  });
}
function addStudySelection(selection, suggestedKey = "") {
  if (!source) throw Error("Load the original pattern before adding comparison materials.");
  if (studyCandidates.length >= 7) throw Error("Keep at most seven material choices plus the original baseline.");
  if (hasStudySelection(selection)) throw Error("This exact frozen material is already in the shortlist.");
  let key = suggestedKey;
  if (typeof key !== "string" || !key.trim() || key === "baseline" || new Blob([key]).size > 128 || studyCandidates.some(candidate => candidate.key === key)) {
    do { key = `material-${++studyCandidateSequence}`; } while (studyCandidates.some(candidate => candidate.key === key));
  }
  studyCandidates.push({ key, selection }); $("studyMode").value = "guided";
  showStudyCandidates(); invalidate(["study", "scenario"], true);
}
function guidedScreenInput(attributes = guidedScreenAttributes) {
  return { query: $("guidedScreenQuery").value, forms: $("guidedScreenForm").value ? [$("guidedScreenForm").value] : [],
    basis: $("guidedScreenBasis").value, allowCatalogEstimates: $("guidedScreenEstimates").checked,
    allowMixedProtocols: $("guidedScreenMixed").checked,
    requiredCapabilities: [["guidedScreenMechanics", "mechanics"], ["guidedScreenSteady", "steady_conduction"],
      ["guidedScreenThermal", "transient_thermal"]].filter(([id]) => $(id).checked).map(([, capability]) => capability),
    attributes };
}
function showGuidedScreenAttributes() {
  const parent = $("guidedScreenAttributes"); parent.replaceChildren();
  for (const [index, attribute] of guidedScreenAttributes.entries()) {
    const row = document.createElement("div"), label = document.createElement("span"), remove = document.createElement("button");
    row.className = "study-choice";
    label.textContent = `${attribute.key} (${attribute.unit}): ` +
      `${attribute.minimum ?? "no minimum"} to ${attribute.maximum ?? "no maximum"}` +
      (attribute.direction ? `; ${attribute.direction}` : "") +
      (attribute.method ? `; exact method ${attribute.method}` : "") +
      (attribute.conditions ? `; conditions ${attribute.conditions}` : "");
    remove.type = "button"; remove.textContent = `Remove property ${index + 1}`;
    remove.onclick = () => {
      const current = guidedScreenAttributes.indexOf(attribute); if (current < 0) return;
      guidedScreenAttributes.splice(current, 1); showGuidedScreenAttributes();
      if (guidedScreenMode()) invalidate(["screen"], true); else controls();
    };
    row.append(label, remove); parent.append(row);
  }
  $("guidedScreenAttributeCount").textContent = `${guidedScreenAttributes.length} property rules added. Only added rules run.`;
}
function addGuidedScreenAttribute() {
  try {
    const property = $("guidedScreenProperty").value;
    const row = { key: property === "custom" ? $("guidedScreenKey").value : property, unit: $("guidedScreenUnit").value };
    for (const [id, key] of [["guidedScreenMinimum", "minimum"], ["guidedScreenMaximum", "maximum"]]) {
      if ($(id).validity?.badInput) throw Error("Correct the incomplete property limit before adding this rule.");
      const raw = $(id).value.trim();
      if (raw) { if (!Number.isFinite(Number(raw))) throw Error("Property limits must be finite numbers."); row[key] = Number(raw); }
    }
    if ($("guidedScreenDirection").value) row.direction = $("guidedScreenDirection").value;
    if ($("guidedScreenMethod").value !== "") row.method = $("guidedScreenMethod").value;
    if ($("guidedScreenConditions").value !== "") row.conditions = $("guidedScreenConditions").value;
    const attributes = [...guidedScreenAttributes, row];
    // Validate the draft independently of the still-unselected run basis.
    buildMaterialScreenRequest({ ...guidedScreenInput(attributes), basis: $("guidedScreenBasis").value || "nominal" });
    guidedScreenAttributes.push(row); showGuidedScreenAttributes();
    if (guidedScreenMode()) invalidate(["screen"], true); else controls();
  } catch (e) { error(String(e.message || e)); }
}
function showStudyCandidates() {
  const parent = $("studyCandidates"); parent.replaceChildren();
  for (const candidate of studyCandidates) {
    const row = document.createElement("div"), label = document.createElement("span"), remove = document.createElement("button");
    row.className = "study-choice";
    const selection = JSON.parse(candidate.selection); // Display only; the retained string is the input authority.
    label.textContent = `${candidate.key}: ${selection.id} (${selection.allow_estimates ? "estimates explicitly admitted" : "estimates disabled"})`;
    remove.type = "button"; remove.textContent = `Remove ${candidate.key}`;
    remove.onclick = () => {
      const index = studyCandidates.indexOf(candidate);
      if (index < 0) return;
      studyCandidates.splice(index, 1); showStudyCandidates();
      if (guidedStudyMode()) invalidate(["study", "scenario"], true); else controls();
    };
    row.append(label, remove); parent.append(row);
  }
  $("studyCandidateCount").textContent = studyCandidates.length ?
    `${studyCandidates.length} frozen material choice${studyCandidates.length === 1 ? "" : "s"} plus the original baseline; limit seven choices.` :
    "No comparison materials added. Load a pattern, then inspect and add materials in section 1.";
  refreshGuidedDesignMaterials();
}
function guidedDesignMaterialSource(key) {
  return key === "baseline" ? sourceText : studyCandidates.find(candidate => candidate.key === key)?.selection ?? null;
}
function refreshGuidedDesignMaterials() {
  const select = $("guidedDesignMaterial"), retained = select.value;
  if (guidedDesignNomination && guidedDesignMaterialSource(guidedDesignNomination.key) !== guidedDesignNomination.source) {
    guidedDesignNomination.invalidated = true;
  }
  const choices = [["", "Choose an explicit material"], ["baseline", "Original pattern materials (baseline)"],
    ...studyCandidates.map(candidate => [candidate.key, candidate.key])];
  select.replaceChildren();
  for (const [key, label] of choices) {
    const option = document.createElement("option"); option.value = key; option.textContent = label; select.append(option);
  }
  select.value = guidedDesignNomination?.invalidated ? "" : retained;
  $("guidedDesignMaterialStatus").textContent = guidedDesignNomination?.invalidated ?
    `The nominated source for ${guidedDesignNomination.key} was removed or changed. Choose a current material explicitly, even if that name has been reused.` : "";
  $("guidedDesignMaterialStatus").hidden = !guidedDesignNomination?.invalidated;
}
function scenarioSourceFile() {
  return roundFiles.has("scenarioRequest") ? roundFiles.get("scenarioRequest") : $("scenarioRequest").files[0];
}
function currentGuidedScenarioCases() {
  return guidedMatrixMode() ? { original: guidedScenarioDraftSource, keys: guidedScenarioDrafts.map(item => item.key) } : guidedScenarioCases;
}
function scenarioSourceIdentity() { return guidedMatrixMode() ? guidedScenarioDraftSource : scenarioSourceFile(); }
function refreshGuidedDesignCases() {
  const cases = currentGuidedScenarioCases();
  if (guidedDesignCaseNomination && (!guidedScenarioMode() || guidedMatrixMode()) &&
    guidedDesignCaseNomination.source !== scenarioSourceIdentity()) guidedDesignCaseNomination.invalidated = true;
  const select = $("guidedDesignCase"); select.replaceChildren();
  for (const key of ["", ...(cases?.keys || [])]) {
    const option = document.createElement("option"); option.value = key;
    option.textContent = key || "Choose an explicit named case"; select.append(option);
  }
  select.value = guidedDesignCaseNomination && !guidedDesignCaseNomination.invalidated ? guidedDesignCaseNomination.key : "";
  $("guidedDesignCaseStatus").textContent = cases?.problem ||
    (guidedDesignCaseNomination?.invalidated ? "The scenario source changed. Choose a current case explicitly, even if its name is unchanged." :
      cases?.keys?.length ? "Discovery follows the selected case's comparison policy. Every case must still admit each trial." :
        guidedMatrixMode() ? "Save at least one named case below to choose it for guidance." :
          cases ? "This is a single scenario; a named guidance case is not needed." : "Load a named scenario matrix below to choose a case.");
}
async function loadGuidedScenarioCases() {
  const generation = ++guidedScenarioCasesRead, original = scenarioSourceFile();
  guidedScenarioCases = null;
  if (guidedDesignCaseNomination) guidedDesignCaseNomination.invalidated = true;
  refreshGuidedDesignCases();
  if (!original) { controls(); return; }
  $("guidedDesignCaseStatus").textContent = "Reading the named scenario cases…";
  controls();
  try {
    const request = await read("scenarioRequest"), value = JSON.parse(request);
    if (generation !== guidedScenarioCasesRead || original !== scenarioSourceFile()) return;
    let keys = [];
    if (value?.schema === "ftx-drape-scenario-matrix/1") {
      if (!Array.isArray(value.cases) || value.cases.length < 1 || value.cases.length > 8 ||
        value.cases.some(item => typeof item?.key !== "string" || !item.key.trim() || new Blob([item.key]).size > 128)) {
        throw Error("A named matrix needs 1–8 cases with nonempty names of at most 128 UTF-8 bytes.");
      }
      keys = value.cases.map(item => item.key);
      if (new Set(keys).size !== keys.length) throw Error("Named scenario cases must have unique names.");
    } else if (value?.schema !== "ftx-drape-scenario/1") {
      throw Error("Choose an ordinary scenario or named scenario matrix.");
    }
    guidedScenarioCases = { original, keys };
  } catch (e) {
    if (generation !== guidedScenarioCasesRead || original !== scenarioSourceFile()) return;
    guidedScenarioCases = { original, keys: [], problem: String(e.message || e) };
  }
  refreshGuidedDesignCases(); controls();
}
function guidedComparisonInput(number, stockMode = guidedStockMode()) {
  if (!["roll", "finite", "job"].includes(stockMode)) throw Error("Choose roll, finite stock or a complete cutting job.");
  for (const id of ["guidedContactResultant", "guidedContactVertex", "guidedMotionContactResultant", "guidedMotionContactVertex"]) {
    if ($(`${id}Limit`).value.trim() && number(`${id}Limit`, "body contact force limit in newtons") < 0) {
      throw Error("Body contact force limits must be nonnegative newtons.");
    }
  }
  for (const [id, metric] of guidedTransportMetrics) {
    const signed = metric === "body_coverage_power_w" || metric.startsWith("transient_");
    for (const suffix of ["MinimumLimit", "Limit"]) {
      const input = $(`${id}${suffix}`);
      if (input.validity?.badInput) throw Error(`Supply a finite limit for ${metric}.`);
      if (!signed && input.value.trim() && number(`${id}${suffix}`, `limit for ${metric}`) < 0) {
        throw Error("Water and vapor limits must be nonnegative in the displayed units.");
      }
    }
  }
  const metrics = guidedMetrics.filter(([, , scope]) => guidedStockScope(scope, stockMode));
  const objectives = metrics.filter(([id]) => $(`${id}Goal`).checked).map(([id, metric]) => ({ metric,
    direction: id === "guidedHeatPower" ? $("guidedHeatDirection").value : "minimize" }));
  objectives.push(...guidedDirectionalMetrics.filter(([id]) => $(`${id}Goal`).checked)
    .map(([, metric, direction]) => ({ metric, direction })));
  objectives.push(...guidedTransportMetrics.filter(([id]) => $(`${id}Goal`).checked)
    .map(([id, metric]) => ({ metric, direction: $(`${id}Direction`).value })));
  const limitBasis = $("guidedLimitBasis").value || "inherit";
  if (!["inherit", "nominal", "reported_bounds"].includes(limitBasis)) throw Error("Choose the basis for comparison limits.");
  const constraints = metrics.filter(([id]) => $(`${id}Limit`).value.trim() ||
    (id === "guidedHeatPower" && $("guidedHeatMinimumLimit").value.trim())).map(([id, metric]) => ({
    metric, ...($(`${id}Limit`).value.trim() ? { maximum: number(`${id}Limit`, `maximum for ${metric}`) } : {}),
    ...(id === "guidedHeatPower" && $("guidedHeatMinimumLimit").value.trim() ?
      { minimum: number("guidedHeatMinimumLimit", "minimum signed sheet heat in watts") } : {}),
    ...(limitBasis === "inherit" ? {} : { basis: limitBasis }),
  }));
  constraints.push(...guidedDirectionalMetrics.filter(([id]) => $(`${id}Limit`).value.trim())
    .map(([id, metric, , bound]) => ({ metric, [bound]: number(`${id}Limit`, `${bound} for ${metric}`),
      ...(limitBasis === "inherit" ? {} : { basis: limitBasis }) })));
  constraints.push(...guidedTransportMetrics.filter(([id]) => $(`${id}Limit`).value.trim() || $(`${id}MinimumLimit`).value.trim())
    .map(([id, metric]) => ({ metric,
      ...($(`${id}MinimumLimit`).value.trim() ? { minimum: number(`${id}MinimumLimit`, `minimum for ${metric}`) } : {}),
      ...($(`${id}Limit`).value.trim() ? { maximum: number(`${id}Limit`, `maximum for ${metric}`) } : {}),
      ...(limitBasis === "inherit" ? {} : { basis: limitBasis }) })));
  const requiredConsumers = guidedRequired.filter(([id, , scope]) => guidedStockScope(scope, stockMode) && $(id).checked).map(([, consumer]) => consumer);
  return { objectives, constraints, requiredConsumers, basis: $("guidedBasis").value };
}
async function guidedStudyInput(scenarioRead) {
  if (!source) throw Error("Load the original pattern first.");
  const number = (id, label) => {
    const raw = $(id).value.trim();
    if (!raw || !Number.isFinite(Number(raw))) throw Error(`Supply a finite ${label}.`);
    return Number(raw);
  };
  const scope = $("studyTarget").value;
  const target = scope === "default" ? { scope: "default" } : scope === "all" ?
    { scope: "pieces", ids: source.pieces.map(p => p.id) } : scope === "pieces" ?
      { scope: "pieces", ids: Array.from($("studyPieces").selectedOptions, option => option.value) } : null;
  if (!target) throw Error("Choose the pattern assignment to compare.");
  const allowance = $("guidedAllowance").value.trim() ?
    { seam_allowance_mm: number("guidedAllowance", "seam allowance in millimetres") } : {};
  const stockMode = guidedStockMode();
  if (!["roll", "finite", "job"].includes(stockMode)) throw Error("Choose roll, finite stock or a complete cutting job.");
  const rollSource = $("guidedCuttingSource").value || "form";
  if (stockMode === "roll" && !["form", "file"].includes(rollSource)) throw Error("Choose the roll form or original roll-planning file.");
  const cutting = $("guidedCutting").checked && stockMode === "roll" && rollSource === "form" ? {
    copies: number("guidedCopies", "whole garment copy count"), gap_mm: number("guidedGap", "piece gap in millimetres"),
    edge_margin_mm: number("guidedEdge", "side margin in millimetres"), end_margin_mm: number("guidedEnd", "end margin in millimetres"),
    ...allowance,
  } : undefined;
  // Freeze controls AND files before any read. execute() rejects an obsolete request after edits;
  // original UTF-8 fragments reach Rust without JS number reserialization or BOM replacement.
  const input = { candidates: studyCandidates.map(candidate => ({ ...candidate })), target,
    pieceIds: source.pieces.map(p => p.id), ...(cutting ? { cutting } : {}), ...guidedComparisonInput(number, stockMode) };
  if ($("guidedDesign").checked) {
    input.design = guidedDesignInput(number, !!scenarioRead);
    if (guidedDesignAfter.length) input.seamIds = guidedDesignSeamIds();
  }
  const nominatedCaseSource = input.design?.caseKey ? guidedDesignCaseNomination?.source : undefined;
  const retained = [], budget = 16 * 1024 * 1024;
  let sourceBytes = 0;
  const retain = (key, id, label) => {
    const original = id === "stockRequest" && selectedStockRequest !== null ? selectedStockRequest :
      roundFiles.has(id) ? roundFiles.get(id) : $(id).files[0];
    if (!original) throw Error(`Choose ${label} first.`);
    const size = typeof original === "string" ? new Blob([original]).size : original.size;
    if (key === "designFit" && size > 256 * 1024) throw Error("Guided alteration fit contract exceeds 256 KiB; no source was read.");
    sourceBytes += size;
    if (sourceBytes > budget) throw Error("Guided study source files exceed 16 MiB combined; no source was read.");
    retained.push({ key, original });
  };
  if (input.design) retain("designFit", "fit", "the original section-3 garment fit contract for linked alterations");
  if ($("guidedCutting").checked) {
    if (stockMode === "roll" && rollSource === "file") retain("cutting", "cutOptions", "the original roll-planning options in section 3");
    if (stockMode === "finite") retain("finiteStock", "stockRequest", "the original finite-stock inventory in section 3");
    if (stockMode === "job") retain("cuttingJob", "cutJobRequest", "the original complete cutting job in section 3");
  }
  if ($("guidedSewing").checked) retain($("sewOptimize").checked ? "sewingSelection" : "sewing", "sewOptions", "the original sewing request in section 3");
  if (stockMode === "roll" && $("guidedPurchasing").checked) {
    retain("quotes", "quotes", "the dated quote book in section 3");
    retain("market", "market", "the comparable market in section 3");
  }
  if (stockMode !== "finite" && $("guidedCombinedCost").checked) input.cutAndSewPurchasing = true;
  if ($("guidedMass").checked) {
    if (has("guidedInventory")) retain("assemblyMass", "guidedInventory", "the sourced assembly inventory");
    else input.assemblyMass = { schema: "ftx-assembly-mass/1", inventory_complete: $("guidedInventoryComplete").checked, extras: [], ...allowance };
  }
  let decodedBytes = 0;
  const [sources, scenario] = await Promise.all([Promise.all(retained.map(async ({ key, original }) => {
    const raw = typeof original === "string" ? original : new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(await original.arrayBuffer());
    const size = new Blob([raw]).size;
    if (key === "designFit" && size > 256 * 1024) throw Error("Guided alteration fit contract exceeds 256 KiB.");
    decodedBytes += size;
    if (decodedBytes > budget) throw Error("Guided study source files exceed 16 MiB combined.");
    JSON.parse(raw);
    return [key, raw];
  })), scenarioRead]);
  const fragments = Object.fromEntries(sources);
  if (input.design?.caseKey && nominatedCaseSource !== scenario?.original) {
    throw Error("The nominated guidance case belongs to a different scenario source. Choose a current case and run the scenario comparison.");
  }
  if (input.design) input.design.fitSpec = fragments.designFit;
  for (const key of ["cutting", "finiteStock", "cuttingJob", "sewing", "sewingSelection", "assemblyMass"]) {
    if (Object.hasOwn(fragments, key)) input[key] = fragments[key];
  }
  if (Object.hasOwn(fragments, "quotes")) input.purchasing = { quotes: fragments.quotes, market: fragments.market };
  const finiteStock = fragments.finiteStock ? JSON.parse(fragments.finiteStock) : null;
  const cuttingJob = fragments.cuttingJob ? JSON.parse(fragments.cuttingJob) : null;
  const retainedPolicies = [fragments.cutting ? JSON.parse(fragments.cutting) : null,
    finiteStock?.options, cuttingJob?.roll, cuttingJob?.finite_stock?.options,
    fragments.assemblyMass ? JSON.parse(fragments.assemblyMass) : null].filter(Boolean);
  if (Object.hasOwn(allowance, "seam_allowance_mm") &&
    retainedPolicies.some(policy => policy.seam_allowance_mm !== allowance.seam_allowance_mm)) {
    throw Error("Retained cutting routes and inventory files must use the same explicit seam allowance override; their source policy is not rewritten.");
  }
  return buildMaterialStudyRequest(input, scenario?.context);
}
function describeScenarioInput(request, original) {
  const value = JSON.parse(request), matrix = value?.schema === "ftx-drape-scenario-matrix/1";
  const cases = matrix && Array.isArray(value.cases) ? value.cases.map(item => item?.scenario) : [value];
  return { request, original, context: { drape: true, surface: cases.some(item => item?.surface != null),
    motion: cases.some(item => item?.motion != null), materialStrain: cases.some(item => item?.material_strain != null),
    contactLoads: cases.some(item => item?.contact_loads != null),
    coverage: cases.some(item => item?.coverage != null), vaporSurface: cases.some(item => item?.vapor_surface != null),
    vaporCoverage: cases.some(item => item?.vapor_coverage != null),
    motionVaporHistory: cases.some(item => item?.motion?.vapor_coverage_history != null),
    motionWetFilm: cases.some(item => item?.motion?.vapor_coverage_history?.wet_film != null),
    transientSurface: cases.some(item => item?.motion?.transient_surface != null),
    textileWater: cases.some(item => hasTextileWaterPolicy(item?.motion?.transient_surface)),
    transientCoverage: cases.some(item => item?.motion?.transient_coverage != null),
    ...(matrix ? { matrix: { maxEvaluations: value.max_evaluations,
      cases: Array.isArray(value.cases) ? value.cases.map(item => ({ key: item?.key,
        surface: item?.scenario?.surface != null, motion: item?.scenario?.motion != null,
        materialStrain: item?.scenario?.material_strain != null,
        contactLoads: item?.scenario?.contact_loads != null,
        coverage: item?.scenario?.coverage != null, vaporSurface: item?.scenario?.vapor_surface != null,
        vaporCoverage: item?.scenario?.vapor_coverage != null,
        motionVaporHistory: item?.scenario?.motion?.vapor_coverage_history != null,
        motionWetFilm: item?.scenario?.motion?.vapor_coverage_history?.wet_film != null,
        transientSurface: item?.scenario?.motion?.transient_surface != null,
        textileWater: hasTextileWaterPolicy(item?.scenario?.motion?.transient_surface),
        transientCoverage: item?.scenario?.motion?.transient_coverage != null,
        ...(item?.comparison != null ? { comparison: item.comparison } : {}) })) : [] } } : {}) }, matrix };
}
function changedGuidedScenarioDraft() {
  if (guidedScenarioMode()) invalidate([guidedMatrixMode() ? "scenarioDraft" : "scenario"], true);
  else controls();
}
function guidedMatrixAllowance() {
  const raw = $("guidedScenarioMatrixBudget").value.trim(), value = Number(raw);
  if (!raw || !Number.isSafeInteger(value) || value < 1 || value > 64) throw Error("Declare a named-case evaluation allowance from 1 to 64.");
  return value;
}
async function scenarioInput() {
  if (guidedMatrixMode()) {
    const original = guidedScenarioDraftSource;
    const request = buildMaterialScenarioMatrixRequest({ maxEvaluations: guidedMatrixAllowance(), cases: guidedScenarioDrafts });
    return describeScenarioInput(request, original);
  }
  if (!guidedScenarioMode()) {
    const original = scenarioSourceFile();
    return describeScenarioInput(await read("scenarioRequest"), original);
  }
  return guidedScenarioInput();
}
function guidedMotionInput(keyframes = guidedMotionKeyframes, includeHeat = true) {
  const number = (id, label) => {
    const raw = $(id).value.trim();
    if (!raw || !Number.isFinite(Number(raw))) throw Error(`Supply a finite ${label}.`);
    return Number(raw);
  };
  const motion = {
    keyframes: keyframes.map(({ heat_boundary, ...frame }) => ({ ...frame, ...(frame.leg_pose ? { leg_pose: { ...frame.leg_pose } } : {}) })),
    max_step_s: number("guidedMotionStep", "maximum motion substep in seconds"),
    max_trials: number("guidedMotionTrials", "trajectory substep allowance"),
    max_pairs: number("guidedMotionPairs", "collision candidate allowance"),
    max_sample_vertices: number("guidedMotionVertices", "retained motion vertex allowance"),
  };
  if (includeHeat && $("guidedMotionHeat").checked) {
    motion.transient_surface = { schema: "ftx-motion-surface-heat/1",
      initial_temperature_c: number("guidedMotionHeatInitial", "initial textile temperature"),
      cells_per_layer: number("guidedMotionHeatCells", "through-sheet cell count"),
      boundaries: keyframes.map((frame, index) => {
        if (!frame.heat_boundary) throw Error(`Keyframe ${index + 1} has no saved heat conditions. Enter its four conditions and replace that endpoint.`);
        return { ...frame.heat_boundary };
      }),
      max_step_s: number("guidedMotionHeatStep", "maximum thermal substep in seconds"),
      max_cell_steps: number("guidedMotionHeatBudget", "temperature-cell advance allowance"),
    };
  }
  return { motion, maxMotionTrials: number("guidedScenarioMotionTrials", "case motion-attempt allowance") };
}
function showGuidedMotionKeyframes() {
  const parent = $("guidedMotionKeyframes"); parent.replaceChildren(); guidedMotionButtons.length = 0;
  for (const [index, frame] of guidedMotionKeyframes.entries()) {
    const row = document.createElement("div"), label = document.createElement("span"); row.className = "study-choice";
    label.textContent = `${index + 1}: ${frame.time_s} s; arms ${frame.arm_abduction_deg}°; ` +
      (frame.leg_pose ? `${frame.leg_pose.posture} at fraction ${frame.leg_pose.fraction}.` : "standing legs.");
    if (frame.heat_boundary) {
      const film = frame.heat_boundary, start = index ? guidedMotionKeyframes[index - 1].time_s : 0;
      label.textContent += ` Saved heat for ${start}–${frame.time_s} s: inside ${film.inside_c} °C / ${film.inside_resistance_m2_k_w} m² K/W; ` +
        `outside ${film.outside_c} °C / ${film.outside_resistance_m2_k_w} m² K/W.`;
    } else if ($("guidedMotionHeat").checked) label.textContent += " Heat conditions not saved; replace this endpoint to add them.";
    row.append(label);
    const replace = document.createElement("button"); replace.type = "button";
    replace.textContent = `Replace keyframe ${index + 1} from current controls`;
    replace.onclick = () => saveGuidedMotionKeyframe(frame);
    const remove = document.createElement("button"); remove.type = "button";
    remove.textContent = `Remove keyframe ${index + 1}`;
    remove.onclick = () => {
      const current = guidedMotionKeyframes.indexOf(frame);
      if (current < 0 || guidedMotionMode() !== "form") return;
      guidedMotionKeyframes.splice(current, 1); changedGuidedMotionDraft();
    };
    row.append(replace, remove); guidedMotionButtons.push(replace, remove); parent.append(row);
  }
  $("guidedMotionSummary").textContent = guidedMotionKeyframes.length ?
    `${guidedMotionKeyframes.length} of 16 keyframes saved, through ${guidedMotionKeyframes.at(-1).time_s} s. Only added endpoints run.` :
    "No motion keyframes saved. Declare an endpoint time and pose, then add it before running motion.";
}
function changedGuidedMotionDraft() {
  showGuidedMotionKeyframes();
  changedGuidedScenarioDraft();
}
function saveGuidedMotionKeyframe(replacing = null) {
  if (!source || !guidedScenarioMode() || guidedMotionMode() !== "form") return;
  try {
    const number = (id, label) => {
      const raw = $(id).value.trim();
      if (!raw || !Number.isFinite(Number(raw))) throw Error(`Supply a finite ${label}.`);
      return Number(raw);
    };
    const legs = $("guidedMotionLegs").value;
    if (!["standing", "seated", "squat"].includes(legs)) throw Error("Choose standing, seated or squat legs explicitly.");
    const frame = { time_s: number("guidedMotionTime", "endpoint time in seconds"),
      arm_abduction_deg: number("guidedMotionArm", "arm angle in degrees"),
      ...(legs === "standing" ? {} : { leg_pose: { posture: legs, fraction: number("guidedMotionLegFraction", "leg-pose fraction") } }) };
    if ($("guidedMotionHeat").checked) {
      frame.heat_boundary = { inside_c: number("guidedMotionHeatInside", "inside reservoir temperature"),
        outside_c: number("guidedMotionHeatOutside", "outside reservoir temperature"),
        inside_resistance_m2_k_w: number("guidedMotionHeatInsideFilm", "inside film resistance"),
        outside_resistance_m2_k_w: number("guidedMotionHeatOutsideFilm", "outside film resistance") };
    } else if (replacing?.heat_boundary) frame.heat_boundary = { ...replacing.heat_boundary };
    const keyframes = guidedMotionKeyframes.slice(), index = replacing ? keyframes.indexOf(replacing) : -1;
    if (replacing && index < 0) throw Error("Choose a current keyframe to replace.");
    if (replacing) keyframes[index] = frame; else keyframes.push(frame);
    buildMaterialMotionRequest(guidedMotionInput(keyframes, false).motion);
    // Validate the new boundary independently so older endpoints can be filled explicitly one at a time.
    if ($("guidedMotionHeat").checked) buildMaterialMotionRequest(guidedMotionInput([frame]).motion);
    guidedMotionKeyframes.splice(0, guidedMotionKeyframes.length, ...keyframes);
    changedGuidedMotionDraft();
  } catch (e) { error(String(e.message || e)); }
}
async function guidedScenarioInput(includeCasePolicy = false) {
  const number = (id, label) => {
    const raw = $(id).value.trim();
    if (!raw || !Number.isFinite(Number(raw))) throw Error(`Supply a finite ${label}.`);
    return Number(raw);
  };
  const fitSource = $("guidedScenarioFitSource").value;
  if (!["file", "none"].includes(fitSource)) throw Error("Choose the original fit contract or explicitly request a numerical study without one.");
  const designFit = guidedStudyMode() && $("guidedDesign").checked;
  const surface = $("guidedScenarioHeat").checked;
  const motionMode = guidedMotionMode(), materialStrain = $("guidedMaterialStrain").checked;
  const contactLoads = $("guidedContactLoads").checked;
  const transport = Object.fromEntries(guidedTransportSources.map(([key, id]) => [key, $(id).checked]));
  const vaporSource = guidedVaporSurfaceMode();
  if (transport.vaporSurface && !["file", "workbench"].includes(vaporSource)) throw Error("Choose original case vapor JSON or the selected section-3 vapor conditions.");
  if (!["none", "form", "file"].includes(motionMode)) throw Error("Choose no motion, authored endpoints or an original motion request.");
  const input = { allowNoFit: fitSource === "none", options: {
    spacing: number("guidedScenarioSpacing", "mesh spacing in millimetres"),
    sew_steps: number("guidedScenarioSewSteps", "sewing step count"),
    drape_steps: number("guidedScenarioDrapeSteps", "gravity step count"),
    step: number("guidedScenarioStep", "time step in seconds"),
    max_settle_steps: number("guidedScenarioSettle", "maximum additional settling steps"),
  }, maxMeshPoints: number("guidedScenarioMeshBudget", "mesh budget"),
    maxVertexFrames: number("guidedScenarioFrameBudget", "total vertex-frame budget") };
  if ($("guidedScenarioSelfContact").checked) {
    input.options.self_contact = { dhat: 0.001, thickness: 0.002, kappa: 100, mu: 0.4, eps_v: 0.001 };
  }
  if (motionMode === "form") Object.assign(input, guidedMotionInput());
  if (motionMode === "file") input.maxMotionTrials = number("guidedScenarioMotionTrials", "case motion-attempt allowance");
  if (materialStrain) {
    const scope = $("guidedMaterialStrainScope").value || "all";
    if (!["all", "instances"].includes(scope)) throw Error("Choose all retained cloth or exact instance names for material strain.");
    input.materialStrain = JSON.stringify({ schema: "ftx-material-strain/1",
      ...(scope === "instances" ? { instances: $("guidedMaterialStrainInstances").value.split(/\r?\n/).filter(line => line.length) } : {}),
      max_triangle_samples: number("guidedMaterialStrainBudget", "directional strain triangle-sample allowance") });
  }
  if (contactLoads) {
    const scope = $("guidedContactLoadsScope").value || "all";
    if (!["all", "instances"].includes(scope)) throw Error("Choose all retained cloth or exact instance names for body contact loads.");
    input.contactLoads = JSON.stringify({ schema: "ftx-contact-loads/1",
      ...(scope === "instances" ? { instances: $("guidedContactLoadsInstances").value.split(/\r?\n/).filter(line => line.length) } : {}),
      max_vertex_samples: number("guidedContactVertexBudget", "body contact vertex-sample allowance"),
      max_pair_evaluations: number("guidedContactPairBudget", "body contact candidate-pair allowance") });
  }
  // Both original files and every control are captured before starting a read.
  const policy = $("guidedScenarioPolicySource").value;
  if (includeCasePolicy && !["inherit", "snapshot", "file"].includes(policy)) throw Error("Choose the shared comparison, current guided goals or an original per-case policy.");
  let authoredComparison;
  if (includeCasePolicy && policy === "snapshot") {
    if (!guidedStudyMode()) throw Error("Select guided material comparisons before saving their current goals as a case policy.");
    const { requiredConsumers, ...comparison } = guidedComparisonInput(number);
    authoredComparison = JSON.stringify({ ...comparison, required_consumers: requiredConsumers, allow_mixed_protocols: false });
  }
  const entries = [...(fitSource === "file" ? [["fitSpec", "fit", "the original section-3 fit contract"]] : []),
    ...(surface ? [["surface", "heatConditions", "the selected section-3 sheet heat conditions"]] : []),
    ...(motionMode === "file" ? [["motion", "guidedMotionFile", "the original prescribed-motion request"]] : []),
    ...guidedTransportSources.filter(([key]) => transport[key]).map(([key, , file, label]) =>
      [key, key === "vaporSurface" && vaporSource === "workbench" ? "vaporSurfaceConditions" : file, label]),
    ...(includeCasePolicy && policy === "file" ? [["comparison", "guidedScenarioComparison", "the original case comparison policy"]] : [])];
  const budget = 16 * 1024 * 1024; let total = 0;
  const retained = entries.map(([key, id, label]) => {
    const original = Object.hasOwn(sheetConditionForms, id) ? sheetConditionsSource(id) : roundFiles.has(id) ? roundFiles.get(id) : $(id).files[0];
    if (!original) throw Error(`Choose ${label} first.`);
    const size = typeof original === "string" ? new Blob([original]).size : original.size;
    if (key === "fitSpec" && designFit && size > 256 * 1024) throw Error("Guided alteration fit contract exceeds 256 KiB; no source was read.");
    total += size;
    if (total > budget) throw Error("Guided scenario source files exceed 16 MiB combined; no source was read.");
    return { key, original };
  });
  let decodedBytes = 0;
  const sources = await Promise.all(retained.map(async ({ key, original }) => {
    const raw = typeof original === "string" ? original : new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(await original.arrayBuffer());
    decodedBytes += new Blob([raw]).size;
    if (decodedBytes > budget) throw Error("Guided scenario sources exceed 16 MiB combined.");
    return [key, raw];
  }));
  const { comparison, ...fragments } = Object.fromEntries(sources);
  const request = buildMaterialScenarioRequest({ ...input, ...fragments });
  const retainedMotion = motionMode === "file" ? JSON.parse(fragments.motion) : input.motion;
  const motionVaporHistory = retainedMotion?.vapor_coverage_history != null;
  const motionWetFilm = retainedMotion?.vapor_coverage_history?.wet_film != null;
  const transientSurface = retainedMotion?.transient_surface != null;
  const textileWater = hasTextileWaterPolicy(retainedMotion?.transient_surface);
  const transientCoverage = retainedMotion?.transient_coverage != null;
  return { request,
    context: { drape: true, surface, motion: motionMode !== "none", materialStrain, contactLoads, ...transport,
      motionVaporHistory, motionWetFilm, transientSurface, textileWater, transientCoverage },
    ...((comparison ?? authoredComparison) === undefined ? {} : { comparison: comparison ?? authoredComparison }) };
}
function showGuidedScenarioDrafts() {
  const parent = $("guidedScenarioCases"); parent.replaceChildren(); guidedScenarioButtons.length = 0;
  let frames = 0, motionTrials = 0;
  for (const [index, item] of guidedScenarioDrafts.entries()) {
    const value = JSON.parse(item.scenario); frames += value.max_vertex_frames;
    if (value.motion) motionTrials += value.max_motion_trials;
    const row = document.createElement("div"), label = document.createElement("span"); row.className = "study-choice";
    label.textContent = `${item.key}: ${value.options.sew_steps} sewing + ${value.options.drape_steps} gravity steps, ` +
      `${value.max_vertex_frames} vertex-frames; ${value.surface ? "retained sheet heat" : "no sheet heat"}; ` +
      (value.motion ? `${value.motion.keyframes.length} motion endpoints, ${value.max_motion_trials} motion attempts; ` : "static pose; ") +
      `${value.material_strain ? `directional strain on ${value.material_strain.instances?.join(", ") || "all retained instances"}, ${value.material_strain.max_triangle_samples} triangle-samples` : "no directional strain request"}; ` +
      `${value.contact_loads ? `body contact loads on ${value.contact_loads.instances?.join(", ") || "all retained instances"}, ${value.contact_loads.max_vertex_samples} vertex-samples and ${value.contact_loads.max_pair_evaluations} pair evaluations` : "no body contact-load request"}; ` +
      `${value.coverage ? "retained body/layer heat" : "no body/layer heat"}; ` +
      `${value.vapor_surface ? "retained independent-sheet vapor" : "no sheet vapor"}; ` +
      `${value.vapor_coverage ? "retained covered-body vapor" : "no covered-body vapor"}; ` +
      `${value.motion?.vapor_coverage_history ? "requested complete-motion vapor history" : "no motion vapor history"}; ` +
      `${value.motion?.vapor_coverage_history?.wet_film != null ? "retained finite body-water policy" : "no body-water inventory"}; ` +
      `${value.motion?.transient_surface ? "retained independent-sheet heat history" : "no sheet heat history"}; ` +
      `${hasTextileWaterPolicy(value.motion?.transient_surface) ? "retained textile-water heat policy" : "no textile-water policy"}; ` +
      `${item.comparison && JSON.parse(item.comparison) !== null ? "retained case comparison" : "shared comparison at run time"}.`;
    row.append(label);
    const action = (title, handler, unavailable = false, replace = false) => {
      const button = document.createElement("button"); button.type = "button"; button.textContent = title; button.onclick = handler;
      guidedScenarioButtons.push({ button, unavailable, replace }); row.append(button);
    };
    action(`Replace ${item.key} from current controls`, () => freezeGuidedScenarioCase(item), false, true);
    action(`Move ${item.key} earlier`, () => {
      const i = guidedScenarioDrafts.indexOf(item);
      if (i <= 0) return;
      [guidedScenarioDrafts[i - 1], guidedScenarioDrafts[i]] = [guidedScenarioDrafts[i], guidedScenarioDrafts[i - 1]];
      changedGuidedScenarioDrafts();
    }, index === 0);
    action(`Move ${item.key} later`, () => {
      const i = guidedScenarioDrafts.indexOf(item);
      if (i < 0 || i + 1 >= guidedScenarioDrafts.length) return;
      [guidedScenarioDrafts[i], guidedScenarioDrafts[i + 1]] = [guidedScenarioDrafts[i + 1], guidedScenarioDrafts[i]];
      changedGuidedScenarioDrafts();
    }, index + 1 === guidedScenarioDrafts.length);
    action(`Remove case ${item.key}`, () => {
      const i = guidedScenarioDrafts.indexOf(item);
      if (i >= 0) { guidedScenarioDrafts.splice(i, 1); changedGuidedScenarioDrafts(); }
    });
    parent.append(row);
  }
  $("guidedScenarioCaseCount").textContent = guidedScenarioDrafts.length ?
    `${guidedScenarioDrafts.length} of 8 named cases saved; ${frames} of 64000000 vertex-frames and ${motionTrials} of 32768 motion attempts allocated. Only saved cases run in named-case mode.` :
    "No named cases saved. Configure a scenario, give it a name and save it before comparing.";
}
function changedGuidedScenarioDrafts() {
  guidedScenarioDraftSource = {};
  showGuidedScenarioDrafts(); refreshGuidedDesignCases();
  invalidate(["scenario", "scenarioDraft"], true);
}
async function freezeGuidedScenarioCase(replacing = null) {
  if (!ready || pending || !source || !guidedMatrixMode()) return;
  const id = ++sequence; pending = { id, op: "scenarioDraft" }; $("error").hidden = true;
  $("status").textContent = "Saving the exact named-case inputs…"; controls();
  try {
    const key = replacing ? replacing.key : $("guidedScenarioCaseName").value;
    if (!key.trim() || new Blob([key]).size > 128) throw Error("Give the case a nonempty name of at most 128 UTF-8 bytes.");
    const index = replacing ? guidedScenarioDrafts.indexOf(replacing) : -1;
    if ((replacing && index < 0) || (!replacing && (guidedScenarioDrafts.length >= 8 || guidedScenarioDrafts.some(item => item.key === key)))) {
      throw Error("Save at most eight uniquely named cases; use Replace to update an existing case.");
    }
    const maxEvaluations = guidedMatrixAllowance(), retained = guidedScenarioDrafts.slice();
    const snapshot = await guidedScenarioInput(true);
    if (pending?.id !== id) return;
    const item = { key, scenario: snapshot.request, ...(snapshot.comparison === undefined ? {} : { comparison: snapshot.comparison }) };
    if (replacing) retained[index] = item; else retained.push(item);
    buildMaterialScenarioMatrixRequest({ maxEvaluations, cases: retained });
    pending = null; guidedScenarioDrafts.splice(0, guidedScenarioDrafts.length, ...retained);
    if (!replacing) $("guidedScenarioCaseName").value = "";
    changedGuidedScenarioDrafts(); $("status").textContent = `Saved ${key} with its original inputs.`;
  } catch (e) {
    if (pending?.id !== id) return;
    pending = null; error(String(e.message || e)); $("status").textContent = "Case not saved; existing cases are unchanged."; controls();
  }
}
function showCombinations() {
  const parent = $("combinationGroups"); parent.replaceChildren();
  let count = combinationGroups.size ? 1 : 0;
  for (const [key, group] of [...combinationGroups].sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0)) {
    count *= group.choices.size;
    const heading = document.createElement("p");
    heading.textContent = `${key} — ${group.target.scope === "default" ? "Pattern default" : group.target.ids.join(", ")}`;
    parent.append(heading);
    for (const [choice, selection] of group.choices) {
      const row = document.createElement("div"), label = document.createElement("span"), remove = document.createElement("button");
      label.textContent = `${choice}: ${JSON.parse(selection).id} `; remove.textContent = `Remove ${choice}`;
      remove.onclick = () => {
        group.choices.delete(choice);
        if (!group.choices.size) combinationGroups.delete(key);
        showCombinations(); invalidate(["study", "scenario"]);
      };
      row.append(label, remove); parent.append(row);
    }
  }
  $("combinationCount").textContent = count ? `${count} complete material combination${count === 1 ? "" : "s"}; limit 32.` : "No material choices added.";
}
async function studyInput(scenarioRead) {
  if (guidedStudyMode()) return guidedStudyInput(scenarioRead);
  const raw = await read("studyRequest");
  if (!$("combinationMode").checked) return raw;
  const policy = JSON.parse(raw);
  if (!policy || typeof policy !== "object" || Array.isArray(policy)
    || !["ftx-material-study/1", "ftx-design-study/1"].includes(policy.schema)) {
    throw Error("Combination mode needs an ftx-material-study/1 or ftx-design-study/1 policy object.");
  }
  if (["variants", "screened", "combinations"].some(key => Object.hasOwn(policy, key))) {
    throw Error("In combination mode, the study policy must omit variants, screened and combinations. Turn off combination mode to use a complete study file.");
  }
  if (!combinationGroups.size) throw Error("Add material choices before running combinations.");
  const groups = [...combinationGroups].map(([key, group]) => {
    const choices = [...group.choices].map(([choice, selection]) => `{"key":${JSON.stringify(choice)},"selection":${selection}}`);
    return `{"key":${JSON.stringify(key)},"target":${JSON.stringify(group.target)},"choices":[${choices.join(",")}]}`;
  });
  let interfaces = "";
  if (has("combinationInterfaces")) {
    const pairs = await read("combinationInterfaces");
    if (!Array.isArray(JSON.parse(pairs))) throw Error("The combination interface pool must be a JSON list.");
    interfaces = `,"interfaces":${pairs}`;
  }
  // Insert authored fields without reserializing source-bound quantities, policies or selections.
  return `${raw.trim().slice(0, -1)},"combinations":{"groups":[${groups.join(",")}]${interfaces}}}`;
}
function resetInterfaces() {
  ++interfaceRead; interfaceChange = null; interfaceReady = true;
  $("interfacesFile").value = "";
  $("interfacesSummary").textContent = "Keep the original pattern's interface evidence.";
}
async function execute(op) {
  if (!ready || pending) return;
  if (op === "bind") clear(["bind", ...boundOperations]);
  else if (op === "fold") clear(["fold", "heat", "vaporSurface", "coverage"]);
  else if (sections.has(op)) clear([op]);
  if (selectionOperations.includes(op) && !(op === "calibrate" && $("calibrationMode").value === "friction_pair")) {
    if (op !== "linearizeOrigin") resetOriginSource();
    else if (originSource) { $("selection").value = originSource.selectionJson; clear(["thermalPoint"]); }
    ++selectionRead; preparedBy = null; inspected = null; $("detail").textContent = ""; $("capabilities").replaceChildren();
  }
  const id = ++sequence; pending = { id, op }; $("error").hidden = true;
  $("status").textContent = `Running ${op}…`; controls();
  try {
    const msg = { type: op, request: id };
    if (op === "catalog") msg.query = $("query").value;
    else if (op === "inspect") msg.selection = $("selection").value;
    else if (op === "linearizeOrigin") msg.originRequest = originRequest();
    else if (op === "thermalPoint") {
      if (!$("thermalPointTemperature").value.trim() || !Number.isFinite(Number($("thermalPointTemperature").value))) throw Error("Supply a finite material temperature in degrees Celsius.");
      msg.selection = $("selection").value; msg.temperatureC = Number($("thermalPointTemperature").value);
    }
    else if (op === "thermalHistory") msg.thermalRequest = await read("thermalRequest");
    else if (op === "bodyCheck") {
      if (!has("measurements") && (photoEvidence || photoTransfer || photoProblem || $("supplementalFiles").files.length)) throw Error("Choose primary measurements before adding evidence.");
      msg.measurements = has("measurements") ? await measurementInput(msg) : "{}";
      if (has("bodyFitFile")) {
        if (!$("bodyFitConsent").checked) throw Error("Associate the accepted fit with this wearer before checking.");
        msg.bodyFit = await read("bodyFitFile"); msg.bodyFitConsent = true;
      }
      if (!has("measurements") && !has("bodyFitFile")) throw Error("Choose measurements or an associated accepted fit before checking.");
      msg.bytes = await read("body", true);
    }
    else if (op === "garmentcode") {
      msg.source = await readGarmentcode("garmentcodeSource");
      if (has("garmentcodeAnnotations")) msg.annotations = await readGarmentcode("garmentcodeAnnotations");
      else msg.annotations = circularArcAnnotations();
    }
    else if (op === "bind") {
      if (!interfaceReady) throw Error("Load a valid replacement interface list or choose Keep original interfaces.");
      msg.pattern = sourceText; msg.changes = changes();
    }
    else if (op === "stageInterface") {
      if (!sourceText || !interfaceReady || !calibratedPair) throw Error("Load the garment, retain valid interface edits and identify a current contact pair first.");
      msg.pattern = sourceText; msg.changes = changes();
      msg.interfaceJson = calibratedPair; msg.replaceExisting = $("replaceCalibratedInterface").checked;
    }
    else if (op === "pressure") {
      msg.compressionLayer = $("compressionLayer").value;
      msg.compressionRequest = await read("compressionRequest");
    }
    else if (op === "treatment") {
      msg.pattern = bound?.pattern_json || sourceText;
      msg.treatmentRequest = await read("treatmentRequest");
    }
    else if (op === "vapor") msg.vaporRequest = await read("vaporRequest");
    else if (op === "calibrate") {
      msg.calibrationMode = $("calibrationMode").value;
      msg.allowEstimates = $("calibrationEstimates").checked;
      msg.calibrationRequest = await read("calibrationRequest");
    }
    else if (["stripAcquisition", "stripExperiment", "stripHistory", "stripFit"].includes(op)) msg.stripRequest = await read(`${op}Request`);
    else if (op === "screen") msg.screenRequest = guidedScreenMode() ?
      buildMaterialScreenRequest(guidedScreenInput()) : await read("screenRequest");
    else if (op === "study" || op === "scenario") {
      if (!sourceText) throw Error("Load the original pattern first.");
      // Alternatives always start at the original, never the currently bound single-material preview.
      msg.pattern = sourceText;
      const onBody = op === "study" && studyUsesBody();
      const scenarioRead = op === "scenario" ? scenarioInput() : undefined;
      const [study, scenario] = await Promise.all([studyInput(scenarioRead), scenarioRead]);
      if (pending?.id !== id) return;
      msg.studyRequest = study;
      if (op === "study") msg.onBody = onBody;
      if (op === "scenario" || msg.onBody) {
        if (op === "scenario") msg.scenarioRequest = scenario.request;
        msg.measurements = await studyBodyInput(msg);
        msg.bytes = await read("body", true);
      }
    }
    else {
      if (!bound) throw Error("Bind the current materials first.");
      msg.pattern = bound.pattern_json;
      if (op === "fold" || op === "exports") { msg.measurements = await measurementInput(msg); msg.fit = await read("fit"); }
      if (op === "fold") {
        if ($("drapeLegFraction").value.trim() === "" || $("maxSettleSteps").value.trim() === "") {
          throw Error("Supply a leg fraction and maximum extra settling steps.");
        }
        msg.legs = $("drapeLegs").value;
        msg.legFraction = Number($("drapeLegFraction").value);
        msg.maxSettleSteps = Number($("maxSettleSteps").value);
        msg.twoStart = $("twoStart").checked;
        msg.armDeg = drapeArmAngle();
        if (has("bodyFitFile")) {
          if (!$("bodyFitConsent").checked) throw Error("Associate the accepted fit with this wearer before draping.");
          msg.bodyFit = await read("bodyFitFile"); msg.bodyFitConsent = true;
        }
        msg.bytes = await read("body", true); msg.selfContactMm = $("selfContact").checked ? 2 : 0;
      }
      if (op === "exports") { const d = new Date(), pad = n => String(n).padStart(2, "0");
        msg.today = `${d.getFullYear()}-${pad(d.getMonth()+1)}-${pad(d.getDate())}`;
        msg.created = `${msg.today}T${pad(d.getHours())}:${pad(d.getMinutes())}`; }
      if (op === "heat" || op === "vaporSurface" || op === "coverage") {
        if (!drape) throw Error("Re-drape the current material first."); msg.report = drape;
        const conditionId = op === "heat" ? "heatConditions" : op === "vaporSurface" ? "vaporSurfaceConditions" : "coverageConditions";
        const authored = op === "coverage" ? null : sheetConditionsSource(conditionId);
        msg.conditions = typeof authored === "string" ? authored : await read(conditionId);
        if (op === "coverage") {
          msg.map = await read("map");
          if (has("coverageBridges")) msg.thermalBridges = await read("coverageBridges");
        }
      }
      if (op === "cut" || op === "price") msg.options = await read("cutOptions");
      if (op === "cut") {
        const marker = Number($("rollPreview").value || "1");
        if (!Number.isSafeInteger(marker) || marker < 1) throw Error("Preview marker number must be a positive integer.");
        msg.rollIndex = marker - 1;
      }
      if (["stock", "stockIdentities", "stockPrice"].includes(op)) msg.stockRequest = await read("stockRequest");
      if (op === "stockPrice") msg.stockPricing = await read("stockPricing");
      if (op === "cutJob") msg.cutJobRequest = await read("cutJobRequest");
      if (op === "price") { msg.quotes = await read("quotes"); msg.market = await read("market"); }
      if (op === "sew") { msg.options = await read("sewOptions"); msg.selectRecipes = $("sewOptimize").checked; }
      if (op === "sewPreview") {
        const cell = currentSewingPreviewCell(), operation = currentSewingPreviewOperation();
        if (!sewingPreviewSource || !cell || !operation) throw Error("Evaluate sewing, then choose a retained cell and operation before previewing.");
        msg.pattern = sewingPreviewSource.pattern; msg.options = sewingPreviewSource.options;
        msg.operation = operation.pass.id ?? operation.pass.seam;
        if (cell.cell !== null) msg.cell = cell.cell;
      }
    }
    if (outfit && (op === "bind" || boundOperations.includes(op) || op === "study" || op === "scenario")) msg.outfit = outfit;
    if (pending?.id !== id) return;
    if (op === "sew") pending.sewingInputs = { pattern: msg.pattern, options: msg.options };
    worker.postMessage(msg, msg.bytes ? [msg.bytes] : []);
  } catch (e) {
    if (pending?.id !== id) return;
    pending = null; error(String(e.message || e)); $("status").textContent = `${op} refused.`; controls();
  }
}
for (const el of commands) el.onclick = () => execute(el.dataset.command);
$("addScreenAttribute").onclick = addGuidedScreenAttribute;
$("clearScreenAttributes").onclick = () => {
  guidedScreenAttributes.length = 0; showGuidedScreenAttributes();
  if (guidedScreenMode()) invalidate(["screen"], true); else controls();
};
for (const id of ["screenMode", "guidedScreenQuery", "guidedScreenForm", "guidedScreenBasis", "guidedScreenEstimates", "guidedScreenMixed",
  "guidedScreenMechanics", "guidedScreenSteady", "guidedScreenThermal"]) {
  const changed = () => { if (id === "screenMode" || guidedScreenMode()) invalidate(["screen"], true); else controls(); };
  $(id).addEventListener("change", changed);
  if (id === "guidedScreenQuery") $(id).addEventListener("input", changed);
}
$("guidedScreenProperty").addEventListener("change", controls);
$("addStudyCandidate").onclick = () => {
  if (!source || !inspected || pending) return;
  try {
    if (studyCandidates.length >= 7) throw Error("Keep at most seven material choices plus the original baseline.");
    let key = $("studyCandidateName").value.trim();
    if (!key) {
      do { key = `material-${++studyCandidateSequence}`; } while (studyCandidates.some(candidate => candidate.key === key));
    }
    if (key === "baseline" || studyCandidates.some(candidate => candidate.key === key)) {
      throw Error("Choose a unique comparison name other than baseline.");
    }
    if (studyCandidates.some(candidate => candidate.selection === inspected)) {
      throw Error("This exact inspected selection is already in the shortlist.");
    }
    studyCandidates.push({ key, selection: inspected });
    $("studyCandidateName").value = ""; $("studyMode").value = "guided";
    showStudyCandidates(); invalidate(["study", "scenario"], true);
  } catch (e) { error(String(e.message || e)); }
};
$("clearStudyCandidates").onclick = () => {
  studyCandidates.length = 0; showStudyCandidates();
  if (guidedStudyMode()) invalidate(["study", "scenario"], true); else controls();
};
$("studyMode").addEventListener("change", () => invalidate(["study", "scenario",
  ...($("guidedScenarioPolicySource").value === "snapshot" ? ["scenarioDraft"] : [])], true));
$("guidedDesignAfterAdd").onclick = () => saveGuidedDesignAfter();
$("guidedDesignAfterClear").onclick = () => { guidedDesignAfter.length = 0; changedGuidedDesignAfter(); };
for (const id of ["guidedDesignAfterKind", "guidedDesignAfterSeam", "guidedDesignAfterEnd", "guidedDesignAfterPiece",
  "guidedDesignAfterSeams", "guidedDesignAfterRatio"]) {
  $(id).addEventListener("change", controls);
  if (id === "guidedDesignAfterRatio") $(id).addEventListener("input", controls);
}
for (const id of ["guidedMaterialStrain", "guidedMaterialStrainScope", "guidedMaterialStrainInstances", "guidedMaterialStrainBudget"]) {
  $(id).addEventListener("change", changedGuidedScenarioDraft);
  if (!["guidedMaterialStrain", "guidedMaterialStrainScope"].includes(id)) $(id).addEventListener("input", changedGuidedScenarioDraft);
}
for (const id of ["guidedContactLoads", "guidedContactLoadsScope", "guidedContactLoadsInstances", "guidedContactVertexBudget", "guidedContactPairBudget"]) {
  const changed = () => {
    if (id === "guidedContactLoads" || $("guidedContactLoads").checked) changedGuidedScenarioDraft();
    else controls();
  };
  $(id).addEventListener("change", changed);
  if (!["guidedContactLoads", "guidedContactLoadsScope"].includes(id)) $(id).addEventListener("input", changed);
}
for (const [, id, file] of guidedTransportSources) {
  $(id).addEventListener("change", changedGuidedScenarioDraft);
  $(file).addEventListener("change", () => {
    if ($(id).checked && (id !== "guidedVaporSurface" || guidedVaporSurfaceMode() === "file")) changedGuidedScenarioDraft();
    else controls();
  });
}
$("guidedVaporSurfaceSource").addEventListener("change", () => {
  if ($("guidedVaporSurface").checked) changedGuidedScenarioDraft(); else controls();
});
for (const [id, form] of Object.entries(sheetConditionForms)) {
  $(id + "Source").addEventListener("change", () => invalidate([form.operation, ...guidedSourceStudies(id)], true));
  for (const [field] of form.fields) {
    const changed = () => {
      if (sheetConditionsMode(id) === "form") invalidate([form.operation, ...guidedSourceStudies(id)], true);
      else controls();
    };
    $(field).addEventListener("change", changed); $(field).addEventListener("input", changed);
  }
}
$("guidedMotionSource").addEventListener("change", () => {
  if ($("guidedScenarioMotion").checked) changedGuidedScenarioDraft(); else controls();
});
$("guidedMotionFile").addEventListener("change", () => {
  if (guidedMotionMode() === "file") changedGuidedScenarioDraft(); else controls();
});
$("addMotionKeyframe").onclick = () => saveGuidedMotionKeyframe();
$("clearMotionKeyframes").onclick = () => {
  if (guidedMotionMode() !== "form") return;
  guidedMotionKeyframes.length = 0; changedGuidedMotionDraft();
};
$("guidedScenarioMotion").addEventListener("change", changedGuidedMotionDraft);
for (const id of ["guidedMotionHeat", "guidedMotionHeatInitial", "guidedMotionHeatCells", "guidedMotionHeatStep", "guidedMotionHeatBudget"]) {
  const changed = () => {
    if (guidedMotionMode() === "form" && (id === "guidedMotionHeat" || $("guidedMotionHeat").checked)) changedGuidedMotionDraft();
    else controls();
  };
  $(id).addEventListener("change", changed);
  if (id !== "guidedMotionHeat") $(id).addEventListener("input", changed);
}
for (const id of ["guidedMotionStep", "guidedMotionTrials", "guidedMotionPairs", "guidedMotionVertices", "guidedScenarioMotionTrials"]) {
  const changed = () => {
    if (guidedMotionMode() === "form" || (guidedMotionMode() === "file" && id === "guidedScenarioMotionTrials")) changedGuidedMotionDraft();
    else controls();
  };
  $(id).addEventListener("change", changed); $(id).addEventListener("input", changed);
}
for (const id of ["guidedMotionTime", "guidedMotionArm", "guidedMotionLegs", "guidedMotionLegFraction",
  "guidedMotionHeatInside", "guidedMotionHeatOutside", "guidedMotionHeatInsideFilm", "guidedMotionHeatOutsideFilm"]) {
  $(id).addEventListener("change", controls);
  if (id !== "guidedMotionLegs") $(id).addEventListener("input", controls);
}
for (const id of ["scenarioMode", "guidedScenarioFitSource", "guidedScenarioHeat", "guidedScenarioSelfContact",
  "guidedScenarioSpacing", "guidedScenarioSewSteps", "guidedScenarioDrapeSteps", "guidedScenarioStep",
  "guidedScenarioSettle", "guidedScenarioMeshBudget", "guidedScenarioFrameBudget"]) {
  const changed = () => {
    if (id === "scenarioMode") refreshGuidedDesignCases();
    invalidate(id === "scenarioMode" ? ["scenario", "scenarioDraft"] : [guidedMatrixMode() ? "scenarioDraft" : "scenario"], true);
  };
  $(id).addEventListener("change", changed);
  if (!["scenarioMode", "guidedScenarioFitSource", "guidedScenarioHeat", "guidedScenarioSelfContact"].includes(id)) $(id).addEventListener("input", changed);
}
$("addScenarioCase").onclick = () => freezeGuidedScenarioCase();
$("clearScenarioCases").onclick = () => { guidedScenarioDrafts.length = 0; changedGuidedScenarioDrafts(); };
for (const id of ["guidedScenarioCaseName", "guidedScenarioPolicySource", "guidedScenarioComparison", "guidedScenarioMatrixBudget"]) {
  const changed = () => invalidate(id === "guidedScenarioMatrixBudget" ? ["scenario", "scenarioDraft"] : ["scenarioDraft"], true);
  $(id).addEventListener("change", changed);
  if (["guidedScenarioCaseName", "guidedScenarioMatrixBudget"].includes(id)) $(id).addEventListener("input", changed);
}
$("clearGuidedInventory").onclick = () => {
  $("guidedInventory").value = ""; roundFiles.delete("guidedInventory");
  if (guidedStudyMode()) invalidate(["study", "scenario"], true); else controls();
};
$("clearStockRequest").onclick = () => selectStockRequest(null);
for (const id of ["studyTarget", "studyPieces", "guidedStockMode", "guidedCutting", "guidedCuttingSource", "guidedMass", "guidedInventoryComplete", "guidedInventory",
  "guidedDesign", "guidedDesignPieces", "guidedDesignVertices", ...Object.values(guidedDesignFields), "guidedDesignMax", "guidedDesignGridBudget",
  "guidedDesignMode", "guidedDesignObjective", "guidedDesignMaterial", "guidedDesignCase", "guidedDesignEvaluations",
  "guidedSewing", "guidedPurchasing", "guidedCombinedCost", "guidedBasis", "guidedLimitBasis", "guidedHeatDirection", "guidedHeatMinimumLimit",
  "guidedCopies", "guidedGap", "guidedEdge", "guidedEnd", "guidedAllowance",
  ...[...guidedMetrics, ...guidedDirectionalMetrics].flatMap(([prefix]) => [`${prefix}Goal`, `${prefix}Limit`]),
  ...guidedTransportMetrics.flatMap(([prefix]) => [`${prefix}Goal`, `${prefix}Direction`, `${prefix}MinimumLimit`, `${prefix}Limit`]),
  ...guidedRequired.map(([id]) => id)]) {
  const changed = () => {
    if (id === "guidedDesignMaterial") {
      const key = $(id).value, original = guidedDesignMaterialSource(key);
      guidedDesignNomination = key && original !== null ? { key, source: original, invalidated: false } : null;
      refreshGuidedDesignMaterials();
    }
    if (id === "guidedDesignCase") {
      const key = $(id).value, cases = currentGuidedScenarioCases();
      guidedDesignCaseNomination = key && cases?.original === scenarioSourceIdentity() && cases.keys.includes(key) ?
        { key, source: cases.original, invalidated: false } : null;
      refreshGuidedDesignCases();
    }
    if (Object.values(guidedDesignFields).includes(id) || ["guidedDesignPieces", "guidedDesignVertices"].includes(id)) {
      if (guidedDesignVariables.length) guidedDesignVariables[guidedDesignCurrent] = captureGuidedDesignVariable();
      showGuidedDesignVariables();
    }
    if (id === "guidedCutting" && !$(id).checked) {
      for (const [prefix, , scope] of guidedMetrics) if (scope === guidedStockMode() && prefix !== "guidedOrderCost") $(`${prefix}Goal`).checked = false;
    }
    if (id === "guidedCuttingSource" && $(id).value !== "file") $("guidedCutTimeGoal").checked = false;
    if (id === "guidedMass" && !$(id).checked) $("guidedMassGoal").checked = false;
    if (id === "guidedSewing" && !$(id).checked) {
      for (const goal of ["guidedSewTimeGoal", "guidedMakespanGoal", "guidedThreadLengthGoal", "guidedThreadCostGoal"]) $(goal).checked = false;
    }
    if (id === "guidedPurchasing" && !$(id).checked) $("guidedOrderCostGoal").checked = false;
    if (id === "guidedCombinedCost" && !$(id).checked) $("guidedCombinedCostGoal").checked = false;
    if (guidedStudyMode()) invalidate(["study", "scenario",
      ...($("guidedScenarioPolicySource").value === "snapshot" ? ["scenarioDraft"] : [])], true); else controls();
  };
  $(id).addEventListener("change", changed);
  if (id.endsWith("Limit") || ["guidedCopies", "guidedGap", "guidedEdge", "guidedEnd", "guidedAllowance",
    "guidedDesignKey", "guidedDesignAt", "guidedDesignMultiplier", "guidedDesignValues", "guidedDesignDx", "guidedDesignDy",
    "guidedDesignMax", "guidedDesignGridBudget", "guidedDesignLower", "guidedDesignUpper", "guidedDesignInitial",
    "guidedDesignInitialStep", "guidedDesignMinimumStep", "guidedDesignEvaluations"].includes(id)) $(id).addEventListener("input", changed);
}
$("rollPreview").addEventListener("change", () => invalidate(["cut"]));
$("sewPreviewCell").addEventListener("change", () => {
  invalidate(["sewPreview"], true); refreshSewingPreviewOperations(); controls();
});
$("sewPreviewOperation").addEventListener("change", () => {
  invalidate(["sewPreview"], true); describeSewingPreviewSelection(); controls();
});
$("cancel").onclick = () => { cancelPhotoReturn(); if (patternTransfer) patternTransfer.pattern = null;
  clear(["sewPreview"]); start(); $("status").textContent = "Cancelled; reloading local engine."; };
$("catalog").onchange = () => {
  if (!$("catalog").value) return;
  invalidateThermalPoint();
  resetOriginSource();
  ++selectionRead;
  if (selectionOperations.includes(pending?.op)) start();
  $("selection").value = text({ id: $("catalog").value, allow_estimates: false });
  preparedBy = null; inspected = null; $("detail").textContent = ""; $("capabilities").replaceChildren(); controls();
};
$("selection").addEventListener("input", () => {
  invalidateThermalPoint();
  resetOriginSource();
  ++selectionRead;
  preparedBy = null; inspected = null; $("detail").textContent = ""; $("capabilities").replaceChildren();
  if (selectionOperations.includes(pending?.op)) start(); controls();
});
$("selectionFile").onchange = async () => {
  invalidateThermalPoint();
  resetOriginSource();
  const rev = ++selectionRead; preparedBy = null; inspected = null; $("detail").textContent = ""; $("capabilities").replaceChildren();
  if (selectionOperations.includes(pending?.op)) start(); controls();
  try { const data = await read("selectionFile"); if (rev !== selectionRead) return;
    invalidateThermalPoint(); $("selection").value = data; controls(); }
  catch (e) { if (rev === selectionRead) error(String(e.message || e)); }
};
function parsePattern(raw) {
  const data = JSON.parse(raw);
  if (!Array.isArray(data?.pieces) || data.pieces.length > 4096) throw Error("Pattern needs at most 4096 source pieces.");
  return data;
}
function guidedDesignSeamIds() {
  return (Array.isArray(source?.seams) ? source.seams : []).map(seam => seam?.id).filter(id => typeof id === "string");
}
function ensureGuidedDesignAfterSources() {
  if (!source || guidedDesignAfterSource === source) return;
  guidedDesignAfterSource = source; guidedDesignAfterProblem = "";
  for (const id of ["guidedDesignAfterSeam", "guidedDesignAfterPiece", "guidedDesignAfterSeams"]) $(id).replaceChildren();
  const option = (id, value, label) => {
    const node = document.createElement("option"); node.value = value; node.textContent = label; $(id).append(node);
  };
  option("guidedDesignAfterSeam", "", "Choose an original seam");
  option("guidedDesignAfterPiece", "", "Choose an original piece");
  const seams = guidedDesignSeamIds().filter(id => id.trim());
  if (seams.length > 2048) {
    guidedDesignAfterProblem = "The guided seam selector supports up to 2048 named source seams. Use a complete design request for this larger source; linked alterations without these adjustments remain available.";
  } else {
    for (const id of seams) {
      option("guidedDesignAfterSeam", id, id); option("guidedDesignAfterSeams", id, id);
    }
    for (const piece of source.pieces) option("guidedDesignAfterPiece", piece.id, piece.id);
  }
  $("guidedDesignAfterSourceStatus").textContent = guidedDesignAfterProblem ||
    (!seams.length ? "The original pattern has no named seams to select. Other linked alterations remain available." : "");
  $("guidedDesignAfterSourceStatus").hidden = !$("guidedDesignAfterSourceStatus").textContent;
  $("guidedDesignAfterSeam").value = ""; $("guidedDesignAfterPiece").value = "";
}
function showGuidedDesignAfter() {
  const parent = $("guidedDesignAfterList"); parent.replaceChildren(); guidedDesignAfterButtons.length = 0;
  for (const [index, operation] of guidedDesignAfter.entries()) {
    const row = document.createElement("div"), label = document.createElement("span"); row.className = "study-choice";
    label.textContent = `${index + 1}: ` + (operation.op === "true_seam" ?
      `true ${operation.seam}, B-side ${operation.end}.` :
      `cap ease on ${operation.piece}, seams ${operation.seams.join(", ")}, total ratio ${operation.target_ratio}.`);
    row.append(label);
    const action = (title, handler, unavailable = false) => {
      const button = document.createElement("button"); button.type = "button"; button.textContent = title; button.onclick = handler;
      guidedDesignAfterButtons.push({ button, unavailable }); row.append(button);
    };
    action(`Replace adjustment ${index + 1} from current controls`, () => saveGuidedDesignAfter(operation));
    action(`Move adjustment ${index + 1} earlier`, () => {
      const i = guidedDesignAfter.indexOf(operation); if (i <= 0) return;
      [guidedDesignAfter[i - 1], guidedDesignAfter[i]] = [guidedDesignAfter[i], guidedDesignAfter[i - 1]];
      changedGuidedDesignAfter();
    }, index === 0);
    action(`Move adjustment ${index + 1} later`, () => {
      const i = guidedDesignAfter.indexOf(operation); if (i < 0 || i + 1 >= guidedDesignAfter.length) return;
      [guidedDesignAfter[i], guidedDesignAfter[i + 1]] = [guidedDesignAfter[i + 1], guidedDesignAfter[i]];
      changedGuidedDesignAfter();
    }, index + 1 === guidedDesignAfter.length);
    action(`Remove adjustment ${index + 1}`, () => {
      const i = guidedDesignAfter.indexOf(operation); if (i < 0) return;
      guidedDesignAfter.splice(i, 1); changedGuidedDesignAfter();
    });
    parent.append(row);
  }
  $("guidedDesignAfterSummary").textContent = guidedDesignAfter.length ?
    `${guidedDesignAfter.length} ordered seam adjustments. These share the 16-operation limit with every linked site.` :
    "No seam adjustments added. Linked alterations retain the original seam checks.";
}
function changedGuidedDesignAfter() {
  showGuidedDesignAfter();
  if (guidedStudyMode() && $("guidedDesign").checked) invalidate(["study", "scenario"], true);
  else controls();
}
function saveGuidedDesignAfter(replacing = null) {
  if (!source || !guidedStudyMode() || !$("guidedDesign").checked) return;
  try {
    ensureGuidedDesignAfterSources();
    if (guidedDesignAfterProblem) throw Error(guidedDesignAfterProblem);
    const kind = $("guidedDesignAfterKind").value;
    let operation;
    if (kind === "true_seam") operation = { op: kind, seam: $("guidedDesignAfterSeam").value, end: $("guidedDesignAfterEnd").value };
    else if (kind === "cap_ease") {
      const ratio = $("guidedDesignAfterRatio").value.trim();
      if (!ratio) throw Error("Declare a positive target total cap / armhole length ratio.");
      operation = { op: kind, piece: $("guidedDesignAfterPiece").value,
        seams: Array.from($("guidedDesignAfterSeams").selectedOptions, option => option.value), target_ratio: Number(ratio) };
    } else throw Error("Choose seam truing or cap ease.");
    const operations = guidedDesignAfter.slice(), index = replacing ? operations.indexOf(replacing) : -1;
    if (replacing && index < 0) throw Error("Choose a current adjustment to replace.");
    if (replacing) operations[index] = operation; else operations.push(operation);
    const retained = buildGuidedDesignAfterOperations(operations, source.pieces.map(piece => piece.id), guidedDesignSeamIds());
    guidedDesignAfter.splice(0, guidedDesignAfter.length, ...retained); changedGuidedDesignAfter();
  } catch (e) { error(String(e.message || e)); }
}
function newGuidedDesignVariable(key = "linked_adjustment") {
  return { key, kind: "slash", axis: "x", at: "", multiplier: "1", values: "", dx: "", dy: "",
    lower: "", upper: "", initial: "", initialStep: "", minimumStep: "", pieces: [], vertices: [] };
}
function ensureGuidedDesignVertices() {
  if (!source || guidedDesignVertexSource === source) return;
  const options = [], limit = 2048;
  // Bound picker allocation independently of pattern/solver admission. Other workflows need no vertex DOM.
  collect: for (const piece of source.pieces) {
    for (const vertex in piece.vertices || {}) {
      if (!Object.hasOwn(piece.vertices, vertex)) continue;
      if (options.length === limit) {
        guidedDesignVertexProblem = "The guided vertex selector supports up to 2048 original vertices. Use a complete design request for this larger source; slash-and-spread and other comparisons remain available.";
        break collect;
      }
      options.push([piece.id, vertex]);
    }
  }
  guidedDesignVertexSource = source; $("guidedDesignVertices").replaceChildren();
  $("guidedDesignVertexStatus").textContent = guidedDesignVertexProblem;
  $("guidedDesignVertexStatus").hidden = !guidedDesignVertexProblem;
  if (guidedDesignVertexProblem) return;
  for (const pair of options) {
    const option = document.createElement("option"); option.value = JSON.stringify(pair);
    option.textContent = `${pair[0]} / ${pair[1]}`; $("guidedDesignVertices").append(option);
  }
}
function captureGuidedDesignVariable() {
  return { ...Object.fromEntries(Object.entries(guidedDesignFields).map(([field, id]) => [field, $(id).value])),
    pieces: Array.from($("guidedDesignPieces").selectedOptions, option => option.value),
    vertices: guidedDesignVertexProblem ? [...(guidedDesignVariables[guidedDesignCurrent]?.vertices || [])] :
      Array.from($("guidedDesignVertices").selectedOptions, option => option.value) };
}
function loadGuidedDesignVariable() {
  const draft = guidedDesignVariables[guidedDesignCurrent];
  for (const [field, id] of Object.entries(guidedDesignFields)) $(id).value = draft[field];
  if (draft.kind === "move") ensureGuidedDesignVertices();
  for (const [id, selected] of [["guidedDesignPieces", draft.pieces], ["guidedDesignVertices", draft.vertices]]) {
    const values = new Set(selected);
    for (const option of $(id).children) option.selected = values.has(option.value);
  }
  showGuidedDesignVariables();
}
function changeGuidedDesignVariables(change, changesRequest = true) {
  if (!source || !$("guidedDesign").checked) return;
  guidedDesignVariables[guidedDesignCurrent] = captureGuidedDesignVariable();
  change(); loadGuidedDesignVariable();
  if (changesRequest && guidedStudyMode()) invalidate(["study", "scenario"], true); else controls();
}
function showGuidedDesignVariables() {
  const parent = $("guidedDesignVariables"); parent.replaceChildren(); guidedDesignButtons.length = 0;
  guidedDesignVariables.forEach((draft, index) => {
    const row = document.createElement("div"), label = document.createElement("span"); row.className = "study-choice";
    label.textContent = `${index + 1}. ${draft.key || "Unnamed variable"}${index === guidedDesignCurrent ? " (editing)" : ""} — ${draft.kind === "move" ? "vertex movement" : "slash and spread"} `;
    row.append(label);
    const action = (title, unavailable, change, changesRequest = true) => {
      const button = document.createElement("button"); button.type = "button"; button.textContent = title;
      button.disabled = !source || !$("guidedDesign").checked || unavailable;
      button.onclick = () => {
        if (!unavailable && guidedDesignVariables[index] === draft) changeGuidedDesignVariables(change, changesRequest);
      };
      guidedDesignButtons.push({ button, unavailable }); row.append(button);
    };
    action(`Edit ${draft.key}`, index === guidedDesignCurrent, () => { guidedDesignCurrent = index; }, false);
    action(`Move ${draft.key} earlier`, index === 0, () => {
      const current = guidedDesignVariables[guidedDesignCurrent];
      [guidedDesignVariables[index - 1], guidedDesignVariables[index]] = [guidedDesignVariables[index], guidedDesignVariables[index - 1]];
      guidedDesignCurrent = guidedDesignVariables.indexOf(current);
    });
    action(`Remove ${draft.key}`, guidedDesignVariables.length === 1, () => {
      const current = guidedDesignVariables[guidedDesignCurrent]; guidedDesignVariables.splice(index, 1);
      const retained = guidedDesignVariables.indexOf(current);
      guidedDesignCurrent = retained < 0 ? Math.min(index, guidedDesignVariables.length - 1) : retained;
    });
    parent.append(row);
  });
}
$("guidedDesignAdd").onclick = () => {
  if (guidedDesignVariables.length >= 3) { error("Guided design supports at most three variables."); return; }
  changeGuidedDesignVariables(() => {
    let n = 2; while (guidedDesignVariables.some(variable => variable.key === `adjustment_${n}`)) n++;
    guidedDesignVariables.push(newGuidedDesignVariable(`adjustment_${n}`)); guidedDesignCurrent = guidedDesignVariables.length - 1;
  });
};
function guidedDesignInput(number, inScenario) {
  const mode = $("guidedDesignMode").value;
  if (!["grid", "adaptive"].includes(mode)) throw Error("Choose a listed grid or bounded adaptive discovery.");
  const drafts = guidedDesignVariables.map((draft, index) => index === guidedDesignCurrent ? captureGuidedDesignVariable() : draft);
  const finite = (raw, label) => {
    if (!raw.trim() || !Number.isFinite(Number(raw))) throw Error(`Supply a finite ${label}.`);
    return Number(raw);
  };
  const variables = drafts.map(draft => {
    const domain = mode === "grid" ? { values: draft.values.split(",").map(raw => finite(raw, `listed value for ${draft.key}`)) } :
      { boundsMm: [finite(draft.lower, `lower bound for ${draft.key}`), finite(draft.upper, `upper bound for ${draft.key}`)],
        initialMm: finite(draft.initial, `initial value for ${draft.key}`), initialStepMm: finite(draft.initialStep, `initial step for ${draft.key}`),
        minimumStepMm: finite(draft.minimumStep, `minimum step for ${draft.key}`) };
    let sites;
    if (draft.kind === "slash") {
      const at = finite(draft.at, `cut coordinate for ${draft.key}`), multiplier = finite(draft.multiplier, `multiplier for ${draft.key}`);
      sites = draft.pieces.map(piece => ({ piece, axis: draft.axis, at_mm: at, multiplier }));
    } else if (draft.kind === "move") {
      ensureGuidedDesignVertices();
      if (guidedDesignVertexProblem) throw Error(guidedDesignVertexProblem);
      const dx = finite(draft.dx, `horizontal multiplier for ${draft.key}`), dy = finite(draft.dy, `vertical multiplier for ${draft.key}`);
      sites = draft.vertices.map(raw => {
        const pair = JSON.parse(raw);
        if (!Array.isArray(pair) || pair.length !== 2 || pair.some(id => typeof id !== "string") ||
          !Object.hasOwn(source.pieces.find(piece => piece.id === pair[0])?.vertices || {}, pair[1])) {
          throw Error("Choose an existing original piece and vertex for each movement.");
        }
        return { op: "move", piece: pair[0], vertex: pair[1], dx_multiplier: dx, dy_multiplier: dy };
      });
    } else throw Error("Choose slash-and-spread or source-vertex movement.");
    return { key: draft.key, ...domain, sites };
  });
  const common = { variables, maxDesigns: number("guidedDesignMax", "maximum retained altered designs"),
    ...(guidedDesignAfter.length ? { after: guidedDesignAfter.map(operation => ({ ...operation,
      ...(operation.seams ? { seams: [...operation.seams] } : {}) })) } : {}) };
  if (mode === "grid") return { ...common,
    ...($("guidedDesignGridBudget").value.trim() ? { maxCandidates: number("guidedDesignGridBudget", "grid screening allowance") } : {}) };
  const objective = $("guidedDesignObjective").value;
  const materialKey = $("guidedDesignMaterial").value;
  if (objective !== "fit_band_violation_mm" && (!guidedDesignNomination || guidedDesignNomination.invalidated ||
    guidedDesignNomination.key !== materialKey || guidedDesignNomination.source !== guidedDesignMaterialSource(materialKey))) {
    throw Error("Choose a current frozen material explicitly to guide adaptive discovery.");
  }
  const caseKey = inScenario && (!guidedScenarioMode() || guidedMatrixMode()) && ["consumer_comparison", "consumer_pareto"].includes(objective) ? $("guidedDesignCase").value : "";
  if (caseKey && (!guidedDesignCaseNomination || guidedDesignCaseNomination.invalidated ||
    guidedDesignCaseNomination.key !== caseKey || guidedDesignCaseNomination.source !== scenarioSourceIdentity())) {
    throw Error("Choose a current named scenario explicitly to guide adaptive discovery.");
  }
  return { ...common, mode, objective, maxEvaluations: number("guidedDesignEvaluations", "unique discovery trial allowance"),
    ...(objective === "fit_band_violation_mm" ? {} : { materialKey }), ...(caseKey ? { caseKey } : {}) };
}
function resetGuidedDesign() {
  $("guidedDesign").checked = false;
  $("guidedDesignPieces").replaceChildren();
  $("guidedDesignVertices").replaceChildren();
  guidedDesignAfter.length = 0; guidedDesignAfterSource = null; guidedDesignAfterProblem = "";
  for (const id of ["guidedDesignAfterSeam", "guidedDesignAfterPiece", "guidedDesignAfterSeams"]) $(id).replaceChildren();
  $("guidedDesignAfterKind").value = "true_seam"; $("guidedDesignAfterEnd").value = ""; $("guidedDesignAfterRatio").value = "";
  $("guidedDesignAfterSourceStatus").textContent = ""; $("guidedDesignAfterSourceStatus").hidden = true; showGuidedDesignAfter();
  guidedDesignVertexSource = null; guidedDesignVertexProblem = "";
  $("guidedDesignVertexStatus").textContent = ""; $("guidedDesignVertexStatus").hidden = true;
  guidedDesignVariables.length = 0; guidedDesignVariables.push(newGuidedDesignVariable()); guidedDesignCurrent = 0;
  $("guidedDesignMode").value = "grid"; $("guidedDesignObjective").value = "fit_band_violation_mm";
  guidedDesignNomination = null;
  guidedDesignCaseNomination = null; refreshGuidedDesignCases();
  $("guidedDesignMaterial").value = ""; $("guidedDesignEvaluations").value = ""; refreshGuidedDesignMaterials();
  $("guidedDesignMax").value = "1"; $("guidedDesignGridBudget").value = ""; loadGuidedDesignVariable();
}
function resetGuidedScenarios() {
  guidedScenarioDrafts.length = 0; guidedScenarioDraftSource = {};
  $("guidedScenarioCaseName").value = ""; showGuidedScenarioDrafts();
  guidedMotionKeyframes.length = 0; $("guidedScenarioMotion").checked = false; $("guidedMotionSource").value = "form";
  $("guidedMotionHeat").checked = false;
  for (const id of ["guidedMotionHeatInitial", "guidedMotionHeatInside", "guidedMotionHeatOutside", "guidedMotionHeatInsideFilm", "guidedMotionHeatOutsideFilm"]) $(id).value = "";
  $("guidedMotionTime").value = ""; $("guidedMotionArm").value = "";
  $("guidedMotionLegs").value = "standing"; $("guidedMotionLegFraction").value = "1"; showGuidedMotionKeyframes();
  $("guidedMaterialStrain").checked = false; $("guidedMaterialStrainScope").value = "all";
  $("guidedMaterialStrainInstances").value = "";
  $("guidedContactLoads").checked = false; $("guidedContactLoadsScope").value = "all";
  $("guidedContactLoadsInstances").value = "";
  for (const [, id] of guidedTransportSources) $(id).checked = false;
}
function installPattern(raw, label, garments = [], data = parsePattern(raw)) {
  resetGuidedScenarios();
  resetGuidedDesign();
  source = data; sourceText = raw;
  $("patternName").textContent = `${label}: ${data.pieces.length} source pieces. Original retained.`;
  const group = (key, label, ids) => {
    outfitTargets.set(key, ids);
    const option = document.createElement("option"); option.value = key; option.textContent = label;
    $("target").append(option);
  };
  if (garments.length) {
    group("@outfit", "Every source piece in this outfit", data.pieces.map(p => p.id));
    garments.forEach((garment, i) => group(`@garment:${i}`, `${garment.id} — every source piece`, Object.values(garment.pieces)));
  }
  for (const p of data.pieces) {
    for (const target of ["target", "combinationPieces", "studyPieces", "guidedDesignPieces"]) {
      const o = document.createElement("option"); o.value = p.id; o.textContent = p.id; $(target).append(o);
    }
  }
}
function installDerivedPattern(raw, label) {
  const data = parsePattern(raw), retainedOutfit = outfit;
  const garments = retainedOutfit ? JSON.parse(retainedOutfit.composition_json).garments : [];
  resetPatternInput();
  ++roundRead; roundToken = null; window.opener = null;
  roundFiles.delete("patternFile"); $("patternFile").value = "";
  installPattern(raw, label, garments, data);
  outfit = retainedOutfit;
  if (outfit) $("selfContact").checked = true;
  if (roundFiles.size) $("roundImportText").textContent =
    "A derived pattern is now the current pattern. Received measurement, fit and body snapshots remain selected. Review them before recomputing this pattern.";
}
function resetPatternInput(keepImport = false) {
  cancelPhotoReturn();
  ++patternRead; source = null; sourceText = null; outfit = null; adoptedImport = false;
  $("bodyFitConsent").checked = false;
  defaultChange = null; pieceChanges.clear(); resetInterfaces(); edits();
  outfitTargets.clear();
  combinationGroups.clear(); showCombinations(); $("combinationPieces").replaceChildren();
  studyCandidates.length = 0; studyCandidateSequence = 0; showStudyCandidates(); $("studyPieces").replaceChildren();
  resetGuidedScenarios(); resetGuidedDesign();
  invalidate(["bodyCheck", "bind", ...boundOperations, "study", "scenario", "outfit", ...(keepImport ? [] : ["garmentcode"])]);
  $("patternName").textContent = "No pattern loaded.";
  $("target").replaceChildren(); const base = document.createElement("option"); base.value = "";
  base.textContent = "Pattern default (preserve explicit piece choices)"; $("target").append(base);
  return patternRead;
}
$("patternFile").onchange = async () => {
  clearRoundSources();
  const rev = resetPatternInput();
  try { const raw = await read("patternFile"), data = JSON.parse(raw); if (rev !== patternRead) return;
    if (data.schema === "ftx-outfit-drape/1") {
      const id = ++sequence; pending = { id, op: "outfit" };
      $("status").textContent = "Preparing source-qualified garments and contact pairs…";
      controls(); worker.postMessage({ type: "outfit", request: id, outfitRequest: raw });
      return;
    }
    installPattern(raw, $("patternFile").files[0].name);
    controls();
  } catch (e) { if (rev === patternRead) { source = null; error(String(e.message || e)); controls(); } }
};
$("assign").onclick = () => {
  if (!inspected || !source || pending) return;
  const target = $("target").value;
  if (target === "") defaultChange = inspected;
  else if (outfitTargets.has(target)) for (const id of outfitTargets.get(target)) pieceChanges.set(id, inspected);
  else pieceChanges.set(target, inspected);
  edits(); invalidate(["bind", ...boundOperations]);
};
$("reset").onclick = () => { defaultChange = null; pieceChanges.clear(); resetInterfaces(); edits(); invalidate(["bind", ...boundOperations]); };
$("useTreatment").onclick = () => {
  if (!ready || !preparedTreatment || pending) return;
  try {
    installDerivedPattern(preparedTreatment, "Derived treatment pattern");
    $("status").textContent = "Derived pattern loaded. Review the current body, fit and process inputs, then Freeze selections and bind to recompute its consumers.";
    controls();
  } catch (e) { error(String(e.message || e)); controls(); }
};
$("useGarmentcode").onclick = () => {
  if (!preparedImport || pending) return;
  const { pattern_json: raw, fit_spec_json: fit } = preparedImport;
  const useFit = $("useGarmentcodeFit").checked;
  if (useFit && typeof fit !== "string") { error("This import has no supplied fit specification."); return; }
  clearRoundSources();
  resetPatternInput(true); preparedImport = null;
  $("useGarmentcodeFit").checked = false;
  $("patternFile").value = "";
  installPattern(raw, "Imported GarmentCode pattern — REVIEW_REQUIRED");
  adoptedImport = true;
  if (useFit) {
    selectedImportFit = fit; roundFiles.set("fit", fit); $("fit").value = "";
    $("importFitNote").hidden = false;
    link("importFit", $("importFitFiles"), { name: "fit_spec.json", text: fit, mime: "application/json" });
  }
  $("status").textContent = "Imported pattern loaded for review. " +
    (useFit ? "The supplied fit specification is selected. " : "") +
    "Assign materials and freeze selections to recompute its consumers. Conversion does not establish fit.";
  controls();
};
function invalidateGarmentcode() {
  if (adoptedImport) {
    clearImportFit();
    resetPatternInput();
    $("patternName").textContent = "Imported pattern withdrawn. Convert the changed inputs and review the new result before adopting it.";
  } else invalidate(["garmentcode"]);
}
for (const id of ["garmentcodeSource", "garmentcodeAnnotations"]) $(id).addEventListener("change", invalidateGarmentcode);
$("garmentcodeArcs").addEventListener("change", invalidateGarmentcode);
for (const id of ["garmentcodeArcSagitta", "garmentcodeArcLength"]) $(id).addEventListener("input", invalidateGarmentcode);
$("clearGarmentcodeAnnotations").onclick = () => { $("garmentcodeAnnotations").value = ""; invalidateGarmentcode(); };
$("addCombination").onclick = () => {
  if (!inspected || !source || pending) return;
  try {
    const key = $("combinationGroup").value.trim(), choice = $("combinationChoice").value.trim();
    if (!key || !choice) throw Error("Name both the material group and its choice.");
    const target = $("combinationScope").value === "default" ? { scope: "default" } :
      { scope: "pieces", ids: [...$("combinationPieces").children].filter(o => o.selected).map(o => o.value).sort() };
    if (target.scope === "pieces" && !target.ids.length) throw Error("Select source pieces for this group.");
    const old = combinationGroups.get(key);
    if (!old && combinationGroups.size >= 8) throw Error("Use at most eight material choice groups.");
    if (old && JSON.stringify(old.target) !== JSON.stringify(target)) throw Error("A group's choices must share the same target. Remove that group's choices before changing its pieces.");
    for (const [other, group] of combinationGroups) {
      if (other === key) continue;
      if (target.scope === "default" && group.target.scope === "default") throw Error("Only one group may change the pattern default.");
      if (target.scope === "pieces" && group.target.scope === "pieces" && target.ids.some(id => group.target.ids.includes(id))) {
        throw Error("Source-piece groups must be disjoint.");
      }
    }
    const choices = new Map(old?.choices || []); choices.set(choice, inspected);
    let count = choices.size;
    for (const [other, group] of combinationGroups) if (other !== key) count *= group.choices.size;
    if (count > 32) throw Error("These choices exceed the limit of 32 complete combinations. Remove a choice first.");
    combinationGroups.set(key, { target, choices }); showCombinations(); invalidate(["study", "scenario"]);
  } catch (e) { error(String(e.message || e)); controls(); }
};
$("clearCombinations").onclick = () => { combinationGroups.clear(); showCombinations(); invalidate(["study", "scenario"]); };
$("combinationMode").addEventListener("change", () => invalidate(["study", "scenario"]));
$("combinationInterfaces").addEventListener("change", () => invalidate(["study", "scenario"]));
$("clearInterfaces").onclick = () => { resetInterfaces(); edits(); invalidate(["bind", ...boundOperations]); };
$("interfacesFile").onchange = async () => {
  const rev = ++interfaceRead; interfaceChange = null; interfaceReady = false;
  edits(); invalidate(["bind", ...boundOperations]);
  $("interfacesSummary").textContent = "Reading replacement interface evidence…";
  if (!has("interfacesFile")) { resetInterfaces(); controls(); return; }
  try {
    const raw = await read("interfacesFile"); if (rev !== interfaceRead) return;
    const pairs = JSON.parse(raw);
    if (!Array.isArray(pairs)) throw Error("Replacement interfaces must be a JSON list; [] explicitly removes all original pairs.");
    // Keep the authored numerical JSON. Rust validates evidence and exact final material identities.
    interfaceChange = raw; interfaceReady = true;
    $("interfacesSummary").textContent = `Replace the complete interface table with ${pairs.length} supplied pair(s). Freeze checks their sources and assigned material identities.`;
    edits(); controls();
  } catch (e) {
    if (rev !== interfaceRead) return;
    $("interfacesSummary").textContent = "Replacement evidence is unavailable; binding is disabled until corrected or cleared.";
    error(String(e.message || e)); controls();
  }
};
for (const id of [...originDomainFields.flat(), ...originRationaleFields, "originEstimates"]) {
  $(id).addEventListener(id === "originEstimates" ? "change" : "input", () => {
    if (preparedBy === "linearizeOrigin") {
      invalidateThermalPoint();
      ++selectionRead; preparedBy = null; inspected = null;
      $("selection").value = originSource?.selectionJson || "";
      $("detail").textContent = ""; $("capabilities").replaceChildren();
    }
    invalidate(["linearizeOrigin"], true);
  });
}
for (const [id, operation] of [["compressionRequest", "pressure"], ["compressionLayer", "pressure"], ["calibrationRequest", "calibrate"],
  ["calibrationMode", "calibrate"], ["calibrationEstimates", "calibrate"], ["stripFitRequest", "stripFit"]]) {
  $(id).addEventListener(id === "compressionLayer" ? "input" : "change", () => {
    if (preparedBy === operation) {
      resetOriginSource();
      ++selectionRead; preparedBy = null; inspected = null;
      $("detail").textContent = ""; $("capabilities").replaceChildren();
    }
    invalidate([operation]);
  });
}
function supplementalChanged(resetConsent) {
  if (resetConsent) $("supplementalConsent").checked = false;
  $("bodyFitConsent").checked = false;
  invalidate(["bodyCheck", "exports", "fold", "heat", "vaporSurface", "coverage", ...bodyDependentStudies("measurements")], true);
}
function cancelPhotoReturn() {
  if (!photoTransfer) return;
  photoTransfer.target.postMessage({type:"ftx-materials-photo-cancelled",token:photoTransfer.token},new URL(import.meta.url).origin);
  photoTransfer = null;
  $("photoEvidenceNote").textContent = "Photo return cancelled. Open the photo tool again or add a downloaded file.";
}
function clearPhotoEvidence() {
  cancelPhotoReturn(); photoEvidence = null; photoProblem = null;
  clear(["photoEvidence"]); $("photoEvidenceFiles").replaceChildren(); $("photoEvidenceNote").hidden = true;
}
$("openPhoto").onclick = () => {
  if (photoTransfer && !photoTransfer.target.closed) { error("Finish or close the current photo tab before opening another."); return; }
  const token = Array.from(crypto.getRandomValues(new Uint8Array(16)),v=>v.toString(16).padStart(2,"0")).join("");
  const url = new URL("photo.html",import.meta.url); url.searchParams.set("materialsReturn",token);
  const target = window.open(url.href,"_blank");
  if (!target) { error("Photo popup blocked. Open the photo tool and add its downloaded photo-measurements.json instead."); return; }
  clearPhotoEvidence(); photoTransfer = {target,token}; supplementalChanged(true);
  $("photoEvidenceNote").hidden = false;
  $("photoEvidenceNote").textContent = "Waiting for the photo tool. Return current records there, then review the same-wearer confirmation here. Clear additional measurements cancels this return.";
};
window.addEventListener("message",event=>{
  const transfer = photoTransfer, m = event.data, origin = new URL(import.meta.url).origin;
  if (!transfer || event.origin !== origin || event.source !== transfer.target || m?.token !== transfer.token) return;
  if (m.type === "ftx-photo-records-offer" && !transfer.offered) {
    transfer.offered = true; transfer.target.postMessage({type:"ftx-photo-records-ready",token:transfer.token},origin); return;
  }
  if (m.type !== "ftx-photo-records-cancelled" && !(m.type === "ftx-photo-records" && transfer.offered)) return;
  photoTransfer = null; supplementalChanged(true);
  if (m.type === "ftx-photo-records-cancelled" || !(m.records instanceof Blob) || m.records.size > 16*1024*1024) {
    photoProblem = "Photo return refused or changed. Clear additional measurements, then measure again or add a downloaded file.";
    $("photoEvidenceNote").textContent = photoProblem;
    transfer.target.postMessage({type:"ftx-materials-photo-cancelled",token:transfer.token},origin); return;
  }
  photoEvidence = m.records;
  link("photoEvidence",$("photoEvidenceFiles"),{name:"photo-measurements.json",bytes:photoEvidence,mime:"application/json"});
  $("photoEvidenceNote").textContent = "Photo records received. Review the original download and confirm this same wearer before checking. They remain PHOTO_ESTIMATED; readiness and physical fit remain separate.";
  transfer.target.postMessage({type:"ftx-materials-photo-received",token:transfer.token},origin);
});
$("supplementalFiles").addEventListener("change", () => supplementalChanged(true));
$("supplementalConsent").addEventListener("change", () => supplementalChanged(false));
$("clearSupplemental").onclick = () => { clearPhotoEvidence(); $("supplementalFiles").value = ""; supplementalChanged(true); };
for (const [id, keys] of Object.entries({ measurements:["bodyCheck","exports","fold","heat","vaporSurface","coverage"], fit:["exports","fold","heat","vaporSurface","coverage"],
  body:["bodyCheck","fold","heat","vaporSurface","coverage"], selfContact:["fold","heat","vaporSurface","coverage"], twoStart:["fold","heat","vaporSurface","coverage"], cutOptions:["cut","price"], quotes:["price"], market:["price"],
  drapeLegs:["fold","heat","vaporSurface","coverage"], drapeLegFraction:["fold","heat","vaporSurface","coverage"], maxSettleSteps:["fold","heat","vaporSurface","coverage"],
  stockRequest:["stock","stockIdentities","stockPrice"], stockPricing:["stockPrice"], cutJobRequest:["cutJob"], sewOptions:["sew"], sewOptimize:["sew"], heatConditions:["heat"], map:["coverage"], coverageConditions:["coverage"], vaporRequest:["vapor"],
  stripAcquisitionRequest:["stripAcquisition"], stripExperimentRequest:["stripExperiment"],
  stripHistoryRequest:["stripHistory"], vaporSurfaceConditions:["vaporSurface"],
  treatmentRequest:["treatment"], screenRequest:["screen"], studyRequest:["study","scenario"], scenarioRequest:["scenario"], scenarioBody:[] })) {
  $(id).addEventListener("change", () => {
    if (Object.hasOwn(sheetConditionForms, id) && sheetConditionsMode(id) !== "file") { controls(); return; }
    let scenarioCasesLoading;
    if (id === "stockRequest") {
      selectedStockRequest = null; clear(["selectedStock"]);
      $("selectedStockFiles").replaceChildren(); $("selectedStockNote").hidden = true;
    }
    if (id === "studyRequest") $("studyMode").value = "file";
    if (id === "screenRequest") $("screenMode").value = "file";
    if (id === "scenarioRequest") {
      $("scenarioMode").value = "file";
      scenarioCasesLoading = loadGuidedScenarioCases();
    }
    if (id === "measurements" || id === "body") cancelPhotoReturn();
    if (id === "fit") clearImportFit();
    if (id === "measurements") $("supplementalConsent").checked = false;
    if (id === "measurements" || id === "body") $("bodyFitConsent").checked = false;
    if (["measurements", "fit", "body", "selfContact", "twoStart", "drapeLegs", "drapeLegFraction", "maxSettleSteps"].includes(id)) {
      ++roundRead; roundToken = null; window.opener = null; roundFiles.delete(id);
      if (id === "body") showReceivedBody();
    }
    const guidedKeys = guidedSourceStudies(id);
    const guidedProcessSource = guidedStudyMode() && ["cutOptions", "stockRequest", "stockPricing", "cutJobRequest",
      "sewOptions", "sewOptimize", "quotes", "market"].includes(id);
    invalidate([...keys, ...bodyDependentStudies(id), ...guidedKeys], guidedProcessSource || guidedKeys.length > 0 ||
      (studyBodyMode() === "preview" && ["drapeLegs", "drapeLegFraction"].includes(id)) ||
      ["measurements", "fit", "body", "selfContact", "scenarioBody", "heatConditions", "vaporSurfaceConditions"].includes(id));
    return scenarioCasesLoading;
  });
}
for (const id of ["drapeLegFraction", "maxSettleSteps"]) {
  $(id).addEventListener("input", () => {
    ++roundRead; roundToken = null; window.opener = null;
    invalidate(["fold", "heat", "vaporSurface", "coverage", ...bodyDependentStudies(id)], true);
  });
}
$("bodyFitFile").addEventListener("change", () => {
  ++roundRead; roundToken = null; window.opener = null; roundFiles.delete("bodyFitFile");
  showReceivedBody();
  $("bodyFitConsent").checked = false;
  invalidate(["bodyCheck", "fold", "heat", "vaporSurface", "coverage", ...bodyDependentStudies("bodyFitFile")], true);
});
for (const id of ["bodyFitConsent", "bodyArmDeg"]) {
  $(id).addEventListener(id === "bodyArmDeg" ? "input" : "change", () => {
    ++roundRead; roundToken = null; window.opener = null;
    invalidate([...(id === "bodyFitConsent" ? ["bodyCheck"] : []), "fold", "heat", "vaporSurface", "coverage", ...bodyDependentStudies(id)], true);
  });
}
$("clearBodyFit").onclick = () => {
  ++roundRead; roundToken = null; window.opener = null;
  $("bodyFitFile").value = ""; roundFiles.delete("bodyFitFile"); $("bodyFitConsent").checked = false;
  showReceivedBody();
  invalidate(["bodyCheck", "fold", "heat", "vaporSurface", "coverage", ...bodyDependentStudies("bodyFitFile")], true);
};
$("studyOnBody").addEventListener("change", () => invalidate(["study"], true));
$("studyBodySource").addEventListener("change", () => invalidate(["scenario", ...(studyUsesBody() ? ["study"] : [])], true));
$("thermalPointTemperature").addEventListener("input", invalidateThermalPoint);
$("thermalRequest").addEventListener("change", () => invalidate(["thermalHistory"]));
$("clearCoverageBridges").onclick = () => { $("coverageBridges").value = ""; invalidate(["coverage"]); };
$("coverageBridges").addEventListener("change", () => invalidate(["coverage"]));
function showReceivedBody() {
  clear(["roundBody"]); $("receivedBodyFiles").replaceChildren();
  const model = roundFiles.get("body"), fit = roundFiles.get("bodyFitFile");
  $("receivedBodyNote").hidden = !model && !fit;
  $("receivedBodyNote").textContent = "Received body sources remain local immutable snapshots. Manual file choices replace them. " +
    (fit ? "Review the original fit, confirm this wearer, and choose the exact model before draping. The transferred angle selects the ordinary drape pose." :
      "The received model will be fitted to the current measurements when you choose Re-drape.");
  if (model) link("roundBody", $("receivedBodyFiles"), { name: "body-model.ftxbody", bytes: model, mime: "application/octet-stream" });
  if (fit) link("roundBody", $("receivedBodyFiles"), { name: "body_fit.json", text: fit, mime: "application/json" });
}
function clearImportFit() {
  if (selectedImportFit !== null && roundFiles.get("fit") === selectedImportFit) roundFiles.delete("fit");
  selectedImportFit = null;
  clear(["importFit"]); $("importFitFiles").replaceChildren(); $("importFitNote").hidden = true;
}
function clearRoundSources() {
  cancelPhotoReturn();
  clearImportFit();
  ++roundRead; roundToken = null; roundFiles.clear(); window.opener = null;
  $("supplementalConsent").checked = false;
  $("bodyFitConsent").checked = false; showReceivedBody();
  $("roundImportNote").hidden = true;
}
$("clearRoundImport").onclick = () => { clearRoundSources(); resetPatternInput(); controls(); };
const roundOrigin = new URL(window.location.href).origin;
$("openPatternWorkbench").onclick = () => {
  if (!ready || pending || !bound || typeof bound.pattern_json !== "string" || !/^[0-9a-f]{64}$/.test(bound.pattern_sha256)) return;
  if (patternTransfer && !patternTransfer.target.closed) {
    $("status").textContent = "A Pattern Workbench handoff is still opening. Finish or close that tab before opening another."; return;
  }
  const token = Array.from(crypto.getRandomValues(new Uint8Array(16)), v => v.toString(16).padStart(2,"0")).join("");
  const url = new URL("index.html", import.meta.url); url.searchParams.set("materialImport", token);
  const target = window.open(url.href, "_blank");
  if (!target) { $("status").textContent = "Pattern Workbench popup blocked. Download pattern.json and select it there instead."; return; }
  patternTransfer = { target, token, pattern: new Blob([bound.pattern_json], { type: "application/json" }), source_sha256: bound.pattern_sha256 };
  $("status").textContent = "Pattern Workbench opened. Select wearer and fit evidence there, then explicitly check before altering the pattern.";
};
window.addEventListener("message", event => {
  const transfer = patternTransfer, m = event.data;
  if (!transfer || event.origin !== roundOrigin || event.source !== transfer.target ||
      m?.type !== "ftx-material-pattern-ready" || m.token !== transfer.token) return;
  transfer.target.postMessage(transfer.pattern ? { type: "ftx-material-pattern", token: transfer.token,
    pattern: transfer.pattern, source_sha256: transfer.source_sha256 } :
    { type: "ftx-material-pattern-cancelled", token: transfer.token }, roundOrigin);
  patternTransfer = null;
});
const handoffType = traceImport ? "ftx-trace-pattern" : "ftx-round-sources";
if (roundToken && /^[0-9a-f]{32}$/.test(roundToken) && roundOrigin !== "null" && window.opener) {
  const sender = window.opener;
  $("roundImportNote").hidden = false;
  if (traceImport) $("clearRoundImport").textContent = "Clear received tracing pattern";
  $("roundImportText").textContent = traceImport ? "Waiting for the current tracing review. Material binding and every computation require your action." :
    "Waiting for the current flat round. Material binding and every computation require your action.";
  window.addEventListener("message", async event => {
    const m = event.data;
    if (!roundToken || event.origin !== roundOrigin || event.source !== sender || m?.token !== roundToken ||
      ![handoffType, `${handoffType}-cancelled`].includes(m.type)) return;
    roundToken = null; window.opener = null;
    const rev = ++roundRead;
    $("supplementalConsent").checked = false;
    resetPatternInput();
    try {
      if (m.type.endsWith("-cancelled")) throw Error("The source review changed before transfer. Review it again.");
      if (traceImport && (typeof m.source_sha256 !== "string" || !/^[0-9a-f]{64}$/.test(m.source_sha256))) {
        throw Error("A traced pattern needs its source image SHA-256.");
      }
      const settings = traceImport ? undefined : m.drapeSettings;
      if (settings !== undefined && (!settings || typeof settings !== "object" || Array.isArray(settings) ||
          ![0, 2].includes(settings.self_contact_mm) || typeof settings.two_start !== "boolean" ||
          !["standing", "seated", "squat"].includes(settings.legs) || !Number.isFinite(settings.leg_fraction) ||
          settings.leg_fraction < 0 || settings.leg_fraction > 1 || !Number.isSafeInteger(settings.max_settle_steps) ||
          settings.max_settle_steps < 0 || settings.max_settle_steps > 256)) throw Error("Received drape settings are incomplete or outside the supported controls.");
      const hasArm = settings !== undefined && Object.hasOwn(settings, "arm_abduction_deg");
      if (hasArm && (!Number.isFinite(settings.arm_abduction_deg) || settings.arm_abduction_deg < 0 || settings.arm_abduction_deg > 180)) {
        throw Error("Received drape arm angle must be from 0 to 180 degrees.");
      }
      const files = new Map();
      const inputs = traceImport ? [["pattern", "patternFile"]] : [["pattern", "patternFile"], ["measurements", "measurements"], ["fit", "fit"]];
      for (const [key, id] of inputs) {
        const blob = traceImport ? m.pattern : m.files?.[key];
        if (!(blob instanceof Blob) || blob.size > 64 * 1024 * 1024) throw Error("Each transferred file must be JSON within 64 MiB.");
        const bytes = await blob.arrayBuffer();
        if (rev !== roundRead) return;
        if (bytes.byteLength > 64 * 1024 * 1024) throw Error("A round file exceeds 64 MiB.");
        const raw = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes);
        const parsed = JSON.parse(raw);
        if (traceImport && parsed?.meta?.scale_status !== "FITTED") throw Error("Fit and review the trace scale before choosing materials; use Pattern Workbench for an unscaled trace.");
        files.set(id, raw);
      }
      const context = traceImport ? undefined : m.bodySource;
      if (context != null) {
        if (typeof context !== "object" || Array.isArray(context) || (!context.model && !context.fit)) throw Error("Body context needs a model or an accepted-fit source.");
        if (context.model != null) {
          if (!(context.model instanceof Blob) || !context.model.size || context.model.size > 256 * 1024 * 1024) throw Error("Received body model must be a nonempty file within 256 MiB.");
          files.set("body", context.model);
        }
        if (context.fit != null) {
          if (!(context.fit instanceof Blob) || context.fit.size > 16 * 1024 * 1024) throw Error("Received accepted fit exceeds its 16 MiB input budget.");
          if (!Number.isFinite(context.armDeg) || context.armDeg < 0 || context.armDeg > 180) throw Error("Received drape arm angle must be from 0 to 180 degrees.");
          if (hasArm && settings.arm_abduction_deg !== context.armDeg) throw Error("Received drape arm angles disagree.");
          const bytes = await context.fit.arrayBuffer();
          if (rev !== roundRead) return;
          if (bytes.byteLength > 16 * 1024 * 1024) throw Error("Received accepted fit exceeds its 16 MiB input budget.");
          const raw = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes);
          const parsed = JSON.parse(raw);
          if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw Error("Received accepted fit must be a JSON object.");
          files.set("bodyFitFile", raw);
        } else if (context.armDeg != null) throw Error("A received drape pose requires an accepted-fit source.");
      }
      installPattern(files.get("patternFile"), traceImport ? `Traced pattern.json — image ${m.source_sha256}` : "Received round pattern.json");
      for (const [id, raw] of files) roundFiles.set(id, raw);
      $("bodyArmDeg").value = String(hasArm ? settings.arm_abduction_deg : context?.fit ? context.armDeg : 35);
      if (settings !== undefined) {
        $("selfContact").checked = settings.self_contact_mm === 2; $("twoStart").checked = settings.two_start;
        $("drapeLegs").value = settings.legs; $("drapeLegFraction").value = String(settings.leg_fraction);
        $("maxSettleSteps").value = String(settings.max_settle_steps);
      }
      $("supplementalConsent").checked = false;
      $("bodyFitConsent").checked = false; showReceivedBody();
      $("roundImportText").textContent = traceImport ?
        `Received traced pattern.json for image ${m.source_sha256}. Exact source retained. Assign materials, then Freeze selections and bind before running a consumer. Select measurements, fit and body evidence separately. Tracing remains REVIEW_ONLY; unscaled geometry is not ready for cutting. Manual file choices replace received inputs.` :
        "Received pattern.json, measurements.json and fit_spec.json. Originals retained in this tab. Review sources, then Freeze selections and bind before running a consumer. " +
        (context ? "Selected body sources also arrived; review them and confirm the accepted-fit association separately before draping. " : "Choose a body separately for draping. ") +
        (settings !== undefined ? "Drape controls retain the source round's posture, settling, contact and sewing-path choices. " : "") +
        "Manual file choices replace received inputs.";
      controls();
    } catch (e) {
      if (rev !== roundRead) return;
      roundFiles.clear(); showReceivedBody(); resetPatternInput();
      $("roundImportText").textContent = `${traceImport ? "Tracing" : "Round"} handoff refused: ${e.message || e} Choose the files manually or return to the current review.`;
      controls();
    }
  });
  sender.postMessage({ type: `${handoffType}-ready`, token: roundToken }, roundOrigin);
} else roundToken = null;
window.addEventListener("pagehide", () => { clearPhotoEvidence(); clearRoundSources(); ++epoch; worker?.terminate(); clear(["bind", ...operations]); });
start();
