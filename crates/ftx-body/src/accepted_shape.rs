//! Recover the shared Anny shape of an accepted multi-frame fit without fitting it again.
//! This checks the saved receipt's internal consistency, not its authenticity or real-world fit.

use std::collections::BTreeSet;

use ftx_core::{Refusal, RefusalCode};
use serde_json::{Value, json};

use crate::{BodyParams, Phenotype, age_parameter_for_years};

/// Fixed shared shape, in the converted model's parameter order. Image-frame poses are not reused.
#[derive(Clone, Debug)]
pub struct AcceptedShape {
    /// Full phenotype and local changes, including fixed/unselected assumptions.
    pub params: BodyParams,
    /// Fixed age from the fitting configuration, not inferred from shape.
    pub years: f64,
    /// Digest of the original image-fitting request, retained as provenance.
    pub input_sha256: String,
}

fn bad(message: &str) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
        .repair("use an intact ACCEPTED report from ftx measure fit-body and its exact model file")
}

fn finite(v: &Value) -> Result<f64, Refusal> {
    v.as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| bad("body-fit shape receipt needs finite numbers"))
}

fn array(v: &Value) -> Result<&Vec<Value>, Refusal> {
    v.as_array()
        .ok_or_else(|| bad("body-fit shape receipt has a missing or malformed array"))
}

fn digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn set(params: &mut BodyParams, labels: &[String], name: &str, value: f64) -> Result<(), Refusal> {
    let target = match name {
        "height" => Some(&mut params.phenotype.height),
        "weight" => Some(&mut params.phenotype.weight),
        "muscle" => Some(&mut params.phenotype.muscle),
        "proportions" => Some(&mut params.phenotype.proportions),
        _ => None,
    };
    if let Some(target) = target {
        if !(0.0..=1.0).contains(&value) {
            return Err(bad("body-fit phenotype would require clamping"));
        }
        *target = value;
    } else {
        let i = labels
            .iter()
            .position(|label| label == name)
            .ok_or_else(|| bad("body-fit shape names a local change absent from this model"))?;
        if !(-1.0..=1.0).contains(&value) {
            return Err(bad("body-fit local change is outside its authored range"));
        }
        params.local_changes[i] = value;
    }
    Ok(())
}

