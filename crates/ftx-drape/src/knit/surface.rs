//! Source-bound continuum surfaces for admitted stitch graphs.
//!
//! Vertices are stitch-loop centres, one per source node. Free boundaries therefore follow
//! the outer loop centres; this discretization does not invent a half-stitch selvedge extension.
//! Faces join actual adjacent course loops and their owned parent arcs, never a filled bounding
//! rectangle. Rib bed changes do not change a loop's logical side of a declared tube.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, Vec2, det};
use ftx_pattern::knit::{Bed, KnitDoc, LoopKind, StitchNode};
use serde::{Deserialize, Serialize};

/// Topology admitted by the stitch-centre surface builder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnitSurfaceTopology {
    /// A connected open panel, including a held-centre neckline and separate shoulders.
    Panel,
    /// A declared, connected torso tube with two uncapped boundary rings.
    Tube,
}

/// Logical fabric side, independent of which needle bed temporarily holds a rib loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnitSurfaceSide {
    /// Logical front half of a circular course.
    Front,
    /// Logical back half of a circular course.
    Back,
}

/// Exact source identity and course coordinates of one loop-centre vertex.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KnitSurfaceVertex {
    /// Unchanged source node identity.
    pub stitch_id: u32,
    /// Unchanged source course index.
    pub course: u32,
    /// Original physical needle coordinate; retained without half-gauge normalization.
    pub wale: i32,
    /// Logical tube side, or `None` for an open panel.
    pub logical_side: Option<KnitSurfaceSide>,
    /// Fabric wale index after an explicitly declared half-gauge layout is normalized.
    pub logical_wale: i32,
    /// Positive material-u distance along course growth, in metres.
    pub course_distance_m: f64,
    /// Index in the course contour: increasing wales for a panel; front ascending then
    /// back descending for a tube. This is not the machine's execution order.
    pub course_position: usize,
    /// Actual count of source loops in this course, including both logical tube sides.
    pub course_stitches: usize,
}

/// A deterministic surface with exact node and triangle source correspondence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KnitSurfaceMesh {
    /// Exact canonical hash of the source document used to construct this surface.
    pub pattern_sha256: String,
    /// Validated physical fabric topology.
    pub topology: KnitSurfaceTopology,
    /// Anatomical vertical orientation declared by the admitted construction pathway.
    pub course_growth_upward: bool,
    /// One loop-centre vertex per source node, in deterministic geometric course order.
    pub vertices: Vec<KnitSurfaceVertex>,
    /// Oriented manifold faces indexing the source-bound vertices.
    pub triangles: Vec<[usize; 3]>,
    /// Intrinsic charts in metres. Material +u is course growth (wale direction); +v
    /// is opposite the ordered course contour. Every triangle has positive chart area.
    /// Shared edges have the same metric on both incident triangles. A shaped tube need
    /// not admit one globally flat chart.
    pub tri_rest_m: Vec<[Vec2; 3]>,
    /// Source stitch IDs in exactly the same order as each triangle's vertices.
    pub triangle_stitch_ids: Vec<[u32; 3]>,
    /// Actual oriented topological boundaries, indexing `vertices`; no caps or virtual seams.
    pub boundary_loops: Vec<Vec<usize>>,
    /// Inclusive unchanged source course range.
    pub course_range: [u32; 2],
    /// Inclusive unchanged source physical wale range.
    pub wale_range: [i32; 2],
    /// Qualified blocked gauge converted from millimetres to metres, without clamping.
    pub wale_pitch_m: f64,
    /// Qualified blocked course height in metres, without clamping.
    pub course_height_m: f64,
}

#[derive(Clone, Debug)]
struct Course {
    vertices: Vec<usize>,
    first_wale: i32,
    last_wale: i32,
}

fn unsupported(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::UnqualifiedProcess, message)
}

