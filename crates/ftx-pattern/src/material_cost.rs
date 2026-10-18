//! Comparable, dated supplier quotes evaluated against the same resolved material as the drape.
//! Rates are empirical offer-sample percentiles, not population estimates or executable purchases.
//! No currency conversion, hidden width assumption, shipping estimate or mixing of quality markets.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode, np::percentile};
use serde::{Deserialize, Serialize};

use crate::material_catalog::ResolvedMaterial;

/// Physical sale unit. Item/hide/sample prices require explicit area characterization before import.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaleUnit {
    /// A linear yard at the selected material's sourced width.
    LinearYard,
    /// A linear metre at the selected material's sourced width.
    LinearMeter,
    /// Area in square metres.
    SquareMeter,
    /// Mass in kilograms (loose fill or yarn, for example).
    Kilogram,
}

/// Dimension of the purchase requirement, not an implicit mass-to-area conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// Sheet area (m²).
    Area,
    /// Material mass (kg).
    Mass,
}

/// One observation of an exact supplier offer, including its selling increment and minimum order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quote {
    /// Stable supplier/variant/tier identity. A later date supersedes this same offer.
    pub offer_id: String,
    /// Exact material identity; finished garments cannot stand in for raw fabric.
    pub material_id: String,
    /// Optional exact resolved material snapshot. A same-ID change cannot reuse a locked offer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_sha256: Option<String>,
    /// Supplier identity.
    pub supplier: String,
    /// Exact SKU/variant/tier locator.
    pub sku: String,
    /// Price for `quantity` sale units in the stated currency.
    pub price: f64,
    /// Number of sale units represented by `price`, e.g. 0.5 for a half-yard listing.
    pub quantity: f64,
    /// Dimension of the offered quantity.
    pub unit: SaleUnit,
    /// Minimum quantity in sale units, not a count of packages.
    pub minimum_quantity: f64,
    /// Required ordering increment in sale units.
    pub order_increment: f64,
    /// Three uppercase currency letters. No exchange rate is assumed.
    pub currency: String,
    /// Exact regional market/delivery-region label.
    pub region: String,
    /// User-defined like-for-like quality/condition stratum, not a quality certification.
    pub quality: String,
    /// Explicit tax/shipping/promotion basis. Only identical terms are pooled.
    pub terms: String,
    /// UTC calendar date on which the response was actually retrieved (YYYY-MM-DD).
    pub observed_on: String,
    /// Last eligible UTC calendar date, inclusive, when explicitly bounded by the source.
    /// Absence means no known expiry, not a guarantee of future availability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_through: Option<String>,
    /// Whether that exact offer was available then; latest unavailable does not revive an old price.
    pub available: bool,
    /// Observed finite capacity in this quote's `unit`, NOT packages or pattern-piece area.
    /// Zero means no quantity; absence means the source did not establish available quantity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_quantity: Option<f64>,
    /// Source endpoint or evidence locator.
    pub source: String,
    /// SHA-256 of the retrieved source bytes, for traceability rather than validation.
    pub source_sha256: String,
}

/// Portable quote history; records remain immutable across refreshes.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuoteBook {
    /// `ftx-textile-quotes/1`.
    pub schema: String,
    /// Source observations; multiple dates for an offer are allowed.
    pub quotes: Vec<Quote>,
}

/// All the choices necessary to define a comparable price sample and an order calculation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    /// Currency of every included quote.
    pub currency: String,
    /// Exact market label.
    pub region: String,
    /// Exact quality/condition label.
    pub quality: String,
    /// Exact cost basis.
    pub terms: String,
    /// Reproducible evaluation date; future observations are excluded.
    pub as_of: String,
    /// Maximum observation age in calendar days; stale offers are excluded.
    pub max_age_days: u32,
    /// Requested square metres or kilograms.
    pub requirement: f64,
    /// Dimension of `requirement` and of the returned rate.
    pub basis: Basis,
    /// Exclude offers whose source does not quantify enough stock for the whole rounded order.
    /// False retains legacy price observations without claiming known available stock.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub require_observed_quantity: bool,
}

