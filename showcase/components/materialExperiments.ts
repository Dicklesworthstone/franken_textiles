export const experiments = {
  cylinder: { title: "The torsion rig", action: "Twist a cloth cylinder", subtitle: "Follow a tube of cloth through tension, twist and compression. Watch the steel grips and read their reactions at each recorded step.", figure: "04" },
  strip: { title: "Cut on the bias", action: "Compare three grain directions", subtitle: "Pull three strips of the same synthetic cloth by the same distance. The direction of the cut changes the force at the grips.", figure: "05" },
} as const;
export type Rig = keyof typeof experiments;
export type Vertex = [number, number, number];
type Frame = { positions_m: Vertex[] };
type CylinderState = Frame & { time_s: number; holder_separation_m: number; rotation_deg: number;
  upper_force_n: number; upper_torque_nm: number; slip_limit_exceeded: boolean; free_dynamic_residual_n: number };
type StripState = Frame & { extension_fraction: number; force_n: number; mid_width_m: number; free_force_residual_n: number };
type Fixture = { cells: [number, number]; width_m: number; gauge_length_m: number };
export type Specimen = { name: string; cells: [number, number]; triangles: number[]; frames: Frame[]; width: number; height: number };
export type Reading = { coordinate: number; forces: number[]; widths: number[]; angle: number; torque: number; gap: number; slip: boolean; residual: number };
export type Experiment = { kind: Rig; specimens: Specimen[]; readings: Reading[]; limit: number; notes: string[]; bench?:{points:number[][]}; comparison?:number[] };

// Same alternating diagonals and row-major vertex numbering as ftx_drape::cloth::grid.
function stripTriangles([nx, ny]: [number, number]) {
  const triangles: number[] = [];
  for (let j = 0; j < ny; j++) for (let i = 0; i < nx; i++) {
    const a = j * (nx + 1) + i, b = a + 1, d = a + nx + 1, c = d + 1;
    triangles.push(...((i + j) % 2 === 0 ? [a, b, c, a, c, d] : [a, b, d, b, c, d]));
  }
  return triangles;
}

export async function loadExperiment(kind: Rig, signal: AbortSignal): Promise<Experiment> {
  const response = await fetch(`/materials/${kind}.json`, { signal });
  if (!response.ok) throw Error("The experiment report could not be loaded.");
  if (kind === "cylinder") {
    const r: { states: CylinderState[]; triangles: number[][]; isometric_mesh_radius_m: number; input: {
      fixture: Fixture & { nominal_free_height_m: number; grip_slip_force_limit_n: number } }; protocol_audit: { discrepancies: string[] } } = await response.json();
    const second=await fetch("/materials/cylinder-refinement.json",{signal});
    if(!second.ok)throw Error("The mesh comparison could not be loaded.");
    const coarse:{
      readings:{time:number;force:number}[];
      additionalMeshes?:{cells:number[];refusal?:{code:string}}[];
    }=await second.json();
    const comparison=r.states.map((state,i)=>{const row=coarse.readings[i];
      if(row?.time!==state.time_s)throw Error("Different experiment schedules.");return row.force;});
    return { kind, comparison, specimens: [{ name: "Cylinder", cells: r.input.fixture.cells, triangles: r.triangles.flat(),
      frames: r.states, width: r.isometric_mesh_radius_m * 2000, height: r.input.fixture.nominal_free_height_m * 1000 }],
      readings: r.states.map(s => ({ coordinate: s.time_s, forces: [s.upper_force_n], widths: [],
        torque: s.upper_torque_nm, angle: s.rotation_deg, gap: s.holder_separation_m * 1000, slip: s.slip_limit_exceeded, residual: s.free_dynamic_residual_n })),
      limit: r.input.fixture.grip_slip_force_limit_n, notes: [
        ...(coarse.additionalMeshes ?? []).map(run => `${run.cells.join(" × ")} mesh: ${run.refusal ? "the solve did not converge." : "the full loading cycle completed."}`),
        ...r.protocol_audit.discrepancies,
      ] };
  }
  const runs: { name: string; report: { input: { fixture: Fixture }; states: StripState[] } }[] = await response.json();
  const source=await fetch("/materials/jersey-readings.json",{signal});
  if(!source.ok)throw Error("The measured strip data could not be loaded.");
  const bench:{rows:{extension:number;force:number[]}[]}=await source.json();
  return { kind, bench:{points:runs[0].report.states.map(s=>{
    const row=bench.rows.find(row=>row.extension===s.extension_fraction);
    if(!row)throw Error("No measured reading at this extension");return row.force;
  })}, specimens: runs.map(({ name, report }) => {
    const f = report.input.fixture;
    return { name, triangles: stripTriangles(f.cells), cells: f.cells, frames: report.states, width: f.width_m * 1000, height: f.gauge_length_m * 1000 };
  }), readings: runs[0].report.states.map((s, k) => ({ coordinate: s.extension_fraction * 100,
    forces: runs.map(r => r.report.states[k].force_n), widths: runs.map(r => r.report.states[k].mid_width_m * 1000),
    angle: 0, torque: 0, gap: 0, slip: false, residual: Math.max(...runs.map(r => r.report.states[k].free_force_residual_n)) })),
    limit: 0, notes: [] };
}