/// Builds an intrinsic continuum mesh from admitted parent-loop topology.
///
/// This bounded first surface pathway admits ordinary cast-on/knit/purl/decrease/bind-off
/// panels and explicitly declared torso tubes. It refuses branched joins, cable crossings,
/// tuck/short-row schedules, parent reuse, omitted live-loop ownership, and unknown circular
/// layouts. A caller's vertex budget is a refusal bound, never a silent decimation request.
///
/// # Errors
/// Invalid source, unsupported topology, degenerate intrinsic triangles, or exceeded budget.
pub fn mesh_knit_doc(doc: &KnitDoc, max_vertices: usize) -> Result<KnitSurfaceMesh, Refusal> {
    doc.validate()?;
    if max_vertices == 0 || doc.stitches.len() > max_vertices {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!(
                "source knit surface requires {} stitch vertices; explicit budget is {max_vertices}",
                doc.stitches.len()
            ),
        ));
    }
    let pathway = doc.metadata.get("pathway").map(String::as_str);
    let (topology, course_growth_upward) = match pathway {
        Some("top_down_shaped_torso_tube") => (KnitSurfaceTopology::Tube, false),
        Some("autoknit_annular_intrinsic_harmonic") => {
            if doc.metadata.get("surface_topology").map(String::as_str) != Some("torso_tube") {
                return Err(unsupported(
                    "AutoKnit drape requires an explicitly retained annular source topology",
                ));
            }
            let upward = match doc
                .metadata
                .get("course_growth_direction")
                .map(String::as_str)
            {
                Some("upward") => true,
                Some("downward") => false,
                _ => {
                    return Err(unsupported(
                        "AutoKnit source does not declare a vertically placeable tube orientation",
                    ));
                }
            };
            (KnitSurfaceTopology::Tube, upward)
        }
        Some("autoknit_idt_harmonic_3d") => {
            return Err(unsupported(
                "legacy AutoKnit documents lack source-annulus validation; regenerate from the retained source shell",
            ));
        }
        Some("bottom_up_three_tube_raglan") => {
            return Err(unsupported(
                "branched sleeve/torso joins need a branch-aware surface; a torso rectangle is not their geometry",
            ));
        }
        _ => (KnitSurfaceTopology::Panel, true),
    };
    let half_gauge = match doc.metadata.get("tube_structure").map(String::as_str) {
        None => false,
        Some("half_gauge_1x1_rib") if topology == KnitSurfaceTopology::Tube => true,
        Some(layout) => {
            return Err(unsupported(format!(
                "knit surface does not admit tube layout '{layout}'"
            )));
        }
    };
    if !doc.short_row_turns.is_empty() || !doc.cable_groups.is_empty() {
        return Err(unsupported(
            "anchored short rows and cable crossings require their own intrinsic surface cells",
        ));
    }
    for node in &doc.stitches {
        if !matches!(
            node.kind,
            LoopKind::CastOn
                | LoopKind::Knit
                | LoopKind::Purl
                | LoopKind::Decrease
                | LoopKind::BindOff
        ) {
            return Err(unsupported(format!(
                "source stitch {} has unsupported surface cell {:?}",
                node.id, node.kind
            )));
        }
        if node.bed.is_slider() {
            return Err(unsupported(
                "a fabric loop cannot be a slider surface vertex",
            ));
        }
    }
    let course_min = doc.stitches.iter().map(|node| node.course).min().unwrap();
    let course_max = doc.stitches.iter().map(|node| node.course).max().unwrap();
    let wale_min = doc.stitches.iter().map(|node| node.wale).min().unwrap();
    let wale_max = doc.stitches.iter().map(|node| node.wale).max().unwrap();
    let wale_pitch_m = doc.gauge.blocked_wale_pitch_mm() * 0.001;
    let course_height_m = doc.gauge.blocked_course_height_mm() * 0.001;
    let mut nodes_by_course: BTreeMap<u32, Vec<&StitchNode>> = BTreeMap::new();
    for node in &doc.stitches {
        nodes_by_course.entry(node.course).or_default().push(node);
    }
    let side_and_wale = |node: &StitchNode| {
        if topology == KnitSurfaceTopology::Panel {
            (None, node.wale)
        } else if half_gauge {
            (
                Some(if node.wale.rem_euclid(2) == 0 {
                    KnitSurfaceSide::Front
                } else {
                    KnitSurfaceSide::Back
                }),
                node.wale.div_euclid(2),
            )
        } else {
            (
                Some(if node.bed == Bed::Front {
                    KnitSurfaceSide::Front
                } else {
                    KnitSurfaceSide::Back
                }),
                node.wale,
            )
        }
    };
    let mut vertices = Vec::with_capacity(doc.stitches.len());
    let mut source_nodes = Vec::with_capacity(doc.stitches.len());
    let mut id_to_vertex = BTreeMap::new();
    let mut courses = BTreeMap::new();
    for (&course, nodes) in &mut nodes_by_course {
        nodes.sort_by_key(|node| {
            let (side, wale) = side_and_wale(node);
            (
                side,
                if side == Some(KnitSurfaceSide::Back) {
                    -i64::from(wale)
                } else {
                    i64::from(wale)
                },
                node.id,
            )
        });
        let mut occupied = BTreeSet::new();
        let mut contour = Vec::with_capacity(nodes.len());
        for (position, node) in nodes.iter().enumerate() {
            let (side, wale) = side_and_wale(node);
            if !occupied.insert((side, wale)) {
                return Err(unsupported(format!(
                    "course {course} has overlapping logical surface loops; declare a supported circular layout"
                )));
            }
            let vertex = vertices.len();
            id_to_vertex.insert(node.id, vertex);
            contour.push(vertex);
            vertices.push(KnitSurfaceVertex {
                stitch_id: node.id,
                course,
                wale: node.wale,
                logical_side: side,
                logical_wale: wale,
                course_distance_m: f64::from(course - course_min) * course_height_m,
                course_position: position,
                course_stitches: nodes.len(),
            });
            source_nodes.push(*node);
        }
        let first_wale = occupied.iter().map(|(_, wale)| *wale).min().unwrap();
        let last_wale = occupied.iter().map(|(_, wale)| *wale).max().unwrap();
        if topology == KnitSurfaceTopology::Tube {
            let side_width = i64::from(last_wale) - i64::from(first_wale) + 1;
            if side_width < 2 || side_width * 2 != nodes.len() as i64 {
                return Err(unsupported(format!(
                    "tube course {course} must have two complete equal-width logical sides"
                )));
            }
            for side in [KnitSurfaceSide::Front, KnitSurfaceSide::Back] {
                for wale in first_wale..=last_wale {
                    if !occupied.contains(&(Some(side), wale)) {
                        return Err(unsupported("tube course has an unrepresented opening"));
                    }
                }
            }
            if nodes
                .iter()
                .any(|node| node.carrier_ids != nodes[0].carrier_ids)
            {
                return Err(unsupported(
                    "independently fed tube components require an explicit branch surface",
                ));
            }
        }
        courses.insert(
            course,
            Course {
                vertices: contour,
                first_wale,
                last_wale,
            },
        );
    }
    let mut held = BTreeSet::new();
    if let Some(ids) = doc.metadata.get("held_neck_stitch_ids") {
        for text in ids.split(',').filter(|text| !text.is_empty()) {
            let id = text.parse::<u32>().map_err(|_| {
                Refusal::new(RefusalCode::InvalidDocument, "invalid held neck stitch ID")
            })?;
            let &vertex = id_to_vertex.get(&id).ok_or_else(|| {
                Refusal::new(RefusalCode::InvalidDocument, "unknown held neck stitch ID")
            })?;
            if !held.insert(vertex) || topology != KnitSurfaceTopology::Panel {
                return Err(unsupported("invalid held neck topology declaration"));
            }
        }
    }
    let mut parents = vec![Vec::new(); vertices.len()];
    for (vertex, node) in source_nodes.iter().enumerate() {
        for parent in &node.parents {
            let parent_vertex = id_to_vertex[&parent.stitch_id];
            let parent_node = source_nodes[parent_vertex];
            if parent_node.course == node.course {
                let a = vertices[vertex].course_position;
                let b = vertices[parent_vertex].course_position;
                let adjacent = if topology == KnitSurfaceTopology::Tube {
                    a.abs_diff(b) == 1 || a.abs_diff(b) + 1 == vertices[vertex].course_stitches
                } else {
                    vertices[vertex]
                        .logical_wale
                        .abs_diff(vertices[parent_vertex].logical_wale)
                        == 1
                };
                if node.kind != LoopKind::BindOff
                    || parent_node.kind != LoopKind::BindOff
                    || !adjacent
                {
                    return Err(unsupported(
                        "same-course surface parents must be adjacent terminal bind-off loops",
                    ));
                }
                continue;
            }
            if parent_node.course.checked_add(1) != Some(node.course)
                || parent_node.kind == LoopKind::BindOff
                || vertices[vertex].logical_side != vertices[parent_vertex].logical_side
                || held.contains(&parent_vertex)
            {
                return Err(unsupported(format!(
                    "stitch {} does not consume an admitted immediately preceding fabric loop",
                    node.id
                )));
            }
            parents[vertex].push(parent_vertex);
        }
        parents[vertex].sort_by_key(|&parent| vertices[parent].course_position);
        if parents[vertex].windows(2).any(|pair| {
            vertices[pair[0]].course_position + 1 != vertices[pair[1]].course_position
                || (topology == KnitSurfaceTopology::Panel
                    && i64::from(vertices[pair[0]].logical_wale) + 1
                        != i64::from(vertices[pair[1]].logical_wale))
        }) {
            return Err(unsupported(
                "a decrease must own one contiguous previous-course arc",
            ));
        }
        if parents[vertex].is_empty() != (node.kind == LoopKind::CastOn) {
            return Err(unsupported(format!(
                "stitch {} does not have the parent ownership required by its surface cell",
                node.id
            )));
        }
    }
    if courses[&course_min]
        .vertices
        .iter()
        .any(|&vertex| source_nodes[vertex].kind != LoopKind::CastOn)
    {
        return Err(unsupported(
            "the first fabric contour must consist of cast-on loops",
        ));
    }
    let mut triangles = Vec::new();
    let mut last_course: Option<u32> = None;
    for (&course, contour) in &courses {
        let Some(previous_number) = last_course else {
            last_course = Some(course);
            continue;
        };
        if previous_number.checked_add(1) != Some(course) {
            return Err(unsupported(
                "missing source courses require explicit held-loop surface cells",
            ));
        }
        let previous: &Course = &courses[&previous_number];
        let mut consumed = BTreeSet::new();
        for &vertex in &contour.vertices {
            for &parent in &parents[vertex] {
                if !consumed.insert(parent) {
                    return Err(unsupported(
                        "a parent loop is consumed by multiple surface cells",
                    ));
                }
            }
            if parents[vertex].is_empty()
                && vertices[vertex].logical_wale >= previous.first_wale
                && vertices[vertex].logical_wale <= previous.last_wale
            {
                return Err(unsupported(
                    "interior cast-on loops need an explicit eyelet/increase surface cell",
                ));
            }
            for pair in parents[vertex].windows(2) {
                triangles.push([pair[0], vertex, pair[1]]);
            }
        }
        if previous
            .vertices
            .iter()
            .any(|vertex| !consumed.contains(vertex) && !held.contains(vertex))
        {
            return Err(unsupported(
                "the source drops or holds a previous-course loop without an admitted boundary declaration",
            ));
        }
        if topology == KnitSurfaceTopology::Tube {
            connect_arc(&contour.vertices, true, &parents, &vertices, &mut triangles)?;
        } else {
            let mut start = 0;
            for end in 1..=contour.vertices.len() {
                let end_of_arc = end == contour.vertices.len()
                    || i64::from(vertices[contour.vertices[end]].logical_wale)
                        - i64::from(vertices[contour.vertices[end - 1]].logical_wale)
                        != 1;
                if end_of_arc {
                    connect_arc(
                        &contour.vertices[start..end],
                        false,
                        &parents,
                        &vertices,
                        &mut triangles,
                    )?;
                    start = end;
                }
            }
        }
        last_course = Some(course);
    }
    let mut tri_rest_m = Vec::with_capacity(triangles.len());
    for triangle in &mut triangles {
        let chart = triangle_chart(
            *triangle,
            &vertices,
            topology,
            wale_pitch_m,
            course_height_m,
        )?;
        let signed_area = (chart[1] - chart[0]).cross(chart[2] - chart[0]);
        if !signed_area.is_finite() || signed_area.abs() <= 1e-24 {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "stitch topology produced a degenerate intrinsic surface cell",
            ));
        }
        if signed_area < 0.0 {
            triangle.swap(1, 2);
            tri_rest_m.push([chart[0], chart[2], chart[1]]);
        } else {
            tri_rest_m.push(chart);
        }
    }
    let boundary_loops = audit_surface(&triangles, &tri_rest_m, vertices.len(), topology)?;
    let triangle_stitch_ids = triangles
        .iter()
        .map(|triangle| triangle.map(|vertex| vertices[vertex].stitch_id))
        .collect();
    Ok(KnitSurfaceMesh {
        pattern_sha256: doc.canonical_sha256()?,
        topology,
        course_growth_upward,
        vertices,
        triangles,
        tri_rest_m,
        triangle_stitch_ids,
        boundary_loops,
        course_range: [course_min, course_max],
        wale_range: [wale_min, wale_max],
        wale_pitch_m,
        course_height_m,
    })
}

