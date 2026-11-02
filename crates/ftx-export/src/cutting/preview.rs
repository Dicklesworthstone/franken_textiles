//! Source-locked roll preview in millimetres. Groups distinguish outlines, drills and annotations;
//! this is not device code or permission to execute an unqualified cutting process.

use ftx_core::{Refusal, RefusalCode, Vec2};

use super::{CutPlan, program};
use crate::{fnum, xml_escape};

fn reversal_attribute(reversed: bool) -> &'static str {
    if reversed {
        " data-grain-reversed=\"true\""
    } else {
        ""
    }
}

fn path(points: &[Vec2], closed: bool) -> String {
    let mut d = String::new();
    for (i, p) in points.iter().enumerate() {
        d.push_str(&format!(
            "{}{} {} ",
            if i == 0 { 'M' } else { 'L' },
            fnum(p.x, 6),
            fnum(p.y, 6)
        ));
    }
    if closed {
        d.push('Z');
    }
    d
}

struct RoutedPreview<'a> {
    marker: &'a program::Marker,
    machine: &'a program::Machine,
    machine_sha256: &'a str,
    fixtures: Option<&'a program::FixtureClearance>,
}

fn routed_preview<'a>(
    report: Option<&'a program::Report>,
    policy: Option<&'a program::Policy>,
    index: usize,
    placements: &[super::CutPlacement],
) -> Result<Option<RoutedPreview<'a>>, Refusal> {
    let Some(report) = report.filter(|r| {
        r.travel_routing.is_some() || !r.stock_loading.is_empty() || r.drilling.is_some()
    }) else {
        return Ok(None);
    };
    let bad = || Refusal::new(RefusalCode::InvalidDocument, "invalid routed knife preview");
    let policy = policy.ok_or_else(bad)?;
    let machine = &policy.machine;
    let fixtures = machine.fixture_clearance.as_ref();
    let mut markers = report.markers.iter().filter(|m| m.marker == index);
    let marker = markers.next().ok_or_else(bad)?;
    if markers.next().is_some()
        || report.travel_routing != policy.travel_routing
        || report.stock_loading != policy.stock_loading
        || report.drilling != machine.drilling
        || (report.travel_routing.is_some() && fixtures.is_none())
        || report.machine_sha256 != program::machine_sha256(machine)?
        || machine.bed_mm.iter().any(|v| !v.is_finite() || *v <= 0.)
    {
        return Err(bad());
    }
    let mut requests = policy
        .stock_loading
        .iter()
        .filter(|loading| loading.marker == index);
    let requested = requests.next();
    if requests.next().is_some()
        || marker.loading.as_ref().map(|loading| loading.lower_left_mm)
            != requested.map(|loading| loading.lower_left_mm)
    {
        return Err(bad());
    }
    if let Some(loading) = &marker.loading {
        let [xmin, ymin, xmax, ymax] = loading.stock_bounds_mm;
        if loading.stock_bounds_mm.iter().any(|v| !v.is_finite())
            || !loading.marker_to_machine_offset_mm.is_finite()
            || Vec2::new(xmin, ymin) != loading.lower_left_mm
            || xmin < 0.
            || ymin < 0.
            || xmax <= xmin
            || ymax <= ymin
            || xmax > machine.bed_mm[0]
            || ymax > machine.bed_mm[1]
            || marker.finite_stock.as_ref().is_some_and(|stock| {
                stock.marker_to_machine_offset_mm != loading.marker_to_machine_offset_mm
            })
        {
            return Err(bad());
        }
    }
    let route = &marker.selected;
    let points = route
        .runs
        .iter()
        .fold(route.return_waypoints_mm.len() + 2, |n, run| {
            n.saturating_add(run.travel_waypoints_mm.len().saturating_add(2))
        });
    let points = route.drills.iter().fold(points, |n, drill| {
        n.saturating_add(drill.travel_waypoints_mm.len().saturating_add(3))
    });
    if points > 1_000_000 {
        return Err(preview_budget());
    }
    route.checked_travel_length()?;
    if route
        .drills
        .iter()
        .flat_map(program::DrillRun::travel_points)
        .chain(route.runs.iter().flat_map(program::Run::travel_points))
        .chain(route.return_points())
        .any(|p| p.x < 0. || p.y < 0. || p.x > machine.bed_mm[0] || p.y > machine.bed_mm[1])
    {
        return Err(bad());
    }
    let offset = marker.loading.as_ref().map_or_else(
        || {
            marker
                .finite_stock
                .as_ref()
                .map_or(Vec2::ZERO, |stock| stock.marker_to_machine_offset_mm)
        },
        |loading| loading.marker_to_machine_offset_mm,
    );
    let mut expected_drills = placements.iter().enumerate().flat_map(|(i, placement)| {
        placement
            .drills_mm
            .iter()
            .enumerate()
            .map(move |(j, drill)| (i, placement, j, drill))
    });
    if route.drills.is_empty() != marker.drill_qualification.is_none() {
        return Err(bad());
    }
    if let Some(id) = &marker.drill_qualification {
        let mut observations = policy
            .qualifications
            .iter()
            .filter(|q| q.id == marker.qualification);
        let qualification = observations.next().ok_or_else(bad)?;
        if observations.next().is_some()
            || qualification.machine_sha256 != report.machine_sha256
            || qualification.material_sha256 != marker.material_sha256
            || qualification.drilling.as_ref().is_none_or(|q| &q.id != id)
        {
            return Err(bad());
        }
    }
    for drill in &route.drills {
        let Some((placement_index, placement, drill_index, (center, diameter))) =
            expected_drills.next()
        else {
            return Err(bad());
        };
        let tool = report.drilling.as_ref().ok_or_else(bad)?;
        let radius = drill.diameter_mm / 2.;
        if drill.placement != placement_index
            || drill.drill != drill_index
            || drill.piece != placement.piece
            || drill.instance != placement.instance
            || drill.copy != placement.copy
            || drill.center_mm != *center + offset
            || drill.diameter_mm != *diameter
            || drill.diameter_mm != tool.diameter_mm
            || !drill.center_mm.is_finite()
            || !radius.is_finite()
            || radius <= 0.
            || drill.center_mm.x - radius < 0.
            || drill.center_mm.y - radius < 0.
            || drill.center_mm.x + radius > machine.bed_mm[0]
            || drill.center_mm.y + radius > machine.bed_mm[1]
        {
            return Err(bad());
        }
    }
    if expected_drills.next().is_some() {
        return Err(bad());
    }
    Ok(Some(RoutedPreview {
        marker,
        machine,
        machine_sha256: &report.machine_sha256,
        fixtures,
    }))
}

