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

/// Optional cheap canonical geometry/fit screening before the material and scenario cross product.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FitScreening {
    /// One to 256 grid points, excluding the baseline, inspected in authored grid order.
    /// Selection retains the first `max_designs` passing points; every inspected result is retained.
    pub max_candidates: usize,
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
    /// Inspect a larger bounded grid prefix with the explicit selected-body fit contract, then
    /// spend the design/material allowance only on its first passing candidates. No score is inferred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit_screening: Option<FitScreening>,
}

/// One exact canonical screening result before any material substitution or consumer evaluation.
#[derive(Clone, Debug, Serialize)]
pub struct ScreenedDesign {
    /// Full original/altered identity, operations, variables, geometry validation and body-fit result.
    /// There is no material key because flat geometry/fit is evaluated once before the cross product.
    pub design: super::Receipt,
    /// Selected for the ordinary material/consumer cross product; not a downstream feasibility claim.
    pub selected: bool,
    /// Canonical alteration, geometry or fit failure; absent on a passing flat candidate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal: Option<Refusal>,
}

/// What was established by flat screening versus what remains unevaluated by real consumers.
#[derive(Clone, Debug, Serialize)]
pub struct FitScreeningReport {
    /// Grid points inspected once, independently of the number of material choices.
    pub screened_designs: usize,
    /// Inspected points passing the complete canonical geometry and explicit body-fit gates.
    pub qualified_designs: usize,
    /// Inspected points refused by an alteration, geometry or fit gate.
    pub rejected_designs: usize,
    /// Qualified points sent to production consumers, excluding the retained original baseline.
    pub selected_designs: usize,
    /// Qualified points not sent downstream because `max_designs` was reached.
    pub qualified_designs_omitted: usize,
    /// Grid points whose flat fit and downstream feasibility remain unknown.
    pub unvisited_designs: usize,
    /// Every declared grid point was inspected by canonical flat screening.
    pub complete_fit_screening: bool,
    /// Every qualified point in the inspected prefix was sent to production consumers.
    /// This does not certify unvisited points, when `complete_fit_screening` is false.
    pub all_qualified_designs_selected: bool,
    /// Authored-order screening results, retaining failures and unselected passing points alike.
    pub rows: Vec<ScreenedDesign>,
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
    /// Choices sent to the material cross product, including later consumer refusals.
    /// With fit screening, this counts only its selected passing points.
    pub generated_designs: usize,
    /// Domain points not sent to the material cross product. With fit screening, consult its
    /// exact rejected/qualified-omitted/unvisited counts rather than treating every omission alike.
    pub omitted_designs: usize,
    /// Every grid point was sent downstream or explicitly rejected by canonical flat screening.
    /// False when any point is unvisited or flat-qualified but omitted from production consumers.
    /// This is not physical convergence, downstream feasibility or a global optimum proof.
    pub complete_grid_enumeration: bool,
    /// A screening or design allowance left an unvisited or flat-qualified unevaluated point.
    pub budget_exhausted: bool,
    /// Always false: even a complete grid is not a continuous or global design optimum.
    pub global_optimum_claim: bool,
    /// Present only when the larger explicit flat-screening allowance was requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fit_screening: Option<FitScreeningReport>,
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
        if self
            .fit_screening
            .as_ref()
            .is_some_and(|screening| !(1..=256).contains(&screening.max_candidates))
        {
            return Err(invalid(
                "design fit_screening needs max_candidates in 1..=256",
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
            fit_screening: None,
        })
    }

    pub(super) fn generate(&self) -> Result<Vec<GeneratedDesign>, Refusal> {
        let receipt = self.receipt()?;
        let count = self
            .fit_screening
            .as_ref()
            .map_or(receipt.generated_designs, |screening| {
                receipt.total_grid_points.min(screening.max_candidates)
            });
        let mut designs = Vec::with_capacity(count);
        for index in 0..count {
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
