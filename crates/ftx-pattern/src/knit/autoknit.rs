//! Annular source-shell to knit drafting (`FTX-K12`, bounded AutoKnit-style pathway).
//!
//! Intrinsic edge flips preserve the piecewise-flat metric. A harmonic field is
//! checked for a single closed contour through every vertex-level interval, then
//! sampled at uniform coarea mean-normal spacing. Lengths belong to that intrinsic
//! metric, not spatial chords through the flipped shell. This does not solve
//! arbitrary branched garments, local short-row placement, drape, or physical fit.

mod geometry;

use std::collections::BTreeMap;

use ftx_core::receipt::canonical_json_sha256;
use ftx_core::shaping::EaseShellMesh;
use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use super::ir::{
    Bed, CamStitchSetting, CarrierId, CourseDir, GaugeSpec, KNIT_SCHEMA, KnitDoc, KnitPass,
    LoopKind, ParentRef, StitchNode, YarnRun,
};
use super::profile::{MachineProfile, WearerAgeClass};

/// Configuration for 3D-to-knit harmonic AutoKnit drafting.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutoKnitConfig {
    /// Output pattern name.
    pub name: String,
    /// Required wearer age classification (non-defaulting).
    pub wearer_age_class: WearerAgeClass,
    /// Catalog yarn identifier.
    pub yarn_id: String,
    /// Primary yarn carrier identifier.
    pub carrier_id: CarrierId,
    /// Dimensionless stitch cam setting.
    pub cam_setting: f64,
    /// Maximum intrinsic Delaunay edge-flip iterations.
    pub max_idt_flips: usize,
    /// True if garment has a hood/neck drawstring (refused on child garments downstream).
    pub has_hood_or_neck_drawstring: bool,
}

/// One actual course and its intrinsic source-contour quantization residual.
#[derive(Clone, Debug, Serialize)]
pub struct AutoKnitCourse {
    /// Cast-on is course zero; the last course forms a real terminal bind-off.
    pub course: u32,
    /// Solved harmonic coordinate at this contour.
    pub harmonic_time: f64,
    /// Coarea mean-normal distance from the start ring, in mm.
    pub distance_from_start_mm: f64,
    /// Intrinsic piecewise-flat contour length, in mm.
    pub shell_girth_mm: f64,
    /// Total front plus back stitches actually present in the document.
    pub stitch_count: u32,
    /// Actual stitch count times blocked wale pitch minus source contour length.
    pub girth_residual_mm: f64,
}

/// Numerical geometry report; the document is exported separately as pattern.knit.json.
#[derive(Clone, Debug, Serialize)]
pub struct AutoKnitReport {
    /// Drafted topological stitch graph (`ftx-knit/1`).
    #[serde(skip_serializing)]
    pub doc: KnitDoc,
    /// Canonical digest of the entire supplied shell, including its source identity.
    pub source_shell_sha256: String,
    /// Names the geometry coordinate and its intrinsic metric explicitly.
    pub geometry_method: String,
    /// Number of intrinsic Delaunay flips applied to eliminate negative cotangent weights.
    pub idt_flips_applied: usize,
    /// Minimum free-edge cotangent weight, including unmodified numerical roundoff.
    pub min_cotangent_weight: f64,
    /// Integrated coarea mean-normal contour span, in mm; not a geodesic distance.
    pub shell_height_mm: f64,
    /// Number of formed courses after cast-on, including terminal bind-off.
    pub course_count: u32,
    /// Formed course count times blocked course height minus source mean-normal span.
    pub height_residual_mm: f64,
    /// Peak absolute girth quantization residual across all courses, in mm.
    pub max_abs_girth_residual_mm: f64,
    /// Source contours and actual integer stitch counts, including both boundaries.
    pub courses: Vec<AutoKnitCourse>,
}

/// Upper bound on generated AutoKnit courses to prevent unbounded memory allocation on malformed shells.
const MAX_AUTOKNIT_COURSES: u32 = 5_000;
const MAX_AUTOKNIT_STITCHES: usize = 100_000;