fn knife_overlay(preview: &RoutedPreview<'_>) -> String {
    let mut out = String::from(
        "<g id=\"knife-fixtures\" fill=\"#fce4dd\" fill-opacity=\"0.6\" stroke=\"#9b3525\"><title>Qualified stationary fixture footprints; not cutting paths</title>\n",
    );
    for fixture in preview
        .fixtures
        .into_iter()
        .flat_map(|clearance| &clearance.fixtures)
    {
        out.push_str(&format!(
            "<path data-fixture=\"{}\" d=\"{}\"/>\n",
            xml_escape(&fixture.id),
            path(&fixture.outline_mm, true)
        ));
    }
    out.push_str("</g>\n<g id=\"knife-raised-travel\" fill=\"none\" stroke=\"#185cbb\" stroke-dasharray=\"1 1\"><title>Complete selected raised-tool centreline, including checked detours and final return; no hardware execution authorization</title>\n");
    for (i, drill) in preview.marker.selected.drills.iter().enumerate() {
        out.push_str(&format!(
            "<path data-drill-run=\"{i}\" data-phase=\"drill-raised-travel\" d=\"{}\"/>\n",
            path(&drill.travel_points().collect::<Vec<_>>(), false)
        ));
    }
    for (i, run) in preview.marker.selected.runs.iter().enumerate() {
        out.push_str(&format!(
            "<path data-run=\"{i}\" data-phase=\"raised-travel\" d=\"{}\"/>\n",
            path(&run.travel_points().collect::<Vec<_>>(), false)
        ));
    }
    if preview.marker.selected.return_mm.is_some() {
        out.push_str(&format!(
            "<path data-phase=\"raised-return\" d=\"{}\"/>\n",
            path(
                &preview.marker.selected.return_points().collect::<Vec<_>>(),
                false
            )
        ));
    }
    out.push_str("</g>\n");
    if let (Some(tool), Some(qualification)) = (
        &preview.machine.drilling,
        &preview.marker.drill_qualification,
    ) {
        out.push_str(&format!(
            "<g id=\"qualified-drills\" data-tool=\"{}\" data-qualification=\"{}\" data-machine-sha256=\"{}\" fill=\"#a67ac7\" fill-opacity=\"0.45\" stroke=\"#6d328d\"><title>Source-qualified through-drill operations before knife cutting; not circular knife paths</title>\n",
            xml_escape(&tool.tool), xml_escape(qualification), xml_escape(preview.machine_sha256)
        ));
        for drill in &preview.marker.selected.drills {
            out.push_str(&format!(
                "<circle class=\"qualified-drill\" data-placement=\"{}\" data-drill=\"{}\" data-piece=\"{}\" data-instance=\"{}\" data-copy=\"{}\" cx=\"{}\" cy=\"{}\" r=\"{}\"/>\n",
                drill.placement, drill.drill, xml_escape(&drill.piece), xml_escape(&drill.instance),
                drill.copy, fnum(drill.center_mm.x, 6), fnum(drill.center_mm.y, 6),
                fnum(drill.diameter_mm / 2., 6)
            ));
        }
        out.push_str("</g>\n");
    }
    out
}