fn connect_arc(
    current: &[usize],
    closed: bool,
    parents: &[Vec<usize>],
    vertices: &[KnitSurfaceVertex],
    triangles: &mut Vec<[usize; 3]>,
) -> Result<(), Refusal> {
    if current.len() < 2 {
        return Err(unsupported(
            "a one-stitch-wide fabric branch has no continuum surface",
        ));
    }
    let anchors: Vec<usize> = (0..current.len())
        .filter(|&index| !parents[current[index]].is_empty())
        .collect();
    if anchors.is_empty() {
        return Err(unsupported(
            "a new independent cast-on requires a separate surface component",
        ));
    }
    let join = |left: usize, right: usize, triangles: &mut Vec<[usize; 3]>| {
        let a = *parents[current[left]].last().unwrap();
        let b = parents[current[right % current.len()]][0];
        let previous_count = vertices[a].course_stitches;
        let is_next = vertices[a].course_position + 1 == vertices[b].course_position
            || (closed
                && vertices[a].course_position + 1 == previous_count
                && vertices[b].course_position == 0);
        let crosses_panel_gap = !closed
            && i64::from(vertices[a].logical_wale) + 1 != i64::from(vertices[b].logical_wale);
        if !is_next || crosses_panel_gap {
            return Err(unsupported(
                "adjacent new stitches have crossing, disconnected, or missing parent arcs",
            ));
        }
        for index in left..right {
            triangles.push([
                a,
                current[(index + 1) % current.len()],
                current[index % current.len()],
            ]);
        }
        triangles.push([a, b, current[right % current.len()]]);
        Ok(())
    };
    for pair in anchors.windows(2) {
        join(pair[0], pair[1], triangles)?;
    }
    if closed {
        join(
            *anchors.last().unwrap(),
            anchors[0] + current.len(),
            triangles,
        )?;
    } else {
        let first = anchors[0];
        let a = parents[current[first]][0];
        for index in 0..first {
            triangles.push([a, current[index + 1], current[index]]);
        }
        let last = *anchors.last().unwrap();
        let a = *parents[current[last]].last().unwrap();
        for index in last..current.len() - 1 {
            triangles.push([a, current[index + 1], current[index]]);
        }
    }
    Ok(())
}

