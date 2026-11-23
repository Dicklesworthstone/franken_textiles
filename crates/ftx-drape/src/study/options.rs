//! Serialization at the scenario boundary, leaving numerical option definitions unchanged.
use crate::{
    cloth::NewtonStrategy,
    contact::{AdaptiveBarrierPolicy, ContactParams, StickSlipFriction},
    damping::DampingOptions,
    elements::{BendingModel, CompressionModel},
    garment::{ConstructionMechanicsOptions, MeshOptions},
    integrator::TimeIntegrator,
    run::{DrapeOptions, StaticEquilibriumOptions},
    start::SelfRepulsionOptions,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

fn is_zero(value: &usize) -> bool {
    *value == 0
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "ContactParams", deny_unknown_fields)]
struct Contact {
    dhat: f64,
    thickness: f64,
    kappa: f64,
    mu: f64,
    eps_v: f64,
}

impl Serialize for ContactParams {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Contact::serialize(self, serializer)
    }
}
impl<'de> Deserialize<'de> for ContactParams {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Contact::deserialize(deserializer)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(
    remote = "DrapeOptions",
    default = "DrapeOptions::default",
    deny_unknown_fields
)]
struct Options {
    spacing: f64,
    sew_steps: usize,
    drape_steps: usize,
    #[serde(skip_serializing_if = "is_zero")]
    max_settle_steps: usize,
    #[serde(skip_serializing_if = "is_zero")]
    elastic_hold_steps: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    static_equilibrium: Option<StaticEquilibriumOptions>,
    step: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_subdivision: Option<crate::stepping::SubdivisionOptions>,
    tol: f64,
    compression: f64,
    lag_rounds: [usize; 2],
    elastic_ea: f64,
    contact: ContactParams,
    self_contact: Option<ContactParams>,
    pcg_iters: usize,
    #[serde(default, skip_serializing_if = "NewtonStrategy::is_default")]
    newton_strategy: NewtonStrategy,
    #[serde(default, skip_serializing_if = "TimeIntegrator::is_default")]
    integrator: TimeIntegrator,
    #[serde(default, skip_serializing_if = "BendingModel::is_default")]
    bending_model: BendingModel,
    #[serde(default, skip_serializing_if = "CompressionModel::is_default")]
    compression_model: CompressionModel,
    #[serde(default, skip_serializing_if = "DampingOptions::is_default")]
    damping: DampingOptions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    adaptive_barrier: Option<AdaptiveBarrierPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stick_slip: Option<StickSlipFriction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    self_stick_slip: Option<StickSlipFriction>,
    #[serde(default, skip_serializing_if = "MeshOptions::is_default")]
    mesh: MeshOptions,
    #[serde(
        default,
        skip_serializing_if = "ConstructionMechanicsOptions::is_default"
    )]
    construction: ConstructionMechanicsOptions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    start_self_repulsion: Option<SelfRepulsionOptions>,
}