/// One eligible quote and its actual minimum/increment-rounded merchandise order.
#[derive(Clone, Debug, Serialize)]
pub struct Offer {
    /// Source observation, including the exact date and evidence hash.
    pub quote: Quote,
    /// Currency per m² or kg, before minimum/increment rounding.
    pub rate: f64,
    /// Ordered quantity in this quote's original sale unit.
    pub order_quantity: f64,
    /// Acquired area/mass after rounding, at least the requested amount.
    pub acquired: f64,
    /// Merchandise cost on the quote's stated terms, not a checkout or landed-cost guarantee.
    pub order_cost: f64,
    /// The source supplied enough quantity for this rounded order on its observation date.
    /// False means unknown capacity, never unlimited stock or a failed quantity check.
    pub quantity_sufficiency_observed: bool,
}

/// Why the latest observation in the requested comparable market cannot supply this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    /// The source's inclusive validity window has ended.
    Expired,
    /// Observation exceeds the caller's permitted age.
    Stale,
    /// The source explicitly reports the offer unavailable.
    Unavailable,
    /// A supplied lock belongs to a different resolved material, even if its ID is unchanged.
    MaterialSnapshotMismatch,
    /// The caller requires observed capacity and the source has not supplied it.
    UnknownQuantity,
    /// The minimum/increment-rounded order exceeds the stated capacity.
    InsufficientQuantity,
    /// The sale unit cannot supply the requested area/mass dimension.
    IncompatibleBasis,
}

/// Retained source observation for an excluded order, without reviving an older price.
#[derive(Clone, Debug, Serialize)]
pub struct ExcludedOffer {
    /// Exact source observation and its immutable evidence locator.
    pub quote: Quote,
    /// Stable, machine-readable reason.
    pub reason: ExclusionReason,
    /// Complete rounded quantity in the quote's sale unit, when rejected for insufficient stock.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_order_quantity: Option<f64>,
}

/// A price sample with enough context to reproduce its membership and calculations.
#[derive(Clone, Debug, Serialize)]
pub struct PriceSample {
    /// Snapshot used for width conversion; changing material width changes price/m² automatically.
    pub material_sha256: String,
    /// Complete filtering/quantity choices.
    pub scope: Scope,
    /// Eligible, latest-per-offer observations in stable offer-ID order.
    pub offers: Vec<Offer>,
    /// Ineligible latest observations in the same market, in stable offer-ID order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub excluded_offers: Vec<ExcludedOffer>,
    /// Linear-interpolated p10, median and p90 of offered unit rates; None for an empty sample.
    pub rate_p10_p50_p90: Option<[f64; 3]>,
    /// This statistic summarizes observed offers, not the unknown worldwide market population.
    pub population_claim: bool,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn positive(name: &str, value: f64) -> Result<f64, Refusal> {
    if !value.is_finite() || value <= 0.0 {
        return Err(invalid(format!("{name} must be finite and positive")));
    }
    Ok(value)
}

// The final integer-increment multiplication may round up by one binary ULP (3 * 0.05,
// for example). Admit only that arithmetic rounding at a stock endpoint; do not enlarge
// source inventory by an application-level tolerance or alter the emitted order quantity.
fn within_capacity(order_quantity: f64, available_quantity: f64) -> bool {
    available_quantity > 0.0
        && (order_quantity <= available_quantity
            || order_quantity.next_down() <= available_quantity)
}

// Gregorian ordinal. Restrict the interface to real ISO calendar dates, not lexical approximations.
fn day(s: &str) -> Result<i64, Refusal> {
    let b = s.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || b.iter()
            .enumerate()
            .any(|(i, c)| i != 4 && i != 7 && !c.is_ascii_digit())
    {
        return Err(invalid("quote dates must be YYYY-MM-DD"));
    }
    let y: i64 = s[..4].parse().map_err(|_| invalid("invalid year"))?;
    let m: usize = s[5..7].parse().map_err(|_| invalid("invalid month"))?;
    let d: i64 = s[8..].parse().map_err(|_| invalid("invalid day"))?;
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if y < 1 || !(1..=12).contains(&m) || d < 1 || d > months[m - 1] {
        return Err(invalid("invalid Gregorian date"));
    }
    let prior = y - 1;
    Ok(365 * prior + prior / 4 - prior / 100
        + prior / 400
        + months[..m - 1].iter().sum::<i64>()
        + d)
}

impl Quote {
    /// Validate one source observation without fetching it or asserting it is still offered.
    ///
    /// # Errors
    /// Missing scope/source identifiers, invalid dates, amounts or evidence hash.
    pub fn validate(&self) -> Result<(), Refusal> {
        for s in [
            &self.offer_id,
            &self.material_id,
            &self.supplier,
            &self.sku,
            &self.region,
            &self.quality,
            &self.terms,
            &self.source,
        ] {
            if s.trim().is_empty() {
                return Err(invalid("quote identifiers and scope must not be empty"));
            }
        }
        if self.currency.len() != 3 || !self.currency.bytes().all(|b| b.is_ascii_uppercase()) {
            return Err(invalid("quote currency must be three uppercase letters"));
        }
        if self.source_sha256.len() != 64
            || !self.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(invalid("quote needs the source SHA-256"));
        }
        if let Some(digest) = &self.material_sha256
            && (digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(invalid("quote material lock must be a SHA-256"));
        }
        if let Some(quantity) = self.available_quantity
            && (!quantity.is_finite() || quantity < 0.0)
        {
            return Err(invalid(
                "observed available quantity must be finite and nonnegative in sale units",
            ));
        }
        let observed = day(&self.observed_on)?;
        if let Some(expiry) = &self.valid_through
            && day(expiry)? < observed
        {
            return Err(invalid("quote expired before its observation date"));
        }
        for (name, v) in [
            ("price", self.price),
            ("quantity", self.quantity),
            ("minimum quantity", self.minimum_quantity),
            ("order increment", self.order_increment),
        ] {
            positive(name, v)?;
        }
        Ok(())
    }