fn triangle_chart(
    triangle: [usize; 3],
    vertices: &[KnitSurfaceVertex],
    topology: KnitSurfaceTopology,
    pitch: f64,
    height: f64,
) -> Result<[Vec2; 3], Refusal> {
    if topology == KnitSurfaceTopology::Panel {
        return Ok(triangle.map(|vertex| {
            Vec2::new(
                vertices[vertex].course_distance_m,
                -f64::from(vertices[vertex].logical_wale) * pitch,
            )
        }));
    }
    let edge_length = |a: usize, b: usize| {
        let a = &vertices[a];
        let b = &vertices[b];
        // A logical side retains its authored wale spacing. Across a circular side seam,
        // the two adjacent edge loops are one wale pitch apart. This is the intrinsic
        // metric of two course charts joined by a one-pitch seam strip; no body radius or
        // placed 3D stretch enters it. The diagonal's squared transverse components add.
        let horizontal = (i64::from(a.logical_wale) - i64::from(b.logical_wale)) as f64 * pitch;
        let across_seam_squared = if a.logical_side == b.logical_side {
            0.0
        } else {
            pitch * pitch
        };
        let vertical = f64::from(a.course.abs_diff(b.course)) * height;
        det::sqrt(horizontal * horizontal + across_seam_squared + vertical * vertical)
    };
    let (mut a, mut b, other) = [(0, 1, 2), (1, 2, 0), (2, 0, 1)]
        .into_iter()
        .find(|&(a, b, _)| vertices[triangle[a]].course == vertices[triangle[b]].course)
        .ok_or_else(|| unsupported("surface triangle has no source course edge"))?;
    let pa = vertices[triangle[a]].course_position;
    let pb = vertices[triangle[b]].course_position;
    let count = vertices[triangle[a]].course_stitches;
    if (pa + 1) % count != pb {
        if (pb + 1) % count != pa {
            return Err(unsupported("surface triangle skips a source course loop"));
        }
        std::mem::swap(&mut a, &mut b);
    }
    let distance_a = edge_length(triangle[a], triangle[other]);
    let distance_b = edge_length(triangle[b], triangle[other]);
    let projection =
        (distance_a * distance_a + pitch * pitch - distance_b * distance_b) / (2.0 * pitch);
    let altitude_squared = distance_a * distance_a - projection * projection;
    if !altitude_squared.is_finite() || altitude_squared <= 1e-24 {
        return Err(unsupported(
            "source gauge and shaping exceed the admitted intrinsic surface cell geometry",
        ));
    }
    let sign = if vertices[triangle[other]].course > vertices[triangle[a]].course {
        1.0
    } else {
        -1.0
    };
    let mut chart = [Vec2::ZERO; 3];
    chart[a] = Vec2::ZERO;
    chart[b] = Vec2::new(0.0, -pitch);
    chart[other] = Vec2::new(sign * det::sqrt(altitude_squared), -projection);
    Ok(chart)
}

