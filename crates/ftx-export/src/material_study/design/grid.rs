//! Bounded, deterministic synthesis of linked canonical alterations from an explicit finite grid.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::Refusal;
use serde::{Deserialize, Serialize};

use super::{MAX_DESIGNS, MAX_OPERATIONS, Operation, Variant, budget, invalid};

type GeneratedDesign = (Variant, BTreeMap<String, f64>);

/// One linked slash site. All sites of a variable receive the same declared scalar value.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Site {
    /// Drafted source-piece ID, not a mirrored instance ID.
    pub piece: String,
    /// Canonical `x` width/ease or `y` length direction.
    pub axis: String,
    /// Cut coordinate in the original millimetre drafting frame.
    pub at_mm: f64,
    /// Explicit signed distribution of this variable across its sites; never inferred from a body.
    pub multiplier: f64,
}

/// One scalar design variable with a finite, explicitly ordered domain.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variable {
    /// Unique nonempty identity retained in every generated design.
    pub key: String,
    /// One to 64 finite, strictly increasing millimetre choices. No range is sampled implicitly.
    pub values_mm: Vec<f64>,
    /// Linked sites applied in authored order, after the preceding variable's sites.
    pub sites: Vec<Site>,
}

/// Finite grid search through the ordinary design/material evaluation and comparison authorities.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// One to three variables; the last variable changes fastest during enumeration.
    pub variables: Vec<Variable>,
    /// Explicit canonical operations after every generated sequence, e.g. seam truing.
    #[serde(default)]
    pub after: Vec<Operation>,
    /// One to 31 grid points, excluding the always retained original baseline.
    /// A smaller allowance evaluates a deterministic prefix and reports incomplete search.
    pub max_designs: usize,
}

/// The exact finite search domain and the part actually expanded for production evaluation.
#[derive(Clone, Debug, Serialize)]
pub struct Receipt {
    /// `ftx-design-grid-search/1`.
    pub schema: &'static str,
    /// Complete authored variable/site domain and explicit work allowance.
    pub request: Request,
    /// Cartesian product before any physical/fit feasibility result is known.
    pub total_grid_points: usize,
    /// Generated choices, including choices later refused by geometry, fit or a consumer.
    pub generated_designs: usize,
    /// Unevaluated domain points; none is silently certified infeasible or dominated.
    pub omitted_designs: usize,
    /// Every declared grid point was generated. This is not physical convergence or global proof.
    pub complete_grid_enumeration: bool,
    /// The explicit design allowance stopped enumeration before the domain was exhausted.
    pub budget_exhausted: bool,
    /// Always false: even a complete grid is not a continuous or global design optimum.
    pub global_optimum_claim: bool,
}

