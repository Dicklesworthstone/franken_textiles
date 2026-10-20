//! Authored quality/placement regions reuse the finite-stock polygon authority.
//! Names are caller-defined constraints, never inferred material laws or quality certificates.
use std::collections::BTreeSet;

use ftx_core::{Refusal, Vec2};
use ftx_pattern::materials::Source;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Options, Stock, Work, bad, checked, enclosed, ring};
use crate::Affine;

/// An admissible placement region within one physical stock item.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Zone {
    /// Local identity matched exactly by `Options::piece_zones`; no intrinsic quality meaning.
    pub id: String,
    /// Simple polygon in the stock's authored mm frame (marker mm frame in a computed plan).
    /// Strictly inside stock; may overlap other regions and contain excluded defects or old cuts.
    pub outline_mm: Vec<Vec2>,
    /// Source/rationale for this region and its claimed placement suitability.
    pub geometry_source: Source,
}

pub(super) fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

pub(super) fn validate_assignments(options: &Options, pieces: &[Value]) -> Result<(), Refusal> {
    let ids: BTreeSet<_> = pieces.iter().filter_map(|p| p["id"].as_str()).collect();
    for (piece, zone) in &options.piece_zones {
        if !ids.contains(piece.as_str()) || zone.trim().is_empty() {
            return Err(bad(
                "finite-stock piece_zones must name existing source pieces and nonempty zones",
            ));
        }
    }
    if options.zone_margin_mm > 0.0 && options.piece_zones.is_empty() {
        return Err(bad(
            "zone_margin_mm needs an explicit piece_zones assignment",
        ));
    }
    Ok(())
}

pub(super) fn compile(
    stock: &Stock,
    transform: &Affine,
    outline: &[Vec2],
    work: &mut Work,
) -> Result<Vec<Zone>, Refusal> {
    let mut ids = BTreeSet::new();
    let mut zones = Vec::with_capacity(stock.zones.len());
    for zone in &stock.zones {
        if zone.id.trim().is_empty() || !ids.insert(zone.id.as_str()) {
            return Err(bad(
                "stock zone IDs must be nonempty and unique within each stock",
            ));
        }
        let source = &zone.geometry_source;
        if source.reference.trim().is_empty()
            || source.accessed_on.trim().is_empty()
            || source.note.trim().is_empty()
        {
            return Err(bad(
                "stock zones need source, date and characterization note",
            ));
        }
        let path: Vec<_> = zone
            .outline_mm
            .iter()
            .map(|p| transform.apply(*p))
            .collect();
        ring(&path, work)?;
        if !checked(enclosed(&path, outline, 0.0, work))? {
            return Err(bad(
                "stock zones must lie strictly inside the stock outline",
            ));
        }
        zones.push(Zone {
            id: zone.id.clone(),
            outline_mm: path,
            geometry_source: source.clone(),
        });
    }
    Ok(zones)
}
