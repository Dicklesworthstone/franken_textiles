//! Transfer planner & `KnitDoc` -> lossless CMU Knitout v2 compiler (`FTX-K04`).
//!
//! Compiles validated `ftx-knit/1` documents into machine-profile-adapted `KnitoutProgram`s
//! (branching carrier insertion cleanly on `has_yarn_inserting_hooks`, planning sliderless
//! bed-to-bed decrease/rib transfers within `±max_racking_quarter_pitches`, and verifying every
//! emitted stream through the V-bed VM before sealing `ftx-machine-cert/1`).

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};

use super::ir::{
    Bed, CarrierId, CourseDir, KNITOUT_SCHEMA, KnitDoc, KnitoutHeader, KnitoutOp, KnitoutProgram,
    LoopKind, Needle, StitchNode,
};
use super::profile::{CalibrationSwatchCaps, MachineProfile, WearerAgeClass};
use super::receipts::{FourClaimsVerdict, KnitMachineCert, KnitSwatchQualification};
use super::vm::{KnitSafetyContext, VBedVmReport, verify_knitout_program};

/// Complete verified compilation bundle returned by [`compile_knit_doc`].
#[derive(Clone, Debug)]
pub struct CompiledKnitBundle {
    /// Lossless CMU Knitout v2 AST.
    pub program: KnitoutProgram,
    /// V-bed VM verification report.
    pub vm_report: VBedVmReport,
    /// Sealed `ftx-machine-cert/1` (`hardware_execution_authorized: false`, `physical_knitting_validated: false`).
    pub cert: KnitMachineCert,
}