/// Drafts an annular tube with exact parent ownership, front/back passes and bind-off.
/// Full qualification and VM admission remain the ordinary compiler/exporter's job.
///
/// # Errors
/// [`RefusalCode::DegenerateGeometry`] for non-annular, intersecting or unsliceable shells,
/// [`RefusalCode::OutOfRange`] or [`RefusalCode::InvalidDocument`] if `cfg` has invalid fields,
/// [`RefusalCode::BudgetExceeded`] if the shell exceeds the course cap,
/// [`RefusalCode::MachineStateInfeasible`] if the circumference or carrier exceeds machine capacity, or
/// [`RefusalCode::NotConverged`] if the harmonic solve fails.
pub fn draft_autoknit_from_shell(
    shell: &EaseShellMesh,
    gauge: &GaugeSpec,
    profile: &MachineProfile,
    cfg: &AutoKnitConfig,
) -> Result<AutoKnitReport, Refusal> {
    gauge.validate()?;
    profile.validate()?;
    if cfg.name.trim().is_empty() || cfg.yarn_id.trim().is_empty() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "AutoKnit config requires non-empty name and yarn_id",
        ));
    }
    CarrierId::new(cfg.carrier_id.as_str())?;
    if !profile.carrier_ids.contains(&cfg.carrier_id) {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "AutoKnit carrier '{}' is not available on machine '{}'",
                cfg.carrier_id.as_str(),
                profile.id
            ),
        ));
    }
    Refusal::finite("cam_setting", cfg.cam_setting)?;
    if cfg.cam_setting < profile.cam_setting_range[0]
        || cfg.cam_setting > profile.cam_setting_range[1]
    {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!(
                "AutoKnit cam setting {} outside machine profile range {:?}",
                cfg.cam_setting, profile.cam_setting_range
            ),
        ));
    }
    if !profile.has_back_bed {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            "an annular AutoKnit shell requires two beds; it cannot be silently flattened",
        ));
    }
    if cfg.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
        return Err(Refusal::new(
            RefusalCode::UnqualifiedProcess,
            "shell shaping and terminal bind-off require production transfers; use the dedicated calibration-swatch drafter",
        ));
    }
    if cfg.max_idt_flips > 100_000 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "AutoKnit max_idt_flips exceeds the bounded limit of 100000",
        ));
    }
    let field = geometry::AnnularField::build(shell, cfg.max_idt_flips)?;
    let source_shell_sha256 = canonical_json_sha256(shell)?;
    let shell_height_mm = field.height_m * 1000.0;

    let course_height_mm = gauge.blocked_course_height_mm();
    let wale_pitch_mm = gauge.blocked_wale_pitch_mm();
    let raw_courses = (shell_height_mm / course_height_mm).round();
    if !raw_courses.is_finite() || raw_courses > f64::from(MAX_AUTOKNIT_COURSES) {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!(
                "AutoKnit shell height {shell_height_mm:.1} mm requires {raw_courses:.0} courses, exceeding cap {MAX_AUTOKNIT_COURSES}"
            ),
        ));
    }
    if raw_courses < 2.0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "AutoKnit shell is too short for one formed course and a terminal bind-off at the selected gauge",
        ));
    }
    let course_count = raw_courses as u32;
    let height_residual_mm = f64::from(course_count) * course_height_mm - shell_height_mm;

    let mut courses = Vec::with_capacity(course_count as usize + 1);
    for course in 0..=course_count {
        let fraction = f64::from(course) / f64::from(course_count);
        let (harmonic_time, perimeter_m) = field.contour(fraction);
        let shell_girth_mm = perimeter_m * 1000.0;
        let per_bed = (shell_girth_mm / (2.0 * wale_pitch_mm)).round();
        if !per_bed.is_finite()
            || per_bed > f64::from(profile.needle_count)
            || per_bed > f64::from(i32::MAX / 2)
        {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                format!(
                    "AutoKnit course {course} girth requires {per_bed:.0} needles per bed, exceeding machine capacity"
                ),
            ));
        }
        if per_bed < 4.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "AutoKnit course {course} is too narrow for an eight-stitch tube at the selected gauge"
                ),
            ));
        }
        courses.push(AutoKnitCourse {
            course,
            harmonic_time,
            distance_from_start_mm: fraction * shell_height_mm,
            shell_girth_mm,
            stitch_count: 2 * per_bed as u32,
            girth_residual_mm: 2.0 * per_bed * wale_pitch_mm - shell_girth_mm,
        });
    }
    // Bind-off consumes one old loop then the same-course chain predecessor.
    // Finish all shaping in the preceding course. Its actual residual is retained,
    // including any one-course anticipation of the terminal contour width.
    let terminal_count = courses[course_count as usize].stitch_count;
    let penultimate = &mut courses[course_count as usize - 1];
    penultimate.stitch_count = terminal_count;
    penultimate.girth_residual_mm =
        f64::from(terminal_count) * wale_pitch_mm - penultimate.shell_girth_mm;
    let mut stitch_budget = 0usize;
    for course in &courses {
        stitch_budget = stitch_budget
            .checked_add(course.stitch_count as usize)
            .ok_or_else(|| {
                Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "AutoKnit stitch count overflow",
                )
            })?;
        if stitch_budget > MAX_AUTOKNIT_STITCHES {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                format!("AutoKnit graph exceeds its {MAX_AUTOKNIT_STITCHES}-stitch budget"),
            ));
        }
    }

    let mut stitches = Vec::with_capacity(stitch_budget);
    let mut passes = Vec::with_capacity(courses.len() * 2);
    let mut prev_front: BTreeMap<i32, u32> = BTreeMap::new();
    let mut prev_back: BTreeMap<i32, u32> = BTreeMap::new();
    let mut next_id = 0u32;
    let max_abs_girth_residual_mm = courses
        .iter()
        .map(|c| c.girth_residual_mm.abs())
        .fold(0.0_f64, f64::max);
    let est_loop_mm = 2.6 * wale_pitch_mm;
    for course in &courses {
        let c = course.course;
        let per_bed = (course.stitch_count / 2) as i32;
        let left = -(per_bed / 2);
        let right = left + per_bed - 1;
        let mut cur_front = BTreeMap::new();
        let mut cur_back = BTreeMap::new();
        let mut bind_chain = None;
        for bed in [Bed::Front, Bed::Back] {
            let (prev_bed, cur_bed) = if bed == Bed::Front {
                (&prev_front, &mut cur_front)
            } else {
                (&prev_back, &mut cur_back)
            };
            let mut wales: Vec<i32> = (left..=right).collect();
            let direction = if bed == Bed::Front {
                CourseDir::Plus
            } else {
                wales.reverse();
                CourseDir::Minus
            };
            let mut stitch_ids = Vec::with_capacity(wales.len());
            for w in wales {
                let mut parents = Vec::new();
                if let Some(&pid) = prev_bed.get(&w) {
                    push_parent(&mut parents, pid, profile)?;
                }
                if w == left {
                    for (&old_wale, &pid) in prev_bed.range(..left).rev() {
                        check_shaping_reach(old_wale, w, profile)?;
                        push_parent(&mut parents, pid, profile)?;
                    }
                }
                if w == right {
                    for (&old_wale, &pid) in prev_bed.range((right + 1)..) {
                        check_shaping_reach(old_wale, w, profile)?;
                        push_parent(&mut parents, pid, profile)?;
                    }
                }
                let kind = if c == course_count {
                    if parents.len() != 1 {
                        return Err(Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            "AutoKnit terminal bind-off requires one retained loop per needle",
                        ));
                    }
                    if let Some(pid) = bind_chain {
                        push_parent(&mut parents, pid, profile)?;
                    }
                    bind_chain = Some(next_id);
                    LoopKind::BindOff
                } else if parents.is_empty() {
                    LoopKind::CastOn
                } else if parents.len() > 1 {
                    LoopKind::Decrease
                } else if bed == Bed::Back {
                    LoopKind::Purl
                } else {
                    LoopKind::Knit
                };
                stitches.push(StitchNode {
                    id: next_id,
                    course: c,
                    wale: w,
                    bed,
                    kind,
                    parents,
                    carrier_ids: vec![cfg.carrier_id.clone()],
                    yarn_id: cfg.yarn_id.clone(),
                    cam_stitch_setting: CamStitchSetting {
                        setting: cfg.cam_setting,
                        machine_family: profile.id.clone(),
                    },
                    yarn_length_mm: Some(est_loop_mm),
                });
                stitch_ids.push(next_id);
                cur_bed.insert(w, next_id);
                next_id += 1;
            }
            passes.push(KnitPass {
                direction,
                stitch_ids,
            });
        }
        prev_front = cur_front;
        prev_back = cur_back;
    }

    let total_yarn_m = stitches.len() as f64 * est_loop_mm / 1000.0;
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "pathway".to_string(),
        "autoknit_annular_intrinsic_harmonic".to_string(),
    );
    // An annulus alone does not identify a body part. Only the source's explicit
    // hem/neck semantics admit torso placement; sleeves, cuffs and generic tubes stay unplaced.
    let is_torso = shell
        .boundary_rings
        .iter()
        .any(|ring| matches!(ring.name.as_str(), "hem" | "waist_hem"))
        && shell.boundary_rings.iter().any(|ring| ring.name == "neck");
    metadata.insert(
        "surface_topology".to_string(),
        if is_torso {
            "torso_tube"
        } else {
            "annular_unplaced"
        }
        .to_string(),
    );
    for ring in &shell.boundary_rings {
        metadata.insert(
            if ring.is_start {
                "source_start_boundary"
            } else {
                "source_end_boundary"
            }
            .to_string(),
            ring.name.clone(),
        );
    }
    // Vertical placement is a separate fact: overlapping source ring heights cannot
    // be guessed as upward or downward torso growth.
    let ring_z_range = |is_start| {
        shell
            .boundary_rings
            .iter()
            .find(|ring| ring.is_start == is_start)
            .unwrap()
            .vertices
            .iter()
            .map(|&vertex| shell.vertices_m[vertex as usize][2])
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| {
                (lo.min(z), hi.max(z))
            })
    };
    let (start_min, start_max) = ring_z_range(true);
    let (end_min, end_max) = ring_z_range(false);
    metadata.insert(
        "course_growth_direction".to_string(),
        if start_max < end_min {
            "upward"
        } else if end_max < start_min {
            "downward"
        } else {
            "unplaced"
        }
        .to_string(),
    );
    metadata.insert(
        "source_body_sha256".to_string(),
        shell.source_body_sha256.clone(),
    );
    metadata.insert("idt_flips_applied".to_string(), field.flips.to_string());
    metadata.insert(
        "shell_height_mm".to_string(),
        format!("{shell_height_mm:.2}"),
    );
    metadata.insert(
        "max_abs_girth_residual_mm".to_string(),
        format!("{max_abs_girth_residual_mm:.2}"),
    );
    metadata.insert(
        "source_shell_sha256".to_string(),
        source_shell_sha256.clone(),
    );
    metadata.insert(
        "geometry_method".to_string(),
        "intrinsic_metric_harmonic_coarea_mean_normal_span".to_string(),
    );
    metadata.insert(
        "height_residual_mm".to_string(),
        format!("{height_residual_mm:+.6}"),
    );
    metadata.insert("construction_scope".to_string(), "One annular tube; no branch synthesis, local short-row correction, drape or physical-fit claim. Widening uses edge cast-ons; the final bind-off loop requires operator tail securing.".to_string());
    metadata.insert(
        "yarn_length_basis".to_string(),
        "Geometric estimate: 2.6 times blocked wale pitch per loop; not measured yarn consumption."
            .to_string(),
    );

    let doc = KnitDoc {
        schema: KNIT_SCHEMA.to_string(),
        name: cfg.name.clone(),
        wearer_age_class: cfg.wearer_age_class,
        gauge: gauge.clone(),
        stitches,
        short_row_turns: vec![],
        passes,
        cable_groups: vec![],
        yarn_runs: vec![YarnRun {
            run_id: 0,
            carrier_id: cfg.carrier_id.clone(),
            yarn_id: cfg.yarn_id.clone(),
            start_course: 0,
            end_course: course_count,
            total_yarn_length_m: total_yarn_m,
            cut_tail_mm: Some(15.0),
        }],
        has_hood_or_neck_drawstring: cfg.has_hood_or_neck_drawstring,
        metadata,
    };
    doc.validate()?;

    Ok(AutoKnitReport {
        doc,
        source_shell_sha256,
        geometry_method: "intrinsic_metric_harmonic_coarea_mean_normal_span".to_string(),
        idt_flips_applied: field.flips,
        min_cotangent_weight: field.min_weight,
        shell_height_mm,
        course_count,
        height_residual_mm,
        max_abs_girth_residual_mm,
        courses,
    })
}

