//! Paired apparel specimen-group observations from De Mey et al. (2025), Table 1.
//! The measured plate-to-plate resistance is retained directly. The independent thickness
//! protocol does not establish hotplate contact pressure or justify deriving conductivity.

use std::collections::BTreeMap;

use crate::materials::{Component, Evidence, Form, Material, Quantity, Source};

const ARTICLE: &str = "https://doi.org/10.3390/textiles5040066";

struct Row {
    slug: &'static str,
    figure: &'static str,
    name: &'static str,
    form: Form,
    composition: &'static [(&'static str, f64)],
    construction: &'static str,
    counts: &'static str,
    // Independently transcribed Table 1 means in its original units.
    mass_g_m2: f64,
    thickness_mm: f64,
    resistance_m2_k_w: f64,
}

const ROWS: [Row; 8] = [
    Row {
        slug: "angora-jersey",
        figure: "1a",
        name: "Angora wool single jersey (green)",
        form: Form::Knit,
        composition: &[("angora wool", 1.0)],
        construction: "single jersey",
        counts: "per 1 dm: wale 43; course 67",
        mass_g_m2: 305.0,
        thickness_mm: 1.65,
        resistance_m2_k_w: 0.0413,
    },
    Row {
        slug: "outlast-pet-jersey",
        figure: "1b",
        name: "PET single jersey (source descriptor Outlast)",
        form: Form::Knit,
        composition: &[("polyester", 1.0)],
        construction: "single jersey",
        counts: "per 1 dm: wale 43; course 67",
        mass_g_m2: 180.0,
        thickness_mm: 1.09,
        resistance_m2_k_w: 0.0259,
    },
    Row {
        slug: "cotton-lycra-jersey",
        figure: "1c",
        name: "Cotton 97% / Lycra 3% single jersey (ecru fancy yarn)",
        form: Form::Knit,
        composition: &[("cotton", 0.97), ("elastane", 0.03)],
        construction: "single jersey",
        counts: "per 1 dm: wale 50; course 66",
        mass_g_m2: 255.0,
        thickness_mm: 2.50,
        resistance_m2_k_w: 0.0350,
    },
    Row {
        slug: "cotton-jersey",
        figure: "1d",
        name: "Cotton single jersey (beige)",
        form: Form::Knit,
        composition: &[("cotton", 1.0)],
        construction: "single jersey",
        counts: "per 1 dm: wale 45; course 62",
        mass_g_m2: 225.0,
        thickness_mm: 1.50,
        resistance_m2_k_w: 0.0368,
    },
    Row {
        slug: "linen65-cotton35-woven",
        figure: "1e",
        name: "Linen 65% / cotton 35% complex woven",
        form: Form::Woven,
        composition: &[("linen", 0.65), ("cotton", 0.35)],
        construction: "complex weave; no repeat supplied",
        counts: "per 1 dm, as printed: warp 123 linen + 248 cotton; weft 64 linen, 127 cotton",
        mass_g_m2: 354.0,
        thickness_mm: 0.90,
        resistance_m2_k_w: 0.0164,
    },
    Row {
        slug: "wool70-linen20-cotton10-woven",
        figure: "1f",
        name: "Wool 70% / linen 20% / cotton 10% plain woven",
        form: Form::Woven,
        composition: &[("wool", 0.70), ("linen", 0.20), ("cotton", 0.10)],
        construction: "plain weave",
        counts: "per 1 dm: warp 62 cotton + 30 linen; weft 50 wool + 150 linen",
        mass_g_m2: 559.0,
        thickness_mm: 1.46,
        resistance_m2_k_w: 0.0289,
    },
    Row {
        slug: "linen60-cotton40-woven",
        figure: "1g",
        name: "Linen 60% / cotton 40% complex woven",
        form: Form::Woven,
        composition: &[("linen", 0.60), ("cotton", 0.40)],
        construction: "complex weave; no repeat supplied",
        counts: "per 1 dm: warp 210 cotton + 160 linen; weft 120 cotton + 120 linen",
        mass_g_m2: 400.0,
        thickness_mm: 0.75,
        resistance_m2_k_w: 0.0145,
    },
    Row {
        slug: "cotton70-linen30-chenille",
        figure: "1h",
        name: "Cotton 70% / linen 30% chenille woven",
        form: Form::Woven,
        composition: &[("cotton", 0.70), ("linen", 0.30)],
        construction: "twill 1/2 Z; chenille",
        counts: "per 1 dm, as printed: warp 140 cotton + 140 linen; weft 180 cotton + 50 cotton; duplicate cotton label unresolved",
        mass_g_m2: 466.0,
        thickness_mm: 1.25,
        resistance_m2_k_w: 0.0235,
    },
];