/// Render one marker with exactly its planned stock dimensions and six-decimal millimetre paths.
/// The input is the computed cut plan, not a new geometry pass or a separately re-resolved material.
/// It contains no script, external resources or machine-specific settings. All labels are escaped.
///
/// # Errors
/// Missing roll index, invalid public geometry fields or dimensions after caller-side mutation.
pub fn svg(plan: &CutPlan, roll_index: usize) -> Result<String, Refusal> {
    let bad = || {
        Refusal::new(
            RefusalCode::InvalidDocument,
            "invalid roll preview geometry",
        )
    };
    let roll = plan.rolls.get(roll_index).ok_or_else(bad)?;
    let w = roll.width_m * 1000.0;
    let h = roll.length_m * 1000.0;
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
        return Err(bad());
    }
    let routed = routed_preview(
        plan.program.as_ref(),
        plan.options.program.as_ref(),
        roll_index,
        &roll.placements,
    )?;
    let [view_w, view_h] = routed.as_ref().map_or([w, h], |p| p.machine.bed_mm);
    let loading = routed.as_ref().and_then(|p| p.marker.loading.as_ref());
    let offset = loading.map_or(Vec2::ZERO, |loading| loading.marker_to_machine_offset_mm);
    if let Some(loading) = loading {
        let stock_width = roll
            .material
            .material()
            .properties
            .get("width")
            .ok_or_else(bad)?
            .bounds
            .map_or(roll.width_m, |bounds| bounds[1])
            * 1000.;
        if offset != loading.lower_left_mm
            || loading.stock_bounds_mm != [offset.x, offset.y, offset.x + stock_width, offset.y + h]
        {
            return Err(bad());
        }
    }
    let (ws, hs) = (fnum(w, 6), fnum(h, 6));
    let (view_ws, view_hs) = (fnum(view_w, 6), fnum(view_h, 6));
    let stock_y = if loading.is_some() {
        format!(
            " x=\"{}\" y=\"{}\"",
            fnum(offset.x, 6),
            fnum(view_h - h - offset.y, 6)
        )
    } else if routed.is_some() {
        format!(" y=\"{}\"", fnum(view_h - h, 6))
    } else {
        String::new()
    };
    let bed = if routed.is_some() {
        format!(
            "<rect id=\"machine-bed\" width=\"{view_ws}\" height=\"{view_hs}\" fill=\"white\" stroke=\"#aaa\" stroke-width=\"0.2\"/>\n"
        )
    } else {
        String::new()
    };
    let grain_policy = if roll.placements.iter().any(|p| p.grain_reversed) {
        "grain-constrained (including explicitly permitted half-turns)"
    } else {
        "one-way-grain"
    };
    let scope = if routed.is_some() {
        "Blue paths show selected raised-tool motion in the declared machine frame. No hardware execution authorization."
    } else {
        "Not machine-qualified toolpaths."
    };
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{view_ws}mm\" height=\"{view_hs}mm\" viewBox=\"0 0 {view_ws} {view_hs}\">\n<title>{}</title>\n<desc>Single-ply {grain_policy} planning preview. {scope}</desc>\n<metadata>pattern_sha256={}; material_sha256={}</metadata>\n{bed}<rect id=\"stock\"{stock_y} width=\"{ws}\" height=\"{hs}\" fill=\"white\" stroke=\"black\" stroke-width=\"0.2\"/>\n<defs><marker id=\"grain-arrow\" markerWidth=\"4\" markerHeight=\"4\" refX=\"4\" refY=\"2\" orient=\"auto\" markerUnits=\"userSpaceOnUse\"><path d=\"M0 0 L4 2 L0 4 Z\"/></marker></defs>\n<g transform=\"translate(0 {view_hs}) scale(1 -1)\" fill=\"none\" stroke=\"black\" stroke-width=\"0.15\">\n",
        xml_escape(&roll.material.material().name),
        xml_escape(&plan.pattern_sha256),
        xml_escape(roll.material.sha256())
    );
    if let Some(loading) = loading {
        let bounds = loading.stock_bounds_mm;
        out.push_str(&format!(
            "<rect id=\"loaded-stock-envelope\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" stroke=\"#975000\" stroke-dasharray=\"2 1\"><title>Complete loaded stock envelope, including the upper source width bound; modeled placement only</title></rect>\n<g id=\"stock-loading\" transform=\"translate({} {})\">\n",
            fnum(bounds[0], 6), fnum(bounds[1], 6), fnum(bounds[2]-bounds[0], 6),
            fnum(bounds[3]-bounds[1], 6), fnum(offset.x, 6), fnum(offset.y, 6)
        ));
    }
    if let Some(layout) = &roll.layout_width {
        let [lo, hi] = layout.reported_bounds_m;
        let layout_mm = layout.width_m * 1000.0;
        if !layout_mm.is_finite()
            || layout_mm <= 0.0
            || layout_mm > w
            || !lo.is_finite()
            || !hi.is_finite()
            || !(hi * 1000.0).is_finite()
            || lo != layout.width_m
            || hi < roll.width_m
        {
            return Err(bad());
        }
        out.push_str(&format!(
            "<g id=\"source-width-envelope\"><title>Layout at source-reported lower width; nominal stock rectangle remains the purchase basis</title><desc>Reported source width range: {} to {} mm. Fixed cutter geometry and margins; no confidence or physical guarantee. Gross area, mass and invoice conversions retain nominal width.</desc><rect id=\"layout-width\" width=\"{}\" height=\"{hs}\" stroke=\"#975000\" stroke-dasharray=\"2 1\"/></g>\n",
            fnum(lo * 1000.0, 6), fnum(hi * 1000.0, 6), fnum(layout_mm, 6)
        ));
    }
    for (index, p) in roll.placements.iter().enumerate() {
        if p.cut_mm.len() < 3
            || p.cut_mm.iter().chain(&p.grain_mm).any(|v| !v.is_finite())
            || p.internal_cut_mm
                .iter()
                .any(|(v, _)| v.len() < 2 || v.iter().any(|p| !p.is_finite()))
            || p.drills_mm
                .iter()
                .any(|(v, d)| !v.is_finite() || !d.is_finite() || *d <= 0.0)
            || p.bounds_mm.iter().any(|v| !v.is_finite())
        {
            return Err(bad());
        }
        out.push_str(&format!(
            "<g id=\"piece-{index}\" data-instance=\"{}\" data-copy=\"{}\"{}><path class=\"cut-exterior\" d=\"{}\"/>\n",
            xml_escape(&p.instance), p.copy, reversal_attribute(p.grain_reversed), path(&p.cut_mm, true)
        ));
        for (v, closed) in &p.internal_cut_mm {
            out.push_str(&format!(
                "<path class=\"cut-interior\" d=\"{}\"/>\n",
                path(v, *closed)
            ));
        }
        for (v, diameter) in &p.drills_mm {
            out.push_str(&format!(
                "<circle class=\"drill\" cx=\"{}\" cy=\"{}\" r=\"{}\"/>\n",
                fnum(v.x, 6),
                fnum(v.y, 6),
                fnum(diameter / 2.0, 6)
            ));
        }
        out.push_str(&format!("<path class=\"grain\" d=\"{}\" stroke-dasharray=\"2 1\" marker-end=\"url(#grain-arrow)\"/></g>\n",
            path(&p.grain_mm, false)));
    }
    if loading.is_some() {
        out.push_str("</g>\n");
    }
    if let Some(preview) = &routed {
        out.push_str(&knife_overlay(preview));
    }
    out.push_str("</g>\n<g id=\"labels\" font-family=\"sans-serif\" font-size=\"3\">\n");
    for p in &roll.placements {
        let (x, y) = if loading.is_some() {
            (
                p.bounds_mm[0] + offset.x + 1.0,
                view_h - p.bounds_mm[3] - offset.y + 4.0,
            )
        } else {
            (p.bounds_mm[0] + 1.0, view_h - p.bounds_mm[3] + 4.0)
        };
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{}\">{} / copy {}</text>\n",
            fnum(x, 6),
            fnum(y, 6),
            xml_escape(&p.instance),
            p.copy
        ));
    }
    out.push_str("</g>\n</svg>\n");
    Ok(out)
}