fn push_parent(
    parents: &mut Vec<ParentRef>,
    stitch_id: u32,
    profile: &MachineProfile,
) -> Result<(), Refusal> {
    if parents.len() >= usize::from(profile.max_loops_per_hook) {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            "AutoKnit shaping exceeds the machine's live-loop stack capacity",
        )
        .repair("reduce the source shell taper per course or use a finer course gauge"));
    }
    parents.push(ParentRef {
        stitch_id,
        stack_pos: parents.len() as u8,
    });
    Ok(())
}

fn check_shaping_reach(from: i32, to: i32, profile: &MachineProfile) -> Result<(), Refusal> {
    if (i64::from(from) - i64::from(to)).abs() * 4 > i64::from(profile.max_racking_quarter_pitches)
    {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            "AutoKnit shaping exceeds the machine's transfer racking reach",
        )
        .repair("reduce the source shell taper per course or use a finer course gauge"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knit::{KnitSwatchQualification, compile_knit_doc};
    use crate::materials::{Evidence, Source};
    use ftx_core::shaping::BoundaryRing;
    use ftx_core::{Vec3, det};

    fn frustum(bottom: f64, top: f64, height: f64, n_theta: usize, n_z: usize) -> EaseShellMesh {
        let mut vertices_m = Vec::new();
        for iz in 0..=n_z {
            let u = iz as f64 / n_z as f64;
            let r = bottom * (1.0 - u) + top * u;
            let z = height * u;
            for it in 0..n_theta {
                let ang = 2.0 * std::f64::consts::PI * (it as f64) / (n_theta as f64);
                vertices_m.push([r * det::cos(ang), r * det::sin(ang), z]);
            }
        }
        let mut triangles = Vec::new();
        for iz in 0..n_z {
            for it in 0..n_theta {
                let it_next = (it + 1) % n_theta;
                let a = (iz * n_theta + it) as u32;
                let b = (iz * n_theta + it_next) as u32;
                let c = ((iz + 1) * n_theta + it_next) as u32;
                let d = ((iz + 1) * n_theta + it) as u32;
                triangles.push([a, b, c]);
                triangles.push([a, c, d]);
            }
        }
        let bot_ring: Vec<u32> = (0..n_theta as u32).collect();
        let top_ring: Vec<u32> = ((n_z * n_theta) as u32..((n_z + 1) * n_theta) as u32)
            .rev()
            .collect();
        EaseShellMesh {
            vertices_m,
            triangles,
            boundary_rings: vec![
                BoundaryRing {
                    name: "hem".to_string(),
                    is_start: true,
                    vertices: bot_ring,
                    perimeter_m: 2.0
                        * n_theta as f64
                        * bottom
                        * det::sin(std::f64::consts::PI / n_theta as f64),
                },
                BoundaryRing {
                    name: "neck".to_string(),
                    is_start: false,
                    vertices: top_ring,
                    perimeter_m: 2.0
                        * n_theta as f64
                        * top
                        * det::sin(std::f64::consts::PI / n_theta as f64),
                },
            ],
            source_body_sha256: "synthetic-frustum".to_string(),
        }
    }

    fn settings(prof: &MachineProfile) -> (GaugeSpec, AutoKnitConfig) {
        let gauge = GaugeSpec {
            wales_per_100mm: 24.0,
            courses_per_100mm: 32.0,
            needle_pitch_mm: prof.needle_pitch_mm,
            yarn_tex: Some(100.0),
            thickness_mm: Some(2.0),
            relaxed_after_wash: true,
        };
        let cfg = AutoKnitConfig {
            name: "tapered_tube".to_string(),
            wearer_age_class: WearerAgeClass::Adult,
            yarn_id: "synthetic-yarn-100tex".to_string(),
            carrier_id: CarrierId("1".to_string()),
            cam_setting: 8.0,
            max_idt_flips: 200,
            has_hood_or_neck_drawstring: false,
        };
        (gauge, cfg)
    }

    fn qualification(profile: &MachineProfile, gauge: &GaugeSpec) -> KnitSwatchQualification {
        KnitSwatchQualification {
            id: "synthetic-autoknit-qualification".to_string(),
            material_sha256: "synthetic-autoknit-material".to_string(),
            machine_sha256: profile.canonical_sha256().unwrap(),
            yarn_lot_id: "synthetic-test-lot".to_string(),
            was_washed_and_blocked: true,
            wales_per_100mm: gauge.wales_per_100mm,
            courses_per_100mm: gauge.courses_per_100mm,
            thickness_mm: gauge.thickness_mm.unwrap(),
            cam_stitch_setting: 8.0,
            wick_dispersion_mm: None,
            shrinkage_axial_pct: None,
            yarn_intake_drift_eps: Some(0.0),
            yarn_intake_drift_sigma_eta: Some(0.01),
            evidence: Evidence::Measured,
            source: Source {
                reference: "test:synthetic-annular-autoknit".to_string(),
                accessed_on: "2026-11-28".to_string(),
                note: "Synthetic software-admission fixture, not a physical machine experiment."
                    .to_string(),
            },
            method: "synthetic test values".to_string(),
            conditioning: BTreeMap::new(),
            allow_estimates: false,
        }
    }

    #[test]
    fn autoknit_solves_harmonic_field_and_recovers_tapered_cylinder_girth() {
        let shell = frustum(0.10, 0.07, 0.25, 24, 8);
        let prof = MachineProfile::kniterate_7e();
        let (gauge, cfg) = settings(&prof);
        let rep = draft_autoknit_from_shell(&shell, &gauge, &prof, &cfg).unwrap();
        assert!(rep.min_cotangent_weight >= -1e-6);
        assert!(rep.course_count >= 60);
        assert!(rep.max_abs_girth_residual_mm < 10.0);
        assert!(rep.doc.stitches.len() > 2000);
        let expected_span =
            det::sqrt(0.25_f64.powi(2) + (0.03 * det::cos(std::f64::consts::PI / 24.0)).powi(2))
                * 1000.0;
        assert!(
            (rep.shell_height_mm - expected_span).abs() < 1e-6,
            "{} != {expected_span}",
            rep.shell_height_mm
        );
        for course in &rep.courses {
            let expected = shell.boundary_rings[0].perimeter_m * 1000.0
                + (shell.boundary_rings[1].perimeter_m - shell.boundary_rings[0].perimeter_m)
                    * 1000.0
                    * f64::from(course.course)
                    / f64::from(rep.course_count);
            assert!((course.shell_girth_mm - expected).abs() < 1e-6);
            assert_eq!(
                rep.doc
                    .stitches
                    .iter()
                    .filter(|node| node.course == course.course)
                    .count(),
                course.stitch_count as usize
            );
            assert!(
                (f64::from(course.stitch_count) * gauge.blocked_wale_pitch_mm()
                    - course.shell_girth_mm
                    - course.girth_residual_mm)
                    .abs()
                    < 1e-10
            );
        }
        let qualified = qualification(&prof, &gauge);
        let compiled = compile_knit_doc(
            &rep.doc,
            &prof,
            Some(&qualified),
            Some(&qualified.material_sha256),
            None,
        )
        .unwrap();
        assert!(compiled.vm_report.total_transfers > 0);
        compiled.cert.verify_integrity().unwrap();
        assert!(!compiled.cert.physical_knitting_validated);
        assert!(!compiled.cert.hardware_execution_authorized);
        assert_eq!(
            rep.source_shell_sha256,
            canonical_json_sha256(&shell).unwrap()
        );
        let json = serde_json::to_value(&rep).unwrap();
        assert!(json.get("doc").is_none());
        assert_eq!(json["courses"].as_array().unwrap().len(), rep.courses.len());
    }

    #[test]
    fn autoknit_refuses_oob_boundary_ring_invalid_cfg_and_oversized_shell() {
        let prof = MachineProfile::kniterate_7e();
        let gauge = GaugeSpec {
            wales_per_100mm: 24.0,
            courses_per_100mm: 32.0,
            needle_pitch_mm: prof.needle_pitch_mm,
            yarn_tex: Some(180.0),
            thickness_mm: Some(2.0),
            relaxed_after_wash: true,
        };
        let base_cfg = AutoKnitConfig {
            name: "tube".to_string(),
            wearer_age_class: WearerAgeClass::Adult,
            yarn_id: "merino-180tex".to_string(),
            carrier_id: CarrierId("1".to_string()),
            cam_setting: 8.0,
            max_idt_flips: 20,
            has_hood_or_neck_drawstring: false,
        };
        let mut shell = EaseShellMesh {
            vertices_m: vec![
                [0.1, 0.0, 0.0],
                [-0.05, 0.086, 0.0],
                [-0.05, -0.086, 0.0],
                [0.1, 0.0, 0.2],
                [-0.05, 0.086, 0.2],
                [-0.05, -0.086, 0.2],
            ],
            triangles: vec![
                [0, 1, 4],
                [0, 4, 3],
                [1, 2, 5],
                [1, 5, 4],
                [2, 0, 3],
                [2, 3, 5],
            ],
            boundary_rings: vec![
                BoundaryRing {
                    name: "hem".to_string(),
                    is_start: true,
                    vertices: vec![0, 1, 2],
                    perimeter_m: 0.6,
                },
                BoundaryRing {
                    name: "neck".to_string(),
                    is_start: false,
                    vertices: vec![3, 4, 999], // out-of-bounds vertex index
                    perimeter_m: 0.6,
                },
            ],
            source_body_sha256: "synthetic".to_string(),
        };
        assert_eq!(
            draft_autoknit_from_shell(&shell, &gauge, &prof, &base_cfg)
                .unwrap_err()
                .code,
            RefusalCode::DegenerateGeometry
        );

        // Fix boundary ring and test invalid cam_setting & unknown carrier
        shell.boundary_rings[1].vertices = vec![5, 4, 3];
        for ring in &mut shell.boundary_rings {
            ring.perimeter_m = (0..ring.vertices.len())
                .map(|i| {
                    let a = shell.vertices_m[ring.vertices[i] as usize];
                    let b = shell.vertices_m[ring.vertices[(i + 1) % ring.vertices.len()] as usize];
                    (Vec3::new(a[0], a[1], a[2]) - Vec3::new(b[0], b[1], b[2])).norm()
                })
                .sum();
        }
        let mut bad_cam = base_cfg.clone();
        bad_cam.cam_setting = f64::NAN;
        assert_eq!(
            draft_autoknit_from_shell(&shell, &gauge, &prof, &bad_cam)
                .unwrap_err()
                .code,
            RefusalCode::NonFinite
        );
        let mut bad_carrier = base_cfg.clone();
        bad_carrier.carrier_id = CarrierId("99".to_string());
        assert_eq!(
            draft_autoknit_from_shell(&shell, &gauge, &prof, &bad_carrier)
                .unwrap_err()
                .code,
            RefusalCode::MachineStateInfeasible
        );

        // Unreasonably tall shell (100 meters) refuses with BudgetExceeded instead of OOM
        for v in &mut shell.vertices_m[3..6] {
            v[2] = 100.0;
        }
        assert_eq!(
            draft_autoknit_from_shell(&shell, &gauge, &prof, &base_cfg)
                .unwrap_err()
                .code,
            RefusalCode::BudgetExceeded
        );
    }

    #[test]
    fn cylinder_height_is_independent_of_rotation_and_axial_tessellation() {
        let profile = MachineProfile::kniterate_7e();
        let (gauge, cfg) = settings(&profile);
        for axial_divisions in [1, 6] {
            let shell = frustum(0.03, 0.03, 0.12, 12, axial_divisions);
            let first = draft_autoknit_from_shell(&shell, &gauge, &profile, &cfg).unwrap();
            let mut rotated = shell.clone();
            for position in &mut rotated.vertices_m {
                let [x, y, z] = *position;
                *position = [z + 0.4, y - 0.7, -x + 0.2];
            }
            let second = draft_autoknit_from_shell(&rotated, &gauge, &profile, &cfg).unwrap();
            assert_eq!(first.doc.metadata["surface_topology"], "torso_tube");
            assert_eq!(first.doc.metadata["course_growth_direction"], "upward");
            assert_eq!(second.doc.metadata["course_growth_direction"], "unplaced");
            let mut reversed = shell.clone();
            for ring in &mut reversed.boundary_rings {
                ring.is_start = !ring.is_start;
            }
            let backwards = draft_autoknit_from_shell(&reversed, &gauge, &profile, &cfg).unwrap();
            assert_eq!(
                backwards.doc.metadata["course_growth_direction"],
                "downward"
            );
            let mut sleeve = shell.clone();
            sleeve.boundary_rings[0].name = "left_wrist".to_string();
            sleeve.boundary_rings[1].name = "left_armhole".to_string();
            let sleeve_report = draft_autoknit_from_shell(&sleeve, &gauge, &profile, &cfg).unwrap();
            assert_eq!(
                sleeve_report.doc.metadata["surface_topology"],
                "annular_unplaced"
            );
            assert_eq!(
                sleeve_report.doc.metadata["source_start_boundary"],
                "left_wrist"
            );
            for report in [&first, &second] {
                assert!((report.shell_height_mm - 120.0).abs() < 1e-8);
                assert_eq!(report.course_count, 38);
                assert!((report.height_residual_mm + 1.25).abs() < 1e-8);
                assert_eq!(report.courses[0].harmonic_time, 0.0);
                assert_eq!(report.courses.last().unwrap().harmonic_time, 1.0);
                for course in &report.courses {
                    assert!(
                        (course.shell_girth_mm - shell.boundary_rings[0].perimeter_m * 1000.0)
                            .abs()
                            < 1e-8
                    );
                }
            }
            assert_eq!(first.doc.passes, second.doc.passes);
        }
    }

    #[test]
    fn tapered_and_widening_shells_compile_with_exact_live_parent_conservation() {
        for profile in [
            MachineProfile::kniterate_7e(),
            MachineProfile::shima_seiki_12e_handoff(),
        ] {
            let (gauge, cfg) = settings(&profile);
            let qualified = qualification(&profile, &gauge);
            for (bottom, top) in [(0.04, 0.025), (0.025, 0.04)] {
                let shell = frustum(bottom, top, 0.12, 12, 4);
                let report = draft_autoknit_from_shell(&shell, &gauge, &profile, &cfg).unwrap();
                let mut live = std::collections::BTreeSet::new();
                for node in &report.doc.stitches {
                    for parent in &node.parents {
                        assert!(
                            live.remove(&parent.stitch_id),
                            "parent {} was lost or reused",
                            parent.stitch_id
                        );
                    }
                    assert!(live.insert(node.id));
                }
                assert_eq!(
                    live.len(),
                    1,
                    "only the final tail-securing loop may remain"
                );
                assert!(
                    report
                        .doc
                        .stitches
                        .iter()
                        .filter(|node| node.course == 0)
                        .all(|node| node.kind == LoopKind::CastOn)
                );
                assert!(
                    report
                        .doc
                        .stitches
                        .iter()
                        .filter(|node| node.course == report.course_count)
                        .all(|node| node.kind == LoopKind::BindOff)
                );
                if top > bottom {
                    assert!(
                        report
                            .doc
                            .stitches
                            .iter()
                            .any(|node| node.course > 0 && node.kind == LoopKind::CastOn)
                    );
                } else {
                    assert!(
                        report
                            .doc
                            .stitches
                            .iter()
                            .any(|node| node.kind == LoopKind::Decrease)
                    );
                }
                for pair in report.doc.passes.as_chunks::<2>().0 {
                    assert_eq!(pair[0].direction, CourseDir::Plus);
                    assert_eq!(pair[1].direction, CourseDir::Minus);
                }
                assert_eq!(
                    compile_knit_doc(&report.doc, &profile, None, None, None)
                        .unwrap_err()
                        .code,
                    RefusalCode::UnqualifiedProcess
                );
                let compiled = compile_knit_doc(
                    &report.doc,
                    &profile,
                    Some(&qualified),
                    Some(&qualified.material_sha256),
                    None,
                )
                .unwrap();
                assert!(compiled.vm_report.total_transfers > 0);
                compiled.cert.verify_integrity().unwrap();
                assert!(!compiled.cert.hardware_execution_authorized);
            }
        }
    }

    #[test]
    fn malformed_branched_and_unrepresentable_shells_refuse_without_substitution() {
        let profile = MachineProfile::kniterate_7e();
        let (gauge, cfg) = settings(&profile);
        let source = frustum(0.03, 0.03, 0.12, 12, 4);
        for mutation in 0..5 {
            let mut shell = source.clone();
            match mutation {
                0 => shell.boundary_rings.push(shell.boundary_rings[1].clone()),
                1 => shell.boundary_rings[1].vertices.reverse(),
                2 => shell.boundary_rings[0].perimeter_m += 0.001,
                3 => {
                    shell.vertices_m.push([0.0, 0.0, 0.06]);
                }
                4 => {
                    shell.triangles.pop();
                }
                _ => unreachable!(),
            }
            let refusal = draft_autoknit_from_shell(&shell, &gauge, &profile, &cfg).unwrap_err();
            assert_eq!(refusal.code, RefusalCode::DegenerateGeometry);
        }
        let mut one_bed = profile.clone();
        one_bed.has_back_bed = false;
        assert_eq!(
            draft_autoknit_from_shell(&source, &gauge, &one_bed, &cfg)
                .unwrap_err()
                .code,
            RefusalCode::MachineStateInfeasible
        );
        let too_short = frustum(0.03, 0.03, 0.002, 12, 1);
        assert_eq!(
            draft_autoknit_from_shell(&too_short, &gauge, &profile, &cfg)
                .unwrap_err()
                .code,
            RefusalCode::OutOfRange
        );
        let mut too_large = source.clone();
        too_large.vertices_m.resize(20_001, [0.0, 0.0, 0.0]);
        assert_eq!(
            draft_autoknit_from_shell(&too_large, &gauge, &profile, &cfg)
                .unwrap_err()
                .code,
            RefusalCode::BudgetExceeded
        );
        let mut invalid_config = serde_json::to_value(&cfg).unwrap();
        invalid_config["silently_enable_branches"] = serde_json::json!(true);
        assert!(serde_json::from_value::<AutoKnitConfig>(invalid_config).is_err());
    }
}