/// Compiles a [`KnitDoc`] into a verified [`KnitoutProgram`] and sealed [`KnitMachineCert`].
///
/// # Errors
/// Fails closed if qualification is required and missing/mismatched, if target bed width is exceeded,
/// or if any of the 7 V-bed VM safety/child-safety checks fail.
pub fn compile_knit_doc(
    doc: &KnitDoc,
    profile: &MachineProfile,
    qualification: Option<&KnitSwatchQualification>,
    expected_material_sha256: Option<&str>,
    drape_receipt_sha256: Option<String>,
) -> Result<CompiledKnitBundle, Refusal> {
    doc.validate()?;
    profile.validate()?;
    let machine_sha256 = profile.canonical_sha256()?;

    let qual_sha = if doc.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly
        || !profile.requires_calibration_swatch
    {
        if let Some(q) = qualification {
            let mat_sha = expected_material_sha256.unwrap_or(&q.material_sha256);
            q.validate_for_production(mat_sha, &machine_sha256)?;
            Some(q.canonical_sha256()?)
        } else {
            None
        }
    } else {
        let Some(q) = qualification else {
            return Err(Refusal::new(
                RefusalCode::UnqualifiedProcess,
                format!(
                    "production knit compilation for machine '{}' requires a washed KnitSwatchQualification",
                    profile.id
                ),
            )
            .repair("generate and measure a calibration swatch via `ftx knit swatch`, then pass `--qual swatch.json`"));
        };
        let mat_sha = expected_material_sha256.unwrap_or(&q.material_sha256);
        q.validate_for_production(mat_sha, &machine_sha256)?;
        Some(q.canonical_sha256()?)
    };

    // Group stitches deterministically by course
    let mut by_course: BTreeMap<u32, Vec<&StitchNode>> = BTreeMap::new();
    let mut min_wale = i32::MAX;
    let mut max_wale = i32::MIN;
    let mut used_carriers: BTreeSet<CarrierId> = BTreeSet::new();
    for st in &doc.stitches {
        by_course.entry(st.course).or_default().push(st);
        min_wale = min_wale.min(st.wale);
        max_wale = max_wale.max(st.wale);
        for c in &st.carrier_ids {
            used_carriers.insert(c.clone());
        }
    }
    if used_carriers.is_empty() {
        used_carriers.insert(profile.carrier_ids[0].clone());
    }
    let carriers_vec: Vec<CarrierId> = used_carriers.into_iter().collect();

    // Center pattern on the machine bed (`0..needle_count`) without i32 subtraction overflow
    let span_i64 = i64::from(max_wale) - i64::from(min_wale) + 1;
    if span_i64 > i64::from(profile.needle_count) {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "pattern requires {span_i64} needles but machine '{}' has only {}",
                profile.id, profile.needle_count
            ),
        ));
    }
    let span_needles = span_i64 as u32;
    let bed_offset_i64 = i64::from((profile.needle_count - span_needles) / 2) - i64::from(min_wale);
    let map_needle = |bed: Bed, wale: i32| -> Needle {
        Needle::new(bed, (i64::from(wale) + bed_offset_i64) as i32)
    };

    let mut ops: Vec<KnitoutOp> = Vec::new();
    ops.push(KnitoutOp::Comment {
        text: format!("FrankenTextiles ftx-knit/1 -> Knitout v2 ({})", doc.name),
    });

    if profile.min_carrier_spacing > 0 {
        ops.push(KnitoutOp::CarrierSpacing {
            needles: profile.min_carrier_spacing,
        });
    }
    if doc.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
        ops.push(KnitoutOp::SpeedNumber { speed: 150 });
        ops.push(KnitoutOp::Comment {
            text: format!(
                "CalibrationSwatchOnly safety caps: speed <= {:.0}%, takedown <= {:.0}%",
                CalibrationSwatchCaps::MAX_SPEED_FRACTION * 100.0,
                CalibrationSwatchCaps::MAX_TAKEDOWN_FRACTION * 100.0
            ),
        });
    }

    // Bring carriers in cleanly depending on machine hook capability
    if profile.has_yarn_inserting_hooks {
        for c in &carriers_vec {
            ops.push(KnitoutOp::InHook {
                carriers: vec![c.clone()],
            });
        }
    } else {
        for c in &carriers_vec {
            ops.push(KnitoutOp::In {
                carriers: vec![c.clone()],
            });
        }
    }

    // Track current bed occupancy per wale index (`wale -> Bed`) for exact transfer planning
    let mut occupied: BTreeMap<i32, Bed> = BTreeMap::new();
    let mut dir = CourseDir::Plus;
    let mut last_cam_int: Option<i32> = None;
    let mut hooks_released = !profile.has_yarn_inserting_hooks;

    for (&course_idx, nodes) in &by_course {
        let mut sorted: Vec<&StitchNode> = nodes.clone();
        match dir {
            CourseDir::Plus => sorted.sort_by_key(|s| (s.wale, s.bed)),
            CourseDir::Minus => sorted.sort_by_key(|s| (-s.wale, s.bed)),
        }

        // Plan transfers between previous course and current course when allowed
        let can_transfer =
            doc.wearer_age_class != WearerAgeClass::CalibrationSwatchOnly && profile.has_back_bed;
        if can_transfer && course_idx > 0 {
            let target_wales: BTreeMap<i32, Bed> = sorted.iter().map(|s| (s.wale, s.bed)).collect();
            let mut xfer_ops: Vec<KnitoutOp> = Vec::new();
            let mut next_occupied = occupied.clone();

            // 1. Symmetrical edge decreases (outer occupied wale missing from target -> rack +-1 and transfer onto neighbor)
            let prev_wales: Vec<(i32, Bed)> = occupied.iter().map(|(&w, &b)| (w, b)).collect();
            for (pw, pbed) in prev_wales {
                if !target_wales.contains_key(&pw) {
                    let neighbor = if target_wales.contains_key(&(pw + 1)) {
                        Some((pw + 1, 1i32))
                    } else if target_wales.contains_key(&(pw - 1)) {
                        Some((pw - 1, -1i32))
                    } else {
                        None
                    };
                    if let Some((nw, shift)) = neighbor {
                        let opp_bed = pbed.opposite();
                        let src_n = map_needle(pbed, pw);
                        let mid_n = map_needle(opp_bed, pw);
                        let dst_n = map_needle(pbed, nw);
                        // Transfer pbed@pw -> opp_bed@pw (rack 0), then rack by shift and transfer -> pbed@nw
                        // Alignment check: if opp_bed is Back, front_dst == back_src + rack -> rack = nw - pw = shift
                        // If opp_bed is Front, front_src == back_dst + rack -> rack = pw - nw = -shift
                        let rack_pitches = if opp_bed == Bed::Back { shift } else { -shift };
                        if (rack_pitches * 4).abs() <= profile.max_racking_quarter_pitches {
                            xfer_ops.push(KnitoutOp::Rack { quarter_pitches: 0 });
                            xfer_ops.push(KnitoutOp::Xfer {
                                from: src_n,
                                to: mid_n,
                            });
                            xfer_ops.push(KnitoutOp::Rack {
                                quarter_pitches: rack_pitches * 4,
                            });
                            xfer_ops.push(KnitoutOp::Xfer {
                                from: mid_n,
                                to: dst_n,
                            });
                            xfer_ops.push(KnitoutOp::Rack { quarter_pitches: 0 });
                            next_occupied.remove(&pw);
                        }
                    }
                }
            }

            // 2. Bed-to-bed rib/jersey flip transfers at same wale (`f_i <-> b_i` at rack 0)
            for (&tw, &tbed) in &target_wales {
                if let Some(&pbed) = next_occupied.get(&tw)
                    && pbed != tbed
                {
                    xfer_ops.push(KnitoutOp::Rack { quarter_pitches: 0 });
                    xfer_ops.push(KnitoutOp::Xfer {
                        from: map_needle(pbed, tw),
                        to: map_needle(tbed, tw),
                    });
                    next_occupied.insert(tw, tbed);
                }
            }
            ops.extend(xfer_ops);
            occupied = next_occupied;
        }

        // Emit stitch-cam update when setting changes
        if let Some(first) = sorted.first() {
            let cam_int = (first.cam_stitch_setting.setting * 10.0).round() as i32;
            if last_cam_int != Some(cam_int) {
                ops.push(KnitoutOp::Stitch {
                    leading: cam_int,
                    trailing: cam_int,
                });
                last_cam_int = Some(cam_int);
            }
        }

        // Emit course stitches
        for st in &sorted {
            let needle = map_needle(st.bed, st.wale);
            match st.kind {
                LoopKind::CastOn => {
                    // Alternating tuck/knit cast-on (sliderless safe)
                    ops.push(KnitoutOp::Tuck {
                        dir,
                        needle,
                        carriers: st.carrier_ids.clone(),
                    });
                    occupied.insert(st.wale, st.bed);
                }
                LoopKind::Tuck => {
                    ops.push(KnitoutOp::Tuck {
                        dir,
                        needle,
                        carriers: st.carrier_ids.clone(),
                    });
                    occupied.insert(st.wale, st.bed);
                }
                LoopKind::Slip => {
                    if st.carrier_ids.is_empty() {
                        ops.push(KnitoutOp::Amiss { needle });
                    } else {
                        ops.push(KnitoutOp::Miss {
                            dir,
                            needle,
                            carriers: st.carrier_ids.clone(),
                        });
                    }
                }
                LoopKind::BindOff => {
                    ops.push(KnitoutOp::Knit {
                        dir,
                        needle,
                        carriers: st.carrier_ids.clone(),
                    });
                    occupied.insert(st.wale, st.bed);
                }
                LoopKind::Knit
                | LoopKind::Purl
                | LoopKind::Increase
                | LoopKind::Decrease
                | LoopKind::Split
                | LoopKind::Cable => {
                    ops.push(KnitoutOp::Knit {
                        dir,
                        needle,
                        carriers: st.carrier_ids.clone(),
                    });
                    occupied.insert(st.wale, st.bed);
                }
            }
        }

        // Release inserting hooks after initial cast-on course (or course 1)
        if !hooks_released && course_idx >= 1 {
            for c in &carriers_vec {
                ops.push(KnitoutOp::ReleaseHook {
                    carriers: vec![c.clone()],
                });
            }
            hooks_released = true;
        }

        dir = dir.reversed();
    }

    if !hooks_released {
        for c in &carriers_vec {
            ops.push(KnitoutOp::ReleaseHook {
                carriers: vec![c.clone()],
            });
        }
    }

    // Park all carriers cleanly
    if profile.has_yarn_inserting_hooks {
        for c in &carriers_vec {
            ops.push(KnitoutOp::OutHook {
                carriers: vec![c.clone()],
            });
        }
    } else {
        for c in &carriers_vec {
            ops.push(KnitoutOp::Out {
                carriers: vec![c.clone()],
            });
        }
    }

    let program = KnitoutProgram {
        schema: KNITOUT_SCHEMA.to_string(),
        header: KnitoutHeader {
            machine: profile.id.clone(),
            gauge: format!("{:.1}", profile.gauge_e),
            width: profile.needle_count,
            position: "Center".to_string(),
            carriers: carriers_vec,
            extra: BTreeMap::new(),
        },
        ops,
    };

    let ctx = KnitSafetyContext {
        wearer_age_class: doc.wearer_age_class,
        blocked_wale_pitch_mm: doc.gauge.blocked_wale_pitch_mm(),
        source_doc: Some(doc),
        speed_fraction: if doc.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
            Some(CalibrationSwatchCaps::MAX_SPEED_FRACTION)
        } else {
            None
        },
        takedown_fraction: if doc.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
            Some(CalibrationSwatchCaps::MAX_TAKEDOWN_FRACTION)
        } else {
            None
        },
    };
    let vm_report = verify_knitout_program(&program, profile, &ctx)?;

    let mut limitations = Vec::new();
    if profile.backend_kind.is_opaque_proprietary() {
        limitations.push(
            "Tier-B proprietary target: emitted Knitout v2 handoff IR only; vendor software must perform machine binary compilation.".to_string(),
        );
    }
    if !profile.automated_carriage_control {
        limitations.push(
            "Manual-carriage machine: operator advances carriage and performs manual rib/shaping transfers per synchronized checklist.".to_string(),
        );
    }

    let cert = KnitMachineCert::seal(
        vm_report.program_sha256.clone(),
        machine_sha256,
        qual_sha,
        drape_receipt_sha256,
        doc.wearer_age_class,
        profile.automated_carriage_control,
        vm_report.checks_passed.clone(),
        limitations,
        FourClaimsVerdict {
            parity_basis: "knitout_v2_vm_verified".to_string(),
            physics_basis: "discrete_vbed_kinematics_and_safety_bounds".to_string(),
            fit_claimed: false,
            performance_basis: "unbenchmarked".to_string(),
        },
    )?;

    Ok(CompiledKnitBundle {
        program,
        vm_report,
        cert,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knit::draft::{ParametricPanelSpec, draft_calibration_swatch, draft_shaped_panel};
    use crate::knit::ir::GaugeSpec;
    use crate::materials::{Evidence, Source};

    #[test]
    fn calibration_swatch_compiles_for_kniterate_without_inhook_and_passes_vm() {
        let prof = MachineProfile::kniterate_7e();
        let doc = draft_calibration_swatch(&prof, "wool-200tex", 30, 12, &[6.0, 8.0]).unwrap();
        let bundle = compile_knit_doc(&doc, &prof, None, None, None).unwrap();
        let text = bundle.program.to_knitout_text().unwrap();
        assert!(text.contains("\nin 1\n"));
        assert!(!text.contains("inhook"));
        assert!(!bundle.cert.hardware_execution_authorized);
        assert!(!bundle.cert.physical_knitting_validated);
        assert_eq!(bundle.vm_report.total_transfers, 0);
        bundle.cert.verify_integrity().unwrap();
    }

    #[test]
    fn shaped_panel_with_transfers_and_child_gates_compiles_and_enforces_drawstring_ban() {
        let prof = MachineProfile::kniterate_7e();
        let gauge = GaugeSpec {
            wales_per_100mm: 24.0,
            courses_per_100mm: 32.0,
            needle_pitch_mm: prof.needle_pitch_mm,
            yarn_tex: Some(180.0),
            thickness_mm: Some(2.0),
            relaxed_after_wash: true,
        };
        let mut spec = ParametricPanelSpec {
            name: "toddler_panel".to_string(),
            wearer_age_class: WearerAgeClass::Child3To14,
            hem_width_mm: 180.0,
            chest_width_mm: 180.0,
            length_mm: 120.0,
            armhole_depth_mm: 40.0,
            armhole_inset_total_mm: 30.0,
            neck_width_mm: 60.0,
            neck_depth_mm: 20.0,
            rib_hem_courses: 4,
            cam_setting: 8.0,
            yarn_id: "merino-180tex".to_string(),
            carrier_id: CarrierId("1".to_string()),
            has_hood_or_neck_drawstring: false,
        };
        let doc = draft_shaped_panel(&spec, &gauge, &prof).unwrap();
        // Without qualification -> UnqualifiedProcess!
        let err = compile_knit_doc(&doc, &prof, None, None, None).unwrap_err();
        assert_eq!(err.code, RefusalCode::UnqualifiedProcess);

        let qual = KnitSwatchQualification {
            id: "sw-merino".to_string(),
            material_sha256: "mat-123".to_string(),
            machine_sha256: prof.canonical_sha256().unwrap(),
            yarn_lot_id: "lot-1".to_string(),
            was_washed_and_blocked: true,
            wales_per_100mm: 24.0,
            courses_per_100mm: 32.0,
            thickness_mm: 2.0,
            cam_stitch_setting: 8.0,
            wick_dispersion_mm: None,
            shrinkage_axial_pct: None,
            yarn_intake_drift_eps: Some(0.002),
            yarn_intake_drift_sigma_eta: Some(0.015),
            evidence: Evidence::Measured,
            source: Source {
                reference: "local:sw-merino".to_string(),
                accessed_on: "2026-11-27".to_string(),
                note: "washed swatch".to_string(),
            },
            method: "flat dry 24h".to_string(),
            conditioning: BTreeMap::new(),
            allow_estimates: false,
        };
        let bundle = compile_knit_doc(&doc, &prof, Some(&qual), Some("mat-123"), None).unwrap();
        assert!(bundle.vm_report.total_transfers > 0);
        bundle.cert.verify_integrity().unwrap();

        // Drawstring on child garment MUST fail closed!
        spec.has_hood_or_neck_drawstring = true;
        let bad_doc = draft_shaped_panel(&spec, &gauge, &prof).unwrap();
        let err =
            compile_knit_doc(&bad_doc, &prof, Some(&qual), Some("mat-123"), None).unwrap_err();
        assert_eq!(err.code, RefusalCode::MaterialProcessHazard);
    }

    #[test]
    fn shima_profile_uses_inhook_and_releasehook() {
        let prof = MachineProfile::shima_seiki_12e_handoff();
        let doc = draft_calibration_swatch(&prof, "sportswear-68tex", 24, 10, &[20.0]).unwrap();
        let bundle = compile_knit_doc(&doc, &prof, None, None, None).unwrap();
        let txt = bundle.program.to_knitout_text().unwrap();
        assert!(txt.contains("inhook 1\n"));
        assert!(txt.contains("releasehook 1\n"));
        assert!(txt.contains("outhook 1\n"));
    }

    #[test]
    fn compile_validates_optional_swatch_qual_and_handles_extreme_wale_span() {
        let prof = MachineProfile::kniterate_7e();
        let mut doc = draft_calibration_swatch(&prof, "merino-180tex", 12, 4, &[7.0]).unwrap();
        let bad_qual = KnitSwatchQualification {
            id: "bad".to_string(),
            material_sha256: "mat".to_string(),
            machine_sha256: "wrong-machine-hash".to_string(),
            yarn_lot_id: "lot".to_string(),
            was_washed_and_blocked: true,
            wales_per_100mm: 24.0,
            courses_per_100mm: 32.0,
            thickness_mm: 2.0,
            cam_stitch_setting: 7.0,
            wick_dispersion_mm: None,
            shrinkage_axial_pct: None,
            yarn_intake_drift_eps: None,
            yarn_intake_drift_sigma_eta: None,
            evidence: Evidence::Measured,
            source: Source {
                reference: "local:sw".to_string(),
                accessed_on: "2026-11-27".to_string(),
                note: "sw".to_string(),
            },
            method: "flat dry".to_string(),
            conditioning: BTreeMap::new(),
            allow_estimates: false,
        };
        assert_eq!(
            compile_knit_doc(&doc, &prof, Some(&bad_qual), None, None)
                .unwrap_err()
                .code,
            RefusalCode::VerificationMismatch
        );

        // Extreme wale indices must return MachineStateInfeasible cleanly, never overflow i32
        doc.stitches[0].wale = i32::MIN + 1;
        doc.stitches[1].wale = i32::MAX - 1;
        assert_eq!(
            compile_knit_doc(&doc, &prof, None, None, None)
                .unwrap_err()
                .code,
            RefusalCode::MachineStateInfeasible
        );
    }
}