pub(super) fn records() -> Vec<Material> {
    ROWS.iter().map(|row| {
        let specimen = format!("DeMey2025:Figure{}", row.figure);
        let quantity = |value, unit: &str, method: &str| Quantity {
            value, unit: unit.into(), evidence: Evidence::Measured,
            source: "paper".into(), method: method.into(), bounds: None,
            conditions: BTreeMap::from([
                ("specimen".into(), specimen.clone()),
                ("conditioning".into(), "20 +/- 2 degrees C; 65 +/- 5 percent RH; preconditioned under standard textile atmosphere".into()),
                ("uncertainty".into(), "No per-fabric SD, confidence interval or raw replicate values supplied in Table 1".into()),
            ]),
        };
        let mut mass = quantity(row.mass_g_m2, "g/m2", "Table 1; section 3.2: ISO 3801:1977; electronic balance; mean of five independent measurements");
        mass.conditions.extend([
            ("conditioning_duration".into(), "at least 24 h".into()),
            ("coupon_geometry".into(), "100 cm2 (10 x 10 cm)".into()),
            ("balance_specification".into(), "+/- 0.001 g; not a fabric-mass uncertainty interval".into()),
            ("sample_count".into(), "5".into()),
        ]);
        let mut thickness = quantity(row.thickness_mm, "mm", "Table 1; section 3.3: ISO 5084:1996; digital thickness gauge; mean of five selected points");
        thickness.conditions.extend([
            ("pressure_kpa".into(), "0.5".into()),
            ("presser_foot_diameter_mm".into(), "10".into()),
            ("measurement_points".into(), "5 randomly selected points avoiding yarn intersections".into()),
            ("scope".into(), "Thickness at the stated gauge pressure; not unloaded loft or a compression law".into()),
        ]);
        let mut resistance = quantity(row.resistance_m2_k_w, "m2*K/W", "Table 1; section 3.4: ISO 8302:1991; NETZSCH guarded hot plate; mean of three measurements");
        resistance.conditions.extend([
            ("sample_count".into(), "3".into()),
            ("direction".into(), "through thickness".into()),
            ("warm_plate_temperature_c".into(), "35 +/- 0.1".into()),
            ("cold_plate_temperature_c".into(), "25 +/- 0.1".into()),
            ("reported_unit".into(), "m2*degrees C/W; temperature difference, identical scale to m2*K/W".into()),
            ("measurement_pressure".into(), "Not reported for hotplate; 0.5 kPa belongs only to the separate thickness protocol".into()),
            ("contact_treatment".into(), "Reported specimen plate-to-plate effective resistance; interface correction and ply count not reported".into()),
            ("temperature_scope".into(), "Single 35/25 degrees C plate protocol; no measured temperature-response interval".into()),
        ]);
        let mut structure = BTreeMap::from([
            ("specimen".into(), specimen),
            ("source_figure".into(), row.figure.into()),
            ("construction".into(), row.construction.into()),
            ("fabric_counts_reported".into(), row.counts.into()),
            ("identity_scope".into(), "Published fabric group in Table 1 and Figure 1; supplier SKU, lot and coupon identifiers not reported".into()),
            ("composition_scope".into(), "Table 1 reported fiber percentages; not an independent composition assay or a mixture law".into()),
            ("thermal_scope".into(), "Measured effective resistance is usable in the existing constant-property steady comparison; no conductivity, heat capacity, temperature law, contact separation or wearer-comfort model is inferred".into()),
            ("rights".into(), "De Mey et al. (2025), CC BY 4.0; attributed numerical transcription; no source pages, images or participant measurements redistributed".into()),
        ]);
        if row.slug == "outlast-pet-jersey" {
            structure.insert("finish_scope".into(), "Outlast is the source's specimen descriptor, not a current product/SKU or a generic PET property. Phase-change composition, transition range and latent heat are unreported; no PCM response is activated".into());
        }
        if row.slug == "cotton-lycra-jersey" {
            structure.insert("composition_label".into(), "Source Table 1 calls the 3% component Lycra; catalog fiber vocabulary uses elastane".into());
        }
        Material {
            id: format!("demey/2025/{}", row.slug),
            name: format!("De Mey specimen {}: {}", row.figure, row.name),
            form: row.form,
            composition: row.composition.iter().map(|&(fiber, fraction)| Component {
                fiber: fiber.into(), fraction, source: "paper".into(),
            }).collect(),
            composition_complete: true,
            structure,
            properties: BTreeMap::from([
                ("areal_density".into(), mass),
                ("thickness".into(), thickness),
                ("thermal_resistance".into(), resistance),
            ]),
            membrane_response: None, membrane_relaxation: None, thermal_response: None,
            optical_faces: None,
            sources: BTreeMap::from([("paper".into(), Source {
                reference: ARTICLE.into(), accessed_on: "2026-10-19".into(),
                note: "De Mey, Ciesielska-Wrobel, Strakowska, Wiecek, Hertleer and Van Langenhove (2025), Short-Time Transient Thermal Behaviour in Textile Fabrics—The Dual Phase Approach, Textiles 5(4), 66. Table 1 (PDF p5), sections 3.1–3.4 (pp4–7), references 40–42. Version of record published 2025-12-08, CC BY 4.0. Primary PDF https://www.mdpi.com/2673-7248/5/4/66/pdf; table visually checked. Only fabric mass, gauge thickness, reported hotplate resistance and construction are transcribed; no fingertip experiment, human measurement or dual-phase fit is imported.".into(),
            })]),
        }
    }).collect()
}
