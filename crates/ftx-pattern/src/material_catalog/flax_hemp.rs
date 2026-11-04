//! Construction-specific flax/hemp observations from Vasile et al. (2024), Tables 1, 2 and 4.
//! Gauge thickness and source-calculated bulk density do not qualify contact or unloaded clearance.

use std::collections::BTreeMap;

use crate::materials::{Component, Evidence, Form, Material, Quantity, Source};

const ARTICLE: &str = "https://doi.org/10.3390/ma17071650";
const PDF: &str = "https://pdfs.semanticscholar.org/f845/0aecca9c9b78efa94ff19a3e44389e3d522e.pdf";
const PDF_SHA256: &str = "823e79cdf4ebf902bfd4443524361817f9ca5610b041cf4b65ae3937a363849b";
const CONDITIONING: &str = "24 h at 20 +/- 2 degrees C and 65 +/- 4 percent RH; ISO 139:2005";

struct Row {
    slug: &'static str,
    label: &'static str,
    weave: &'static str,
    hemp_weft: bool,
    // Table 2 means and source-labelled standard deviations, in the original units.
    mass_g_m2: [f64; 2],
    thickness_mm: [f64; 2],
    density_kg_m3: [f64; 2],
    abrasion_cycles: f64,
}

const ROWS: [Row; 6] = [
    Row {
        slug: "hemp-plain",
        label: "HEMP PLAIN",
        weave: "plain 1/1",
        hemp_weft: true,
        mass_g_m2: [193.2, 2.1],
        thickness_mm: [0.52, 0.01],
        density_kg_m3: [375.2, 10.6],
        abrasion_cycles: 5000.0,
    },
    Row {
        slug: "hemp-pan",
        label: "HEMP PAN",
        weave: "panama 2/2",
        hemp_weft: true,
        mass_g_m2: [181.3, 2.8],
        thickness_mm: [0.54, 0.01],
        density_kg_m3: [338.4, 9.7],
        abrasion_cycles: 4000.0,
    },
    Row {
        slug: "hemp-twill",
        label: "HEMP TWILL",
        weave: "twill 2/2",
        hemp_weft: true,
        mass_g_m2: [184.0, 3.2],
        thickness_mm: [0.55, 0.01],
        density_kg_m3: [333.4, 8.6],
        abrasion_cycles: 6000.0,
    },
    Row {
        slug: "flax-plain",
        label: "FLAX PLAIN",
        weave: "plain 1/1",
        hemp_weft: false,
        mass_g_m2: [199.7, 1.7],
        thickness_mm: [0.51, 0.01],
        density_kg_m3: [392.0, 10.8],
        abrasion_cycles: 5000.0,
    },
    Row {
        slug: "flax-pan",
        label: "FLAX PAN",
        weave: "panama 2/2",
        hemp_weft: false,
        mass_g_m2: [188.9, 1.9],
        thickness_mm: [0.55, 0.02],
        density_kg_m3: [346.2, 9.7],
        abrasion_cycles: 4000.0,
    },
    Row {
        slug: "flax-twill",
        label: "FLAX TWILL",
        weave: "twill 2/2",
        hemp_weft: false,
        mass_g_m2: [188.9, 2.6],
        thickness_mm: [0.54, 0.01],
        density_kg_m3: [350.1, 5.9],
        abrasion_cycles: 6000.0,
    },
];

fn spread(quantity: &mut Quantity, sd: f64) {
    quantity.conditions.insert(
        "reported_standard_deviation".into(),
        format!("{sd} {}", quantity.unit),
    );
    quantity.conditions.insert(
        "variability_scope".into(),
        "Table 2 standard deviation in original units; not hard bounds, uncertainty of the mean or a confidence interval".into(),
    );
}