    fn offer(&self, material: &ResolvedMaterial, scope: &Scope) -> Result<Option<Offer>, Refusal> {
        let factor = match (self.unit, scope.basis) {
            (SaleUnit::SquareMeter, Basis::Area) | (SaleUnit::Kilogram, Basis::Mass) => 1.0,
            (SaleUnit::LinearYard | SaleUnit::LinearMeter, Basis::Area) => {
                let width =
                    material
                        .material()
                        .require("width", "m", material.allows_estimates())?;
                positive("material width", width)?
                    * if self.unit == SaleUnit::LinearYard {
                        0.9144
                    } else {
                        1.0
                    }
            }
            _ => return Ok(None),
        };
        let rate = positive("unit rate", (self.price / self.quantity) / factor)?;
        let needed = (scope.requirement / factor).max(self.minimum_quantity);
        let order_quantity = positive(
            "order quantity",
            (needed / self.order_increment).ceil() * self.order_increment,
        )?;
        let acquired = positive("acquired quantity", order_quantity * factor)?;
        let order_cost = positive("order cost", order_quantity * (self.price / self.quantity))?;
        if acquired < scope.requirement * (1.0 - 1e-12) {
            return Err(invalid("order arithmetic lost required quantity"));
        }
        Ok(Some(Offer {
            quote: self.clone(),
            rate,
            order_quantity,
            acquired,
            order_cost,
            quantity_sufficiency_observed: self.available_quantity.is_some(),
        }))
    }
}

