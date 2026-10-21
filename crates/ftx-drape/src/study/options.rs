//! Serialization at the scenario boundary, leaving numerical option definitions unchanged.
use crate::{contact::ContactParams, run::DrapeOptions};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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