fn record(row: &Row) -> Material {
    let specimen = format!("Vasile2024:{}", row.label);
    let q = |value, unit: &str, evidence, table: &str, method: &str| Quantity {
        value,
        unit: unit.into(),
        evidence,
        source: "paper".into(),
        method: method.into(),
        bounds: None,
        conditions: BTreeMap::from([
            ("specimen".into(), specimen.clone()),
            ("conditioning".into(), CONDITIONING.into()),
            ("source_table".into(), table.into()),
            ("source_row".into(), row.label.into()),
        ]),
    };
    let mut mass = q(
        row.mass_g_m2[0],
        "g/m2",
        Evidence::Measured,
        "2",
        "ISO 3801:1977; section 2.2.1; mean mass per area of ten 100 cm2 square specimens",
    );
    spread(&mut mass, row.mass_g_m2[1]);
    mass.conditions.extend([
        ("sample_count".into(), "10".into()),
        ("coupon_area_cm2".into(), "100".into()),
        ("balance_accuracy_g".into(), "0.001; instrument specification, not a material tolerance".into()),
        ("mass_basis".into(), "Conditioned fabric mass; not an oven-dry mass or moisture-content measurement".into()),
        ("rounding_scope".into(), "Table 2 decimals retained; methods prose says nearest gram, not used to re-round the table".into()),
    ]);
    let mut thickness = q(
        row.thickness_mm[0],
        "mm",
        Evidence::Measured,
        "2",
        "ISO 5084:1996; section 2.2.1; Hess HDM-3 gauge; mean of ten measurements at 1 kPa",
    );
    spread(&mut thickness, row.thickness_mm[1]);
    thickness.conditions.extend([
        ("sample_count".into(), "10".into()),
        ("pressure_pa".into(), "1000".into()),
        ("gauge_accuracy_mm".into(), "0.01; instrument specification, not a material tolerance".into()),
        ("rounding_scope".into(), "Table 2 decimals retained; methods prose says nearest 0.1 mm, not used to re-round the table".into()),
        ("scope".into(), "Reported compressed gauge thickness only; no contact-thickness qualification, unloaded clearance or pressure-response law".into()),
    ]);
    let mut density = q(
        row.density_kg_m3[0],
        "kg/m3",
        Evidence::Published,
        "2",
        "Section 2.2.1 source-calculated fabric bulk density from areal mass and gauge thickness; Table 2 result retained without recomputing from rounded means",
    );
    spread(&mut density, row.density_kg_m3[1]);
    density.conditions.extend([
        ("pressure_pa".into(), "1000".into()),
        ("density_basis".into(), "Fabric bulk density at the source thickness protocol; not constituent-fiber density or an independent density acquisition".into()),
    ]);
    let mut abrasion = q(
        row.abrasion_cycles,
        "cycles",
        Evidence::Measured,
        "4",
        "ISO 12947-2:2016; sections 2.2.2 and 3.2.4; Martindale rubbing cycles at observed specimen breakdown",
    );
    abrasion.conditions.extend([
        ("test_standard".into(), "ISO 12947-2:2016".into()),
        ("pressure_pa".into(), "12000".into()),
        ("sample_count".into(), "4".into()),
        ("inspection_interval_cycles".into(), "1000".into()),
        ("test_limit_cycles".into(), "6000".into()),
        ("failure_criterion".into(), "Complete breakdown of the first two yarns".into()),
        ("variability_scope".into(), "One reported result per fabric; individual trial values and SD not supplied. Inspection-cycle count is not a continuous failure time, lifetime guarantee or uncertainty bound".into()),
    ]);
    let mut warp = q(
        24.0,
        "1/cm",
        Evidence::Published,
        "1",
        "Section 2.1 and Table 1: loom warp construction, 24 ends/cm; not an independent post-weaving count measurement",
    );
    warp.conditions.insert("direction".into(), "warp".into());
    let mut weft = q(
        23.0,
        "1/cm",
        Evidence::Published,
        "1",
        "Table 1: weft construction, 23 picks/cm; not an independent post-weaving count measurement",
    );
    weft.conditions.insert("direction".into(), "weft".into());
    Material {
        id: format!("vasile/2024/{}", row.slug),
        name: format!("Vasile {}: {} {}", row.label,
            if row.hemp_weft { "hemp-weft/flax-warp linen" } else { "flax linen" }, row.weave),
        form: Form::Woven,
        composition: if row.hemp_weft { Vec::new() } else {
            vec![Component { fiber: "flax (linen)".into(), fraction: 1.0, source: "paper".into() }]
        },
        composition_complete: !row.hemp_weft,
        structure: BTreeMap::from([
            ("specimen".into(), specimen),
            ("source_label".into(), row.label.into()),
            ("construction".into(), row.weave.into()),
            ("warp_yarn".into(), "Bleached bio-flax, Nm 26/1".into()),
            ("weft_yarn".into(), if row.hemp_weft { "Bleached hemp, Nm 26/1" } else { "Ecru bio-flax, Nm 24/1" }.into()),
            ("composition_scope".into(), if row.hemp_weft {
                "Flax warp/hemp weft identified; section 2.2.1 assumes 50:50 for its porosity calculation. Exact fiber mass fractions are unmeasured and remain absent"
            } else {
                "Both yarn directions identified as flax in section 2.1/Table 1; source-described fiber composition, not an independent composition assay"
            }.into()),
            ("identity_scope".into(), "Study-specific woven specimen group; no supplier SKU, purchase width, lot or individual coupon identity supplied. The 300 cm loom width is not usable fabric width".into()),
            ("source_pdf".into(), PDF.into()),
            ("source_pdf_sha256".into(), PDF_SHA256.into()),
            ("source_tables".into(), "Tables 1, 2 and 4, PDF pages 3, 6 and 10; visually verified 2026-11-03".into()),
            ("physical_scope".into(), "Mass and test-qualified screening observations; no membrane, bending, interface-friction, thermal, vapor, wet-state or manufacturing law inferred".into()),
        ]),
        properties: BTreeMap::from([
            ("areal_density".into(), mass),
            ("reported_thickness".into(), thickness),
            ("bulk_density".into(), density),
            ("thread_density_warp".into(), warp),
            ("thread_density_weft".into(), weft),
            ("abrasion_cycles".into(), abrasion),
        ]),
        membrane_response: None,
        membrane_relaxation: None,
        thermal_response: None,
        optical_faces: None,
        sources: BTreeMap::from([("paper".into(), Source {
            reference: ARTICLE.into(), accessed_on: "2026-11-03".into(),
            note: "Simona Vasile, Sofie Vermeire, Katrien Vandepitte, Veronique Troch and Alexandra De Raeve (2024), Effect of Weave and Weft Type on Mechanical and Comfort Properties of Hemp-Linen Fabrics, Materials 17(7):1650, published 2024-04-03. Sections 2.1-2.2 and 3.2.4, Tables 1, 2 and 4. CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/); attributed numerical transcription and explicit SI normalization. Primary published PDF mirrored by Semantic Scholar, digest retained in structure. No source images or PDF redistributed; no author endorsement or new empirical validation asserted.".into(),
        })]),
    }
}

pub(super) fn records() -> Vec<Material> {
    ROWS.iter().map(record).collect()
}