impl QuoteBook {
    /// Validate the entire history, including stale observations; duplicate day/offer conflicts refuse.
    ///
    /// # Errors
    /// Unsupported schema, invalid record, or conflicting observation for one offer on the same day.
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.schema != "ftx-textile-quotes/1" {
            return Err(invalid("unsupported quote schema"));
        }
        let mut seen = BTreeMap::new();
        let mut identities = BTreeMap::new();
        for q in &self.quotes {
            q.validate()?;
            let identity = (&q.material_id, &q.supplier, &q.sku);
            if let Some(old) = identities.insert(&q.offer_id, identity)
                && old != identity
            {
                return Err(invalid(
                    "an offer ID cannot be rebound to a different material or SKU",
                ));
            }
            let key = (
                &q.offer_id,
                &q.currency,
                &q.region,
                &q.quality,
                &q.terms,
                &q.observed_on,
            );
            if let Some(old) = seen.insert(key, q)
                && old != q
            {
                return Err(invalid(
                    "conflicting observations of one offer on the same day",
                ));
            }
        }
        Ok(())
    }

    /// Filter to a reproducible, like-for-like sample, then use material width for area pricing.
    /// Missing/stale/out-of-stock observations produce an explicitly empty sample, never zero cost.
    ///
    /// # Errors
    /// Invalid history/scope, missing or unapproved width for an eligible linear quote, or overflow.
    pub fn evaluate(
        &self,
        material: &ResolvedMaterial,
        scope: &Scope,
    ) -> Result<PriceSample, Refusal> {
        self.validate()?;
        positive("purchase requirement", scope.requirement)?;
        let as_of = day(&scope.as_of)?;
        if scope.currency.len() != 3
            || !scope.currency.bytes().all(|b| b.is_ascii_uppercase())
            || [&scope.region, &scope.quality, &scope.terms]
                .iter()
                .any(|s| s.trim().is_empty())
        {
            return Err(invalid("invalid price sample scope"));
        }
        let mut latest = BTreeMap::<&str, &Quote>::new();
        for q in &self.quotes {
            if q.material_id != material.authored().id
                || q.currency != scope.currency
                || q.region != scope.region
                || q.quality != scope.quality
                || q.terms != scope.terms
                || day(&q.observed_on)? > as_of
            {
                continue;
            }
            if latest
                .get(q.offer_id.as_str())
                .is_none_or(|old| old.observed_on < q.observed_on)
            {
                latest.insert(&q.offer_id, q);
            }
        }
        let mut offers = Vec::new();
        let mut excluded_offers = Vec::new();
        for q in latest.values() {
            // Apply expiry AFTER selecting the latest observation: expired prices must not revive
            // an older unbounded quote. Do this before width/order arithmetic for ineligible data.
            let reason = if q
                .valid_through
                .as_ref()
                .map(|expiry| day(expiry))
                .transpose()?
                .is_some_and(|expiry| expiry < as_of)
            {
                Some(ExclusionReason::Expired)
            } else if !q.available {
                Some(ExclusionReason::Unavailable)
            } else if as_of - day(&q.observed_on)? > i64::from(scope.max_age_days) {
                Some(ExclusionReason::Stale)
            } else if q
                .material_sha256
                .as_ref()
                .is_some_and(|digest| !digest.eq_ignore_ascii_case(material.sha256()))
            {
                Some(ExclusionReason::MaterialSnapshotMismatch)
            } else if scope.require_observed_quantity && q.available_quantity.is_none() {
                Some(ExclusionReason::UnknownQuantity)
            } else {
                None
            };
            if let Some(reason) = reason {
                excluded_offers.push(ExcludedOffer {
                    quote: (*q).clone(),
                    reason,
                    required_order_quantity: None,
                });
                continue;
            }
            match q.offer(material, scope)? {
                Some(offer)
                    if q.available_quantity
                        .is_none_or(|limit| within_capacity(offer.order_quantity, limit)) =>
                {
                    offers.push(offer);
                }
                offer => excluded_offers.push(ExcludedOffer {
                    quote: (*q).clone(),
                    reason: if offer.is_some() {
                        ExclusionReason::InsufficientQuantity
                    } else {
                        ExclusionReason::IncompatibleBasis
                    },
                    required_order_quantity: offer.map(|o| o.order_quantity),
                }),
            }
        }
        let rates: Vec<f64> = offers.iter().map(|o| o.rate).collect();
        let percentiles =
            (!rates.is_empty()).then(|| [10., 50., 90.].map(|p| percentile(&rates, p)));
        Ok(PriceSample {
            material_sha256: material.sha256().into(),
            scope: scope.clone(),
            offers,
            excluded_offers,
            rate_p10_p50_p90: percentiles,
            population_claim: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{material_catalog::Selection, textile_import::catalog};

    fn material() -> ResolvedMaterial {
        catalog()
            .unwrap()
            .resolve(&Selection {
                id: "fs_il019_natural".into(),
                ..Selection::default()
            })
            .unwrap()
    }
    fn quote() -> Quote {
        Quote {
            offer_id: "supplier/sku/retail".into(),
            material_id: "fs_il019_natural".into(),
            material_sha256: None,
            supplier: "supplier".into(),
            sku: "sku/retail".into(),
            price: 10.,
            quantity: 0.5,
            unit: SaleUnit::LinearYard,
            minimum_quantity: 1.,
            order_increment: 0.5,
            currency: "USD".into(),
            region: "US".into(),
            quality: "first-quality".into(),
            terms: "merchandise-ex-tax-shipping".into(),
            observed_on: "2026-10-13".into(),
            valid_through: None,
            available: true,
            available_quantity: None,
            source: "https://example.org/synthetic".into(),
            source_sha256: "0".repeat(64),
        }
    }
    fn scope() -> Scope {
        Scope {
            currency: "USD".into(),
            region: "US".into(),
            quality: "first-quality".into(),
            terms: "merchandise-ex-tax-shipping".into(),
            as_of: "2026-10-13".into(),
            max_age_days: 7,
            requirement: 2.,
            basis: Basis::Area,
            require_observed_quantity: false,
        }
    }
    fn book(quotes: Vec<Quote>) -> QuoteBook {
        QuoteBook {
            schema: "ftx-textile-quotes/1".into(),
            quotes,
        }
    }

    #[test]
    fn source_expiry_is_inclusive_and_cannot_revive_an_old_price() {
        let mut old = quote();
        old.observed_on = "2026-10-12".into();
        let mut current = quote();
        current.valid_through = Some("2026-10-14".into());
        let history = book(vec![current, old]);
        let mut s = scope();
        s.as_of = "2026-10-14".into();
        assert_eq!(history.evaluate(&material(), &s).unwrap().offers.len(), 1);
        s.as_of = "2026-10-15".into();
        let expired = history.evaluate(&material(), &s).unwrap();
        assert!(expired.offers.is_empty());
        assert!(expired.rate_p10_p50_p90.is_none());
    }

    #[test]
    fn optional_expiry_roundtrips_and_invalid_or_conflicting_dates_refuse() {
        let q = quote();
        let legacy = serde_json::to_value(&q).unwrap();
        assert!(legacy.get("valid_through").is_none());
        assert_eq!(serde_json::from_value::<Quote>(legacy).unwrap(), q);
        for expiry in ["2026-10-12", "2026-02-30", "2026-10-14T12:00:00Z"] {
            let mut invalid = q.clone();
            invalid.valid_through = Some(expiry.into());
            assert!(invalid.validate().is_err());
        }
        let mut bounded = q.clone();
        bounded.valid_through = Some("2026-10-20".into());
        let wire = serde_json::to_string(&bounded).unwrap();
        assert_eq!(serde_json::from_str::<Quote>(&wire).unwrap(), bounded);
        assert!(book(vec![q, bounded]).validate().is_err());
    }

    #[test]
    fn width_half_yard_and_order_increments_are_not_conflated() {
        let m = material();
        let sample = book(vec![quote()]).evaluate(&m, &scope()).unwrap();
        let offer = &sample.offers[0];
        let area_per_yard = 58. * 0.0254 * 0.9144;
        assert!((offer.rate - 20. / area_per_yard).abs() < 1e-12);
        assert_eq!(offer.order_quantity, 1.5);
        assert_eq!(offer.order_cost, 30.);
        assert!(offer.acquired >= 2.);
        let mut selection = m.frozen_selection();
        selection.expected_sha256 = None;
        selection.allow_estimates = true;
        let width = selection
            .record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("width")
            .unwrap();
        width.value *= 2.;
        width.evidence = crate::materials::Evidence::Estimate;
        width.method =
            "Synthetic doubled width for unit-conversion regression, not a supplier claim".into();
        let wider = catalog().unwrap().resolve(&selection).unwrap();
        let wider = book(vec![quote()]).evaluate(&wider, &scope()).unwrap();
        assert_eq!(wider.offers[0].rate, offer.rate / 2.);
        assert_eq!(wider.offers[0].order_cost, 20.);
    }

    #[test]
    fn latest_stock_state_supersedes_old_available_quotes() {
        let mut old = quote();
        old.observed_on = "2026-10-12".into();
        let mut latest = quote();
        latest.available = false;
        assert!(
            book(vec![old, latest])
                .evaluate(&material(), &scope())
                .unwrap()
                .offers
                .is_empty()
        );
    }

    #[test]
    fn legacy_quotes_preserve_serialization_and_unknown_capacity_is_explicit() {
        let q = quote();
        let value = serde_json::to_value(&q).unwrap();
        assert!(value.get("material_sha256").is_none());
        assert!(value.get("available_quantity").is_none());
        assert_eq!(serde_json::from_value::<Quote>(value).unwrap(), q);
        assert!(
            serde_json::to_value(scope())
                .unwrap()
                .get("require_observed_quantity")
                .is_none()
        );
        let history = book(vec![q]);
        let observed = history.evaluate(&material(), &scope()).unwrap();
        assert!(!observed.offers[0].quantity_sufficiency_observed);
        let mut s = scope();
        s.require_observed_quantity = true;
        let strict = history.evaluate(&material(), &s).unwrap();
        assert!(strict.offers.is_empty());
        assert!(strict.rate_p10_p50_p90.is_none());
        assert_eq!(
            strict.excluded_offers[0].reason,
            ExclusionReason::UnknownQuantity
        );
    }

    #[test]
    fn observed_capacity_gates_the_whole_increment_rounded_order_in_sale_units() {
        let mut q = quote();
        q.unit = SaleUnit::SquareMeter;
        q.available_quantity = Some(2.25);
        let mut s = scope();
        s.requirement = 2.1;
        let excluded = book(vec![q.clone()]).evaluate(&material(), &s).unwrap();
        assert!(excluded.offers.is_empty());
        assert_eq!(
            excluded.excluded_offers[0].reason,
            ExclusionReason::InsufficientQuantity
        );
        assert_eq!(
            excluded.excluded_offers[0].required_order_quantity,
            Some(2.5)
        );
        assert_eq!(
            excluded.excluded_offers[0].quote.available_quantity,
            Some(2.25)
        );
        q.available_quantity = Some(2.5);
        let accepted = book(vec![q.clone()]).evaluate(&material(), &s).unwrap();
        assert_eq!(accepted.offers[0].order_quantity, 2.5);
        assert_eq!(accepted.offers[0].order_cost, 50.0);
        assert!(accepted.offers[0].quantity_sufficiency_observed);
        // Minimum quantities count too; net required area is not the merchandise order.
        q.minimum_quantity = 3.0;
        assert!(
            book(vec![q.clone()])
                .evaluate(&material(), &s)
                .unwrap()
                .offers
                .is_empty()
        );
        q.minimum_quantity = 1.0;
        q.available_quantity = Some(0.0);
        assert!(
            book(vec![q.clone()])
                .evaluate(&material(), &s)
                .unwrap()
                .offers
                .is_empty()
        );
        // For yarn/fill pricing the same source field is kg, without an invented areal density.
        q.unit = SaleUnit::Kilogram;
        q.available_quantity = Some(2.5);
        s.basis = Basis::Mass;
        let kg = book(vec![q]).evaluate(&material(), &s).unwrap();
        assert_eq!(kg.offers[0].acquired, 2.5);
    }

    #[test]
    fn latest_insufficient_quantity_or_changed_snapshot_never_revives_old_offer() {
        let m = material();
        let mut old = quote();
        old.observed_on = "2026-10-12".into();
        let mut latest = quote();
        latest.material_sha256 = Some(m.sha256().to_uppercase());
        latest.available_quantity = Some(1.5);
        assert_eq!(
            book(vec![old.clone(), latest.clone()])
                .evaluate(&m, &scope())
                .unwrap()
                .offers
                .len(),
            1
        );
        latest.available_quantity = Some(1.0);
        let limited = book(vec![old.clone(), latest.clone()])
            .evaluate(&m, &scope())
            .unwrap();
        assert!(limited.offers.is_empty());
        assert_eq!(
            limited.excluded_offers[0].reason,
            ExclusionReason::InsufficientQuantity
        );
        latest.available_quantity = Some(10.0);
        latest.material_sha256 = Some("0".repeat(64));
        let mismatched = book(vec![old.clone(), latest.clone()])
            .evaluate(&m, &scope())
            .unwrap();
        assert!(mismatched.offers.is_empty());
        assert_eq!(
            mismatched.excluded_offers[0].reason,
            ExclusionReason::MaterialSnapshotMismatch
        );
        // An explicit historical date still observes the historical source state.
        let mut prior = scope();
        prior.as_of = "2026-10-12".into();
        assert_eq!(
            book(vec![latest, old])
                .evaluate(&m, &prior)
                .unwrap()
                .offers
                .len(),
            1
        );
    }

    #[test]
    fn material_lock_rejects_same_id_width_edits_before_any_price_conversion() {
        let m = material();
        let mut q = quote();
        q.material_sha256 = Some(m.sha256().into());
        let mut selection = m.frozen_selection();
        selection.expected_sha256 = None;
        selection
            .record
            .as_mut()
            .unwrap()
            .properties
            .remove("width");
        let changed = catalog().unwrap().resolve(&selection).unwrap();
        assert_eq!(m.authored().id, changed.authored().id);
        let result = book(vec![q]).evaluate(&changed, &scope()).unwrap();
        assert!(result.offers.is_empty());
        assert_eq!(
            result.excluded_offers[0].reason,
            ExclusionReason::MaterialSnapshotMismatch
        );
    }

    #[test]
    fn malformed_capacity_locks_and_conflicting_stock_observations_refuse() {
        for quantity in [f64::NAN, f64::INFINITY, -1.0] {
            let mut q = quote();
            q.available_quantity = Some(quantity);
            assert!(q.validate().is_err());
        }
        for digest in ["", "invalid", &"g".repeat(64)] {
            let mut q = quote();
            q.material_sha256 = Some(digest.into());
            assert!(q.validate().is_err());
        }
        let mut q = quote();
        q.available_quantity = Some(10.0);
        assert!(book(vec![quote(), q]).validate().is_err());
    }

    #[test]
    fn decimal_increment_endpoint_allows_only_final_multiplication_rounding() {
        assert!(!within_capacity(f64::from_bits(1), 0.0));
        let mut q = quote();
        q.unit = SaleUnit::SquareMeter;
        q.minimum_quantity = 0.05;
        q.order_increment = 0.05;
        q.available_quantity = Some(0.15);
        let mut s = scope();
        s.requirement = 0.14;
        let result = book(vec![q.clone()]).evaluate(&material(), &s).unwrap();
        assert!(result.offers[0].quantity_sufficiency_observed);
        assert_eq!(result.offers[0].order_quantity, 3.0 * 0.05);
        q.available_quantity = Some(0.15 - 1e-12);
        assert!(
            book(vec![q])
                .evaluate(&material(), &s)
                .unwrap()
                .offers
                .is_empty()
        );
    }

    #[test]
    fn sample_is_scoped_deduplicated_and_not_a_market_population() {
        let mut qs = Vec::new();
        for (i, price) in [10., 20., 30.].into_iter().enumerate() {
            let mut q = quote();
            q.offer_id = format!("offer/{i}");
            q.price = price;
            q.unit = SaleUnit::SquareMeter;
            q.quantity = 1.;
            qs.push(q);
        }
        qs.push(qs[0].clone()); // repeated download is not another independent offer
        let mut euro = quote();
        euro.currency = "EUR".into();
        qs.push(euro);
        let mut stale = quote();
        stale.observed_on = "2026-09-01".into();
        stale.offer_id = "stale".into();
        qs.push(stale);
        let mut future = quote();
        future.observed_on = "2026-10-14".into();
        future.offer_id = "future".into();
        qs.push(future);
        let out = book(qs).evaluate(&material(), &scope()).unwrap();
        assert_eq!(out.offers.len(), 3);
        assert_eq!(out.rate_p10_p50_p90, Some([12., 20., 28.]));
        assert!(!out.population_claim);
    }

    #[test]
    fn missing_width_wrong_dimensions_conflicts_and_bad_dates_refuse() {
        for s in [
            "2026-02-29",
            "2026-00-01",
            "2026-13-01",
            "2026-01-00",
            "2026-01-32",
            "0000-01-01",
            "2026-1-01",
            "abcd-01-01",
        ] {
            assert!(day(s).is_err(), "{s}");
        }
        assert_eq!(day("2024-03-01").unwrap() - day("2024-02-28").unwrap(), 2);
        let mut q = quote();
        q.price = 11.;
        assert!(book(vec![quote(), q]).validate().is_err());
        let mut rebound = quote();
        rebound.observed_on = "2026-10-14".into();
        rebound.material_id = "another_variant".into();
        assert!(book(vec![quote(), rebound]).validate().is_err());
        let mut s = scope();
        s.basis = Basis::Mass;
        assert!(
            book(vec![quote()])
                .evaluate(&material(), &s)
                .unwrap()
                .rate_p10_p50_p90
                .is_none()
        );
        let mut selection = material().frozen_selection();
        selection.expected_sha256 = None;
        selection
            .record
            .as_mut()
            .unwrap()
            .properties
            .remove("width");
        let narrow = catalog().unwrap().resolve(&selection).unwrap();
        assert!(book(vec![quote()]).evaluate(&narrow, &scope()).is_err());
    }
}