impl Request {
    /// Validate the entire domain, including sites and values beyond a truncated prefix.
    pub(super) fn validate(&self) -> Result<(), Refusal> {
        // This admission precedes generation: repeated long keys/sites/after operations must not
        // amplify an arbitrary request into many full documents before the aggregate reservation.
        if serde_json::to_vec(self)
            .map_err(|e| invalid(e.to_string()))?
            .len()
            > 256 * 1024
        {
            return Err(budget(
                "design grid exceeds 256 KiB of authored domain data",
            ));
        }
        if self.variables.is_empty()
            || self.variables.len() > 3
            || !(1..=MAX_DESIGNS).contains(&self.max_designs)
        {
            return Err(invalid(
                "design grid needs 1..3 variables and max_designs in 1..=31",
            ));
        }
        let operations = self
            .variables
            .iter()
            .fold(self.after.len(), |n, v| n.saturating_add(v.sites.len()));
        if operations > MAX_OPERATIONS {
            return Err(budget(
                "design grid exceeds 16 operations per generated design",
            ));
        }
        let mut keys = BTreeSet::new();
        for variable in &self.variables {
            if variable.key.trim().is_empty()
                || !keys.insert(&variable.key)
                || variable.values_mm.is_empty()
                || variable.values_mm.len() > 64
                || variable.values_mm.iter().any(|v| !v.is_finite())
                || variable.values_mm.windows(2).any(|v| v[0] >= v[1])
                || variable.sites.is_empty()
            {
                return Err(invalid(
                    "grid variables need unique nonempty keys, linked sites and 1..64 finite strictly increasing values_mm",
                ));
            }
            for site in &variable.sites {
                if site.piece.trim().is_empty()
                    || !matches!(site.axis.as_str(), "x" | "y")
                    || !site.at_mm.is_finite()
                    || !site.multiplier.is_finite()
                    || site.multiplier == 0.0
                    || variable.values_mm.iter().any(|v| {
                        let spread = v * site.multiplier;
                        !spread.is_finite() || (*v != 0.0 && spread == 0.0)
                    })
                {
                    return Err(invalid(
                        "grid sites need a piece, x/y axis, finite cut and nonzero finite multiplier with finite spreads over the complete domain",
                    ));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn receipt(&self) -> Result<Receipt, Refusal> {
        self.validate()?;
        let total = self
            .variables
            .iter()
            .map(|v| v.values_mm.len())
            .product::<usize>();
        let generated = total.min(self.max_designs);
        Ok(Receipt {
            schema: "ftx-design-grid-search/1",
            request: self.clone(),
            total_grid_points: total,
            generated_designs: generated,
            omitted_designs: total - generated,
            complete_grid_enumeration: generated == total,
            budget_exhausted: generated < total,
            global_optimum_claim: false,
        })
    }

    pub(super) fn generate(&self) -> Result<Vec<GeneratedDesign>, Refusal> {
        let receipt = self.receipt()?;
        let mut designs = Vec::with_capacity(receipt.generated_designs);
        for index in 0..receipt.generated_designs {
            let mut quotient = index;
            let mut indices = vec![0; self.variables.len()];
            for (i, variable) in self.variables.iter().enumerate().rev() {
                indices[i] = quotient % variable.values_mm.len();
                quotient /= variable.values_mm.len();
            }
            let mut values = BTreeMap::new();
            let mut operations = Vec::new();
            for (variable, choice) in self.variables.iter().zip(indices) {
                let value = variable.values_mm[choice];
                values.insert(variable.key.clone(), value);
                operations.extend(variable.sites.iter().map(|site| Operation::SlashSpread {
                    piece: site.piece.clone(),
                    axis: site.axis.clone(),
                    at_mm: site.at_mm,
                    add_mm: value * site.multiplier,
                }));
            }
            operations.extend(self.after.iter().cloned());
            designs.push((
                Variant {
                    key: format!("grid:{index:04}"),
                    operations,
                },
                values,
            ));
        }
        Ok(designs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(max_designs: usize) -> Request {
        serde_json::from_value(json!({"variables":[
            {"key":"ease","values_mm":[0,4,8],"sites":[
                {"piece":"front","axis":"x","at_mm":10,"multiplier":0.5},
                {"piece":"back","axis":"x","at_mm":10,"multiplier":0.5}]},
            {"key":"length","values_mm":[-5,5],"sites":[
                {"piece":"front","axis":"y","at_mm":50,"multiplier":1}]}],
            "max_designs":max_designs}))
        .unwrap()
    }

    #[test]
    fn declared_order_linked_spreads_and_finite_search_scope_are_retained() {
        let request = request(4);
        let generated = request.generate().unwrap();
        assert_eq!(generated.len(), 4);
        let choices: Vec<_> = generated
            .iter()
            .map(|(_, p)| (p["ease"], p["length"]))
            .collect();
        assert_eq!(choices, [(0.0, -5.0), (0.0, 5.0), (4.0, -5.0), (4.0, 5.0)]);
        assert!(matches!(
            generated[2].0.operations[0],
            Operation::SlashSpread { add_mm: 2.0, .. }
        ));
        assert!(matches!(
            generated[2].0.operations[1],
            Operation::SlashSpread { add_mm: 2.0, .. }
        ));
        let receipt = request.receipt().unwrap();
        assert_eq!(receipt.omitted_designs, 2);
        assert!(receipt.budget_exhausted);
        assert!(!receipt.complete_grid_enumeration);
        assert!(!receipt.global_optimum_claim);
        let mut complete = request;
        complete.max_designs = 6;
        assert!(complete.receipt().unwrap().complete_grid_enumeration);
    }

    #[test]
    fn invalid_unvisited_domain_values_cannot_hide_behind_a_small_allowance() {
        let mut input = request(1);
        input.variables[0].values_mm[2] = f64::MAX;
        input.variables[0].sites[0].multiplier = 2.0;
        assert!(input.generate().is_err());
        let mut input = request(1);
        input.variables[0].values_mm = vec![0.0, 0.0];
        assert!(input.generate().is_err());
        let mut input = request(1);
        input.variables[1].key = input.variables[0].key.clone();
        assert!(input.generate().is_err());
        let mut input = request(1);
        input.variables[0].values_mm = vec![1e-300];
        input.variables[0].sites[0].multiplier = 1e-300;
        assert!(input.generate().is_err());
        let mut input = request(1);
        input.variables[0].key = "x".repeat(256 * 1024);
        assert!(input.generate().is_err());
    }
}