/// Preview one individually identified hide/remnant from a complete finite-stock plan.
/// Coordinates are the plan's rigid cutting-axis frame in mm; boundaries/defects are NOT cut paths.
/// There are no external resources, scripts, device settings or implicit scaling of the stock.
///
/// # Errors
/// Invalid stock index, more than one million rendered points, or over 64 MiB of preview output.
pub fn stock_svg(plan: &super::stock::Plan, index: usize) -> Result<String, Refusal> {
    let marker = plan.markers().get(index).ok_or_else(|| {
        Refusal::new(
            RefusalCode::InvalidDocument,
            "finite-stock preview index is out of range",
        )
    })?;
    let mut count = marker.outline_mm.len();
    for n in marker
        .defects_mm
        .iter()
        .chain(&marker.occupied_mm)
        .map(Vec::len)
        .chain(marker.zones.iter().map(|z| z.outline_mm.len()))
        .chain(marker.placements.iter().flat_map(|p| {
            [p.cut_mm.len(), 2, p.drills_mm.len()]
                .into_iter()
                .chain(p.internal_cut_mm.iter().map(|(v, _)| v.len()))
        }))
    {
        count = count.checked_add(n).ok_or_else(preview_budget)?;
    }
    if count > 1_000_000 {
        return Err(preview_budget());
    }
    let b = super::bbox(&marker.outline_mm)?;
    let routed = if marker.placements.is_empty() {
        None
    } else {
        routed_preview(
            plan.program(),
            plan.request().options.program.as_ref(),
            index,
            &marker.placements,
        )?
    };
    let (view, transform, offset, stock_frame, bed) = if let Some(preview) = &routed {
        let offset = preview
            .marker
            .finite_stock
            .as_ref()
            .ok_or_else(|| {
                Refusal::new(
                    RefusalCode::InvalidDocument,
                    "missing finite-stock knife loading frame",
                )
            })?
            .marker_to_machine_offset_mm;
        let origin = preview
            .marker
            .loading
            .as_ref()
            .map_or(Vec2::ZERO, |loading| loading.lower_left_mm);
        let expected = if preview.marker.loading.is_some() {
            origin - Vec2::new(b[0], b[1])
        } else {
            Vec2::new(-b[0], -b[1])
        };
        if offset != expected
            || preview.marker.loading.as_ref().is_some_and(|loading| {
                loading.stock_bounds_mm
                    != [
                        origin.x,
                        origin.y,
                        origin.x + (b[2] - b[0]),
                        origin.y + (b[3] - b[1]),
                    ]
            })
        {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "changed finite-stock knife loading frame",
            ));
        }
        let [width, height] = preview.machine.bed_mm;
        (
            [0., 0., width, height],
            format!("translate(0 {}) scale(1 -1)", fnum(height, 6)),
            offset,
            format!(
                "<g transform=\"translate({} {})\">\n",
                fnum(offset.x, 6),
                fnum(offset.y, 6)
            ),
            format!(
                "<rect id=\"machine-bed\" width=\"{}\" height=\"{}\" fill=\"white\" stroke=\"#aaa\" stroke-width=\"0.2\"/>\n",
                fnum(width, 6),
                fnum(height, 6)
            ),
        )
    } else {
        (
            [b[0], -b[3], b[2] - b[0], b[3] - b[1]],
            "scale(1 -1)".into(),
            Vec2::ZERO,
            String::new(),
            String::new(),
        )
    };
    let (w, h) = (fnum(view[2], 6), fnum(view[3], 6));
    let grain_policy = if marker.placements.iter().any(|p| p.grain_reversed) {
        "cutting axis +y with explicitly permitted -y half-turns"
    } else {
        "one-way cutting axis +y"
    };
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}mm\" height=\"{h}mm\" viewBox=\"{} {} {w} {h}\">\n<title>{}</title>\n<desc>Finite single-ply stock review. Stock boundary and defects are NOT cut paths. No machine calibration or execution authorization. Coordinate units: mm; {grain_policy}.</desc>\n<metadata>material_sha256={}; stock_index={index}</metadata>\n{bed}<defs><marker id=\"grain-arrow\" markerWidth=\"4\" markerHeight=\"4\" refX=\"4\" refY=\"2\" orient=\"auto\" markerUnits=\"userSpaceOnUse\"><path d=\"M0 0 L4 2 L0 4 Z\"/></marker></defs>\n<g transform=\"{transform}\" stroke-width=\"0.15\">\n{stock_frame}<path id=\"stock-boundary\" d=\"{}\" fill=\"white\" stroke=\"black\"/>\n<g id=\"stock-defects\" fill=\"#ddd\" stroke=\"#666\" stroke-dasharray=\"2 1\">\n",
        fnum(view[0], 6),
        fnum(view[1], 6),
        xml_escape(&format!(
            "{} — {}",
            marker.id,
            marker.material.material().name
        )),
        xml_escape(marker.material.sha256()),
        path(&marker.outline_mm, true)
    );
    for defect in &marker.defects_mm {
        out.push_str(&format!("<path d=\"{}\"/>\n", path(defect, true)));
    }
    out.push_str(
        "</g>\n<g id=\"stock-occupied\" fill=\"#eee\" stroke=\"#999\" stroke-dasharray=\"1 1\">\n",
    );
    for occupied in &marker.occupied_mm {
        out.push_str(&format!("<path d=\"{}\"/>\n", path(occupied, true)));
    }
    out.push_str("</g>\n");
    if !marker.zones.is_empty() {
        out.push_str(
            "<g id=\"stock-zones\" fill=\"none\" stroke=\"#17695e\" stroke-dasharray=\"3 1\">\n",
        );
        for zone in &marker.zones {
            out.push_str(&format!(
                "<path data-zone=\"{}\" d=\"{}\"><title>{}</title></path>\n",
                xml_escape(&zone.id),
                path(&zone.outline_mm, true),
                xml_escape(&format!(
                    "{}: placement region, NOT a cut path; {}",
                    zone.id, zone.geometry_source.note
                ))
            ));
            if out.len() > 64 * 1024 * 1024 {
                return Err(preview_budget());
            }
        }
        out.push_str("</g>\n");
    }
    out.push_str("<g id=\"placements\" fill=\"none\" stroke=\"black\">\n");
    for (i, p) in marker.placements.iter().enumerate() {
        let zone_attribute = plan
            .request()
            .options
            .piece_zones
            .get(&p.piece)
            .map_or_else(String::new, |id| {
                format!(" data-required-zone=\"{}\"", xml_escape(id))
            });
        out.push_str(&format!(
            "<g id=\"piece-{i}\" data-instance=\"{}\" data-copy=\"{}\"{}{zone_attribute}><path class=\"cut-exterior\" d=\"{}\"/>\n",
            xml_escape(&p.instance), p.copy, reversal_attribute(p.grain_reversed), path(&p.cut_mm, true)
        ));
        for (v, closed) in &p.internal_cut_mm {
            out.push_str(&format!(
                "<path class=\"cut-interior\" d=\"{}\"/>\n",
                path(v, *closed)
            ));
        }
        for (v, diameter) in &p.drills_mm {
            out.push_str(&format!(
                "<circle class=\"drill\" cx=\"{}\" cy=\"{}\" r=\"{}\"/>\n",
                fnum(v.x, 6),
                fnum(v.y, 6),
                fnum(diameter / 2.0, 6)
            ));
        }
        out.push_str(&format!(
            "<path class=\"grain\" d=\"{}\" stroke-dasharray=\"2 1\" marker-end=\"url(#grain-arrow)\"/></g>\n",
            path(&p.grain_mm, false)
        ));
        if out.len() > 64 * 1024 * 1024 {
            return Err(preview_budget());
        }
    }
    out.push_str("</g>");
    if let Some(preview) = &routed {
        out.push_str("</g>\n");
        out.push_str(&knife_overlay(preview));
    }
    out.push_str("</g>\n<g id=\"labels\" font-family=\"sans-serif\" font-size=\"3\">\n");
    for p in &marker.placements {
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{}\">{} / copy {}</text>\n",
            fnum(p.bounds_mm[0] + offset.x + 1.0, 6),
            fnum(
                routed.as_ref().map_or(0., |p| p.machine.bed_mm[1]) - (p.bounds_mm[3] + offset.y)
                    + 4.0,
                6
            ),
            xml_escape(&p.instance),
            p.copy
        ));
    }
    out.push_str("</g>\n</svg>\n");
    if out.len() > 64 * 1024 * 1024 {
        return Err(preview_budget());
    }
    Ok(out)
}
fn preview_budget() -> Refusal {
    Refusal::new(
        RefusalCode::BudgetExceeded,
        "finite-stock preview exceeds output budget",
    )
}