fn audit_surface(
    triangles: &[[usize; 3]],
    charts: &[[Vec2; 3]],
    vertex_count: usize,
    topology: KnitSurfaceTopology,
) -> Result<Vec<Vec<usize>>, Refusal> {
    let mut edges: BTreeMap<(usize, usize), Vec<(usize, usize, f64)>> = BTreeMap::new();
    let mut used = BTreeSet::new();
    let mut neighbors = vec![BTreeSet::new(); vertex_count];
    for (triangle, chart) in triangles.iter().zip(charts) {
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            let from = triangle[a];
            let to = triangle[b];
            used.insert(from);
            neighbors[from].insert(to);
            neighbors[to].insert(from);
            edges
                .entry((from.min(to), from.max(to)))
                .or_default()
                .push((from, to, (chart[b] - chart[a]).norm()));
        }
    }
    if used.len() != vertex_count {
        return Err(unsupported(
            "source stitch vertices are orphaned from the continuum surface",
        ));
    }
    let mut visited = BTreeSet::new();
    let mut pending = vec![0];
    while let Some(vertex) = pending.pop() {
        if visited.insert(vertex) {
            pending.extend(neighbors[vertex].iter().copied());
        }
    }
    if visited.len() != vertex_count {
        return Err(unsupported(
            "disconnected fabrics require independent placed surfaces",
        ));
    }
    let mut boundary_next = BTreeMap::new();
    let mut boundary_incoming = BTreeSet::new();
    for incident in edges.values() {
        match incident.as_slice() {
            &[(from, to, _)] => {
                if boundary_next.insert(from, to).is_some() || !boundary_incoming.insert(to) {
                    return Err(unsupported(
                        "source surface has a non-manifold boundary vertex",
                    ));
                }
            }
            &[(a, b, length_a), (c, d, length_b)]
                if a == d
                    && b == c
                    && (length_a - length_b).abs() <= 1e-10 * length_a.max(1e-9) => {}
            _ => {
                return Err(unsupported(
                    "source surface has a non-manifold edge or inconsistent adjacent intrinsic metrics",
                ));
            }
        }
    }
    if boundary_next.keys().copied().collect::<BTreeSet<_>>() != boundary_incoming {
        return Err(unsupported("source surface has an open boundary chain"));
    }
    let mut boundary_loops = Vec::new();
    while let Some((&start, _)) = boundary_next.first_key_value() {
        let mut ring = Vec::new();
        let mut vertex = start;
        loop {
            ring.push(vertex);
            vertex = boundary_next.remove(&vertex).ok_or_else(|| {
                unsupported("source surface boundary is not a simple oriented loop")
            })?;
            if vertex == start {
                break;
            }
        }
        if ring.len() < 3 {
            return Err(unsupported(
                "surface boundary has fewer than three source loops",
            ));
        }
        boundary_loops.push(ring);
    }
    let expected = if topology == KnitSurfaceTopology::Tube {
        2
    } else {
        1
    };
    if boundary_loops.len() != expected {
        return Err(unsupported(format!(
            "source surface has {} boundary loops, expected {expected} for {topology:?}",
            boundary_loops.len()
        )));
    }
    Ok(boundary_loops)
}