impl Serialize for DrapeOptions {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Options::serialize(self, serializer)
    }
}
impl<'de> Deserialize<'de> for DrapeOptions {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Options::deserialize(deserializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subdivision_is_opt_in_and_preserves_the_exact_requested_budget() {
        let ordinary = serde_json::to_value(DrapeOptions::default()).unwrap();
        assert!(ordinary.get("time_subdivision").is_none());
        let mut declared = ordinary.clone();
        declared["time_subdivision"] = serde_json::json!({"max_halvings":8,"max_trials":512});
        let options: DrapeOptions = serde_json::from_value(declared.clone()).unwrap();
        options.validate().unwrap();
        assert_eq!(serde_json::to_value(options).unwrap(), declared);
        assert!(
            serde_json::from_value::<DrapeOptions>(ordinary)
                .unwrap()
                .time_subdivision
                .is_none()
        );
        declared["time_subdivision"]["relax_tolerance"] = serde_json::json!(true);
        assert!(serde_json::from_value::<DrapeOptions>(declared).is_err());
    }

    #[test]
    fn omitted_hold_preserves_the_wire_and_an_explicit_window_round_trips() {
        let ordinary = serde_json::to_value(DrapeOptions::default()).unwrap();
        assert!(ordinary.get("elastic_hold_steps").is_none());
        let mut declared = ordinary.clone();
        declared["elastic_hold_steps"] = serde_json::json!(2);
        let options: DrapeOptions = serde_json::from_value(declared.clone()).unwrap();
        options.validate().unwrap();
        assert_eq!(serde_json::to_value(options).unwrap(), declared);
        assert_eq!(
            serde_json::from_value::<DrapeOptions>(ordinary)
                .unwrap()
                .elastic_hold_steps,
            0
        );
    }

    #[test]
    fn settling_is_opt_in_and_its_explicit_cap_round_trips() {
        let ordinary = serde_json::to_value(DrapeOptions::default()).unwrap();
        assert!(ordinary.get("max_settle_steps").is_none());
        let mut declared = ordinary.clone();
        declared["max_settle_steps"] = serde_json::json!(32);
        let options: DrapeOptions = serde_json::from_value(declared.clone()).unwrap();
        options.validate().unwrap();
        assert_eq!(serde_json::to_value(options).unwrap(), declared);
        assert_eq!(
            serde_json::from_value::<DrapeOptions>(ordinary)
                .unwrap()
                .max_settle_steps,
            0
        );
    }

    #[test]
    fn static_equilibrium_preserves_the_ordinary_wire_and_exact_explicit_settings() {
        let ordinary = serde_json::to_value(DrapeOptions::default()).unwrap();
        assert!(ordinary.get("static_equilibrium").is_none());
        let mut declared = ordinary.clone();
        declared["static_equilibrium"] = serde_json::json!({
            "correction_tolerance_m":2e-6, "max_newton_iterations":120
        });
        let options: DrapeOptions = serde_json::from_value(declared.clone()).unwrap();
        options.validate().unwrap();
        assert_eq!(serde_json::to_value(options).unwrap(), declared);
        declared["static_equilibrium"]["time_s"] = serde_json::json!(1.0);
        assert!(serde_json::from_value::<DrapeOptions>(declared).is_err());
        assert!(
            serde_json::from_value::<DrapeOptions>(ordinary)
                .unwrap()
                .static_equilibrium
                .is_none()
        );
    }

    #[test]
    fn newton_strategy_and_integrator_are_opt_in_and_round_trip() {
        let ordinary = serde_json::to_value(DrapeOptions::default()).unwrap();
        assert!(ordinary.get("newton_strategy").is_none());
        assert!(ordinary.get("integrator").is_none());

        let mut declared = ordinary.clone();
        declared["newton_strategy"] = serde_json::json!({
            "kind": "adaptive_ilu",
            "pcg_iters": 64,
            "max_shifts": 4
        });
        declared["integrator"] = serde_json::json!({
            "kind": "generalized_alpha",
            "rho_inf": 0.8
        });
        let options: DrapeOptions = serde_json::from_value(declared.clone()).unwrap();
        options.validate().unwrap();
        assert_eq!(serde_json::to_value(options).unwrap(), declared);

        // Invalid rho_inf and unknown fields must refuse fail-closed.
        let mut bad_rho = declared.clone();
        bad_rho["integrator"]["rho_inf"] = serde_json::json!(1.5);
        let bad_opts: DrapeOptions = serde_json::from_value(bad_rho).unwrap();
        assert!(bad_opts.validate().is_err());

        declared["integrator"]["extra"] = serde_json::json!(true);
        assert!(serde_json::from_value::<DrapeOptions>(declared).is_err());
    }

    #[test]
    fn bending_compression_and_damping_options_are_opt_in_and_round_trip() {
        let ordinary = serde_json::to_value(DrapeOptions::default()).unwrap();
        assert!(ordinary.get("bending_model").is_none());
        assert!(ordinary.get("compression_model").is_none());
        assert!(ordinary.get("damping").is_none());

        let mut declared = ordinary;
        declared["bending_model"] = serde_json::json!("discrete_shells");
        declared["compression_model"] = serde_json::json!("principal");
        declared["damping"] = serde_json::json!({
            "air_drag": {
                "rho_air_kg_m3": 1.225,
                "c_normal": 1.2,
                "c_tangent": 0.05,
                "eps_v_m_s": 0.001
            },
            "rayleigh_membrane_s": 0.002,
            "rayleigh_bending_s": 0.003
        });
        let options: DrapeOptions = serde_json::from_value(declared.clone()).unwrap();
        options.validate().unwrap();
        assert_eq!(serde_json::to_value(options).unwrap(), declared);

        let mut bad = declared.clone();
        bad["damping"]["rayleigh_membrane_s"] = serde_json::json!(-0.1);
        let bad_opts: DrapeOptions = serde_json::from_value(bad).unwrap();
        assert!(bad_opts.validate().is_err());

        declared["damping"]["unknown_field"] = serde_json::json!(1);
        assert!(serde_json::from_value::<DrapeOptions>(declared).is_err());
    }

    #[test]
    fn adaptive_barrier_and_stick_slip_are_opt_in_and_round_trip() {
        let ordinary = serde_json::to_value(DrapeOptions::default()).unwrap();
        assert!(ordinary.get("adaptive_barrier").is_none());
        assert!(ordinary.get("stick_slip").is_none());
        assert!(ordinary.get("self_stick_slip").is_none());

        let mut declared = ordinary.clone();
        declared["self_contact"] = serde_json::to_value(DrapeOptions::default().contact).unwrap();
        declared["adaptive_barrier"] =
            serde_json::to_value(AdaptiveBarrierPolicy::default()).unwrap();
        declared["stick_slip"] = serde_json::json!({
            "mu_s": 0.6,
            "mu_k": 0.3,
            "transition_v_m_s": 0.02
        });
        declared["self_stick_slip"] = serde_json::json!({
            "mu_s": 0.5,
            "mu_k": 0.25,
            "transition_v_m_s": 0.015
        });
        let options: DrapeOptions = serde_json::from_value(declared.clone()).unwrap();
        options.validate().unwrap();
        assert_eq!(serde_json::to_value(options).unwrap(), declared);

        // self_stick_slip without self_contact must refuse fail-closed.
        let mut missing_self = declared.clone();
        missing_self["self_contact"] = serde_json::Value::Null;
        let missing_self_opts: DrapeOptions = serde_json::from_value(missing_self).unwrap();
        assert!(missing_self_opts.validate().is_err());

        // mu_s < mu_k must refuse fail-closed.
        let mut bad_ss = declared.clone();
        bad_ss["stick_slip"]["mu_s"] = serde_json::json!(0.1);
        let bad_ss_opts: DrapeOptions = serde_json::from_value(bad_ss).unwrap();
        assert!(bad_ss_opts.validate().is_err());

        // Unknown fields in adaptive_barrier or stick_slip must be rejected.
        declared["adaptive_barrier"]["extra"] = serde_json::json!(1.0);
        assert!(serde_json::from_value::<DrapeOptions>(declared).is_err());
        assert!(
            serde_json::from_value::<DrapeOptions>(ordinary)
                .unwrap()
                .adaptive_barrier
                .is_none()
        );
    }

    #[test]
    fn mesh_construction_and_self_repulsion_are_opt_in_and_round_trip() {
        let ordinary = serde_json::to_value(DrapeOptions::default()).unwrap();
        assert!(ordinary.get("mesh").is_none());
        assert!(ordinary.get("construction").is_none());
        assert!(ordinary.get("start_self_repulsion").is_none());

        let mut declared = ordinary.clone();
        declared["mesh"] = serde_json::to_value(MeshOptions::adaptive(6.0)).unwrap();
        declared["construction"] =
            serde_json::to_value(ConstructionMechanicsOptions::standard()).unwrap();
        declared["start_self_repulsion"] =
            serde_json::to_value(SelfRepulsionOptions::default()).unwrap();
        let options: DrapeOptions = serde_json::from_value(declared.clone()).unwrap();
        options.validate().unwrap();
        assert_eq!(serde_json::to_value(options).unwrap(), declared);

        // min_spacing_mm > spacing must refuse fail-closed.
        let mut bad_mesh = declared.clone();
        bad_mesh["mesh"]["min_spacing_mm"] = serde_json::json!(20.0);
        let bad_mesh_opts: DrapeOptions = serde_json::from_value(bad_mesh).unwrap();
        assert!(bad_mesh_opts.validate().is_err());

        // hem_mass_multiplier > 1 with hem_width_mm == 0 must refuse fail-closed.
        let mut bad_constr = declared.clone();
        bad_constr["construction"]["hem_width_mm"] = serde_json::json!(0.0);
        let bad_constr_opts: DrapeOptions = serde_json::from_value(bad_constr).unwrap();
        assert!(bad_constr_opts.validate().is_err());

        // period == 0 in start_self_repulsion must refuse fail-closed.
        let mut bad_rep = declared.clone();
        bad_rep["start_self_repulsion"]["period"] = serde_json::json!(0);
        let bad_rep_opts: DrapeOptions = serde_json::from_value(bad_rep).unwrap();
        assert!(bad_rep_opts.validate().is_err());

        // Unknown fields must be rejected fail-closed.
        for key in ["mesh", "construction", "start_self_repulsion"] {
            let mut extra = declared.clone();
            extra[key]["unknown_field"] = serde_json::json!(1);
            assert!(
                serde_json::from_value::<DrapeOptions>(extra).is_err(),
                "{key} must reject unknown fields"
            );
        }
        let round_default: DrapeOptions = serde_json::from_value(ordinary).unwrap();
        assert!(round_default.mesh.is_default());
        assert!(round_default.construction.is_default());
        assert!(round_default.start_self_repulsion.is_none());
    }

    #[test]
    fn scenario_validate_enforces_newton_strategy_work_budgets() {
        let base_scenario = |newton_strategy| crate::study::Scenario {
            schema: "ftx-drape-scenario/1".into(),
            options: DrapeOptions {
                newton_strategy,
                ..DrapeOptions::default()
            },
            fit_spec: serde_json::json!({}),
            max_mesh_points: 512,
            max_vertex_frames: 65_536,
            max_motion_trials: 64,
            surface: None,
            vapor_surface: None,
            coverage: None,
            vapor_coverage: None,
            motion: None,
            material_strain: None,
            contact_loads: None,
        };

        base_scenario(NewtonStrategy::ProjectedNewton)
            .validate()
            .unwrap();
        base_scenario(NewtonStrategy::adaptive_ilu_default())
            .validate()
            .unwrap();
        base_scenario(NewtonStrategy::trust_region_default())
            .validate()
            .unwrap();

        let err = base_scenario(NewtonStrategy::AdaptiveIlu {
            pcg_iters: 257,
            max_shifts: 8,
        })
        .validate()
        .unwrap_err();
        assert_eq!(err.code, ftx_core::RefusalCode::BudgetExceeded);

        let err = base_scenario(NewtonStrategy::AdaptiveIlu {
            pcg_iters: 64,
            max_shifts: 17,
        })
        .validate()
        .unwrap_err();
        assert_eq!(err.code, ftx_core::RefusalCode::OutOfRange);

        let err = base_scenario(NewtonStrategy::TrustRegionSteihaug {
            cg_iters: 4097,
            initial_radius: 0.05,
            max_radius: 0.5,
            min_radius: 1e-12,
        })
        .validate()
        .unwrap_err();
        assert_eq!(err.code, ftx_core::RefusalCode::BudgetExceeded);
    }
}
