//! Steady-sliding acquisition to the same source-locked material/interface used by cloth contact.
use ftx_core::Refusal;
use ftx_pattern::material_friction::{Binding, Input, calibrate};
use ftx_pattern::textile_import::catalog;
use serde_json::{Value, json};

use super::{read, simulation_readiness};

pub(super) fn run(path: &str, allow_estimates: bool) -> Result<Value, Refusal> {
    let input: Input = read(path)?;
    let db = catalog()?;
    let calibration = calibrate(&input, &db)?;
    let binding = calibration.binding(&db, allow_estimates)?;
    let readiness = match &binding {
        Binding::Interface {
            materials,
            interface,
        } => {
            let admitted = interface.validate();
            let mut sheets = Vec::new();
            for selection in materials {
                let m = db.resolve(selection)?;
                let ready = simulation_readiness(&m);
                sheets.push(json!({"material_sha256":m.sha256(),"ready":ready.is_ok(),"refusal":ready.err()}));
            }
            json!({"scope":"interface_only", "ready":admitted.is_ok(),"refusal":admitted.err(),
                "sheets":sheets,"whole_garment_checked":false})
        }
        Binding::Material { selection } => {
            let m = db.resolve(selection)?;
            let ready = simulation_readiness(&m);
            json!({"scope":"material_mechanics","ready":ready.is_ok(),"refusal":ready.err(),
                "whole_garment_checked":false})
        }
    };
    Ok(
        json!({"calibration":calibration,"binding":binding,"readiness":readiness,
        "fit_claim":false,"empirical_validation_claim":false,
        "scope":"Effective constant isotropic kinetic coefficient. Not a static-friction calibration, condition-dependent law or runtime load/speed admission check."}),
    )
}
