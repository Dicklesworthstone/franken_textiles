//! Serialization at the scenario boundary, leaving numerical option definitions unchanged.
use crate::{contact::ContactParams, run::DrapeOptions};
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
    elastic_hold_steps: usize,
    step: f64,
    tol: f64,
    compression: f64,
    lag_rounds: [usize; 2],
    elastic_ea: f64,
    contact: ContactParams,
    self_contact: Option<ContactParams>,
    pcg_iters: usize,
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
}