impl AcceptedShape {
    /// Read `ftx-body-fit/1` from `ftx measure fit-body`. The caller supplies the SHA-256 and labels
    /// from the SAME verified model bytes it will evaluate. No file access, solver, or evidence
    /// fusion occurs here. A CLI envelope is not a report and is deliberately rejected.
    ///
    /// Reconstruct all parameters from the fixed configuration and accepted shape vector, then
    /// compare against the saved expanded parameters. Never trust an unlabelled vector alone.
    /// All starts/checks must have passed; rejected, incomplete, or contradictory receipts refuse.
    /// This does not rerun residuals or prove that a hand-edited report came from the fitter.
    pub fn read(report: &Value, model_sha256: &str, labels: &[String]) -> Result<Self, Refusal> {
        let config = &report["anny_configuration"];
        let solution = &report["solution"];
        if report["schema"] != "ftx-body-fit/1"
            || report["status"] != "ACCEPTED"
            || !solution.is_object()
            || !digest(model_sha256)
            || config["model_sha256"].as_str() != Some(model_sha256)
            || report["model"]["model_id"] != format!("anny/0.6.1/sha256:{model_sha256}")
        {
            return Err(bad(
                "body-fit is not an accepted Anny shape for this exact model",
            ));
        }
        let input_sha256 = report["input_sha256"]
            .as_str()
            .filter(|s| digest(s))
            .ok_or_else(|| bad("body-fit request digest is missing or malformed"))?;
        if labels.len() > 4096 || labels.iter().collect::<BTreeSet<_>>().len() != labels.len() {
            return Err(bad("model local-change labels must be unique and bounded"));
        }
        let years = finite(&config["age_years"])?;
        let gender = finite(&config["gender"])?;
        if !(0.0..=110.0).contains(&years) || !(0.0..=1.0).contains(&gender) {
            return Err(bad("body-fit age or gender is outside the modelled range"));
        }
        let mut params = BodyParams {
            phenotype: Phenotype {
                age: age_parameter_for_years(years),
                gender,
                ..Phenotype::default()
            },
            local_changes: vec![0.0; labels.len()],
        };
        let fixed = config["fixed_shape"]
            .as_object()
            .ok_or_else(|| bad("body-fit fixed-shape configuration is missing"))?;
        for (name, v) in fixed {
            set(&mut params, labels, name, finite(v)?)?;
        }
        let shape = array(&solution["shape"])?;
        let variables = array(&config["shape_parameters"])?;
        if !(1..=12).contains(&shape.len())
            || shape.len() != variables.len()
            || report["model"]["shape"] != config["shape_parameters"]
        {
            return Err(bad("body-fit shape vector and named layout disagree"));
        }
        let mut seen = BTreeSet::new();
        for (v, p) in shape.iter().zip(variables) {
            let name = p["name"]
                .as_str()
                .ok_or_else(|| bad("unnamed shape parameter"))?;
            let (lo, hi, value) = (finite(&p["lower"])?, finite(&p["upper"])?, finite(v)?);
            let min = if ["height", "weight", "muscle", "proportions"].contains(&name) {
                0.0
            } else {
                -1.0
            };
            if !seen.insert(name)
                || fixed.contains_key(name)
                || lo < min
                || hi > 1.0
                || !(lo < value && value < hi)
            {
                return Err(bad(
                    "body-fit shape is duplicated, fixed twice, or outside its declared bounds",
                ));
            }
            set(&mut params, labels, name, value)?;
        }
        let p = params.phenotype;
        let expanded = json!({"provenance":"MODEL_FIT", "phenotype":{
            "gender":p.gender,"age":p.age,"muscle":p.muscle,"weight":p.weight,
            "height":p.height,"proportions":p.proportions},"local_changes":params.local_changes});
        if solution["anny_parameters"] != expanded {
            return Err(bad(
                "body-fit expanded parameters do not reproduce its named shared shape",
            ));
        }
        let attempts = array(&report["attempts"])?;
        if !(2..=4).contains(&attempts.len())
            || array(&report["input"]["starts"])?.len() != attempts.len()
            || !(2..=8).contains(&array(&report["input"]["frames"])?.len())
        {
            return Err(bad(
                "body-fit is missing its multi-frame/multistart evidence",
            ));
        }
        for attempt in attempts {
            if attempt["status"] != "CONVERGED" || !array(&attempt["flags"])?.is_empty() {
                return Err(bad("body-fit contains a failed initialization"));
            }
            let checks = array(&attempt["checks"])?;
            if checks.is_empty() {
                return Err(bad("body-fit is missing residual checks"));
            }
            for check in checks {
                let (residual, limit) = (finite(&check["residual"])?, finite(&check["limit"])?);
                if check["passed"] != true
                    || residual < 0.0
                    || limit <= 0.0
                    || residual > limit
                    || !matches!(check["unit"].as_str(), Some("px" | "m"))
                    || (check["unit"] == "px" && limit != 3.0)
                {
                    return Err(bad("body-fit contains a failed or invalid residual check"));
                }
            }
        }
        let spread = array(&report["shape_spread_fraction"])?;
        let limit = finite(&report["options"]["max_shape_spread_fraction"])?;
        if !(0.0..=0.01).contains(&limit) || limit == 0.0 || spread.len() != shape.len() {
            return Err(bad(
                "body-fit shared-shape agreement policy is missing or weakened",
            ));
        }
        for v in spread {
            if !(0.0..=limit).contains(&finite(v)?) {
                return Err(bad("body-fit shared shapes do not agree across starts"));
            }
        }
        Ok(Self {
            params,
            years,
            input_sha256: input_sha256.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt() -> Value {
        let hash = "a".repeat(64);
        let parameters = json!([{"name":"height","lower":0.0,"upper":1.0},
            {"name":"belly","lower":-1.0,"upper":1.0}]);
        let attempt = json!({"status":"CONVERGED","flags":[],
            "checks":[{"unit":"px","residual":0.4,"limit":3.0,"passed":true}]});
        json!({"schema":"ftx-body-fit/1","status":"ACCEPTED","input_sha256":"b".repeat(64),
            "input":{"starts":[{},{}],"frames":[{},{}]},"attempts":[attempt,attempt],
            "options":{"max_shape_spread_fraction":0.01},"shape_spread_fraction":[0.0,0.002],
            "model":{"model_id":format!("anny/0.6.1/sha256:{hash}"),"shape":parameters},
            "anny_configuration":{"model_sha256":hash,"age_years":2.5,"gender":1.0,
                "fixed_shape":{"muscle":0.25},"shape_parameters":parameters},
            "solution":{"shape":[0.6,-0.2],"anny_parameters":{"provenance":"MODEL_FIT",
                "phenotype":{"gender":1.0,"age":age_parameter_for_years(2.5),"height":0.6,
                    "weight":0.5,"muscle":0.25,"proportions":0.5},"local_changes":[-0.2,0.0]}}})
    }
    fn read(v: &Value) -> Result<AcceptedShape, Refusal> {
        AcceptedShape::read(v, &"a".repeat(64), &["belly".into(), "head".into()])
    }
    #[test]
    fn shared_shape_is_reconstructed_without_rounding_or_refitting() {
        let v = receipt();
        let accepted = read(&v).unwrap();
        assert_eq!(accepted.params.phenotype.height, 0.6);
        assert_eq!(accepted.params.phenotype.muscle, 0.25);
        assert_eq!(accepted.params.local_changes, [-0.2, 0.0]);
        assert_eq!(accepted.years, 2.5);
        assert_eq!(accepted.input_sha256, "b".repeat(64));
        assert_eq!(v, receipt());
    }
    #[test]
    fn rejected_missing_or_contradictory_evidence_never_yields_a_shape() {
        for (pointer, value) in [
            ("/status", json!("AMBIGUOUS")),
            ("/solution", Value::Null),
            ("/input_sha256", json!("bad")),
            ("/attempts", json!([])),
            ("/attempts/1/status", json!("BUDGET_STOP")),
            ("/attempts/0/checks/0/passed", json!(false)),
            ("/attempts/0/checks/0/residual", json!(3.1)),
            ("/attempts/0/checks/0/limit", json!(300.0)),
            ("/shape_spread_fraction/0", json!(0.02)),
            ("/options/max_shape_spread_fraction", json!(1.0)),
            ("/solution/shape/0", json!(1.0)),
            ("/solution/anny_parameters/local_changes/0", json!(0.2)),
            (
                "/solution/anny_parameters/provenance",
                json!("MEASURED_TAPE"),
            ),
            ("/anny_configuration/age_years", json!(5.0)),
            ("/anny_configuration/fixed_shape/age", json!(0.5)),
        ] {
            let mut v = receipt();
            if pointer.ends_with("/age") {
                v["anny_configuration"]["fixed_shape"]["age"] = value;
            } else {
                *v.pointer_mut(pointer).unwrap() = value;
            }
            assert!(read(&v).is_err(), "accepted {pointer}");
        }
    }
    #[test]
    fn wrong_model_label_order_and_duplicate_definitions_refuse() {
        let v = receipt();
        assert!(
            AcceptedShape::read(&v, &"c".repeat(64), &["belly".into(), "head".into()]).is_err()
        );
        assert!(
            AcceptedShape::read(&v, &"a".repeat(64), &["head".into(), "belly".into()]).is_err()
        );
        assert!(
            AcceptedShape::read(&v, &"a".repeat(64), &["belly".into(), "belly".into()]).is_err()
        );
        let mut v = v;
        v["anny_configuration"]["fixed_shape"]["height"] = json!(0.6);
        assert!(read(&v).is_err());
        assert!(read(&json!({"result":receipt()})).is_err());
    }
}
