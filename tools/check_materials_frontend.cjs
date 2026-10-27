// Actual material page/worker modules with explicit DOM/WASM doubles: lifecycle tests, not physics.
// Run directly or through check_trace_frontend.cjs (the existing hosted frontend command).
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const read = name => fs.readFileSync(path.join(__dirname, '../web', name), 'utf8');
const plain = value => JSON.parse(JSON.stringify(value));
const deferred = () => { let resolve; const promise = new Promise(r => { resolve = r; }); return { promise, resolve }; };
// Retained Rust result doubles: these tests verify transport and UI lifecycle, not treatment physics.
function treatmentReport(pattern = false) {
  const before=' {"id":"denim-before","expected_sha256":"before-lock","x":-0.0}\n',
    after=' {"id":"denim-after","expected_sha256":"after-lock","value":1.0}\n';
  const transition={schema:'ftx-material-treatment-report/1',input_sha256:'transition-lock',
    input:{specimen:'denim-1',cycles:5,process:'ISO 6330; <literal source>',conditioning_after:'20 C, 65% RH'},
    stretch:[0.9517,0.945],area_scale:0.8993565,
    properties_no_longer_available:['thread_density_warp','thread_density_weft'],responses_no_longer_available:[],
    before_selection_json:before,after_selection_json:after};
  if (!pattern) return transition;
  return {schema:'ftx-pattern-treatment-report/1',mode:'treat',transitions:{front:transition},
    source_pattern_sha256:'source-lock',pattern_sha256:'derived-lock',
    source_pattern_json:' {"pieces":[{"id":"front","x":-0.0}],"value":1.0}\n',
    pattern_json:' {"pieces":[{"id":"front","x":-0.0}],"derived":1.0}\n',
    frozen_request_json:' {"schema":"ftx-pattern-treatment/1","frozen":1.0,"x":-0.0}\n',
    seams_before:[{seam:'side',status:'compatible'}],seams_after:[{seam:'side',status:'incompatible',residual_mm:2.5}]};
}
const calibrationSelection = admission => `{"id":"identified","expected_sha256":"locked","allow_estimates":${admission},"value":-0.0,"coefficient":1.0}`;
function vaporReport() {
  const replay = '{"schema":"ftx-vapor-stack/1","area_m2":1.0,"source_value":-0.0}';
  return { schema:'ftx-vapor-stack-report/1', request:{area_m2:1}, request_sha256:'source-lock',
    replay_request:JSON.parse(replay), replay_request_json:replay,
    layers:[{key:'<shell>',selection:{id:'retained-specimen'},material_sha256:'material-lock',
      observation:{value:20,unit:'m2*Pa/W',evidence:'measured',method:'Original <measurement>',
        conditions:{air_temperature_c:'35',relative_humidity_percent:'40',air_speed_m_s:'1'}},
      source:{title:'Retained source'},inside_pressure_pa:2500,outside_pressure_pa:1500,pressure_drop_pa:1000}],
    layer_resistance_m2_pa_w:20,total_resistance_m2_pa_w:40,pressure_difference_pa:2000,
    inside_film_pressure_drop_pa:500,outside_film_pressure_drop_pa:500,
    potential_heat_flux_w_m2:50,potential_heat_flow_w:50,
    potential_mass_flux_kg_m2_s:0.00002,potential_mass_flow_kg_s:0.00002,mixed_protocols:false,
    effective_model:'Declared synthetic constant-resistance series network',physical_validation_claim:false,
    notes:['Synthetic UI test only; latent heat is a caller-supplied conversion.'] };
}
function vaporCoverageReport() {
  const replay=' {"schema":"ftx-vapor-coverage/1","source":"<synthetic retained geometry>","value":1.0,"x":-0.0}\n';
  return {schema:'ftx-vapor-coverage-report/1',request_sha256:'coverage-lock',replay_request_json:replay,
    coverage:{body_area_m2:2,covered_area_m2:1.5,uncovered_area_m2:0.5,unassigned_cloth_reference_area_m2:0.25,
      covered_potential_heat_flow_w:30,covered_potential_mass_flow_kg_s:0.000012,
      regions:[{id:'body-2-region-0',body_triangle:2,vapor:vaporReport()}]},
    projection:{geometry_operations:12},request:{source:'<synthetic retained geometry>'},physical_validation_claim:false};
}
function vaporHistoryReport(integration='right_endpoint') {
  const replay=` {"schema":"ftx-vapor-coverage-history/1","policy":{"integration":"${integration}"},"value":1.0,"x":-0.0}\n`;
  const sample=(covered,uncovered,power,mass)=>{
    const result=vaporCoverageReport();
    Object.assign(result.coverage,{covered_area_m2:covered,uncovered_area_m2:uncovered,
      covered_potential_heat_flow_w:power,covered_potential_mass_flow_kg_s:mass});
    return result;
  };
  // Retained synthetic Rust results: the frontend must display these integrals unchanged.
  const trapezoid=integration==='composite_trapezoid';
  return {schema:'ftx-vapor-coverage-history-report/1',request_sha256:'history-lock',path_sha256:'motion-lock',
    replay_request_json:replay,duration_s:1,integration,
    totals:trapezoid?{potential_energy_j:26.25,potential_mass_kg:0.0000105}:{potential_energy_j:17.5,potential_mass_kg:0.000007},
    initial:{time_s:0,result:sample(1,1,20,0.000008)},
    samples:[
      {time_s:0.25,dt_s:0.25,result:sample(2,0,40,0.000016),
        potential_energy_j:trapezoid?7.5:10,potential_mass_kg:trapezoid?0.000003:0.000004},
      {time_s:1,dt_s:0.75,result:sample(0.5,1.5,10,0.000004),
        potential_energy_j:trapezoid?18.75:7.5,potential_mass_kg:trapezoid?0.0000075:0.000003}
    ],latent_heat_coupled:false,physical_validation_claim:false};
}
function wetFilmHistoryReport() {
  const r=vaporHistoryReport();
  const first={starting_liquid_mass_kg:0.000003,supplied_mass_kg:0.00000025,
    potential_evaporation_mass_kg:0.000004,evaporated_mass_kg:0.00000325,unmet_demand_mass_kg:0.00000075,
    remaining_liquid_mass_kg:0,body_reservoir_latent_energy_j:8.125,mass_balance_error_kg:0};
  const last={starting_liquid_mass_kg:0,supplied_mass_kg:0.00000075,
    potential_evaporation_mass_kg:0.000003,evaporated_mass_kg:0.00000075,unmet_demand_mass_kg:0.00000225,
    remaining_liquid_mass_kg:0,body_reservoir_latent_energy_j:1.875,mass_balance_error_kg:0};
  const totals={starting_liquid_mass_kg:0.000003,supplied_mass_kg:0.000001,
    potential_evaporation_mass_kg:0.000007,evaporated_mass_kg:0.000004,unmet_demand_mass_kg:0.000003,
    remaining_liquid_mass_kg:0,body_reservoir_latent_energy_j:10,mass_balance_error_kg:0};
  r.wet_film={source:'<literal wet-film acquisition>',basis:'Explicit synthetic body-film model',
    body_mesh_sha256:'b'.repeat(64),textile_temperature_coupled:false,uncertainty_propagated:false,
    body:[{body_triangle:0,...totals}],totals};
  for (const [i,totals] of [first,last].entries()) r.samples[i].wet_film={body:[{body_triangle:0,...totals}],totals};
  r.replay_request_json=' {"schema":"ftx-vapor-coverage-history/1","policy":{"wet_film":{"initial_liquid_mass_kg":[0.0000030]}},"value":1.0,"x":-0.0}\n';
  return r;
}
function calibrationReport(mode = 'nonlinear', admission = false, check = true) {
  if (mode === 'membrane_plasticity') return permanentCalibration('ACCEPTED', admission, check);
  if (mode === 'clamped_strip') return stripCalibration('ACCEPTED', admission, check);
  if (['linear','bending','friction_pair','friction_self','friction_body'].includes(mode)) return measuredCalibration(mode,admission,check);
  const selection = calibrationSelection(admission), quality = { components: 12, rms_scaled: 0.02, peak_scaled: 0.04 };
  return { selection: JSON.parse(selection), selection_json: selection, fit_claim: false,
    material: { selection: JSON.parse(selection), selection_json: selection, snapshot: { id: 'identified' },
      capabilities: { mechanical_coefficients: admission ? {ready:true} : {ready:false,refusal:{message:'Fitted estimates need admission'}} } },
    calibration: { schema: mode === 'nonlinear' ? 'ftx-nonlinear-swatch-calibration-report/1' : 'ftx-relaxation-calibration-report/1',
      input: { orthotropic: mode === 'relaxation' ? {times_seconds:[0.3,2,8,32]} : undefined,
        strain_range: [[-0.1,0.2],[-0.2,0.3],[-0.4,0.4]], max_rms_scaled:0.05, max_peak_scaled:0.1 },
      fitted_strain_range: [[-0.1,0.2],[-0.2,0.3],[-0.4,0.4]],
      material: { membrane_relaxation: { orthotropic_branches: [{id:'warp'},{id:'shear'},{id:'weft'}] } },
      orthotropic_fit: mode === 'relaxation' ? {candidates:[
        {time_seconds:0.3,stiffness_n_m:[240,60,-30,0],active:true},
        {time_seconds:2,stiffness_n_m:[0,0,0,90],active:true},
        {time_seconds:8,stiffness_n_m:[0,120,0,0],active:true},
        {time_seconds:32,stiffness_n_m:[0,0,0,0],active:false}]} : undefined,
      fit: quality, check: check ? quality : null, input_sha256:'source-lock', missing_mechanics:[] } };
}
function measuredCalibration(mode, admission = false, check = true) {
  const result=calibrationReport('nonlinear',admission,check), c=result.calibration;
  if (mode==='linear') {
    c.schema='ftx-swatch-calibration-report/1'; c.stiffness_n_m=[[300,60,0],[60,180,0],[0,0,70]];
    c.excitation_pivot=0.02; return result;
  }
  if (mode==='bending') {
    c.schema='ftx-bending-calibration-report/1'; c.bending_nm=[[0.02,0.003,0],[0.003,0.01,0],[0,0,0.004]];
    c.preferred_curvature_m_inv=[0.5,-0.25,0.1]; c.excitation_pivot=0.03;
    c.input.fit_preferred_curvature=true; return result;
  }
  const kind={friction_pair:'pair',friction_self:'self_contact',friction_body:'body'}[mode];
  const coefficient={value:0.3,unit:'1',evidence:'estimate',source:'sliding-fit'};
  c.schema='ftx-friction-calibration-report/1'; c.coefficient=coefficient;
  c.fit_normal_n_range=[1,4];c.fit_speed_m_s_range=[0.01,0.04];
  c.input.target={kind,counterface:kind==='body'?{reference:'local:declared-surrogate',note:'Synthetic <counterface>'}:undefined};
  result.readiness={scope:'material_mechanics',ready:admission,whole_garment_checked:false};
  result.interface_json=null;result.binding={kind:'material',selection:result.selection};
  if (kind==='pair') {
    const interface_json=`{"materials":["left-lock","right-lock"],"friction":{"value":0.3,"bounds":[-0.0,1.0],"unit":"1","evidence":"estimate","source":"sliding-fit"},"sources":{"sliding-fit":{"reference":"local:synthetic-fit","note":"Synthetic <sliding evidence>"}},"allow_estimates":${admission}}`;
    result.interface_json=interface_json;
    result.binding={kind:'interface',materials:[{id:'left',expected_sha256:'left-lock'},{id:'right',expected_sha256:'right-lock'}],
      interface:JSON.parse(interface_json)};
    result.readiness={scope:'interface_only',ready:admission,
      refusal:admission?null:{message:'Interface estimates need their own admission'},whole_garment_checked:false,
      sheets:[{material_sha256:'left-lock',ready:false,refusal:{message:'Missing areal mass'}},{material_sha256:'right-lock',ready:true}]};
    result.selection=null;result.selection_json=null;result.material=null;result.simulation_ready=false;
  }
  result.binding_json=JSON.stringify(result.binding);
  return result;
}
function areaCalibrationReport() {
  const result=calibrationReport('nonlinear',true), c=result.calibration;
  const conditions={specimen:'synthetic <area specimen>',conditioning:'synthetic reference',temperature_c:'20'};
  const quantity=value=>({value,unit:'N/m',evidence:'estimate',source:'joint-area-fit',
    method:'Synthetic joint identification; <literal evidence>',conditions});
  c.input.area_response={model:'area_green_strain/1',area_ratio_range:[0.7,1.5]};
  c.fitted_area_ratio_range=[0.75,1.45];c.parameter_count=9;c.excitation_pivot=0.0015;
  c.material={membrane_response:{area_response:{model:'area_green_strain/1',quadratic:quantity(40),
    log_squared:quantity(15),area_ratio_range:[0.75,1.45],conditions}},
    sources:{'joint-area-fit':{reference:'local:synthetic-area-fit',note:'Synthetic source <retained>'}}};
  return result;
}
function recoveryComparison(status = 'AMBIGUOUS') {
  const selected = calibrationReport('relaxation', true);
  const comparison = { schema:'ftx-relaxation-model-comparison-report/1', status,
    objective_evaluations:2, projected_iterations:0,
    fit_decision:{equivalent_candidate_ids:['complete','extra'],ambiguous:true,
      parsimonious_candidate_id:'complete',chosen_candidate_id:status === 'AMBIGUOUS' ? null : 'complete'},
    candidates:['complete','extra'].map((id, i) => ({id,declared_parameters:12+4*i,
      fit_status:'ACCEPTED',calibration:selected.calibration,check_status:'ACCEPTED'})) };
  return status === 'ACCEPTED' ? {...selected,comparison} :
    {comparison,calibration:null,material:null,selection:null,selection_json:null,fit_claim:false};
}

function permanentCalibration(status = 'ACCEPTED', admission = false, check = true) {
  const selected = calibrationReport('nonlinear', admission, check), quality = selected.calibration.fit;
  const properties = Object.fromEntries(['warp','weft','shear'].flatMap(axis => [
    [`membrane_strain_limit_${axis}`,{value:0.25,unit:'1'}],
    [`membrane_accumulation_limit_${axis}`,{value:1,unit:'1'}]]));
  selected.calibration = {schema:'ftx-membrane-plasticity-calibration-report/1',status,
    input:{material:{properties},yield_bounds_n_m:[[1,20],[1,20],[1,10]],
      hardening_bounds_n_m:[[0,200],[0,100],[0,50]],max_rms_scaled:0.05,max_peak_scaled:0.1},
    parameters:{yield_n_m:[10,8,5],hardening_n_m:[100,50,25]},fit:quality,check:check?quality:null,
    check_status:check?(status==='REJECTED'?'REJECTED':'ACCEPTED'):'NOT_PROVIDED',check_refusal:null,
    decision_reasons:status==='ACCEPTED'?[]:['FIT coefficients are not independently resolved.'],
    input_sha256:'exact-cycle-source',predictions:[{plastic_strain:[0.03,0,0],stored_j_m2:2,plastic_dissipation_j_m2:1}],
    diagnostics:{starts_completed:4,starts_attempted:4,evaluations:400,evaluation_limit:4096,
      history_steps:3000,history_step_limit:2000000,trials_refused:1,excitation_pivot:0.2,sensitivity_evaluated:true}};
  if (status !== 'ACCEPTED') Object.assign(selected,{selection:null,selection_json:null,material:null});
  return selected;
}

function stripCalibration(status = 'ACCEPTED', admission = false, check = true) {
  const selected = calibrationReport('nonlinear', admission, check), quality = selected.calibration.fit;
  selected.calibration = {schema:'ftx-strip-calibration-report/1',accepted:status==='ACCEPTED',
    input:{method:'Explicit synthetic midpoint chord; <literal evidence>',
      parameter_bounds:[[100,500],[100,300],[0.05,0.4],[10,100]],
      policy:{max_forward_solves:8000,max_mesh_scaled:1},
      experiments:[{id:'strip-source',partition:'fit',
        fixture:{width_m:0.06,gauge_length_m:0.1,warp_axis:[0.6,0.8],cells:[2,4]},
        observations:[{mid_width:{value_m:0.059,scale_m:0.001}}]}]},
    parameters:[300,180,0.2,70],fit:quality,check:check?quality:null,
    fitted_green_strain_range:[[-0.02,0.1],[-0.03,0.12],[-0.1,0.1]],input_sha256:'strip-source-lock',
    reasons:status==='ACCEPTED'?[]:[`${status}: The declared strip observations do not resolve this law.`],
    starts:[{converged:true},{converged:true},{converged:true}],excitation_pivot:0.1,
    forward_solves:600,mesh_change_peak_scaled:0.08,domain_replay_verified:status==='ACCEPTED',
    predictions:[{experiment:'strip-source',observation:'point-1',partition:'fit',
      state:{extension_fraction:0.05,force_n:1.2,mid_width_m:0.059},force_residual_n:0.01,width_residual_m:-0.0001}]};
  if (status !== 'ACCEPTED') Object.assign(selected,{selection:null,selection_json:null,material:null});
  return selected;
}

function nonlinearStripCalibration(status = 'ACCEPTED') {
  const result = stripCalibration(status,true), c = result.calibration;
  const curve = {points:[[-0.1,-10],[0,0],[0.05,8],[0.1,24]],source:'declared-shape',evidence:'estimate',
    method:'Synthetic supplied shape; <literal source>',conditions:{specimen_id:'synthetic'}};
  c.input.response_family = {model:'scaled_piecewise_linear/1',warp:curve,weft:curve,shear:curve,allow_estimated_shapes:true};
  c.input.parameter_bounds = [[0.5,2],[0.4,1.2],[0.5,3],[-30,30]];
  c.parameters = [1.25,0.75,2,-12];
  c.parameter_descriptors = [
    {id:'warp_scale',label:'Warp shape scale',unit:'1'},
    {id:'weft_scale',label:'Weft shape scale',unit:'1'},
    {id:'shear_scale',label:'Shear shape scale',unit:'1'},
    {id:'normal_coupling',label:'Normal coupling',unit:'N/m'}
  ];
  return result;
}

async function workerModule({ failFold = false, failExport = false, trap = false, rejectLoad = false,
  failScenario = false, trapScenario = false, bindings = null } = {}) {
  const sent = [], called = [], freed = [];
  const imports = bindings || {
    default: () => rejectLoad ? Promise.reject(Error('bundle unavailable')) : Promise.resolve(),
    material_catalog: q => JSON.stringify({ materials: [], query: q }),
    material_inspect: s => JSON.stringify({ selection: JSON.parse(s) }),
    material_pressure_condition: r => {
      called.push(['pressure', r]);
      const selection = '{"id":"compressed","allow_estimates":true,"value":-0.0}';
      return `{"selection":${selection},"selection_json":${JSON.stringify(selection)},"input":{"pressure_pa":250.0},"interpolated":true}`;
    },
    material_calibrate_nonlinear: (r, a) => { called.push(['calibrate-nonlinear',r,a]); return JSON.stringify(calibrationReport('nonlinear',a)); },
    material_calibrate_linear: (r, a) => { called.push(['calibrate-linear',r,a]); return JSON.stringify(calibrationReport('linear',a)); },
    material_calibrate_bending: (r, a) => { called.push(['calibrate-bending',r,a]); return JSON.stringify(calibrationReport('bending',a)); },
    material_calibrate_friction: (r, a) => { called.push(['calibrate-friction',r,a]);
      const mode={pair:'friction_pair',self_contact:'friction_self',body:'friction_body'}[JSON.parse(r).target.kind];
      return JSON.stringify(calibrationReport(mode,a)); },
    material_calibrate_relaxation: (r, a) => { called.push(['calibrate-relaxation',r,a]); return JSON.stringify(calibrationReport('relaxation',a)); },
    material_calibrate_membrane_plasticity: (r, a) => { called.push(['calibrate-membrane_plasticity',r,a]); return JSON.stringify(calibrationReport('membrane_plasticity',a)); },
    material_calibrate_strip: (r, a) => { called.push(['calibrate-clamped_strip',r,a]); return JSON.stringify(calibrationReport('clamped_strip',a)); },
    material_screen: r => { called.push(['screen', r]); return '{"alternatives":[],"coefficient":1.0}'; },
    material_study: (p, r) => { called.push(['study', p, r]); return '{"alternatives":[],"coordinate":-0.0}'; },
    material_scenario_study: (b, p, r, s, m) => {
      called.push(['scenario', b, p, r, s, m]);
      if (trapScenario) throw new WebAssembly.RuntimeError('scenario trap');
      if (failScenario) throw Error('scenario refused');
      return '{"manufacturing":{"alternatives":[]},"simulations":[{"key":"A","evaluation":{"status":"refused","refusal":{"message":"missing mechanics"}}}],"coordinate":-0.0}';
    },
    material_bind: (p, c) => { called.push(['bind', p, c]); return JSON.stringify({ pattern: JSON.parse(p), pattern_json:p }); },
    WebBody: class { constructor() { called.push(['body']); } free() { freed.push('body'); } },
    fold_up: (_b, p, m, f, contact) => {
      called.push(['fold', p, m, f, contact]);
      if (trap) throw new WebAssembly.RuntimeError('synthetic trap');
      return { report: () => { if (failFold) throw Error('report refused'); return '{"material":"new","coordinate":-0.0,"coefficient":1.0}'; },
        viewer: () => '<!doctype html>', free: () => freed.push('fold') };
    },
    thermal_steady_surface: (r, c) => { called.push(['heat', r, c]); return '{}'; },
    thermal_steady_coverage: r => { called.push(['coverage', JSON.parse(r), r]); return '{}'; },
    thermal_vapor: r => { called.push(['vapor',r]); return JSON.stringify(vaporReport()); },
    cut_plan: (p, o) => { called.push(['cut', p, o]); return '{"rolls":[{},{}]}'; },
    cut_svg: (p, o, i) => { called.push(['svg', p, o, i]); return '<svg/>'; },
    stock_plan: (p, r) => { called.push(['stock', p, r]); return '{"markers":[{"placements":[]},{"placements":[{}]}],"stock_value":1.0}'; },
    stock_residual: (p, r) => { called.push(['stock-residual', p, r]); return '{"schema":"ftx-finite-stock-request/1","mass":1.0}'; },
    stock_svg: (p, r, i) => { called.push(['stock-svg', p, r, i]); return '<svg/>'; },
    cut_price: (...args) => { called.push(['price', ...args]); return '{"complete_offer_coverage":false}'; },
    sew_stitches: (p, o) => { called.push(['sew', p, o]); return '{"stitches":[]}'; },
    run: (r, d, t) => {
      called.push(['exports', JSON.parse(r), d, t]);
      return { page: () => failExport ? '{"status":"BLOCKED","errors":["invalid"]}' : '{"status":"REVIEW"}',
        zip: () => new Uint8Array([1, 2]), zip_name: () => 'round.zip', free: () => freed.push('round') };
    }
  };
  const context = vm.createContext({ Uint8Array, TextEncoder, WebAssembly, JSON, Error, self: {}, postMessage: m => sent.push(m) });
  const stub = new vm.SyntheticModule(Object.keys(imports), function () {
    for (const [name, value] of Object.entries(imports)) this.setExport(name, value);
  }, { context });
  const module = new vm.SourceTextModule(read('materials-worker.js'), { context });
  await module.link(() => stub); await module.evaluate();
  const send = async data => {
    // Real wasm-bindgen imports execute in Node's module realm, while this worker is a VM realm.
    // Forward its worker-global progress import to the actual worker callback for this invocation.
    const previous = globalThis.ftxProgress;
    if (bindings) globalThis.ftxProgress = line => context.self.ftxProgress(line);
    try { await context.self.onmessage({ data }); return sent.at(-1); }
    finally {
      if (bindings) {
        if (previous === undefined) delete globalThis.ftxProgress;
        else globalThis.ftxProgress = previous;
      }
    }
  };
  return { send, sent, called, freed };
}

test('treatment worker preserves exact requests, reports, selections and derived pattern bytes', async () => {
  const calls=[], bindings={default:()=>Promise.resolve()}, source=' {"pieces":[],"x":-0.0,"n":1.0}\n';
  for (const pattern of [false,true]) {
    const report=treatmentReport(pattern), raw=` ${JSON.stringify(report)}\n`;
    bindings[pattern?'material_treatment_pattern':'material_treatment']=(...args)=>{calls.push(args);return raw;};
  }
  const w=await workerModule({bindings});
  for (const pattern of [false,true]) {
    const schema=pattern?'ftx-pattern-treatment/1':'ftx-material-treatment/1',
      treatmentRequest=` {"schema":"${schema}","x":-0.0,"n":1.0}\n`, report=treatmentReport(pattern);
    const result=await w.send({type:'treatment',request:191,pattern:source,treatmentRequest});
    assert.equal(result.type,'result',result.message);
    assert.deepEqual(calls.at(-1),pattern?[source,treatmentRequest]:[treatmentRequest]);
    assert.deepEqual(plain(result.files.map(f=>[f.name,f.text])),pattern?[
      ['treatment-report.json',` ${JSON.stringify(report)}\n`],['treatment-source-pattern.json',report.source_pattern_json],
      ['treatment-request.json',report.frozen_request_json],['treat-pattern.json',report.pattern_json]]:[
      ['treatment-report.json',` ${JSON.stringify(report)}\n`],['treatment-request.json',treatmentRequest],
      ['before-treatment-selection.json',report.before_selection_json],['after-treatment-selection.json',report.after_selection_json]]);
  }
});

test('treatment dispatch refuses unknown schemas, old WASM and incomplete frozen results without partial files', async () => {
  for (const [schema,bindings,pattern,reason] of [
    ['ftx-material-treatment-report/1',{},'{}',/Choose a material treatment/],
    ['ftx-material-treatment/1',{},'{}',/missing material treatment engine/],
    ['ftx-pattern-treatment/1',{},'{}',/missing pattern treatment engine/],
    ['ftx-pattern-treatment/1',{},undefined,/Load a pattern/],
    ['ftx-material-treatment/1',{material_treatment:()=>'{}'},'{}',/missing frozen treatment selections/],
    ['ftx-pattern-treatment/1',{material_treatment_pattern:()=>'{}'},'{}',/incomplete frozen treatment result/]
  ]) {
    const w=await workerModule({bindings:{default:()=>Promise.resolve(),...bindings}}),
      result=await w.send({type:'treatment',request:192,pattern,treatmentRequest:JSON.stringify({schema})});
    assert.equal(result.type,'error'); assert.match(result.message,reason);
    assert.equal(result.request,192); assert.equal(result.files,undefined);
    assert.equal(w.sent.some(m=>m.type==='result'),false);
  }
});

test('material worker calls the shared consumers with exact current inputs and complete outputs', async () => {
  const w = await workerModule();
  const base = { request: 1, pattern: '{"pieces":[]}', options: '{"copies":2}', report: '{}', conditions: '{}', map: '{}', quotes: '{}', market: '{}' };
  for (const type of ['bind', 'heat', 'coverage', 'cut', 'price', 'sew']) {
    const r = await w.send({ ...base, type, changes: '{}' });
    assert.equal(r.type, 'result'); assert.equal(r.request, 1); assert.ok(r.files.length);
  }
  assert.deepEqual(plain(w.called.find(c => c[0] === 'coverage')[1]), {
    schema: 'ftx-steady-coverage-request/1', report: {}, map: {}, conditions: {}
  });
  assert.deepEqual(w.called.find(c => c[0] === 'price'), ['price', base.pattern, base.options, '{}', '{}']);
  assert.equal(w.called.filter(c => c[0] === 'svg').length, 1); // bounded first-roll preview
  const r = await w.send({ ...base, type: 'exports', fit: '{}', measurements: '{}', today: '2026-10-15', created: '2026-10-15T12:00' });
  assert.equal(r.files[0].name, 'round.zip'); assert.deepEqual([...r.files[0].bytes], [1, 2]);
  assert.deepEqual(w.freed, ['round']);
});
test('vapor worker retains the original request, exact Rust report and frozen replay bytes', async () => {
  const w=await workerModule(), vaporRequest=' {"schema":"ftx-vapor-stack/1","area_m2":1.0,"value":-0.0}\n';
  const result=await w.send({type:'vapor',request:41,vaporRequest});
  assert.equal(result.type,'result',result.message);
  assert.deepEqual(w.called,[['vapor',vaporRequest]]);
  assert.deepEqual(plain(result.files.map(f=>[f.name,f.text])),[
    ['vapor-report.json',JSON.stringify(vaporReport())],['vapor-request.json',vaporRequest],
    ['vapor-replay-request.json',vaporReport().replay_request_json]]);
  assert.equal(result.files.length,3); assert.deepEqual(w.freed,[]);
});
test('vapor worker dispatches stack, layered coverage and history schemas without rewriting bytes', async () => {
  const cases=[['ftx-vapor-stack/1','thermal_vapor',vaporReport()],
    ['ftx-vapor-coverage/1','thermal_vapor_coverage',vaporCoverageReport()],
    ['ftx-vapor-coverage-history/1','thermal_vapor_coverage_history',vaporHistoryReport()]];
  const calls=[], bindings={default:()=>Promise.resolve()};
  for (const [,engine,report] of cases) bindings[engine]=input=>{
    calls.push([engine,input]); return ` ${JSON.stringify(report)}\n`;
  };
  const w=await workerModule({bindings});
  for (const [schema,engine,report] of cases) {
    const vaporRequest=` {"schema":"${schema}","value":1.0,"x":-0.0}\n`;
    const result=await w.send({type:'vapor',request:42,vaporRequest});
    assert.equal(result.type,'result',result.message); assert.equal(result.request,42);
    assert.deepEqual(calls.at(-1),[engine,vaporRequest]);
    assert.deepEqual(plain(result.files.map(f=>[f.name,f.text])),[
      ['vapor-report.json',` ${JSON.stringify(report)}\n`],['vapor-request.json',vaporRequest],
      ['vapor-replay-request.json',report.replay_request_json]]);
    assert.equal(result.result.schema,report.schema);
  }
  assert.equal(calls.length,cases.length);
});
test('vapor dispatch refuses unknown schemas, unavailable engines and incomplete histories without partial files', async () => {
  let fallbackCalls=0;
  const w=await workerModule({bindings:{default:()=>Promise.resolve(),
    thermal_vapor:()=>{fallbackCalls++;return JSON.stringify(vaporReport());}}});
  for (const [schema,reason] of [
    ['ftx-vapor-coverage-history-report/1',/Choose a vapor stack/],
    ['ftx-vapor-coverage/1',/missing requested vapor engine/],
    ['ftx-vapor-coverage-history/1',/missing requested vapor engine/]
  ]) {
    const result=await w.send({type:'vapor',request:43,vaporRequest:JSON.stringify({schema})});
    assert.equal(result.type,'error'); assert.match(result.message,reason);
    assert.equal(result.request,43); assert.equal(result.files,undefined);
  }
  assert.equal(fallbackCalls,0); assert.equal(w.sent.some(m=>m.type==='result'),false);
  for (const fail of [false,true]) {
    const failed=await workerModule({bindings:{default:()=>Promise.resolve(),thermal_vapor_coverage_history:()=>{
      if(fail) throw Error('SOURCE_MISMATCH: retained material changed');
      return '{"schema":"ftx-vapor-coverage-history-report/1"}';
    }}});
    const result=await failed.send({type:'vapor',request:44,vaporRequest:'{"schema":"ftx-vapor-coverage-history/1"}'});
    assert.equal(result.type,'error');
    assert.match(result.message,fail?/SOURCE_MISMATCH/:/frozen vapor replay request/);
    assert.equal(result.files,undefined); assert.equal(failed.sent.some(m=>m.type==='result'),false);
  }
});
test('bounded-marker worker previews the selected intact marker and refuses invalid indices', async () => {
  const w = await workerModule(), pattern = '{"origin":-0.0}',
    options = '{"copies":3,"maximum_marker_length_m":0.12,"edge_margin_mm":-0.0}';
  const result = await w.send({type:'cut',request:9,pattern,options,rollIndex:1});
  assert.equal(result.type,'result',result.message);
  assert.deepEqual(w.called.find(c=>c[0]==='cut'),['cut',pattern,options]);
  assert.deepEqual(w.called.find(c=>c[0]==='svg'),['svg',pattern,options,1]);
  assert.equal(result.files.length,2);
  assert.equal(result.files[0].name,'cut-plan.json');
  assert.equal(result.files[0].text,'{"rolls":[{},{}]}');
  assert.equal(result.files[1].name,'roll-1.svg');
  assert.equal(result.picture,'<svg/>');
  for (const rollIndex of [-1,2,0.5,Infinity,Number.MAX_SAFE_INTEGER+1]) {
    const refused = await w.send({type:'cut',pattern,options,rollIndex});
    assert.equal(refused.type,'error');assert.match(refused.message,/between 1 and 2/);
  }
  assert.equal(w.called.filter(c=>c[0]==='svg').length,1);
  assert.equal(w.sent.filter(m=>m.type==='result').length,1);
});
test('material worker frees model and fold on success or report failure, without caching an old drape', async () => {
  for (const failFold of [false, true]) {
    const w = await workerModule({ failFold });
    const r = await w.send({ type: 'fold', request: 4, bytes: new ArrayBuffer(8), pattern: '{}', measurements: '{}', fit: '{}', selfContactMm: 2 });
    assert.equal(r.type, failFold ? 'error' : 'result');
    assert.deepEqual(w.freed, ['fold', 'body']);
    assert.equal((await w.send({ type: 'coverage', report: 'invalid', map: '{}', conditions: '{}' })).type, 'error');
    assert.equal(w.sent.filter(m => m.type === 'result').length, failFold ? 0 : 1);
  }
});
test('material worker rejects blocked exports, initialization failure and further work after a WASM trap', async () => {
  const e = await workerModule({ failExport: true });
  assert.equal((await e.send({ type: 'exports', pattern: '{}', fit: '{}', measurements: '{}' })).type, 'error');
  assert.deepEqual(e.freed, ['round']);
  const w = await workerModule({ trap: true });
  assert.equal((await w.send({ type: 'fold', bytes: new ArrayBuffer(8) })).fatal, true);
  assert.deepEqual(w.freed, ['body']);
  assert.equal((await w.send({ type: 'catalog', query: '' })).type, 'error');
  const bad = await workerModule({ rejectLoad: true });
  await bad.send({ type: 'catalog', query: '' });
  assert.ok(bad.sent.some(m => m.type === 'loadError')); assert.ok(!bad.sent.some(m => m.type === 'result'));
});
test('worker retains numerical JSON identities across binding, drape files and nested thermal requests', async () => {
  const w=await workerModule(), pattern='{"pieces":[],"coordinate":-0.0,"coefficient":1.0}';
  const bind=await w.send({type:'bind',pattern,changes:'{}'});
  assert.equal(bind.files[0].text,pattern);
  assert.equal(bind.files[2].name,'material-changes.json');
  assert.equal(bind.files[2].text,'{}');
  assert.equal(bind.files[3].text,pattern);
  const fold=await w.send({type:'fold',bytes:new ArrayBuffer(8)});
  assert.equal(fold.reportText,fold.files[0].text);assert.match(fold.reportText,/-0\.0/);
  await w.send({type:'coverage',report:fold.reportText,map:'{}',conditions:'{}'});
  assert.ok(w.called.find(c=>c[0]==='coverage')[2].includes(fold.reportText));
});

test('finite stock uses unmodified source text and previews the first allocated physical item', async () => {
  const w = await workerModule(), pattern = '{"coordinate":-0.0}', stockRequest = '{"mass":1.0}';
  const r = await w.send({ type: 'stock', pattern, stockRequest, request: 11 });
  assert.equal(r.type, 'result'); assert.equal(r.files[1].name, 'residual-stock-request.json'); assert.equal(r.files[2].name, 'stock-1.svg');
  assert.deepEqual(w.called.find(c => c[0] === 'stock'), ['stock', pattern, stockRequest]);
  assert.deepEqual(w.called.find(c => c[0] === 'stock-residual'), ['stock-residual', pattern, stockRequest]);
  assert.deepEqual(w.called.find(c => c[0] === 'stock-svg'), ['stock-svg', pattern, stockRequest, 1]);
  assert.match(r.files[0].text, /1\.0/); assert.match(r.files[1].text, /1\.0/);
});

test('screening and studies call production entry points and download unmodified source/request text', async () => {
  const w = await workerModule(), pattern = '{"x":-0.0}', request = '{"coefficient":1.0}';
  const screen = await w.send({ type: 'screen', request: 12, screenRequest: request });
  assert.equal(screen.type, 'result'); assert.equal(screen.request, 12);
  assert.deepEqual(w.called.at(-1), ['screen', request]);
  assert.equal(screen.files[1].text, request); assert.match(screen.files[0].text, /1\.0/);
  const study = await w.send({ type: 'study', pattern, studyRequest: request });
  assert.equal(study.type, 'result'); assert.deepEqual(w.called.at(-1), ['study', pattern, request]);
  assert.equal(study.files[1].text, pattern); assert.equal(study.files[2].text, request);
  assert.match(study.files[0].text, /-0\.0/);
});

test('design studies use the canonical study APIs and retain exact source, operations and report bytes', async () => {
  const schemas = [
    ['ftx-design-study-report/1', 'study', 'design-study.json'],
    ['ftx-design-scenario-study/1', 'scenario', 'design-scenario-study.json'],
    ['ftx-design-scenario-matrix-study/1', 'scenario', 'design-scenario-matrix-study.json']
  ];
  for (const [schema, type, name] of schemas) {
    const calls = [], freed = [], raw = ` {"schema":"${schema}","coefficient":1.0,"x":-0.0}\n`;
    const w = await workerModule({ bindings: { default: () => Promise.resolve(),
      WebBody: class { free() { freed.push('body'); } },
      material_study: (...args) => { calls.push(args); return raw; },
      material_scenario_study: (_body, ...args) => { calls.push(args); return raw; } } });
    const msg = { type, bytes: new ArrayBuffer(8), pattern: ' {"x":-0.0}\n',
      studyRequest: ' {"schema":"ftx-design-study/1","designs":[{"key":"wider","operations":[{"op":"move","dx_mm":1.0}]}]}\n',
      scenarioRequest: '{"temperature":293.0}', measurements: '{"length":10.0}' };
    const result = await w.send(msg);
    assert.equal(result.type, 'result', result.message);
    assert.deepEqual(calls, [type === 'study' ? [msg.pattern, msg.studyRequest] :
      [msg.pattern, msg.studyRequest, msg.scenarioRequest, msg.measurements]]);
    assert.deepEqual(freed, type === 'study' ? [] : ['body']);
    assert.deepEqual(plain(result.files.slice(0, 3).map(f => [f.name, f.text])),
      [[name, raw], ['original-pattern.json', msg.pattern], ['study-request.json', msg.studyRequest]]);
  }
});

test('selected-body manufacturing uses the canonical model API, frees handles and never falls back', async () => {
  const calls=[],freed=[],raw=' {"schema":"ftx-design-study-report/1","alternatives":[],"x":-0.0}\n';
  let fail=false;
  const w=await workerModule({bindings:{default:async()=>{},
    WebBody:class {free(){freed.push('body');}},
    material_study:()=>{throw Error('must not fall back');},
    material_study_on_body:(_model,...args)=>{calls.push(args);if(fail)throw Error('fit refused');return raw;}}});
  const msg={type:'study',request:42,onBody:true,bytes:new ArrayBuffer(8),
    pattern:' {"p":1.0}',studyRequest:' {"v":-0.0}',measurements:' {"schema":"ftx-scenario-body/1"}\n'};
  const result=await w.send(msg);assert.equal(result.type,'result');
  assert.deepEqual(calls[0],[msg.pattern,msg.studyRequest,msg.measurements]);
  assert.equal(result.files.find(f=>f.name==='design-study.json').text,raw);
  assert.equal(result.files.find(f=>f.name==='body-input.json').text,msg.measurements);
  assert.deepEqual(freed,['body']);
  fail=true;assert.match((await w.send(msg)).message,/fit refused/);assert.deepEqual(freed,['body','body']);
  assert.match((await w.send({...msg,onBody:'yes'})).message,/boolean/);
});

test('pressure conditioning preserves exact request and frozen selection through the shared worker', async () => {
  const w = await workerModule(), request = '{"pressure_pa":250.0,"coordinate":-0.0}';
  const result = await w.send({ type: 'pressure', request: 24, compressionRequest: request });
  assert.equal(result.type, 'result');
  assert.deepEqual(w.called.at(-1), ['pressure', request]);
  assert.equal(result.files[2].name, 'compression-request.json');
  assert.equal(result.files[2].text, request);
  assert.equal(result.files[1].text, result.result.selection_json);
  assert.match(result.files[1].text, /-0\.0/);
  assert.equal(result.result.inspection.selection.id, 'compressed');
});

test('compression histories use the shared heat API and preserve every final layer selection', async () => {
  for (const version of [1, 2]) {
    const request = ` {"schema":"ftx-compressed-thermal-history/${version}","source_value":-0.0}\n`;
    const selections = ['{"id":"inner","value":-0.0}', '{"id":"outer","coefficient":1.0}'];
    const raw = JSON.stringify({schema:`ftx-compressed-thermal-history-report/${version}`,
      final_compression:{layers:selections.map((s,i)=>({id:['inner','outer'][i],
        conditioned:{selection:JSON.parse(s),selection_json:s}}))},steps:[]});
    const calls = [], w = await workerModule({bindings:{default:()=>Promise.resolve(),
      thermal_compression_history:r=>{calls.push(r);return raw;},
      material_inspect:s=>JSON.stringify({selection:JSON.parse(s)})}});
    const result = await w.send({type:'pressure',compressionRequest:request,compressionLayer:'outer',request:27});
    assert.equal(result.type,'result',result.message); assert.equal(result.request,27);
    assert.deepEqual(calls,[request]);
    assert.equal(result.result.chosen_layer_id,'outer');
    assert.equal(result.result.selection_json,selections[1]);
    assert.equal(result.result.inspection.selection.id,'outer');
    assert.deepEqual(plain(result.files.map(f=>f.name)),['compression-thermal-report.json','compression-request.json',
      'compression-layer-1-selection.json','compression-layer-2-selection.json']);
    assert.deepEqual(plain(result.files.map(f=>f.text)),[raw,request,...selections]);
    assert.equal((await w.send({type:'pressure',compressionRequest:request})).type,'error');
    assert.equal(calls.length,1);
    const bad = await w.send({type:'pressure',compressionRequest:request,compressionLayer:'missing'});
    assert.equal(bad.type,'error'); assert.equal(bad.files,undefined);
  }
});

test('calibration worker routes all shared engines with explicit consent and lossless reusable selections', async () => {
  const w = await workerModule(), input = '{"coefficient":1.0,"origin":-0.0}';
  for (const mode of ['linear','bending','nonlinear','relaxation','membrane_plasticity','clamped_strip']) for (const allowEstimates of [false,true]) {
    const result = await w.send({type:'calibrate',calibrationMode:mode,calibrationRequest:input,allowEstimates,request:19});
    assert.equal(result.type,'result'); assert.equal(result.request,19);
    assert.deepEqual(w.called.at(-1),['calibrate-'+mode,input,allowEstimates]);
    assert.equal(result.result.material.capabilities.mechanical_coefficients.ready,allowEstimates);
    assert.equal(result.files[0].text,JSON.stringify(calibrationReport(mode,allowEstimates)));
    assert.equal(result.files[1].text,calibrationSelection(allowEstimates));
    assert.equal(result.files[2].text,input);
  }
  for (const bad of [{calibrationMode:'arbitrary',allowEstimates:false},{calibrationMode:'nonlinear',allowEstimates:'true'}]) {
    const count=w.called.length;
    assert.equal((await w.send({type:'calibrate',calibrationRequest:input,...bad})).type,'error');
    assert.equal(w.called.length,count);
  }
  const incomplete=await workerModule({bindings:{default:()=>Promise.resolve(),material_calibrate_nonlinear:()=>'{"selection":{"id":"old"}}'}});
  assert.match((await incomplete.send({type:'calibrate',calibrationMode:'nonlinear',allowEstimates:false,calibrationRequest:input})).message,/selection and capability bridge/);
});

test('friction worker distinguishes exact pairs, self contact and body counterfaces without rewriting evidence', async () => {
  const w=await workerModule();
  for (const [mode,kind] of [['friction_pair','pair'],['friction_self','self_contact'],['friction_body','body']]) {
    for (const allowEstimates of [false,true]) {
      const input=` {"target":{"kind":"${kind}"},"source":-0.0,"scale":1.0}\n`;
      const result=await w.send({type:'calibrate',calibrationMode:mode,calibrationRequest:input,allowEstimates,request:71});
      const report=calibrationReport(mode,allowEstimates);
      assert.equal(result.type,'result',result.message);assert.equal(result.request,71);
      assert.deepEqual(w.called.at(-1),['calibrate-friction',input,allowEstimates]);
      assert.equal(result.files[0].text,JSON.stringify(report));
      assert.equal(result.files.find(f=>f.name==='friction-binding.json').text,report.binding_json);
      assert.equal(result.files.at(-1).text,input);
      if (kind==='pair') {
        assert.equal(result.result.selection,null);
        assert.equal(result.files.some(f=>f.name==='calibrated-selection.json'),false);
        assert.equal(result.files.find(f=>f.name==='calibrated-interface.json').text,report.interface_json);
      } else assert.equal(result.files.find(f=>f.name==='calibrated-selection.json').text,calibrationSelection(allowEstimates));
    }
    const count=w.called.length;
    const mismatch=await w.send({type:'calibrate',calibrationMode:mode,calibrationRequest:'{"target":{"kind":"unknown"}}',allowEstimates:true});
    assert.equal(mismatch.type,'error');assert.match(mismatch.message,/target.kind/);assert.equal(w.called.length,count);
  }
  const malformed={...calibrationReport('friction_pair',true),selection_json:calibrationSelection(true)};
  const bad=await workerModule({bindings:{default:()=>Promise.resolve(),material_calibrate_friction:()=>JSON.stringify(malformed)}});
  const result=await bad.send({type:'calibrate',calibrationMode:'friction_pair',calibrationRequest:'{"target":{"kind":"pair"}}',allowEstimates:true});
  assert.equal(result.type,'error');assert.match(result.message,/cannot replace a material selection/);
});

test('interface staging worker preserves original pattern, pending edits and exact pair through the canonical merger', async () => {
  const calls=[], pair=calibrationReport('friction_pair',true).interface_json;
  const interfaces=`[{"materials":["retained","pair"],"value":-0.0},${pair}]`;
  const report={interfaces_json:interfaces,changes_json:`{"interfaces":${interfaces},"value":1.0}`,geometry_changed:false};
  const w=await workerModule({bindings:{default:()=>Promise.resolve(),material_stage_interface:(...args)=>{
    calls.push(args);return JSON.stringify(report);
  }}});
  const base={type:'stageInterface',pattern:'{"coordinate":-0.0}',changes:'{"default":null,"value":1.0}',interfaceJson:pair,request:72};
  for (const replaceExisting of [false,true]) {
    const result=await w.send({...base,replaceExisting});
    assert.equal(result.type,'result',result.message);assert.equal(result.request,72);
    assert.deepEqual(calls.at(-1),[base.pattern,base.changes,pair,replaceExisting]);
    assert.equal(result.result.interfaces_json,interfaces);assert.equal(result.result.changes_json,report.changes_json);
  }
  assert.equal((await w.send({...base,replaceExisting:'true'})).type,'error');assert.equal(calls.length,2);
  const bad=await workerModule({bindings:{default:()=>Promise.resolve(),material_stage_interface:()=>'{}'}});
  assert.match((await bad.send({...base,replaceExisting:false})).message,/incomplete staged interface/);
});

test('unresolved recovery comparisons preserve reports and source without publishing a selection artifact', async () => {
  const input='{"schema":"ftx-relaxation-model-comparison/1","origin":-0.0}';
  for (const status of ['AMBIGUOUS','CHOSEN_CANDIDATE_REJECTED','BUDGET_STOP','ACCEPTED']) {
    const report=recoveryComparison(status), raw=JSON.stringify(report);
    const w=await workerModule({bindings:{default:()=>Promise.resolve(),material_calibrate_relaxation:()=>raw}});
    const result=await w.send({type:'calibrate',calibrationMode:'relaxation',calibrationRequest:input,allowEstimates:true});
    assert.equal(result.type,'result',result.message);
    assert.equal(result.files[0].text,raw); assert.equal(result.files.at(-1).text,input);
    assert.equal(result.files.some(f=>f.name === 'calibrated-selection.json'),status === 'ACCEPTED');
  }
  const invalid={...recoveryComparison(),selection_json:calibrationSelection(true)};
  const w=await workerModule({bindings:{default:()=>Promise.resolve(),material_calibrate_relaxation:()=>JSON.stringify(invalid)}});
  const result=await w.send({type:'calibrate',calibrationMode:'relaxation',calibrationRequest:input,allowEstimates:true});
  assert.equal(result.type,'error'); assert.match(result.message,/cannot publish a selection/);
});

test('permanent-stretch worker keeps rejected and ambiguous histories without selectable output', async () => {
  const input='{"schema":"ftx-membrane-plasticity-calibration/1","origin":-0.0}';
  for (const status of ['ACCEPTED','AMBIGUOUS','BUDGET_EXHAUSTED','REJECTED']) {
    const report=permanentCalibration(status,true), raw=JSON.stringify(report);
    const w=await workerModule({bindings:{default:()=>Promise.resolve(),material_calibrate_membrane_plasticity:()=>raw}});
    const result=await w.send({type:'calibrate',calibrationMode:'membrane_plasticity',calibrationRequest:input,allowEstimates:true});
    assert.equal(result.type,'result',result.message);
    assert.equal(result.files[0].text,raw); assert.equal(result.files.at(-1).text,input);
    assert.equal(result.files.some(f=>f.name==='calibrated-selection.json'),status==='ACCEPTED');
  }
  const invalid={...permanentCalibration('REJECTED',true),selection_json:calibrationSelection(true)};
  const w=await workerModule({bindings:{default:()=>Promise.resolve(),material_calibrate_membrane_plasticity:()=>JSON.stringify(invalid)}});
  const result=await w.send({type:'calibrate',calibrationMode:'membrane_plasticity',calibrationRequest:input,allowEstimates:true});
  assert.equal(result.type,'error'); assert.match(result.message,/cannot publish a selection/);
});

test('clamped-strip worker preserves unresolved physical acquisitions without a selectable material', async () => {
  const input='{"schema":"ftx-strip-calibration/1","origin":-0.0}';
  for (const status of ['ACCEPTED','AMBIGUOUS','REJECTED']) {
    const raw=JSON.stringify(stripCalibration(status,true));
    const w=await workerModule({bindings:{default:()=>Promise.resolve(),material_calibrate_strip:()=>raw}});
    const result=await w.send({type:'calibrate',calibrationMode:'clamped_strip',calibrationRequest:input,allowEstimates:true});
    assert.equal(result.type,'result',result.message);
    assert.equal(result.files[0].text,raw);assert.equal(result.files.at(-1).text,input);
    assert.equal(result.files.some(f=>f.name==='calibrated-selection.json'),status==='ACCEPTED');
  }
  const invalid={...stripCalibration('AMBIGUOUS',true),selection_json:calibrationSelection(true)};
  const w=await workerModule({bindings:{default:()=>Promise.resolve(),material_calibrate_strip:()=>JSON.stringify(invalid)}});
  assert.match((await w.send({type:'calibrate',calibrationMode:'clamped_strip',calibrationRequest:input,allowEstimates:true})).message,/cannot publish a selection/);
});

test('strip worker preserves original acquisition and experiment requests without constructing measurements', async () => {
  const input='{"source":-0.0,"value":1.0,"allow_estimates":false}', calls=[];
  const raw='{"source_value":1.0,"origin":-0.0}';
  const w=await workerModule({bindings:{default:()=>Promise.resolve(),
    material_strip_acquisition:(...args)=>{calls.push(['acquisition',...args]);return raw;},
    material_strip_experiment:(...args)=>{calls.push(['experiment',...args]);return raw;}}});
  for (const [type, name] of [['stripAcquisition','acquisition'],['stripExperiment','experiment']]) {
    const result=await w.send({type,stripRequest:input,request:33});
    assert.equal(result.type,'result',result.message); assert.equal(result.request,33);
    assert.deepEqual(calls.at(-1),[name,input]);
    assert.equal(result.files.length,2); assert.equal(result.files[0].text,raw); assert.equal(result.files[1].text,input);
    assert.equal(result.result.selection,undefined);
  }
});

test('strip calibration worker publishes exact frozen selections only after all gates accept', async () => {
  for (const admission of [false,true]) for (const accepted of [false,true]) {
    const input=`{"schema":"ftx-strip-calibration/1","allow_estimates":${admission},"source":-0.0}`;
    const report=stripCalibration(accepted?'ACCEPTED':'REJECTED',admission), raw=JSON.stringify(report), calls=[];
    const w=await workerModule({bindings:{default:()=>Promise.resolve(),
      material_calibrate_strip:(...args)=>{calls.push(args);return raw;}}});
    const result=await w.send({type:'stripFit',stripRequest:input});
    assert.equal(result.type,'result',result.message); assert.deepEqual(calls,[[input,admission]]);
    assert.equal(result.files[0].text,raw); assert.equal(result.files[1].text,input);
    assert.equal(result.files.some(f=>f.name==='strip-calibrated-selection.json'),accepted);
    if (accepted) {
      assert.equal(result.files[2].text,calibrationSelection(admission));
      assert.equal(result.result.material.capabilities.mechanical_coefficients.ready,admission);
    }
  }
  const input='{"schema":"ftx-strip-calibration/1","allow_estimates":false,"source":-0.0}';
  for (const report of [
    {...stripCalibration('REJECTED'),selection_json:calibrationSelection(true)},
    {...stripCalibration(),selection_json:null}
  ]) {
    const w=await workerModule({bindings:{default:()=>Promise.resolve(),material_calibrate_strip:()=>JSON.stringify(report)}});
    const result=await w.send({type:'stripFit',stripRequest:input});
    assert.equal(result.type,'error'); assert.match(result.message,/selection/);
    assert.equal(w.sent.some(m=>m.type==='result'),false);
  }
  const defaults=[];
  const w=await workerModule({bindings:{default:()=>Promise.resolve(),
    material_calibrate_strip:(...args)=>{defaults.push(args);return JSON.stringify(stripCalibration());}}});
  assert.equal((await w.send({type:'stripFit',stripRequest:'{}'})).type,'result');
  assert.deepEqual(defaults,[['{}',false]]);
  assert.equal((await w.send({type:'stripFit',stripRequest:'{"allow_estimates":"true"}'})).type,'error');
  assert.equal(defaults.length,1);
});

test('scenario studies retain partial failures and free the fresh model on success, refusal and trap', async () => {
  for (const mode of [{}, { failScenario: true }, { trapScenario: true }]) {
    const w = await workerModule(mode);
    const msg = { type: 'scenario', request: 13, bytes: new ArrayBuffer(8), pattern: '{"x":-0.0}',
      studyRequest: '{"value":1.0}', scenarioRequest: '{"temperature":293.0}', measurements: '{"length":10.0}' };
    const r = await w.send(msg);
    assert.deepEqual(w.freed, ['body']);
    assert.deepEqual(w.called.find(c => c[0] === 'scenario').slice(2),
      [msg.pattern, msg.studyRequest, msg.scenarioRequest, msg.measurements]);
    assert.equal(r.type, mode.failScenario || mode.trapScenario ? 'error' : 'result');
    if (r.type === 'result') {
      assert.equal(r.result.simulations[0].evaluation.status, 'refused');
      assert.equal(r.files.length, 5); assert.match(r.files[0].text, /-0\.0/);
      assert.deepEqual(plain(r.files.slice(1).map(f => f.text)),
        [msg.pattern, msg.studyRequest, msg.scenarioRequest, msg.measurements]);
      await w.send(msg); assert.equal(w.called.filter(c => c[0] === 'body').length, 2);
      assert.deepEqual(w.freed, ['body', 'body']);
    } else {
      assert.equal(w.sent.some(m => m.type === 'result'), false);
      assert.equal(r.fatal, !!mode.trapScenario);
    }
  }
});

test('aggregate study JSON budget refuses before calling any consumer or allocating a body', async () => {
  const w = await workerModule();
  const chunk = ' '.repeat(33 * 1024 * 1024);
  const r = await w.send({ type: 'scenario', pattern: chunk, studyRequest: chunk });
  assert.equal(r.type, 'error'); assert.match(r.message, /64 MiB/); assert.deepEqual(w.called, []);
});

test('scenario matrices use one native dispatch and retain exact matrix sources, partial refusals and model cleanup', async () => {
  const report=scenarioMatrixReport(), raw=` ${JSON.stringify(report)}\n`;
  const scenarioRequest=' {"schema":"ftx-drape-scenario-matrix/1","max_evaluations":4,"cases":[{"key":"cold","scenario":{"x":-0.0}},{"key":"warm","scenario":{"temperature":303.0}}]}\n';
  for (const failure of [null,Error('matrix evaluation budget exceeded'),new WebAssembly.RuntimeError('matrix trap')]) {
    const calls=[], freed=[];
    const w=await workerModule({bindings:{default:()=>Promise.resolve(),
      WebBody:class {constructor(){calls.push(['body']);}free(){freed.push('body');}},
      material_scenario_study:(...args)=>{calls.push(['matrix',...args]);if(failure)throw failure;return raw;}}});
    const msg={type:'scenario',request:17,bytes:new ArrayBuffer(8),pattern:'{"x":-0.0}',
      studyRequest:' {"coefficient":1.0}\n',scenarioRequest,measurements:'{"length":10.0}'};
    const result=await w.send(msg);
    assert.deepEqual(calls.map(c=>c[0]),['body','matrix']); assert.deepEqual(freed,['body']);
    assert.deepEqual(calls[1].slice(2),[msg.pattern,msg.studyRequest,scenarioRequest,msg.measurements]);
    if (failure) {
      assert.equal(result.type,'error'); assert.equal(result.fatal,failure instanceof WebAssembly.RuntimeError);
      assert.equal(w.sent.some(m=>m.type==='result'),false);
    } else {
      assert.equal(result.type,'result',result.message);
      assert.equal(result.result.cases[1].simulations.find(row=>row.key==='B').evaluation.status,'refused');
      assert.deepEqual(plain(result.files.map(f=>[f.name,f.text])),[
        ['material-scenario-matrix-study.json',raw],['original-pattern.json',msg.pattern],
        ['study-request.json',msg.studyRequest],['scenario-request.json',scenarioRequest],['body-input.json',msg.measurements]]);
    }
  }
});

class Element {
  constructor() { this.value = ''; this.files = []; this.children = []; this.handlers = {}; this.checked = false; this.disabled = false; }
  addEventListener(name, fn) { this.handlers[name] = fn; }
  append(...items) { this.children.push(...items); }
  replaceChildren(...items) { this.children = items; }
  removeAttribute(name) { delete this[name]; }
  getContext(kind) {
    assert.equal(this.tag, 'canvas'); assert.equal(kind, '2d');
    this.paint = [];
    return { clearRect: () => { this.paint = []; }, beginPath() {}, closePath() {}, stroke() {},
      moveTo: (...point) => this.paint.push(['move', ...point]), lineTo: (...point) => this.paint.push(['line', ...point]) };
  }
}
async function pageModule() {
  const html = read('materials.html');
  const e = Object.fromEntries([...html.matchAll(/id="([^"]+)"/g)].map(m => [m[1], new Element()]));
  const commands = [...html.matchAll(/<button([^>]*)>/g)].filter(m => m[1].includes('data-command')).map(m => {
    const el = e[/id="([^"]+)"/.exec(m[1])[1]]; el.dataset = { command: /data-command="([^"]+)"/.exec(m[1])[1] }; return el;
  });
  const workers = [], live = new Set(), blobs = new Map(), windowEvents = {}; let next = 0;
  class Worker {
    constructor() { this.messages = []; this.dead = false; workers.push(this); }
    postMessage(message) { assert.equal(this.dead, false); this.messages.push(message); }
    terminate() { this.dead = true; }
    emit(message) { this.onmessage({ data: message }); }
  }
  class LocalURL extends URL {}
  LocalURL.createObjectURL = blob => { const id = `blob:${++next}`; live.add(id); blobs.set(id, blob); return id; };
  LocalURL.revokeObjectURL = id => { assert.equal(live.delete(id), true); blobs.delete(id); };
  const context = vm.createContext({ Worker, URL: LocalURL, Blob, JSON, Date, document: {
    getElementById: id => { assert.ok(e[id], `HTML missing ${id}`); return e[id]; },
    querySelectorAll: () => commands, createElement: tag => Object.assign(new Element(), { tag }),
  }, window: { addEventListener: (name, fn) => { windowEvents[name] = fn; } } });
  const module = new vm.SourceTextModule(read('materials-page.js'), { context, initializeImportMeta: m => { m.url = 'https://unit.invalid/materials-page.js'; } });
  await module.link(() => { throw Error('unexpected import'); }); await module.evaluate();
  const current = () => workers.at(-1), ready = () => current().emit({ type: 'ready' }); ready();
  const select = (id, text = '{}') => { e[id].files = [{ name: `${id}.json`, size: text.length, text: async () => text }]; };
  const emit = (result, files = [], reportText = JSON.stringify(result)) => current().emit({ type: 'result', request: current().messages.at(-1).request, result, files, reportText });
  const pattern = { pieces: [{ id: 'front' }, { id: 'lining' }] };
  const load = async () => { select('patternFile', JSON.stringify(pattern)); await e.patternFile.onchange(); };
  const inspect = async id => { e.selection.value = JSON.stringify({ id }); e.selection.handlers.input(); await e.inspect.onclick(); emit({ selection: { id, expected_sha256: 'locked' }, capabilities: {}, snapshot: { id } }); };
  const bind = async () => { await e.bind.onclick(); emit({ pattern, pattern_json:JSON.stringify(pattern), pieces: [], pattern_sha256: 'test' }, [{ name: 'pattern.json', text: '{}', mime: 'application/json' }]); };
  return { e, current, workers, ready, select, emit, load, inspect, bind, live, blobs, windowEvents };
}
test('treatment page uses the current bound basis, displays measured changes and adopts exact derived bytes', async () => {
  const p=await pageModule(), report=treatmentReport(true), original=report.source_pattern_json,
    bound=' {"pieces":[{"id":"front"}],"bound":1.0,"x":-0.0}\n', request=' {"schema":"ftx-pattern-treatment/1","x":-0.0}\n';
  p.select('patternFile',original); await p.e.patternFile.onchange();
  p.select('treatmentRequest',request); await p.e.treatment.onclick();
  assert.equal(p.current().messages.at(-1).pattern,original);
  p.emit(treatmentReport()); assert.equal(p.e.useTreatment.disabled,true);
  await p.e.bind.onclick(); p.emit({pattern:JSON.parse(bound),pattern_json:bound,pieces:[],pattern_sha256:'bound'});
  await p.e.treatment.onclick(); const sent=p.current().messages.at(-1);
  assert.equal(sent.pattern,bound); assert.equal(sent.treatmentRequest,request);
  const raw=` ${JSON.stringify(report)}\n`;
  p.emit(report,[{name:'treatment-report.json',text:raw,mime:'application/json'},
    {name:'treat-pattern.json',text:report.pattern_json,mime:'application/json'}]);
  const s=section(p,'Laundering and dimensional treatment'), tables=descendants(s).filter(n=>n.tag==='table');
  assert.deepEqual(plain(tables[0].children[2].children.map(n=>n.textContent)),
    ['front','5','0.9517','0.945','0.8993565','thread_density_warp, thread_density_weft']);
  assert.deepEqual(plain(tables[1].children[2].children.map(n=>n.textContent)),['side','compatible','incompatible','2.5']);
  const source=descendants(s).find(n=>n.tag==='details'); source.open=true; source.handlers.toggle();
  assert.match(visibleText(source),/<literal source>/); assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  for (const [name,text] of [['treatment-report.json',raw],['treat-pattern.json',report.pattern_json]]) {
    const link=descendants(s).find(n=>n.download===name); assert.equal(await p.blobs.get(link.href).text(),text);
  }
  assert.equal(p.e.useTreatment.disabled,false); p.e.useTreatment.onclick();
  assert.equal(s.hidden,true); assert.equal(p.e.useTreatment.disabled,true); assert.equal(p.e.cut.disabled,true);
  assert.equal(p.live.size,0); await p.e.bind.onclick();
  assert.equal(p.current().messages.at(-1).pattern,report.pattern_json);
});

test('request, source pattern and material assignment changes withdraw prepared treatment adoption', async () => {
  for (const change of ['request','source','material']) {
    const p=await pageModule(); await p.load(); await p.inspect('replacement');
    p.select('treatmentRequest','{"schema":"ftx-pattern-treatment/1"}'); await p.e.treatment.onclick();
    const report=treatmentReport(true); p.emit(report,[{name:'treat-pattern.json',text:report.pattern_json,mime:'application/json'}]);
    assert.equal(p.e.useTreatment.disabled,false);
    if (change==='request') p.e.treatmentRequest.handlers.change();
    else if (change==='source') await p.load();
    else {p.e.target.value='front';p.e.assign.onclick();}
    assert.equal(p.e.useTreatment.disabled,true,change);
    assert.equal(section(p,'Laundering and dimensional treatment').hidden,true,change); assert.equal(p.live.size,0);
    p.e.useTreatment.onclick(); await p.e.bind.onclick();
    assert.notEqual(p.current().messages.at(-1).pattern,report.pattern_json);
  }
});

test('changed treatment requests, source patterns and material interfaces cancel delayed reads and late workers', async () => {
  for (const change of ['request','source','interfaces']) {
    const p=await pageModule(), delay=deferred(); await p.load();
    p.select('treatmentRequest','{"schema":"ftx-pattern-treatment/1"}');
    p.e.treatmentRequest.files[0].text=()=>delay.promise;
    const old=p.current(), running=p.e.treatment.onclick();
    if(change==='request') p.e.treatmentRequest.handlers.change();
    else if(change==='source') await p.load();
    else {p.select('interfacesFile','[]');await p.e.interfacesFile.onchange();}
    assert.equal(old.dead,true,change); p.ready();
    delay.resolve('{"schema":"ftx-pattern-treatment/1"}'); await running;
    assert.equal(old.messages.filter(m=>m.type==='treatment').length,0);
    old.emit({type:'result',request:1,result:treatmentReport(true),files:[{name:'stale.json',text:'{}'}]});
    assert.equal(p.e.useTreatment.disabled,true); assert.equal(p.live.size,0);
    assert.equal(section(p,'Laundering and dimensional treatment').hidden,true);
  }
});

test('selected-body manufacturing is opt-in and invalidates only current body-dependent outputs', async () => {
  const p=await pageModule();await p.load();p.select('studyRequest',' {"schema":"ftx-design-study/1","x":1.0}');
  p.e.studyRequest.handlers.change();assert.equal(p.e.study.disabled,false);
  p.e.studyOnBody.checked=true;p.e.studyOnBody.handlers.change();assert.equal(p.e.study.disabled,true);
  p.select('body');p.e.body.files[0].arrayBuffer=async()=>new Uint8Array([1,2,3]).buffer;
  p.e.body.handlers.change();assert.equal(p.e.study.disabled,true);
  p.select('scenarioBody',' {"schema":"ftx-scenario-body/1","n":-0.0}');p.e.scenarioBody.handlers.change();
  p.ready();assert.equal(p.e.study.disabled,false);assert.equal(p.e.scenario.disabled,true); // no scenario required
  await p.e.study.onclick();const msg=p.current().messages.at(-1);
  assert.equal(msg.onBody,true);assert.equal(msg.measurements,' {"schema":"ftx-scenario-body/1","n":-0.0}');
  assert.deepEqual([...new Uint8Array(msg.bytes)],[1,2,3]);assert.equal(msg.scenarioRequest,undefined);
  p.emit({alternatives:[]},[{name:'body-input.json',text:msg.measurements,mime:'application/json'}]);
  assert.equal(p.live.size,1);
  p.e.scenarioRequest.handlers.change();assert.equal(p.live.size,1);
  p.e.measurements.handlers.change();assert.equal(p.live.size,1); // separate selected-body request owns evidence
  p.e.body.handlers.change();assert.equal(p.live.size,0);
  p.ready();await p.e.study.onclick();const old=p.current(),request=old.messages.at(-1).request;
  p.e.scenarioBody.handlers.change();assert.equal(old.dead,true);
  old.emit({type:'result',request,result:{alternatives:[]},files:[{name:'stale.json',text:'{}'}]});assert.equal(p.live.size,0);
  p.ready();p.e.studyOnBody.checked=false;p.e.studyOnBody.handlers.change();p.ready();
  await p.e.study.onclick();assert.equal(p.current().messages.at(-1).onBody,false);
  assert.equal(p.current().messages.at(-1).measurements,undefined);
});

test('material page freezes named assignments, withdraws dependent outputs and ignores stale worker replies', async () => {
  const p = await pageModule(); await p.load(); await p.inspect('linen');
  p.e.target.value = 'lining'; p.e.assign.onclick(); await p.e.bind.onclick();
  const request = p.current().messages.at(-1);
  assert.equal(JSON.parse(request.changes).pieces.lining.expected_sha256, 'locked');
  p.emit({ pattern: { pieces: [] }, pattern_json:'{"pieces":[]}', pieces: [], pattern_sha256: 'old' }, [{ name: 'pattern.json', text: '{}', mime: 'application/json' }]);
  assert.equal(p.live.size, 1);
  p.select('cutOptions'); await p.e.cut.onclick(); const old = p.current(); const id = old.messages.at(-1).request;
  p.e.cutOptions.handlers.change(); assert.equal(old.dead, true); p.ready();
  old.emit({ type: 'result', request: id, result: { rolls: [] }, files: [{ name: 'old.json', text: '{}' }] });
  assert.equal(p.live.size, 1); // only the still-current bound pattern
  await p.inspect('leather'); p.e.target.value = ''; p.e.assign.onclick();
  assert.equal(p.live.size, 0); assert.equal(p.e.cut.disabled, true);
});
test('material page cancels delayed files before transfer and frees all downloads on page exit', async () => {
  const p = await pageModule(); await p.load(); await p.bind();
  p.select('cutOptions'); const d = deferred(); p.e.cutOptions.files[0].text = () => d.promise;
  const old = p.current(), running = p.e.cut.onclick(); p.e.cancel.onclick(); p.ready(); d.resolve('{}'); await running;
  assert.equal(old.messages.filter(m => m.type === 'cut').length, 0);
  assert.equal(p.live.size, 1); p.windowEvents.pagehide(); assert.equal(p.live.size, 0); assert.equal(p.current().dead, true);
});
test('material page protects newer pattern and selection edits against delayed reads', async () => {
  const p = await pageModule(); const d = deferred(); p.select('selectionFile'); p.e.selectionFile.files[0].text = () => d.promise;
  const old = p.e.selectionFile.onchange(); p.e.selection.value = '{"id":"new"}'; p.e.selection.handlers.input();
  d.resolve('{"id":"old"}'); await old; assert.equal(p.e.selection.value, '{"id":"new"}');
  const a = deferred(); p.select('patternFile'); p.e.patternFile.files[0].text = () => a.promise;
  const loading = p.e.patternFile.onchange(); await p.load(); a.resolve('{"pieces":[{"id":"stale"}]}'); await loading;
  await p.e.bind.onclick(); assert.equal(JSON.parse(p.current().messages.at(-1).pattern).pieces[0].id, 'front');
});
function outfitImport() {
  const request_json = ' {"schema":"ftx-outfit-drape/1","garments":[{"id":"inner"},{"id":"outer"}]}\n';
  const pattern_json = '{"schema":"fabric-pattern/1","pieces":[{"id":"inner::band","x":-0.0},{"id":"outer::band","coefficient":1.0}]}';
  const composition = {schema:'ftx-outfit-composition/1',composed_pattern_sha256:'initial-composed-lock',
    garments:[{id:'inner',pieces:{band:'inner::band'}},{id:'outer',pieces:{band:'outer::band'}}]};
  return {schema:'ftx-outfit-workbench/1',request_json,request_sha256:'original-file-lock',pattern_json,
    composition,composition_json:JSON.stringify(composition),self_contact_required:true,
    scope:'Original composition before subsequent material substitutions.'};
}
test('outfit worker retains exact sources across imports and substitutions and requires joint contact', async () => {
  const imported = outfitImport(), called = [];
  const w = await workerModule({bindings:{default:()=>Promise.resolve(),
    material_outfit_prepare:input=>{called.push(['outfit',input]);return JSON.stringify(imported);},
    material_bind:(p,c)=>{called.push(['bind',p,c]);return JSON.stringify({pattern_json:p});},
    WebBody:class {constructor(){called.push(['body']);}free(){}},
    fold_up:(_b,p)=>{called.push(['fold',p]);return {report:()=>'{"mass_kg":1.0}',viewer:()=>'<html></html>',free(){}};},
    material_scenario_study:(_b,p)=>{called.push(['scenario',p]);return '{"manufacturing":{"alternatives":[]},"simulations":[]}';}
  }});
  const prepared = await w.send({type:'outfit',request:1,outfitRequest:imported.request_json});
  assert.equal(prepared.type,'result',prepared.message);
  assert.deepEqual(called[0],['outfit',imported.request_json]);
  assert.equal(prepared.files.find(f=>f.name==='outfit-pattern.json').text,imported.pattern_json);
  const outfit = {request_json:imported.request_json,composition_json:imported.composition_json};
  const edited = imported.pattern_json.replace('1.0','2.0');
  const rebound = await w.send({type:'bind',request:2,pattern:edited,changes:'{}',outfit});
  assert.equal(rebound.files.find(f=>f.name==='outfit-pattern.json').text,edited);
  assert.equal(rebound.files.find(f=>f.name==='outfit-request.json').text,imported.request_json);
  assert.equal(rebound.files.find(f=>f.name==='outfit-composition.json').text,imported.composition_json);
  const base = {pattern:edited,outfit,bytes:new ArrayBuffer(1),measurements:'{}',fit:'{}',studyRequest:'{}'};
  const disabled = await w.send({...base,type:'fold',request:3,selfContactMm:0});
  assert.equal(disabled.type,'error');assert.match(disabled.message,/require.*self-contact/);
  assert.equal(called.some(c=>c[0]==='body'),false);
  const folded = await w.send({...base,type:'fold',request:4,selfContactMm:2});
  assert.equal(folded.type,'result',folded.message);
  assert.equal(folded.files.find(f=>f.name==='outfit-pattern.json').text,edited);
  const unsupported = await w.send({...base,type:'scenario',request:5,scenarioRequest:'{"options":{}}'});
  assert.equal(unsupported.type,'error');assert.match(unsupported.message,/options.self_contact/);
  assert.equal(called.some(c=>c[0]==='scenario'),false);
  const supported = await w.send({...base,type:'scenario',request:6,scenarioRequest:'{"options":{"self_contact":{}}}'});
  assert.equal(supported.type,'result',supported.message);
  assert.deepEqual(called.find(c=>c[0]==='scenario'),['scenario',edited]);
  const matrix={schema:'ftx-drape-scenario-matrix/1',max_evaluations:2,cases:[
    {key:'contact',scenario:{options:{self_contact:{}}}},{key:'missing contact',scenario:{options:{}}}]};
  const incomplete=await w.send({...base,type:'scenario',request:7,scenarioRequest:JSON.stringify(matrix)});
  assert.equal(incomplete.type,'error'); assert.match(incomplete.message,/self_contact in every case/);
  assert.equal(called.filter(c=>c[0]==='scenario').length,1);
  matrix.cases[1].scenario.options.self_contact={};
  const complete=await w.send({...base,type:'scenario',request:8,scenarioRequest:JSON.stringify(matrix)});
  assert.equal(complete.type,'result',complete.message);
  assert.equal(called.filter(c=>c[0]==='scenario').length,2);
});
test('outfit page imports namespaced pieces and binds exact composed JSON with its source receipt', async () => {
  const p = await pageModule(), imported = outfitImport();
  p.select('patternFile',imported.request_json);await p.e.patternFile.onchange();
  assert.equal(p.current().messages.at(-1).type,'outfit');assert.equal(p.e.bind.disabled,true);
  p.emit(imported,[{name:'outfit-request.json',text:imported.request_json,mime:'application/json'},
    {name:'outfit-composition.json',text:imported.composition_json,mime:'application/json'}]);
  assert.equal(p.e.selfContact.checked,true);assert.equal(p.e.selfContact.disabled,true);
  assert.match(p.e.patternName.textContent,/2 garments/);
  assert.deepEqual(p.e.target.children.slice(1).map(c=>c.value),['@outfit','@garment:0','@garment:1','inner::band','outer::band']);
  await p.inspect('leather');p.e.target.value='@garment:1';p.e.assign.onclick();await p.e.bind.onclick();
  const binding = p.current().messages.at(-1);
  assert.equal(binding.pattern,imported.pattern_json);
  assert.equal(JSON.parse(binding.changes).pieces['outer::band'].id,'leather');
  assert.deepEqual(plain(binding.outfit),{request_json:imported.request_json,composition_json:imported.composition_json});
  p.emit({pattern_json:imported.pattern_json,pattern_sha256:'changed-material-lock',pieces:[]});
  p.select('studyRequest','{"schema":"ftx-material-study/1"}');await p.e.study.onclick();
  const study = p.current().messages.at(-1);
  assert.equal(study.pattern,imported.pattern_json);assert.equal(study.outfit.request_json,imported.request_json);
  p.e.cancel.onclick();p.ready();await p.load();
  assert.equal(p.e.selfContact.disabled,false);
  assert.equal(section(p,'Imported joint outfit').hidden,true);
  await p.e.bind.onclick();assert.equal(p.current().messages.at(-1).outfit,undefined);
});
test('replacing or refusing an outfit import cannot publish stale garments or retain an old binding', async () => {
  const p = await pageModule(), imported = outfitImport();await p.load();await p.bind();
  p.select('patternFile',imported.request_json);await p.e.patternFile.onchange();
  assert.equal(p.e.fold.disabled,true);assert.equal(p.e.bind.disabled,true);
  const old = p.current(), request = old.messages.at(-1).request;
  await p.load();p.ready();old.emit({type:'result',request,result:imported,files:[]});
  assert.doesNotMatch(p.e.patternName.textContent,/garments/);
  await p.e.bind.onclick();assert.equal(JSON.parse(p.current().messages.at(-1).pattern).pieces[0].id,'front');
  p.e.cancel.onclick();p.ready();
  p.select('patternFile',imported.request_json);await p.e.patternFile.onchange();
  p.current().emit({type:'error',request:p.current().messages.at(-1).request,message:'Missing pair evidence'});
  assert.equal(p.e.bind.disabled,true);assert.equal(p.e.fold.disabled,true);assert.match(p.e.error.textContent,/pair evidence/);
});
test('catalog changes cannot publish another material inspection or silently enable estimates', async () => {
  const p = await pageModule(); await p.load(); p.e.selection.value = '{"id":"old"}'; await p.e.inspect.onclick();
  const old = p.current(), id = old.messages.at(-1).request;
  p.e.catalog.value = '<new>'; p.e.catalog.onchange(); p.ready();
  old.emit({ type: 'result', request: id, result: { selection: { id: 'old' }, capabilities: {}, snapshot: {} } });
  assert.equal(p.e.assign.disabled, true); assert.equal(JSON.parse(p.e.selection.value).allow_estimates, false);
  const d = deferred(); p.select('selectionFile'); p.e.selectionFile.files[0].text = () => d.promise;
  const reading = p.e.selectionFile.onchange(); p.e.catalog.value = 'latest'; p.e.catalog.onchange();
  d.resolve('{"id":"late"}'); await reading; assert.equal(JSON.parse(p.e.selection.value).id, 'latest');
});
test('interface replacements bind with exact material edits, retain numeric text and can explicitly clear pairs', async () => {
  const p=await pageModule(); await p.load(); await p.inspect('replacement-shell');
  p.e.assign.onclick();
  const pairs='[{"materials":["shell-hash","lining-hash"],"friction":{"value":1.0,"bounds":[-0.0,1.0]},"allow_estimates":false}]';
  p.select('interfacesFile',pairs); await p.e.interfacesFile.onchange();
  assert.equal(p.e.bind.disabled,false);
  await p.e.bind.onclick(); const request=p.current().messages.at(-1);
  assert.ok(request.changes.includes(pairs));
  assert.equal(JSON.parse(request.changes).default.id,'replacement-shell');
  assert.equal(JSON.parse(request.changes).interfaces[0].allow_estimates,false);
  p.emit({pattern_json:'{}',pieces:[]});
  p.select('interfacesFile','[]'); await p.e.interfacesFile.onchange();
  await p.e.bind.onclick(); assert.deepEqual(JSON.parse(p.current().messages.at(-1).changes).interfaces,[]);
  p.emit({pattern_json:'{}',pieces:[]});
  p.e.clearInterfaces.onclick(); await p.e.bind.onclick();
  assert.equal(Object.hasOwn(JSON.parse(p.current().messages.at(-1).changes),'interfaces'),false);
});
test('invalid and superseded interface reads cannot silently reuse the old table or attach to another pattern', async () => {
  const p=await pageModule(); await p.load(); await p.bind();
  p.select('interfacesFile','{"interfaces":[]}'); await p.e.interfacesFile.onchange();
  assert.equal(p.e.bind.disabled,true); assert.equal(p.e.cut.disabled,true); assert.equal(p.live.size,0);
  assert.match(p.e.error.textContent,/JSON list/);
  const old=p.current().messages.length; await p.e.bind.onclick();
  assert.equal(p.current().messages.length,old);
  p.e.clearInterfaces.onclick(); assert.equal(p.e.bind.disabled,false);
  const delay=deferred(); p.select('interfacesFile'); p.e.interfacesFile.files[0].text=()=>delay.promise;
  const reading=p.e.interfacesFile.onchange(); assert.equal(p.e.bind.disabled,true);
  await p.load(); delay.resolve('[{"materials":["stale","pair"]}]'); await reading;
  await p.e.bind.onclick(); assert.equal(Object.hasOwn(JSON.parse(p.current().messages.at(-1).changes),'interfaces'),false);
});
test('changing interface evidence withdraws bound consumers and rejects an in-flight old result', async () => {
  const p=await pageModule(); await p.load(); await p.bind();
  p.select('cutOptions'); await p.e.cut.onclick(); const old=p.current(), id=old.messages.at(-1).request;
  p.select('interfacesFile','[]'); await p.e.interfacesFile.onchange();
  assert.equal(old.dead,true); assert.equal(p.live.size,0); assert.equal(p.e.cut.disabled,true);
  p.ready(); old.emit({type:'result',request:id,result:{rolls:[]},files:[{name:'stale.svg',text:'old',mime:'image/svg+xml'}]});
  assert.equal(p.live.size,0); assert.equal(p.e.cut.disabled,true);
  p.e.reset.onclick(); await p.e.bind.onclick();
  assert.equal(Object.hasOwn(JSON.parse(p.current().messages.at(-1).changes),'interfaces'),false);
});
test('sewing setup results retain Rust order, proof scope and complete downloads until recipe changes', async () => {
  const p = await pageModule(); await p.load(); await p.bind();
  const options = '{"ordering":{"objective":"minimize_setup_changes","max_search_nodes":1000},"source":-0.0}';
  p.select('sewOptions', options); p.e.sewOptions.handlers.change(); p.ready();
  await p.e.sew.onclick(); assert.equal(p.current().messages.at(-1).options, options);
  const changes = n => ({needle_changes:n,thread_changes:n,total_changes:2*n});
  const report = {schema:'ftx-sewing-program/1',stitches:[{}],plan:{schema:'ftx-sewing-plan/6',
    options:JSON.parse(options),feed_seconds:12,hardware_execution_authorized:false,
    seams:['a','c','b'].map((id,i)=>({pass:{id,seam:'side',needle:i<2?'N1':'N2',thread:i<2?'T1':'T2',after:id==='c'?['a']:[]}})),
    ordering:{baseline_order:['a','b','c'],selected_order:['a','c','b'],baseline_changes:changes(2),
      selected_changes:changes(1),search_nodes:8,optimality_proven:true,budget_exhausted:false}}};
  const raw=JSON.stringify(report), files=[{name:'sewing.json',text:raw,mime:'application/json'}];
  p.emit(report, files); const s=section(p,'sew');
  assert.match(visibleText(s),/Minimum setup-change count proven for the declared operations/);
  assert.match(visibleText(s),/No initial setup was supplied/);
  const tables=descendants(s).filter(n=>n.tag==='table');
  assert.deepEqual(plain(tables[0].children[3].children.map(n=>n.textContent)),['Selected order','1','1','2']);
  assert.deepEqual(plain(tables[1].children[3].children.map(n=>n.textContent)),['2','c','side','N1','T1','a']);
  const download=descendants(s).find(n=>n.download==='sewing.json');
  assert.equal(await p.blobs.get(download.href).text(),raw);
  await p.e.sew.onclick(); report.plan.ordering.optimality_proven=false; report.plan.ordering.budget_exhausted=true;
  p.emit(report); assert.match(visibleText(s),/minimum setup-change count has not been proven/);
  assert.match(visibleText(s),/search reached its declared work limit/);
  p.e.sewOptions.handlers.change(); assert.equal(s.hidden,true);
  assert.equal(p.live.size,1); // only the still-current bound pattern remains
});

test('batch sewing displays production ordering, scoped copy dependencies and actual batch feed with objective-specific proof', async () => {
  const p=await pageModule(); await p.load(); await p.bind(); p.select('sewOptions');
  const changes=n=>({needle_changes:n,thread_changes:n,total_changes:2*n});
  for (const timed of [false,true]) {
    const order={baseline_order:['0/a','0/b','1/a','1/b'],selected_order:['1/a','0/a','1/b','0/b'],
      baseline_changes:changes(3),selected_changes:changes(1),optimality_proven:true,budget_exhausted:false,search_nodes:17,
      ...(timed?{timing:{selected_setup_milliseconds:3500,feed_and_setup_seconds:28.5}}:{})};
    const report={schema:'ftx-sewing-program/1',stitches:[],plan:{feed_seconds:12,hardware_execution_authorized:false,
      options:{production:{ordering:{objective:timed?'minimize_setup_time':'minimize_setup_changes',initial_setup:{needle:'N0',thread:'T0'}}}},
      seams:[{pass:{id:'a',seam:'join',needle:'N1',thread:'T1',after:[]}},{pass:{id:'b',seam:'hem',needle:'N2',thread:'T2',after:['a']}}],
      ordering:{baseline_changes:changes(99),selected_changes:changes(98)},
      production:{production_copies:2,feed_seconds:25,ordering:order,operations:[
        {id:'0/a',seam_index:0,after:[]},{id:'0/b',seam_index:1,after:['0/a']},
        {id:'1/a',seam_index:0,after:[]},{id:'1/b',seam_index:1,after:['1/a']}]}}};
    await p.e.sew.onclick(); const raw=JSON.stringify(report);
    p.emit(report,[{name:'sewing.json',text:raw,mime:'application/json'}]);
    const s=section(p,'sew'), tables=descendants(s).filter(n=>n.tag==='table'), summary=JSON.parse(s.children[1].textContent);
    assert.deepEqual(plain(tables[0].children[3].children.map(n=>n.textContent)),['Selected order','1','1','2']);
    assert.deepEqual(plain(tables[1].children.slice(2).map(row=>row.children[1].textContent)),order.selected_order);
    assert.deepEqual(plain(tables[1].children[4].children.map(n=>n.textContent)),['3','1/b','hem','N2','T2','1/a']);
    assert.equal(summary.production_copies,2); assert.equal(summary.feed_seconds,25);
    assert.deepEqual(summary.setup_changes,changes(1));
    assert.equal(summary.ordering_objective,timed?'minimize_setup_time':'minimize_setup_changes');
    assert.equal(summary.ordering_optimality_proven,true);
    if(timed) {
      assert.equal(summary.setup_seconds,3.5); assert.equal(summary.feed_and_setup_seconds,28.5);
      assert.match(visibleText(s),/Minimum declared setup duration proven/);
      assert.doesNotMatch(visibleText(s),/Minimum setup-change count proven/);
      assert.notEqual(summary.minimum_setup_changes_proven,true);
    } else assert.match(visibleText(s),/Minimum setup-change count proven/);
    assert.match(visibleText(s),/supplied initial setup is included/);
    const link=descendants(s).find(n=>n.download==='sewing.json'); assert.equal(await p.blobs.get(link.href).text(),raw);
    await p.e.sew.onclick(); order.optimality_proven=false; order.budget_exhausted=true; p.emit(report);
    assert.equal(JSON.parse(s.children[1].textContent).ordering_optimality_proven,false);
    assert.match(visibleText(s),timed?/minimum declared setup duration has not been proven/:/minimum setup-change count has not been proven/);
    assert.match(visibleText(s),/search reached its declared work limit/);
  }
});

test('qualified handling displays joint timing, physical copies and source evidence without a setup-only optimum claim', async () => {
  const p=await pageModule(); await p.load(); await p.bind(); scenarioInputs(p); p.select('sewOptions');
  const order={baseline_order:['0/join','0/finish'],selected_order:['0/join','0/finish'],
    baseline_changes:{needle_changes:0,thread_changes:2,total_changes:2},
    selected_changes:{needle_changes:0,thread_changes:2,total_changes:2},optimality_proven:true,
    budget_exhausted:false,search_nodes:4,timing:{baseline_setup_milliseconds:6500,
      selected_setup_milliseconds:6500,feed_and_setup_seconds:30.5},
    handling_timing:{baseline_handling_milliseconds:10600,selected_handling_milliseconds:10600,
      baseline_modeled_production_seconds:41.1,selected_modeled_production_seconds:41.1}};
  const plan={options:{production:{ordering:{initial_setup:{}},handling:{source:{note:'<timed garment handling>'}}}},
    seams:[{pass:{id:'join',seam:'side',needle:'N',thread:'A',after:[]}},
      {pass:{id:'finish',seam:'side',needle:'N',thread:'B',after:['join']}}],
    production:{production_copies:1,feed_seconds:24,ordering:order,
      operations:[{id:'0/join',seam_index:0,after:[]},{id:'0/finish',seam_index:1,after:['0/join']}],
      handling:{selected:{steps:[{from:null,to:'0/join',milliseconds:100},
        {from:'0/join',to:'0/finish',milliseconds:10300},{from:'0/finish',to:null,milliseconds:200}]}}}};
  for(const op of ['sew','study','scenario']) {
    const row=studyRow('handled'); row.sewing=outcome(plan);
    const study=studyReport([row]);
    const report=op==='sew'?{stitches:[],plan}:op==='study'?study:{...scenarioReport(),manufacturing:study};
    await p.e[op].onclick(); p.emit(report);
    const s=section(p,op==='sew'?'sew':op==='study'?'Fixed-pattern manufacturing alternatives':'Fresh drape and heat alternatives');
    const table=ending=>descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent.endsWith(ending));
    assert.deepEqual(plain(table('serial production durations').children[3].children.map(n=>n.textContent)),
      ['Selected order','24','6.5','10.6','41.1']);
    assert.deepEqual(plain(table('selected garment handling').children[2].children.map(n=>n.textContent)),
      ['Initial loading','0/join','100']);
    assert.deepEqual(plain(table('selected garment handling').children[4].children.map(n=>n.textContent)),
      ['0/finish','Final unloading','200']);
    assert.match(visibleText(s),/joint optimum does not prove the setup-only minimum/);
    const source=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent.endsWith('handling evidence, exact qualifications and complete timelines'));
    source.open=true; source.handlers.toggle(); assert.match(visibleText(source),/<timed garment handling>/);
    if(op==='sew') {
      const summary=JSON.parse(s.children[1].textContent);
      assert.equal(summary.ordering_objective,'minimize_production_time');
      assert.equal(summary.handling_seconds,10.6); assert.equal(summary.modeled_production_seconds,41.1);
      assert.match(visibleText(s),/Minimum declared setup plus handling duration proven/);
      assert.doesNotMatch(visibleText(s),/Minimum declared setup duration proven/);
    }
  }
  p.e.sewOptions.handlers.change(); p.e.studyRequest.handlers.change();
  assert.equal(section(p,'sew').hidden,true); assert.equal(section(p,'Fresh drape and heat alternatives').hidden,true);
});

test('parallel cells display elapsed completion, local-to-global copies and exact sources without serial proof leakage', async () => {
  const p=await pageModule(); await p.load(); await p.bind(); scenarioInputs(p); p.select('sewOptions');
  const local=copies=>({production_copies:copies,feed_seconds:copies*8,
    ordering:{selected_order:Array.from({length:copies},(_,copy)=>[`${copy}/join`,`${copy}/reinforce`]).flat(),
      optimality_proven:true},
    operations:Array.from({length:copies},(_,copy)=>[
      {id:`${copy}/join`,copy,operation_id:'join',seam_index:0,after:[]},
      {id:`${copy}/reinforce`,copy,operation_id:'reinforce',seam_index:1,after:[`${copy}/join`]}]).flat()});
  const selected={makespan_milliseconds:25500,feed_seconds:24,scheduled_feed_milliseconds:24000,
    setup_milliseconds:5500,handling_milliseconds:5900,cells:[
      {cell:'cell-a',copies:[0],available_at_milliseconds:0,finish_milliseconds:9900,
        scheduled_feed_milliseconds:8000,feed_padding_seconds:0,schedule:local(1)},
      {cell:'cell-b',copies:[1,2],available_at_milliseconds:0,finish_milliseconds:25500,
        scheduled_feed_milliseconds:16000,feed_padding_seconds:0,schedule:local(2)}]};
  const parallel={schema:'ftx-sewing-parallel/1',time_basis:'ceil_cell_feed_milliseconds/1',
    baseline:{makespan_milliseconds:41100,feed_seconds:24,scheduled_feed_milliseconds:24000,
      setup_milliseconds:6500,handling_milliseconds:10600},selected,
    lower_bound_makespan_milliseconds:25500,search_nodes:73,optimality_proven:true,budget_exhausted:false};
  const plan={schema:'ftx-sewing-plan/16',hardware_execution_authorized:false,
    options:{production:{parallel:{source:{note:'<qualified independent labor>'},cells:[{id:'cell-a'},{id:'cell-b'}]}}},
    seams:[{pass:{id:'join',seam:'side',needle:'N',thread:'A',after:[]}},
      {pass:{id:'reinforce',seam:'side',needle:'N',thread:'B',after:['join']}}],
    production:{production_copies:3,feed_seconds:24,ordering:{optimality_proven:true},parallel}};
  for (const op of ['sew','study','scenario']) {
    const row=studyRow('cells'); row.sewing=outcome(plan); row.metrics.sewing_parallel_makespan_seconds=25.5;
    const study=studyReport([row]);
    const report=op==='sew'?{schema:'ftx-sewing-program/5',stitches:[],plan}:
      op==='study'?study:{...scenarioReport(),manufacturing:study};
    const raw=JSON.stringify(report);
    await p.e[op].onclick(); p.emit(report,[{name:'parallel-report.json',text:raw,mime:'application/json'}]);
    const s=section(p,op==='sew'?'sew':op==='study'?'Fixed-pattern manufacturing alternatives':'Fresh drape and heat alternatives');
    const table=ending=>descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent.endsWith(ending));
    assert.deepEqual(plain(table('parallel completion and aggregate work').children[3].children.map(n=>n.textContent)),
      ['Selected cells','25.5','24','24','5.5','5.9']);
    assert.deepEqual(plain(table('cell-local sewing order').children.at(-1).children.map(n=>n.textContent)),
      ['cell-b','4','2','reinforce','N','B','1/join']);
    assert.match(visibleText(s),/Minimum parallel completion time proved/);
    assert.match(visibleText(s),/Elapsed completion is distinct from summed machine work/);
    assert.doesNotMatch(visibleText(s),/Minimum declared setup|Selected cross-garment sewing order/);
    const source=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent.endsWith('cell independence, timing sources and complete allocations'));
    source.open=true; source.handlers.toggle(); assert.match(visibleText(source),/<qualified independent labor>/);
    const download=descendants(s).find(n=>n.download==='parallel-report.json');
    assert.equal(await p.blobs.get(download.href).text(),raw);
    assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
    if(op==='sew') {
      const summary=JSON.parse(s.children[1].textContent);
      assert.equal(summary.parallel_makespan_seconds,25.5); assert.equal(summary.parallel_optimality_proven,true);
      assert.equal(Object.hasOwn(summary,'modeled_production_seconds'),false);
      assert.equal(Object.hasOwn(summary,'ordering_optimality_proven'),false);
      await p.e.sew.onclick(); parallel.optimality_proven=false; parallel.budget_exhausted=true;
      p.emit({stitches:[],plan}); assert.match(visibleText(s),/minimum parallel completion time is not proved/);
      assert.match(visibleText(s),/inner schedule reached its work limit/);
      parallel.optimality_proven=true; parallel.budget_exhausted=false;
    }
  }
  p.e.sewOptions.handlers.change(); p.e.studyRequest.handlers.change();
  assert.equal(section(p,'sew').hidden,true); assert.equal(section(p,'Fixed-pattern manufacturing alternatives').hidden,true);
  assert.equal(section(p,'Fresh drape and heat alternatives').hidden,true);
});

test('heterogeneous cells display their actual machine recipes and selected thread contributions', async () => {
  const p=await pageModule(); await p.load(); await p.bind(); p.select('sewOptions'); await p.e.sew.onclick();
  const source=(cell,machine,thread,pitch,rate)=>({cell,machine_sha256:machine+'-hash',
    plan:{options:{machine:{id:machine},qualifications:[{id:thread+'-qualification'}]},
      seams:[{pass:{id:'join',seam:'side',needle:machine+'-needle',thread,max_stitch_mm:pitch,stitches_per_minute:rate}}]}});
  const schedule=feed=>({feed_seconds:feed,operations:[{id:'0/join',copy:0,operation_id:'join',seam_index:0,after:[]}],
    ordering:{selected_order:['0/join'],optimality_proven:true}});
  const selected={makespan_milliseconds:2000,feed_seconds:3,scheduled_feed_milliseconds:3000,
    setup_milliseconds:0,handling_milliseconds:0,cells:[
      {cell:'cell-a',copies:[0],available_at_milliseconds:0,finish_milliseconds:2000,feed_padding_seconds:0,schedule:schedule(2)},
      {cell:'cell-b',copies:[1],available_at_milliseconds:0,finish_milliseconds:1000,feed_padding_seconds:0,schedule:schedule(1)}]};
  const parallel={schema:'ftx-sewing-parallel/2',baseline:{...selected,makespan_milliseconds:4000},selected,
    cell_plans:[source('cell-a','machine-a','thread-a',2.5,600),source('cell-b','machine-b','thread-b',5,1200)],
    lower_bound_makespan_milliseconds:2000,search_nodes:9,optimality_proven:true,budget_exhausted:false};
  const plan={seams:[{pass:{id:'join',needle:'REFERENCE',thread:'REFERENCE'}}],options:{production:{parallel:{cells:[]}}},
    production:{production_copies:2,feed_seconds:3,parallel},consumables:{schema:'ftx-sewing-consumables/3',production_copies:2,
      complete_consumption_coverage:true,total_required_length_m:1.3,operation_channels:[
        {cell:'cell-a',operation_id:'join',qualification_id:'thread-a-qualification',channel_id:'needle',material_sha256:'yarn-a',
          stitch_intervals:40,required_length_m:0.8,per_copy:{stitch_intervals:40,traversal_length_m:0.6,start_allowance_m:0.1,end_allowance_m:0.1,required_length_m:0.8}},
        {cell:'cell-b',operation_id:'join',qualification_id:'thread-b-qualification',channel_id:'needle',material_sha256:'yarn-b',
          stitch_intervals:20,required_length_m:0.5,per_copy:{stitch_intervals:20,traversal_length_m:0.3,start_allowance_m:0.1,end_allowance_m:0.1,required_length_m:0.5}}]}};
  p.emit({plan,stitches:[]});
  const s=section(p,'sew'), tables=descendants(s).filter(n=>n.tag==='table');
  const order=tables.find(n=>visibleText(n).includes('Maximum stitch (mm)'));
  assert.deepEqual(plain(order.children[3].children.map(n=>n.textContent)),
    ['cell-b','1','1','join','machine-b-needle','thread-b','5','1200','None']);
  assert.doesNotMatch(visibleText(order),/REFERENCE/);
  const channels=tables.find(n=>visibleText(n).includes('Traversed intervals')&&visibleText(n).includes('Qualification'));
  assert.deepEqual(plain(channels.children[3].children.map(n=>n.textContent)),
    ['cell-b','join','thread-b-qualification','needle','yarn-b','20','0.5']);
  assert.match(visibleText(s),/reported consequences, not separately optimized quantities/);
  assert.equal(JSON.parse(s.children[1].textContent).feed_seconds,3);
  p.e.sewOptions.handlers.change(); assert.equal(s.hidden,true);
});

test('partial sewing spans display both source sides without an ordering policy or new geometry', async () => {
  const p=await pageModule(); await p.load(); await p.bind(); p.select('sewOptions');
  await p.e.sew.onclick();
  const report={schema:'ftx-sewing-program/1',stitches:[{}],plan:{schema:'ftx-sewing-plan/10',
    feed_seconds:1,hardware_execution_authorized:false,seams:[{
      pass:{id:'left-part',seam:'join',direction:'reverse'},
      source_span:{source_ranges:[[0,0.4],[0,0.6]],source_lengths_mm:[40,60],sewn_lengths_mm:[42,63]}
    },{pass:{id:'whole-reinforcement',seam:'join',direction:'forward'}}]}};
  const raw=JSON.stringify(report);
  p.emit(report,[{name:'sewing.json',text:raw,mime:'application/json'}]);
  const s=section(p,'sew'), t=descendants(s).find(n=>n.tag==='table');
  assert.deepEqual(plain(t.children[2].children.map(n=>n.textContent)),
    ['left-part','join','[0, 0.4]','[0, 0.6]','40 / 60','42 / 63','reverse']);
  assert.equal(t.children[3].children[2].textContent,'Whole seam');
  assert.match(visibleText(s),/complete coverage on both source sides/);
  const download=descendants(s).find(n=>n.download==='sewing.json');
  assert.equal(await p.blobs.get(download.href).text(),raw);
  p.e.sewOptions.handlers.change(); assert.equal(s.hidden,true);
});

test('vapor calculation works without a pattern and preserves unrelated inspected material', async () => {
  const p=await pageModule(); await p.inspect('existing-selection');
  assert.equal(p.e.vapor.disabled,true);
  const selected=p.e.selection.value, request=' {"schema":"ftx-vapor-stack/1","area_m2":1.0,"value":-0.0}\n';
  p.select('vaporRequest',request); p.e.vaporRequest.handlers.change();
  assert.equal(p.e.vapor.disabled,false); await p.e.vapor.onclick();
  assert.equal(p.current().messages.at(-1).vaporRequest,request);
  const report=vaporReport(), raw=JSON.stringify(report);
  p.emit(report,[{name:'vapor-report.json',text:raw,mime:'application/json'},
    {name:'vapor-replay-request.json',text:report.replay_request_json,mime:'application/json'}]);
  const s=section(p,'Potential evaporative transfer'), tables=descendants(s).filter(n=>n.tag==='table');
  assert.match(visibleText(s),/Potential steady transfer/);
  assert.match(visibleText(s),/actual evaporative cooling/);
  assert.deepEqual(plain(tables[1].children[2].children.map(n=>n.textContent)),
    ['<shell>','retained-specimen','20','2500','1500','1000','measured']);
  assert.equal(p.e.selection.value,selected);
  const source=descendants(s).find(n=>n.tag==='details'); source.open=true; source.handlers.toggle();
  assert.match(visibleText(source),/"air_temperature_c": "35"/);
  assert.match(visibleText(source),/Original <measurement>/);
  const download=descendants(s).find(n=>n.download==='vapor-replay-request.json');
  assert.equal(await p.blobs.get(download.href).text(),report.replay_request_json);
  p.e.vaporRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
  assert.equal(p.e.selection.value,selected);
  await p.e.vapor.onclick(); p.current().emit({type:'error',request:p.current().messages.at(-1).request,message:'Missing vapor resistance'});
  assert.equal(s.hidden,true); assert.equal(p.live.size,0);
});

test('standalone coverage and history replays retain source bytes, units, integration rules and initial samples', async () => {
  const cases=[['thermal_vapor_coverage',vaporCoverageReport()],
    ['thermal_vapor_coverage_history',vaporHistoryReport()],
    ['thermal_vapor_coverage_history',vaporHistoryReport('composite_trapezoid')],
    ['thermal_vapor_coverage_history',wetFilmHistoryReport()]];
  for (const [engine,report] of cases) {
    const p=await pageModule(), raw=` ${JSON.stringify(report)}\n`;
    const w=await workerModule({bindings:{default:()=>Promise.resolve(),[engine]:()=>raw}});
    p.select('vaporRequest',report.replay_request_json); p.e.vaporRequest.handlers.change();
    assert.equal(p.e.vapor.disabled,false); await p.e.vapor.onclick();
    const request=p.current().messages.at(-1);
    assert.equal(request.vaporRequest,report.replay_request_json);
    const result=await w.send(request); assert.equal(result.type,'result',result.message);
    p.current().emit(result);
    const s=section(p,'Potential evaporative transfer');
    const table=caption=>descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent===caption);
    assert.equal(s.hidden,false); assert.match(visibleText(s),/"physical_validation_claim": false/);
    if (engine==='thermal_vapor_coverage') {
      const t=table('Layered vapor coverage');
      assert.deepEqual(plain(t.children.slice(2).map(row=>row.children.map(n=>n.textContent))),[
        ['Covered body area','1.5','m²'],['Uncovered body area — transfer unknown','0.5','m²'],
        ['Covered potential heat flow','30','W'],['Covered potential water flow','0.000012','kg/s']]);
      const d=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent==='Ordered layers, pressure profiles and source conditions');
      d.open=true; d.handlers.toggle(); assert.match(visibleText(d),/Original <measurement>/);
      assert.match(visibleText(s),/uncovered transfer and moisture availability remain unknown/);
    } else {
      const totals=table('Replay: moving layered vapor potential'), samples=table('Replay: vapor coverage at accepted motion samples');
      assert.deepEqual(plain(totals.children[1].children.map(n=>n.textContent)),
        ['History','Duration (s)','Accepted samples','Potential energy (J)','Potential water mass (kg)','Integration rule']);
      assert.deepEqual(plain(totals.children[2].children.map(n=>n.textContent)),
        ['Available','1','2',String(report.totals.potential_energy_j),String(report.totals.potential_mass_kg),report.integration]);
      assert.deepEqual(plain(samples.children[1].children.map(n=>n.textContent)),
        ['Time (s)','Interval (s)','Covered body (m²)','Uncovered body (m²)','Unassigned reference cloth (m²)',
          'Potential power (W)','Potential water flow (kg/s)','Interval energy (J)','Interval water mass (kg)']);
      assert.deepEqual(plain(samples.children[2].children.map(n=>n.textContent)),
        ['0','Initial only','1','1','0.25','20','0.000008','Initial only','Initial only']);
      assert.deepEqual(plain(samples.children[3].children.map(n=>n.textContent)),
        ['0.25','0.25','2','0','0.25','40','0.000016',String(report.samples[0].potential_energy_j),String(report.samples[0].potential_mass_kg)]);
      assert.deepEqual(plain(samples.children[4].children.map(n=>n.textContent)),
        ['1','0.75','0.5','1.5','0.25','10','0.000004',String(report.samples[1].potential_energy_j),String(report.samples[1].potential_mass_kg)]);
      const trapezoid=report.integration==='composite_trapezoid';
      assert.match(visibleText(s),trapezoid?/composite trapezoidal rule/:/right-endpoint rule/);
      assert.doesNotMatch(visibleText(s),trapezoid?/right-endpoint rule/:/composite trapezoidal rule/);
      assert.match(visibleText(s),/Uncovered body transfer remains unknown/);
      assert.match(visibleText(s),/refining motion samples can change the integral/);
      const d=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent.startsWith('Replay: moving vapor sources'));
      d.open=true; d.handlers.toggle(); assert.match(visibleText(d),/history-lock/);
      assert.match(visibleText(d),/motion-lock/); assert.match(visibleText(d),/Original <measurement>/);
      if (report.wet_film) {
        const water=table('Replay: finite body water and latent energy');
        assert.deepEqual(plain(water.children.slice(2).map(row=>row.children.slice(1).map(n=>n.textContent))),[
          ['0.000003','kg'],['0.000001','kg'],['0.000007','kg'],['0.000004','kg'],
          ['0.000003','kg'],['0','kg'],['10','J'],['0','kg']]);
        const trace=table('Replay: body water at accepted motion samples');
        assert.deepEqual(plain(trace.children[2].children.map(n=>n.textContent)),
          ['0.25','2.5e-7','0.00000325','7.5e-7','0','8.125','0']);
        assert.deepEqual(plain(trace.children[3].children.map(n=>n.textContent)),
          ['1','7.5e-7','7.5e-7','0.00000225','0','1.875','0']);
        const source=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent.includes('body water source'));
        source.open=true; source.handlers.toggle(); assert.match(visibleText(source),/<literal wet-film acquisition>/);
        assert.match(visibleText(s),/prescribed body reservoir supplies the reported latent energy/);
        assert.match(visibleText(s),/Textile temperatures, garment mass and wet-material properties are unchanged/);
      } else assert.equal(table('Replay: finite body water and latent energy'),undefined);
    }
    for (const [name,text] of [['vapor-report.json',raw],['vapor-request.json',report.replay_request_json],
      ['vapor-replay-request.json',report.replay_request_json]]) {
      const download=descendants(s).find(n=>n.download===name);
      assert.equal(await p.blobs.get(download.href).text(),text);
    }
    assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
    assert.equal(p.live.size,3);
    p.e.vaporRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
    await p.e.vapor.onclick();
    p.current().emit({type:'error',request:p.current().messages.at(-1).request,message:'late geometry budget exhausted'});
    assert.equal(s.hidden,true); assert.equal(p.live.size,0);
  }
});

test('replacing a vapor request cancels delayed file reads and stale worker results', async () => {
  const p=await pageModule(), delay=deferred(), oldRequest=vaporHistoryReport().replay_request_json,
    newRequest=vaporCoverageReport().replay_request_json;
  p.select('vaporRequest',oldRequest);
  p.e.vaporRequest.files[0].text=()=>delay.promise;
  const old=p.current(), running=p.e.vapor.onclick();
  p.select('vaporRequest',newRequest); p.e.vaporRequest.handlers.change(); p.ready();
  delay.resolve(oldRequest); await running;
  assert.equal(old.dead,true); assert.equal(old.messages.length,0);
  old.emit({type:'result',request:1,result:vaporHistoryReport(),files:[]});
  assert.equal(section(p,'Potential evaporative transfer').hidden,true);
  await p.e.vapor.onclick(); assert.equal(p.current().messages.at(-1).vaporRequest,newRequest);
});

test('pressure-conditioned selections enter ordinary piece bindings without a numeric JSON round trip', async () => {
  const p = await pageModule(); await p.load();
  const request = '{"pressure_pa":250.0,"source_value":-0.0}';
  p.select('compressionRequest', request); p.e.compressionRequest.handlers.change();
  assert.equal(p.e.pressure.disabled, false);
  await p.e.pressure.onclick();
  assert.equal(p.current().messages.at(-1).compressionRequest, request);
  const selection = '{"id":"compressed","expected_sha256":"locked","overrides":{"test":{"value":-0.0}},"coefficient":1.0}';
  p.emit({ selection: JSON.parse(selection), selection_json: selection, input: { pressure_pa: 250 },
    pressure_range_pa: [0, 500], bracket_pa: [0, 500], interpolated: true,
    discarded_properties: ['bulk_density'], discarded_responses: [], discarded_structure: [],
    inspection: { snapshot: { id: 'compressed' }, capabilities: {} } },
    [{ name: 'pressure-conditioned-selection.json', text: selection, mime: 'application/json' }]);
  assert.equal(p.e.selection.value, selection); assert.equal(p.e.assign.disabled, false);
  p.e.target.value = 'front'; p.e.assign.onclick(); await p.e.bind.onclick();
  const binding = p.current().messages.at(-1).changes;
  assert.ok(binding.includes(selection));
  assert.equal(JSON.parse(binding).pieces.front.id, 'compressed');
  p.emit({ pattern: {}, pattern_json: '{}', pieces: [], pattern_sha256: 'new' });
  p.e.compressionRequest.handlers.change();
  assert.equal(p.e.assign.disabled, true);
  assert.equal(p.live.size, 0);
});

test('a newer selection cancels a delayed pressure request before it can replace the inspected material', async () => {
  const p = await pageModule(), delay = deferred(); await p.load();
  p.select('compressionRequest'); p.e.compressionRequest.files[0].text = () => delay.promise;
  const old = p.current(), running = p.e.pressure.onclick();
  p.e.catalog.value = 'newer-material'; p.e.catalog.onchange(); p.ready();
  delay.resolve('{"pressure_pa":250.0}'); await running;
  assert.equal(old.dead, true);
  assert.equal(old.messages.filter(m => m.type === 'pressure').length, 0);
  assert.equal(JSON.parse(p.e.selection.value).id, 'newer-material');
  assert.equal(p.e.assign.disabled, true);
});

test('heat histories display chronological ledgers and withdraw final selections when inputs change', async () => {
  const p = await pageModule(); await p.load();
  const request = '{"schema":"ftx-compressed-thermal-history/2","source":-0.0}';
  p.select('compressionRequest',request); p.e.compressionRequest.handlers.change();
  p.e.compressionLayer.value='outer'; await p.e.pressure.onclick();
  assert.equal(p.current().messages.at(-1).compressionRequest,request);
  assert.equal(p.current().messages.at(-1).compressionLayer,'outer');
  const selection='{"id":"final-outer","expected_sha256":"locked","value":-0.0}';
  const report={schema:'ftx-compressed-thermal-history-report/2',chosen_layer_id:'outer',
    selection:JSON.parse(selection),selection_json:selection,inspection:{snapshot:{id:'final-outer'},capabilities:{}},
    initial_compression:{external_work_j:0},final_compression:{reference_mass_kg:0.025},
    transitions:[{before_hold:1,source:'<literal source>'}],no_claim:'Synthetic display test only',
    steps:[10,20].map((elapsed_s,i)=>({elapsed_s,pressure_pa:i?0:500,thickness_m:i?0.02:0.01,
      temperature_c:[12+i,14+i],cumulative_external_work_j:i?0:0.625,cumulative_inside_heat_j:20+i,
      cumulative_outside_heat_j:5+i,cumulative_stored_change_j:15,cumulative_heat_balance_error_j:0}))};
  const raw=JSON.stringify(report);
  p.emit(report,[{name:'compression-thermal-report.json',text:raw,mime:'application/json'},
    {name:'compression-request.json',text:request,mime:'application/json'}]);
  const s=section(p,'Pressure-conditioned material'), t=descendants(s).find(n=>n.tag==='table');
  assert.equal(t.children.length,4);
  assert.deepEqual(plain(t.children[3].children.map(n=>n.textContent)),['20','0','0.02','13, 15','0','21','6','15','0']);
  assert.match(visibleText(s),/External mechanical work is separate from sensible heat/);
  assert.equal(p.e.selection.value,selection); assert.equal(p.e.assign.disabled,false);
  const link=descendants(s).find(n=>n.download==='compression-request.json');
  assert.equal(await p.blobs.get(link.href).text(),request);
  p.e.assign.onclick(); await p.e.bind.onclick();
  assert.ok(p.current().messages.at(-1).changes.includes(selection));
  p.emit({pattern:{},pattern_json:'{}',pieces:[],pattern_sha256:'bound'});
  p.e.compressionLayer.value='inner'; p.e.compressionLayer.handlers.input();
  assert.equal(s.hidden,true); assert.equal(p.e.assign.disabled,true); assert.equal(p.live.size,0);
});

test('identified swatches show independent errors, retain their source across pattern loading and bind without numeric mutation', async () => {
  const p=await pageModule(), input='{"coefficient":1.0,"origin":-0.0}';
  p.e.calibrationMode.value='nonlinear'; p.select('calibrationRequest',input); p.e.calibrationRequest.handlers.change();
  assert.equal(p.e.calibrate.disabled,false); assert.equal(p.e.calibrationEstimates.checked,false);
  await p.e.calibrate.onclick();
  assert.equal(p.current().messages.at(-1).calibrationRequest,input);
  assert.equal(p.current().messages.at(-1).allowEstimates,false);
  const selection=calibrationSelection(false);
  p.emit(calibrationReport('nonlinear',false,false),[{name:'calibrated-selection.json',text:selection,mime:'application/json'}]);
  const s=section(p,'Identified material behavior');
  assert.match(visibleText(s),/Held out: not supplied/);
  assert.match(visibleText(s),/Fitted estimates need admission/);
  assert.match(visibleText(s),/Strain domain from fitting observations/);
  assert.doesNotMatch(visibleText(s),/Area coefficients|Physical area ratio J|Joint identification diagnostics/);
  assert.equal(p.e.selection.value,selection); assert.equal(p.e.assign.disabled,true);
  await p.load(); assert.equal(s.hidden,false); assert.equal(p.live.size,1); assert.equal(p.e.assign.disabled,false);
  p.e.target.value='front'; p.e.assign.onclick(); await p.e.bind.onclick();
  assert.ok(p.current().messages.at(-1).changes.includes(selection));
  assert.equal(JSON.parse(p.current().messages.at(-1).changes).pieces.front.allow_estimates,false);
});

test('linear, bending and single-material friction fits expose measured responses and bind exact selections', async () => {
  for (const mode of ['linear','bending','friction_self','friction_body']) {
    const p=await pageModule();await p.load();await p.inspect('prior-material');
    p.e.calibrationMode.value=mode;p.select('calibrationRequest');p.e.calibrationRequest.handlers.change();
    await p.e.calibrate.onclick();assert.equal(p.e.assign.disabled,true);
    const report=calibrationReport(mode,false,false), raw=JSON.stringify(report);
    p.emit(report,[{name:'material-calibration.json',text:raw,mime:'application/json'},
      {name:'calibrated-selection.json',text:report.selection_json,mime:'application/json'}]);
    const s=section(p,'Identified material behavior'), tables=descendants(s).filter(n=>n.tag==='table');
    assert.match(visibleText(s),/Held out: not supplied/);
    assert.match(visibleText(s),/Fitted estimates need admission/);
    assert.equal(p.e.selection.value,calibrationSelection(false));assert.equal(p.e.assign.disabled,false);
    assert.equal(p.e.stageInterface.disabled,true);
    if (mode==='linear') {
      const stiffness=tables.find(t=>visibleText(t).includes('Linear membrane stiffness tensor'));
      assert.deepEqual(plain(stiffness.children[2].children.map(n=>n.textContent)),['Warp / wale (E11)','300','60','0']);
      assert.match(visibleText(s),/CHECK observations do not widen/);
    } else if (mode==='bending') {
      const tensor=tables.find(t=>visibleText(t).includes('Directional bending tensor'));
      assert.deepEqual(plain(tensor.children[3].children.map(n=>n.textContent)),['Weft / course (κvv)','0.003','0.01','0']);
      const curvature=tables.find(t=>visibleText(t).includes('Identified preferred curvature'));
      assert.deepEqual(plain(curvature.children[4].children.map(n=>n.textContent)),['Twist (2 κuv)','0.1']);
      assert.match(visibleText(s),/retains independent seam bending/);
    } else {
      assert.match(visibleText(s),/Effective isotropic kinetic coefficient/);
      assert.match(visibleText(s),/do not enforce a runtime load or speed domain/);
      if (mode==='friction_body') {
        assert.match(visibleText(s),/surrogate does not establish human-skin behavior/);
        const source=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent.startsWith('Complete source observations'));
        source.open=true;source.handlers.toggle();assert.match(visibleText(source),/Synthetic <counterface>/);
      }
    }
    p.e.target.value='front';p.e.assign.onclick();await p.e.bind.onclick();
    assert.ok(p.current().messages.at(-1).changes.includes(report.selection_json));
    p.emit({pattern:{},pattern_json:'{}',pieces:[],pattern_sha256:'bound'});
    await p.e.calibrate.onclick();
    p.current().emit({type:'error',request:p.current().messages.at(-1).request,message:'Independent check refused'});
    assert.equal(p.e.assign.disabled,true);assert.equal(s.hidden,true);
  }
});

test('pair friction preserves inspected materials and stages lossless evidence without erasing retained pairs', async () => {
  const p=await pageModule();await p.load();
  const selection='{"id":"existing-material","expected_sha256":"existing-lock","value":-0.0,"scale":1.0}';
  p.e.selection.value=selection;p.e.selection.handlers.input();await p.e.inspect.onclick();
  p.emit({selection:JSON.parse(selection),selection_json:selection,capabilities:{},snapshot:{id:'existing-material'}});
  p.e.target.value='front';p.e.assign.onclick();
  const retained=' [{"materials":["prior-left","prior-right"],"friction":{"value":1.0,"bounds":[-0.0,1.0]}}]\n';
  p.select('interfacesFile',retained);await p.e.interfacesFile.onchange();await p.bind();
  p.e.calibrationMode.value='friction_pair';p.e.calibrationEstimates.checked=true;
  const input=' {"target":{"kind":"pair"},"source_value":1.0}\n';p.select('calibrationRequest',input);
  p.e.calibrationRequest.handlers.change();await p.e.calibrate.onclick();
  assert.equal(p.e.selection.value,selection);
  const report=calibrationReport('friction_pair',true), files=[
    {name:'calibrated-interface.json',text:report.interface_json,mime:'application/json'},
    {name:'friction-binding.json',text:report.binding_json,mime:'application/json'}];
  p.emit(report,files);
  assert.equal(p.e.selection.value,selection);assert.equal(p.e.assign.disabled,false);assert.equal(p.e.stageInterface.disabled,false);
  const s=section(p,'Identified material behavior');
  assert.match(visibleText(s),/Exact contact pair and separate material readiness/);
  assert.match(visibleText(s),/Missing areal mass/);assert.match(visibleText(s),/inspected material selection is unchanged/);
  const download=descendants(s).find(n=>n.download==='calibrated-interface.json');
  assert.equal(await p.blobs.get(download.href).text(),report.interface_json);
  const links=p.live.size, priorSummary=p.e.interfacesSummary.textContent;
  await p.e.stageInterface.onclick();let sent=p.current().messages.at(-1);
  assert.equal(sent.type,'stageInterface');assert.equal(sent.interfaceJson,report.interface_json);assert.equal(sent.replaceExisting,false);
  assert.ok(sent.changes.includes(retained));assert.ok(sent.changes.includes(selection));
  p.current().emit({type:'error',request:sent.request,message:'Existing pair requires explicit replacement'});
  assert.equal(p.e.interfacesSummary.textContent,priorSummary);assert.equal(p.live.size,links);
  assert.equal(p.e.selection.value,selection);assert.equal(p.e.stageInterface.disabled,false);
  p.e.replaceCalibratedInterface.checked=true;await p.e.stageInterface.onclick();sent=p.current().messages.at(-1);
  assert.equal(sent.replaceExisting,true);
  const combined=`[${retained.trim().slice(1,-1)},${report.interface_json}]`;
  p.emit({interfaces_json:combined,changes_json:`{"interfaces":${combined}}`,geometry_changed:false});
  assert.equal(p.e.replaceCalibratedInterface.checked,false);assert.equal(p.e.cut.disabled,true);
  assert.equal(p.live.size,files.length);assert.match(p.e.interfacesSummary.textContent,/2 retained contact pair/);
  assert.equal(p.e.selection.value,selection);assert.equal(p.e.assign.disabled,false);
  await p.e.bind.onclick();sent=p.current().messages.at(-1);
  assert.ok(sent.changes.includes(combined));assert.ok(sent.changes.includes(selection));
  p.emit({pattern:{},pattern_json:'{}',pieces:[],pattern_sha256:'rebound'});
  p.e.calibrationRequest.handlers.change();assert.equal(p.e.stageInterface.disabled,true);
  assert.equal(p.e.selection.value,selection);assert.equal(p.e.assign.disabled,false);
  await p.e.bind.onclick();assert.ok(p.current().messages.at(-1).changes.includes(combined));
});

test('pair refusals and stale staging cannot replace the selected material or publish withdrawn interface edits', async () => {
  const p=await pageModule();await p.load();await p.inspect('unrelated-material');
  const selection=p.e.selection.value, report=calibrationReport('friction_pair',false);
  p.e.calibrationMode.value='friction_pair';p.select('calibrationRequest','{"target":{"kind":"pair"}}');
  await p.e.calibrate.onclick();p.emit(report);
  assert.match(visibleText(section(p,'Identified material behavior')),/Interface estimates need their own admission/);
  await p.e.stageInterface.onclick();const old=p.current(), request=old.messages.at(-1).request;
  p.select('interfacesFile','[]');await p.e.interfacesFile.onchange();p.ready();
  old.emit({type:'result',request,result:{interfaces_json:`[${report.interface_json}]`,changes_json:'{}'},files:[]});
  assert.equal(old.dead,true);assert.equal(p.e.selection.value,selection);
  await p.e.bind.onclick();assert.deepEqual(JSON.parse(p.current().messages.at(-1).changes).interfaces,[]);
  p.emit({pattern:{},pattern_json:'{}',pieces:[],pattern_sha256:'bound'});
  await p.e.stageInterface.onclick();const previous=p.current(), staged=previous.messages.at(-1).request;
  p.e.calibrationEstimates.handlers.change();p.ready();
  previous.emit({type:'result',request:staged,result:{interfaces_json:`[${report.interface_json}]`,changes_json:'{}'},files:[]});
  assert.equal(p.e.stageInterface.disabled,true);assert.equal(p.e.assign.disabled,false);assert.equal(p.e.selection.value,selection);
  await p.e.calibrate.onclick();p.current().emit({type:'error',request:p.current().messages.at(-1).request,message:'Source hash changed'});
  assert.equal(p.e.stageInterface.disabled,true);assert.equal(p.e.assign.disabled,false);assert.equal(p.e.selection.value,selection);
  assert.equal(section(p,'Identified material behavior').hidden,true);
});

test('joint swatches expose area coefficients, exact applicability and source evidence without changing reusable selections', async () => {
  const p=await pageModule();p.e.calibrationMode.value='nonlinear';p.select('calibrationRequest');
  await p.e.calibrate.onclick();
  const report=areaCalibrationReport(),raw=JSON.stringify(report);
  p.emit(report,[{name:'material-calibration.json',text:raw,mime:'application/json'}]);
  const s=section(p,'Identified material behavior'), tables=descendants(s).filter(n=>n.tag==='table');
  const coefficients=tables.find(n=>visibleText(n).includes('Area coefficients'));
  assert.deepEqual(plain(coefficients.children.slice(2).map(n=>n.children.map(v=>v.textContent))),[
    ['(J − 1)²','40','N/m','estimate','joint-area-fit'],['(ln J)²','15','N/m','estimate','joint-area-fit']]);
  const domain=tables.find(n=>visibleText(n).includes('Physical area ratio J'));
  assert.deepEqual(plain(domain.children.slice(2).map(n=>n.children.map(v=>v.textContent))),[
    ['Admissible input domain','0.7','1.5'],['FIT observations and undeformed reference','0.75','1.45'],
    ['Published material domain','0.75','1.45']]);
  assert.match(visibleText(s),/Nonlinear stretch, shear and area response/);
  assert.match(visibleText(s),/Identified coefficients\s+9/);
  assert.match(visibleText(s),/CHECK cannot widen/);
  const evidence=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent.startsWith('Area response,'));
  evidence.open=true;evidence.handlers.toggle();
  assert.match(visibleText(evidence),/"temperature_c": "20"/);
  assert.match(visibleText(evidence),/Synthetic source <retained>/);
  assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  assert.equal(p.e.selection.value,report.selection_json);
  const download=descendants(s).find(n=>n.download==='material-calibration.json');
  assert.equal(await p.blobs.get(download.href).text(),raw);
});

test('adaptive heat histories display native local errors and charged work without changing selections', async () => {
  const p=await pageModule(); await p.load();
  p.select('compressionRequest',' {"schema":"ftx-compressed-thermal-history/1"}\n');
  await p.e.pressure.onclick();
  const selection='{"id":"adaptive-layer","expected_sha256":"frozen"}';
  const report={schema:'ftx-compressed-thermal-history-report/1',chosen_layer_id:'layer',
    selection:JSON.parse(selection),selection_json:selection,inspection:{snapshot:{id:'adaptive-layer'},capabilities:{}},
    input:{adaptive:{max_cell_solves:120}},initial_compression:{external_work_j:0},
    final_compression:{reference_mass_kg:0.025},no_claim:'Synthetic display fixture only',
    steps:[{elapsed_s:10,pressure_pa:500,thickness_m:0.01,temperature_c:[15.9],
      cumulative_external_work_j:0.625,cumulative_inside_heat_j:500,cumulative_outside_heat_j:102.5,
      cumulative_stored_change_j:397.5,cumulative_heat_balance_error_j:0,
      integration:{accepted_substeps:20,rejected_trials:3,cell_solves:39,cumulative_cell_solves:39,max_accepted_error_ratio:0.7}}]};
  const raw=' '+JSON.stringify(report)+'\n';
  p.emit(report,[{name:'compression-thermal-report.json',text:raw,mime:'application/json'}]);
  const s=section(p,'Pressure-conditioned material');
  const t=descendants(s).find(n=>n.tag==='table'&&visibleText(n).includes('Charged cell solves'));
  assert.deepEqual(plain(t.children.at(-1).children.map(n=>n.textContent)),['10','20','3','39','39','0.7']);
  assert.match(visibleText(s),/not global accuracy or confidence bounds/);
  assert.equal(p.e.selection.value,selection); assert.equal(p.e.assign.disabled,false);
  const link=descendants(s).find(n=>n.download==='compression-thermal-report.json');
  assert.equal(await p.blobs.get(link.href).text(),raw);
  p.e.compressionRequest.handlers.change();
  assert.equal(s.hidden,true); assert.equal(p.e.assign.disabled,true);
});

test('recovery identification displays the fixed plastic history at unobserved nodes and withdraws stale selections', async()=>{
  const p=await pageModule();p.e.calibrationMode.value='relaxation';p.select('calibrationRequest');
  await p.e.calibrate.onclick();
  const r=calibrationReport('relaxation',true),c=r.calibration;
  c.input.plastic_equilibrium={initialization:'pristine_reference',allow_estimated_network:true,
    network:{model:'parallel_green_maxwell_plastic/1',source:'fixed-network',method:'<separate branch qualification>'}};
  c.input.material={id:'known-plastic',properties:{membrane_yield_warp:{
    value:20,unit:'N/m',source:'fixed-yield',method:'<independently observed yield>'}},
    sources:{'fixed-yield':{reference:'test:independent-yield',note:'<retained fixed observation>'}}};
  c.predictions=[{experiment:'load',partition:'fit',time_seconds:0.1,residual_n_m:[[null,null],[null,null]],
    plastic_equilibrium:{plastic_strain:[0.05,0,0],accumulated_strain:[0.05,0,0],
      nominal_n_m:[[25,0],[0,0]],step_dissipated_j_m2:1,cumulative_dissipated_j_m2:1}},
    {experiment:'load',partition:'fit',time_seconds:0.3,
      plastic_equilibrium:{plastic_strain:[0.05,0,0],accumulated_strain:[0.05,0,0],
        nominal_n_m:[[-5,0],[0,0]],step_dissipated_j_m2:0,cumulative_dissipated_j_m2:1}}];
  r.time_integration='Analytic Maxwell convolution; recorded-node plastic return map.';
  p.emit(r);const s=section(p,'Identified material behavior');
  const t=descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent==='Known permanent-stretch branch at every acquisition node');
  assert.deepEqual(plain(t.children[2].children.map(n=>n.textContent)),
    ['load','fit','0.1','0.05, 0, 0','0.05, 0, 0','25, 0 / 0, 0','1','1']);
  assert.equal(t.children.length,4);assert.match(visibleText(s),/including nodes without observed force/);
  assert.match(visibleText(s),/Only the Maxwell recovery branches are identified/);
  assert.match(visibleText(s),/without a continuous-path or refinement claim/);
  assert.equal(p.e.selection.value,r.selection_json);
  const d=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent==='Fixed permanent-stretch observations and combined-network qualification');
  d.open=true;d.handlers.toggle();assert.match(visibleText(d),/<separate branch qualification>/);
  assert.match(visibleText(d),/<independently observed yield>/);
  assert.match(visibleText(d),/<retained fixed observation>/);
  assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  p.e.calibrationRequest.handlers.change();assert.equal(s.hidden,true);assert.equal(p.e.assign.disabled,true);
});

test('calibration consent and response changes withdraw only the prepared result and cannot revive stale work', async () => {
  const p=await pageModule(); await p.load();
  p.e.calibrationMode.value='relaxation'; p.e.calibrationEstimates.checked=true;
  p.select('calibrationRequest'); p.e.calibrationRequest.handlers.change(); await p.e.calibrate.onclick();
  assert.equal(p.current().messages.at(-1).calibrationMode,'relaxation');
  p.emit(calibrationReport('relaxation',true),[{name:'calibrated-selection.json',text:calibrationSelection(true),mime:'application/json'}]);
  assert.match(visibleText(section(p,'Identified material behavior')),/Directional relaxation/);
  const recovery=descendants(section(p,'Identified material behavior')).find(n=>n.tag==='table' &&
    n.children[0].textContent==='Directional recovery — published stiffness at supplied times');
  assert.deepEqual(plain(recovery.children[2].children.map(n=>n.textContent)),['0.3','Active','240','60','-30','0']);
  assert.deepEqual(plain(recovery.children[5].children.map(n=>n.textContent)),['32','Inactive','0','0','0','0']);
  assert.match(visibleText(section(p,'Identified material behavior')),/not optimized from the histories/);
  p.e.calibrationEstimates.checked=false; p.e.calibrationEstimates.handlers.change();
  assert.equal(p.live.size,0); assert.equal(p.e.assign.disabled,true);
  await p.e.calibrate.onclick(); const old=p.current(), id=old.messages.at(-1).request;
  p.e.calibrationMode.value='nonlinear'; p.e.calibrationMode.handlers.change(); p.ready();
  old.emit({type:'result',request:id,result:calibrationReport('relaxation',true),files:[]});
  assert.equal(old.dead,true); assert.equal(section(p,'Identified material behavior').hidden,true);
  assert.equal(p.e.assign.disabled,true);
  const delay=deferred(); p.e.calibrationRequest.files[0].text=()=>delay.promise;
  const before=p.current(), running=p.e.calibrate.onclick();
  p.e.catalog.value='newer'; p.e.catalog.onchange(); p.ready(); delay.resolve('{}'); await running;
  assert.equal(before.messages.length,0); assert.equal(JSON.parse(p.e.selection.value).id,'newer');
});

test('directional time search shows identified values inside declared windows with local convergence and excitation', async () => {
  const p=await pageModule(); p.e.calibrationMode.value='relaxation'; p.e.calibrationEstimates.checked=true;
  p.select('calibrationRequest'); await p.e.calibrate.onclick();
  const report=calibrationReport('relaxation',true), c=report.calibration;
  c.input.orthotropic={time_bounds_seconds:[[0.1,0.9],[1,4],[5,24]]};
  c.orthotropic_fit.candidates=c.orthotropic_fit.candidates.slice(0,3);
  c.starts_attempted=5;
  c.orthotropic_fit.time_search={starts_attempted:5,starts_converged:2,iterations:11,objective_evaluations:73,
    projected_iterations:135,gradient_scaled_max:0,time_sensitivity:[13,7,4],
    joint_excitation_pivot:0.02,boundary_distance_fraction:[0.5,0.5,0.3]};
  p.emit(report);
  const s=section(p,'Identified material behavior'), rendered=visibleText(s);
  const tables=descendants(s).filter(n=>n.tag==='table');
  const windows=tables.find(n=>n.children[0].textContent==='Directional time search — declared windows and local sensitivity');
  assert.deepEqual(plain(windows.children.slice(2).map(row=>row.children.map(n=>n.textContent))),[
    ['0.1','0.3','0.9','13','0.5'],['1','2','4','7','0.5'],['5','8','24','4','0.3']
  ]);
  const diagnostics=tables.find(n=>n.children[0].textContent==='Directional time search — local optimizer diagnostics');
  assert.deepEqual(plain(diagnostics.children.slice(2).map(row=>row.children.map(n=>n.textContent))),[
    ['Converged starts / attempted','2 / 5'],['Iterations in selected solve','11'],
    ['Objective evaluations across starts and trials','73'],['Projected tensor iterations across all solves','135'],
    ['Maximum scaled gradient','0'],
    ['Joint stiffness / time excitation pivot','0.02']
  ]);
  assert.match(rendered,/Directional recovery — published stiffness at identified times/);
  assert.match(rendered,/do not establish a global optimum, a unique spectrum or confidence intervals/);
  assert.match(rendered,/Every identified time must retain an observable recovery branch/);
  assert.doesNotMatch(rendered,/at supplied times|not optimized from the histories|entirely inactive spectrum retains/);
  assert.equal(p.e.selection.value,calibrationSelection(true));
});

test('joint permanent-stretch identification distinguishes the fixed recovery spectrum and preserves exact evidence', async () => {
  const p=await pageModule(); await p.load();
  p.e.calibrationMode.value='membrane_plasticity'; p.e.calibrationEstimates.checked=true;
  p.select('calibrationRequest','{"schema":"ftx-membrane-plasticity-calibration/1"}');
  p.e.calibrationRequest.handlers.change(); p.ready(); await p.e.calibrate.onclick();
  const report=permanentCalibration('ACCEPTED',true,true), material=report.calibration.input.material;
  const q=(value,unit)=>({value,unit,source:'independent-spectrum',evidence:'estimate'});
  material.sources={'independent-spectrum':{reference:'test:independent',note:'<fixed spectrum; no fabric claim>'}};
  material.membrane_relaxation={model:'green_maxwell/1',initialization:'stress_free_reference',
    branches:[{id:'proportional',stiffness_ratio:q(0.5,'1'),relaxation_time:q(0.25,'s')}],
    orthotropic_branches:[{id:'directional',stiffness:{warp:q(90,'N/m'),weft:q(40,'N/m'),
      coupling:q(10,'N/m'),shear:q(20,'N/m')},relaxation_time:q(2,'s')}],
    parallel_plasticity:{model:'parallel_green_maxwell_plastic/1',source:'independent-spectrum'}};
  report.calibration.predictions=[{experiment:'held-out-cycle',partition:'check',stored_j_m2:2,
    plastic_dissipation_j_m2:1,parallel_history:{maxwell_stored_j_m2:3,total_stored_j_m2:5,maxwell_dissipation_j_m2:4}}];
  report.calibration.diagnostics.fixed_maxwell_branch_steps=80;
  const raw=JSON.stringify(report);
  p.emit(report,[{name:'material-calibration.json',text:raw,mime:'application/json'}]);
  const s=section(p,'Identified material behavior'), rendered=visibleText(s);
  assert.match(rendered,/Only the six permanent-stretch and hardening coefficients are identified/);
  assert.match(rendered,/Recorded elapsed times advance its delayed-recovery state/);
  assert.match(rendered,/residuals compare the total parallel-network load/);
  assert.match(rendered,/Fixed Maxwell branch-node advances \(included above\)/);
  const history=descendants(s).find(n=>n.tag==='table'&&visibleText(n).includes('Total storage (J/m²)'));
  assert.deepEqual(plain(history.children[2].children.map(n=>n.textContent)),['held-out-cycle / check','2','3','5','1','4']);
  const spectrum=descendants(s).find(n=>n.tag==='table'&&visibleText(n).includes('Stiffness definition'));
  assert.deepEqual(plain(spectrum.children[2].children.map(n=>n.textContent)),
    ['proportional','Equilibrium-tensor ratio: 0.5 1','0.25 s','estimate','independent-spectrum']);
  assert.match(visibleText(spectrum),/warp: 90 N\/m; weft: 40 N\/m; coupling: 10 N\/m; shear: 20 N\/m/);
  const evidence=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent.startsWith('Fixed recovery spectrum'));
  evidence.open=true; evidence.handlers.toggle(); assert.match(visibleText(evidence),/<fixed spectrum; no fabric claim>/);
  assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  assert.equal(p.e.selection.value,report.selection_json); assert.equal(p.e.assign.disabled,false);
  const download=descendants(s).find(n=>n.download==='material-calibration.json');
  assert.equal(await p.blobs.get(download.href).text(),raw);
  p.e.calibrationRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.e.assign.disabled,true);
});

test('jointly identified recovery exposes both unknowns and never labels them as a fixed spectrum', async () => {
  const p=await pageModule(); await p.load();
  p.e.calibrationMode.value='membrane_plasticity'; p.e.calibrationEstimates.checked=true;
  const input='{"schema":"ftx-membrane-plasticity-calibration/1","joint_relaxation":{"source":-0.0}}';
  p.select('calibrationRequest',input); p.e.calibrationRequest.handlers.change(); p.ready();
  await p.e.calibrate.onclick();
  const report=permanentCalibration('ACCEPTED',true,true), c=report.calibration;
  c.input.joint_relaxation={branch_id:'recovery',stiffness_ratio_bounds:[0.1,3],
    relaxation_time_bounds_seconds:[0.01,2],strain_range:[[-0.2,0.2],[-0.2,0.2],[-0.2,0.2]],
    initialization:'stress_free_reference',network:{model:'parallel_green_maxwell_plastic/1',source:'network'}};
  c.input.material.sources={network:{reference:'test:declared-network',note:'<independent topology; unknown coefficients>'}};
  c.parameters.relaxation={stiffness_ratio:1.5,relaxation_time_seconds:0.25};
  c.diagnostics.joint_relaxation={parameter_sensitivity:[4,7],at_bounds:[false,false],branch_steps:3600};
  c.predictions=[{experiment:'check-cycle',partition:'check',stored_j_m2:2,plastic_dissipation_j_m2:1,
    parallel_history:{maxwell_stored_j_m2:3,total_stored_j_m2:5,maxwell_dissipation_j_m2:4}}];
  const raw=JSON.stringify(report);
  p.emit(report,[{name:'material-calibration.json',text:raw,mime:'application/json'}]);
  const s=section(p,'Identified material behavior'), rendered=visibleText(s);
  assert.match(rendered,/all eight unknowns/);
  assert.match(rendered,/Joint Maxwell branch-node advances \(included above\)/);
  assert.doesNotMatch(rendered,/held fixed during identification|Only the six permanent-stretch/);
  const spectrum=descendants(s).find(n=>n.tag==='table'&&visibleText(n).includes('Jointly identified recovery'));
  assert.deepEqual(plain(spectrum.children.slice(2).map(row=>row.children.map(n=>n.textContent))),[
    ['recovery: Equilibrium-tensor stiffness ratio','1.5','[0.1, 3]','4','No'],
    ['recovery: Relaxation time (s)','0.25','[0.01, 2]','7','No']]);
  const evidence=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent.startsWith('Joint model'));
  evidence.open=true; evidence.handlers.toggle();
  assert.match(visibleText(evidence),/<independent topology; unknown coefficients>/);
  assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  assert.equal(p.e.selection.value,report.selection_json); assert.equal(p.e.assign.disabled,false);
  const download=descendants(s).find(n=>n.download==='material-calibration.json');
  assert.equal(await p.blobs.get(download.href).text(),raw);
  await p.e.calibrate.onclick();
  const unresolved=plain(report);
  Object.assign(unresolved,{selection:null,selection_json:null,material:null});
  unresolved.calibration.status='AMBIGUOUS'; unresolved.calibration.parameters=null;
  unresolved.calibration.diagnostics.sensitivity_evaluated=false;
  unresolved.calibration.decision_reasons=['The eight FIT sensitivities do not resolve all unknowns.'];
  p.emit(unresolved);
  assert.match(visibleText(s),/Not evaluated/); assert.equal(p.e.assign.disabled,true);
  assert.equal(p.e.selection.value,'');
  p.e.calibrationRequest.handlers.change(); assert.equal(s.hidden,true);
});

test('permanent-stretch results show bounded coefficients and withdraw selections for every unresolved outcome', async () => {
  const p=await pageModule(); await p.load();
  p.e.calibrationMode.value='membrane_plasticity'; p.e.calibrationEstimates.checked=true;
  const input='{"schema":"ftx-membrane-plasticity-calibration/1","source":-0.0}';
  p.select('calibrationRequest',input); p.e.calibrationRequest.handlers.change(); p.ready();
  await p.e.calibrate.onclick(); assert.equal(p.current().messages.at(-1).calibrationMode,'membrane_plasticity');
  const report=permanentCalibration('ACCEPTED',true,false), raw=JSON.stringify(report);
  p.emit(report,[{name:'material-calibration.json',text:raw,mime:'application/json'}]);
  const s=section(p,'Identified material behavior');
  assert.equal(p.e.assign.disabled,false); assert.equal(p.e.selection.value,report.selection_json);
  assert.match(visibleText(s),/Held-out replay: NOT_PROVIDED/);
  assert.match(visibleText(s),/second-Piola sheet resultants/);
  const parameters=descendants(s).find(n=>n.tag==='table'&&visibleText(n).includes('Yield search range'));
  assert.deepEqual(plain(parameters.children[2].children.map(n=>n.textContent)),['Warp / wale','10','[1, 20]','100','[0, 200]']);
  const download=descendants(s).find(n=>n.download==='material-calibration.json');
  assert.equal(await p.blobs.get(download.href).text(),raw);
  p.e.target.value='front'; p.e.assign.onclick(); await p.e.bind.onclick();
  assert.ok(p.current().messages.at(-1).changes.includes(report.selection_json));
  p.emit({pattern_json:'{}',pieces:[]});
  for (const status of ['AMBIGUOUS','BUDGET_EXHAUSTED','REJECTED']) {
    await p.e.calibrate.onclick(); p.emit(permanentCalibration(status,true));
    assert.equal(p.e.assign.disabled,true); assert.equal(p.e.selection.value,'');
    assert.match(visibleText(s),new RegExp(`identification: ${status}`));
    assert.match(visibleText(s),/No identified selection is available/);
  }
  p.e.calibrationRequest.handlers.change(); assert.equal(s.hidden,true);
});

test('clamped-strip results show geometry and operator assumptions and withdraw unresolved selections', async () => {
  const p=await pageModule();await p.load();
  p.e.calibrationMode.value='clamped_strip';p.e.calibrationEstimates.checked=true;
  const input='{"schema":"ftx-strip-calibration/1","origin":-0.0}';
  p.select('calibrationRequest',input);p.e.calibrationRequest.handlers.change();p.ready();
  await p.e.calibrate.onclick();assert.equal(p.current().messages.at(-1).calibrationMode,'clamped_strip');
  const report=stripCalibration('ACCEPTED',true,false),raw=JSON.stringify(report);
  p.emit(report,[{name:'material-calibration.json',text:raw,mime:'application/json'}]);
  const s=section(p,'Identified material behavior');
  assert.equal(p.e.assign.disabled,false);assert.equal(p.e.selection.value,report.selection_json);
  assert.match(visibleText(s),/Not supplied/);
  assert.match(visibleText(s),/Explicit synthetic midpoint chord; <literal evidence>/);
  assert.match(visibleText(s),/Grip extension is not treated as uniform fabric strain/);
  const geometry=descendants(s).find(n=>n.tag==='table'&&visibleText(n).includes('Warp axis [across, along]'));
  assert.deepEqual(plain(geometry.children[2].children.map(n=>n.textContent)),
    ['strip-source','fit','0.06','0.1','0.6, 0.8','2 × 4','Chord at half the reference gauge']);
  assert.match(visibleText(s),/Doubled-mesh change/);
  assert.match(visibleText(s),/Enforced-domain replay/);
  const predictions=descendants(s).find(n=>n.tag==='table'&&visibleText(n).includes('Actual refined strip predictions'));
  assert.deepEqual(plain(predictions.children[2].children.map(n=>n.textContent)),
    ['strip-source / point-1','fit','0.05','1.2','0.01','0.059','-0.0001']);
  assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  const download=descendants(s).find(n=>n.download==='material-calibration.json');
  assert.equal(await p.blobs.get(download.href).text(),raw);
  for (const status of ['AMBIGUOUS','REJECTED']) {
    await p.e.calibrate.onclick();p.emit(stripCalibration(status,true));
    assert.equal(p.e.assign.disabled,true);assert.equal(p.e.selection.value,'');
    assert.match(visibleText(s),new RegExp(status));
    assert.match(visibleText(s),/No identified selection is available/);
  }
  p.e.calibrationMode.value='nonlinear';p.e.calibrationMode.handlers.change();assert.equal(s.hidden,true);
});

test('declared strip extensometers retain station, signed axis and observed residuals in both controls', async () => {
  for (const dedicated of [false,true]) {
    const p=await pageModule(); await p.load();
    p.e.calibrationMode.value='clamped_strip'; p.e.calibrationEstimates.checked=true;
    const operation=dedicated?'stripFit':'calibrate', inputControl=dedicated?'stripFitRequest':'calibrationRequest';
    const input=' {"schema":"ftx-strip-calibration/1","operator_origin":-0.0}\n';
    p.select(inputControl,input); await p.e[operation].onclick();
    const report=stripCalibration('ACCEPTED',true), c=report.calibration;
    const chord={kind:'edge_chord',station_fraction:0.25};
    const projection={kind:'edge_projection',station_fraction:0.75,axis:[-1,0]};
    c.input.experiments[0].observations=[{id:'point-1',mid_width:null,extensometers:[
      {id:'<chord>',operator:chord,value_m:0.058,scale_m:0.001},
      {id:'signed',operator:projection,value_m:-0.04,scale_m:0.002}]}];
    c.predictions[0].width_residual_m=null;
    c.predictions[0].extensometers=[
      {id:'<chord>',operator:chord,predicted_m:0.059,residual_m:0.001},
      {id:'signed',operator:projection,predicted_m:-0.039,residual_m:0.001}];
    const raw=JSON.stringify(report);
    p.emit(report,[{name:'extensometer-report.json',text:raw,mime:'application/json'}]);
    const s=section(p,dedicated?'Clamped-strip identification':'Identified material behavior');
    const tables=descendants(s).filter(n=>n.tag==='table');
    const acquisition=tables.find(t=>t.children[0].textContent==='Declared transverse extensometer acquisitions');
    assert.deepEqual(plain(acquisition.children.slice(2).map(n=>n.children.map(v=>v.textContent))),[
      ['strip-source / point-1','fit','<chord>','Euclidean edge chord','0.25','Not applicable','0.058','0.001'],
      ['strip-source / point-1','fit','signed','Signed edge projection','0.75','-1, 0','-0.04','0.002']]);
    const predictions=tables.find(t=>t.children[0].textContent==='Refined extensometer predictions and residuals');
    assert.deepEqual(plain(predictions.children[3].children.map(v=>v.textContent)),
      ['strip-source / point-1','fit','signed','Signed edge projection','0.75','-1, 0','-0.039','0.001']);
    const geometry=tables.find(t=>t.children[0].textContent==='Actual strip geometry and declared width comparison');
    assert.equal(geometry.children[2].children[6].textContent,'Declared extensometers (see below)');
    assert.match(visibleText(s),/no source contraction is mapped automatically/);
    assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
    assert.equal(p.e.selection.value,report.selection_json);
    const download=descendants(s).find(n=>n.download==='extensometer-report.json');
    assert.equal(await p.blobs.get(download.href).text(),raw);
    p.e[inputControl].handlers.change(); assert.equal(s.hidden,true);
    assert.equal(p.e.assign.disabled,true); assert.equal(p.live.size,0);
  }
});

test('nonlinear strip coefficients retain declared shape assumptions, physical units and rejection gates in both controls', async () => {
  for (const dedicated of [false,true]) {
    const p=await pageModule();await p.load();
    const input='{"schema":"ftx-strip-calibration/1","allow_estimates":true,"origin":-0.0}';
    const operation=dedicated?'stripFit':'calibrate';
    p.e.calibrationMode.value='clamped_strip';p.e.calibrationEstimates.checked=true;
    p.select(dedicated?'stripFitRequest':'calibrationRequest',input);
    await p.e[operation].onclick();
    const report=nonlinearStripCalibration(),raw=JSON.stringify(report);
    p.emit(report,[{name:'nonlinear-calibration.json',text:raw,mime:'application/json'}]);
    const s=section(p,dedicated?'Clamped-strip identification':'Identified material behavior');
    const coefficients=descendants(s).find(n=>n.tag==='table'&&visibleText(n).includes('Identified nonlinear shape scales'));
    assert.deepEqual(plain(coefficients.children.slice(2).map(n=>n.children.map(v=>v.textContent))),[
      ['Warp shape scale','1.25','1','0.5','2'],['Weft shape scale','0.75','1','0.4','1.2'],
      ['Shear shape scale','2','1','0.5','3'],['Normal coupling','-12','N/m','-30','30']]);
    assert.doesNotMatch(visibleText(coefficients),/Poisson/);
    assert.match(visibleText(s),/shapes and strain knots were supplied/);
    assert.match(visibleText(s),/does not identify the curve shapes or impose area preservation/);
    const shapes=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent==='Declared nonlinear response shapes and source evidence');
    shapes.open=true;shapes.handlers.toggle();
    assert.match(visibleText(shapes),/Synthetic supplied shape; <literal source>/);
    assert.match(visibleText(shapes),/"allow_estimated_shapes": true/);
    assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
    assert.equal(p.e.selection.value,report.selection_json);assert.equal(p.e.assign.disabled,false);
    const download=descendants(s).find(n=>n.download==='nonlinear-calibration.json');
    assert.equal(await p.blobs.get(download.href).text(),raw);
    await p.e[operation].onclick();p.emit(nonlinearStripCalibration('REJECTED'));
    assert.equal(p.e.selection.value,'');assert.equal(p.e.assign.disabled,true);
    assert.match(visibleText(s),/Candidate nonlinear shape scales and coupling/);
    assert.match(visibleText(s),/No identified selection is available/);
  }
});

test('strip area reports display all six search bounds or retained source coefficients in both controls', async () => {
  for (const mode of ['identify','preserve']) for (const dedicated of [false,true]) {
    const p=await pageModule();await p.load();
    const operation=dedicated?'stripFit':'calibrate';
    p.e.calibrationMode.value='clamped_strip';p.e.calibrationEstimates.checked=true;
    p.select(dedicated?'stripFitRequest':'calibrationRequest');await p.e[operation].onclick();
    const report=nonlinearStripCalibration(),c=report.calibration;
    c.material=areaCalibrationReport().calibration.material;
    const area=c.material.membrane_response.area_response;
    c.input.area_response=mode==='identify'?{mode,parameter_bounds:[[10,80],[5,30]],area_ratio_range:[0.7,1.5]}:{mode,allow_estimates:true};
    if(mode==='identify') {
      c.parameters.push(40,15);c.parameter_bounds=[...c.input.parameter_bounds,[10,80],[5,30]];
      c.parameter_descriptors.push({id:'area_quadratic',label:'Quadratic area coefficient',unit:'N/m'},
        {id:'area_log_squared',label:'Log-squared area coefficient',unit:'N/m'});
      c.fitted_area_ratio_range=[0.75,1.45];
    } else {
      area.quadratic.evidence='published';area.log_squared.evidence='published';
    }
    c.input.material={membrane_response:{area_response:structuredClone(area)}};
    p.emit(report);
    const s=section(p,dedicated?'Clamped-strip identification':'Identified material behavior');
    const parameters=descendants(s).find(n=>n.tag==='table'&&visibleText(n).includes(mode==='identify'?
      'Identified directional and physical-area coefficients':'Identified nonlinear shape scales'));
    if(mode==='identify') {
      assert.deepEqual(plain(parameters.children.slice(-2).map(n=>n.children.map(v=>v.textContent))),[
        ['Quadratic area coefficient','40','N/m','10','80'],['Log-squared area coefficient','15','N/m','5','30']]);
      assert.match(visibleText(s),/jointly with the two physical-area coefficients/);
      assert.match(visibleText(s),/FIT observations and undeformed reference/);
      const domain=descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent==='Physical area ratio J — applicability');
      assert.deepEqual(plain(domain.children.slice(2,4).map(n=>n.children.map(v=>v.textContent))),[
        ['Admissible input domain','0.7','1.5'],['Inherited material domain','0.75','1.45']]);
      assert.match(visibleText(s),/both the declared and inherited J intervals/);
      await p.e[operation].onclick();
      c.accepted=false;c.material=null;report.material=null;report.selection=null;report.selection_json=null;
      p.emit(report);
      assert.match(visibleText(s),/Candidate directional and physical-area coefficients/);
      assert.equal(p.e.assign.disabled,true);
      assert.equal(p.e.selection.value,'');
    } else {
      assert.equal(parameters.children.length,6);
      assert.match(visibleText(s),/Area coefficients — preserved source observations/);
      assert.match(visibleText(s),/published/);
      assert.match(visibleText(s),/retained from the supplied material/);
      assert.doesNotMatch(visibleText(s),/FIT observations and undeformed reference/);
      assert.equal(p.e.selection.value,report.selection_json);
    }
  }
});

test('fresh scalar and area identification retains separate trial domains and withdraws rejected material in both controls', async () => {
  for(const dedicated of [false,true]) {
    const p=await pageModule();await p.load();
    const operation=dedicated?'stripFit':'calibrate';
    p.e.calibrationMode.value='clamped_strip';p.e.calibrationEstimates.checked=true;
    p.select(dedicated?'stripFitRequest':'calibrationRequest');await p.e[operation].onclick();
    const result=stripCalibration('ACCEPTED',true),c=result.calibration;
    c.input.material={properties:{},sources:{}};
    c.input.area_response={mode:'identify',parameter_bounds:[[10,50],[5,35]],area_ratio_range:[0.65,1.45],
      scalar_green_strain_range:[[-0.4,0.8],[-0.3,0.7],[-0.2,0.6]]};
    c.input.parameter_bounds=[[270,320],[145,180],[-0.52,-0.37],[80,112]];
    c.parameters=[290,160,-0.44,96,31,19];
    c.parameter_bounds=[...c.input.parameter_bounds,...c.input.area_response.parameter_bounds];
    c.parameter_descriptors=[
      {label:'Base warp stiffness',unit:'N/m'},{label:'Base weft stiffness',unit:'N/m'},
      {label:'Base major Poisson ratio',unit:'1'},{label:'Base shear stiffness',unit:'N/m'},
      {label:'Physical-area quadratic coefficient',unit:'N/m'},{label:'Physical-area log-squared coefficient',unit:'N/m'}];
    c.material=areaCalibrationReport().calibration.material;c.fitted_area_ratio_range=[0.75,1.45];
    c.material.membrane_response.area_response.quadratic.value=31;
    c.material.membrane_response.area_response.log_squared.value=19;
    const raw=' '+JSON.stringify(result)+'\n';
    p.emit(result,[{name:'fresh-area-report.json',text:raw,mime:'application/json'}]);
    const s=section(p,dedicated?'Clamped-strip identification':'Identified material behavior');
    const tables=()=>descendants(s).filter(n=>n.tag==='table');
    const parameters=tables().find(t=>t.children[0].textContent==='Identified scalar base and physical-area coefficients');
    assert.equal(parameters.children.length,8);
    assert.equal(parameters.children[4].children[1].textContent,'-0.44');
    const support=tables().find(t=>t.children[0].textContent==='Scalar base — declared trial strain support');
    assert.deepEqual(plain(support.children.slice(2).map(n=>n.children.map(v=>v.textContent))),[
      ['Warp / wale (E11)','-0.4','0.8'],['Weft / course (E22)','-0.3','0.7'],['Engineering shear (2 E12)','-0.2','0.6']]);
    assert.match(visibleText(s),/No prior area coefficients were supplied/);
    assert.match(visibleText(s),/base Poisson ratio is not the transverse response of the combined law/);
    assert.equal(p.e.selection.value,result.selection_json);assert.equal(p.e.assign.disabled,false);
    const download=descendants(s).find(n=>n.download==='fresh-area-report.json');
    assert.equal(await p.blobs.get(download.href).text(),raw);
    await p.e[operation].onclick();
    c.accepted=false;c.material=null;c.fitted_area_ratio_range=null;
    result.material=null;result.selection=null;result.selection_json=null;p.emit(result);
    assert.match(visibleText(s),/Candidate scalar base and physical-area coefficients/);
    assert.match(visibleText(s),/Only an accepted result publishes area coefficients/);
    const domain=tables().find(t=>t.children[0].textContent==='Physical area ratio J — applicability');
    assert.deepEqual(plain(domain.children.slice(2).map(n=>n.children.map(v=>v.textContent))),[
      ['Admissible input domain','0.65','1.45']]);
    assert.doesNotMatch(visibleText(s),/Published material domain/);
    assert.equal(p.e.assign.disabled,true);assert.equal(p.e.selection.value,'');assert.equal(p.live.size,0);
  }
});

test('joint area strip calibration displays all six native bounds through both fit controls', async () => {
  for (const dedicated of [false,true]) {
    const p=await pageModule();await p.load();
    p.e.calibrationMode.value='clamped_strip';p.e.calibrationEstimates.checked=true;
    const operation=dedicated?'stripFit':'calibrate',input=dedicated?'stripFitRequest':'calibrationRequest';
    p.select(input);await p.e[operation].onclick();
    const result=nonlinearStripCalibration(),c=result.calibration;
    c.input.area_response={mode:'identify',parameter_bounds:[[10,50],[5,35]]};
    c.parameters.push(31,19);
    c.parameter_descriptors.push({id:'area_quadratic',label:'Physical-area quadratic coefficient',unit:'N/m'},
      {id:'area_log_squared',label:'Physical-area log-squared coefficient',unit:'N/m'});
    c.parameter_bounds=[...c.input.parameter_bounds,...c.input.area_response.parameter_bounds];
    const raw=' '+JSON.stringify(result)+'\n';
    p.emit(result,[{name:'joint-area.json',text:raw,mime:'application/json'}]);
    const s=section(p,dedicated?'Clamped-strip identification':'Identified material behavior');
    const coefficients=descendants(s).find(n=>n.tag==='table'&&visibleText(n).includes('Physical-area quadratic coefficient'));
    assert.deepEqual(plain(coefficients.children.slice(-2).map(n=>n.children.map(v=>v.textContent))),[
      ['Physical-area quadratic coefficient','31','N/m','10','50'],
      ['Physical-area log-squared coefficient','19','N/m','5','35']]);
    assert.equal(coefficients.children.length,8);
    assert.equal(p.e.selection.value,result.selection_json);
    assert.equal(p.e.assign.disabled,false);
    const download=descendants(s).find(n=>n.download==='joint-area.json');
    assert.equal(await p.blobs.get(download.href).text(),raw);
    p.e[input].handlers.change();assert.equal(s.hidden,true);assert.equal(p.e.assign.disabled,true);
  }
});

test('independent nonlinear strip intervals retain every tangent, declared knots and exact accepted selection', async () => {
  const p=await pageModule();await p.load();
  p.e.calibrationMode.value='clamped_strip';p.select('calibrationRequest');
  await p.e.calibrate.onclick();
  const report=stripCalibration('ACCEPTED',true),c=report.calibration;
  c.input.parameter_bounds=[];
  c.input.nonlinear={strain_knots:[[-0.1,0,0.05,0.2],[-0.1,0,0.2],[-0.2,0,0.2]],
    tangent_bounds:[[[50,150],[200,400],[500,900]],[[40,120],[100,300]],[[20,40],[40,60]]],
    coupling_bounds:[1,20]};
  c.parameters=[100,300,700,80,200,30,50,10];
  c.parameter_descriptors=['Warp <negative>','Warp low','Warp high','Weft negative','Weft positive',
    'Shear negative','Shear positive','Normal coupling'].map((label,i)=>({id:`parameter_${i}`,label,unit:'N/m'}));
  c.parameter_bounds=[...c.input.nonlinear.tangent_bounds.flat(),c.input.nonlinear.coupling_bounds];
  const raw=JSON.stringify(report);
  p.emit(report,[{name:'interval-calibration.json',text:raw,mime:'application/json'}]);
  const s=section(p,'Identified material behavior');
  const coefficients=descendants(s).find(n=>n.tag==='table'&&visibleText(n).includes('Identified nonlinear sheet tangents'));
  assert.equal(coefficients.children.length,10);
  assert.deepEqual(plain(coefficients.children[2].children.map(n=>n.textContent)),
    ['Warp <negative>','100','N/m','50','150']);
  assert.deepEqual(plain(coefficients.children[9].children.map(n=>n.textContent)),
    ['Normal coupling','10','N/m','1','20']);
  assert.doesNotMatch(visibleText(coefficients),/Poisson/);
  const knots=descendants(s).find(n=>n.tag==='table'&&visibleText(n).includes('Fixed strain knots'));
  assert.deepEqual(plain(knots.children[2].children.map(n=>n.textContent)),
    ['Warp / wale (E11)','-0.1, 0, 0.05, 0.2','3']);
  assert.match(visibleText(s),/no compression branch or symmetry is inferred/);
  assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  assert.equal(p.e.selection.value,report.selection_json);assert.equal(p.e.assign.disabled,false);
  const download=descendants(s).find(n=>n.download==='interval-calibration.json');
  assert.equal(await p.blobs.get(download.href).text(),raw);
  p.e.calibrationRequest.handlers.change();assert.equal(s.hidden,true);assert.equal(p.live.size,0);
});

test('a refused replacement calibration leaves no previous selection or report available for assignment', async () => {
  const p=await pageModule(); await p.load(); p.e.calibrationMode.value='nonlinear'; p.select('calibrationRequest');
  await p.e.calibrate.onclick(); p.emit(calibrationReport(),[{name:'material-calibration.json',text:'{}',mime:'application/json'}]);
  assert.equal(p.live.size,1); assert.equal(p.e.assign.disabled,false);
  await p.e.calibrate.onclick(); const request=p.current().messages.at(-1).request;
  p.current().emit({type:'error',request,message:'held-out peak exceeds declared budget'});
  assert.match(p.e.error.textContent,/held-out peak/); assert.equal(p.live.size,0);
  assert.equal(p.e.assign.disabled,true); assert.equal(section(p,'Identified material behavior').hidden,true);
});

test('strip identification exposes candidate gates and preserves exact selection admission through assignment', async () => {
  const p=await pageModule(); await p.load();
  const input='{"schema":"ftx-strip-calibration/1","allow_estimates":false,"origin":-0.0}';
  p.select('stripFitRequest',input); p.e.stripFitRequest.handlers.change();
  assert.equal(p.e.stripFit.disabled,false); await p.e.stripFit.onclick();
  assert.equal(p.current().messages.at(-1).stripRequest,input);
  assert.equal(Object.hasOwn(p.current().messages.at(-1),'allowEstimates'),false);
  const report=stripCalibration(), raw=JSON.stringify(report);
  p.emit(report,[{name:'strip-calibration-report.json',text:raw,mime:'application/json'},
    {name:'strip-calibrated-selection.json',text:report.selection_json,mime:'application/json'}]);
  const s=section(p,'Clamped-strip identification');
  assert.equal(p.e.selection.value,calibrationSelection(false)); assert.equal(p.e.assign.disabled,false);
  assert.match(visibleText(s),/ACCEPTED/);
  assert.match(visibleText(s),/CHECK/); assert.match(visibleText(s),/Enforced-domain replay/);
  assert.match(s.children[1].textContent,/"mechanical_coefficients_available": false/);
  const t=descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent==='Identified orthotropic sheet coefficients');
  assert.deepEqual(plain(t.children[2].children.map(n=>n.textContent)),['Warp / wale stiffness','300','N/m','100','500']);
  const download=descendants(s).find(n=>n.download==='strip-calibration-report.json');
  assert.equal(await p.blobs.get(download.href).text(),raw);
  p.e.target.value='front'; p.e.assign.onclick(); await p.e.bind.onclick();
  assert.ok(p.current().messages.at(-1).changes.includes(calibrationSelection(false))); p.emit({pattern_json:'{}',pieces:[]});
  p.e.stripFitRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.e.assign.disabled,true);
  await p.e.stripFit.onclick(); const rejected=stripCalibration('REJECTED');
  p.emit(rejected,[{name:'strip-calibration-report.json',text:JSON.stringify(rejected),mime:'application/json'}]);
  assert.equal(p.e.assign.disabled,true); assert.equal(p.e.selection.value,'');
  assert.match(visibleText(s),/NOT ACCEPTED/); assert.match(visibleText(s),/No identified selection is available/);
  assert.ok(descendants(s).some(n=>n.download==='strip-calibration-report.json'));
  assert.equal(descendants(s).some(n=>n.download==='strip-calibrated-selection.json'),false);
});

test('changed strip inputs and later selections cancel pending work and delayed reads without stale publication', async () => {
  const p=await pageModule(); await p.load(); p.select('stripFitRequest');
  await p.e.stripFit.onclick(); const old=p.current(), id=old.messages.at(-1).request;
  p.e.stripFitRequest.handlers.change(); p.ready();
  old.emit({type:'result',request:id,result:stripCalibration(),files:[]});
  assert.equal(old.dead,true); assert.equal(p.e.assign.disabled,true); assert.equal(section(p,'Clamped-strip identification').hidden,true);
  const delay=deferred(); p.e.stripFitRequest.files[0].text=()=>delay.promise;
  const before=p.current(), reading=p.e.stripFit.onclick();
  p.e.catalog.value='newer-material'; p.e.catalog.onchange(); p.ready(); delay.resolve('{}'); await reading;
  assert.equal(before.messages.length,0); assert.equal(JSON.parse(p.e.selection.value).id,'newer-material');
});

test('strip acquisition and forward inspection retain independent outputs without assigning a material', async () => {
  const p=await pageModule(); await p.load(); await p.inspect('existing-material'); const selection=p.e.selection.value;
  p.select('stripAcquisitionRequest','{"original":-0.0}'); p.e.stripAcquisitionRequest.handlers.change();
  await p.e.stripAcquisition.onclick(); assert.equal(p.current().messages.at(-1).stripRequest,'{"original":-0.0}');
  p.emit({acquisition_sha256:'archive-lock',archive_url:'https://example.invalid/source',retrieved_on:'2026-10-20',
    metadata_json:'{"original":1.0}',license:'Source license',protocol:{free_gauge_length_m:0.1,reference_width_m:0.06},
    curves:[{specimen:'SJ1',direction:'warp',entry:'source.csv',sha256:'curve-lock',
      observations:[{extension_fraction:0.1,force_n:-0.02,branch:'loading'}]}],
    transverse_curves:[{specimen:'SJ1',direction:'bias',entry:'transverse.csv',sha256:'transverse-lock',
      source:{reference:'<original source>',accessed_on:'2026-10-21'},measurement_scope:'Exact spatial measurement coordinates are not reported',
      observations:[{csv_line:2,extension_fraction:0.0,compression_fraction:-0.001}]}],
    transverse_readme:{entry:'dataset/README.md',sha256:'readme-lock',text:'<Original fractional units>'}});
  const acquired=section(p,'Complete clamped-strip acquisitions');
  assert.match(visibleText(acquired), /Original transverse acquisitions/);
  assert.match(visibleText(acquired), /transverse-lock/);
  assert.match(visibleText(acquired), /equivalence to the solver's mid-gauge width is unverified/);
  const transverse=descendants(acquired).find(n=>n.tag==='details'&&n.children[0].textContent.includes('original transverse observations'));
  transverse.open=true; transverse.handlers.toggle();
  assert.match(visibleText(transverse), /"compression_fraction": -0.001/);
  assert.match(visibleText(transverse), /<original source>/);
  assert.doesNotMatch(visibleText(transverse), /"width_m"|"poisson"/);
  const readme=descendants(acquired).find(n=>n.tag==='details'&&n.children[0].textContent.startsWith('Original acquisition README'));
  readme.open=true; readme.handlers.toggle(); assert.match(visibleText(readme), /<Original fractional units>/);
  assert.match(visibleText(acquired),/SJ1/); assert.match(visibleText(acquired),/Grip extension does not establish local membrane strain/);
  assert.equal(p.e.selection.value,selection); assert.equal(p.e.assign.disabled,false);
  p.select('stripExperimentRequest','{"extension":0.1}'); p.e.stripExperimentRequest.handlers.change(); await p.e.stripExperiment.onclick();
  p.emit({input_sha256:'request-lock',input:{fixture:{gauge_length_m:0.1},controls:{}},model_scope:'Planar model',
    states:[{extension_fraction:0.1,force_n:2,fixed_grip_force_n:-2,free_force_residual_n:1e-8,
      grip_balance_residual_n:2e-8,mid_width_m:0.058,energy_j:0.002}]});
  const forward=section(p,'Clamped-strip experiment'); assert.match(visibleText(forward),/Grip imbalance/);
  assert.equal(p.e.selection.value,selection); assert.equal(p.e.assign.disabled,false);
  p.e.stripExperimentRequest.handlers.change(); assert.equal(forward.hidden,true); assert.equal(acquired.hidden,false);
  p.e.stripAcquisitionRequest.handlers.change(); assert.equal(acquired.hidden,true);
});

test('recovery ambiguity displays competing models and withdraws the previous assignable material', async () => {
  const p=await pageModule(); await p.load(); p.e.calibrationMode.value='relaxation'; p.select('calibrationRequest');
  await p.e.calibrate.onclick(); p.emit(recoveryComparison('ACCEPTED'));
  assert.equal(p.e.assign.disabled,false); assert.equal(p.e.selection.value,calibrationSelection(true));
  await p.e.calibrate.onclick(); p.emit(recoveryComparison(),[
    {name:'material-calibration.json',text:'{}',mime:'application/json'},
    {name:'calibration-input.json',text:'{}',mime:'application/json'}]);
  const s=section(p,'Identified material behavior'), t=descendants(s).find(n=>n.tag==='table');
  assert.match(visibleText(s),/Comparison outcome: AMBIGUOUS/);
  assert.match(visibleText(s),/Choice before held-out replay: unresolved/);
  assert.deepEqual(plain(t.children[2].children.map(n=>n.textContent)),
    ['complete','12','ACCEPTED','0.02','0.04','Yes','ACCEPTED']);
  assert.equal(p.e.selection.value,''); assert.equal(p.e.assign.disabled,true); assert.equal(p.live.size,2);
});

test('material changes withdraw drape and thermal outputs while environment edits preserve mechanics', async () => {
  const p = await pageModule(); await p.load(); await p.bind();
  for (const k of ['measurements', 'fit', 'body', 'heatConditions']) p.select(k);
  p.e.body.files[0].arrayBuffer = async () => new ArrayBuffer(8);
  p.e.measurements.handlers.change(); await p.e.fold.onclick();
  const raw='{"material":{"basis":"test"},"coordinate":-0.0,"coefficient":1.0}';
  p.emit(JSON.parse(raw), [{ name: 'sim_report.json', text: raw, mime: 'application/json' }, { name: 'viewer.html', text: '<html/>', mime: 'text/html' }],raw);
  await p.e.heat.onclick();assert.equal(p.current().messages.at(-1).report,raw);
  p.emit({ steady: {}, comfort_claim: false }, [{ name: 'heat.json', text: '{}', mime: 'application/json' }]);
  assert.equal(p.live.size, 4); p.e.heatConditions.handlers.change(); assert.equal(p.live.size, 3); assert.equal(p.e.viewer.hidden, false);
  await p.inspect('different'); p.e.assign.onclick(); assert.equal(p.live.size, 0); assert.equal(p.e.viewer.hidden, true); assert.equal(p.e.heat.disabled, true);
});

test('original drafting worker also retains replayable drape JSON and frees its fold handle', async () => {
  const sent=[], raw='{"coordinate":-0.0,"coefficient":1.0}', freed=[];
  const values={default:async()=>{},program_info:()=> '{}',calibration_dictionaries:()=> '["DICT_4X4_50"]',
    run:()=>({page:()=>'{"status":"CONVERGED"}',file:n=>new TextEncoder().encode(n==='pattern.json'?'{"pieces":[{"placement":{"part":"torso"}}]}':'{}'),free(){}}),
    WebBody:class{},fold_up:()=>({report:()=>raw,viewer:()=>'<html/>',free:()=>freed.push('fold')})};
  const context=vm.createContext({JSON,Date,TextDecoder,Uint8Array,WebAssembly,Error,self:{},onmessage:null,postMessage:m=>sent.push(m)});
  const imports=new vm.SyntheticModule(Object.keys(values),function(){for(const[n,v]of Object.entries(values))this.setExport(n,v);},{context});
  await imports.link(()=>{});await imports.evaluate();
  const module=new vm.SourceTextModule(read('worker.js'),{context,importModuleDynamically:()=>imports});
  await module.link(()=>{});await module.evaluate();
  for(const msg of [{type:'init',pkg:'./pkg/'},{type:'run',req:{}},{type:'body',bytes:new ArrayBuffer(8)},{type:'fold',selfContactMm:2}]) {
    await context.onmessage({data:msg});
    if(msg.type==='init') assert.equal(sent.at(-1).type,'ready');
  }
  assert.equal(sent.at(-1).type,'folded');assert.equal(sent.at(-1).reportText,raw);assert.deepEqual(freed,['fold']);
  assert.ok(read('index.html').includes('new Blob([msg.reportText]'));
});

test('drafting worker discards refused replacements, frees failed rounds and checks fold and body sources', async () => {
  const sent=[], freed=[], calls=[]; let failure, flatOnly=false;
  const values={default:async()=>{},program_info:()=> '{}',calibration_dictionaries:()=> '["DICT_4X4_50"]',
    run:()=> {
      if(failure==='run') throw Error('run refused');
      return {page:()=>{if(failure==='page') throw Error('page refused');return '{"status":"CONVERGED","zip":"round.zip"}';},
        zip:()=>{if(failure==='zip') throw Error('zip refused');return new Uint8Array([1]);},
        file:n=>{if(failure===n) throw Error('file refused');return new TextEncoder().encode(n==='pattern.json'?(flatOnly?'{"pieces":[]}':'{"pieces":[{"placement":{"part":"torso"}}]}'):'{}');},
        free:()=>freed.push('round')};
    },
    WebBody:class {constructor(bytes){if(bytes[0]===0)throw Error('body refused');}free(){freed.push('body');}},
    inspect_body:(_body,measurements,source)=>{calls.push({inspect:measurements,source});
      if(source==='refused')throw Error('import refused');return '{"cloth_simulated":false,"raw":-0.0}';},
    fold_up:()=>{calls.push('fold');return {report:()=>'{"gating":[]}',viewer:()=>'<html/>',free:()=>freed.push('fold')};},
    fold_up_with_body_fit:(_body,_pattern,_measurements,_fit,mm,source,angle)=>{
      calls.push({source,mm,angle});if(source==='refused')throw Error('import refused');
      return {report:()=>'{"body":{"provenance":"MODEL_FIT"}}',viewer:()=>'<html/>',free:()=>freed.push('import')};}};
  const context=vm.createContext({JSON,Date,TextDecoder,Uint8Array,WebAssembly,Error,self:{},onmessage:null,postMessage:m=>sent.push(m)});
  const imports=new vm.SyntheticModule(Object.keys(values),function(){for(const[n,v]of Object.entries(values))this.setExport(n,v);},{context});
  await imports.link(()=>{});await imports.evaluate();
  const module=new vm.SourceTextModule(read('worker.js'),{context,importModuleDynamically:()=>imports});
  await module.link(()=>{});await module.evaluate();
  const send=async data=>{await context.onmessage({data});return sent.at(-1);};
  assert.equal((await send({type:'init',pkg:'./pkg/'})).type,'ready');
  const body=()=>send({type:'body',request:2,bytes:new Uint8Array([1]).buffer});
  const run=()=>send({type:'run',request:3,req:{}});
  const fold=extra=>send({type:'fold',request:4,round:3,body:2,...extra});
  await body();await run();assert.equal((await fold()).type,'folded');
  assert.equal((await fold({round:1})).type,'error');assert.equal(calls.length,1);
  for(const stage of ['run','page','zip','pattern.json','measurements.json','fit_spec.json']) {
    failure=undefined;await run();const count=freed.filter(x=>x==='round').length;
    failure=stage;const refused=await run();assert.equal(refused.type,'error');assert.equal(refused.request,3);
    assert.equal(freed.filter(x=>x==='round').length,count+(stage==='run'?0:1),stage);
    assert.match((await fold()).message,/run a round first/);
  }
  failure=undefined;await run();await send({type:'body',request:5,bytes:new Uint8Array([0]).buffer});
  assert.match((await fold()).message,/load the body model/); // no fallback to the prior model
  await body();assert.equal((await fold()).type,'folded');
  await send({type:'clearBody'});assert.match((await fold()).message,/load the body model/);
  await body();
  const exact='{ "schema": "ftx-body-fit/1", "unrounded": 1.0000000000000002 }';
  assert.match((await fold({bodyFit:exact})).message,/associate/);
  const imported=await fold({bodyFit:exact,bodyFitConsent:true,armDeg:50,selfContactMm:2});
  assert.equal(imported.type,'folded');assert.equal(imported.bodyFitText,exact);
  assert.deepEqual(calls.at(-1),{source:exact,mm:2,angle:50});
  const tapeCalls=calls.filter(x=>x==='fold').length;
  assert.match((await fold({bodyFit:'refused',bodyFitConsent:true,armDeg:50})).message,/import refused/);
  assert.equal(calls.filter(x=>x==='fold').length,tapeCalls); // never fall back on import failure
  assert.match((await fold({bodyFit:{},bodyFitConsent:true})).message,/JSON text/);
  assert.equal((await fold()).type,'folded');assert.equal(sent.at(-1).bodyFitText,null);
  flatOnly=true;await run();assert.equal(sent.at(-1).foldable,false);assert.equal(sent.at(-1).bodyCheckable,true);
  const inspect=extra=>send({type:'inspectBody',request:8,round:3,body:2,...extra});
  const foldCalls=()=>calls.filter(x=>x==='fold'||(typeof x==='object'&&Object.hasOwn(x,'mm'))).length;
  const folds=foldCalls();
  const checked=await inspect();assert.equal(checked.type,'bodyChecked');
  assert.equal(checked.reportText,'{"cloth_simulated":false,"raw":-0.0}');
  assert.deepEqual(calls.at(-1),{inspect:'{}',source:undefined});
  assert.equal((await inspect({round:9})).type,'error');
  assert.match((await inspect({bodyFit:exact})).message,/associate/);
  const accepted=await inspect({bodyFit:exact,bodyFitConsent:true});assert.equal(accepted.bodyFitText,exact);
  assert.deepEqual(calls.at(-1),{inspect:'{}',source:exact});
  assert.match((await inspect({bodyFit:'refused',bodyFitConsent:true})).message,/import refused/);
  assert.equal(foldCalls(),folds); // body checking never calls either cloth path, including failure
  failure='run';assert.equal((await run()).type,'error');assert.match((await inspect()).message,/run a round first/);
  failure=undefined;await run();await send({type:'clearBody'});assert.match((await inspect()).message,/load the body model/);
});

// The original inline workshop script, using a small DOM transport double. These tests inspect
// source/result association and handle lifetime, not browser rendering or computed garment physics.
function draftingPage() {
  const html=read('index.html'), e=Object.fromEntries([...html.matchAll(/id="([^"]+)"/g)].map(m=>[m[1],new Element()]));
  for(const el of Object.values(e)) {
    el.addEventListener=(name,fn)=>{const previous=el.handlers[name];el.handlers[name]=(...args)=>{
      const a=previous?.(...args),b=fn(...args);return Promise.all([a,b]);};};
    const classes=new Set(['hidden']);el.classList={add:x=>classes.add(x),remove:x=>classes.delete(x),
      contains:x=>classes.has(x),toggle:(x,on)=>on?classes.add(x):classes.delete(x)};
    el.dataset={};el.querySelectorAll=()=>[];el.scrollIntoView=()=>{};
  }
  e.stature.value='92';e.age.value='30';e.style.value='{}';
  const modes=['chart','tape','file'].map(value=>Object.assign(new Element(),{value,checked:value==='chart'}));
  const messages=[], live=new Map();let worker, next=0;
  class Worker {constructor(){worker=this;}postMessage(m){messages.push(m);}}
  class LocalURL extends URL {}
  LocalURL.createObjectURL=blob=>{const u=`blob:${++next}`;live.set(u,blob);return u;};
  LocalURL.revokeObjectURL=u=>assert.equal(live.delete(u),true);
  const context=vm.createContext({Worker,URL:LocalURL,Blob,JSON,Date,TextDecoder,location:{href:'https://unit.invalid/'},
    localStorage:{getItem:()=>null,setItem(){}},document:{body:{dataset:{}},getElementById:id=>{assert.ok(e[id],id);return e[id];},
      querySelector:()=>modes.find(m=>m.checked),querySelectorAll:()=>modes,
      createElement:tag=>Object.assign(new Element(),{tag})}});
  vm.runInContext(/<script>([\s\S]*?)<\/script>/.exec(html)[1],context);
  const emit=m=>worker.onmessage({data:m}), last=type=>messages.findLast(m=>m.type===type);
  const sheetDictionaries=['DICT_4X4_50','DICT_5X5_50'];
  emit({type:'ready',info:{zouave_bolero:{style:{}}},sheetDictionaries,version:'test',dxf:'test'});
  assert.deepEqual(e.sheetDictionaries.children.map(option=>option.value),sheetDictionaries);
  const result=(request,extra={})=>emit({type:'result',request:request??last('run').request,foldable:true,bodyCheckable:true,
    res:{title:'current',status:'CONVERGED',basis:{},zip:'round.zip',files:[],validation:{}},blobs:{zip:new Uint8Array([1])},...extra});
  const body=async()=>{e.bodyFile.files=[{arrayBuffer:async()=>new ArrayBuffer(1)}];await e.bodyFile.handlers.change();
    emit({type:'bodyReady',request:last('body').request});};
  const dispatch=(id,event)=>e[id].handlers[event]?.();
  return {e,modes,messages,live,emit,last,result,body,worker,dispatch};
}

test('primary workshop withdraws obsolete rounds and folds while retaining the latest positive workflow', async () => {
  const p=draftingPage();await p.e.go.handlers.click();p.result();await p.body();
  assert.equal(p.e.foldGo.disabled,false);p.e.foldGo.handlers.click();const fold=p.last('fold');
  assert.equal(fold.round,p.last('run').request);assert.equal(fold.body,p.last('body').request);
  p.emit({type:'folded',request:fold.request,report:{settled:true,gating:[]},reportText:'{"value":-0.0}',viewer:'<html/>'});
  assert.equal(p.live.size,3);assert.match(p.e.foldSummary.innerHTML,/no gating flag/);
  assert.ok([...p.live.values()].some(b=>b.type==='application/json'));
  p.e.stature.value='93';p.dispatch('stature','input');
  assert.equal(p.live.size,0);assert.equal(p.e.foldGo.disabled,true);assert.ok(p.e.results.classList.contains('hidden'));
  assert.equal(p.e.status.textContent,'Inputs changed; draft and check again.');
  p.result(fold.round);p.emit({type:'folded',request:fold.request,report:{settled:true},reportText:'{}',viewer:'old'});
  assert.equal(p.live.size,0);assert.equal(p.e.foldGo.disabled,true);
  await p.e.go.handlers.click();p.result();assert.equal(p.e.foldGo.disabled,false);
  p.e.foldGo.handlers.click();const current=p.last('fold');
  p.e.selfContact.handlers.change();assert.equal(p.live.size,1);assert.equal(p.e.foldGo.disabled,false);
  p.emit({type:'folded',request:current.request,reportText:'{}',report:{},viewer:'obsolete'});assert.equal(p.live.size,1);
  const oldRound=p.last('run').request;await p.e.go.handlers.click();
  assert.equal(p.e.foldGo.disabled,true);assert.equal(p.live.size,0);
  p.emit({type:'error',op:'run',request:oldRound,message:'obsolete failure'});assert.ok(p.e.errorBox.classList.contains('hidden'));
  p.emit({type:'error',op:'run',request:p.last('run').request,message:'replacement refused'});
  assert.equal(p.e.errorText.textContent,'replacement refused');assert.equal(p.e.foldGo.disabled,true);
});

test('primary workshop cancels delayed measurement reads and accepts only the latest fields', async () => {
  const p=draftingPage(), pending=deferred();p.modes[0].checked=false;p.modes[2].checked=true;
  p.e.measFile.files=[{text:()=>pending.promise}];const loading=p.e.go.handlers.click();
  p.dispatch('stature','input');pending.resolve('{}');await loading;
  assert.equal(p.last('run'),undefined);assert.equal(p.e.go.disabled,false);
  p.e.measFile.files=[{text:async()=>'{}'}];await p.e.go.handlers.click();p.result();assert.equal(p.live.size,1);
  const old=p.last('fields');p.e.stature.handlers.change();const fresh=p.last('fields');
  p.emit({type:'fields',request:old.request,program:'zouave_bolero',rows:[{name:'old'}]});assert.equal(p.e.tapeRows.innerHTML,undefined);
  p.emit({type:'fields',request:fresh.request,program:'zouave_bolero',rows:[]});assert.equal(p.e.tapeRows.innerHTML,'');
});

test('primary workshop transports original local evidence only with current wearer consent', async () => {
  const p=draftingPage();p.modes[0].checked=false;p.modes[1].checked=true;
  const raw='\n{"schema":"fabric-measurements/1","signed":-0.0}\n',bytes=Buffer.from(raw);
  p.e.supplementalFiles.files=[{size:bytes.length,arrayBuffer:async()=>bytes}];
  await p.e.go.handlers.click();assert.equal(p.last('run'),undefined);
  assert.match(p.e.errorText.textContent,/same wearer/);
  p.e.supplementalConsent.checked=true;await p.e.go.handlers.click();
  const request=p.last('run');assert.equal(request.req.body.supplemental_same_wearer,true);
  assert.deepEqual([...request.req.body.supplemental_measurements],[raw]);
  p.result();assert.equal(p.live.size,1);
  p.dispatch('stature','input');assert.equal(p.e.supplementalConsent.checked,false);
  assert.equal(p.live.size,0);p.result(request.request);assert.equal(p.live.size,0);
  p.modes[1].checked=false;p.modes[2].checked=true;
  p.e.measFile.files=[{text:async()=>'{}'}];p.e.supplementalConsent.checked=true;
  await p.e.go.handlers.click();assert.equal(p.last('run').req.body.mode,'file');
  assert.deepEqual([...p.last('run').req.body.supplemental_measurements],[raw]);
  p.dispatch('measFile','change');assert.equal(p.e.supplementalConsent.checked,false);
});

test('primary workshop rejects invalid evidence bytes and superseded reads without a prior result', async () => {
  const p=draftingPage();p.modes[0].checked=false;p.modes[1].checked=true;
  const pending=deferred();p.e.supplementalFiles.files=[{size:2,arrayBuffer:()=>pending.promise}];
  p.e.supplementalConsent.checked=true;const reading=p.e.go.handlers.click();
  p.dispatch('supplementalFiles','change');pending.resolve(Buffer.from('{}'));await reading;
  assert.equal(p.last('run'),undefined);assert.equal(p.e.supplementalConsent.checked,false);
  p.e.supplementalFiles.files=[{size:2,arrayBuffer:async()=>new Uint8Array([0xc3,0x28])}];
  p.e.supplementalConsent.checked=true;await p.e.go.handlers.click();
  assert.equal(p.last('run'),undefined);assert.match(p.e.errorText.textContent,/encoded data|encoding/i);
  p.e.supplementalFiles.files=[{size:16*1024*1024+1,arrayBuffer:()=>{throw Error('must not read');}}];
  await p.e.go.handlers.click();assert.match(p.e.errorText.textContent,/16 MiB/);
  p.e.supplementalFiles.files=Array(9).fill({size:1,arrayBuffer:()=>{throw Error('must not read');}});
  await p.e.go.handlers.click();assert.match(p.e.errorText.textContent,/at most 8/);
});

test('standalone body checks work without fold placements and retain exact reports without changing readiness', async () => {
  const p=draftingPage();await p.e.go.handlers.click();p.result(undefined,{foldable:false});await p.body();
  assert.equal(p.e.foldGo.disabled,true);assert.equal(p.e.bodyCheckGo.disabled,false);
  const gate=p.e.resGate.innerHTML;p.e.bodyCheckGo.handlers.click();const request=p.last('inspectBody');
  assert.equal(request.round,p.last('run').request);assert.equal(request.body,p.last('body').request);
  assert.equal(p.e.bodyCheckGo.disabled,true);assert.equal(p.last('fold'),undefined);
  const report={cloth_simulated:false,body:{provenance:'MODEL_FIT',mass_kg:12,readings_mm:{chest:500.0000003},
    fit:{tape:{chest:{target_mm:500,body_mm:500.0000003,residual_mm:0.0000003}}}}};
  const raw=JSON.stringify(report);
  p.emit({type:'bodyChecked',request:request.request,report,reportText:raw});
  assert.match(p.e.bodyCheckSummary.innerHTML,/MODEL_FIT/);assert.match(p.e.bodyCheckSummary.innerHTML,/500.0000003/);
  assert.equal(p.e.resGate.innerHTML,gate);assert.equal(p.e.foldGo.disabled,true);
  const saved=[...p.live.values()].find(b=>b.type==='application/json');assert.equal(await saved.text(),raw);
  p.dispatch('stature','input');assert.equal(p.live.size,0);assert.equal(p.e.bodyCheckGo.disabled,true);
  p.emit({type:'bodyChecked',request:request.request,report,reportText:raw});assert.equal(p.live.size,0);
  p.emit({type:'error',op:'inspectBody',request:request.request,message:'old refusal'});
  assert.notEqual(p.e.errorText.textContent,'old refusal');
});

test('body checks require current accepted-source association and withdraw failed or replaced results', async () => {
  const p=draftingPage();await p.e.go.handlers.click();p.result();await p.body();
  const raw=' {"schema":"ftx-body-fit/1","value":-0.0}\n',bytes=Buffer.from(raw);
  p.e.bodyFitFile.files=[{size:bytes.length,arrayBuffer:async()=>bytes}];await p.e.bodyFitFile.handlers.change();
  assert.equal(p.e.bodyCheckGo.disabled,true);p.e.bodyFitConsent.checked=true;p.dispatch('bodyFitConsent','change');
  p.e.bodyCheckGo.handlers.click();const current=p.last('inspectBody');
  assert.equal(current.bodyFit,raw);assert.equal(current.bodyFitConsent,true);
  p.emit({type:'bodyChecked',request:current.request,report:{body:{}},reportText:'{}',bodyFitText:raw});
  assert.equal(p.live.size,3);assert.ok([...p.live.values()].some(b=>b.size===bytes.length));
  p.e.bodyFitConsent.checked=false;p.dispatch('bodyFitConsent','change');assert.equal(p.live.size,1);
  p.e.bodyFitConsent.checked=true;p.dispatch('bodyFitConsent','change');p.e.bodyCheckGo.handlers.click();
  const refused=p.last('inspectBody');p.emit({type:'error',op:'inspectBody',request:refused.request,message:'import refused'});
  assert.equal(p.live.size,1);assert.equal(p.e.errorText.textContent,'import refused');assert.equal(p.last('fold'),undefined);
  p.e.bodyFitFile.files=[{size:1,arrayBuffer:async()=>{throw Error('replacement read failed');}}];
  await p.e.bodyFitFile.handlers.change();assert.equal(p.e.bodyCheckGo.disabled,true);assert.equal(p.live.size,1);
});

test('primary workshop preserves signed body-fit precision, escapes targets and leaves readiness separate', async () => {
  // UI transport fixture only; numerical fitting is exercised by the compiled-browser model lane.
  const p=draftingPage();await p.e.go.handlers.click();p.result();await p.body();
  const readiness=p.e.resStatus.textContent;
  p.e.foldGo.handlers.click();let request=p.last('fold').request;
  p.emit({type:'folded',request,report:{settled:true},reportText:'{}',viewer:'<html/>'});
  assert.match(p.e.foldSummary.innerHTML,/Body-fitting residuals unavailable/);
  p.e.foldGo.handlers.click();request=p.last('fold').request;
  const residual=100.0004-100;
  const report={settled:true,gating:[],body:{provenance:'MODEL_FIT',mass_kg:10.0003,fit:{
    tape:{chest:{target_mm:100,body_mm:100.0004,residual_mm:residual},
      '<img src=x onerror=boom>':{target_mm:100,body_mm:99.9996,residual_mm:-0.0004},
      unavailable:{target_mm:null}},
    weight_kg:{target_kg:10.0001,body_kg:10.0003,residual_kg:10.0003-10.0001}}}};
  const raw=JSON.stringify(report);
  p.emit({type:'folded',request,report,reportText:raw,viewer:'<html/>'});
  const html=p.e.foldSummary.innerHTML;
  assert.match(html,/MODEL_FIT/);assert.ok(html.includes('+'+String(residual)));
  assert.ok(html.includes('100.0004'));assert.ok(html.includes('-0.0004'));
  assert.ok(html.includes('&lt;img'));assert.equal(html.includes('<img src=x'),false);
  assert.ok((html.match(/unavailable/g)||[]).length>=3);
  assert.ok(html.includes('+'+String(10.0003-10.0001)));
  assert.match(html,/do not establish measurement readiness or physical fit/);
  assert.equal(p.e.resStatus.textContent,readiness);
  assert.equal(await [...p.live.values()].find(b=>b.type==='application/json').text(),raw);
  p.e.selfContact.handlers.change();assert.equal(p.e.foldSummary.textContent,'');
  assert.equal(p.live.size,1);
});

test('primary workshop retains exact accepted-fit sources and invalidates replacements without tape fallback', async () => {
  // Explicit UI transport double; actual imported shape/garment checks use native and compiled WASM.
  const p=draftingPage();await p.e.go.handlers.click();p.result();await p.body();
  const readiness=p.e.resStatus.textContent, raw='{ "schema": "ftx-body-fit/1", "v": 1.0000000000000002 }';
  p.e.bodyArmDeg.value='50';
  const select=async text=>{p.e.bodyFitFile.files=[{size:100,arrayBuffer:async()=>new TextEncoder().encode(text).buffer}];await p.e.bodyFitFile.handlers.change();};
  await select(raw);assert.equal(p.e.foldGo.disabled,true);
  p.e.bodyFitConsent.checked=true;p.e.bodyFitConsent.handlers.change();assert.equal(p.e.foldGo.disabled,false);
  p.e.foldGo.handlers.click();const msg=p.last('fold');
  assert.equal(msg.bodyFit,raw);assert.equal(msg.bodyFitConsent,true);assert.equal(msg.armDeg,50);
  const report={settled:true,body:{mass_kg:9,shape_source:{kind:'accepted_multi_frame_fit'},
    measurement_comparison:[{name:'chest',target_mm:100,body_mm:100.0001,residual_mm:0.0001}]}};
  p.emit({type:'folded',request:msg.request,report,reportText:JSON.stringify(report),viewer:'<html/>',bodyFitText:raw});
  assert.match(p.e.foldSummary.innerHTML,/without refitting/);assert.match(p.e.foldSummary.innerHTML,/\+0\.0001/);
  assert.match(p.e.foldSummary.innerHTML,/Original accepted body fit/);
  assert.ok((await Promise.all([...p.live.values()].map(b=>b.text()))).includes(raw));
  assert.equal(p.e.resStatus.textContent,readiness);
  p.e.bodyArmDeg.handlers.input();assert.equal(p.live.size,1); // withdraw report/viewer/source while editing
  const pending=deferred();p.e.bodyFitFile.files=[{size:100,arrayBuffer:()=>pending.promise}];
  const loading=p.e.bodyFitFile.handlers.change();assert.equal(p.e.foldGo.disabled,true);
  await select('replacement');pending.resolve(new TextEncoder().encode(raw).buffer);await loading;
  p.e.bodyFitConsent.checked=true;p.e.bodyFitConsent.handlers.change();p.e.foldGo.handlers.click();
  assert.equal(p.last('fold').bodyFit,'replacement');
  const old=p.last('fold');p.e.bodyFitFile.files=[{size:100,arrayBuffer:async()=>{throw Error('fit read refused');}}];
  await p.e.bodyFitFile.handlers.change();assert.equal(p.e.foldGo.disabled,true);
  p.emit({type:'folded',request:old.request,report,reportText:'{}',viewer:'old',bodyFitText:raw});assert.equal(p.live.size,1);
  p.e.bodyFitFile.files=[{size:16*1024*1024+1,arrayBuffer:async()=>{throw Error('must not read');}}];
  await p.e.bodyFitFile.handlers.change();assert.match(p.e.errorText.textContent,/exceeds 16 MiB/);
  assert.equal(p.e.foldGo.disabled,true);
  p.e.bodyFitFile.files=[{size:2,arrayBuffer:async()=>new Uint8Array([0xff,0xff]).buffer}];
  await p.e.bodyFitFile.handlers.change();assert.equal(p.e.foldGo.disabled,true);
  assert.match(p.e.errorText.textContent,/encoded data|encoding/i);
  await select(raw);p.e.bodyFitConsent.checked=true;p.e.bodyFitConsent.handlers.change();
  p.dispatch('stature','input');assert.equal(p.e.bodyFitConsent.checked,false);
  await p.e.go.handlers.click();p.result();assert.equal(p.e.foldGo.disabled,true);
  p.e.bodyFitFile.files=[];await p.e.bodyFitFile.handlers.change();assert.equal(p.e.foldGo.disabled,false);
  p.e.foldGo.handlers.click();assert.equal(p.last('fold').bodyFit,null);
});

test('primary workshop cannot install a superseded body, revive a cleared model or use a dead worker', async () => {
  const p=draftingPage(), pending=deferred();await p.e.go.handlers.click();p.result();await p.body();
  p.e.bodyFile.files=[{arrayBuffer:()=>pending.promise}];const loading=p.e.bodyFile.handlers.change();
  assert.equal(p.e.foldGo.disabled,true);assert.equal(p.live.size,1);
  await p.body();const current=p.last('body').request;pending.resolve(new ArrayBuffer(2));await loading;
  assert.equal(p.last('body').request,current);assert.equal(p.e.foldGo.disabled,false);
  p.e.bodyFile.files=[];await p.e.bodyFile.handlers.change();p.emit({type:'bodyReady',request:current});assert.equal(p.e.foldGo.disabled,true);
  p.e.bodyFile.files=[{arrayBuffer:async()=>{throw Error('read refused');}}];await p.e.bodyFile.handlers.change();
  assert.equal(p.e.errorText.textContent,'read refused');assert.equal(p.e.foldGo.disabled,true);
  p.emit({type:'error',op:'fold',request:0,fatal:true,message:'trap'});assert.equal(p.live.size,0);assert.equal(p.e.go.disabled,true);
  p.result();assert.equal(p.live.size,0);assert.equal(p.e.go.textContent,'Reload the page');
});

test('finite-stock input and material edits invalidate only their current dependencies', async () => {
  const p = await pageModule(); await p.load(); await p.bind();
  p.select('stockRequest', '{"value":1.0}'); p.e.stockRequest.handlers.change();
  assert.equal(p.e.stock.disabled, false);
  await p.e.stock.onclick();
  assert.equal(p.current().messages.at(-1).stockRequest, '{"value":1.0}');
  p.emit({ markers: [], limitations: [] }, [
    { name: 'plan.json', text: '{}', mime: 'application/json' },
    { name: 'stock-0.svg', text: '<svg/>', mime: 'image/svg+xml' }
  ]);
  assert.equal(p.live.size, 3); assert.equal(p.e.stockMarker.hidden, false);
  p.select('quotes'); p.e.quotes.handlers.change(); assert.equal(p.live.size, 3);
  p.e.stockRequest.handlers.change(); assert.equal(p.live.size, 1); assert.equal(p.e.stockMarker.hidden, true);
  await p.e.stock.onclick(); const old = p.current(), id = old.messages.at(-1).request;
  p.e.stockRequest.handlers.change(); p.ready();
  old.emit({ type: 'result', request: id, files: [{ name: 'stale.json', text: '{}' }] });
  assert.equal(p.live.size, 1);
  await p.inspect('replacement'); p.e.assign.onclick(); assert.equal(p.live.size, 0);
  assert.equal(p.e.stock.disabled, true);
});

const descendants = el => [el, ...el.children.flatMap(descendants)];
const visibleText = el => descendants(el).map(n => n.textContent || '').join('\n');
const section = (p, title) => p.e.outputs.children.find(s => s.children[0].textContent === title);
const outcome = (result = {}) => ({ status: 'available', result });
const refused = message => ({ status: 'refused', refusal: { code: 'MISSING_DATA', message } });
const omitted = () => ({ status: 'not_requested' });
const studyRow = (key = 'A', pattern = '{"pieces":[],"x":-0.0,"value":1.0}') => ({
  key, binding: outcome({ pattern_json: pattern, pattern_sha256: 'pattern-lock', pieces: [{ material: 'retained source' }] }),
  frozen_changes: { default: { id: 'source-A', expected_sha256: 'source-lock' } },
  cutting: refused('missing stock width'), sewing: outcome(), purchasing: omitted(),
  metrics: { stock_area_m2: null, stock_mass_kg: null, stitch_intervals: 0, feed_seconds: 0 }
});
const studyReport = (rows = [studyRow()]) => ({ schema: 'ftx-material-study/1', alternatives: rows,
  source_pattern_sha256: 'original-lock', geometry_changed: false, physical_validation_claim: false, hardware_execution_authorized: false });
function designReport() {
  const designs = ['baseline', '<wider & longer>', 'invalid'];
  const rows = designs.map((key, i) => {
    const row = studyRow(JSON.stringify([key, '<source choice>']));
    row.design = { key, material_key: '<source choice>', source_pattern_sha256: 'original-lock',
      operations: i ? [{ op: 'slash_spread', piece: '<front>', axis: 'x', at_mm: 10, add_mm: 20 }] : [],
      altered_pattern_sha256: i === 2 ? null : `design-lock-${i}`,
      altered_pattern_json: i === 2 ? null : ` {"pieces":[],"x":-0.0,"width":${i ? '120.0' : '100.0'}}\n`,
      geometry_changed: i === 2 ? null : i !== 0, validation: i === 2 ? null : { passed: true } };
    if (i === 2) {
      row.binding = refused('Alteration refused: missing piece <front>');
      row.cutting = row.sewing = row.purchasing = omitted();
    }
    return row;
  });
  return { ...studyReport(rows), schema: 'ftx-design-study-report/1', geometry_changed: true };
}
const screenReport = () => ({ schema: 'ftx-material-screen-report/1', basis: 'reported_bounds', allow_mixed_protocols: false, frontier: ['A'],
  alternatives: [{ key: 'A', eligible: true, dominated_by: [], exclusions: [],
    readings: { areal_density: { value: 0.2, unit: 'kg/m2', bounds: [0.19, 0.21] } }, selection: { id: 'source-A' } },
    { key: '<excluded>', eligible: false, dominated_by: [], exclusions: [{ message: 'missing upper bound' }],
      readings: {}, selection: { id: 'source-B', source: '<literal evidence>' } }] });
const scenarioReport = () => ({ schema: 'ftx-material-scenario-study/1', scenario: { model_sha256: 'model-lock' },
  manufacturing: studyReport([studyRow('A'), studyRow('B')]),
  // Deliberately not manufacturing order: the UI must join on stable keys, not array positions.
  simulations: [{ key: 'B', evaluation: refused('missing mechanics') }, { key: 'A', evaluation: outcome({
    metrics: { simulation_gating_clear: false, cloth_mass_kg: 0, independent_surface_power_w: -4.5, body_coverage_power_w: null },
    surface: outcome(), coverage: refused('missing conductivity'), drape: { settled: false, flags: ['unsettled'] },
    body_geometry_sha256: 'body-lock', positions_sha256: 'positions-lock' }) }] });
function scenarioMatrixReport() {
  const cold=scenarioReport(), warm=scenarioReport();
  const candidate=(key,value,unit,eligible=true)=>({key,eligible,dominated_by:[],
    exclusions:eligible?[]:[{message:'missing mechanics in this case'}],readings:value==null?{}:{[unit==='W'?
      'independent_surface_power_w':'motion_covered_vapor_potential_energy_j']:{value,unit,
      ...(unit==='J'?{reported_bounds:[14,28]}:{})}}});
  cold.comparison={policy:{basis:'nominal',allow_mixed_protocols:false,objectives:[
    {metric:'independent_surface_power_w',direction:'minimize'}],constraints:[]},
    motion_required:false,frontier:['A'],incomparable:[],alternatives:[candidate('A',-4.5,'W'),candidate('B',-3,'W')]};
  cold.comparison.alternatives[1].dominated_by=['A'];
  warm.comparison={policy:{basis:'reported_bounds',allow_mixed_protocols:false,objectives:[
    {metric:'motion_covered_vapor_potential_energy_j',direction:'maximize'}],constraints:[]},
    motion_required:true,frontier:['A'],incomparable:[],alternatives:[candidate('A',17.5,'J'),candidate('B',null,'J',false)]};
  for (const [index,report] of [cold,warm].entries()) {
    const initial=report.simulations.find(row=>row.key==='A').evaluation.result;
    initial.drape={settled:true,flags:[]}; initial.metrics.simulation_gating_clear=true;
    const coverage=vaporCoverageReport(), history=vaporHistoryReport();
    coverage.replay_request_json=` {"schema":"ftx-vapor-coverage/1","source":"case ${index+1}","value":1.0,"x":-0.0}\n`;
    history.replay_request_json=` {"schema":"ftx-vapor-coverage-history/1","source":"case ${index+1}","value":1.0,"x":-0.0}\n`;
    history.totals.reported_bounds={potential_energy_j:[14,28],potential_mass_kg:[0.0000056,0.0000112]};
    initial.vapor_coverage=outcome(coverage);
    initial.motion=outcome({duration_s:1,trials:1,samples:[],keyframes:[{time_s:1,arm_abduction_deg:40,
      surface:omitted(),coverage:omitted(),vapor_coverage:outcome(coverage)}],vapor_coverage_history:outcome(history)});
  }
  const successfulB=plain(cold.simulations.find(row=>row.key==='A'));
  successfulB.key='B'; successfulB.evaluation.result.metrics.independent_surface_power_w=-3;
  successfulB.evaluation.result.motion=omitted(); successfulB.evaluation.result.vapor_coverage=omitted();
  cold.simulations[0]=successfulB;
  warm.simulations[0].evaluation=refused('missing mechanics in this case');
  const cases=[{key:'<cold>',request_sha256:'cold-lock',scenario:cold.scenario,simulations:cold.simulations,comparison:cold.comparison},
    {key:'warm',request_sha256:'warm-lock',scenario:warm.scenario,simulations:warm.simulations,comparison:warm.comparison}];
  return {schema:'ftx-material-scenario-matrix-study/1',request_sha256:'matrix-lock',max_evaluations:4,
    planned_evaluations:4,evaluations:4,manufacturing:cold.manufacturing,cases,notes:['Synthetic frontend matrix; no empirical claim.'],
    comparison:{schema:'ftx-material-scenario-matrix-comparison/1',case_order:cases.map(row=>row.key),
      policies:Object.fromEntries(cases.map(row=>[row.key,row.comparison.policy])),frontier:['A'],incomparable:[],
      alternatives:['A','B'].map(key=>({key,eligible:key==='A',dominated_by:[],
        cases:Object.fromEntries(cases.map(row=>[row.key,row.comparison.alternatives.find(a=>a.key===key)]))}))}};
}
function scenarioInputs(p) {
  p.select('studyRequest', '{"value":1.0}'); p.select('scenarioRequest', '{"temperature":293.0}');
  p.select('measurements', '{"length":10.0}'); p.select('body');
  p.e.body.files[0].arrayBuffer = async () => new ArrayBuffer(8); p.e.body.handlers.change();
}

test('named scenario matrices share manufacturing, retain native case decisions and publish distinct exact replays', async () => {
  const p=await pageModule(); await p.load(); scenarioInputs(p);
  await p.e.study.onclick(); p.emit(studyReport()); const priorUrls=new Set(p.live);
  const input=' {"schema":"ftx-drape-scenario-matrix/1","max_evaluations":4,"cases":[{"key":"<cold>","scenario":{"x":-0.0}},{"key":"warm","scenario":{"temperature":303.0}}]}\n';
  p.select('scenarioRequest',input); p.e.scenarioRequest.handlers.change(); await p.e.scenario.onclick();
  const request=p.current().messages.at(-1), report=scenarioMatrixReport(), raw=` ${JSON.stringify(report)}\n`;
  const w=await workerModule({bindings:{default:()=>Promise.resolve(),WebBody:class {free(){}},
    material_scenario_study:(_body,pattern,study,matrix,body)=>{
      assert.equal(matrix,input); assert.equal(pattern,request.pattern); assert.equal(study,request.studyRequest);
      assert.equal(body,request.measurements); return raw;}}});
  p.current().emit(await w.send(request));
  const s=section(p,'Fresh drape and heat alternatives'), tables=descendants(s).filter(n=>n.tag==='table');
  assert.equal(s.hidden,false);
  assert.equal(tables.filter(t=>t.children[0].textContent==='Independent manufacturing outcomes').length,1);
  const shared=descendants(s).find(n=>n.tag==='section'&&n.children[0]?.textContent==='Shared manufacturing — evaluated once');
  assert.ok(shared); assert.equal(descendants(shared).filter(n=>n.download?.endsWith('-pattern.json')).length,2);
  const groups=['Case 1: <cold>','Case 2: warm'].map(title=>
    descendants(s).find(n=>n.tag==='section'&&n.children[0]?.textContent===title));
  assert.ok(groups.every(Boolean)); assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  assert.equal(groups.some(g=>visibleText(g).includes('Independent manufacturing outcomes')),false);
  assert.match(visibleText(groups[0]),/Computed nominal frontier: A/);
  assert.match(visibleText(groups[1]),/Computed reported_bounds frontier: A/);
  assert.match(visibleText(groups[1]),/missing mechanics in this case/);
  const initial=g=>descendants(g).find(n=>n.tag==='table'&&n.children[0].textContent.startsWith('Initial static-drape results'));
  assert.equal(initial(groups[0]).children[3].children[5].textContent,'-3');
  assert.equal(initial(groups[1]).children[3].children[5].textContent,'Unknown');
  assert.match(visibleText(s),/Computed cross-case frontier: A/);
  const aggregate=tables.find(t=>t.children[0].textContent==='Named scenario decisions — authoritative Rust comparison');
  assert.deepEqual(plain(aggregate.children[2].children.map(n=>n.textContent)),
    ['A','Eligible','None reported','<cold>','Eligible','independent_surface_power_w: -4.5 W']);
  assert.deepEqual(plain(aggregate.children[3].children.map(n=>n.textContent)),
    ['A','Eligible','None reported','warm','Eligible','motion_covered_vapor_potential_energy_j: 17.5 J; reported range [14, 28] J']);
  assert.deepEqual(plain(aggregate.children[5].children.map(n=>n.textContent)),['B','Excluded','None reported','warm',
    'Excluded: missing mechanics in this case','motion_covered_vapor_potential_energy_j: Unknown']);
  const expected=[['material-scenario-matrix-study.json',raw],['scenario-request.json',input]];
  for (const [index,item] of report.cases.entries()) {
    const a=item.simulations.find(row=>row.key==='A').evaluation.result, prefix=`case-${index+1}-alternative-1-`;
    expected.push([`${prefix}vapor-coverage-replay.json`,a.vapor_coverage.result.replay_request_json],
      [`${prefix}motion-vapor-coverage-replay.json`,a.motion.result.vapor_coverage_history.result.replay_request_json],
      [`${prefix}motion-endpoint-1-vapor-coverage-replay.json`,a.motion.result.keyframes[0].vapor_coverage.result.replay_request_json]);
  }
  for (const [name,text] of expected) {
    const links=descendants(s).filter(n=>n.download===name); assert.equal(links.length,1,name);
    assert.equal(await p.blobs.get(links[0].href).text(),text);
  }
  const downloads=descendants(s).filter(n=>n.download).map(n=>n.download);
  assert.equal(new Set(downloads).size,downloads.length);
  const summary=JSON.parse(s.children[1].textContent);
  assert.equal(summary.planned_evaluations,4); assert.equal(summary.evaluations,4); assert.equal(summary.max_evaluations,4);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden,true); assert.deepEqual(p.live,priorUrls);
  assert.equal(section(p,'Fixed-pattern manufacturing alternatives').hidden,false);
});

test('matrix policies remain optional per case and replacement cancels every stale case result and replay', async () => {
  const p=await pageModule(); await p.load(); scenarioInputs(p);
  const report=scenarioMatrixReport(); delete report.comparison; delete report.cases[0].comparison;
  report.notes=['Case <cold> has no effective comparison policy.'];
  await p.e.scenario.onclick(); p.emit(report);
  const s=section(p,'Fresh drape and heat alternatives');
  assert.match(visibleText(s),/No cross-case comparison is available/);
  assert.match(visibleText(s),/Case <cold> has no effective comparison policy/);
  assert.equal(descendants(s).filter(n=>n.tag==='table'&&n.children[0].textContent.startsWith('Declared computed objectives')).length,1);
  assert.equal(descendants(s).filter(n=>n.tag==='table'&&n.children[0].textContent.startsWith('Initial static-drape results')).length,2);
  const delay=deferred(); p.e.scenarioRequest.files[0].text=()=>delay.promise;
  const old=p.current(), messages=old.messages.length, running=p.e.scenario.onclick();
  p.select('scenarioRequest','{"schema":"ftx-drape-scenario-matrix/1","cases":[]}');
  p.e.scenarioRequest.handlers.change(); p.ready(); delay.resolve('{"schema":"ftx-drape-scenario-matrix/1"}'); await running;
  assert.equal(old.dead,true); assert.equal(old.messages.length,messages); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
  await p.e.scenario.onclick(); const stale=p.current(), id=stale.messages.at(-1).request;
  p.e.body.handlers.change(); p.ready();
  stale.emit({type:'result',request:id,result:report,files:[]});
  assert.equal(s.hidden,true); assert.equal(p.live.size,0);
  await p.e.scenario.onclick(); p.emit(report); assert.equal(s.hidden,false);
  p.e.scenarioBody.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
});

function addChoice(p, group, choice, pieces = ['front']) {
  p.e.combinationGroup.value = group; p.e.combinationChoice.value = choice;
  p.e.combinationScope.value = pieces === null ? 'default' : 'pieces';
  for (const option of p.e.combinationPieces.children) option.selected = pieces?.includes(option.value) || false;
  p.e.addCombination.onclick();
}

test('combination controls retain frozen selections and exact policy/pair numbers for native study evaluation', async () => {
  for (const schema of ['ftx-material-study/1','ftx-design-study/1']) {
  const p = await pageModule(); await p.load();
  p.e.combinationMode.checked = true; p.e.combinationMode.handlers.change();
  assert.equal(p.e.study.disabled, true);
  const designs = schema==='ftx-design-study/1' ? ',"designs":[{"key":"wider","operations":[{"op":"slash_spread","piece":"front","axis":"x","at_mm":10.0,"add_mm":8.0}]}]' : '';
  const policy = `{"schema":"${schema}"${designs},"assembly_mass":{"schema":"ftx-assembly-mass/1","inventory_complete":true,"extras":[],"seam_allowance_mm":-0.0}}`;
  p.select('studyRequest', policy); p.e.studyRequest.handlers.change();
  await p.inspect('light'); addChoice(p, 'shell', 'light');
  await p.inspect('heavy'); addChoice(p, 'shell', 'heavy');
  await p.inspect('lining'); addChoice(p, 'lining', 'soft', ['lining']);
  assert.match(p.e.combinationCount.textContent, /^2 complete material combinations/);
  const pairs = '[{"materials":["a","b"],"friction":{"value":1.0,"bounds":[-0.0,1.0]}}]';
  p.select('combinationInterfaces', pairs); p.e.combinationInterfaces.handlers.change();
  assert.equal(p.e.study.disabled, false);
  await p.e.study.onclick();
  const sent = p.current().messages.at(-1);
  assert.equal(sent?.type,'study',p.e.error.textContent);
  const request = JSON.parse(sent.studyRequest);
  assert.equal(request.schema,schema);
  if (designs) assert.ok(sent.studyRequest.includes(designs));
  assert.ok(sent.studyRequest.includes('"seam_allowance_mm":-0.0'));
  assert.ok(sent.studyRequest.includes(pairs));
  assert.equal(request.combinations.groups[0].choices[0].selection.expected_sha256, 'locked');
  assert.deepEqual(plain(request.combinations.groups.map(g => [g.key, g.target])),
    [['shell', {scope:'pieces',ids:['front']}], ['lining',{scope:'pieces',ids:['lining']}]]);
  assert.equal(Object.hasOwn(request, 'variants'), false);
  assert.equal(sent.pattern, JSON.stringify({pieces:[{id:'front'},{id:'lining'}]}));
  const row = studyRow('generated'); row.choices = {lining:'soft', shell:'heavy'};
  p.emit(studyReport([row]));
  const result = section(p, 'Fixed-pattern manufacturing alternatives');
  assert.match(visibleText(result), /lining: soft; shell: heavy/);
  assert.equal(descendants(result).some(n => 'innerHTML' in n), false);
  await p.inspect('replacement'); addChoice(p, 'shell', 'heavy');
  assert.equal(result.hidden, true); assert.equal(p.live.size, 0);
  assert.match(p.e.combinationCount.textContent, /^2 complete/);
  await p.e.study.onclick();
  assert.equal(JSON.parse(p.current().messages.at(-1).studyRequest).combinations.groups[0].choices[1].selection.id, 'replacement');
  }
});

test('combination controls reject overlapping targets, changed group targets, excessive products and ambiguous study sources', async () => {
  const p = await pageModule(); await p.load(); await p.inspect('source');
  p.e.combinationMode.checked = true;
  addChoice(p, 'shell', 'one');
  addChoice(p, 'other', 'one'); assert.match(p.e.error.textContent, /disjoint/);
  addChoice(p, 'shell', 'two', ['lining']); assert.match(p.e.error.textContent, /same target/);
  for (let i = 2; i <= 32; i++) addChoice(p, 'shell', `choice-${i}`);
  assert.match(p.e.combinationCount.textContent, /^32 complete/);
  addChoice(p, 'shell', 'too many'); assert.match(p.e.error.textContent, /limit of 32/);
  assert.match(p.e.combinationCount.textContent, /^32 complete/);
  for (const schema of ['ftx-material-study/1','ftx-design-study/1']) for (const source of ['variants','screened','combinations']) {
    p.select('studyRequest',JSON.stringify({schema,[source]:[]}));
    const before = p.current().messages.length; await p.e.study.onclick();
    assert.equal(p.current().messages.length, before);
    assert.match(p.e.error.textContent, /must omit variants/);
  }
  for (const policy of ['null','[]','"not an object"','{"schema":"wrong"}']) {
    p.select('studyRequest',policy);const before=p.current().messages.length;await p.e.study.onclick();
    assert.equal(p.current().messages.length,before);assert.match(p.e.error.textContent,/policy object/);
  }
  p.e.clearCombinations.onclick(); assert.equal(p.e.study.disabled, true);
  addChoice(p, 'base', 'one', null); addChoice(p, 'another base', 'two', null);
  assert.match(p.e.error.textContent, /Only one group/);
  assert.match(p.e.combinationCount.textContent, /^1 complete/);
});

test('combination changes and pattern replacement cancel pending reads and cannot publish stale alternatives', async () => {
  const p = await pageModule(); await p.load(); await p.inspect('source');
  p.e.combinationMode.checked = true; addChoice(p, 'shell', 'one');
  p.select('studyRequest','{"schema":"ftx-material-study/1"}');
  const delayed = deferred(); p.e.studyRequest.files[0].text = () => delayed.promise;
  const old = p.current(), running = p.e.study.onclick();
  p.e.combinationMode.checked = false; p.e.combinationMode.handlers.change(); p.ready();
  delayed.resolve('{"schema":"ftx-material-study/1"}'); await running;
  assert.equal(old.messages.filter(m => m.type === 'study').length, 0);
  p.e.combinationMode.checked = true;
  p.select('studyRequest','{"schema":"ftx-material-study/1"}');
  await p.e.study.onclick(); const previous = p.current(), id = previous.messages.at(-1).request;
  await p.load(); p.ready();
  previous.emit({type:'result',request:id,result:studyReport(),files:[{name:'stale.json',text:'{}'}]});
  assert.equal(p.live.size, 0); assert.equal(p.e.study.disabled, true);
  assert.equal(p.e.combinationGroups.children.length, 0);
  assert.equal(p.e.combinationPieces.children.length, 2);
});

test('purchasing displays exact intraday evidence, excluded latest offers and date-only history without repricing', async () => {
  const p = await pageModule(); await p.load(); await p.bind();
  const quotes = '{"schema":"ftx-textile-quotes/2","price":1.0}';
  const market = '{"as_of":"2026-10-20","as_of_at":"2026-10-20T12:00:00.000000001Z"}';
  p.select('cutOptions'); p.select('quotes', quotes); p.select('market', market);
  p.e.market.handlers.change(); await p.e.price.onclick();
  const sent = p.current().messages.at(-1);
  assert.equal(sent.quotes, quotes); assert.equal(sent.market, market);
  const scope = { currency: 'USD', region: 'US', quality: 'reviewed', terms: 'merchandise only',
    as_of: '2026-10-20', as_of_at: '2026-10-20T12:00:00.000000001Z' };
  const quote = { offer_id: '<literal supplier>', observed_on: '2026-10-20',
    observed_at: '2026-10-20T12:00:00Z', valid_through_at: '2026-10-20T12:00:00.000000001Z', unit: 'linear_meter',
    maximum_continuous_quantity: 5 };
  const report = { complete_offer_coverage: false, complete_observed_quantity_coverage: false,
    complete_continuous_length_limit_coverage: false,
    currency: 'USD', least_observed_independent_order_total: null, limits: ['Observed prices only'], roll_prices: [
      { roll_index: 0, required_stock_area_m2: 0.5, required_length_m: 0.5, sample: { scope,
        offers: [{ quote, order_quantity: 0.5, order_cost: 7.25, within_observed_continuous_limit: true }],
        excluded_offers: [{ quote: { ...quote, offer_id: 'expired', valid_through_at: '2026-10-20T12:00:00Z' }, reason: 'expired' },
          { quote: { ...quote, offer_id: 'limited' }, reason: 'insufficient_quantity', required_order_quantity: 1.5 },
          { quote: { ...quote, offer_id: 'short-cut', maximum_continuous_quantity: 0.4 },
            reason: 'insufficient_continuous_length', required_continuous_length_m: 0.5 }] } },
      { roll_index: 1, required_stock_area_m2: 1, sample: { scope: { ...scope, as_of_at: undefined }, offers: [],
        excluded_offers: [{ quote: { offer_id: 'daily', observed_on: '2026-10-20', valid_through: '2026-10-20' }, reason: 'unavailable' }] } }
    ] };
  p.emit(report, [{ name: 'quotes.json', text: quotes, mime: 'application/json' }]);
  const s = section(p, 'price'), tables = descendants(s).filter(n => n.tag === 'table');
  assert.deepEqual(plain(tables[0].children[2].children.map(n => n.textContent)),
    ['<literal supplier>', '2026-10-20T12:00:00Z', '2026-10-20T12:00:00.000000001Z', 'Eligible', '0.5 linear_meter', '7.25',
      '5 linear_meter; within observed limit']);
  assert.equal(tables[0].children[3].children[3].textContent, 'Excluded: expired');
  assert.equal(tables[0].children[3].children[5].textContent, 'Unknown');
  assert.equal(tables[0].children[4].children[4].textContent, '1.5 linear_meter');
  assert.equal(tables[0].children[5].children[3].textContent, 'Excluded: insufficient continuous length');
  assert.equal(tables[0].children[5].children[6].textContent, '0.4 linear_meter');
  assert.equal(tables[1].children[2].children[1].textContent, '2026-10-20 (date only)');
  assert.equal(tables[1].children[2].children[6].textContent, 'Not reported');
  assert.match(visibleText(s), /one continuous 0.5 m cut/);
  assert.match(visibleText(s), /not a guarantee of continuous inventory/);
  assert.match(visibleText(s), /Evaluated at 2026-10-20T12:00:00\.000000001Z UTC/);
  assert.match(visibleText(s), /Evaluated at 2026-10-20 \(date only\) UTC/);
  assert.equal(JSON.parse(s.children[1].textContent).least_observed_independent_order_total, null);
  assert.equal(JSON.parse(s.children[1].textContent).complete_continuous_length_limit_coverage, false);
  assert.equal(descendants(s).some(n => 'innerHTML' in n), false);
  const link = descendants(s).find(n => n.download === 'quotes.json');
  assert.equal(await p.blobs.get(link.href).text(), quotes);
  p.select('market', market.replace('000000001', '000000002')); p.e.market.handlers.change();
  assert.equal(s.hidden, true); assert.equal(s.children[2].children.length, 0);
  assert.equal(p.live.has(link.href), false);
});

test('bounded-marker purchasing shows one whole order and preserves exact totals and stock exclusions', async () => {
  const p = await pageModule();await p.load();await p.bind();
  const options='{"copies":3,"maximum_marker_length_m":0.12,"gap_mm":1.0}', quotes='{"schema":"ftx-textile-quotes/1","quotes":[]}', market='{"as_of":"2026-10-21"}';
  p.select('cutOptions',options);p.select('quotes',quotes);p.select('market',market);
  await p.e.price.onclick();
  assert.equal(p.current().messages.at(-1).options,options);
  const tier={minimum_quantity:0.25,price:10,sku:'<bulk-lot>'};
  const quote={offer_id:'<whole order>',observed_on:'2026-10-21',unit:'linear_meter',price:12,quantity:1,price_breaks:[tier],
    available_quantity:0.25,maximum_continuous_quantity:0.12};
  const offer={quote,order_quantity:0.25,order_cost:2.5,applied_price_break:tier,quantity_sufficiency_observed:true,within_observed_continuous_limit:true};
  const report={schema:'ftx-cut-procurement/2',currency:'USD',complete_offer_coverage:true,
    complete_observed_quantity_coverage:true,complete_continuous_length_limit_coverage:true,
    least_observed_independent_order_total:2.5,limits:['Requested segmentation, not guaranteed supplier fulfillment'],
    roll_prices:[0,1].map(roll_index=>({roll_index,material_order_index:0,required_length_m:0.12,required_stock_area_m2:0.012,continuity:'within_observed_limit'})),
    material_orders:[{material_order_index:0,material_sha256:'same-exact-material',roll_indices:[0,1],marker_lengths_m:[0.12,0.12],
      required_length_m:0.24,required_stock_area_m2:0.024,required_continuous_length_m:0.12,least_observed_order:offer,
      sample:{scope:{currency:'USD',region:'US',quality:'synthetic',terms:'merchandise only',as_of:'2026-10-21'},offers:[offer],
        excluded_offers:[{quote:{...quote,offer_id:'short stock',available_quantity:0.2},reason:'insufficient_quantity',required_order_quantity:0.25}]}}]};
  const raw=JSON.stringify(report);
  p.emit(report,[{name:'cut-price.json',text:raw,mime:'application/json'}]);
  const s=section(p,'price'),tables=descendants(s).filter(n=>n.tag==='table');
  assert.equal(tables.length,1); // Two markers consume one order and show one selected price row.
  assert.match(tables[0].children[0].textContent,/Material order 1/);
  assert.deepEqual(plain(tables[0].children[2].children.slice(3,7).map(n=>n.textContent)),
    ['Eligible','0.25 linear_meter','2.5','0.12 linear_meter; within observed limit']);
  assert.deepEqual(plain(tables[0].children[3].children.slice(3,6).map(n=>n.textContent)),
    ['Excluded: insufficient quantity','0.25 linear_meter','Unknown']);
  assert.equal(tables[0].children[2].children[7].textContent,'From 0.25 linear_meter: 10 USD per 1 linear_meter; SKU <bulk-lot>');
  assert.equal(tables[0].children[3].children[7].textContent,'Not applied: excluded');
  assert.match(visibleText(s),/Markers 1, 2 require 0.12, 0.12 m respectively/);
  assert.match(visibleText(s),/total 0.24 m, longest continuous segment 0.12 m/);
  assert.match(visibleText(s),/available stock apply once to the combined material order/);
  assert.equal(JSON.parse(s.children[1].textContent).least_observed_independent_order_total,2.5);
  const source=descendants(s).find(n=>n.tag==='details');source.open=true;source.handlers.toggle();
  assert.match(visibleText(source),/"available_quantity": 0.2/);
  assert.match(visibleText(source),/"available_quantity": 0.25/);
  const link=descendants(s).find(n=>n.download==='cut-price.json');
  assert.equal(await p.blobs.get(link.href).text(),raw);
  assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  p.e.cutOptions.handlers.change();
  assert.equal(s.hidden,true);assert.equal(p.live.has(link.href),false);
});

test('marker selection forwards a zero-based index and withdraws stale preview replies independently of price', async () => {
  const p=await pageModule();await p.load();await p.bind();
  const options='{"copies":3,"maximum_marker_length_m":0.12,"width_basis":"reported_lower_bound"}';
  p.select('cutOptions',options);
  p.select('quotes');p.select('market');await p.e.price.onclick();
  p.emit({roll_prices:[],least_observed_independent_order_total:2.5},[{name:'price.json',text:'{"total":2.5}',mime:'application/json'}]);
  const price=section(p,'price'), priceLink=descendants(price).find(n=>n.download==='price.json');
  p.e.rollPreview.value='2';await p.e.cut.onclick();
  assert.equal(p.current().messages.at(-1).rollIndex,1);
  assert.equal(p.current().messages.at(-1).options,options);
  const layout={width_m:0.075,reported_bounds_m:[0.075,0.11],notes:['Fixed source lower width; nominal purchase width retained']};
  const plan={schema:'ftx-roll-cut-plan/2',options:JSON.parse(options),
    rolls:[{width_m:0.1,layout_width:layout,length_m:0.12},{width_m:0.1,layout_width:layout,length_m:0.12}],limitations:[]};
  p.emit(plan,[{name:'cut-plan.json',text:JSON.stringify(plan),mime:'application/json'},
    {name:'roll-1.svg',text:'<svg id="second"/>',mime:'image/svg+xml'}]);
  const cut=section(p,'cut'), preview=p.e.marker.src;
  assert.equal(p.e.marker.hidden,false);
  assert.equal(await p.blobs.get(preview).text(),'<svg id="second"/>');
  const summary=JSON.parse(cut.children[1].textContent);
  assert.equal(summary.maximum_marker_length_m,0.12);
  assert.deepEqual(summary.rolls.map(r=>r.marker_number),[1,2]);
  assert.equal(summary.rolls[1].width_m,0.1);
  assert.deepEqual(summary.rolls[1].layout_width,layout);
  p.e.rollPreview.value='1';p.e.rollPreview.handlers.change();
  assert.equal(cut.hidden,true);assert.equal(p.e.marker.hidden,true);assert.equal(p.live.has(preview),false);
  assert.equal(price.hidden,false);assert.equal(p.live.has(priceLink.href),true);
  await p.e.cut.onclick();const old=p.current(), request=old.messages.at(-1).request;
  p.e.rollPreview.value='2';p.e.rollPreview.handlers.change();assert.equal(old.dead,true);p.ready();
  old.emit({type:'result',request,result:plan,files:[{name:'stale.svg',text:'<svg/>',mime:'image/svg+xml'}]});
  assert.equal(cut.hidden,true);assert.equal(p.e.marker.hidden,true);
  assert.equal(price.hidden,false);
  for (const value of ['0','-1','1.5','Infinity','9007199254740992']) {
    p.e.rollPreview.value=value;const before=p.current().messages.length;await p.e.cut.onclick();
    assert.equal(p.current().messages.length,before);assert.match(p.e.error.textContent,/positive integer/);
  }
});

test('grouped marker purchasing displays each whole material order once without per-marker samples', async () => {
  const p=await pageModule(); await p.load(); await p.bind();
  p.select('cutOptions'); p.select('quotes'); p.select('market'); await p.e.price.onclick();
  const scope={currency:'USD',region:'US',quality:'test',terms:'test',as_of:'2026-10-21'};
  const quote={offer_id:'<literal supplier>',observed_on:'2026-10-21',unit:'linear_meter',maximum_continuous_quantity:0.9};
  const report={schema:'ftx-cut-procurement/2',currency:'USD',complete_offer_coverage:false,
    complete_observed_quantity_coverage:false,complete_continuous_length_limit_coverage:false,
    least_observed_independent_order_total:null,limits:['Synthetic display data only'],
    roll_prices:[{roll_index:0,material_order_index:0},{roll_index:1,material_order_index:0},{roll_index:2,material_order_index:1}],
    material_orders:[{material_order_index:0,roll_indices:[0,1],marker_lengths_m:[0.7,0.8],required_stock_area_m2:1.5,
      required_length_m:1.5,required_continuous_length_m:0.8,sample:{scope,
        offers:[{quote,order_quantity:2,order_cost:7.25,within_observed_continuous_limit:true}],excluded_offers:[]}},
      {material_order_index:1,roll_indices:[2],marker_lengths_m:[0.5],required_stock_area_m2:0.5,required_length_m:0.5,
        required_continuous_length_m:0.5,sample:{scope,offers:[],excluded_offers:[{
          quote:{...quote,offer_id:'stock-lost'},reason:'insufficient_quantity',required_order_quantity:1}]}}]};
  const raw=' '+JSON.stringify(report).replace('"order_cost":7.25','"order_cost":7.2500')+'\n';
  p.emit(report,[{name:'cut-price.json',text:raw,mime:'application/json'}]);
  const s=section(p,'price'), tables=descendants(s).filter(n=>n.tag==='table');
  assert.equal(tables.length,2);
  assert.equal(tables[0].children[0].textContent,'Material order 1 — latest supplier observations');
  assert.deepEqual(plain(tables[0].children[2].children.map(n=>n.textContent)),
    ['<literal supplier>','2026-10-21 (date only)','Not reported','Eligible','2 linear_meter','7.25','0.9 linear_meter; within observed limit']);
  assert.match(visibleText(s),/Markers 1, 2 require 0.7, 0.8 m respectively; total 1.5 m, longest continuous segment 0.8 m/);
  assert.match(visibleText(s),/Minimum quantity, order increments and available stock apply once/);
  assert.equal(tables[1].children[2].children[3].textContent,'Excluded: insufficient quantity');
  assert.equal(JSON.parse(s.children[1].textContent).least_observed_independent_order_total,null);
  assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  const link=descendants(s).find(n=>n.download==='cut-price.json');
  assert.equal(await p.blobs.get(link.href).text(),raw);
  p.e.market.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.has(link.href),false);
});

test('screening is available without a pattern and retains excluded source evidence for inspection', async () => {
  const p = await pageModule(); assert.equal(p.e.screen.disabled, true);
  p.select('screenRequest', '{"threshold":1.0}'); p.e.screenRequest.handlers.change();
  assert.equal(p.e.screen.disabled, false); assert.equal(p.e.study.disabled, true);
  await p.e.screen.onclick(); assert.equal(p.current().messages.at(-1).screenRequest, '{"threshold":1.0}');
  p.emit(screenReport());
  const s = section(p, 'Source-qualified screening');
  assert.match(visibleText(s), /reported_bounds/); assert.match(visibleText(s), /missing upper bound/);
  assert.match(visibleText(s), /0.2 kg\/m2; \[0.19, 0.21\] reported bounds/); assert.match(visibleText(s), /Unknown/);
  assert.match(visibleText(s), /not garment-level ranking/);
  const details = descendants(s).filter(n => n.tag === 'details');
  details[1].open = true; details[1].handlers.toggle();
  assert.match(visibleText(s), /<literal evidence>/);
  assert.equal(descendants(s).some(n => 'innerHTML' in n), false);
});

test('manufacturing studies use original lossless source, not bound previews, and download exact alternative patterns', async () => {
  const p = await pageModule(), original = '{"pieces":[{"id":"front"}],"x":-0.0,"coefficient":1.0}';
  p.select('patternFile', original); await p.e.patternFile.onchange();
  p.select('studyRequest', '{"coefficient":1.0}'); p.e.studyRequest.handlers.change();
  assert.equal(p.e.study.disabled, false); assert.equal(p.e.cut.disabled, true);
  await p.e.bind.onclick(); p.emit({ pattern_json: '{"different":"preview"}', pieces: [] });
  await p.e.study.onclick(); const request = p.current().messages.at(-1);
  assert.equal(request.pattern, original); assert.equal(request.studyRequest, '{"coefficient":1.0}');
  const row = studyRow('../<unsafe-name>'); p.emit(studyReport([row]));
  const s = section(p, 'Fixed-pattern manufacturing alternatives');
  assert.match(visibleText(s), /missing stock width/); assert.match(visibleText(s), /Available/);
  const t = descendants(s).find(n => n.tag === 'table'); const values = t.children[2].children.map(n => n.textContent);
  assert.equal(values[7], 'Unknown'); assert.equal(values[8], 'Unknown');
  assert.equal(values[9], '0'); assert.equal(values[10], '0');
  const a = descendants(s).find(n => n.download);
  assert.equal(a.download, 'alternative-1-pattern.json');
  assert.equal(await p.blobs.get(a.href).text(), row.binding.result.pattern_json);
  p.windowEvents.pagehide(); assert.equal(p.live.size, 0);
});

test('design alternatives retain baseline, refused geometry and exact altered and material-bound patterns', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  const report = designReport(); await p.e.study.onclick(); p.emit(report);
  const s = section(p, 'Fixed-pattern manufacturing alternatives');
  const designs = descendants(s).find(n => n.tag === 'table' && n.children[0].textContent === 'Pattern designs and material choices');
  assert.ok(designs); assert.equal(designs.children.length, 5);
  assert.deepEqual(plain(designs.children[2].children.map(n => n.textContent)),
    [report.alternatives[0].key, 'baseline', '<source choice>', '0', 'Unchanged']);
  assert.equal(designs.children[3].children[1].textContent, '<wider & longer>');
  assert.equal(designs.children[3].children[4].textContent, 'Changed');
  assert.equal(designs.children[4].children[4].textContent, 'Unavailable');
  assert.match(visibleText(s), /Alteration refused: missing piece <front>/);
  assert.equal(descendants(s).some(n => 'innerHTML' in n), false);
  for (const [index, row] of report.alternatives.entries()) {
    for (const [suffix, text] of [['design-pattern', row.design.altered_pattern_json], ['pattern', row.binding.result?.pattern_json]]) {
      const links = descendants(s).filter(n => n.download === `alternative-${index + 1}-${suffix}.json`);
      assert.equal(links.length, text == null ? 0 : 1);
      if (text != null) assert.equal(await p.blobs.get(links[0].href).text(), text);
    }
  }
  const urls = new Set(p.live); assert.equal(urls.size, 4);
  p.e.studyRequest.handlers.change(); assert.equal(s.hidden, true); assert.equal(p.live.size, 0);
  assert.ok([...urls].every(url => !p.live.has(url)));
});

test('treatment-aware designs retain exact intermediate patterns and refused-stage evidence in both workflows', async function treatmentDesignStages() {
  const p=await pageModule(); await p.load(); scenarioInputs(p);
  for(const op of ['study','scenario']) {
    const report=designReport(), treated=treatmentReport(true), compensated=structuredClone(treated);
    compensated.mode='compensate';
    report.alternatives[1].design.treatments=[{operation_index:1,report:treated}];
    report.alternatives[2].design.treatments=[{operation_index:0,report:compensated}];
    await p.e[op].onclick();
    p.emit(op==='study'?report:{...scenarioReport(),manufacturing:report});
    const s=section(p,op==='study'?'Fixed-pattern manufacturing alternatives':'Fresh drape and heat alternatives');
    assert.match(visibleText(s),/operation 2 — Material treatment/);
    assert.match(visibleText(s),/operation 1 — Cutting compensation/);
    assert.match(visibleText(s),/Review final geometry, fit and material binding admission/);
    assert.match(visibleText(s),/Completed stages can remain available for inspection/);
    assert.match(visibleText(s),/Alteration refused: missing piece <front>/);
    assert.match(visibleText(s),/0.9517/);
    assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
    for(const [index,step,r] of [[2,2,treated],[3,1,compensated]]) {
      for(const [suffix,field] of [['before-pattern','source_pattern_json'],['after-pattern','pattern_json'],['request','frozen_request_json']]) {
        const matches=descendants(s).filter(n=>n.download===`alternative-${index}-treatment-${step}-${suffix}.json`);
        assert.equal(matches.length,1); assert.equal(await p.blobs.get(matches[0].href).text(),r[field]);
      }
    }
    const names=descendants(s).filter(n=>n.download).map(n=>n.download);
    assert.equal(names.some(name=>name.startsWith('alternative-1-treatment-')),false);
    assert.equal(new Set(names).size,names.length);
  }
  p.e.studyRequest.handlers.change(); assert.equal(p.live.size,0);
});

test('screened and adaptive treatment trials expose exact stage snapshots without marking them selected', async function treatmentSearchStages() {
  const p=await pageModule(); await p.load(); scenarioInputs(p);
  for(const adaptive of [false,true]) {
    const report=designReport(), design=structuredClone(report.alternatives[1].design);
    design.key='screened treatment'; design.treatments=[{operation_index:2,report:treatmentReport(true)}];
    design.fit=outcome({passed:true,body_identity:{provenance:'supplied body'},validation:{points:[]}});
    const rows=[{design,selected:false,max_fit_band_violation_mm:0,total_fit_band_violation_mm:0}];
    report.design_search=adaptive?{schema:'ftx-design-adaptive-search/1',evaluations:1,qualified_designs:1,
      selected_designs:0,stop_reason:'evaluation_limit',rows}:
      {schema:'ftx-design-grid-search/1',total_grid_points:2,fit_screening:{screened_designs:1,qualified_designs:1,
        rejected_designs:0,selected_designs:0,qualified_designs_omitted:1,unvisited_designs:1,
        complete_fit_screening:false,all_qualified_designs_selected:false,rows}};
    await p.e.study.onclick(); p.emit(report);
    const s=section(p,'Fixed-pattern manufacturing alternatives');
    assert.match(visibleText(s),/Not selected/); assert.match(visibleText(s),/operation 3 — Material treatment/);
    const prefix=adaptive?'adaptive':'screened';
    for(const [suffix,field] of [['before-pattern','source_pattern_json'],['after-pattern','pattern_json'],['request','frozen_request_json']]) {
      const link=descendants(s).find(n=>n.download===`${prefix}-design-1-treatment-3-${suffix}.json`);
      assert.ok(link); assert.equal(await p.blobs.get(link.href).text(),design.treatments[0].report[field]);
    }
    p.e.studyRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
  }
});

test('design scenario matrices display shared geometry once and keep every case separate', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  const report = scenarioMatrixReport(), designs = designReport();
  report.schema = 'ftx-design-scenario-matrix-study/1';
  report.manufacturing = designs; report.comparison = null;
  for (const item of report.cases) {
    item.comparison = null;
    item.simulations = designs.alternatives.map(row => ({ key: row.key, evaluation: refused(`Case ${item.key}: geometry gate`) }));
  }
  await p.e.scenario.onclick(); p.emit(report);
  const s = section(p, 'Fresh drape and heat alternatives');
  assert.equal(descendants(s).filter(n => n.tag === 'table' && n.children[0].textContent === 'Pattern designs and material choices').length, 1);
  assert.equal(descendants(s).filter(n => n.download?.endsWith('-design-pattern.json')).length, 2);
  assert.match(visibleText(s), /Case <cold>: geometry gate/); assert.match(visibleText(s), /Case warm: geometry gate/);
  assert.equal(JSON.parse(s.children[1].textContent).schema, 'ftx-design-scenario-matrix-study/1');
  const downloads = descendants(s).filter(n => n.download).map(n => n.download);
  assert.equal(new Set(downloads).size, downloads.length);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden, true); assert.equal(p.live.size, 0);
});

test('mixed cutting jobs retain complete inventory, independent pricing and exact replay in both study workflows', async () => {
  const p=await pageModule(); await p.load(); scenarioInputs(p);
  const a=studyRow('<mixed job>'), b=studyRow('unpriced'), c=studyRow('missing inventory');
  const job={complete_inventory:true,inventory:[{piece:'shell',stock:'finite_stock'},{piece:'lining',stock:'roll'}],
    resolved_routes:[{piece:'shell',stock:'finite_stock',material_sha256:'exact-hide-source'},
      {piece:'lining',stock:'roll',material_sha256:'exact-roll-source'}],
    roll:{rolls:[{}]},finite_stock:{used_stock_count:1},
    stock_area_m2:0.1,external_cut_area_m2:0.03,stock_mass_kg:null,
    purchasing:{currency:'USD',complete_offer_coverage:true,least_observed_independent_order_total:12.5,
      roll:outcome({complete_offer_coverage:true}),finite_stock:outcome({complete_offer_coverage:true})},
    pattern_json:'{"source":"<locked pattern>","x":-0.0,"value":1.0}',
    replay_request_json:'{"schema":"ftx-cutting-job/1","source":"<locked inventory>","x":-0.0}',
    hardware_execution_authorized:false};
  a.cutting_job=outcome(job);
  b.cutting_job=outcome({...job,purchasing:{currency:'USD',complete_offer_coverage:false,
    least_observed_independent_order_total:null,roll:outcome({complete_offer_coverage:false}),
    finite_stock:refused('No matching dated offer')}});
  c.cutting_job=refused('No finite stock matches the substituted shell');
  for (const row of [a,b,c]) { row.cutting=omitted(); row.finite_stock=omitted(); row.purchasing=omitted(); }
  for (const op of ['study','scenario']) {
    await p.e[op].onclick(); const report=studyReport([a,b,c]);
    p.emit(op==='study'?report:{...scenarioReport(),manufacturing:report});
    const s=section(p,op==='study'?'Fixed-pattern manufacturing alternatives':'Fresh drape and heat alternatives');
    const t=descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent==='Complete cutting jobs — rolls, hides and remnants');
    assert.deepEqual(plain(t.children[2].children.map(n=>n.textContent)),
      ['<mixed job>','Available','Yes','2','1','1','0.1','0.03','Unknown','Complete offer coverage','12.5 USD']);
    assert.deepEqual(plain(t.children[3].children.map(n=>n.textContent)),
      ['unpriced','Available','Yes','2','1','1','0.1','0.03','Unknown','Incomplete offer coverage','Unknown']);
    assert.deepEqual(plain(t.children[4].children.map(n=>n.textContent)),
      ['missing inventory','Refused: No finite stock matches the substituted shell','Unknown','Unknown','Unknown','Unknown','Unknown','Unknown','Unknown','Unknown','Unknown']);
    const routes=descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent==='<mixed job>: cutting routes for the selected materials');
    assert.deepEqual(plain(routes.children[2].children.map(n=>n.textContent)),
      ['shell','Finite hide, sheet or remnant','exact-hide-source']);
    assert.deepEqual(plain(routes.children[3].children.map(n=>n.textContent)),
      ['lining','Roll','exact-roll-source']);
    for (const [suffix,expected] of [['pattern',job.pattern_json],['request',job.replay_request_json]]) {
      const download=descendants(s).find(n=>n.download===`alternative-1-cut-job-${suffix}.json`);
      assert.equal(await p.blobs.get(download.href).text(),expected);
    }
    assert.equal(descendants(s).some(n=>n.download==='alternative-3-cut-job-request.json'),false);
    const evidence=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent.startsWith('unpriced: routed physical inventory'));
    evidence.open=true; evidence.handlers.toggle(); assert.match(visibleText(evidence),/No matching dated offer/);
    assert.match(visibleText(s),/same original inventory/);
    assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  }
  p.e.studyRequest.handlers.change(); assert.equal(p.live.size,0);
  assert.equal(section(p,'Fixed-pattern manufacturing alternatives').hidden,true);
  assert.equal(section(p,'Fresh drape and heat alternatives').hidden,true);
  await p.e.study.onclick(); p.emit(studyReport());
  assert.doesNotMatch(visibleText(section(p,'Fixed-pattern manufacturing alternatives')),/Complete cutting jobs/);
});

test('material studies display independent vapor outcomes and exact source-frozen replay in both workflows', async () => {
  const p=await pageModule(); await p.load(); scenarioInputs(p);
  const a=studyRow('<vapor-source>'), b=studyRow('missing'), c=studyRow('omitted');
  a.vapor=outcome(vaporReport()); b.vapor=refused('Missing source-qualified vapor resistance'); c.vapor=omitted();
  for (const op of ['study','scenario']) {
    await p.e[op].onclick();
    const report=studyReport([a,b,c]);
    p.emit(op==='study'?report:{...scenarioReport(),manufacturing:report});
    const s=section(p,op==='study'?'Fixed-pattern manufacturing alternatives':'Fresh drape and heat alternatives');
    const t=descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent==='Material-following potential vapor transfer');
    assert.deepEqual(plain(t.children[2].children.map(n=>n.textContent)),
      ['<vapor-source>','Available','1','40','50','0.00002','Reported protocols match']);
    assert.deepEqual(plain(t.children[3].children.map(n=>n.textContent)),
      ['missing','Refused: Missing source-qualified vapor resistance','Unknown','Unknown','Unknown','Unknown','Unknown']);
    assert.equal(t.children[4].children[1].textContent,'Not requested');
    assert.match(visibleText(s),/not actual sweat evaporation/);
    assert.match(visibleText(s),/Missing vapor observations do not erase independent/);
    const evidence=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent.startsWith('<vapor-source>: vapor layers'));
    evidence.open=true; evidence.handlers.toggle();
    assert.match(visibleText(evidence),/"air_temperature_c": "35"/);
    assert.match(visibleText(evidence),/Original <measurement>/);
    assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
    const download=descendants(s).find(n=>n.download==='alternative-1-vapor-replay.json');
    assert.equal(await p.blobs.get(download.href).text(),a.vapor.result.replay_request_json);
    assert.equal(descendants(s).some(n=>n.download==='alternative-2-vapor-replay.json'),false);
  }
  p.e.studyRequest.handlers.change(); assert.equal(p.live.size,0);
  assert.equal(section(p,'Fixed-pattern manufacturing alternatives').hidden,true);
  assert.equal(section(p,'Fresh drape and heat alternatives').hidden,true);
});

test('material study sewing choices retain bounded search status separately from unavailable plans', async () => {
  const p = await pageModule(); await p.load(); p.select('studyRequest', '{}');
  const bounded = studyRow('bounded'), missing = studyRow('missing');
  bounded.sewing_selection = outcome({selected_choices:{hem:'qualified knit recipe'},trials:[{},{}],
    search_complete:false,budget_exhausted:true,optimality_proven:false,unresolved_trials:1});
  missing.sewing_selection = outcome({selected_choices:null,trials:[{}],
    search_complete:true,budget_exhausted:false,optimality_proven:false,unresolved_trials:0});
  missing.sewing = refused('No source-qualified listed sewing recipe');
  missing.metrics.stitch_intervals = null; missing.metrics.feed_seconds = null;
  await p.e.study.onclick(); p.emit(studyReport([bounded,missing]));
  const s = section(p, 'Fixed-pattern manufacturing alternatives');
  const choices = descendants(s).find(n => n.tag === 'table' && n.children[0].textContent === 'Material-specific sewing recipe search');
  assert.deepEqual(plain(choices.children[2].children.map(n => n.textContent)),
    ['bounded','Available','hem: qualified knit recipe','2','false','true','false','1']);
  assert.deepEqual(plain(choices.children[3].children.map(n => n.textContent)),
    ['missing','Available','No listed recipe selected','1','true','false','false','0']);
  const consumers = descendants(s).find(n => n.tag === 'table');
  assert.match(visibleText(consumers.children[3]), /Refused: No source-qualified listed sewing recipe/);
  assert.deepEqual(plain(consumers.children[3].children.slice(9,11).map(n => n.textContent)), ['Unknown','Unknown']);
  assert.match(visibleText(choices), /Listed optimum proved/);
  const receipt = descendants(s).find(n => n.tag === 'details' && n.children[0].textContent.startsWith('bounded: sewing recipe choices'));
  receipt.open = true; receipt.handlers.toggle(); assert.match(visibleText(receipt), /"unresolved_trials": 1/);
  p.e.studyRequest.handlers.change(); assert.equal(s.hidden, true); assert.equal(p.live.size, 0);
});

test('thread-cost recipe selection displays only the canonical complete incumbent and its proof scope', async () => {
  const p = await pageModule(); await p.load(); p.select('studyRequest', '{}');
  const selected = studyRow('least cost'), bounded = studyRow('bounded cost'), missing = studyRow('unpriced');
  for (const row of [selected,bounded,missing]) {
    row.sewing_selection = outcome({request:{objective:'minimize_thread_observed_order_cost'},
      selected_choices:{hem:'qualified recipe'},objective_currency:'EUR',selected_thread_order_cost:14,
      trials:[{thread_order_cost:14},{thread_order_cost:28}],search_complete:true,
      budget_exhausted:false,optimality_proven:true,unresolved_trials:0});
  }
  Object.assign(bounded.sewing_selection.result,{search_complete:false,budget_exhausted:true,optimality_proven:false});
  Object.assign(missing.sewing_selection.result,{selected_choices:null,selected_thread_order_cost:null,
    trials:[{feed_seconds:0.01,objective_unavailable:{code:'invalid_document',message:'Missing <source> price'}}],
    optimality_proven:false,unresolved_trials:1});
  missing.sewing = refused('No complete cost-ranked recipe available');
  await p.e.study.onclick(); p.emit(studyReport([selected,bounded,missing]));
  const s = section(p, 'Fixed-pattern manufacturing alternatives');
  const costs = descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent==='Thread-cost-directed recipe search');
  assert.deepEqual(plain(costs.children.slice(2).map(row=>row.children.map(n=>n.textContent))),[
    ['least cost','14','EUR','true','0'],['bounded cost','14','EUR','false','0'],['unpriced','Unknown','EUR','false','1']]);
  assert.match(visibleText(s),/Trials with incomplete consumption or prices remain unresolved/);
  const receipt = descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent.startsWith('unpriced: sewing recipe choices'));
  receipt.open=true; receipt.handlers.toggle(); assert.match(visibleText(receipt),/Missing <source> price/);
  p.e.studyRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
});

test('compiled WASM thread-cost recipe and study results equal both native commands with incomplete orders retained',
  {skip:process.env.FTX_REAL_WASM!=='1' || !process.env.FTX_THREAD_COST_REPLAY}, async()=>{
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const dir=process.env.FTX_THREAD_COST_REPLAY, input=name=>fs.readFileSync(path.join(dir,name),'utf8');
    assert.deepEqual(JSON.parse(ftx.sew_select(input('pattern.json'),input('selection.json'))),JSON.parse(input('selection-report.json')));
    const w=await workerModule({bindings:ftx});
    for(const name of ['priced','unpriced']) {
      const result=await w.send({type:'study',request:190,pattern:input('pattern.json'),studyRequest:input(`${name}-study.json`)});
      assert.equal(result.type,'result',result.message);
      assert.deepEqual(result.result,JSON.parse(input(`${name}-report.json`)));
      const search=result.result.alternatives[0].sewing_selection.result;
      assert.equal(search.objective_currency,'USD'); assert.equal(search.no_feasible_listed_recipe,false);
      assert.equal(search.optimality_proven,name==='priced');
      assert.equal(result.files.find(file=>file.name==='study-request.json').text,input(`${name}-study.json`));
    }
  });

test('finite sewing fixtures retain capacities, operation lifecycles and source locks across all result views', async function finiteFixtureViews() {
  const plan={schema:'ftx-sewing-plan/13',seams:[],hardware_execution_authorized:false,
    options:{production:{workholding:{source:{note:'<observed fixture inventory>'},machine_sha256:'full-machine-lock'}}},
    production:{production_copies:3,feed_seconds:24,workholding:{schema:'ftx-sewing-workholding/1',
      fixture_geometry_and_access_validated:false,resources:[{id:'<jig>',capacity:2}],
      baseline:{peak_occupancy:{'<jig>':1},operations:[]},
      selected:{peak_occupancy:{'<jig>':2},operations:[
        {operation:'1/join',occupied_before:{'<jig>':1},acquired:['1/held'],occupied_during:{'<jig>':2},
          released:[],occupied_after:{'<jig>':2}},
        {operation:'0/reinforce',occupied_before:{'<jig>':2},acquired:[],occupied_during:{'<jig>':2},
          released:['0/held'],occupied_after:{'<jig>':1}}]}}}};
  const p=await pageModule(); await p.load(); await p.bind(); scenarioInputs(p); p.select('sewOptions');
  for(const op of ['sew','study','scenario']) {
    const row=studyRow('limited fixtures'); row.sewing=outcome(plan);
    const study=studyReport([row]);
    const report=op==='sew'?{schema:'ftx-sewing-program/4',stitches:[],plan}:
      op==='study'?study:{...scenarioReport(),manufacturing:study};
    const raw=JSON.stringify(report), file=`${op}-exact-workholding.json`;
    await p.e[op].onclick(); p.emit(report,[{name:file,text:raw,mime:'application/json'}]);
    const s=section(p,op==='sew'?'sew':op==='study'?'Fixed-pattern manufacturing alternatives':'Fresh drape and heat alternatives');
    const table=ending=>descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent.endsWith(ending));
    assert.deepEqual(plain(table('finite fixture inventory').children[2].children.map(n=>n.textContent)),['<jig>','2','1','2']);
    assert.deepEqual(plain(table('selected fixture occupancy').children[2].children.map(n=>n.textContent)),
      ['1/join','<jig>: 1','1/held','<jig>: 2','None','<jig>: 2']);
    assert.deepEqual(plain(table('selected fixture occupancy').children[3].children.map(n=>n.textContent)),
      ['0/reinforce','<jig>: 2','None','<jig>: 2','0/held','<jig>: 1']);
    assert.match(visibleText(s),/acquires its units before the named operation starts/);
    assert.match(visibleText(s),/Fixture geometry, seam access, handling duration and hardware execution remain unvalidated/);
    const detail=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent.endsWith('both complete timelines'));
    detail.open=true; detail.handlers.toggle(); assert.match(visibleText(detail),/<observed fixture inventory>/);
    assert.match(visibleText(detail),/full-machine-lock/);
    assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
    const link=descendants(s).find(n=>n.download===file); assert.equal(await p.blobs.get(link.href).text(),raw);
    if(op==='sew') assert.equal(JSON.parse(s.children[1].textContent).finite_workholding_enforced,true);
  }
  p.e.sewOptions.handlers.change(); p.e.studyRequest.handlers.change();
  assert.equal(section(p,'sew').hidden,true); assert.equal(section(p,'Fixed-pattern manufacturing alternatives').hidden,true);
  assert.equal(section(p,'Fresh drape and heat alternatives').hidden,true);
});

test('planar sewing access retains complete-path clearances, witnesses and source locks in every result view', async () => {
  const access={schema:'ftx-sewing-access/1',model:'planar_disk_sweep/1',frame:'operation_local_mm',
    machine_sha256:'complete-machine-lock',qualification:{id:'qualified-process',sha256:'complete-process-lock'},
    tool_radius_mm:2,required_clearance_mm:1,minimum_clearance_mm:3,minimum_centerline_distance_mm:5,
    checked_segments:4,fixture_count:1,physical_access_validated:false,
    witness:{fixture:'<clamp>',needle_segment:[0,1],fixture_edge:[3,0],needle_mm:[20,0],fixture_mm:[25,0]}};
  const policy={model:'planar_disk_sweep/1',machine_sha256:'complete-machine-lock',
    qualifications:[access.qualification],source:{note:'<declared tool and clamp geometry>'}};
  const plan={schema:'ftx-sewing-plan/14',options:{},hardware_execution_authorized:false,seams:[
    {pass:{id:'joined panel',seam:'side',access:policy},access,
      assembly:{frame:'operation_local_mm',needle_mm:[[20,0],[20,2.5],[20,5],[20,7.5],[20,10]]}},
    {pass:{id:'legacy operation',seam:'hem'}}]};
  const p=await pageModule(); await p.load(); await p.bind(); scenarioInputs(p); p.select('sewOptions');
  const boundUrls=[...p.live];
  for(const op of ['sew','study','scenario']) {
    const row=studyRow('qualified access'); row.sewing=outcome(plan);
    const study=studyReport([row]);
    const report=op==='sew'?{schema:'ftx-sewing-program/2',stitches:[],plan}:
      op==='study'?study:{...scenarioReport(),manufacturing:study};
    const raw=` ${JSON.stringify(report)}\n`, file=`${op}-access.json`;
    await p.e[op].onclick(); p.emit(report,[{name:file,text:raw,mime:'application/json'}]);
    const s=section(p,op==='sew'?'sew':op==='study'?'Fixed-pattern manufacturing alternatives':'Fresh drape and heat alternatives');
    const table=ending=>descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent.endsWith(ending));
    const clearances=table('planar tool and fixture clearance');
    assert.equal(clearances.children.length,3);
    assert.deepEqual(plain(clearances.children[2].children.map(n=>n.textContent)),
      ['joined panel','Declared planar model checked','2','1','3','1','4']);
    assert.deepEqual(plain(table('limiting planar clearance witnesses').children[2].children.map(n=>n.textContent)),
      ['joined panel','<clamp>','[0, 1]','[3, 0]','[20, 0]','[25, 0]']);
    assert.match(visibleText(s),/every complete needle-path segment/);
    assert.match(visibleText(s),/do not establish fixture grip, three-dimensional approach clearance/);
    const detail=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent.endsWith('tool geometry, assembly and qualification sources'));
    detail.open=true; detail.handlers.toggle();
    assert.match(visibleText(detail),/<declared tool and clamp geometry>/);
    assert.match(visibleText(detail),/complete-machine-lock/); assert.match(visibleText(detail),/complete-process-lock/);
    assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
    const link=descendants(s).find(n=>n.download===file); assert.equal(await p.blobs.get(link.href).text(),raw);
    if(op==='sew') assert.equal(JSON.parse(s.children[1].textContent).planar_access_checked_operations,1);
  }
  p.e.sewOptions.handlers.change(); p.e.studyRequest.handlers.change();
  assert.equal(section(p,'sew').hidden,true); assert.equal(section(p,'Fixed-pattern manufacturing alternatives').hidden,true);
  assert.equal(section(p,'Fresh drape and heat alternatives').hidden,true);
  assert.deepEqual([...p.live],boundUrls);
});

test('missing planar receipts remain unknown and ordinary plans have no access claim', async () => {
  const p=await pageModule(); await p.load(); await p.bind(); p.select('sewOptions');
  const plan={schema:'ftx-sewing-plan/14',options:{},seams:[{pass:{seam:'side',access:{model:'planar_disk_sweep/1'}}}]};
  await p.e.sew.onclick(); p.emit({stitches:[],plan});
  const s=section(p,'sew'), text=visibleText(s);
  assert.match(text,/No retained clearance result/); assert.match(text,/remain unchecked/);
  assert.equal(JSON.parse(s.children[1].textContent).planar_access_checked_operations,undefined);
  const result=descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent.endsWith('planar tool and fixture clearance'));
  assert.deepEqual(plain(result.children[2].children.slice(2).map(n=>n.textContent)),Array(5).fill('Unknown'));
  await p.e.sew.onclick(); p.emit({stitches:[],plan:{options:{},seams:[{pass:{seam:'side'}}]}});
  assert.equal(descendants(s).some(n=>n.tag==='table'&&n.children[0].textContent.endsWith('planar tool and fixture clearance')),false);
});

test('study timing separates sourced setup duration from feed time and preserves missing evidence', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  for (const op of ['study', 'scenario']) {
    const timed = studyRow('timed'), unknown = studyRow('unknown'), failed = studyRow('failed');
    timed.metrics = {...timed.metrics, feed_seconds:2, sewing_setup_seconds:10, sewing_feed_and_setup_seconds:12};
    timed.sewing = outcome({ordering:{timing:{selected_setup_milliseconds:10000, feed_and_setup_seconds:12,
      evidence:{source_id:'<measured setup>',conditions:'qualified machine and operator'}}}});
    unknown.metrics = {...unknown.metrics, feed_seconds:5, sewing_setup_seconds:null, sewing_feed_and_setup_seconds:null};
    failed.sewing = refused('changed material has no qualified recipe');
    failed.metrics = {...failed.metrics, feed_seconds:null, sewing_setup_seconds:null, sewing_feed_and_setup_seconds:null};
    const report = studyReport([timed, unknown, failed]);
    await p.e[op].onclick(); p.emit(op === 'study' ? report : {...scenarioReport(), manufacturing:report});
    const s = section(p, op === 'study' ? 'Fixed-pattern manufacturing alternatives' : 'Fresh drape and heat alternatives');
    const timing = descendants(s).find(n => n.tag === 'table' && n.children[0].textContent === 'Source-qualified sewing durations');
    assert.deepEqual(plain(timing.children[2].children.map(n => n.textContent)), ['timed','Available','2','10','12']);
    assert.deepEqual(plain(timing.children[3].children.map(n => n.textContent)), ['unknown','Available','5','Unknown','Unknown']);
    assert.deepEqual(plain(timing.children[4].children.slice(2).map(n => n.textContent)), ['Unknown','Unknown','Unknown']);
    assert.match(visibleText(timing), /changed material has no qualified recipe/);
    assert.match(visibleText(s), /exclude undeclared handling/);
    const evidence = descendants(s).find(n => n.tag === 'details' && n.children[0].textContent.startsWith('timed: setup timing evidence'));
    evidence.open = true; evidence.handlers.toggle(); assert.match(visibleText(evidence), /<measured setup>/);
    assert.equal(descendants(s).some(n => 'innerHTML' in n), false);
  }
  p.e.studyRequest.handlers.change(); assert.equal(p.live.size, 0);
  assert.equal(section(p, 'Fixed-pattern manufacturing alternatives').hidden, true);
  assert.equal(section(p, 'Fresh drape and heat alternatives').hidden, true);
});

function selectedThreadPlan() {
  const channels=['needle','bobbin'].map((channel,i)=>({operation_id:'hem <selected>',qualification_id:'qualified-recipe',
    channel_id:channel,material_sha256:`exact-${channel}`,stitch_intervals:10,metres_per_traversal:i?0.1:0.2,
    traversal_length_m:i?1:2,start_allowance_m:0.1,end_allowance_m:0.1,required_length_m:i?1.2:2.2}));
  const requirements=channels.map(c=>({material_sha256:c.material_sha256,required_length_m:c.required_length_m,
    required_continuous_length_m:c.required_length_m}));
  const orders=requirements.map((requirement,i)=>({requirement,excluded_unlocked_offer_ids:['unlocked-offer'],
    excluded_shared_stock_offer_ids:[],sample:{scope:{currency:'EUR'},offers:[]},
    least_observed_order:{quote:{offer_id:`observed-${i}`,currency:'EUR',unit:'linear_meter',
      material_sha256:requirement.material_sha256,source:{reference:'fixture:thread-order'}},
      order_quantity:5,order_cost:i?2:4,quantity_sufficiency_observed:true,within_observed_continuous_limit:true}}));
  return {seams:[{pass:{id:'hem <selected>'},qualification_id:'qualified-recipe'}],options:{consumables:{purchasing:{
      market:{currency:'EUR',as_of:'2026-10-23'},quote_book:{source:'<original thread offers>'}}},
    qualifications:[{id:'qualified-recipe',thread_consumption:{channels_complete:true,channels,
      method:'<independent consumption observation>',source:{reference:'fixture:thread-acquisition'}}}]},
    consumables:{schema:'ftx-sewing-consumables/1',complete_consumption_coverage:true,missing_operations:[],
      operation_channels:channels,materials:Object.fromEntries(channels.map(c=>[c.material_sha256,
        {id:c.channel_id,expected_sha256:c.material_sha256,record:{form:'yarn'}}])),
      material_requirements:requirements,total_required_length_m:3.4,purchasing:{currency:'EUR',
        complete_offer_coverage:true,complete_observed_quantity_coverage:true,
        complete_continuous_length_limit_coverage:true,material_orders:orders,
        least_observed_independent_order_total:6,unavailable_reason:null}}};
}

test('consumed thread mass preserves missing density while length inventory stays available', async()=>{
  const p=await pageModule(); await p.load(); await p.bind(); p.select('sewOptions');
  const plan=selectedThreadPlan(), r=plan.consumables;
  r.complete_mass_coverage=true; r.total_required_mass_kg=0.0000816;
  for(const yarn of r.material_requirements) yarn.mass={linear_density_kg_m:0.000024,
    required_mass_kg:yarn.material_sha256==='exact-needle'?0.0000528:0.0000288};
  await p.e.sew.onclick(); p.emit({stitches:[],plan});
  const s=section(p,'sew');
  const table=ending=>descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent.endsWith(ending));
  assert.deepEqual(plain(table('consumed thread mass').children[2].children.map(n=>n.textContent)),
    ['Complete qualified yarn density','0.0000816']);
  assert.match(visibleText(s),/Consumed thread includes the declared allowances and is not retained assembly mass/);
  r.complete_mass_coverage=false; r.total_required_mass_kg=null;
  r.material_requirements[1].mass=null; r.material_requirements[1].mass_unavailable_reason='Exact yarn density not qualified';
  await p.e.sew.onclick(); p.emit({stitches:[],plan});
  assert.deepEqual(plain(table('consumed thread mass').children[2].children.map(n=>n.textContent)),
    ['Incomplete or unavailable density','Unknown']);
  assert.match(visibleText(table('exact yarn length and mass requirements')),/Exact yarn density not qualified/);
  assert.match(visibleText(s),/Complete channel coverage/); assert.match(visibleText(s),/3.4/);
  p.e.sewOptions.handlers.change(); assert.equal(s.hidden,true);
});

test('production thread inventory displays batch and per-garment quantities without repricing', async()=>{
  const p=await pageModule(); await p.load(); scenarioInputs(p);
  const row=studyRow('three garments'), plan=selectedThreadPlan(), report=plan.consumables;
  report.schema='ftx-sewing-consumables/2'; report.production_copies=3;
  report.total_required_length_m=10.2;
  report.purchasing.least_observed_independent_order_total=9;
  for (const channel of report.operation_channels) {
    channel.per_copy=Object.fromEntries(['stitch_intervals','traversal_length_m','start_allowance_m','end_allowance_m','required_length_m']
      .map(name=>[name,channel[name]]));
    channel.stitch_intervals=30;
    channel.required_length_m=channel.channel_id==='needle'?6.6:3.6;
  }
  row.sewing=outcome(plan); row.metrics.thread_required_length_m=10.2; row.metrics.thread_observed_order_cost=9;
  await p.e.study.onclick(); p.emit(studyReport([row]));
  const s=section(p,'Fixed-pattern manufacturing alternatives');
  const table=caption=>descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent===caption);
  assert.deepEqual(plain(table('Thread for the selected sewing plan — complete or unknown').children[2].children.map(n=>n.textContent)),
    ['three garments','3','Complete channel coverage','10.2','Complete offer coverage','EUR','9 EUR']);
  assert.deepEqual(plain(table('three garments: thread consumption per garment before production aggregation').children[2].children.map(n=>n.textContent)),
    ['hem <selected>','needle','10','2','0.1','0.1','2.2']);
  assert.equal(table('three garments: selected operation and thread channel requirements').children[2].children[5].textContent,'6.6');
  assert.match(visibleText(s),/Explicit production schedules report their complete batch timing/);
  assert.match(visibleText(s),/without multiplying its splice-free length/);
  p.e.studyRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
});

test('selected thread study quantities and orders retain independent currencies, unknowns and source invalidation', async()=>{
  const p=await pageModule(); await p.load(); scenarioInputs(p);
  const selected=studyRow('selected'), unpriced=studyRow('unpriced'), missing=studyRow('missing offer'),
    partial=studyRow('partial channels'), failed=studyRow('failed'), omittedRow=studyRow('not requested');
  for(const row of [selected,unpriced,missing,partial]) {
    row.sewing=outcome(selectedThreadPlan()); row.metrics.thread_required_length_m=3.4;
    row.metrics.thread_observed_order_cost=6;
  }
  selected.sewing_selection=outcome({selected_choices:{'hem <selected>':'qualified-recipe'},
    trials:[{choice:'unused recipe',thread_required_length_m:0.1}],search_complete:true,
    budget_exhausted:false,optimality_proven:true,unresolved_trials:0});
  selected.purchasing=outcome({currency:'USD',complete_offer_coverage:true});
  selected.metrics.least_observed_independent_order_total=14;
  delete unpriced.sewing.result.consumables.purchasing; delete unpriced.metrics.thread_observed_order_cost;
  Object.assign(missing.sewing.result.consumables.purchasing,{complete_offer_coverage:false,
    least_observed_independent_order_total:null,unavailable_reason:'No eligible exact-yarn offer'});
  delete missing.metrics.thread_observed_order_cost;
  Object.assign(partial.sewing.result.consumables,{complete_consumption_coverage:false,total_required_length_m:null,
    material_requirements:[],operation_channels:partial.sewing.result.consumables.operation_channels.slice(0,1),
    missing_operations:[{operation_id:'hem <selected>',reason:'Bobbin consumption is not qualified'}]});
  Object.assign(partial.sewing.result.consumables.purchasing,{complete_offer_coverage:false,
    least_observed_independent_order_total:null,material_orders:[],unavailable_reason:'Partial requirements are not priced'});
  delete partial.metrics.thread_required_length_m; delete partial.metrics.thread_observed_order_cost;
  failed.sewing=refused('No qualified selected sewing recipe');
  const rows=[selected,unpriced,missing,partial,failed,omittedRow];
  const policy={basis:'nominal',allow_mixed_protocols:false,required_consumers:['sewing_consumables','sewing_purchasing'],
    objectives:[{metric:'thread_required_length_m',direction:'minimize'},{metric:'thread_observed_order_cost',direction:'minimize'},
      {metric:'observed_order_cost',direction:'minimize'}],constraints:[]};
  const computed={policy,frontier:['selected'],incomparable:[],alternatives:rows.map(row=>({key:row.key,
    eligible:row===selected,dominated_by:[],exclusions:row===selected?[]:[{message:'Complete thread offers required'}],
    readings:row===selected?{thread_required_length_m:{value:3.4,unit:'m'},thread_observed_order_cost:{value:6,unit:'EUR'},
      observed_order_cost:{value:14,unit:'USD'}}:{}}))};
  for(const op of ['study','scenario']) {
    const report={...studyReport(rows),comparison:computed};
    await p.e[op].onclick(); p.emit(op==='study'?report:{...scenarioReport(),manufacturing:report,comparison:computed});
    const s=section(p,op==='study'?'Fixed-pattern manufacturing alternatives':'Fresh drape and heat alternatives');
    const table=caption=>descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent===caption);
    const totals=table('Thread for the selected sewing plan — complete or unknown');
    assert.deepEqual(plain(totals.children.slice(2).map(row=>row.children.map(n=>n.textContent))),[
      ['selected','1','Complete channel coverage','3.4','Complete offer coverage','EUR','6 EUR'],
      ['unpriced','1','Complete channel coverage','3.4','Not requested','Unknown','Unknown'],
      ['missing offer','1','Complete channel coverage','3.4','Incomplete offer coverage','EUR','Unknown'],
      ['partial channels','1','Incomplete channel coverage','Unknown','Incomplete offer coverage','EUR','Unknown'],
      ['failed','Unknown','Refused: No qualified selected sewing recipe','Unknown','Refused: No qualified selected sewing recipe','Unknown','Unknown'],
      ['not requested','Unknown','Not requested','Unknown','Not requested','Unknown','Unknown']]);
    assert.equal(table('Independent manufacturing outcomes').children[2].children[8].textContent,'14 USD');
    assert.deepEqual(plain(table('Declared computed objectives — authoritative Rust comparison').children[2].children.slice(3).map(n=>n.textContent)),
      ['3.4 m','6 EUR','14 USD']);
    assert.match(visibleText(s),/Required thread length \(minimize\)/);
    assert.deepEqual(plain(table('selected: selected operation and thread channel requirements').children.slice(2).map(row=>row.children.map(n=>n.textContent))),[
      ['hem <selected>','qualified-recipe','needle','exact-needle','10','2.2'],
      ['hem <selected>','qualified-recipe','bobbin','exact-bobbin','10','1.2']]);
    assert.match(visibleText(s),/Bobbin consumption is not qualified/); assert.match(visibleText(s),/No eligible exact-yarn offer/);
    assert.match(visibleText(s),/same production copies as cutting/); assert.doesNotMatch(visibleText(s),/unused recipe/);
    const receipt=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent==='selected: selected thread channels, exact yarns and order evidence (display only)');
    receipt.open=true; receipt.handlers.toggle(); assert.match(visibleText(receipt),/<independent consumption observation>/);
    assert.match(visibleText(receipt),/fixture:thread-order/); assert.match(visibleText(receipt),/unlocked-offer/);
    assert.match(visibleText(receipt),/"order_quantity": 5/); assert.doesNotMatch(visibleText(receipt),/unused recipe/);
    assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  }
  p.e.sewOptions.handlers.change();
  assert.equal(section(p,'Fixed-pattern manufacturing alternatives').hidden,false);
  p.e.studyRequest.handlers.change(); assert.equal(p.live.size,0);
  assert.equal(section(p,'Fixed-pattern manufacturing alternatives').hidden,true);
  assert.equal(section(p,'Fresh drape and heat alternatives').hidden,true);
  await p.e.study.onclick(); const stale=p.current(), request=stale.messages.at(-1).request;
  await p.load(); p.ready(); stale.emit({type:'result',request,result:studyReport(rows),files:[]});
  assert.equal(section(p,'Fixed-pattern manufacturing alternatives').hidden,true);
});

test('required thread consumers reveal unknown quantities without hiding successful sewing and standalone source changes withdraw receipts', async()=>{
  const p=await pageModule(); await p.load(); p.select('studyRequest','{}');
  const report=studyReport(); report.comparison={policy:{basis:'nominal',allow_mixed_protocols:false,
    required_consumers:['sewing_consumables','sewing_purchasing'],objectives:[{metric:'feed_seconds',direction:'minimize'}],constraints:[]},
    frontier:[],incomparable:[],alternatives:[{key:'A',eligible:false,dominated_by:[],
      exclusions:[{message:'Thread consumption was not requested'}],readings:{feed_seconds:{value:0,unit:'s'}}}]};
  await p.e.study.onclick(); p.emit(report);
  let s=section(p,'Fixed-pattern manufacturing alternatives');
  const totals=descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent==='Thread for the selected sewing plan — complete or unknown');
  assert.deepEqual(plain(totals.children[2].children.map(n=>n.textContent)),['A','Unknown','Not requested','Unknown','Not requested','Unknown','Unknown']);
  assert.match(visibleText(s),/Thread consumption was not requested/);
  const manufacturing=descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent==='Independent manufacturing outcomes');
  assert.equal(manufacturing.children[2].children[3].textContent,'Available');
  await p.bind(); p.select('sewOptions','{"consumables":{}}'); await p.e.sew.onclick();
  p.emit({schema:'ftx-sewing-program/1',plan:selectedThreadPlan(),stitches:[]});
  s=section(p,'sew'); assert.match(visibleText(s),/6 EUR/);
  assert.match(visibleText(s),/Selected sewing plan: selected operation and thread channel requirements/);
  p.e.sewOptions.handlers.change(); assert.equal(s.hidden,true);
});

test('scenario comparisons join keys, preserve signed values, and distinguish gating and independent refusals', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  assert.equal(p.e.scenario.disabled, false); assert.equal(p.e.fold.disabled, true); // no bind or single-preview fit
  await p.e.scenario.onclick(); const request = p.current().messages.at(-1);
  assert.equal(request.scenarioRequest, '{"temperature":293.0}'); assert.equal(request.measurements, '{"length":10.0}');
  assert.equal(request.bytes.byteLength, 8); assert.equal('fit' in request, false);
  p.emit(scenarioReport()); const s = section(p, 'Fresh drape and heat alternatives');
  const t = descendants(s).filter(n => n.tag === 'table')[1];
  assert.deepEqual(plain(t.children[2].children.map(n => n.textContent)),
    ['A', 'Available', 'NO — review flags', '0', 'Available', '-4.5', 'Refused: missing conductivity', 'Unknown']);
  assert.match(t.children[3].children[1].textContent, /missing mechanics/);
  assert.equal(t.children[3].children[3].textContent, 'Unknown');
  const d = descendants(s).find(n => n.tag === 'details' && n.children[0].textContent.startsWith('A: simulation'));
  d.open = true; d.handlers.toggle(); assert.match(visibleText(d), /unsettled/); assert.match(visibleText(d), /body-lock/);
});

test('draped vapor coverage keeps uncovered area, source replay and independent failures visible', async () => {
  const p=await pageModule(); await p.load(); scenarioInputs(p); await p.e.scenario.onclick();
  const report=scenarioReport(); report.scenario.scenario={vapor_coverage:{body_triangle_ids:[2]}};
  const a=report.simulations.find(row=>row.key==='A').evaluation.result;
  const replay='{"schema":"ftx-vapor-coverage/1","source":"<retained geometry>","value":1.0,"x":-0.0}';
  a.vapor_coverage=outcome({replay_request_json:replay,source_sha256:'source-lock',
    coverage:{body_area_m2:2,covered_area_m2:1.5,uncovered_area_m2:0.5,unassigned_cloth_reference_area_m2:0.25,
      covered_potential_heat_flow_w:3,covered_potential_mass_flow_kg_s:0.0000012,
      regions:[{id:'body-2-region-0',body_triangle:2,vapor:vaporReport()}]}});
  report.manufacturing.alternatives.push(studyRow('C'));
  report.simulations.push({key:'C',evaluation:outcome({vapor_coverage:refused('Missing Ret for outer layer'),
    surface:outcome({}),coverage:outcome({}),drape:{settled:true,flags:[]}})});
  p.emit(report);
  const s=section(p,'Fresh drape and heat alternatives');
  const t=descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent==='Draped layered vapor coverage — covered-region potentials');
  assert.deepEqual(plain(t.children[2].children.map(n=>n.textContent)),
    ['A','Available','2','1.5','0.5','0.25','3','0.0000012']);
  assert.equal(t.children[3].children[1].textContent,'Refused: missing mechanics');
  assert.equal(t.children[4].children[1].textContent,'Refused: Missing Ret for outer layer');
  assert.equal(t.children[4].children.slice(2).every(n=>n.textContent==='Unknown'),true);
  assert.match(visibleText(s),/missing conductivity/);
  assert.match(visibleText(s),/covered subtotal is not whole-body transfer/);
  const details=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent.startsWith('A: vapor coverage regions'));
  details.open=true; details.handlers.toggle(); assert.match(visibleText(details),/body-2-region-0/);
  assert.match(visibleText(details),/Original <measurement>/);
  const download=descendants(s).find(n=>n.download==='alternative-1-vapor-coverage-replay.json');
  assert.equal(await p.blobs.get(download.href).text(),replay);
  assert.equal(descendants(s).some(n=>n.download==='alternative-3-vapor-coverage-replay.json'),false);
  assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
  await p.e.scenario.onclick(); p.emit(scenarioReport());
  assert.doesNotMatch(visibleText(section(p,'Fresh drape and heat alternatives')),/Draped layered vapor coverage/);
});

test('scenario attachment reports preserve physical mass scopes, source targets and independent refusals', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  await p.e.scenario.onclick();
  const report = scenarioReport();
  const result = report.simulations.find(row => row.key === 'A').evaluation.result;
  result.drape.attached_mass = { schema:'ftx-attached-mass-report/1', input_sha256:'attachment-source-lock',
    cloth_mass_kg:0.2, attached_mass_kg:0.03, total_inertial_mass_kg:0.23, mass_balance_residual_kg:0,
    input:{groups:[{extra:{id:'synthetic-closure',sources:{test:{reference:'test:<literal-source>'}}}}]},
    groups:[{id:'synthetic-closure',mass_kg:0.03,targets:[{instance:'front.R',vertex:'hem',solver_vertex:4,fraction:1}]}] };
  result.metrics.cloth_mass_kg = 0.2;
  p.emit(report);
  const s = section(p,'Fresh drape and heat alternatives');
  const t = descendants(s).find(n => n.tag === 'table' && n.children[0]?.textContent === 'Attached accessory mass — gravity and inertia');
  assert.deepEqual(plain(t.children[2].children.map(n => n.textContent)),['A','Available','0.2','0.03','0.23','0']);
  assert.deepEqual(plain(t.children[3].children.map(n => n.textContent)),['B','Refused: missing mechanics','Unknown','Unknown','Unknown','Unknown']);
  const d = descendants(s).find(n => n.tag === 'details' && n.children[0].textContent.startsWith('A: attached mass sources'));
  d.open = true; d.handlers.toggle();
  assert.match(visibleText(d),/attachment-source-lock/); assert.match(visibleText(d),/test:<literal-source>/);
  assert.match(visibleText(d),/front.R/); assert.match(visibleText(s),/Cloth thermal storage/);
  p.e.studyRequest.handlers.change(); assert.equal(p.live.size,0); assert.equal(s.hidden,true);
  await p.e.scenario.onclick(); p.emit(scenarioReport());
  assert.doesNotMatch(visibleText(section(p,'Fresh drape and heat alternatives')),/Attached accessory mass/);
});

test('finite-stock studies expose used inventory and independent refusal without inventing mass', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  await p.e.study.onclick();
  const a = studyRow('hide-A'), b = studyRow('hide-B');
  a.finite_stock = outcome({ markers: [{ id: 'physical-hide', defects_mm: [], placements: [{ instance: 'front' }] }] });
  a.metrics.used_stock_count = 1; a.metrics.used_stock_gross_area_m2 = 0.18;
  a.metrics.finite_stock_external_cut_area_m2 = 0.12; a.metrics.used_stock_mass_kg = null;
  b.finite_stock = refused('inventory material does not match the selected snapshot');
  p.emit(studyReport([a, b]));
  const s = section(p, 'Fixed-pattern manufacturing alternatives');
  const stock = descendants(s).find(n => n.tag === 'table' && visibleText(n).includes('Finite hides and remnants'));
  assert.deepEqual(plain(stock.children[2].children.map(n => n.textContent)),
    ['hide-A', 'Available', '1', '0.18', '0.12', 'Unknown']);
  assert.match(stock.children[3].children[1].textContent, /inventory material does not match/);
  assert.equal(stock.children[3].children.slice(2).every(n => n.textContent === 'Unknown'), true);
  assert.match(visibleText(s), /missing stock width/); // roll failure did not erase the valid hide plan
  const d = descendants(s).find(n => n.tag === 'details' && n.children[0].textContent.includes('physical stock'));
  d.open = true; d.handlers.toggle(); assert.match(visibleText(d), /physical-hide/);
  p.e.stockRequest.handlers.change(); assert.equal(s.hidden, false); // shared study inventory is authoritative
  p.e.studyRequest.handlers.change(); assert.equal(s.hidden, true); assert.equal(p.live.size, 0);
});

test('motion displays complete continuation separately from initial static heat and preserves endpoint refusals', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  const raw = '{"motion":{"time_s":1.0,"coordinate":-0.0}}';
  p.select('scenarioRequest', raw); await p.e.scenario.onclick();
  assert.equal(p.current().messages.at(-1).scenarioRequest, raw);
  const report = scenarioReport(); report.scenario.scenario = { motion: {} };
  const initial = report.simulations.find(r => r.key === 'A').evaluation.result;
  initial.motion = outcome({ duration_s: 1, samples: [{}, {}], trials: 3, initial_drape_gating: [],
    path_sha256: 'exact-motion-source', metrics: { max_absolute_edge_strain_pct: 7.25 },
    keyframes: [{ time_s: 1, arm_abduction_deg: 40, surface: outcome({ steady: { power_w: -2.5 } }),
      coverage: refused('<missing endpoint coverage>') }], thermal_basis: 'steady snapshots only' });
  p.emit(report);
  const s = section(p, 'Fresh drape and heat alternatives');
  const table = label => descendants(s).find(n => n.tag === 'table' && n.children[0].textContent.startsWith(label));
  assert.equal(table('Initial static-drape').children[2].children[5].textContent, '-4.5');
  assert.deepEqual(plain(table('Prescribed motion').children[2].children.map(n => n.textContent)),
    ['A', 'Available', 'NO — review initial state', '1', '2', '3', '7.25']);
  assert.match(table('Prescribed motion').children[3].children[1].textContent, /missing mechanics/);
  assert.equal(table('Prescribed motion').children[3].children[3].textContent, 'Unknown');
  assert.deepEqual(plain(table('A: endpoint').children[2].children.map(n => n.textContent)),
    ['1', '40', 'Available', '-2.5', 'Refused: <missing endpoint coverage>', 'Unknown']);
  const d = descendants(s).find(n => n.tag === 'details' && n.children[0].textContent.startsWith('A: motion identities'));
  d.open = true; d.handlers.toggle(); assert.match(visibleText(d), /exact-motion-source/);
  assert.match(visibleText(s), /not transient heat/); assert.equal(descendants(s).some(n => 'innerHTML' in n), false);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden, true); assert.equal(p.live.size, 0);
  // A later refusal cannot retain a successful earlier trajectory or its endpoint heat table.
  await p.e.scenario.onclick(); initial.motion = refused('trajectory budget exhausted'); p.emit(report);
  assert.match(visibleText(table('Prescribed motion')), /trajectory budget exhausted/);
  assert.equal(table('A: endpoint'), undefined);
  assert.equal(table('Initial static-drape').children[2].children[5].textContent, '-4.5');
});

test('moving layered vapor preserves endpoint source replay and refusals without accumulating potential transfer', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p); await p.e.scenario.onclick();
  const report = scenarioReport(); report.scenario.scenario = { motion: {}, vapor_coverage: {} };
  const initial = report.simulations.find(r => r.key === 'A').evaluation.result;
  const replay = ' {"schema":"ftx-vapor-coverage/1","value":1.0,"coordinate":-0.0,"source":"<literal>"}\n';
  initial.motion = outcome({ duration_s: 2, samples: [{}, {}], trials: 2, metrics: {},
    keyframes: [{ time_s: 1, surface: omitted(), coverage: omitted(), vapor_surface: omitted(),
      vapor_coverage: outcome({ replay_request_json: replay, coverage: { covered_area_m2: 0.4, uncovered_area_m2: 0.1,
        covered_potential_heat_flow_w: -2.5, covered_potential_mass_flow_kg_s: -0.000001,
        regions: [{ source: '<literal material source>' }] } }) },
      { time_s: 2, surface: omitted(), coverage: omitted(), vapor_surface: omitted(),
        vapor_coverage: refused('boundary geometry lock does not match this endpoint') }] });
  p.emit(report);
  const s = section(p, 'Fresh drape and heat alternatives');
  const table = () => descendants(s).find(n => n.tag === 'table' && n.children[0].textContent === 'A: endpoint vapor potentials — covered layers');
  assert.deepEqual(plain(table().children[2].children.map(n => n.textContent)),
    ['1', 'Available', '0.4', '0.1', '-2.5', '-0.000001']);
  assert.deepEqual(plain(table().children[3].children.map(n => n.textContent)),
    ['2', 'Refused: boundary geometry lock does not match this endpoint', 'Unknown', 'Unknown', 'Unknown', 'Unknown']);
  assert.match(visibleText(s), /endpoint rates are separate from the optional history integral/);
  const download = descendants(s).find(n => n.download === 'alternative-1-motion-endpoint-1-vapor-coverage-replay.json');
  assert.equal(await p.blobs.get(download.href).text(), replay);
  assert.equal(descendants(s).some(n => n.download === 'alternative-1-motion-endpoint-2-vapor-coverage-replay.json'), false);
  const d = descendants(s).find(n => n.tag === 'details' && n.children[0].textContent.startsWith('A: endpoint 1 vapor layers'));
  d.open = true; d.handlers.toggle(); assert.match(visibleText(d), /<literal material source>/);
  assert.equal(descendants(s).some(n => 'innerHTML' in n), false);
  p.e.scenarioRequest.handlers.change(); assert.equal(p.live.has(download.href), false); assert.equal(s.hidden, true);
  await p.e.scenario.onclick(); initial.motion = refused('complete motion failed'); p.emit(report);
  assert.equal(table(), undefined);
  assert.equal(descendants(s).some(n => n.download?.includes('motion-endpoint')), false);
});

test('motion inspector selects exact adaptive states at one physical scale, including accepted-only reports', async () => {
  for (const includeInitial of [true, false]) {
    const p = await pageModule(); await p.load(); scenarioInputs(p); await p.e.scenario.onclick();
    const report = scenarioReport(); report.scenario.scenario = { motion: {} };
    const sample = (time_s, x, y) => ({ time_s,
      body_positions_m: [[x,y,0],[x+1,y,0],[x,y,1]], positions_m: [[x,y+1,0],[x+1,y+1,0],[x,y+1,1]] });
    const motion = { duration_s: 0.00375, trials: 2, keyframes: [], triangles: [[0,1,2]], body_triangles: [[0,1,2]],
      samples: [sample(0.001,1,0), sample(0.00375,3,1)] };
    if (includeInitial) motion.initial_sample = sample(0,0,0);
    report.simulations.find(row => row.key === 'A').evaluation.result.motion = outcome(motion); p.emit(report);
    const s = section(p, 'Fresh drape and heat alternatives');
    const inspector = descendants(s).find(n => n.tag === 'details' && n.children[0].textContent.includes('inspect retained motion samples'));
    assert.equal(descendants(inspector).some(n => n.tag === 'canvas'), false);
    inspector.open = true; inspector.handlers.toggle();
    const slider = descendants(inspector).find(n => n.tag === 'input');
    const canvases = descendants(inspector).filter(n => n.tag === 'canvas');
    const stamp = descendants(inspector).find(n => n.tag === 'output');
    assert.equal(canvases.length, 2); assert.equal(slider.step, '1');
    assert.equal(slider.max, includeInitial ? '2' : '1');
    assert.equal(stamp.textContent, includeInitial ? 'Initial state · t = 0 s' : 'Accepted step 1 of 2 · t = 0.001 s');
    const first = plain(canvases.map(c => c.paint[0]));
    assert.deepEqual(first, includeInitial ? [['move',24,256.5],['move',117,256.5]] : [['move',24,272],['move',86,272]]);
    slider.value = slider.max; slider.handlers.input();
    assert.equal(stamp.textContent, 'Accepted step 2 of 2 · t = 0.00375 s');
    const last = plain(canvases.map(c => c.paint[0]));
    assert.deepEqual(last, includeInitial ? [['move',303,256.5],['move',210,256.5]] : [['move',272,272],['move',210,272]]);
    assert.equal(canvases[0].paint[1][1] - canvases[0].paint[0][1], includeInitial ? 93 : 124);
    inspector.handlers.toggle(); assert.equal(descendants(inspector).filter(n => n.tag === 'canvas').length, 2);
    p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden, true);
    assert.equal(descendants(s).filter(n => n.tag === 'canvas').length, 0);
  }
});

test('invalid retained display geometry or timestamps cannot erase independent scenario outcomes', async () => {
  for (const badTime of [false, true]) {
    const p = await pageModule(); await p.load(); scenarioInputs(p); await p.e.scenario.onclick();
    const report = scenarioReport(); report.scenario.scenario = { motion: {} };
    const sample = { time_s: 0.001, positions_m: [[0,0,0],[1,0,0],[0,0,1]], body_positions_m: [[0,1,0],[1,1,0],[0,1,1]] };
    report.simulations.find(row => row.key === 'A').evaluation.result.motion = outcome({ keyframes: [], samples: [sample, sample],
      triangles: [[0,1,badTime ? 2 : 3]], body_triangles: [[0,1,2]] }); p.emit(report);
    const s = section(p, 'Fresh drape and heat alternatives');
    const inspector = descendants(s).find(n => n.tag === 'details' && n.children[0].textContent.includes('inspect retained motion samples'));
    inspector.open = true; inspector.handlers.toggle();
    assert.match(visibleText(inspector), badTime ? /physical sample times are invalid/ : /geometry is incomplete/);
    assert.equal(descendants(inspector).some(n => n.tag === 'canvas'), false);
    const initial = descendants(s).find(n => n.tag === 'table' && n.children[0].textContent.startsWith('Initial static-drape'));
    assert.equal(initial.children[2].children[5].textContent, '-4.5');
    assert.match(visibleText(s), /missing mechanics/);
  }
});

test('study output invalidation follows the actual request/body dependencies, not unrelated preview controls', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  await p.e.study.onclick(); p.emit(studyReport());
  await p.e.scenario.onclick(); p.emit(scenarioReport());
  assert.equal(p.live.size, 3);
  for (const id of ['cutOptions', 'heatConditions', 'fit', 'selfContact', 'quotes']) p.e[id].handlers.change();
  assert.equal(p.live.size, 3);
  p.e.measurements.handlers.change(); assert.equal(p.live.size, 1);
  assert.equal(section(p, 'Fresh drape and heat alternatives').hidden, true);
  assert.equal(section(p, 'Fixed-pattern manufacturing alternatives').hidden, false);
  await p.e.scenario.onclick(); p.emit(scenarioReport());
  p.e.scenarioRequest.handlers.change(); assert.equal(p.live.size, 1);
  await p.e.scenario.onclick(); p.emit(scenarioReport());
  p.e.studyRequest.handlers.change(); assert.equal(p.live.size, 0);
  await p.e.study.onclick(); p.emit(studyReport()); await p.e.scenario.onclick(); p.emit(scenarioReport());
  await p.load(); assert.equal(p.live.size, 0); // no earlier-pattern results remain
});

test('study replacement/cancellation cannot publish late file reads or stale scenario success', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  await p.e.scenario.onclick(); p.emit(scenarioReport()); assert.equal(p.live.size, 2);
  const d = deferred(); p.e.scenarioRequest.files[0].text = () => d.promise;
  const old = p.current(), running = p.e.scenario.onclick(); assert.equal(p.live.size, 0);
  p.e.cancel.onclick(); p.ready(); d.resolve('{}'); await running;
  assert.equal(old.messages.filter(m => m.type === 'scenario').length, 1);
  scenarioInputs(p); await p.e.scenario.onclick(); const stale = p.current(), id = stale.messages.at(-1).request;
  p.e.scenarioRequest.handlers.change(); p.ready();
  stale.emit({ type: 'result', request: id, result: scenarioReport(), files: [] });
  assert.equal(p.live.size, 0);
  await p.e.scenario.onclick(); p.emit(scenarioReport());
  p.select('scenarioRequest', '{malformed'); await p.e.scenario.onclick();
  assert.equal(p.live.size, 0); assert.equal(section(p, 'Fresh drape and heat alternatives').hidden, true);
  assert.equal(p.e.error.hidden, false);
});

test('computed objectives display Rust admission, signed units and incomparability without a browser scorer', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  const comparison = {
    policy: { basis: 'nominal', allow_mixed_protocols: false,
      objectives: [{ metric: 'independent_surface_power_w', direction: 'minimize' }], constraints: [] },
    frontier: ['A'], motion_required: false, uncertainty_propagated: false,
    alternatives: [
      { key: 'A', eligible: true, dominated_by: [], exclusions: [], readings: { independent_surface_power_w: { value: -8, unit: 'W' } } },
      { key: 'B', eligible: false, dominated_by: [], exclusions: [{ message: 'not settled' }], readings: {} }],
    incomparable: [{ alternatives: ['A', '<C>'], reason: 'Different conditioning' }]
  };
  await p.e.scenario.onclick();
  const report = scenarioReport(); report.comparison = comparison;
  p.emit(report);
  const s = section(p, 'Fresh drape and heat alternatives');
  assert.match(visibleText(s), /Computed nominal frontier: A/);
  assert.match(visibleText(s), /Reported input protocols must match/);
  assert.match(visibleText(s), /does not require motion completion/);
  assert.match(visibleText(s), /A \/ <C>: Different conditioning/);
  const t = descendants(s).find(n => n.tag === 'table');
  assert.deepEqual(plain(t.children[2].children.map(n => n.textContent)), ['A', 'Eligible', 'None reported', '-8 W']);
  assert.deepEqual(plain(t.children[3].children.map(n => n.textContent)), ['B', 'Excluded: not settled', 'None reported', 'Unknown']);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden, true);
  await p.e.study.onclick();
  const study = studyReport(); study.comparison = { ...comparison, frontier: [], policy: { ...comparison.policy, allow_mixed_protocols: true } };
  p.emit(study);
  const m = section(p, 'Fixed-pattern manufacturing alternatives');
  assert.match(visibleText(m), /No eligible non-dominated alternatives/);
  assert.match(visibleText(m), /Mixed source protocols explicitly admitted/);

  // A refused motion cannot look qualified merely because static heat is available. Display the
  // Rust frontier and signed, domain-specific readings verbatim, including below-zero temperatures.
  comparison.motion_required = true;
  comparison.policy = { basis: 'nominal', allow_mixed_protocols: false, constraints: [],
    required_consumers: ['transient_coverage'], objectives: [
      { metric: 'transient_surface_net_boundary_energy_j', direction: 'minimize' },
      { metric: 'transient_surface_final_min_temperature_c', direction: 'maximize' }] };
  comparison.alternatives[0].readings = {
    transient_surface_net_boundary_energy_j: { value: -3, unit: 'J', consumer: 'transient_surface' },
    transient_surface_final_min_temperature_c: { value: -5, unit: 'C', consumer: 'transient_surface' } };
  comparison.alternatives[1].exclusions = [{ message: 'complete motion coverage history required' }];
  await p.e.scenario.onclick(); p.emit(report);
  assert.match(visibleText(s), /Motion-qualified comparison/);
  const motionTable = descendants(s).find(n => n.tag === 'table');
  assert.deepEqual(plain(motionTable.children[2].children.map(n => n.textContent)),
    ['A', 'Eligible', 'None reported', '-3 J', '-5 C']);
  assert.deepEqual(plain(motionTable.children[3].children.map(n => n.textContent)),
    ['B', 'Excluded: complete motion coverage history required', 'None reported', 'Unknown', 'Unknown']);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden, true);

  const filmMetrics=['evaporated_mass_kg','remaining_liquid_mass_kg','unmet_demand_mass_kg','supplied_mass_kg','body_reservoir_latent_energy_j'];
  comparison.policy={basis:'nominal',allow_mixed_protocols:false,constraints:[],required_consumers:['motion_wet_film'],
    objectives:filmMetrics.map(metric=>({metric:`motion_wet_film_${metric}`,direction:'minimize'}))};
  comparison.alternatives[0].readings=Object.fromEntries(filmMetrics.map((metric,i)=>[
    `motion_wet_film_${metric}`,{value:i+0.25,unit:metric.endsWith('_j')?'J':'kg',consumer:'motion_wet_film'}]));
  comparison.alternatives[1].exclusions=[{message:'Finite body water history unavailable'}];
  await p.e.scenario.onclick();p.emit(report);
  assert.match(visibleText(s),/Finite body water comparison requires complete covered motion/);
  assert.match(visibleText(s),/does not include textile-temperature feedback/);
  const filmTable=descendants(s).find(n=>n.tag==='table');
  assert.match(visibleText(filmTable),/Modeled covered evaporation/);
  assert.match(visibleText(filmTable),/Prescribed-body latent energy/);
  assert.deepEqual(plain(filmTable.children[2].children.map(n=>n.textContent)),
    ['A','Eligible','None reported','0.25 kg','1.25 kg','2.25 kg','3.25 kg','4.25 J']);
  assert.deepEqual(plain(filmTable.children[3].children.slice(3).map(n=>n.textContent)),Array(5).fill('Unknown'));
  const textileMetrics=['textile_water_evaporated_mass_kg','textile_water_remaining_mass_kg','textile_water_latent_energy_j','textile_water_supplied_mass_kg'];
  comparison.policy={basis:'nominal',allow_mixed_protocols:false,constraints:[],required_consumers:['transient_surface'],
    objectives:textileMetrics.map(metric=>({metric,direction:'minimize'}))};
  comparison.alternatives[0].readings=Object.fromEntries(textileMetrics.map((metric,i)=>[
    metric,{value:[0.003,0.002,7500,0.005][i],unit:i===2?'J':'kg',consumer:'transient_surface'}]));
  comparison.alternatives[1].exclusions=[{message:'Coupled textile water ledger is inconsistent'}];
  await p.e.scenario.onclick();p.emit(report);
  assert.match(visibleText(s),/Textile water objectives use the complete admitted coupled temperature history/);
  assert.match(visibleText(s),/body-reservoir water has separate objectives/);
  const textileTable=descendants(s).find(n=>n.tag==='table');
  assert.match(visibleText(textileTable),/Evaporated textile water/);
  assert.match(visibleText(textileTable),/Textile evaporation latent energy/);
  assert.match(visibleText(textileTable),/Supplied textile liquid/);
  assert.deepEqual(plain(textileTable.children[2].children.map(n=>n.textContent)),
    ['A','Eligible','None reported','0.003 kg','0.002 kg','7500 J','0.005 kg']);
  assert.deepEqual(plain(textileTable.children[3].children.slice(3).map(n=>n.textContent)),Array(4).fill('Unknown'));
  p.e.scenarioRequest.handlers.change();assert.equal(s.hidden,true);
});

test('reported mass comparisons display admitted ranges and unresolved alternatives without nominal reranking', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  const comparison = {
    policy: { basis: 'reported_bounds', allow_mixed_protocols: false,
      objectives: [{ metric: 'assembly_mass_kg', direction: 'minimize' }],
      constraints: [{ metric: 'assembly_mass_kg', maximum: 0.22 }] },
    frontier: ['A', 'B'], motion_required: false, uncertainty_propagated: false,
    alternatives: [
      { key: 'A', eligible: true, dominated_by: [], exclusions: [],
        readings: { assembly_mass_kg: { value: 0.2, unit: 'kg', reported_bounds: [0.19, 0.21] } } },
      { key: 'B', eligible: true, dominated_by: [], exclusions: [],
        readings: { assembly_mass_kg: { value: 0.21, unit: 'kg', reported_bounds: [0.2, 0.22] } } },
      { key: 'C', eligible: false, dominated_by: [], exclusions: [{ message: 'No complete reported bounds' }],
        readings: { assembly_mass_kg: { value: 0.18, unit: 'kg' } } }],
    incomparable: [{ alternatives: ['A', 'B'], reason: 'Reported mass ranges overlap; no strict dominance' }]
  };
  const manufacturing = studyReport(['A', 'B', 'C'].map(key => studyRow(key)));
  for (const op of ['study', 'scenario']) {
    await p.e[op].onclick();
    p.emit(op === 'study' ? { ...manufacturing, comparison } : { ...scenarioReport(), manufacturing, comparison });
    const s = section(p, op === 'study' ? 'Fixed-pattern manufacturing alternatives' : 'Fresh drape and heat alternatives');
    assert.match(visibleText(s), /Computed reported_bounds frontier: A, B/);
    assert.match(visibleText(s), /mass at fixed authored geometry and inventory/);
    assert.match(visibleText(s), /Complete source-reported ranges evaluated by Rust/);
    assert.doesNotMatch(visibleText(s), /Declared nominal objectives/);
    assert.match(visibleText(s), /A \/ B: Reported mass ranges overlap/);
    const t = descendants(s).find(n => n.tag === 'table');
    assert.deepEqual(plain(t.children[2].children.map(n => n.textContent)),
      ['A', 'Eligible', 'None reported', '0.2 kg; reported range [0.19, 0.21] kg']);
    assert.deepEqual(plain(t.children[3].children.map(n => n.textContent)),
      ['B', 'Eligible', 'None reported', '0.21 kg; reported range [0.2, 0.22] kg']);
    assert.deepEqual(plain(t.children[4].children.map(n => n.textContent)),
      ['C', 'Excluded: No complete reported bounds', 'None reported', '0.18 kg; reported range unknown']);
  }
});

test('assembly mass and finite stock retain independent totals and refusals in manufacturing and scenario studies', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  const a = studyRow('complete'), b = studyRow('unknown'), c = studyRow('refused');
  a.metrics.stock_mass_kg = 9;
  a.assembly_mass = outcome({ request: { inventory_complete: true, extras: [{ id: '<retained source>' }] },
    sheet_mass: { kg: 1 }, assembly_mass: { kg: 1.2, reported_bounds_kg: [1.1, 1.3] } });
  b.assembly_mass = outcome({ request: { inventory_complete: false }, sheet_mass: { kg: 1 },
    assembly_mass: { kg: null, reported_bounds_kg: null, unavailable: { message: 'extra inventory incomplete' } } });
  c.assembly_mass = refused('cutout geometry refused');
  a.finite_stock = outcome({ markers: [{ id: 'complete-hide' }] });
  Object.assign(a.metrics, { used_stock_count: 1, used_stock_gross_area_m2: 2,
    finite_stock_external_cut_area_m2: 0.5, used_stock_mass_kg: 7 });
  b.finite_stock = refused('physical inventory mismatch');
  c.finite_stock = outcome({ markers: [{ id: 'untrimmed-remnant' }] });
  Object.assign(c.metrics, { used_stock_count: 1, used_stock_gross_area_m2: 1,
    finite_stock_external_cut_area_m2: 0.25, used_stock_mass_kg: 4 });
  const report = studyReport([a, b, c]);
  for (const op of ['study', 'scenario']) {
    await p.e[op].onclick();
    p.emit(op === 'study' ? report : { ...scenarioReport(), manufacturing: report });
    const s = section(p, op === 'study' ? 'Fixed-pattern manufacturing alternatives' : 'Fresh drape and heat alternatives');
    const t = descendants(s).find(n => n.tag === 'table' && n.children[0].textContent === 'Declared assembly mass — complete or unknown');
    assert.deepEqual(plain(t.children[2].children.map(n => n.textContent)),
      ['complete', 'Complete declared inventory', '1', '1.2', '[1.1, 1.3]', 'Declared complete']);
    assert.deepEqual(plain(t.children[3].children.map(n => n.textContent)),
      ['unknown', 'Report available; total unknown', '1', 'Unknown', 'Not reported', 'Incomplete']);
    assert.equal(t.children[4].children[1].textContent, 'Refused: cutout geometry refused');
    const manufacturing = descendants(s).find(n => n.tag === 'table' && n.children[0].textContent === 'Independent manufacturing outcomes');
    assert.equal(manufacturing.children[2].children[7].textContent, '9');
    const stock = descendants(s).find(n => n.tag === 'table' && n.children[0].textContent === 'Finite hides and remnants — used physical inventory');
    assert.deepEqual(plain(stock.children[2].children.map(n => n.textContent)),
      ['complete', 'Available', '1', '2', '0.5', '7']);
    assert.equal(stock.children[3].children[1].textContent, 'Refused: physical inventory mismatch');
    assert.deepEqual(plain(stock.children[4].children.map(n => n.textContent)),
      ['refused', 'Available', '1', '1', '0.25', '4']);
    assert.match(visibleText(s), /not confidence intervals/);
    const d = descendants(s).find(n => n.tag === 'details' && n.children[0].textContent.startsWith('complete: cut-piece mass'));
    d.open = true; d.handlers.toggle(); assert.match(visibleText(d), /<retained source>/);
    assert.equal(descendants(s).some(n => 'innerHTML' in n), false);
  }
  assert.equal(p.live.size, 6);
  p.e.studyRequest.handlers.change(); assert.equal(p.live.size, 0);
  assert.equal(section(p, 'Fixed-pattern manufacturing alternatives').hidden, true);
  assert.equal(section(p, 'Fresh drape and heat alternatives').hidden, true);
});

// Opt in after tools/build_web.sh. This executes the real production WASM through the actual
// worker module, with only the Node transport/module loader adapted. It is not HTTP/browser or
// native parity, successful anatomical body fitting, or empirical textile validation.
test('compiled WASM measured treatment and batch sewing replay through the actual worker',
  {skip:process.env.FTX_REAL_WASM!=='1'},async()=>{
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const fixture=name=>fs.readFileSync(path.join(__dirname,'../tests/fixtures/materials',name),'utf8');
    const w=await workerModule({bindings:ftx}), treatmentRequest=fixture('kanat-ozdil-2026-denim-1-treatment.json');
    const transition=JSON.parse(ftx.material_treatment(treatmentRequest));
    assert.equal(transition.input.cycles,5); assert.ok(Math.abs(transition.stretch[0]-0.9517)<1e-14);
    assert.ok(Math.abs(transition.stretch[1]-0.945)<1e-14);
    const material=await w.send({type:'treatment',request:193,treatmentRequest});
    assert.equal(material.type,'result',material.message); assert.deepEqual(material.result,transition);
    assert.equal(material.files.find(f=>f.name==='treatment-request.json').text,treatmentRequest);
    for(const side of ['before','after']) {
      const selection=transition[`${side}_selection_json`];
      assert.equal(material.files.find(f=>f.name===`${side}-treatment-selection.json`).text,selection);
      const inspection=JSON.parse(ftx.material_inspect(selection));
      assert.equal(inspection.capabilities.mechanical_coefficients.ready,false);
      assert.ok(inspection.capabilities.mechanical_coefficients.refusal);
    }
    const panel=fixture('synthetic-sewing-pattern.json'), input=JSON.parse(treatmentRequest);
    const bound=JSON.parse(ftx.material_bind(panel,JSON.stringify({schema:'ftx-material-bindings/1',default:input.before}))).pattern_json;
    const request=JSON.stringify({schema:'ftx-pattern-treatment/1',mode:'treat',
      transitions:{launder:input},pieces:{front:'launder',back:'launder'}});
    const expected=ftx.material_treatment_pattern(bound,request);
    const treated=await w.send({type:'treatment',request:194,pattern:bound,treatmentRequest:request});
    assert.equal(treated.type,'result',treated.message); assert.deepEqual(treated.result,JSON.parse(expected));
    const r=treated.result;
    assert.equal(treated.files.find(f=>f.name==='treatment-report.json').text,expected);
    for(const [name,text] of [['treatment-source-pattern.json',r.source_pattern_json],
      ['treatment-request.json',r.frozen_request_json],['treat-pattern.json',r.pattern_json]])
      assert.equal(treated.files.find(f=>f.name===name).text,text);
    assert.equal(ftx.material_treatment_pattern(r.source_pattern_json,r.frozen_request_json),expected);
    const vertices=JSON.parse(r.pattern_json).pieces[0].vertices;
    assert.ok(Math.abs(vertices.c[0]-18.9)<1e-12); assert.ok(Math.abs(vertices.c[1]-95.17)<1e-12);
    const studyRequest=JSON.stringify({schema:'ftx-design-study/1',
      designs:[{key:'washed',operations:[{op:'treatment',request:JSON.parse(request)}]}],
      variants:[{key:'retained',changes:{schema:'ftx-material-bindings/1'}}],
      assembly_mass:{schema:'ftx-assembly-mass/1',inventory_complete:true,extras:[],seam_allowance_mm:0}});
    const study=await w.send({type:'study',request:196,pattern:bound,studyRequest});
    assert.equal(study.type,'result',study.message);
    assert.deepEqual(study.result,JSON.parse(ftx.material_study(bound,studyRequest)));
    assert.equal(study.files.find(f=>f.name==='study-request.json').text,studyRequest);
    const washed=study.result.alternatives.find(row=>row.design.key==='washed');
    assert.equal(washed.binding.status,'available'); assert.equal(washed.assembly_mass.status,'available');
    assert.equal(washed.design.treatments.length,1); assert.equal(washed.design.treatments[0].operation_index,0);
    const stage=washed.design.treatments[0].report;
    assert.deepEqual(JSON.parse(ftx.material_treatment_pattern(stage.source_pattern_json,
      stage.frozen_request_json)),stage);
    assert.ok(Math.abs(washed.metrics.assembly_mass_kg-0.004*0.9517*0.945*0.37010)<1e-12);
    assert.equal(JSON.parse(ftx.material_bind(washed.design.altered_pattern_json,
      JSON.stringify(washed.frozen_changes))).pattern_json,washed.binding.result.pattern_json);
    const options=fs.readFileSync(path.join(__dirname,'../examples/sewing-production-options.json'),'utf8');
    const sewing=await w.send({type:'sew',request:195,pattern:panel,options});
    assert.equal(sewing.type,'result',sewing.message);
    assert.deepEqual(sewing.result,JSON.parse(ftx.sew_stitches(panel,options)));
    const batch=sewing.result.plan.production;
    assert.equal(batch.production_copies,3); assert.equal(batch.stitch_intervals,240); assert.equal(batch.feed_seconds,24);
    assert.deepEqual(batch.ordering.selected_order,['0/join','1/join','2/join','0/reinforce','1/reinforce','2/reinforce']);
    assert.equal(batch.ordering.timing.selected_setup_milliseconds,1500); assert.equal(batch.ordering.optimality_proven,true);
    assert.equal(sewing.result.plan.hardware_execution_authorized,false);
    const heldOptions=JSON.parse(fs.readFileSync(path.join(__dirname,'../examples/sewing-workholding-options.json'),'utf8'));
    for(const [capacity,setup] of [[1,6500],[2,4000],[3,1500]]) {
      heldOptions.production.workholding.resources[0].capacity=capacity;
      const options=JSON.stringify(heldOptions);
      const held=await w.send({type:'sew',request:196+capacity,pattern:panel,options});
      assert.equal(held.type,'result',held.message); assert.deepEqual(held.result,JSON.parse(ftx.sew_stitches(panel,options)));
      assert.equal(held.result.plan.schema,'ftx-sewing-plan/13');
      const batch=held.result.plan.production, report=batch.workholding, id=report.resources[0].id;
      assert.equal(batch.feed_seconds,24); assert.equal(batch.stitch_intervals,240);
      assert.equal(batch.ordering.timing.selected_setup_milliseconds,setup);
      assert.equal(batch.ordering.optimality_proven,true); assert.equal(report.selected.peak_occupancy[id],capacity);
      assert.deepEqual(report.selected.operations.map(step=>step.operation),batch.ordering.selected_order);
      assert.equal(report.selected.operations.at(-1).occupied_after[id],0);
      assert.equal(report.fixture_geometry_and_access_validated,false);
    }
  });

test('compiled WASM executes handling-aware batch timing through the actual sewing and study workers',
  {skip:process.env.FTX_REAL_WASM!=='1'}, async()=>{
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const pattern=fs.readFileSync(path.join(__dirname,'../tests/fixtures/materials/synthetic-sewing-pattern.json'),'utf8');
    const options=fs.readFileSync(path.join(__dirname,'../examples/sewing-handling-options.json'),'utf8');
    const w=await workerModule({bindings:ftx});
    const result=await w.send({type:'sew',request:201,pattern,options});
    assert.equal(result.type,'result',result.message);
    assert.deepEqual(result.result,JSON.parse(ftx.sew_stitches(pattern,options)));
    const plan=result.result.plan, batch=plan.production;
    assert.equal(plan.schema,'ftx-sewing-plan/15'); assert.equal(result.result.stitches.length,240);
    assert.deepEqual(batch.ordering.selected_order,['0/join','0/reinforce','1/join','1/reinforce','2/join','2/reinforce']);
    assert.equal(batch.ordering.timing.selected_setup_milliseconds,6500);
    assert.equal(batch.ordering.handling_timing.selected_handling_milliseconds,10600);
    assert.ok(Math.abs(batch.ordering.handling_timing.selected_modeled_production_seconds-41.1)<1e-12);
    assert.equal(batch.workholding.selected.peak_occupancy['holding-jig'],1);
    assert.equal(batch.handling.selected.steps.length,7);
    const studyRequest=JSON.stringify({schema:'ftx-material-study/1',
      variants:[{key:'retained',changes:{schema:'ftx-material-bindings/1',default:JSON.parse(pattern).fabric.material}}],
      sewing:JSON.parse(options),comparison:{basis:'nominal',required_consumers:['sewing'],
        objectives:[{metric:'sewing_modeled_production_seconds',direction:'minimize'}]}});
    const study=await w.send({type:'study',request:202,pattern,studyRequest});
    assert.equal(study.type,'result',study.message);
    assert.deepEqual(study.result,JSON.parse(ftx.material_study(pattern,studyRequest)));
    assert.deepEqual(study.result.comparison.frontier,['retained']);
    assert.equal(study.result.alternatives[0].metrics.sewing_handling_seconds,10.6);
    const row=study.result.alternatives[0];
    assert.deepEqual(row.sewing.result.production,batch);
    assert.deepEqual(row.sewing.result,JSON.parse(ftx.sew_stitches(row.binding.result.pattern_json,
      JSON.stringify(row.sewing.result.options))).plan);
  });

test('compiled WASM parallel cells preserve native schedules, recipe choices and actual stitch assignments through workers',
  {skip:process.env.FTX_REAL_WASM!=='1'}, async()=>{
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const pattern=fs.readFileSync(path.join(__dirname,'../tests/fixtures/materials/synthetic-sewing-pattern.json'),'utf8');
    const options=fs.readFileSync(path.join(__dirname,'../examples/sewing-parallel-options.json'),'utf8');
    const w=await workerModule({bindings:ftx});
    const result=await w.send({type:'sew',request:211,pattern,options});
    assert.equal(result.type,'result',result.message);
    assert.deepEqual(result.result,JSON.parse(ftx.sew_stitches(pattern,options)));
    const plan=result.result.plan, parallel=plan.production.parallel;
    assert.equal(plan.schema,'ftx-sewing-plan/16'); assert.equal(result.result.schema,'ftx-sewing-program/5');
    assert.equal(parallel.baseline.makespan_milliseconds,41100);
    assert.equal(parallel.selected.makespan_milliseconds,25500); assert.equal(parallel.optimality_proven,true);
    assert.equal(result.result.stitches.length,240);
    const copies=new Set();
    for(const cell of parallel.selected.cells) {
      for(const copy of cell.copies) {assert.equal(copies.has(copy),false);copies.add(copy);}
      const stitches=result.result.stitches.filter(step=>step.cell===cell.cell);
      assert.equal(stitches.length,cell.copies.length*80);
      assert.ok(stitches.every(step=>cell.copies.includes(step.copy)));
    }
    assert.deepEqual([...copies].sort(),[0,1,2]);
    assert.deepEqual(JSON.parse(result.files.find(file=>file.name==='sewing-program.json').text),result.result);
    const selection={schema:'ftx-sewing-selection/5',options:JSON.parse(options),
      objective:'minimize_parallel_makespan',max_plan_evaluations:2,
      alternatives:[{operation:'join',recipes:[{id:'faster-qualified',needle:'example-needle',
        thread:'example-thread-a',max_stitch_mm:2.5,stitches_per_minute:1000}]}]};
    const chosen=JSON.parse(ftx.sew_select(pattern,JSON.stringify(selection)));
    assert.deepEqual(chosen.report.selected_choices,{join:'faster-qualified'});
    assert.equal(chosen.report.selected_objective_seconds,22.3); assert.equal(chosen.report.optimality_proven,true);
    assert.deepEqual(JSON.parse(ftx.sew_stitches(pattern,chosen.selected_options_json)).plan,chosen.report.selected_plan);
    const studyRequest=JSON.stringify({schema:'ftx-material-study/1',
      variants:[{key:'parallel',changes:{schema:'ftx-material-bindings/1',default:JSON.parse(pattern).fabric.material}}],
      sewing_selection:selection,comparison:{basis:'nominal',required_consumers:['sewing'],
        objectives:[{metric:'sewing_parallel_makespan_seconds',direction:'minimize'}]}});
    const study=await w.send({type:'study',request:212,pattern,studyRequest});
    assert.equal(study.type,'result',study.message);
    assert.deepEqual(study.result,JSON.parse(ftx.material_study(pattern,studyRequest)));
    assert.equal(study.result.alternatives[0].metrics.sewing_parallel_makespan_seconds,22.3);
    assert.equal(Object.hasOwn(study.result.alternatives[0].metrics,'sewing_modeled_production_seconds'),false);
    assert.deepEqual(study.result.comparison.frontier,['parallel']);
    assert.equal(study.files.find(file=>file.name==='study-request.json').text,studyRequest);
    if(process.env.FTX_PARALLEL_PRODUCTION_REPLAY) {
      const expected=name=>JSON.parse(fs.readFileSync(path.join(process.env.FTX_PARALLEL_PRODUCTION_REPLAY,name),'utf8'));
      assert.deepEqual(result.result,expected('program.json'));
      assert.deepEqual(chosen,expected('selection-report.json'));
      assert.deepEqual(study.result,expected('study-report.json'));
    }
    const denied=JSON.parse(options); denied.production.parallel.independence='one-shared-worker';
    const refused=await w.send({type:'sew',request:213,pattern,options:JSON.stringify(denied)});
    assert.equal(refused.type,'error');
    assert.equal(w.sent.some(message=>message.type==='result'&&message.request===213),false);
  });

test('compiled WASM heterogeneous cells replay native geometry, thread orders and material study decisions',
  {skip:process.env.FTX_REAL_WASM!=='1'}, async()=>{
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const pattern=fs.readFileSync(path.join(__dirname,'../tests/fixtures/materials/synthetic-sewing-pattern.json'),'utf8');
    const options=fs.readFileSync(path.join(__dirname,'../examples/sewing-heterogeneous-options.json'),'utf8');
    const w=await workerModule({bindings:ftx});
    const result=await w.send({type:'sew',request:221,pattern,options});
    assert.equal(result.type,'result',result.message);
    assert.deepEqual(result.result,JSON.parse(ftx.sew_stitches(pattern,options)));
    const plan=result.result.plan, parallel=plan.production.parallel;
    assert.equal(parallel.schema,'ftx-sewing-parallel/2'); assert.equal(parallel.selected.makespan_milliseconds,9900);
    assert.equal(result.result.stitches.length,160);
    assert.ok(Math.abs(plan.consumables.total_required_length_m-3)<1e-12);
    assert.equal(plan.consumables.purchasing.material_orders.length,1);
    assert.equal(plan.consumables.purchasing.least_observed_independent_order_total,14);
    assert.ok(plan.consumables.operation_channels.every(channel=>typeof channel.cell==='string'));
    for(const cell of parallel.selected.cells) {
      const source=parallel.cell_plans.find(source=>source.cell===cell.cell).plan;
      const stitches=result.result.stitches.filter(step=>step.cell===cell.cell);
      assert.ok(stitches.every(step=>cell.copies.includes(step.copy)));
      assert.equal(stitches.length,cell.copies.length*source.stitch_intervals);
    }
    let studyRequest=JSON.stringify({schema:'ftx-material-study/1',
      variants:[{key:'heterogeneous',changes:{schema:'ftx-material-bindings/1',default:JSON.parse(pattern).fabric.material}}],
      sewing:JSON.parse(options),comparison:{basis:'nominal',required_consumers:['sewing','sewing_consumables','sewing_purchasing'],
        objectives:[{metric:'sewing_parallel_makespan_seconds',direction:'minimize'},
          {metric:'thread_required_length_m',direction:'minimize'},{metric:'thread_required_mass_kg',direction:'minimize'},
          {metric:'thread_observed_order_cost',direction:'minimize'}]}});
    const native=process.env.FTX_HETEROGENEOUS_PRODUCTION_REPLAY;
    if(native) studyRequest=fs.readFileSync(path.join(native,'study.json'),'utf8');
    const study=await w.send({type:'study',request:222,pattern,studyRequest});
    assert.equal(study.type,'result',study.message);
    assert.deepEqual(study.result,JSON.parse(ftx.material_study(pattern,studyRequest)));
    assert.deepEqual(study.result.comparison.frontier,['heterogeneous']);
    assert.equal(study.result.alternatives[0].metrics.sewing_parallel_makespan_seconds,9.9);
    if(native) {
      assert.deepEqual(result.result,JSON.parse(fs.readFileSync(path.join(native,'program.json'),'utf8')));
      assert.deepEqual(study.result,JSON.parse(fs.readFileSync(path.join(native,'study-report.json'),'utf8')));
    }
    const changed=JSON.parse(options);
    changed.production.parallel.cells[0].configuration.machine.max_stitches_per_minute=1;
    const refused=await w.send({type:'sew',request:223,pattern,options:JSON.stringify(changed)});
    assert.equal(refused.type,'error');
    assert.equal(w.sent.some(message=>message.type==='result'&&message.request===223),false);
  });

test('compiled WASM purchases exact catalog yarn by mass and exposes complete consumed-mass objectives',
  {skip:process.env.FTX_REAL_WASM!=='1'}, async()=>{
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const pattern=fs.readFileSync(path.join(__dirname,'../tests/fixtures/materials/synthetic-sewing-pattern.json'),'utf8');
    const selection=JSON.parse(fs.readFileSync(path.join(__dirname,'../examples/sewing-thread-cost-selection.json'),'utf8'));
    const yarn=JSON.parse(ftx.material_inspect(JSON.stringify({id:'coats/epic-2994120-2026'}))).selection;
    for(const qualification of selection.options.qualifications)
      for(const channel of qualification.thread_consumption.channels) channel.material=yarn;
    const consumables=selection.options.consumables;
    consumables.production_copies=3;
    consumables.purchasing.market.require_observed_continuous_length=false;
    Object.assign(consumables.purchasing.quote_book.quotes[0],{material_id:yarn.id,
      material_sha256:yarn.expected_sha256,unit:'kilogram',price:100,quantity:1,
      minimum_quantity:0.0001,order_increment:0.0001,available_quantity:0.0003,
      maximum_continuous_quantity:null,price_breaks:[{minimum_quantity:0.0002,price:80}],
      source:'test:synthetic-kilogram-yarn-order'});
    const studyRequest=JSON.stringify({schema:'ftx-material-study/1',
      variants:[{key:'original',changes:{schema:'ftx-material-bindings/1',default:JSON.parse(pattern).fabric.material}}],
      sewing_selection:selection,comparison:{basis:'nominal',required_consumers:['sewing_purchasing'],
        objectives:[{metric:'thread_required_mass_kg',direction:'minimize'},
          {metric:'thread_observed_order_cost',direction:'minimize'}]}});
    const w=await workerModule({bindings:ftx});
    const result=await w.send({type:'study',request:203,pattern,studyRequest});
    assert.equal(result.type,'result',result.message);
    assert.deepEqual(result.result,JSON.parse(ftx.material_study(pattern,studyRequest)));
    const row=result.result.alternatives[0], inventory=row.sewing.result.consumables;
    assert.equal(inventory.complete_mass_coverage,true);
    assert.ok(Math.abs(row.metrics.thread_required_length_m-5.1)<1e-12);
    assert.ok(Math.abs(row.metrics.thread_required_mass_kg-0.0001224)<1e-15);
    assert.ok(Math.abs(row.metrics.thread_observed_order_cost-0.016)<1e-15);
    const order=inventory.purchasing.material_orders[0].least_observed_order;
    assert.equal(order.quote.unit,'kilogram'); assert.equal(order.order_quantity,0.0002);
    assert.ok(Math.abs(order.yarn_linear_density_kg_m-0.000024)<1e-18);
    assert.equal(Object.hasOwn(order,'within_observed_continuous_limit'),false);
    assert.deepEqual(result.result.comparison.frontier,['original']);
  });

test('compiled WASM checks the full tool sweep through the actual sewing worker',
  {skip:process.env.FTX_REAL_WASM!=='1'}, async()=>{
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const pattern=fs.readFileSync(path.join(__dirname,'../tests/fixtures/materials/synthetic-sewing-pattern.json'),'utf8');
    const options=fs.readFileSync(path.join(__dirname,'../examples/sewing-access-options.json'),'utf8');
    const w=await workerModule({bindings:ftx});
    const result=await w.send({type:'sew',request:198,pattern,options});
    assert.equal(result.type,'result',result.message);
    assert.deepEqual(result.result,JSON.parse(ftx.sew_stitches(pattern,options)));
    assert.equal(result.result.plan.schema,'ftx-sewing-plan/14');
    assert.equal(result.result.stitches.length,240);
    for(const seam of result.result.plan.seams) {
      assert.equal(seam.access.minimum_clearance_mm,3);
      assert.equal(seam.access.checked_segments,40);
      assert.equal(seam.access.physical_access_validated,false);
    }
    const collision=JSON.parse(options); collision.passes[0].access.tool_radius_mm=6;
    const refused=await w.send({type:'sew',request:199,pattern,options:JSON.stringify(collision)});
    assert.equal(refused.type,'error'); assert.match(refused.message,/planar access/);
    assert.equal(w.sent.filter(message=>message.type==='result'&&message.request===199).length,0);
  });

test('compiled WASM authored designs replay real stock and sewing while excluding invalid geometry',
  { skip: process.env.FTX_REAL_WASM !== '1' }, async () => {
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const pattern=fs.readFileSync(path.join(__dirname,'../tests/fixtures/materials/synthetic-sewing-pattern.json'),'utf8');
    const original=fs.readFileSync(path.join(__dirname,'../examples/design-study.json'),'utf8');
    for (const width of [0.100,0.090]) {
      const input=JSON.parse(original);
      input.variants[0].changes.default.record.properties.width.value=width;
      const request=JSON.stringify(input), raw=ftx.material_study(pattern,request), report=JSON.parse(raw);
      assert.equal(report.schema,'ftx-design-study-report/1');assert.equal(report.alternatives.length,8);
      assert.equal(report.physical_validation_claim,false);assert.equal(report.hardware_execution_authorized,false);
      assert.equal(ftx.material_study(pattern,request),raw);
      for (const row of report.alternatives) {
        const design=row.design;
        assert.equal(design.source_pattern_sha256,report.source_pattern_sha256);
        if (design.key==='unbalanced-seam') {
          assert.equal(design.validation.passed,false);assert.equal(row.binding.status,'refused');
          assert.equal(row.cutting.status,'refused');assert.equal(row.sewing.status,'refused');
          assert.equal(report.comparison.frontier.includes(row.key),false);continue;
        }
        assert.equal(design.validation.passed,true);
        const rebound=JSON.parse(ftx.material_bind(design.altered_pattern_json,JSON.stringify(row.frozen_changes)));
        const bound=row.binding.result.pattern_json;
        assert.equal(rebound.pattern_json,bound);
        assert.equal(row.binding.result.source_pattern_sha256,design.altered_pattern_sha256);
        assert.deepEqual(JSON.parse(ftx.cut_plan(bound,JSON.stringify(input.cutting))),row.cutting.result);
        assert.deepEqual(JSON.parse(ftx.sew_plan(bound,JSON.stringify(input.sewing))),row.sewing.result);
      }
      const row=key=>report.alternatives.find(r=>r.design.key===key&&r.design.material_key==='light-sheet');
      for (const [key,cutWidth,length] of [['baseline',0.040,0.120],['more-ease',0.048,width===0.100?0.120:0.242]]) {
        const metrics=row(key).metrics;
        assert.ok(Math.abs(metrics.external_cut_area_m2-2*cutWidth*0.120)<1e-14);
        assert.ok(Math.abs(metrics.stock_area_m2-width*length)<1e-14);
        assert.ok(Math.abs(metrics.stock_mass_kg-width*length*0.180)<1e-14);
      }
      assert.equal(row('more-ease').metrics.feed_seconds,row('baseline').metrics.feed_seconds);
      assert.ok(row('longer').metrics.feed_seconds>row('baseline').metrics.feed_seconds);
      const fixed={...input,schema:'ftx-material-study/1'};delete fixed.designs;
      for (const prior of JSON.parse(ftx.material_study(pattern,JSON.stringify(fixed))).alternatives) {
        const baseline=structuredClone(report.alternatives.find(r=>r.design.key==='baseline'&&r.design.material_key===prior.key));
        delete baseline.design;baseline.key=prior.key;assert.deepEqual(baseline,prior);
      }
      assert.throws(()=>ftx.material_study(pattern,JSON.stringify({...input,schema:'ftx-material-study/1'})));
      const over={...input,designs:Array.from({length:16},(_,i)=>({key:`extra-${i}`,operations:[
        {op:'move',piece:'front',vertex:'b',dx_mm:0,dy_mm:0}]}))};
      assert.throws(()=>ftx.material_study(pattern,JSON.stringify(over)),/32|budget|expanded/i);
    }
  });

test('compiled WASM explicit extensometers retain acquired widths, frozen forward replay and CHECK rejection',
  { skip: process.env.FTX_REAL_WASM !== '1' }, async () => {
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    // Retain the existing numerical acquisitions exactly; explicitly name their existing operator.
    // These are synthetic software observations, not new measured data or a regenerated oracle.
    const input=JSON.parse(fs.readFileSync(path.join(__dirname,'../examples/strip-identification/synthetic-calibration.json'),'utf8'));
    for(const experiment of input.experiments) for(const observation of experiment.observations) {
      const width=observation.mid_width;
      observation.extensometers=[{id:'midpoint',operator:{kind:'edge_chord',station_fraction:0.5},
        value_m:width.value_m,scale_m:width.scale_m}];
      delete observation.mid_width;
    }
    const request=process.env.FTX_EXTENSOMETER_INPUT?
      fs.readFileSync(process.env.FTX_EXTENSOMETER_INPUT,'utf8'):JSON.stringify(input);
    assert.deepEqual(JSON.parse(request),input);
    const w=await workerModule({bindings:ftx});
    const result=await w.send({type:'stripFit',request:47,stripRequest:request});
    assert.equal(result.type,'result',result.message);
    assert.equal(result.files.find(f=>f.name==='strip-calibration-input.json').text,request);
    assert.equal(result.files.find(f=>f.name==='strip-calibrated-selection.json').text,result.result.selection_json);
    if(process.env.FTX_EXTENSOMETER_REPLAY) {
      const {material,...nativeFields}=result.result;
      assert.deepEqual(nativeFields,JSON.parse(fs.readFileSync(process.env.FTX_EXTENSOMETER_REPLAY,'utf8')));
    }
    assert.deepEqual(result.result.material,JSON.parse(ftx.material_inspect(result.result.selection_json)));
    const c=result.result.calibration;
    assert.equal(c.accepted,true); assert.equal(c.domain_replay_verified,true);
    for(const prediction of c.predictions) {
      assert.equal(prediction.width_residual_m,null);
      assert.deepEqual(prediction.extensometers[0].operator,{kind:'edge_chord',station_fraction:0.5});
      assert.equal(prediction.extensometers[0].predicted_m,prediction.state.mid_width_m);
    }
    const check=c.input.experiments.find(e=>e.partition==='check');
    const forwardRequest={schema:'ftx-strip-experiment/1',material:result.result.selection,
      fixture:{...check.fixture,cells:check.fixture.cells.map(n=>2*n)},
      extensions:check.observations.map(o=>o.extension_fraction),controls:c.input.controls};
    const forward=await w.send({type:'stripExperiment',request:48,stripRequest:JSON.stringify(forwardRequest)});
    assert.equal(forward.type,'result',forward.message);
    assert.deepEqual(forward.result.states,c.predictions.filter(p=>p.experiment===check.id).map(p=>p.state));
    assert.equal(ftx.material_strip_experiment(JSON.stringify(forward.result.input)),forward.files[0].text);
    const p=await pageModule(); await p.load(); p.select('stripFitRequest',request); await p.e.stripFit.onclick();
    p.emit(result.result,result.files);
    const sectionResult=section(p,'Clamped-strip identification');
    assert.match(visibleText(sectionResult),/Declared transverse extensometer acquisitions/);
    assert.match(visibleText(sectionResult),/Refined extensometer predictions and residuals/);
    assert.equal(p.e.assign.disabled,false); assert.equal(p.e.selection.value,result.result.selection_json);
    const bad=JSON.parse(request);
    bad.experiments.find(e=>e.partition==='check').observations[0].extensometers[0].value_m+=0.1;
    const badRequest=JSON.stringify(bad);
    const rejected=await w.send({type:'stripFit',request:49,stripRequest:badRequest});
    assert.equal(rejected.type,'result',rejected.message);
    assert.equal(rejected.result.calibration.accepted,false);
    assert.deepEqual(rejected.result.calibration.parameters,c.parameters);
    assert.ok(rejected.result.calibration.check.peak_scaled>1);
    assert.equal(rejected.result.selection_json,null);
    assert.equal(rejected.files.some(f=>f.name==='strip-calibrated-selection.json'),false);
    p.select('stripFitRequest',badRequest); p.e.stripFitRequest.handlers.change(); p.ready(); await p.e.stripFit.onclick();
    p.emit(rejected.result,rejected.files);
    assert.equal(p.e.assign.disabled,true); assert.equal(p.e.selection.value,'');
    assert.match(visibleText(sectionResult),/No identified selection is available/);
  });

test('compiled WASM draped vapor coverage preserves source geometry, replay and unknown uncovered area',
  { skip: process.env.FTX_REAL_WASM !== '1' }, async () => {
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const input=fs.readFileSync(path.join(__dirname,'../examples/vapor-coverage/layered.json'),'utf8');
    const raw=ftx.thermal_vapor_coverage(input), report=JSON.parse(raw);
    if(process.env.FTX_VAPOR_COVERAGE_REPLAY) {
      assert.deepEqual(report,JSON.parse(fs.readFileSync(process.env.FTX_VAPOR_COVERAGE_REPLAY,'utf8')));
    }
    assert.equal(ftx.thermal_vapor_coverage(report.replay_request_json),raw);
    assert.ok(report.coverage.covered_area_m2>0);
    assert.ok(report.coverage.covered_potential_mass_flow_kg_s>0);
    assert.ok(report.coverage.uncovered_area_m2>=0);
    assert.ok(report.coverage.regions.some(region=>region.vapor.layers.length>1));
    const changed=JSON.parse(report.replay_request_json);
    changed.source.materials[0].record.name+=' changed';
    assert.throws(()=>ftx.thermal_vapor_coverage(JSON.stringify(changed)));
    const p=await pageModule(); await p.load(); scenarioInputs(p); await p.e.scenario.onclick();
    const scenario=scenarioReport(); scenario.scenario.scenario={vapor_coverage:{}};
    scenario.simulations.find(row=>row.key==='A').evaluation.result.vapor_coverage=outcome(report);
    p.emit(scenario);
    const s=section(p,'Fresh drape and heat alternatives');
    assert.match(visibleText(s),/Draped layered vapor coverage/);
    const download=descendants(s).find(n=>n.download==='alternative-1-vapor-coverage-replay.json');
    assert.equal(await p.blobs.get(download.href).text(),report.replay_request_json);
    p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
  });

test('compiled WASM moving vapor integrates changing area and replays the complete frozen history',
  { skip: process.env.FTX_REAL_WASM !== '1' }, async () => {
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const initial=JSON.parse(fs.readFileSync(path.join(__dirname,'../examples/vapor-coverage/layered.json'),'utf8'));
    const stretch=positions=>positions.map(([x,y,z])=>[2*x,y,z]);
    const input={schema:'ftx-vapor-coverage-history/1',
      policy:{schema:'ftx-motion-vapor-coverage/1',integration:'right_endpoint',max_geometry_operations:10000},
      initial,samples:[
        {time_s:1,positions_m:initial.source.positions_m,body_positions_m:initial.map.body_positions_m},
        {time_s:3,positions_m:stretch(initial.source.positions_m),body_positions_m:stretch(initial.map.body_positions_m)}]};
    const raw=ftx.thermal_vapor_coverage_history(JSON.stringify(input)), report=JSON.parse(raw);
    const energy=1000*0.002/(2*11.27+5)*5;
    assert.ok(Math.abs(report.totals.potential_energy_j-energy)<1e-12);
    assert.ok(Math.abs(report.totals.potential_mass_kg-energy/2500000)<1e-18);
    assert.equal(report.samples.length,2); assert.equal(report.duration_s,3);
    assert.equal(report.latent_heat_coupled,false);
    assert.equal(ftx.thermal_vapor_coverage_history(report.replay_request_json),raw);
    const compositeInput=JSON.stringify({...input,policy:{...input.policy,integration:'composite_trapezoid'}});
    const worker=await workerModule({bindings:ftx});
    const composite=await worker.send({type:'vapor',request:17,vaporRequest:compositeInput});
    assert.equal(composite.type,'result',composite.message);
    const compositeReport=composite.result, compositeRaw=ftx.thermal_vapor_coverage_history(compositeInput);
    // One second at initial power, then two seconds averaging the original and doubled areas.
    const compositeEnergy=1000*0.002/(2*11.27+5)*4;
    assert.equal(compositeReport.integration,'composite_trapezoid');
    assert.ok(Math.abs(compositeReport.totals.potential_energy_j-compositeEnergy)<1e-12);
    assert.ok(Math.abs(compositeReport.totals.potential_mass_kg-compositeEnergy/2500000)<1e-18);
    assert.deepEqual(plain(composite.files.map(f=>[f.name,f.text])),[
      ['vapor-report.json',compositeRaw],['vapor-request.json',compositeInput],
      ['vapor-replay-request.json',compositeReport.replay_request_json]]);
    assert.equal(ftx.thermal_vapor_coverage_history(compositeReport.replay_request_json),compositeRaw);
    if(process.env.FTX_VAPOR_HISTORY_REPLAY) {
      assert.deepEqual(report,JSON.parse(fs.readFileSync(process.env.FTX_VAPOR_HISTORY_REPLAY,'utf8')));
    }
    const changed=JSON.parse(report.replay_request_json);
    changed.initial.source.materials[0].record.name+=' changed';
    assert.throws(()=>ftx.thermal_vapor_coverage_history(JSON.stringify(changed)));
    changed.initial=input.initial; changed.policy.max_geometry_operations=1;
    assert.throws(()=>ftx.thermal_vapor_coverage_history(JSON.stringify(changed)));
  });

test('compiled WASM mixed cutting jobs cover every part and replay the frozen complete allocation',
  { skip: process.env.FTX_REAL_WASM !== '1' }, async () => {
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const pattern=fs.readFileSync(path.join(__dirname,'../examples/mixed-cutting/pattern.json'),'utf8');
    const request=fs.readFileSync(path.join(__dirname,'../examples/mixed-cutting/study.json'),'utf8');
    const w=await workerModule({bindings:ftx});
    const result=await w.send({type:'study',request:61,pattern,studyRequest:request});
    assert.equal(result.type,'result',result.message);
    assert.equal(result.files[0].text,ftx.material_study(pattern,request));
    assert.equal(result.files.find(f=>f.name==='original-pattern.json').text,pattern);
    assert.equal(result.files.find(f=>f.name==='study-request.json').text,request);
    if(process.env.FTX_MIXED_CUTTING_REPLAY) {
      assert.deepEqual(result.result,JSON.parse(fs.readFileSync(process.env.FTX_MIXED_CUTTING_REPLAY,'utf8')));
    }
    const row=result.result.alternatives[0], job=row.cutting_job.result;
    assert.equal(row.cutting_job.status,'available'); assert.equal(job.complete_inventory,true);
    assert.equal(job.inventory.length,6); assert.equal(job.hardware_execution_authorized,false);
    assert.deepEqual(job.roll.source_piece_ids,['lining']);
    assert.deepEqual(job.finite_stock.source_piece_ids,['shell']);
    assert.equal(new Set(job.inventory.map(i=>`${i.instance}/${i.copy}`)).size,6);
    assert.deepEqual(JSON.parse(ftx.material_cut_job(job.pattern_json,job.replay_request_json)),job);
    assert.equal(job.roll.pattern_sha256,job.pattern_sha256);
    assert.equal(job.finite_stock.pattern_sha256,job.pattern_sha256);
    const p=await pageModule(); await p.load(); p.select('studyRequest',request); await p.e.study.onclick();
    p.emit(result.result,result.files);
    const s=section(p,'Fixed-pattern manufacturing alternatives');
    assert.match(visibleText(s),/Complete cutting jobs/);
    const download=descendants(s).find(n=>n.download==='alternative-1-cut-job-request.json');
    assert.equal(await p.blobs.get(download.href).text(),job.replay_request_json);
    const bad=JSON.parse(request); bad.cutting_job.routes.pop();
    const refusedStudy=await w.send({type:'study',request:62,pattern,studyRequest:JSON.stringify(bad)});
    assert.equal(refusedStudy.type,'result',refusedStudy.message);
    assert.equal(refusedStudy.result.alternatives[0].binding.status,'available');
    assert.equal(refusedStudy.result.alternatives[0].cutting_job.status,'refused');
    assert.deepEqual(refusedStudy.result.comparison.frontier,[]);
    assert.throws(()=>ftx.material_cut_job(pattern,JSON.stringify(bad.cutting_job)));
  });

test('compiled WASM material studies recompute vapor from final assignments and retain independent refusals',
  { skip: process.env.FTX_REAL_WASM !== '1' }, async () => {
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const w=await workerModule({bindings:ftx});
    const pattern=fs.readFileSync(path.join(__dirname,'../tests/fixtures/materials/synthetic-sewing-pattern.json'),'utf8');
    const request=fs.readFileSync(path.join(__dirname,'../examples/vapor-study/leather.json'),'utf8');
    const result=await w.send({type:'study',request:45,pattern,studyRequest:request});
    assert.equal(result.type,'result',result.message);
    assert.equal(result.files[0].text,ftx.material_study(pattern,request));
    assert.equal(result.files.find(f=>f.name==='study-request.json').text,request);
    if(process.env.FTX_VAPOR_STUDY_REPLAY) {
      assert.deepEqual(result.result,JSON.parse(fs.readFileSync(process.env.FTX_VAPOR_STUDY_REPLAY,'utf8')));
    }
    const rows=Object.fromEntries(result.result.alternatives.map(row=>[row.key,row]));
    const chrome=rows['chrome-neck'].vapor.result, alternate=rows['synthetic-tanned-neck'].vapor.result;
    assert.equal(chrome.layers[0].observation.value,11.27);
    assert.equal(alternate.layers[0].observation.value,30.38);
    assert.ok(chrome.potential_mass_flow_kg_s>alternate.potential_mass_flow_kg_s);
    assert.notEqual(chrome.layers[0].material_sha256,alternate.layers[0].material_sha256);
    assert.equal(rows.uncharacterized.binding.status,'available');
    assert.equal(rows.uncharacterized.vapor.status,'refused');
    assert.deepEqual(result.result.comparison.frontier,['chrome-neck']);
    assert.equal(result.result.geometry_changed,false);
    assert.equal(result.result.physical_validation_claim,false);
    for(const key of ['chrome-neck','synthetic-tanned-neck']) {
      const row=rows[key], vapor=row.vapor.result;
      const bound=JSON.parse(ftx.material_bind(pattern,JSON.stringify(row.frozen_changes)));
      assert.equal(bound.pattern_json,row.binding.result.pattern_json);
      const replay=JSON.parse(ftx.thermal_vapor(vapor.replay_request_json));
      assert.deepEqual(replay.layers,vapor.layers);
      assert.equal(replay.potential_heat_flow_w,vapor.potential_heat_flow_w);
      assert.equal(replay.potential_mass_flow_kg_s,vapor.potential_mass_flow_kg_s);
      assert.equal(vapor.layers[0].observation.conditions.air_temperature_c,'35');
    }
    const missing=JSON.parse(request); missing.vapor.layers[0].target.id='absent-source-piece';
    const refusedStudy=await w.send({type:'study',request:46,pattern,studyRequest:JSON.stringify(missing)});
    assert.equal(refusedStudy.type,'result',refusedStudy.message);
    assert.ok(refusedStudy.result.alternatives.every(row=>row.binding.status==='available'&&row.vapor.status==='refused'));
    assert.deepEqual(refusedStudy.result.comparison.frontier,[]);
  });

test('compiled WASM vapor exchange preserves source-qualified substitutions and frozen replay',
  { skip: process.env.FTX_REAL_WASM !== '1' }, async () => {
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const w=await workerModule({bindings:ftx});
    const request=fs.readFileSync(path.join(__dirname,'../tests/fixtures/materials/leather-vapor-stack.json'),'utf8');
    const first=await w.send({type:'vapor',request:1,vaporRequest:request});
    assert.equal(first.type,'result',first.message);
    assert.equal(first.files[0].text,ftx.thermal_vapor(request));
    assert.equal(first.files[1].text,request);
    assert.equal(first.files[2].text,first.result.replay_request_json);
    assert.equal(first.result.layers[0].observation.value,11.27);
    assert.equal(first.result.layers[0].observation.conditions.air_temperature_c,'35');
    assert.equal(first.result.physical_validation_claim,false);
    const frozen=JSON.parse(ftx.thermal_vapor(first.result.replay_request_json));
    assert.deepEqual(frozen.layers,first.result.layers);
    assert.equal(frozen.potential_heat_flow_w,first.result.potential_heat_flow_w);
    const swap=JSON.parse(request); swap.layers[0].selection.id='zagreb/leather-2022/syn-d-h-neck';
    const second=await w.send({type:'vapor',request:2,vaporRequest:JSON.stringify(swap)});
    assert.equal(second.type,'result',second.message);
    assert.equal(second.result.layers[0].observation.value,30.38);
    assert.ok(second.result.potential_heat_flow_w<first.result.potential_heat_flow_w);
    assert.notEqual(second.result.layers[0].material_sha256,first.result.layers[0].material_sha256);
    assert.deepEqual(second.result.request.required_conditions,first.result.request.required_conditions);
    if(process.env.FTX_VAPOR_REPLAY) assert.deepEqual(first.result,JSON.parse(fs.readFileSync(process.env.FTX_VAPOR_REPLAY,'utf8')));
    const tampered=JSON.parse(first.result.replay_request_json);
    tampered.layers[0].selection.record.name+=' changed';
    const rejected=await w.send({type:'vapor',request:3,vaporRequest:JSON.stringify(tampered)});
    assert.equal(rejected.type,'error'); assert.equal(rejected.files,undefined);
  });

test('compiled WASM partial sewing spans reach the same stitch program and reject uncovered joins',
  { skip: process.env.FTX_REAL_WASM !== '1' }, async () => {
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const w=await workerModule({bindings:ftx}), fixture=n=>fs.readFileSync(path.join(__dirname,'../tests/fixtures/materials',n),'utf8');
    const pattern=fixture('synthetic-sewing-pattern.json'), options=fixture('synthetic-sewing-span-options.json');
    const result=await w.send({type:'sew',request:1,pattern,options});
    assert.equal(result.type,'result',result.message);
    assert.equal(result.files[0].text,ftx.sew_stitches(pattern,options));
    assert.equal(result.result.plan.schema,'ftx-sewing-plan/10');
    assert.ok(result.result.plan.seams.every(s=>s.source_span));
    assert.equal(result.result.plan.hardware_execution_authorized,false);
    if(process.env.FTX_SEWING_SPAN_REPLAY) assert.deepEqual(result.result,JSON.parse(fs.readFileSync(process.env.FTX_SEWING_SPAN_REPLAY,'utf8')));
    const gap=JSON.parse(options); gap.passes[1].source_range[0]+=0.1;
    const rejected=await w.send({type:'sew',request:2,pattern,options:JSON.stringify(gap)});
    assert.equal(rejected.type,'error'); assert.equal(rejected.files,undefined);
  });

test('compiled WASM studies preserve source replay and optional model-backed motion equality',
  { skip: process.env.FTX_REAL_WASM !== '1' }, async () => {
    const pkg = process.env.FTX_WASM_PKG ? path.resolve(process.env.FTX_WASM_PKG) : path.join(__dirname, '../web/pkg');
    assert.ok(fs.existsSync(path.join(pkg, 'ftx_wasm_bg.wasm')), 'Build web/pkg before FTX_REAL_WASM=1');
    const ftx = await import(require('node:url').pathToFileURL(path.join(pkg, 'ftx_wasm.js')).href);
    ftx.initSync({ module: fs.readFileSync(path.join(pkg, 'ftx_wasm_bg.wasm')) });
    const w = await workerModule({ bindings: ftx });
    const fixture = n => fs.readFileSync(path.join(__dirname, '../tests/fixtures/materials', n), 'utf8');
    for (const admission of [false, true]) {
      const calibration = JSON.parse(ftx.material_calibrate_nonlinear(fixture('synthetic-nonlinear-swatch.json'), admission));
      assert.equal(calibration.material.capabilities.mechanical_coefficients.ready, admission);
      assert.deepEqual(calibration.material, JSON.parse(ftx.material_inspect(calibration.material.selection_json)));
    }
    for (const [mode, filename] of [['linear','synthetic-swatch.json'], ['bending','synthetic-bending-coupons.json'],
      ['nonlinear','synthetic-nonlinear-swatch.json'],
      ['relaxation','synthetic-relaxation-history.json'], ['relaxation','synthetic-orthotropic-relaxation-history.json'],
      ['relaxation','synthetic-orthotropic-relaxation-search.json'],
      ['membrane_plasticity','synthetic-membrane-plasticity-calibration.json']]) {
      const calibrationRequest = fixture(filename);
      for (const allowEstimates of [false, true]) {
        const identified = await w.send({type:'calibrate',calibrationMode:mode,calibrationRequest,allowEstimates});
        assert.equal(identified.type,'result',identified.message);
        const expected = ftx[`material_calibrate_${mode}`](calibrationRequest,allowEstimates);
        assert.equal(identified.files[0].text,expected);
        assert.equal(identified.files[1].text,identified.result.selection_json);
        assert.equal(identified.files[2].text,calibrationRequest);
        assert.deepEqual(JSON.parse(ftx.material_inspect(identified.result.selection_json)),identified.result.material);
        assert.equal(identified.result.selection.allow_estimates,allowEstimates);
        assert.equal(identified.result.material.capabilities.mechanical_coefficients.ready,allowEstimates);
        if (JSON.parse(calibrationRequest).orthotropic?.time_bounds_seconds?.length) {
          const c=identified.result.calibration;
          assert.ok(c.orthotropic_fit.time_search.starts_converged>0);
          assert.equal(c.orthotropic_fit.time_search.starts_attempted,c.starts_attempted);
        }
        const changes = `{"schema":"ftx-material-bindings/1","default":${identified.result.selection_json}}`;
        const binding = await w.send({type:'bind',pattern:fixture('synthetic-sewing-pattern.json'),changes});
        assert.equal(binding.type,'result',binding.message);
        assert.equal(binding.result.geometry_changed,false);
        assert.equal(binding.result.pattern.fabric.material.expected_sha256,identified.result.selection.expected_sha256);
        assert.equal(binding.result.drape_materials_with_self_contact.ready,allowEstimates);
      }
    }
    const sliding=fixture('synthetic-friction-sliding.json'), slidingInput=JSON.parse(sliding);
    const sourceSelections=slidingInput.target.materials.map(material=>JSON.parse(ftx.material_inspect(JSON.stringify(material))).selection_json);
    const pairEdits=`{"schema":"ftx-material-bindings/1","default":${sourceSelections[0]},"pieces":{"back":${sourceSelections[1]}}}`;
    for (const allowEstimates of [false,true]) {
      const fitted=await w.send({type:'calibrate',calibrationMode:'friction_pair',calibrationRequest:sliding,allowEstimates});
      assert.equal(fitted.type,'result',fitted.message);
      assert.equal(fitted.files[0].text,ftx.material_calibrate_friction(sliding,allowEstimates));
      assert.equal(fitted.result.selection_json,null);assert.equal(fitted.result.material,null);
      assert.equal(fitted.result.readiness.ready,allowEstimates);
      assert.equal(fitted.result.readiness.whole_garment_checked,false);
      assert.ok(fitted.result.readiness.sheets.every(sheet=>sheet.ready));
      assert.equal(fitted.files.find(f=>f.name==='calibrated-interface.json').text,fitted.result.interface_json);
      assert.equal(fitted.files.find(f=>f.name==='friction-binding.json').text,fitted.result.binding_json);
      assert.equal(fitted.files.at(-1).text,sliding);
      const stage={type:'stageInterface',pattern:fixture('synthetic-sewing-pattern.json'),changes:pairEdits,
        interfaceJson:fitted.result.interface_json,replaceExisting:false};
      const staged=await w.send(stage);assert.equal(staged.type,'result',staged.message);
      assert.deepEqual(staged.result,JSON.parse(ftx.material_stage_interface(stage.pattern,pairEdits,stage.interfaceJson,false)));
      const binding=await w.send({type:'bind',pattern:stage.pattern,changes:staged.result.changes_json});
      assert.equal(binding.type,'result',binding.message);assert.equal(binding.result.geometry_changed,false);
      assert.equal(binding.result.drape_materials_with_self_contact.ready,allowEstimates);
      assert.deepEqual(binding.result.pattern.fabric.interfaces,[fitted.result.binding.interface]);
      assert.equal(binding.result.pieces[0].material_sha256,JSON.parse(sourceSelections[0]).expected_sha256);
      const duplicate=await w.send({...stage,changes:staged.result.changes_json});
      assert.equal(duplicate.type,'error');assert.match(duplicate.message,/replace_existing/);
      const replaced=await w.send({...stage,changes:staged.result.changes_json,replaceExisting:true});
      assert.equal(replaced.type,'result',replaced.message);assert.equal(JSON.parse(replaced.result.interfaces_json).length,1);
      const stale=stage.interfaceJson.replace(JSON.parse(sourceSelections[0]).expected_sha256,'0'.repeat(64));
      assert.equal((await w.send({...stage,interfaceJson:stale})).type,'error');
    }
    for (const [mode,kind,key] of [['friction_self','self_contact','friction_self'],['friction_body','body','friction_skin']]) {
      const input={...slidingInput,target:{kind,material:slidingInput.target.materials[0]},replace_existing:true};
      if (kind==='body') input.target.counterface={reference:'test:synthetic-surrogate',accessed_on:'2026-10-18',note:'Synthetic software counterface; no human-skin qualification'};
      const request=JSON.stringify(input), fitted=await w.send({type:'calibrate',calibrationMode:mode,calibrationRequest:request,allowEstimates:true});
      assert.equal(fitted.type,'result',fitted.message);assert.equal(fitted.result.binding.kind,'material');
      assert.equal(fitted.result.interface_json,null);assert.equal(fitted.result.readiness.scope,'material_mechanics');
      assert.ok(Math.abs(fitted.result.selection.record.properties[key].value-0.4)<1e-12);
      assert.equal(fitted.files.find(f=>f.name==='calibrated-selection.json').text,fitted.result.selection_json);
      assert.equal(fitted.files.find(f=>f.name==='friction-binding.json').text,fitted.result.binding_json);
      assert.deepEqual(JSON.parse(ftx.material_inspect(fitted.result.selection_json)),fitted.result.material);
    }
    const stripRequest=fixture('synthetic-clamped-strip.json');
    const strip=await w.send({type:'calibrate',calibrationMode:'clamped_strip',
      calibrationRequest:stripRequest,allowEstimates:true});
    assert.equal(strip.type,'result',strip.message);
    assert.equal(strip.result.calibration.accepted,true);
    assert.equal(strip.result.calibration.domain_replay_verified,true);
    assert.deepEqual(strip.result.calibration.reasons,[]);
    assert.equal(strip.result.calibration.fit.components,12);
    assert.equal(strip.result.calibration.check.components,2);
    assert.equal(strip.result.material.capabilities.mechanical_coefficients.ready,false);
    assert.equal(strip.files[1].text,strip.result.selection_json);
    assert.equal(strip.files[2].text,stripRequest);
    assert.deepEqual(JSON.parse(ftx.material_inspect(strip.result.selection_json)),strip.result.material);
    if(process.env.FTX_STRIP_REPLAY) {
      const native=JSON.parse(fs.readFileSync(process.env.FTX_STRIP_REPLAY,'utf8'));
      assert.deepEqual(strip.result.calibration,native.calibration);
      assert.equal(strip.result.selection_json,native.selection_json);
    }
    const stripInput=JSON.parse(stripRequest);
    const forwardRequest=`{"schema":"ftx-strip-experiment/1","material":${strip.result.selection_json},`+
      `"fixture":${JSON.stringify(stripInput.experiments[0].fixture)},"extensions":[0,0.03],`+
      `"controls":${JSON.stringify(stripInput.controls)}}`;
    const forwardRaw=ftx.material_strip_experiment(forwardRequest),forward=JSON.parse(forwardRaw);
    assert.equal(forward.states.length,2);
    assert.ok(forward.states[1].force_n>0);
    assert.equal(forward.input.material.expected_sha256,strip.result.selection.expected_sha256);
    assert.equal(ftx.material_strip_experiment(JSON.stringify(forward.input)),forwardRaw);
    const stripBinding=await w.send({type:'bind',pattern:fixture('synthetic-sewing-pattern.json'),
      changes:`{"schema":"ftx-material-bindings/1","default":${strip.result.selection_json}}`});
    assert.equal(stripBinding.type,'result',stripBinding.message);
    assert.equal(stripBinding.result.geometry_changed,false);
    assert.equal(stripBinding.result.drape_materials_with_self_contact.ready,false);
    const intervalRequest=fixture('synthetic-nonlinear-clamped-strip.json');
    const intervals=await w.send({type:'calibrate',calibrationMode:'clamped_strip',
      calibrationRequest:intervalRequest,allowEstimates:true});
    assert.equal(intervals.type,'result',intervals.message);
    const intervalCalibration=intervals.result.calibration;
    assert.equal(intervalCalibration.accepted,true);
    assert.equal(intervalCalibration.domain_replay_verified,true);
    assert.equal(intervalCalibration.parameters.length,9);
    assert.equal(intervalCalibration.parameter_descriptors.length,9);
    assert.equal(intervalCalibration.parameter_bounds.length,9);
    assert.ok(intervalCalibration.parameter_descriptors.every(d=>d.unit==='N/m'));
    assert.deepEqual(intervalCalibration.reasons,[]);
    assert.equal(intervals.files[1].text,intervals.result.selection_json);
    assert.equal(intervals.files[2].text,intervalRequest);
    assert.equal(intervals.result.material.capabilities.mechanical_coefficients.ready,false);
    assert.deepEqual(JSON.parse(ftx.material_inspect(intervals.result.selection_json)),intervals.result.material);
    if(process.env.FTX_INTERVAL_STRIP_REPLAY) {
      const native=JSON.parse(fs.readFileSync(process.env.FTX_INTERVAL_STRIP_REPLAY,'utf8'));
      assert.deepEqual(intervalCalibration,native.calibration);
      assert.equal(intervals.result.selection_json,native.selection_json);
    }
    const intervalInput=JSON.parse(intervalRequest);
    const heldOut=intervalInput.experiments.find(e=>e.partition==='check');
    const refined={...heldOut.fixture,cells:heldOut.fixture.cells.map(n=>n*2)};
    const intervalForward=JSON.parse(ftx.material_strip_experiment(
      `{"schema":"ftx-strip-experiment/1","material":${intervals.result.selection_json},`+
      `"fixture":${JSON.stringify(refined)},`+
      `"extensions":${JSON.stringify(heldOut.observations.map(o=>o.extension_fraction))},`+
      `"controls":${JSON.stringify(intervalInput.controls)}}`));
    const predicted=intervalCalibration.predictions.filter(p=>p.experiment===heldOut.id);
    assert.deepEqual(intervalForward.states,predicted.map(p=>p.state));
    const intervalBinding=await w.send({type:'bind',pattern:fixture('synthetic-sewing-pattern.json'),
      changes:`{"schema":"ftx-material-bindings/1","default":${intervals.result.selection_json}}`});
    assert.equal(intervalBinding.type,'result',intervalBinding.message);
    assert.equal(intervalBinding.result.geometry_changed,false);
    assert.equal(intervalBinding.result.drape_materials_with_self_contact.ready,false);
    const unfinished={...stripInput,policy:{...stripInput.policy,max_iterations:1}};
    const unfinishedRaw=JSON.stringify(unfinished);
    const unfinishedReport=await w.send({type:'calibrate',calibrationMode:'clamped_strip',
      calibrationRequest:unfinishedRaw,allowEstimates:true});
    assert.equal(unfinishedReport.type,'result',unfinishedReport.message);
    assert.equal(unfinishedReport.result.calibration.accepted,false);
    assert.equal(unfinishedReport.result.selection_json,null);
    assert.equal(unfinishedReport.files.length,2);
    assert.equal(unfinishedReport.files[1].text,unfinishedRaw);
    const exhausted={...stripInput,policy:{...stripInput.policy,max_forward_solves:1}};
    const exhaustedReport=await w.send({type:'calibrate',calibrationMode:'clamped_strip',
      calibrationRequest:JSON.stringify(exhausted),allowEstimates:true});
    assert.equal(exhaustedReport.type,'error');
    assert.match(exhaustedReport.message,/budget/i);
    const compressionRequest = fixture('synthetic-compression.json');
    const prepared = await w.send({ type: 'pressure', compressionRequest });
    assert.equal(prepared.type, 'result', prepared.message);
    const pressureReport = ftx.material_pressure_condition(compressionRequest);
    assert.equal(prepared.files[0].text, pressureReport);
    assert.equal(prepared.files[1].text, prepared.result.selection_json);
    assert.equal(prepared.files[2].text, compressionRequest);
    assert.equal(prepared.result.inspection.capabilities.steady_conduction.ready, true);
    assert.equal(prepared.result.inspection.capabilities.transient_thermal.ready, true);
    assert.equal(prepared.result.inspection.capabilities.sewing_thickness_m.ready, false);
    assert.equal(prepared.result.material.properties.thickness.value, 0.015);
    assert.equal(prepared.result.material.properties.thermal_resistance.value, 0.375);
    assert.equal((await w.send({ type: 'pressure', compressionRequest })).files[0].text, pressureReport);
    if (process.env.FTX_PRESSURE_REPLAY) {
      assert.deepEqual(JSON.parse(pressureReport), JSON.parse(fs.readFileSync(process.env.FTX_PRESSURE_REPLAY, 'utf8')));
    }
    const screenRequest = fixture('screen-insulation.json');
    const screened = await w.send({ type: 'screen', screenRequest });
    assert.equal(screened.type, 'result', screened.message);
    assert.deepEqual(screened.result, JSON.parse(ftx.material_screen(screenRequest)));
    assert.equal(screened.files[1].text, screenRequest);
    const knitRequest = JSON.stringify({ schema:'ftx-material-screen/1', basis:'nominal', query:'sperl2022/',
      forms:['knit'], allow_mixed_protocols:true,
      constraints:[{attribute:{key:'strip_force_30pct_weft',unit:'N',conditions:{branch:'loading',
        free_gauge_length_mm:'100',reference_strip_width_mm:'60'}},minimum:2}],
      objectives:[{attribute:{key:'areal_density',unit:'kg/m2'},direction:'minimize'}] });
    const knits = await w.send({type:'screen',screenRequest:knitRequest});
    assert.equal(knits.type,'result',knits.message);
    assert.equal(knits.files[0].text,ftx.material_screen(knitRequest));
    assert.deepEqual(knits.result.frontier,['sperl2022/SJ3']);
    assert.equal(knits.result.alternatives.filter(row => row.key.startsWith('sperl2022/')).length,3);
    const knitWinner = knits.result.alternatives.find(row => row.key === 'sperl2022/SJ3');
    assert.equal(knitWinner.readings.strip_force_30pct_weft.value,2.48);
    const knitInspection = JSON.parse(ftx.material_inspect(JSON.stringify(knitWinner.selection)));
    assert.equal(knitInspection.capabilities.mechanical_coefficients.ready,false);
    assert.equal(knitInspection.capabilities.steady_conduction.ready,false);
    assert.equal(knitInspection.capabilities.sewing_thickness_m.ready,false);
    const pattern = fixture('synthetic-sewing-pattern.json');
    const pressureChanges = `{"schema":"ftx-material-bindings/1","default":${prepared.result.selection_json}}`;
    const pressureBinding = await w.send({ type: 'bind', pattern, changes: pressureChanges });
    assert.equal(pressureBinding.type, 'result', pressureBinding.message);
    assert.equal(pressureBinding.result.geometry_changed, false);
    assert.equal(pressureBinding.result.drape_materials_with_self_contact.ready, true);
    assert.equal(pressureBinding.result.pattern.fabric.material.expected_sha256, prepared.result.selection.expected_sha256);
    // Different loaded-state selections require their own explicitly qualified interface.
    const highInput=JSON.parse(compressionRequest); highInput.pressure_pa=500;
    const high=JSON.parse(ftx.material_pressure_condition(JSON.stringify(highInput)));
    const pair={materials:[prepared.result.selection.expected_sha256,high.selection.expected_sha256],
      friction:{value:0.18,unit:'1',evidence:'estimate',source:'test',method:'Synthetic software pair observation'},
      sources:{test:{reference:'test:compiled-interface',accessed_on:'2026-10-19',note:'Synthetic test only'}},allow_estimates:true};
    const interfaceChanges=`{"schema":"ftx-material-bindings/1","default":${prepared.result.selection_json},"pieces":{"back":${high.selection_json}},"interfaces":${JSON.stringify([pair])}}`;
    const paired=await w.send({type:'bind',pattern,changes:interfaceChanges});
    assert.equal(paired.type,'result',paired.message);
    assert.equal(paired.result.drape_materials_with_self_contact.ready,true);
    assert.deepEqual(paired.result.pattern.fabric.interfaces,[pair]);
    assert.equal(paired.files[2].text,interfaceChanges);
    const pairedStudy=`{"schema":"ftx-material-study/1","variants":[{"key":"pair","changes":${interfaceChanges}}],"cutting":{"copies":1,"gap_mm":2,"edge_margin_mm":0,"end_margin_mm":0}}`;
    const pairRow=JSON.parse(ftx.material_study(pattern,pairedStudy)).alternatives[0];
    assert.deepEqual(pairRow.frozen_changes.interfaces,[pair]);
    assert.equal(pairRow.binding.result.pattern_json,paired.result.pattern_json);
    assert.equal(JSON.parse(ftx.material_bind(pattern,JSON.stringify(pairRow.frozen_changes))).pattern_json,paired.result.pattern_json);
    const combinationPattern = pressureBinding.result.pattern_json;
    const combinationGroups = ['front', 'back'].map(piece => `{"key":"${piece}","target":{"scope":"pieces","ids":["${piece}"]},"choices":[{"key":"low","selection":${prepared.result.selection_json}},{"key":"high","selection":${high.selection_json}}]}`).join(',');
    const combinationRequest = pool => `{"schema":"ftx-material-study/1","combinations":{"groups":[${combinationGroups}],"interfaces":${JSON.stringify(pool)}},"cutting":{"copies":1,"gap_mm":2,"edge_margin_mm":0,"end_margin_mm":0}}`;
    for (const supplied of [false, true]) {
      const request = combinationRequest(supplied ? [pair] : []);
      const combined = await w.send({ type:'study', pattern:combinationPattern, studyRequest:request });
      assert.equal(combined.type,'result',combined.message);
      assert.equal(combined.files[0].text,ftx.material_study(combinationPattern,request));
      assert.equal(combined.files[2].text,request);
      assert.equal(combined.result.alternatives.length,4);
      for (const row of combined.result.alternatives) {
        const mixed = row.choices.front !== row.choices.back;
        assert.equal(row.binding.status,'available');
        const replay = JSON.parse(ftx.material_bind(combinationPattern,JSON.stringify(row.frozen_changes)));
        assert.equal(replay.drape_materials_with_self_contact.ready,!mixed || supplied);
        assert.deepEqual(row.frozen_changes.interfaces,mixed && supplied ? [pair] : []);
        assert.equal(replay.pattern_json,row.binding.result.pattern_json);
      }
    }
    pair.allow_estimates=false;
    // Replace only the pair policy; each material keeps its independently authored consent.
    const deniedChanges=`{"schema":"ftx-material-bindings/1","default":${prepared.result.selection_json},"pieces":{"back":${high.selection_json}},"interfaces":${JSON.stringify([pair])}}`;
    const deniedInterface=await w.send({type:'bind',pattern,changes:deniedChanges});
    assert.equal(deniedInterface.type,'result',deniedInterface.message);
    assert.equal(deniedInterface.result.drape_materials_with_self_contact.ready,false);
    const variants = [['a-wide', 0.1, 0.4], ['b-narrow', 0.06, 2], ['c-unknown-width', null, 0.4]].map(([key, width, thickness]) => {
      const selection = JSON.parse(pattern).fabric.material;
      for (const [name, value, unit] of [['width', width, 'm'], ['thickness', thickness, 'mm'], ['areal_density', 0.2, 'kg/m2']]) {
        if (value !== null) selection.record.properties[name] = { value, unit, evidence: 'estimate', source: 'test',
          method: 'Synthetic software test; not measured textile data' };
      }
      return { key, changes: { schema: 'ftx-material-bindings/1', default: selection } };
    });
    const studyRequest = JSON.stringify({ schema: 'ftx-material-study/1', variants,
      cutting: { copies: 1, gap_mm: 2, edge_margin_mm: 0, end_margin_mm: 0 },
      sewing: JSON.parse(fixture('synthetic-sewing-options.json')), purchasing: { quotes: { schema: 'ftx-textile-quotes/1', quotes: [] },
        market: { currency: 'USD', region: 'US', quality: 'test', terms: 'test', as_of: '2026-10-15', max_age_days: 30 } } });
    const study = await w.send({ type: 'study', pattern, studyRequest });
    assert.equal(study.type, 'result', study.message);
    assert.deepEqual(study.result, JSON.parse(ftx.material_study(pattern, studyRequest)));
    assert.equal(study.files[1].text, pattern); assert.equal(study.files[2].text, studyRequest);
    const rows = study.result.alternatives;
    assert.equal(rows.length, 3); assert.ok(rows.every(r => r.binding.status === 'available'));
    assert.equal(rows[0].cutting.status, 'available'); assert.equal(rows[1].cutting.status, 'available');
    assert.ok(rows[1].cutting.result.rolls[0].length_m > rows[0].cutting.result.rolls[0].length_m);
    assert.equal(rows[0].sewing.status, 'available'); assert.equal(rows[1].sewing.status, 'refused');
    assert.equal(rows[2].cutting.status, 'refused'); assert.equal(rows[2].sewing.status, 'available');
    assert.ok(rows.every(r => r.metrics.least_observed_independent_order_total === null));
    for (const row of rows) {
      const replay = JSON.parse(ftx.material_bind(pattern, JSON.stringify(row.frozen_changes)));
      assert.equal(replay.pattern_json, row.binding.result.pattern_json);
    }
    assert.equal((await w.send({ type: 'study', pattern, studyRequest })).files[0].text, study.files[0].text);
    const compared = JSON.parse(studyRequest);
    compared.comparison = { basis: 'nominal', objectives: [{ metric: 'stock_mass_kg', direction: 'minimize' }], required_consumers: ['sewing'] };
    const comparisonRequest = JSON.stringify(compared);
    const comparison = await w.send({ type: 'study', pattern, studyRequest: comparisonRequest });
    assert.equal(comparison.type, 'result', comparison.message);
    assert.deepEqual(comparison.result.alternatives, study.result.alternatives);
    assert.deepEqual(comparison.result, JSON.parse(ftx.material_study(pattern, comparisonRequest)));
    assert.deepEqual(comparison.result.comparison.frontier, ['a-wide']);
    assert.equal(comparison.result.comparison.uncertainty_propagated, false);
    assert.equal(comparison.result.comparison.alternatives[0].readings.stock_mass_kg.value, rows[0].metrics.stock_mass_kg);
    assert.equal(comparison.files[2].text, comparisonRequest);
    compared.comparison.objectives = [{ metric: 'observed_order_cost', direction: 'minimize' }];
    const noOffers = await w.send({ type: 'study', pattern, studyRequest: JSON.stringify(compared) });
    assert.equal(noOffers.type, 'result', noOffers.message);
    assert.deepEqual(noOffers.result.comparison.frontier, []);
    // Authored, unlocked synthetic alternatives; source-locked results never go through stringify.
    const massOnly = { schema: 'ftx-material-study/1', variants: JSON.parse(studyRequest).variants,
      assembly_mass: JSON.parse(fixture('study-assembly-mass.json')).assembly_mass,
      comparison: { basis: 'nominal', objectives: [{ metric: 'assembly_mass_kg', direction: 'minimize' }] } };
    const densities = [0.2, 0.4, 0.1];
    massOnly.variants.forEach((v, i) => { v.changes.default.record.properties.areal_density.value = densities[i]; });
    const massRequest = JSON.stringify(massOnly);
    const mass = await w.send({ type: 'study', pattern, studyRequest: massRequest });
    assert.equal(mass.type, 'result', mass.message);
    assert.deepEqual(mass.result, JSON.parse(ftx.material_study(pattern, massRequest)));
    assert.deepEqual(mass.result.comparison.frontier, ['c-unknown-width']);
    for (const [i, row] of mass.result.alternatives.entries()) {
      assert.equal(row.cutting.status, 'not_requested');
      assert.equal(row.assembly_mass.status, 'available');
      assert.ok(Math.abs(row.metrics.assembly_mass_kg - (0.0096 * densities[i] + 0.015)) < 1e-12);
      assert.equal(row.assembly_mass.result.pattern_sha256, row.binding.result.pattern_sha256);
    }
    assert.equal(mass.files[2].text, massRequest);
    assert.equal((await w.send({ type: 'study', pattern, studyRequest: massRequest })).files[0].text, mass.files[0].text);
    const attachedSource = JSON.parse(pattern);
    const attachedExtra = {...massOnly.assembly_mass.extras[0],mass:{...massOnly.assembly_mass.extras[0].mass,conditions:{}}};
    attachedSource.attached_mass = {schema:'ftx-attached-mass/1',groups:[{extra:attachedExtra,
      targets:[{instance:'front',vertex:'a',fraction:1}]}]};
    const attachedPattern = JSON.stringify(attachedSource);
    const attached = await w.send({type:'study',pattern:attachedPattern,studyRequest:massRequest});
    assert.equal(attached.type,'result',attached.message);
    assert.equal(attached.files[0].text,ftx.material_study(attachedPattern,massRequest));
    for (const [i,row] of attached.result.alternatives.entries()) {
      assert.equal(row.assembly_mass.status,'available');
      assert.equal(row.assembly_mass.result.extras.length,1);
      assert.deepEqual(row.assembly_mass.result.attached_inventory,attachedSource.attached_mass);
      assert.deepEqual(row.assembly_mass.result.assembly_mass,mass.result.alternatives[i].assembly_mass.result.assembly_mass);
      assert.deepEqual(row.binding.result.pattern.attached_mass,attachedSource.attached_mass);
    }
    massOnly.assembly_mass.extras[0].mass = null;
    const unknown = await w.send({ type: 'study', pattern, studyRequest: JSON.stringify(massOnly) });
    assert.equal(unknown.type, 'result', unknown.message);
    assert.deepEqual(unknown.result.comparison.frontier, []);
    assert.ok(unknown.result.alternatives.every(r => r.assembly_mass.result.sheet_mass.kg > 0 &&
      r.assembly_mass.result.assembly_mass.kg === null && r.metrics.assembly_mass_kg === undefined));
    const invalid = await w.send({ type: 'scenario', pattern, studyRequest, scenarioRequest: '{}', measurements: '{}', bytes: new ArrayBuffer(8) });
    assert.equal(invalid.type, 'error'); assert.match(invalid.message, /ftxbody/);
    // Optional positive source-model motion replay from the existing real-binary fixture producer.
    // It is synthetic geometry, not private wearer data or an anatomical acceptance experiment.
    if (process.env.FTX_MOTION_REPLAY) {
      const dir = process.env.FTX_MOTION_REPLAY;
      const input = name => fs.readFileSync(path.join(dir, name), 'utf8');
      const bytes = fs.readFileSync(path.join(dir, 'model.ftxbody'));
      const msg = { type: 'scenario', request: 93, pattern: input('pattern.json'), studyRequest: input('study.json'),
        scenarioRequest: input('scenario.json'), measurements: input('body-input.json'),
        bytes: bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) };
      const result = await w.send(msg);
      assert.equal(result.type, 'result', result.message);
      assert.deepEqual(result.result, JSON.parse(input('motion-report.json')));
      assert.equal(result.files.find(f => f.name === 'scenario-request.json').text, msg.scenarioRequest);
      assert.equal(result.files.find(f => f.name === 'body-input.json').text, msg.measurements);
      const a = result.result.simulations[0].evaluation.result.motion;
      const b = result.result.simulations[1].evaluation.result.motion;
      assert.equal(a.status, 'available'); assert.equal(b.status, 'available');
      assert.deepEqual(a.result.samples, b.result.samples);
      assert.notEqual(a.result.keyframes[1].surface.result.steady.power_w, b.result.keyframes[1].surface.result.steady.power_w);
      for (const name of ['transient_surface', 'transient_coverage']) {
        if (!JSON.parse(msg.scenarioRequest).motion[name]) continue;
        const h = a.result[name]; assert.equal(h.status, 'available');
        assert.equal(b.result[name].status, 'available');
        assert.equal(h.result.duration_s,a.result.duration_s);
        assert.equal(h.result.path_sha256,a.result.path_sha256);
        assert.equal(h.result.trace.length,a.result.samples.length);
        assert.ok(Math.abs(h.result.balance_error_j)<1e-8);
      }
      if (a.result.transient_surface) assert.notDeepEqual(a.result.transient_surface.result.final.temperature_c,
        b.result.transient_surface.result.final.temperature_c);
      if (a.result.transient_coverage) assert.notDeepEqual(a.result.transient_coverage.result.checkpoint.exposed_temperature_c,
        b.result.transient_coverage.result.checkpoint.exposed_temperature_c);
      assert.equal((await w.send(msg)).files[0].text, result.files[0].text);
      assert.ok(w.sent.some(m => m.type === 'progress' && m.request === 93 && m.text.includes('motion endpoint')));
      // Exhaust only the aggregate motion allowance; every independent static result survives.
      const limited = JSON.parse(msg.scenarioRequest); limited.max_motion_trials = 1;
      const exhausted = await w.send({ ...msg, request: 94, scenarioRequest: JSON.stringify(limited) });
      assert.equal(exhausted.type, 'result', exhausted.message); assert.equal(exhausted.request, 94);
      assert.deepEqual(exhausted.result.manufacturing, result.result.manufacturing);
      assert.equal(exhausted.result.simulations.length, result.result.simulations.length);
      exhausted.result.simulations.forEach((row, i) => {
        assert.equal(row.evaluation.status, 'available');
        const actual = row.evaluation.result, original = result.result.simulations[i].evaluation.result;
        assert.equal(actual.motion.status, 'refused'); assert.equal(actual.motion.result, undefined);
        for (const key of ['drape', 'surface', 'coverage', 'positions_m', 'positions_sha256', 'metrics']) {
          assert.deepEqual(actual[key], original[key], `Motion allowance changed independent ${key}`);
        }
      });
      // Reuse the same unlocked synthetic source and body: reverse only the existing steady
      // temperatures, with independent static drapes and no new measurement/physics fixture.
      const matrixStudy=JSON.parse(msg.studyRequest), forward=JSON.parse(msg.scenarioRequest);
      delete forward.motion;
      matrixStudy.comparison={basis:'nominal',objectives:[{metric:'independent_surface_power_w',direction:'minimize'}]};
      const reverse=plain(forward);
      for (const boundary of reverse.surface.boundaries) {
        [boundary.inside_c,boundary.outside_c]=[boundary.outside_c,boundary.inside_c];
      }
      const cases=[{key:'forward',scenario:forward},{key:'reverse',scenario:reverse,comparison:{
        ...matrixStudy.comparison,constraints:[{metric:'independent_surface_power_w',maximum:0}]}}];
      const matrixRequest=` ${JSON.stringify({schema:'ftx-drape-scenario-matrix/1',max_evaluations:4,cases})}\n`;
      const matrixStudyText=JSON.stringify(matrixStudy);
      const matrixMessage={...msg,request:96,studyRequest:matrixStudyText,scenarioRequest:matrixRequest};
      const matrix=await w.send(matrixMessage);
      assert.equal(matrix.type,'result',matrix.message);
      assert.equal(matrix.result.schema,'ftx-material-scenario-matrix-study/1');
      assert.deepEqual(matrix.result.cases.map(c=>c.key),['forward','reverse']);
      assert.equal(matrix.result.planned_evaluations,4); assert.equal(matrix.result.evaluations,4);
      assert.equal(matrix.files.find(f=>f.name==='scenario-request.json').text,matrixRequest);
      assert.equal(matrix.files.find(f=>f.name==='study-request.json').text,matrixStudyText);
      assert.equal(matrix.files.find(f=>f.name==='body-input.json').text,msg.measurements);
      const model=new ftx.WebBody(new Uint8Array(msg.bytes)), priorProgress=globalThis.ftxProgress;
      globalThis.ftxProgress=()=>{};
      try {
        const raw=ftx.material_scenario_study(model,msg.pattern,matrixStudyText,matrixRequest,msg.measurements);
        assert.equal(matrix.files.find(f=>f.name==='material-scenario-matrix-study.json').text,raw);
        assert.deepEqual(matrix.result,JSON.parse(raw));
        for (const [i,item] of cases.entries()) {
          const independent=JSON.parse(ftx.material_scenario_study(model,msg.pattern,
            JSON.stringify({...matrixStudy,comparison:item.comparison||matrixStudy.comparison}),
            JSON.stringify(item.scenario),msg.measurements));
          const actual=matrix.result.cases[i];
          assert.equal(actual.request_sha256,independent.request_sha256);
          assert.deepEqual(actual.scenario,independent.scenario);
          assert.deepEqual(actual.simulations,independent.simulations);
          assert.deepEqual(actual.comparison,independent.comparison);
          assert.deepEqual(matrix.result.manufacturing.alternatives,independent.manufacturing.alternatives);
          for (const candidate of actual.comparison.alternatives) {
            const evaluated=actual.simulations.find(row=>row.key===candidate.key).evaluation;
            assert.equal(evaluated.status,'available'); assert.equal(candidate.eligible,true);
            assert.equal(candidate.readings.independent_surface_power_w.value,evaluated.result.metrics.independent_surface_power_w);
            assert.deepEqual(matrix.result.comparison.alternatives.find(row=>row.key===candidate.key).cases[item.key],candidate);
          }
        }
      } finally {
        model.free();
        if (priorProgress===undefined) delete globalThis.ftxProgress; else globalThis.ftxProgress=priorProgress;
      }
      const forwardResult=matrix.result.cases[0].simulations[0].evaluation.result;
      const reverseResult=matrix.result.cases[1].simulations[0].evaluation.result;
      assert.equal(forwardResult.positions_sha256,reverseResult.positions_sha256);
      assert.ok(forwardResult.metrics.independent_surface_power_w>0);
      assert.ok(reverseResult.metrics.independent_surface_power_w<0);
      assert.notDeepEqual(matrix.result.cases[0].comparison.frontier,matrix.result.cases[1].comparison.frontier);
      assert.equal(matrix.result.comparison.frontier.length,2);
      assert.equal((await w.send(matrixMessage)).files[0].text,matrix.files[0].text);
    }
    // Separate native fixture keeps the original uncoupled motion replay intact. All material
    // observations and body geometry here are synthetic software inputs, not measured textiles.
    if (process.env.FTX_PLASTIC_HEAT_REPLAY) {
      const dir = process.env.FTX_PLASTIC_HEAT_REPLAY;
      const input = name => fs.readFileSync(path.join(dir, name), 'utf8');
      const bytes = fs.readFileSync(path.join(dir, 'model.ftxbody'));
      const msg = { type: 'scenario', request: 95, pattern: input('pattern.json'), studyRequest: input('study.json'),
        scenarioRequest: input('scenario.json'), measurements: input('body-input.json'),
        bytes: bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) };
      const replay = await w.send(msg);
      assert.equal(replay.type, 'result', replay.message);
      assert.deepEqual(replay.result, JSON.parse(input('motion-report.json')));
      assert.equal(replay.files.find(f => f.name === 'scenario-request.json').text, msg.scenarioRequest);
      assert.equal(replay.files.find(f => f.name === 'body-input.json').text, msg.measurements);
      assert.equal(replay.result.simulations.length, 2);
      assert.deepEqual(replay.result.comparison.frontier, ['unconverted']);
      assert.deepEqual(replay.result.comparison.incomparable, []);
      assert.ok(replay.result.comparison.alternatives.every(row => row.eligible));
      const motions = new Map(replay.result.simulations.map(row => {
        assert.equal(row.evaluation.status, 'available');
        const result = row.evaluation.result;
        assert.equal(result.drape.settled, true); assert.deepEqual(result.drape.gating, []);
        assert.equal(result.metrics.simulation_gating_clear, true);
        assert.equal(result.motion.status, 'available');
        const motion = result.motion.result;
        assert.equal(motion.transient_surface.status, 'available');
        const heat = motion.transient_surface.result, work = heat.plastic_dissipation_heat;
        assert.equal(work.distribution, 'uniform_reference_thickness');
        assert.equal(work.temporal_distribution, 'uniform_over_accepted_interval');
        assert.ok(work.initial_plastic_dissipation_j > 0 && work.plastic_dissipation_j > 0);
        assert.ok(Math.abs(work.initial_plastic_dissipation_j -
          result.drape.membrane_plasticity_history.plastic_dissipation_j) < 1e-12);
        assert.ok(Math.abs(work.initial_plastic_dissipation_j + work.plastic_dissipation_j -
          motion.membrane_plasticity_history.plastic_dissipation_j) < 1e-12);
        assert.ok(['unconverted', 'converted'].includes(row.key));
        const fraction = row.key === 'unconverted' ? 0 : 0.8;
        assert.equal(work.material_fractions[0].fraction, fraction);
        assert.ok(Math.abs(work.supplied_heat_j - fraction * work.plastic_dissipation_j) < 1e-12);
        assert.ok(Math.abs(work.supplied_heat_j + work.unconverted_dissipation_j - work.plastic_dissipation_j) < 1e-12);
        const balance = heat.stored_change_j - heat.net_boundary_energy_j - work.supplied_heat_j;
        assert.ok(Math.abs(balance) < 1e-9);
        assert.ok(Math.abs(balance - heat.balance_error_j) < 1e-12);
        if (fraction === 0) {
          assert.equal(work.supplied_heat_j, 0);
          assert.ok(heat.final.temperature_c.flat().every(t => Math.abs(t - 20) < 1e-10));
        } else assert.ok(work.supplied_heat_j > 0);
        assert.equal(heat.trace.length, motion.samples.length);
        assert.equal(motion.transient_coverage.status, 'available');
        const coverage = motion.transient_coverage.result;
        assert.deepEqual(coverage.plastic_dissipation_heat, work);
        assert.equal(coverage.trace.length, motion.samples.length);
        for (const value of [coverage, ...coverage.trace]) {
          const residual = value.physical_stored_change_j - value.net_boundary_energy_j -
            value.plastic_dissipation_heat.supplied_heat_j;
          assert.ok(Math.abs(residual) < 1e-9);
          assert.ok(Math.abs(residual - value.balance_error_j) < 1e-12);
        }
        return [row.key, motion];
      }));
      const cold = motions.get('unconverted'), warm = motions.get('converted');
      assert.deepEqual(cold.samples, warm.samples);
      assert.notDeepEqual(cold.transient_surface.result.final.temperature_c,
        warm.transient_surface.result.final.temperature_c);
      assert.notDeepEqual(cold.transient_coverage.result.checkpoint.exposed_temperature_c,
        warm.transient_coverage.result.checkpoint.exposed_temperature_c);
    }
  });

test('explicit body sources replace only scenario evidence and retain exact replay text', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  const raw = '{"schema":"ftx-scenario-body/1","arm_abduction_deg":50.0,"accepted_body_fit":{"test":-0.0}}';
  p.select('scenarioBody', raw); p.e.measurements.files = [];
  p.e.scenarioBody.handlers.change(); assert.equal(p.e.scenario.disabled, false);
  await p.e.scenario.onclick();
  assert.equal(p.current().messages.at(-1).measurements, raw);
  p.emit(scenarioReport()); assert.equal(p.live.size, 2);
  p.select('measurements', '{"wearer":{"stature_mm":1000.0}}'); p.e.measurements.handlers.change();
  assert.equal(p.live.size, 2); // overridden tapes are not a dependency of this result
  p.e.scenarioBody.handlers.change(); assert.equal(p.live.size, 0);
  p.e.scenarioBody.files = []; p.e.scenarioBody.handlers.change();
  await p.e.scenario.onclick();
  assert.equal(p.current().messages.at(-1).measurements, '{"wearer":{"stature_mm":1000.0}}');
  const w = await workerModule();
  const r = await w.send({type:'scenario',request:55,bytes:new ArrayBuffer(8),
    pattern:'{}',studyRequest:'{}',scenarioRequest:'{}',measurements:raw});
  const artifact = r.files.find(f => f.name === 'body-input.json');
  assert.equal(artifact.text, raw); assert.equal(w.called.find(c => c[0] === 'scenario').at(-1), raw);
});

test('invalid or superseded body-source files cannot fall back to tapes or publish stale outcomes', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  p.select('scenarioBody', '{malformed'); p.e.scenarioBody.handlers.change();
  const oldCount = p.current().messages.length;
  await p.e.scenario.onclick();
  assert.equal(p.current().messages.length, oldCount); assert.equal(p.e.error.hidden, false);
  assert.equal(p.live.size, 0);
  const d = deferred(); p.select('scenarioBody', '{}'); p.e.scenarioBody.files[0].text = () => d.promise;
  const old = p.current(), running = p.e.scenario.onclick();
  p.select('scenarioBody', '{"schema":"ftx-scenario-body/1"}'); p.e.scenarioBody.handlers.change();
  p.ready(); d.resolve('{"schema":"ftx-scenario-body/1","arm_abduction_deg":35.0}'); await running;
  assert.equal(old.messages.length, oldCount);
  await p.e.scenario.onclick(); const stale = p.current(), id = stale.messages.at(-1).request;
  p.e.body.handlers.change(); p.ready();
  stale.emit({type:'result',request:id,result:scenarioReport(),files:[]});
  assert.equal(p.live.size, 0); assert.equal(section(p, 'Fresh drape and heat alternatives').hidden, true);
});

test('transient heat displays signed energy and remap separately, preserving refusal and invalidation', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  const r = scenarioReport();
  r.scenario = { scenario: { motion: { transient_surface: {}, transient_coverage: {} } } };
  const motion = { duration_s: 1, samples: [{}], trials: 1, keyframes: [], metrics: {},
    transient_surface: outcome({ duration_s: 1, thermal_steps: 2, trace: [{temperature_range_c:[-5,0]}],
      net_boundary_energy_j: -3, stored_change_j: -3, balance_error_j: 0 }),
    transient_coverage: outcome({ duration_s: 1, thermal_steps: 4, trace: [{temperature_range_c:[0,5]}],
      net_boundary_energy_j: 4, physical_stored_change_j: 4, balance_error_j: 0, signed_remap_residual_j: -1e-12,
      checkpoint: { source: '<synthetic locked source>' } }) };
  r.simulations = [{key:'A',evaluation:outcome({drape:{settled:false},motion:outcome(motion)})},
    {key:'B',evaluation:outcome({drape:{settled:true},motion:refused('motion exhausted')})}];
  await p.e.scenario.onclick(); p.emit(r);
  const s = section(p, 'Fresh drape and heat alternatives');
  const t = descendants(s).find(n => n.tag === 'table' && n.children[0].textContent === 'A: transient heat — complete time history or refusal');
  assert.deepEqual(plain(t.children[2].children.map(n => n.textContent)),
    ['Independent sheets','Available','1','2','[-5, 0]','-3','-3','0','Not applicable']);
  assert.deepEqual(plain(t.children[3].children.map(n => n.textContent)),
    ['Body coverage + exposed cloth','Available','1','4','[0, 5]','4','4','0','-1e-12']);
  const failed = descendants(s).find(n => n.tag === 'table' && n.children[0].textContent.startsWith('B: transient heat'));
  assert.match(visibleText(failed), /Unavailable: Refused: motion exhausted/);
  assert.equal(failed.children[2].children[5].textContent, 'Unknown');
  assert.match(visibleText(s), /Do not add their energy totals/);
  const d = descendants(s).find(n => n.tag === 'details' && n.children[0].textContent.startsWith('A: Body coverage + exposed cloth temperature'));
  d.open=true; d.handlers.toggle(); assert.match(visibleText(d), /<synthetic locked source>/);
  motion.transient_coverage = refused('missing heat capacity');
  await p.e.scenario.onclick(); p.emit(r);
  assert.match(visibleText(s), /Refused: missing heat capacity/);
  assert.match(visibleText(s), /Independent sheets/);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
});

test('material-direction strain preserves signed axes, regional witnesses and independent motion refusal', async () => {
  const p=await pageModule(); await p.load(); scenarioInputs(p);
  const report=scenarioReport();
  report.scenario.scenario={material_strain:{schema:'ftx-material-strain/1',instances:['<panel>'],max_triangle_samples:100},motion:{}};
  const extrema={green_strain_range:[[-0.02,0.05],[-0.03,0.02],[-0.04,0.06]],
    minimum_witness:[0,1,2].map(i=>({source_triangle:i,sample_index:0,time_s:0})),
    maximum_witness:[0,1,2].map(i=>({source_triangle:i+3,sample_index:2,time_s:0.5}))};
  const strain={...extrema,triangle_samples:18,duration_s:0.5,instances:{'<panel>':extrema},
    source:{instances:['<panel>'],materials:{source:'<retained source>'}}};
  const initial=report.simulations.find(row=>row.key==='A').evaluation.result;
  initial.material_strain=outcome({...strain,duration_s:0,triangle_samples:6});
  initial.motion=outcome({samples:[{}],keyframes:[],material_strain:outcome(strain)});
  report.comparison={policy:{basis:'nominal',allow_mixed_protocols:false,required_consumers:['motion_material_strain'],
    objectives:[{metric:'motion_min_engineering_shear_green_strain',direction:'maximize'}],
    constraints:[{metric:'max_warp_green_strain',maximum:0.05}]},frontier:['A'],motion_required:true,
    uncertainty_propagated:false,incomparable:[],alternatives:[{key:'A',eligible:true,dominated_by:[],exclusions:[],
      readings:{motion_min_engineering_shear_green_strain:{value:-0.04,unit:'1'},max_warp_green_strain:{value:0.05,unit:'1'}}}]};
  await p.e.scenario.onclick(); p.emit(report);
  const s=section(p,'Fresh drape and heat alternatives');
  const table=name=>descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent===name);
  const moving=table('A: accepted-motion material-direction strain');
  assert.deepEqual(plain(moving.children[4].children.map(n=>n.textContent)),
    ['Selected scope','Engineering shear 2E12','-0.04','Triangle 2, sample 0 at 0 s','0.06','Triangle 5, sample 2 at 0.5 s']);
  assert.equal(moving.children[5].children[0].textContent,'<panel>');
  assert.match(visibleText(s),/Motion minimum shear 2E12/);
  assert.match(visibleText(s),/Initial maximum warp E11/);
  assert.match(visibleText(s),/not engineering extension percent/);
  assert.match(visibleText(s),/Inclusive constraints are declared design requirements/);
  const source=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent==='A: accepted-motion strain scope, sources and sampled witnesses (display only)');
  source.open=true; source.handlers.toggle(); assert.match(visibleText(source),/<retained source>/);
  assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  initial.motion.result.material_strain=refused('material-strain triangle-sample budget exhausted');
  await p.e.scenario.onclick(); p.emit(report);
  assert.equal(table('A: accepted-motion material-direction strain'),undefined);
  assert.ok(table('A: initial material-direction strain'));
  assert.match(visibleText(s),/material-strain triangle-sample budget exhausted/);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
});

test('body-contact loads retain signed vectors, force witnesses and independent motion refusal', async () => {
  const p=await pageModule(); await p.load(); scenarioInputs(p);
  const report=scenarioReport();
  report.scenario.scenario={contact_loads:{schema:'ftx-contact-loads/1',instances:['<cuff>'],max_vertex_samples:100,max_pair_evaluations:1000},motion:{}};
  const frames=[{sample_index:0,time_s:0,body:{resultant_n:[-3,0,4],resultant_magnitude_n:5,max_vertex_force_n:4,max_force_vertex:7}},
    {sample_index:1,time_s:0.5,body:{resultant_n:[0,-8,6],resultant_magnitude_n:10,max_vertex_force_n:6,max_force_vertex:9}}];
  const loads={schema:'ftx-contact-loads-report/1',source:{instances:{'<cuff>':'<material digest>'}},
    frames,peak_body_resultant_n:10,peak_body_resultant_sample:1,max_body_vertex_force_n:6,max_body_vertex_force_sample:1,
    vertex_samples:24,pair_evaluations:32,duration_s:0.5};
  const initial=report.simulations.find(row=>row.key==='A').evaluation.result;
  initial.contact_loads=outcome({...loads,frames:frames.slice(0,1),peak_body_resultant_n:5,peak_body_resultant_sample:0,
    max_body_vertex_force_n:4,max_body_vertex_force_sample:0,duration_s:0});
  initial.motion=outcome({samples:[{}],keyframes:[],contact_loads:outcome(loads)});
  report.comparison={policy:{basis:'nominal',allow_mixed_protocols:false,required_consumers:['motion_contact_loads'],
    objectives:[{metric:'motion_peak_body_contact_resultant_n',direction:'minimize'}],
    constraints:[{metric:'max_body_contact_vertex_force_n',maximum:5}]},frontier:['A'],motion_required:true,
    uncertainty_propagated:false,incomparable:[],alternatives:[{key:'A',eligible:true,dominated_by:[],exclusions:[],
      readings:{motion_peak_body_contact_resultant_n:{value:10,unit:'N'},max_body_contact_vertex_force_n:{value:4,unit:'N'}}}]};
  await p.e.scenario.onclick(); p.emit(report);
  const s=section(p,'Fresh drape and heat alternatives');
  const table=name=>descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent===name);
  assert.deepEqual(plain(table('A: accepted-motion body-contact loads').children[2].children.map(n=>n.textContent)),
    ['10','Sample 1 at 0.5 s','6','Sample 1 at 0.5 s']);
  assert.deepEqual(plain(table('A: accepted-motion sampled body-contact forces').children[3].children.map(n=>n.textContent)),
    ['1','0.5','0','-8','6','10','6','9']);
  assert.match(visibleText(s),/Motion peak body-contact resultant/);
  assert.match(visibleText(s),/Nodal maxima depend on mesh resolution/);
  assert.match(visibleText(s),/Selected garment instances: <cuff>/);
  assert.match(visibleText(s),/exclude tangential friction/);
  const source=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent==='A: accepted-motion contact scope, materials and sampled force witnesses (display only)');
  source.open=true; source.handlers.toggle(); assert.match(visibleText(source),/<material digest>/);
  assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  initial.motion.result.contact_loads=refused('contact-load pair budget exhausted');
  await p.e.scenario.onclick(); p.emit(report);
  assert.equal(table('A: accepted-motion body-contact loads'),undefined);
  assert.ok(table('A: initial body-contact loads'));
  assert.match(visibleText(s),/contact-load pair budget exhausted/);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
});

test('textile water cooling exposes separate conserved mass and the full ledger required by comparison admission', async () => {
  const p=await pageModule(); await p.load(); scenarioInputs(p);
  const report=scenarioReport(); report.scenario.scenario={motion:{transient_surface:{surface_water:{schema:'ftx-textile-surface-water/1'}}}};
  const water={initial_liquid_kg:0.005,evaporated_liquid_kg:0.003,remaining_liquid_kg:0.002,
    mass_balance_error_kg:0,dryout_events:2,latent_energy_j:7500,
    exported_vapor_sensible_energy_j:252,liquid_stored_change_j:-254,balance_error_j:0,
    request:{source:{reference:'<independent free-liquid acquisition>'}},
    patches:[{instance:'<wet panel>',remaining_liquid_kg:0},{instance:'dry panel',remaining_liquid_kg:0}],
    unit_pressure_networks:[{source:'<canonical Ret>'}]};
  const heat={duration_s:1,thermal_steps:12,trace:[{temperature_range_c:[15,20],surface_water:water}],
    net_boundary_energy_j:0,stored_change_j:-7498,balance_error_j:0,surface_water:water};
  const motion={samples:[{}],keyframes:[],transient_surface:outcome(heat)};
  report.simulations.find(row=>row.key==='A').evaluation.result.motion=outcome(motion);
  await p.e.scenario.onclick(); p.emit(report);
  const s=section(p,'Fresh drape and heat alternatives');
  const table=name=>descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent===name);
  assert.deepEqual(plain(table('A: textile surface liquid — conserved initial inventory').children[2].children.map(n=>n.textContent)),
    ['0.005','0.003','0.002','0','2']);
  assert.deepEqual(plain(table('A: textile evaporation and sensible energy').children[2].children.map(n=>n.textContent)),
    ['7500','252','-254','0']);
  assert.match(visibleText(s),/temperature feeds the declared vapor-pressure table/);
  assert.match(visibleText(s),/Comparison admission checks the complete liquid and vapor ledger/);
  const source=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent==='A: textile liquid source, exact patches and canonical vapor paths (display only)');
  source.open=true; source.handlers.toggle(); assert.match(visibleText(source),/<independent free-liquid acquisition>/);
  motion.transient_surface=refused('source pressure table exceeded');
  await p.e.scenario.onclick(); p.emit(report);
  assert.equal(table('A: textile evaporation and sensible energy'),undefined);
  assert.match(visibleText(s),/source pressure table exceeded/);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
});

test('supplied textile water displays delivery and incoming enthalpy separately through rewetting and refusals', async () => {
  const p=await pageModule(); await p.load(); scenarioInputs(p);
  const report=scenarioReport(); report.scenario.scenario={motion:{transient_surface:{surface_water:{schema:'ftx-textile-surface-water/2'}}}};
  const water={schema:'ftx-textile-surface-water-result/2',initial_liquid_kg:0,supplied_liquid_kg:0.005,
    evaporated_liquid_kg:0.003,remaining_liquid_kg:0.002,mass_balance_error_kg:0,dryout_events:1,
    latent_energy_j:7500,exported_vapor_sensible_energy_j:-252,liquid_stored_change_j:-254,
    supplied_liquid_sensible_energy_j:-420,balance_error_j:0,
    request:{supply:{source:{reference:'<independent incoming liquid>'}}}};
  const motion={samples:[{}],keyframes:[],transient_surface:outcome({duration_s:1,thermal_steps:12,
    trace:[{temperature_range_c:[15,20]}],surface_water:water})};
  report.simulations.find(row=>row.key==='A').evaluation.result.motion=outcome(motion);
  await p.e.scenario.onclick(); p.emit(report);
  const s=section(p,'Fresh drape and heat alternatives');
  const table=name=>descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent===name);
  assert.deepEqual(plain(table('A: textile surface liquid — conserved initial and supplied inventory').children[2].children.map(n=>n.textContent)),
    ['0','0.005','0.003','0.002','0','1']);
  assert.deepEqual(plain(table('A: textile evaporation and sensible energy').children[2].children.map(n=>n.textContent)),
    ['7500','-252','-254','-420','0']);
  assert.match(visibleText(s),/can support evaporation on a dry patch and can rewet it/);
  assert.match(visibleText(s),/Loading beyond the qualified cap refuses the interval/);
  const source=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent==='A: textile liquid source, exact patches and canonical vapor paths (display only)');
  source.open=true; source.handlers.toggle(); assert.match(visibleText(source),/<independent incoming liquid>/);
  assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  motion.transient_surface=refused('incoming liquid lacks independent evidence');
  await p.e.scenario.onclick(); p.emit(report);
  assert.equal(table('A: textile evaporation and sensible energy'),undefined);
  assert.match(visibleText(s),/incoming liquid lacks independent evidence/);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
});

test('condensation retains incoming mass and warming separately from later evaporation and withdraws refused histories', async () => {
  const p=await pageModule(); await p.load(); scenarioInputs(p);
  const report=scenarioReport(); report.scenario.scenario={motion:{transient_surface:{surface_water:{schema:'ftx-textile-surface-water/3'}}}};
  const water={schema:'ftx-textile-surface-water-result/3',initial_liquid_kg:0,supplied_liquid_kg:0.001,
    condensed_liquid_kg:0.004,evaporated_liquid_kg:0.003,remaining_liquid_kg:0.002,
    mass_balance_error_kg:0,dryout_events:0,latent_energy_j:7500,condensation_latent_energy_j:10000,
    exported_vapor_sensible_energy_j:252,liquid_stored_change_j:168,supplied_liquid_sensible_energy_j:84,
    incoming_vapor_sensible_energy_j:336,balance_error_j:0,
    request:{condensation:{source:{reference:'<independent reverse-transfer evidence>'}}}};
  const motion={samples:[{}],keyframes:[],transient_surface:outcome({duration_s:1,thermal_steps:12,
    trace:[{temperature_range_c:[15,20]}],surface_water:water})};
  report.simulations.find(row=>row.key==='A').evaluation.result.motion=outcome(motion);
  await p.e.scenario.onclick(); p.emit(report);
  const s=section(p,'Fresh drape and heat alternatives');
  const table=name=>descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent===name);
  assert.deepEqual(plain(table('A: textile surface liquid — conserved initial, supplied and condensed inventory').children[2].children.map(n=>n.textContent)),
    ['0','0.001','0.004','0.003','0.002','0','0']);
  assert.deepEqual(plain(table('A: textile evaporation and sensible energy').children[2].children.map(n=>n.textContent)),
    ['7500','252','168','84','10000','336','0']);
  assert.match(visibleText(s),/condense on dry or wet cloth and later re-evaporate/);
  const source=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent==='A: textile liquid source, exact patches and canonical vapor paths (display only)');
  source.open=true; source.handlers.toggle(); assert.match(visibleText(source),/<independent reverse-transfer evidence>/);
  motion.transient_surface=refused('condensation exceeds qualified loading');
  await p.e.scenario.onclick(); p.emit(report);
  assert.equal(table('A: textile evaporation and sensible energy'),undefined);
  assert.match(visibleText(s),/condensation exceeds qualified loading/);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
});

test('plastic and relaxation heat display each domain source separately and preserve independent refusals', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  const report = scenarioReport(); report.scenario.scenario = { motion: { transient_surface: {}, transient_coverage: {} } };
  const surface = { duration_s: 1, thermal_steps: 2, trace: [{temperature_range_c:[20,22]}],
    net_boundary_energy_j: -3, stored_change_j: 6, balance_error_j: 0 };
  const coverage = { net_boundary_energy_j: 4, physical_stored_change_j: 4, balance_error_j: 0 };
  const motion = { samples: [{}], keyframes: [], transient_surface: outcome(surface),
    transient_coverage: outcome(coverage) };
  report.simulations.find(row => row.key === 'A').evaluation.result.motion = outcome(motion);
  const s = section(p, 'Fresh drape and heat alternatives');
  const work = () => descendants(s).find(n => n.tag === 'table' && n.children[0].textContent === 'A: material work supplied to independent-sheet heat');
  const coverageWork = () => descendants(s).find(n => n.tag === 'table' && n.children[0].textContent === 'A: material work supplied to coverage and exposed-cloth heat');
  await p.e.scenario.onclick(); p.emit(report); assert.equal(work(), undefined);
  surface.plastic_dissipation_heat = { distribution: 'uniform_reference_thickness', temporal_distribution: 'uniform_over_accepted_interval',
    plastic_dissipation_j: 12, supplied_heat_j: 9, unconverted_dissipation_j: 3, initial_plastic_dissipation_j: 5,
    material_fractions: [{ source: '<qualified material fraction>', fraction: 0.75 }] };
  await p.e.scenario.onclick(); p.emit(report);
  assert.deepEqual(plain(work().children[2].children.map(n => n.textContent)), ['Plastic yield','12','9','3','5']);
  const heat = descendants(s).find(n => n.tag === 'table' && n.children[0].textContent.startsWith('A: transient heat'));
  assert.deepEqual(plain(heat.children[2].children.slice(5,8).map(n => n.textContent)), ['-3','6','0']);
  assert.deepEqual(plain(heat.children[3].children.slice(5,8).map(n => n.textContent)), ['4','4','0']);
  assert.match(visibleText(s), /stored change minus net boundary energy minus the sum of qualified source heat/);
  assert.match(visibleText(s), /initial drape, reversible storage and numerical loss are excluded/);
  assert.equal(coverageWork(), undefined);
  coverage.plastic_dissipation_heat = {...surface.plastic_dissipation_heat, supplied_heat_j: 6, unconverted_dissipation_j: 6};
  coverage.physical_stored_change_j = 10;
  await p.e.scenario.onclick(); p.emit(report);
  assert.deepEqual(plain(coverageWork().children[2].children.map(n => n.textContent)), ['Plastic yield','12','6','6','5']);
  coverage.relaxation_dissipation_heat = { distribution:'uniform_reference_thickness', temporal_distribution:'uniform_over_accepted_interval',
    viscous_dissipation_j:8,supplied_heat_j:2,unconverted_dissipation_j:6,initial_viscous_dissipation_j:4,
    material_fractions:[{source:'<independent Maxwell fraction>',fraction:0.25}] };
  coverage.physical_stored_change_j = 12;
  await p.e.scenario.onclick(); p.emit(report);
  assert.deepEqual(plain(coverageWork().children[2].children.map(n => n.textContent)), ['Plastic yield','12','6','6','5']);
  assert.deepEqual(plain(coverageWork().children[3].children.map(n => n.textContent)), ['Maxwell dashpots','8','2','6','4']);
  assert.equal(work().children.length, 3); // A coverage source is never duplicated into independent sheets.
  const combinedHeat = descendants(s).find(n => n.tag === 'table' && n.children[0].textContent.startsWith('A: transient heat'));
  assert.deepEqual(plain(combinedHeat.children[3].children.slice(5,8).map(n => n.textContent)), ['4','12','0']);
  assert.match(visibleText(s), /Source heat follows covered and exposed reference material; bare body receives none/);
  const source = descendants(s).find(n => n.tag === 'details' && n.children[0].textContent.startsWith('A: Independent sheets temperature'));
  source.open = true; source.handlers.toggle(); assert.match(visibleText(source), /<qualified material fraction>/);
  assert.equal(descendants(s).some(n => 'innerHTML' in n), false);
  surface.plastic_dissipation_heat.unconverted_dissipation_j = null;
  await p.e.scenario.onclick(); p.emit(report); assert.equal(work().children[2].children[3].textContent, 'Unknown');
  motion.transient_surface = refused('missing source-qualified plastic heat fraction');
  await p.e.scenario.onclick(); p.emit(report); assert.equal(work(), undefined);
  assert.deepEqual(plain(coverageWork().children[2].children.map(n => n.textContent)), ['Plastic yield','12','6','6','5']);
  assert.deepEqual(plain(coverageWork().children[3].children.map(n => n.textContent)), ['Maxwell dashpots','8','2','6','4']);
  assert.match(visibleText(s), /Refused: missing source-qualified plastic heat fraction/);
  assert.match(visibleText(s), /Body coverage \+ exposed cloth/);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden, true); assert.equal(p.live.size, 0);
});

test('bending heat authored domain opt-ins reach the worker and replay without rewriting another source policy', async () => {
  for (const domain of ['transient_surface','transient_coverage']) {
    const other = domain === 'transient_surface' ? 'transient_coverage' : 'transient_surface';
    const scenario = ` {"schema":"ftx-drape-scenario/1","motion":{"${domain}":{"bending_plastic_dissipation_heat":"uniform_reference_thickness","initial_temperature_c":20.0},"${other}":{"plastic_dissipation_heat":"uniform_reference_thickness","relaxation_dissipation_heat":"uniform_reference_thickness","origin":-0.0}}}\n`;
    const p = await pageModule(); await p.load(); scenarioInputs(p);
    p.select('scenarioRequest',scenario); p.e.scenarioRequest.handlers.change(); await p.e.scenario.onclick();
    const stale = p.current(), msg = stale.messages.at(-1);
    assert.equal(msg.scenarioRequest,scenario);
    const w = await workerModule(), result = await w.send(msg);
    assert.equal(result.type,'result',result.message);
    assert.equal(w.called.find(row=>row[0]==='scenario')[4],scenario);
    assert.equal(result.files.find(file=>file.name==='scenario-request.json').text,scenario);
    assert.deepEqual(w.freed,['body']);
    const policy = JSON.parse(scenario).motion;
    assert.equal(policy[other].bending_plastic_dissipation_heat,undefined);
    assert.equal(policy[domain].plastic_dissipation_heat,undefined);
    assert.equal(policy[domain].relaxation_dissipation_heat,undefined);
    p.e.scenarioRequest.handlers.change(); p.ready();
    stale.emit({type:'result',request:msg.request,result:scenarioReport(),files:[]});
    assert.equal(section(p,'Fresh drape and heat alternatives').hidden,true);
  }
});

test('bending heat displays independent source receipts, zero conversion and domain refusals without stale results', async () => {
  const p=await pageModule(); await p.load(); scenarioInputs(p);
  const report=scenarioReport(); report.scenario.scenario={motion:{transient_surface:{},transient_coverage:{}}};
  const surface={duration_s:1,thermal_steps:2,trace:[{temperature_range_c:[20,20]}],
    net_boundary_energy_j:0,stored_change_j:0,balance_error_j:0};
  const source={distribution:'uniform_reference_thickness',temporal_distribution:'uniform_over_accepted_interval',
    plastic_dissipation_j:12,supplied_heat_j:0,unconverted_dissipation_j:12,initial_plastic_dissipation_j:5,
    material_fractions:[{material_sha256:'qualified-bending-material',law_sha256:'qualified-curvature-law',
      fraction:0,temperature_range_c:[0,80],observation:{method:'<explicit zero conversion>',
        conditions:{work_basis:'irreversible_bending_plastic_dissipation'}}}]};
  const coverage={duration_s:1,thermal_steps:2,trace:[{temperature_range_c:[20,20]}],
    net_boundary_energy_j:0,physical_stored_change_j:0,balance_error_j:0,signed_remap_residual_j:0,
    bending_plastic_dissipation_heat:source};
  const motion={samples:[{}],keyframes:[],transient_surface:outcome(surface),transient_coverage:outcome(coverage)};
  report.simulations.find(row=>row.key==='A').evaluation.result.motion=outcome(motion);
  const s=section(p,'Fresh drape and heat alternatives');
  const work=domain=>descendants(s).find(n=>n.tag==='table' && n.children[0].textContent===`A: material work supplied to ${domain}`);
  await p.e.scenario.onclick(); p.emit(report);
  assert.equal(work('independent-sheet heat'),undefined);
  assert.deepEqual(plain(work('coverage and exposed-cloth heat').children[2].children.map(n=>n.textContent)),['Bending yield','12','0','12','5']);
  assert.match(visibleText(s),/Bending work uses each source triangle's reference area/);
  const receipt=descendants(s).find(n=>n.tag==='details' && n.children[0].textContent==='A: Bending yield conversion observations for coverage and exposed-cloth heat (display only)');
  receipt.open=true; receipt.handlers.toggle();
  assert.match(visibleText(receipt),/<explicit zero conversion>/);
  assert.match(visibleText(receipt),/qualified-curvature-law/);
  assert.match(visibleText(receipt),/irreversible_bending_plastic_dissipation/);
  assert.equal(descendants(s).some(n=>'innerHTML' in n),false);

  surface.bending_plastic_dissipation_heat={...source,supplied_heat_j:3,unconverted_dissipation_j:9,
    material_fractions:source.material_fractions.map(row=>({...row,fraction:0.25}))};
  surface.stored_change_j=3;
  await p.e.scenario.onclick(); p.emit(report);
  assert.deepEqual(plain(work('independent-sheet heat').children[2].children.map(n=>n.textContent)),['Bending yield','12','3','9','5']);
  assert.deepEqual(plain(work('coverage and exposed-cloth heat').children[2].children.map(n=>n.textContent)),['Bending yield','12','0','12','5']);
  motion.transient_coverage=refused('missing source-qualified bending heat fraction');
  await p.e.scenario.onclick(); p.emit(report);
  assert.equal(work('coverage and exposed-cloth heat'),undefined);
  assert.ok(work('independent-sheet heat'));
  assert.match(visibleText(s),/Refused: missing source-qualified bending heat fraction/);
  surface.bending_plastic_dissipation_heat.unconverted_dissipation_j=null;
  await p.e.scenario.onclick(); p.emit(report);
  assert.equal(work('independent-sheet heat').children[2].children[3].textContent,'Unknown');
  p.e.scenarioRequest.handlers.change();
  assert.equal(s.hidden,true); assert.equal(p.live.size,0);
});

test('compiled WASM outfit import reaches exact material binding and complete assembly studies',
  { skip: process.env.FTX_REAL_WASM !== '1' }, async () => {
    const pkg = process.env.FTX_WASM_PKG ? path.resolve(process.env.FTX_WASM_PKG) : path.join(__dirname, '../web/pkg');
    const ftx = await import(require('node:url').pathToFileURL(path.join(pkg, 'ftx_wasm.js')).href);
    ftx.initSync({ module: fs.readFileSync(path.join(pkg, 'ftx_wasm_bg.wasm')) });
    const w = await workerModule({ bindings: ftx });
    const request = fs.readFileSync(path.join(__dirname,'../tests/fixtures/materials/synthetic-joint-outfit.json'),'utf8');
    const imported = await w.send({type:'outfit',request:1,outfitRequest:request});
    assert.equal(imported.type,'result',imported.message);
    assert.equal(imported.result.request_json,request);
    const pattern = imported.result.pattern_json;
    assert.deepEqual(JSON.parse(pattern).pieces.map(p=>p.id),['inner::band','outer::band']);
    assert.equal(imported.files.find(f=>f.name==='outfit-pattern.json').text,pattern);
    const outfit = {request_json:request,composition_json:imported.result.composition_json};
    const heavy = JSON.parse(request).garments[0].pattern.fabric.material;
    heavy.record.properties.areal_density.value = 0.4;
    const changes = {schema:'ftx-material-bindings/1',default:heavy,pieces:{'inner::band':heavy,'outer::band':heavy}};
    const bound = await w.send({type:'bind',request:2,pattern,changes:JSON.stringify(changes),outfit});
    assert.equal(bound.type,'result',bound.message);assert.equal(bound.result.geometry_changed,false);
    assert.equal(bound.result.drape_materials_with_self_contact.ready,true);
    assert.equal(bound.files.find(f=>f.name==='outfit-request.json').text,request);
    assert.equal(bound.files.find(f=>f.name==='outfit-pattern.json').text,bound.result.pattern_json);
    const studyRequest = JSON.stringify({schema:'ftx-material-study/1',variants:[
      {key:'light',changes:{schema:'ftx-material-bindings/1'}},{key:'heavy',changes}],
      assembly_mass:{schema:'ftx-assembly-mass/1',inventory_complete:true,seam_allowance_mm:0,extras:[]}});
    const study = await w.send({type:'study',request:3,pattern,studyRequest,outfit});
    assert.equal(study.type,'result',study.message);
    const byKey = Object.fromEntries(study.result.alternatives.map(a=>[a.key,a]));
    assert.equal(byKey.light.assembly_mass.status,'available');
    assert.equal(byKey.heavy.assembly_mass.status,'available');
    assert.equal(byKey.heavy.metrics.assembly_mass_kg,2*byKey.light.metrics.assembly_mass_kg);
    assert.equal(study.files.find(f=>f.name==='outfit-composition.json').text,outfit.composition_json);
    const refused = await w.send({type:'scenario',request:4,pattern,outfit,scenarioRequest:'{"options":{}}'});
    assert.equal(refused.type,'error');assert.match(refused.message,/options.self_contact/);
  });

test('compiled WASM bounded markers retain whole geometry, selected SVG and one finite-stock order through the worker',
  {skip:process.env.FTX_REAL_WASM!=='1'},async()=>{
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const w=await workerModule({bindings:ftx});
    const property=(value,unit)=>({value,unit,source:'test',evidence:'estimate',method:'Synthetic bounded-marker test'});
    const pattern=JSON.stringify({schema:'fabric-pattern/1',units:'mm',meta:{sa_mm:0},
      fabric:{material:{id:'bounded-marker-sheet',allow_estimates:true,record:{id:'bounded-marker-sheet',name:'Synthetic sheet',form:'woven',
        properties:{width:property(0.1,'m'),areal_density:property(0.2,'kg/m2')},
        sources:{test:{reference:'test:bounded-marker-worker',accessed_on:'2026-10-21',note:'Synthetic software data'}}}}},
      pieces:[{id:'front',cut:{qty:1},grain:[[0,0],[0,10]],vertices:{a:[0,0],b:[40,0],c:[40,100],d:[0,100]},
        edges:[{id:'bottom',from:'a',to:'b'},{id:'right',from:'b',to:'c'},{id:'top',from:'c',to:'d'},{id:'left',from:'d',to:'a'}]}],seams:[]});
    const options='{"copies":3,"gap_mm":5,"edge_margin_mm":5,"end_margin_mm":10,"maximum_marker_length_m":0.12}';
    const cut=await w.send({type:'cut',request:51,pattern,options,rollIndex:1});
    assert.equal(cut.type,'result',cut.message);
    assert.equal(cut.files[0].text,ftx.cut_plan(pattern,options));
    assert.equal(cut.result.schema,'ftx-roll-cut-plan/2');
    assert.deepEqual(cut.result.rolls.map(r=>r.length_m),[0.12,0.12]);
    const pieces=cut.result.rolls.flatMap(r=>r.placements.map(p=>[p.instance,p.copy]));
    assert.deepEqual(pieces,[['front',0],['front',1],['front',2]]);
    assert.equal(cut.files[1].name,'roll-1.svg');
    assert.equal(cut.files[1].text,ftx.cut_svg(pattern,options,1));
    assert.match(cut.files[1].text,/data-copy="2"/);
    assert.doesNotMatch(cut.files[1].text,/data-copy="0"/);
    const quote={offer_id:'short-stock',material_id:'bounded-marker-sheet',supplier:'Synthetic supplier',sku:'short-stock',
      price:1,quantity:1,unit:'linear_meter',minimum_quantity:0.2,order_increment:0.05,currency:'USD',region:'US',quality:'test',terms:'test',
      observed_on:'2026-10-21',available:true,available_quantity:0.2,maximum_continuous_quantity:0.12,
      source:'test:bounded-marker-worker',source_sha256:'0'.repeat(64)};
    const quotes=JSON.stringify({schema:'ftx-textile-quotes/1',quotes:[quote,
      {...quote,offer_id:'whole-order',sku:'whole-order',price:10,available_quantity:0.25}]});
    const market=JSON.stringify({currency:'USD',region:'US',quality:'test',terms:'test',as_of:'2026-10-21',max_age_days:1,
      require_observed_quantity:true,require_observed_continuous_length:true});
    const priced=await w.send({type:'price',request:52,pattern,options,quotes,market});
    assert.equal(priced.type,'result',priced.message);
    assert.equal(priced.files[0].text,ftx.cut_price(pattern,options,quotes,market));
    assert.equal(priced.result.schema,'ftx-cut-procurement/2');
    assert.equal(priced.result.least_observed_independent_order_total,2.5);
    assert.equal(priced.result.material_orders.length,1);
    const order=priced.result.material_orders[0];
    assert.deepEqual(order.roll_indices,[0,1]);
    assert.equal(order.required_length_m,0.24);assert.equal(order.required_continuous_length_m,0.12);
    assert.equal(order.least_observed_order.order_quantity,0.25);
    assert.equal(order.least_observed_order.quote.offer_id,'whole-order');
    assert.equal(order.sample.excluded_offers[0].reason,'insufficient_quantity');
    assert.equal(order.sample.excluded_offers[0].required_order_quantity,0.25);
    assert.ok(priced.result.roll_prices.every(r=>r.material_order_index===0&&!('least_observed_order' in r)));
    const studyRequest=JSON.stringify({schema:'ftx-material-study/1',variants:[{key:'unchanged',changes:{schema:'ftx-material-bindings/1'}}],
      cutting:JSON.parse(options),purchasing:{quotes:JSON.parse(quotes),market:JSON.parse(market)}});
    const study=await w.send({type:'study',request:53,pattern,studyRequest});
    assert.equal(study.type,'result',study.message);
    assert.equal(study.files[0].text,ftx.material_study(pattern,studyRequest));
    const row=study.result.alternatives[0];
    assert.equal(row.cutting.status,'available');assert.equal(row.cutting.result.rolls.length,2);
    assert.equal(row.purchasing.status,'available');assert.equal(row.metrics.least_observed_independent_order_total,2.5);
    // Each current source-locked supplier can cover one intact marker, but not the complete order.
    const locked={...quote,material_sha256:cut.result.rolls[0].material.sha256};
    const splitBook={schema:'ftx-textile-quotes/1',quotes:[
      {...locked,offer_id:'supplier-a',supplier:'Synthetic A',sku:'stock-a'},
      {...locked,offer_id:'supplier-b',supplier:'Synthetic B',sku:'stock-b',price:2}]};
    const splitMarket={...JSON.parse(market),
      split_orders:{schema:'ftx-cut-order-allocation/1',max_search_nodes:1000}};
    const splitQuotes=JSON.stringify(splitBook), splitPolicy=JSON.stringify(splitMarket);
    const split=await w.send({type:'price',request:55,pattern,options,quotes:splitQuotes,market:splitPolicy});
    assert.equal(split.type,'result',split.message);
    assert.equal(split.files[0].text,ftx.cut_price(pattern,options,splitQuotes,splitPolicy));
    assert.equal(split.result.schema,'ftx-cut-procurement/3');
    assert.deepEqual(split.result.cut_plan,cut.result);
    assert.equal(split.result.complete_offer_coverage,true);
    assert.equal(split.result.complete_observed_quantity_coverage,true);
    assert.equal(split.result.complete_continuous_length_limit_coverage,true);
    assert.equal(split.result.search_complete,true);assert.equal(split.result.budget_exhausted,false);
    assert.equal(split.result.order_allocation_optimality_proven,true);
    assert.ok(Math.abs(split.result.selected_observed_order_total-0.6)<1e-12);
    assert.equal(split.result.least_observed_independent_order_total,split.result.selected_observed_order_total);
    const allocation=split.result.material_orders[0].allocation, orders=allocation.selected_orders;
    assert.deepEqual(allocation.whole_order_sample.offers,[]);
    assert.equal(orders.length,2);
    assert.deepEqual(orders.flatMap(o=>o.roll_indices).sort((a,b)=>a-b),[0,1]);
    assert.deepEqual(orders.map(o=>o.offer.quote.offer_id),['supplier-a','supplier-b']);
    for(const selected of orders) {
      assert.equal(selected.offer.quote.material_sha256,locked.material_sha256);
      assert.equal(selected.offer.order_quantity,0.2);
      assert.equal(selected.required_continuous_length_m,0.12);
      assert.equal(selected.offer.quantity_sufficiency_observed,true);
      assert.equal(selected.offer.within_observed_continuous_limit,true);
    }
    // One canonical evaluation can retain a whole-order fallback without claiming a minimum.
    const fallbackBook=JSON.stringify({...splitBook,quotes:[...splitBook.quotes,
      {...locked,offer_id:'whole-fallback',supplier:'Synthetic whole',sku:'whole',price:10,available_quantity:0.25}]});
    const boundedMarket=JSON.stringify({...splitMarket,split_orders:{...splitMarket.split_orders,max_search_nodes:1}});
    const bounded=await w.send({type:'price',request:56,pattern,options,quotes:fallbackBook,market:boundedMarket});
    assert.equal(bounded.type,'result',bounded.message);
    assert.equal(bounded.files[0].text,ftx.cut_price(pattern,options,fallbackBook,boundedMarket));
    assert.equal(bounded.result.complete_offer_coverage,true);
    assert.equal(bounded.result.selected_observed_order_total,2.5);
    assert.equal(bounded.result.least_observed_independent_order_total,null);
    assert.equal(bounded.result.order_allocation_optimality_proven,false);
    assert.equal(bounded.result.search_complete,false);assert.equal(bounded.result.budget_exhausted,true);
    assert.equal(bounded.result.search_nodes+bounded.result.quote_evaluations,1);
    const fallbackOrders=bounded.result.material_orders[0].allocation.selected_orders;
    assert.equal(fallbackOrders.length,1);assert.deepEqual(fallbackOrders[0].roll_indices,[0,1]);
    assert.equal(fallbackOrders[0].offer.quote.offer_id,'whole-fallback');
    // Losing observed capacity removes both complete totals, never substituting a partial subtotal.
    const depletedBook=JSON.stringify({...splitBook,quotes:splitBook.quotes.map(q=>({...q,available_quantity:0.1}))});
    const depleted=await w.send({type:'price',request:57,pattern,options,quotes:depletedBook,market:splitPolicy});
    assert.equal(depleted.type,'result',depleted.message);
    assert.equal(depleted.files[0].text,ftx.cut_price(pattern,options,depletedBook,splitPolicy));
    assert.equal(depleted.result.complete_offer_coverage,false);
    assert.equal(depleted.result.selected_observed_order_total,null);
    assert.equal(depleted.result.least_observed_independent_order_total,null);
    assert.equal(depleted.result.material_orders[0].allocation.selected_orders,null);
    assert.equal(depleted.result.material_orders[0].allocation.no_feasible_listed_allocation,true);
    const invalid=await w.send({type:'cut',request:54,pattern,options,rollIndex:2});
    assert.equal(invalid.type,'error');assert.match(invalid.message,/between 1 and 2/);

    // The source range changes actual complete-marker placement, while purchasing keeps nominal width.
    const widthSource=JSON.parse(pattern);
    widthSource.fabric.material.record.properties.width.bounds=[0.075,0.11];
    const widthPattern=JSON.stringify(widthSource);
    const widthNominal=await w.send({type:'cut',request:58,pattern:widthPattern,options,rollIndex:1});
    assert.equal(widthNominal.type,'result',widthNominal.message);
    assert.equal(widthNominal.result.rolls.length,2);
    assert.ok(widthNominal.result.rolls.every(r=>r.width_m===0.1&&!('layout_width' in r)));
    const widthOptions=JSON.stringify({...JSON.parse(options),width_basis:'reported_lower_bound'});
    const widthCut=await w.send({type:'cut',request:59,pattern:widthPattern,options:widthOptions,rollIndex:2});
    assert.equal(widthCut.type,'result',widthCut.message);
    assert.equal(widthCut.files[0].text,ftx.cut_plan(widthPattern,widthOptions));
    assert.deepEqual(widthCut.result.rolls.map(r=>r.length_m),[0.12,0.12,0.12]);
    assert.deepEqual(widthCut.result.rolls.flatMap(r=>r.placements.map(p=>[p.instance,p.copy])),
      [['front',0],['front',1],['front',2]]);
    for(const marker of widthCut.result.rolls) {
      assert.equal(marker.width_m,0.1);assert.equal(marker.layout_width.width_m,0.075);
      assert.deepEqual(marker.layout_width.reported_bounds_m,[0.075,0.11]);
      assert.equal(marker.material.sha256,widthNominal.result.rolls[0].material.sha256);
      assert.ok(Math.abs(marker.stock_area_m2-0.012)<1e-12);
      assert.ok(Math.abs(marker.stock_mass_kg-0.0024)<1e-12);
      assert.equal(marker.placements.length,1);
      assert.ok(marker.placements[0].bounds_mm[2]<=70);
    }
    assert.equal(widthCut.files[1].name,'roll-2.svg');
    assert.equal(widthCut.files[1].text,ftx.cut_svg(widthPattern,widthOptions,2));
    assert.match(widthCut.files[1].text,/id="stock" width="100"/);
    assert.match(widthCut.files[1].text,/id="layout-width" width="75"/);
    assert.match(widthCut.files[1].text,/data-copy="2"/);
    const widthBook=JSON.stringify({schema:'ftx-textile-quotes/1',quotes:[{...quote,
      offer_id:'width-whole-order',sku:'width-whole-order',price:10,available_quantity:0.4,
      material_sha256:widthCut.result.rolls[0].material.sha256}]});
    const widthPrice=await w.send({type:'price',request:60,pattern:widthPattern,options:widthOptions,quotes:widthBook,market});
    assert.equal(widthPrice.type,'result',widthPrice.message);
    assert.equal(widthPrice.files[0].text,ftx.cut_price(widthPattern,widthOptions,widthBook,market));
    assert.deepEqual(widthPrice.result.cut_plan,widthCut.result);
    assert.equal(widthPrice.result.complete_offer_coverage,true);
    const widthOrder=widthPrice.result.material_orders[0];
    assert.deepEqual(widthOrder.roll_indices,[0,1,2]);
    assert.ok(Math.abs(widthOrder.required_length_m-0.36)<1e-12);
    assert.ok(Math.abs(widthOrder.required_stock_area_m2-0.036)<1e-12);
    assert.ok(Math.abs(widthOrder.sample.scope.requirement-0.036)<1e-12);
    assert.equal(widthOrder.required_continuous_length_m,0.12);
    assert.equal(widthOrder.least_observed_order.order_quantity,0.4);
    assert.equal(widthPrice.result.least_observed_independent_order_total,4);
  });

test('compiled WASM fresh scalar and area identification publishes a reusable law with exact CHECK replay',
  {skip:process.env.FTX_REAL_WASM!=='1'},async()=>{
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const w=await workerModule({bindings:ftx});
    const request=fs.readFileSync(path.join(__dirname,'../tests/fixtures/materials/synthetic-scalar-area-clamped-strip.json'),'utf8');
    const result=await w.send({type:'stripFit',request:81,stripRequest:request});
    assert.equal(result.type,'result',result.message);
    assert.equal(result.files.find(f=>f.name==='strip-calibration-input.json').text,request);
    assert.equal(result.files.find(f=>f.name==='strip-calibrated-selection.json').text,result.result.selection_json);
    const c=result.result.calibration;
    assert.equal(c.accepted,true);assert.equal(c.domain_replay_verified,true);
    assert.equal(c.input.material.membrane_response,undefined);
    assert.equal(c.parameters.length,6);assert.equal(c.parameter_bounds.length,6);
    assert.deepEqual(c.parameter_descriptors.map(d=>d.id),['warp_stiffness','weft_stiffness',
      'poisson_warp_weft','shear_stiffness','area_quadratic','area_log_squared']);
    assert.deepEqual(c.material.membrane_response.area_response.area_ratio_range,c.fitted_area_ratio_range);
    assert.equal(c.material.membrane_response.area_response.quadratic.evidence,'estimate');
    assert.equal(result.result.fit_claim,false);
    assert.deepEqual(result.result.material,JSON.parse(ftx.material_inspect(result.result.selection_json)));
    if(process.env.FTX_SCALAR_AREA_REPLAY) {
      const raw=JSON.parse(fs.readFileSync(process.env.FTX_SCALAR_AREA_REPLAY,'utf8')),native=raw.result??raw;
      for(const key of ['calibration','selection','selection_json','fit_claim']) assert.deepEqual(result.result[key],native[key]);
    }
    const check=c.input.experiments.find(e=>e.partition==='check');
    const forwardRequest={schema:'ftx-strip-experiment/1',material:result.result.selection,
      fixture:{...check.fixture,cells:check.fixture.cells.map(n=>2*n)},
      extensions:check.observations.map(o=>o.extension_fraction),controls:c.input.controls};
    const forward=await w.send({type:'stripExperiment',request:82,stripRequest:JSON.stringify(forwardRequest)});
    assert.equal(forward.type,'result',forward.message);
    assert.deepEqual(forward.result.states,c.predictions.filter(p=>p.experiment===check.id).map(p=>p.state));
    const replay=await w.send({type:'stripExperiment',request:83,stripRequest:JSON.stringify(forward.result.input)});
    assert.equal(replay.type,'result',replay.message);assert.equal(replay.files[0].text,forward.files[0].text);
    for(const dedicated of [false,true]) {
      const p=await pageModule();await p.load();p.e.calibrationMode.value='clamped_strip';p.e.calibrationEstimates.checked=true;
      p.select(dedicated?'stripFitRequest':'calibrationRequest',request);await p.e[dedicated?'stripFit':'calibrate'].onclick();
      p.emit(result.result,result.files);
      const s=section(p,dedicated?'Clamped-strip identification':'Identified material behavior');
      assert.match(visibleText(s),/Identified scalar base and physical-area coefficients/);
      assert.match(visibleText(s),/Scalar base — declared trial strain support/);
      assert.match(visibleText(s),/No prior area coefficients were supplied/);
      assert.equal(p.e.selection.value,result.result.selection_json);assert.equal(p.e.assign.disabled,false);
    }
    const missing=JSON.parse(request);delete missing.area_response.scalar_green_strain_range;
    const refused=await w.send({type:'stripFit',request:84,stripRequest:JSON.stringify(missing)});
    assert.equal(refused.type,'error');assert.match(refused.message,/scalar_green_strain_range/);
    assert.equal(refused.files,undefined);
  });

test('compiled WASM joint area identification publishes the fitted law and exact frozen strip replay',
  {skip:process.env.FTX_REAL_WASM!=='1'},async()=>{
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const w=await workerModule({bindings:ftx});
    const calibrationRequest=fs.readFileSync(path.join(__dirname,'../examples/area-identification/swatch.json'),'utf8');
    const identified=await w.send({type:'calibrate',request:71,calibrationMode:'nonlinear',calibrationRequest,allowEstimates:true});
    assert.equal(identified.type,'result',identified.message);
    assert.equal(identified.files[0].text,ftx.material_calibrate_nonlinear(calibrationRequest,true));
    assert.equal(identified.files[1].text,identified.result.selection_json);
    assert.equal(identified.files[2].text,calibrationRequest);
    const c=identified.result.calibration,area=c.material.membrane_response.area_response;
    assert.equal(c.parameter_count,9);
    assert.ok(Math.abs(area.quadratic.value-40)<1e-7);
    assert.ok(Math.abs(area.log_squared.value-15)<1e-7);
    assert.equal(area.quadratic.evidence,'estimate');assert.equal(area.log_squared.evidence,'estimate');
    assert.deepEqual(area.area_ratio_range,c.fitted_area_ratio_range);
    assert.ok(c.fit.rms_scaled<1e-9&&c.check.rms_scaled<1e-9);
    assert.equal(identified.result.fit_claim,false);
    assert.equal(identified.result.material.capabilities.mechanical_coefficients.ready,false);
    assert.deepEqual(JSON.parse(ftx.material_inspect(identified.result.selection_json)),identified.result.material);
    if(process.env.FTX_AREA_CALIBRATION_REPLAY) {
      const native=JSON.parse(fs.readFileSync(process.env.FTX_AREA_CALIBRATION_REPLAY,'utf8'));
      for(const field of ['calibration','selection','selection_json','fit_claim']) assert.deepEqual(identified.result[field],native[field]);
    }
    const requestFor=selection=>`{"schema":"ftx-strip-experiment/1","material":${selection},`+
      '"fixture":{"gauge_length_m":0.1,"width_m":0.06,"warp_axis":[0,1],"cells":[2,4]},"extensions":[0,0.02]}';
    const stripRequest=requestFor(identified.result.selection_json);
    const forward=await w.send({type:'stripExperiment',request:72,stripRequest});
    assert.equal(forward.type,'result',forward.message);
    assert.equal(forward.files[0].text,ftx.material_strip_experiment(stripRequest));
    assert.ok(forward.result.states[1].force_n>0);
    assert.equal(forward.result.input.material.expected_sha256,identified.result.selection.expected_sha256);
    if(process.env.FTX_AREA_IDENTIFIED_STRIP_REPLAY) {
      assert.deepEqual(forward.result,JSON.parse(fs.readFileSync(process.env.FTX_AREA_IDENTIFIED_STRIP_REPLAY,'utf8')));
    }
    const replay=await w.send({type:'stripExperiment',request:73,stripRequest:JSON.stringify(forward.result.input)});
    assert.equal(replay.type,'result',replay.message);assert.equal(replay.files[0].text,forward.files[0].text);
    const denied=JSON.parse(ftx.material_calibrate_nonlinear(calibrationRequest,false));
    assert.deepEqual(denied.calibration,c);
    const refusal=await w.send({type:'stripExperiment',request:74,stripRequest:requestFor(denied.selection_json)});
    assert.equal(refusal.type,'error');assert.match(refusal.message,/estimate/);assert.equal(refusal.files,undefined);
  });

test('compiled WASM area response changes strip mechanics and preserves source, frozen replay and refusals through the worker',
  {skip:process.env.FTX_REAL_WASM!=='1'},async()=>{
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const w=await workerModule({bindings:ftx}), last=[];
    let enabled;
    for (const name of ['baseline','enabled']) {
      const stripRequest=fs.readFileSync(path.join(__dirname,`../examples/area-response/${name}.json`),'utf8');
      const result=await w.send({type:'stripExperiment',request:61,stripRequest});
      assert.equal(result.type,'result',result.message);
      assert.equal(result.files[0].text,ftx.material_strip_experiment(stripRequest));
      assert.equal(result.files[1].text,stripRequest);
      assert.equal(result.files.length,2); // A forward experiment does not publish a fitted selection.
      assert.equal(result.result.states.length,3);
      assert.ok(result.result.input.material.expected_sha256);
      const inspected=JSON.parse(ftx.material_inspect(JSON.stringify(JSON.parse(stripRequest).material)));
      assert.deepEqual(result.result.input.material.record,inspected.snapshot.authored);
      const frozen=JSON.stringify(result.result.input);
      const replay=await w.send({type:'stripExperiment',request:62,stripRequest:frozen});
      assert.equal(replay.type,'result',replay.message);
      assert.equal(replay.files[0].text,result.files[0].text);
      if(process.env.FTX_AREA_REPLAY) {
        const native=JSON.parse(fs.readFileSync(path.join(process.env.FTX_AREA_REPLAY,`${name}-report.json`),'utf8'));
        assert.deepEqual(result.result,native);
      }
      last.push(result.result.states.at(-1));
      if(name==='enabled') enabled=result.result;
    }
    assert.ok(Math.abs(last[1].force_n-last[0].force_n)>1e-6);
    assert.ok(Math.abs(last[1].mid_width_m-last[0].mid_width_m)>1e-8);
    const original=JSON.parse(fs.readFileSync(path.join(__dirname,'../examples/area-response/enabled.json'),'utf8'));
    const denied=structuredClone(original);denied.material.allow_estimates=false;
    const mismatch=structuredClone(original);
    mismatch.material.record.membrane_response.area_response.quadratic.conditions.specimen='different synthetic specimen';
    const outside=structuredClone(original);
    outside.material.record.membrane_response.area_response.area_ratio_range=[1,1.00001];
    const tampered=structuredClone(enabled.input);
    tampered.material.record.membrane_response.area_response.quadratic.value=101;
    for(const [request,reason] of [[denied,/estimate/],[mismatch,/match/],[outside,/range|domain/],[tampered,/expected_sha256/]]) {
      const stripRequest=JSON.stringify(request);
      let nativeBoundary;
      assert.throws(()=>ftx.material_strip_experiment(stripRequest),error=>{nativeBoundary=error.message;return reason.test(error.message);});
      const result=await w.send({type:'stripExperiment',request:63,stripRequest});
      assert.equal(result.type,'error');assert.equal(result.message,nativeBoundary);
      assert.equal(result.files,undefined);
    }
  });


test('timed strip and draped vapor workers preserve exact source and frozen replay bytes', async () => {
  const input='{"extension_fraction":1.0,"coordinate":-0.0}', conditions='{"pressure":2000.0}';
  const replay='{"source":{"coordinate":-0.0},"coefficient":1.0}';
  const raw=JSON.stringify({replay_request_json:replay}), calls=[];
  const w=await workerModule({bindings:{default:()=>Promise.resolve(),
    material_strip_history:(...args)=>{calls.push(['strip',...args]);return raw;},
    thermal_vapor_surface:(...args)=>{calls.push(['vapor',...args]);return raw;}}});
  const strip=await w.send({type:'stripHistory',stripRequest:input,request:104});
  assert.equal(strip.type,'result',strip.message); assert.deepEqual(calls.at(-1),['strip',input]);
  assert.deepEqual(plain(strip.files.map(f=>f.text)),[raw,input,replay]);
  assert.equal(strip.files[2].name,'strip-history-replay.json');
  const vapor=await w.send({type:'vaporSurface',report:input,conditions,request:105});
  assert.equal(vapor.type,'result',vapor.message); assert.deepEqual(calls.at(-1),['vapor',input,conditions]);
  assert.deepEqual(plain(vapor.files.map(f=>f.text)),[raw,replay,conditions,input]);
  for (const type of ['stripHistory','vaporSurface']) {
    const bad=await workerModule({bindings:{default:()=>Promise.resolve(),
      material_strip_history:()=> '{}',thermal_vapor_surface:()=> '{}'}});
    const denied=await bad.send({type,stripRequest:input,report:input,conditions});
    assert.equal(denied.type,'error'); assert.match(denied.message,/frozen.*replay/);
    assert.equal(bad.sent.some(m=>m.type==='result'),false);
  }
});

test('timed strip page keeps prior material assignment and invalidates only its experiment', async () => {
  const p=await pageModule(); await p.load(); await p.inspect('source-material');
  const selection=p.e.selection.value, input='{"schema":"ftx-strip-history/1","value":-0.0}';
  p.select('stripHistoryRequest',input); p.e.stripHistoryRequest.handlers.change();
  await p.e.stripHistory.onclick();
  assert.equal(p.current().messages.at(-1).stripRequest,input);
  p.emit({schema:'ftx-strip-history-report/1',input:{material:{id:'test'}},states:[
    {time_s:1,extension_fraction:0.1,force_n:2,mid_width_m:0.06,
      free_dynamic_residual_n:1e-9,stored_membrane_energy_j:0.02,free_kinetic_energy_j:0.001,
      maxwell_dissipation_j:0.003,plastic_dissipation_j:0}]});
  const s=section(p,'Strip loading and recovery');
  assert.match(visibleText(s),/Maxwell dissipation/); assert.match(visibleText(s),/Dynamic residual/);
  const t=descendants(s).find(n=>n.tag==='table');
  assert.deepEqual(plain(t.children[2].children.map(n=>n.textContent)),
    ['1','0.1','2','0.06','1e-9','0.02','0.001','0.003','0']);
  assert.equal(p.e.selection.value,selection); assert.equal(p.e.assign.disabled,false);
  p.e.stripHistoryRequest.handlers.change(); assert.equal(s.hidden,true);
});

test('drape vapor page uses retained original JSON and withdraws stale geometry results', async () => {
  const p=await pageModule(); await p.load(); await p.bind();
  for (const id of ['measurements','fit','body','vaporSurfaceConditions']) p.select(id);
  p.e.body.files[0].arrayBuffer=async()=>new ArrayBuffer(8);
  p.e.measurements.handlers.change();
  assert.equal(p.e.vaporSurface.disabled,true);
  await p.e.fold.onclick();
  const raw='{"material":{"basis":"test"},"coordinate":-0.0,"coefficient":1.0}';
  p.emit(JSON.parse(raw),[],raw);
  assert.equal(p.e.vaporSurface.disabled,false); await p.e.vaporSurface.onclick();
  assert.equal(p.current().messages.at(-1).report,raw);
  p.emit({schema:'ftx-vapor-surface-result/1',area_m2:0.5,potential_heat_flow_w:25,
    potential_mass_flow_kg_s:0.00001,columns:[],pieces:[{instance:'<shell>',area_m2:0.5,
      potential_heat_flow_w:25,potential_mass_flow_kg_s:0.00001}]});
  const s=section(p,'Vapor transfer on this drape');
  assert.match(visibleText(s),/Independent piece potentials/);
  assert.match(visibleText(s),/<shell>/); assert.match(visibleText(s),/actual wearer evaporation/);
  assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  p.e.vaporSurfaceConditions.handlers.change(); assert.equal(s.hidden,true);
  assert.equal(p.e.vaporSurface.disabled,false);
  p.e.fit.handlers.change(); assert.equal(p.e.vaporSurface.disabled,true);
});

test('scenario displays independent vapor refusals and freshly evaluated motion endpoints', async () => {
  const p=await pageModule(); await p.load(); scenarioInputs(p); await p.e.scenario.onclick();
  const report=scenarioReport(), initial=report.simulations.find(row=>row.key==='A').evaluation.result;
  const transfer={area_m2:0.5,potential_heat_flow_w:25,potential_mass_flow_kg_s:0.00001};
  initial.vapor_surface=outcome(transfer);
  initial.motion=outcome({duration_s:1,trials:1,samples:[],keyframes:[
    {time_s:1,arm_abduction_deg:40,surface:refused('no dry heat'),coverage:refused('no coverage'),
      vapor_surface:outcome({...transfer,area_m2:0.6,potential_heat_flow_w:30})}]});
  p.emit(report);
  const s=section(p,'Fresh drape and heat alternatives');
  const table=label=>descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent===label);
  assert.deepEqual(plain(table('Initial drape vapor potentials — independent sheets').children[2].children.map(n=>n.textContent)),
    ['A','Available','0.5','25','0.00001']);
  assert.deepEqual(plain(table('A: endpoint vapor potentials — independent sheets').children[2].children.map(n=>n.textContent)),
    ['1','Available','0.6','30','0.00001']);
  assert.match(visibleText(table('Initial drape vapor potentials — independent sheets')),/missing mechanics/);
});

test('moving layered vapor displays accepted coverage, exact replay and independent late refusal', async () => {
  const p=await pageModule(); await p.load(); scenarioInputs(p); await p.e.scenario.onclick();
  const report=scenarioReport(); report.scenario.scenario={motion:{vapor_coverage_history:{}}};
  const initial=report.simulations.find(row=>row.key==='A').evaluation.result;
  const transfer={coverage:{covered_area_m2:0.5,uncovered_area_m2:0.1,unassigned_cloth_reference_area_m2:0.2,
    covered_potential_heat_flow_w:10,covered_potential_mass_flow_kg_s:0.000004}};
  const replay='{"schema":"ftx-vapor-coverage-history/1","coordinate":-0.0,"duration":1.0}';
  const history={duration_s:1,integration:'right_endpoint',totals:{potential_energy_j:10,potential_mass_kg:0.000004},
    initial:{time_s:0,result:transfer},samples:[{time_s:1,dt_s:1,result:transfer,potential_energy_j:10,potential_mass_kg:0.000004}],
    replay_request_json:replay,path_sha256:'current-motion-path'};
  initial.motion=outcome({duration_s:1,trials:1,samples:[{}],vapor_coverage_history:outcome(history),keyframes:[
    {time_s:1,arm_abduction_deg:40,surface:refused('no dry heat'),coverage:refused('no heat capacity'),vapor_coverage:outcome(transfer)}]});
  p.emit(report);
  const s=section(p,'Fresh drape and heat alternatives');
  const table=label=>descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent===label);
  assert.deepEqual(plain(table('A: moving layered vapor potential').children[2].children.map(n=>n.textContent)),
    ['Available','1','1','10','0.000004','right_endpoint']);
  assert.deepEqual(plain(table('A: vapor coverage at accepted motion samples').children[2].children.map(n=>n.textContent)),
    ['0','Initial only','0.5','0.1','0.2','10','0.000004','Initial only','Initial only']);
  assert.deepEqual(plain(table('A: vapor coverage at accepted motion samples').children[3].children.map(n=>n.textContent)),
    ['1','1','0.5','0.1','0.2','10','0.000004','10','0.000004']);
  assert.deepEqual(plain(table('A: endpoint vapor potentials — covered layers').children[2].children.map(n=>n.textContent)),
    ['1','Available','0.5','0.1','10','0.000004']);
  assert.match(visibleText(s),/complete-domain comparisons require coverage at every sample/);
  assert.match(visibleText(s),/refining motion samples can change the integral/);
  assert.match(visibleText(table('B: moving layered vapor potential')),/missing mechanics/);
  const download=descendants(s).find(n=>n.download==='alternative-1-motion-vapor-coverage-replay.json');
  assert.equal(await p.blobs.get(download.href).text(),replay);
  assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
  await p.e.scenario.onclick(); initial.motion.result.vapor_coverage_history=refused('late geometry budget exhausted'); p.emit(report);
  assert.equal(table('A: vapor coverage at accepted motion samples'),undefined);
  assert.match(visibleText(table('A: moving layered vapor potential')),/late geometry budget exhausted/);
  assert.equal(table('A: moving layered vapor potential').children[2].children.slice(1).every(n=>n.textContent==='Unknown'),true);
  assert.ok(table('A: endpoint vapor potentials — covered layers'));
  assert.equal(descendants(s).some(n=>n.download==='alternative-1-motion-vapor-coverage-replay.json'),false);
});

test('material scenarios retain finite body water separately from potential demand and withdraw stale ledgers', async () => {
  const p=await pageModule(); await p.load(); scenarioInputs(p); await p.e.scenario.onclick();
  const report=scenarioReport(), history=wetFilmHistoryReport();
  report.scenario.scenario={motion:{vapor_coverage_history:{wet_film:{}}}};
  const initial=report.simulations.find(row=>row.key==='A').evaluation.result;
  initial.motion=outcome({duration_s:1,trials:2,samples:[{},{}],keyframes:[],vapor_coverage_history:outcome(history)});
  const evaporation='motion_wet_film_evaporated_mass_kg', latent='motion_wet_film_body_reservoir_latent_energy_j';
  report.comparison={policy:{basis:'nominal',allow_mixed_protocols:false,required_consumers:['motion_wet_film'],
    objectives:[{metric:evaporation,direction:'maximize'}],constraints:[{metric:latent,max:10}]},
    motion_required:true,frontier:['A'],incomparable:[],alternatives:[
      {key:'A',eligible:true,dominated_by:[],exclusions:[],readings:{[evaporation]:{value:0.000004,unit:'kg'},[latent]:{value:10,unit:'J'}}},
      {key:'B',eligible:false,dominated_by:[],exclusions:[{message:'complete finite-water history unavailable'}],readings:{}}]};
  p.emit(report);
  const s=section(p,'Fresh drape and heat alternatives');
  const table=label=>descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent===label);
  assert.deepEqual(plain(table('A: moving layered vapor potential').children[2].children.slice(3,5).map(n=>n.textContent)),
    ['17.5','0.000007']);
  const water=table('A: finite body water and latent energy');
  assert.equal(water.children[5].children[1].textContent,'0.000004');
  assert.equal(water.children[8].children[1].textContent,'10');
  assert.equal(table('B: finite body water and latent energy'),undefined);
  const comparison=()=>table('Declared computed objectives — authoritative Rust comparison');
  assert.deepEqual(plain(comparison().children[2].children.map(n=>n.textContent)),['A','Eligible','None reported','0.000004 kg','10 J']);
  assert.deepEqual(plain(comparison().children[3].children.slice(3).map(n=>n.textContent)),['Unknown','Unknown']);
  assert.match(visibleText(s),/water is not pooled between triangles/);
  assert.match(visibleText(s),/do not propagate water-state uncertainty/);
  const link=descendants(s).find(n=>n.download==='alternative-1-motion-vapor-coverage-replay.json');
  assert.equal(await p.blobs.get(link.href).text(),history.replay_request_json);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
  await p.e.scenario.onclick();
  initial.motion.result.vapor_coverage_history=refused('body water is bound to a different initial mesh');
  report.comparison.frontier=[];
  Object.assign(report.comparison.alternatives[0],{eligible:false,readings:{},exclusions:[{message:'body water is bound to a different initial mesh'}]});
  p.emit(report);
  assert.equal(table('A: finite body water and latent energy'),undefined);
  assert.match(visibleText(table('A: moving layered vapor potential')),/different initial mesh/);
  assert.match(comparison().children[2].children[1].textContent,/Excluded: body water is bound/);
  assert.deepEqual(plain(comparison().children[2].children.slice(3).map(n=>n.textContent)),['Unknown','Unknown']);
  assert.equal(descendants(s).some(n=>n.download==='alternative-1-motion-vapor-coverage-replay.json'),false);
});

test('finite-water endpoint comparisons display native final ranges, unresolved pairs and unavailable bounds without altering nominal histories', async () => {
  const p=await pageModule(); await p.load(); scenarioInputs(p);
  const report=scenarioReport(), history=wetFilmHistoryReport();
  report.scenario.scenario={motion:{vapor_coverage_history:{wet_film:{}}}};
  report.simulations.find(row=>row.key==='A').evaluation.result.motion=outcome({duration_s:1,trials:2,
    samples:[{},{}],keyframes:[],vapor_coverage_history:outcome(history)});
  const fields=['evaporated_mass_kg','remaining_liquid_mass_kg','unmet_demand_mass_kg','supplied_mass_kg','body_reservoir_latent_energy_j'];
  const ranges=[[0.000003,0.000004],[0,0.000001],[0,0.000006],[0.000001,0.000001],[7.5,10]];
  const readings=Object.fromEntries(fields.map((field,i)=>[`motion_wet_film_${field}`,
    {value:history.wet_film.totals[field],unit:field.endsWith('_j')?'J':'kg',reported_bounds:ranges[i]}]));
  const comparison=report.comparison={policy:{basis:'reported_bounds',allow_mixed_protocols:false,
    required_consumers:['motion_wet_film'],objectives:fields.map(field=>({metric:`motion_wet_film_${field}`,direction:'minimize'})),constraints:[]},
    motion_required:true,frontier:['B','A'],incomparable:[{alternatives:['A','B'],reason:'Final water ranges overlap; native decision unresolved'}],
    alternatives:['A','B'].map(key=>({key,eligible:true,dominated_by:[],exclusions:[],readings:plain(readings)}))};
  await p.e.scenario.onclick(); p.emit(report);
  const s=section(p,'Fresh drape and heat alternatives');
  const table=label=>descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent===label);
  const decisions=()=>table('Declared computed objectives — authoritative Rust comparison');
  assert.deepEqual(plain(decisions().children[2].children.slice(3).map(n=>n.textContent)),[
    '0.000004 kg; reported range [0.000003, 0.000004] kg','0 kg; reported range [0, 0.000001] kg',
    '0.000003 kg; reported range [0, 0.000006] kg','0.000001 kg; reported range [0.000001, 0.000001] kg',
    '10 J; reported range [7.5, 10] J']);
  assert.match(visibleText(s),/Computed reported_bounds frontier: B, A/);
  assert.match(visibleText(s),/A \/ B: Final water ranges overlap; native decision unresolved/);
  assert.match(visibleText(s),/initial liquid, supply and integration fixed/);
  assert.match(visibleText(s),/Remaining liquid reverses the demand endpoint order/);
  assert.match(visibleText(s),/replay remain nominal, without per-interval water bounds/);
  assert.match(visibleText(s),/do not establish general water-state uncertainty or confidence intervals/);
  assert.equal(table('A: finite body water and latent energy').children[5].children[1].textContent,'0.000004');
  assert.doesNotMatch(visibleText(table('A: body water at accepted motion samples')),/reported range/);
  const replay=descendants(s).find(n=>n.download==='alternative-1-motion-vapor-coverage-replay.json');
  assert.equal(await p.blobs.get(replay.href).text(),history.replay_request_json);

  const missing=comparison.alternatives[1];
  missing.eligible=false; missing.exclusions=[{message:'Consumed Ret endpoints unavailable for final finite-water ranges'}];
  for (const field of fields.filter(field=>field!=='supplied_mass_kg')) delete missing.readings[`motion_wet_film_${field}`].reported_bounds;
  comparison.frontier=['A']; comparison.incomparable=[];
  await p.e.scenario.onclick(); p.emit(report);
  assert.match(decisions().children[3].children[1].textContent,/Excluded: Consumed Ret endpoints unavailable/);
  assert.equal(decisions().children[3].children[3].textContent,'0.000004 kg; reported range unknown');
  assert.equal(decisions().children[3].children[6].textContent,'0.000001 kg; reported range [0.000001, 0.000001] kg');
  assert.equal(table('A: finite body water and latent energy').children[8].children[1].textContent,'10');

  // Fixed supply does not require Ret endpoints, and consumer-only water qualification does not compute water ranges.
  comparison.policy.objectives=[{metric:'motion_wet_film_supplied_mass_kg',direction:'minimize'}];
  missing.eligible=true; missing.exclusions=[]; comparison.frontier=['A','B'];
  await p.e.scenario.onclick(); p.emit(report);
  assert.match(visibleText(s),/fixed singleton range and needs no Ret bounds/);
  assert.doesNotMatch(visibleText(s),/source ranges replay low- and high-demand Ret endpoints/);
  comparison.policy.objectives=[{metric:'assembly_mass_kg',direction:'minimize'}];
  for (const row of comparison.alternatives) row.readings={assembly_mass_kg:{value:0.2,unit:'kg',reported_bounds:[0.19,0.21]}};
  await p.e.scenario.onclick(); p.emit(report);
  assert.match(visibleText(s),/These nominal results do not propagate water-state uncertainty/);
  assert.doesNotMatch(visibleText(s),/source ranges replay low- and high-demand Ret endpoints|replay remain nominal, without per-interval water bounds/);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
});

function splitPurchaseReport() {
  const scope={currency:'USD',region:'US',quality:'test',terms:'merchandise only',as_of:'2026-10-22'};
  const offer=(supplier,offer_id,cost,quantity)=>({quote:{supplier,offer_id,sku:`${offer_id}/base`,unit:'linear_meter',
    observed_on:'2026-10-22',material_sha256:'locked-revision',available_quantity:quantity,maximum_continuous_quantity:5},
    order_cost:cost,order_quantity:quantity,quantity_sufficiency_observed:true,within_observed_continuous_limit:true});
  const selected_orders=[
    {offer:offer('<Supplier A>','A/retail',12,5),roll_indices:[0,2],required_length_m:5,
      required_stock_area_m2:6,required_continuous_length_m:3,scope},
    {offer:offer('Supplier B','B/retail',11.5,5),roll_indices:[1],required_length_m:5,
      required_stock_area_m2:6,required_continuous_length_m:5,scope}];
  const tier={minimum_quantity:5,price:2.4,sku:'A-five'};
  selected_orders[0].offer.quote.price_breaks=[tier];
  selected_orders[0].offer.applied_price_break=tier;
  return {schema:'ftx-cut-procurement/3',market:{...scope,split_orders:{schema:'ftx-cut-order-allocation/1',max_search_nodes:30}},
    quotes:{schema:'ftx-textile-quotes/1',quotes:selected_orders.map(o=>o.offer.quote)},currency:'USD',
    complete_offer_coverage:true,complete_observed_quantity_coverage:true,complete_continuous_length_limit_coverage:true,
    selected_observed_order_total:23.5,least_observed_independent_order_total:null,
    order_allocation_optimality_proven:false,search_complete:false,budget_exhausted:true,search_nodes:20,quote_evaluations:10,
    material_orders:[{material_order_index:0,material_sha256:'locked-revision',roll_indices:[0,1,2],
      marker_lengths_m:[3,5,2],required_length_m:10,required_stock_area_m2:12,required_continuous_length_m:5,
      allocation:{selected_orders,candidate_offer_ids:['A/retail','B/retail'],excluded_unlocked_offer_ids:['unlocked'],
        search_nodes:20,quote_evaluations:10,search_complete:false,budget_exhausted:true,optimality_proven:false}}],
    limits:['Synthetic UI display only']};
}

test('split supplier purchasing displays whole-marker assignments and bounded feasible cost without inventing a minimum', async () => {
  const p=await pageModule(); await p.load(); await p.bind();
  const market='{"split_orders":{"schema":"ftx-cut-order-allocation/1","max_search_nodes":30},"value":-0.0}';
  p.select('cutOptions'); p.select('quotes'); p.select('market',market); await p.e.price.onclick();
  assert.equal(p.current().messages.at(-1).market,market);
  const report=splitPurchaseReport(), raw=JSON.stringify(report);
  p.emit(report,[{name:'split-orders.json',text:raw,mime:'application/json'}]);
  const s=section(p,'price'), t=caption=>descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent===caption);
  assert.deepEqual(plain(t('Whole-marker supplier allocation').children[2].children.map(n=>n.textContent)),
    ['Yes','23.5 USD','Unknown','false','true','false','20','10']);
  const assigned=t('Whole-marker supplier allocation: material order 1');
  assert.deepEqual(plain(assigned.children[2].children.map(n=>n.textContent)),
    ['<Supplier A> / A/retail','1, 3','6','5 linear_meter','12','5 linear_meter','3','5 linear_meter; within observed limit','A-five; tier from 5 linear_meter']);
  assert.equal(assigned.children[3].children[1].textContent,'2');
  assert.equal(assigned.children[3].children[8].textContent,'B/retail/base');
  assert.match(visibleText(s),/complete feasible allocation remains usable/);
  assert.match(visibleText(s),/Least cost has not been proved/);
  assert.match(visibleText(s),/does not reserve stock|do not reserve stock/);
  assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  const download=descendants(s).find(n=>n.download==='split-orders.json');
  assert.equal(await p.blobs.get(download.href).text(),raw);
  p.e.market.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.has(download.href),false);
  await p.e.price.onclick();
  report.complete_offer_coverage=false; report.selected_observed_order_total=null;
  report.material_orders[0].allocation.selected_orders=null; p.emit(report);
  assert.deepEqual(plain(t('Whole-marker supplier allocation').children[2].children.slice(0,3).map(n=>n.textContent)),
    ['No complete allocation found','Unknown','Unknown']);
  assert.match(visibleText(t('Whole-marker supplier allocation: material order 1')),/No complete allocation found within the work limit/);
  assert.doesNotMatch(visibleText(s),/23.5 USD/);
});

test('studies retain split order assignments and distinguish mixed-job selected cost from unproved minimum', async () => {
  const p=await pageModule(); await p.load(); scenarioInputs(p);
  const a=studyRow('roll orders'), b=studyRow('mixed job'), purchase=splitPurchaseReport();
  a.purchasing=outcome(purchase); a.metrics.selected_observed_order_total=23.5;
  b.cutting_job=outcome({complete_inventory:true,inventory:[{},{}],roll:{rolls:[{},{}]},finite_stock:{used_stock_count:1},
    stock_area_m2:13,external_cut_area_m2:8,stock_mass_kg:2,
    purchasing:{currency:'USD',complete_offer_coverage:true,selected_observed_order_total:33.5,
      least_observed_independent_order_total:null,order_allocation_optimality_proven:false,
      roll:outcome(purchase),finite_stock:outcome({complete_offer_coverage:true,least_observed_independent_order_total:10})}});
  for (const op of ['study','scenario']) {
    await p.e[op].onclick(); const report=studyReport([a,b]);
    p.emit(op==='study'?report:{...scenarioReport(),manufacturing:report});
    const s=section(p,op==='study'?'Fixed-pattern manufacturing alternatives':'Fresh drape and heat alternatives');
    const t=caption=>descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent===caption);
    assert.deepEqual(plain(t('mixed job: complete mixed-stock purchasing').children[2].children.map(n=>n.textContent)),
      ['Yes','33.5 USD','Unknown','false']);
    for (const caption of ['roll orders: whole-marker supplier allocation','mixed job: roll-route supplier allocation']) {
      assert.equal(t(caption).children[2].children[1].textContent,'23.5 USD');
      assert.equal(t(caption).children[2].children[2].textContent,'Unknown');
      assert.equal(t(`${caption}: material order 1`).children[2].children[1].textContent,'1, 3');
    }
    assert.equal(t('Independent manufacturing outcomes').children[2].children[8].textContent,'Unknown');
  }
  p.e.studyRequest.handlers.change();
  assert.equal(section(p,'Fixed-pattern manufacturing alternatives').hidden,true);
  assert.equal(section(p,'Fresh drape and heat alternatives').hidden,true);
});

test('vapor bounds stay source-qualified and missing endpoints retain nominal display', async () => {
  const p=await pageModule(); p.select('vaporRequest'); await p.e.vapor.onclick();
  const report=vaporReport();
  report.reported_bounds={layer_resistance_m2_pa_w:[15,30],total_resistance_m2_pa_w:[35,50],
    potential_heat_flux_w_m2:[40,57],potential_heat_flow_w:[40,57],
    potential_mass_flux_kg_m2_s:[0.000016,0.000023],potential_mass_flow_kg_s:[0.000016,0.000023]};
  report.bounds_unavailable=[]; report.layers[0].observation.bounds=[15,30]; p.emit(report);
  const s=section(p,'Potential evaporative transfer'), t=()=>descendants(s).filter(n=>n.tag==='table');
  assert.deepEqual(plain(t()[0].children[2].children.map(n=>n.textContent)),
    ['Total vapor resistance','40; reported range [35, 50]','m²·Pa/W']);
  assert.equal(t()[0].children[5].children[1].textContent,'50; reported range [40, 57]');
  assert.equal(t()[1].children[2].children[2].textContent,'20; source bounds [15, 30]');
  assert.match(visibleText(s),/not confidence intervals/);
  await p.e.vapor.onclick(); report.reported_bounds=null;
  report.bounds_unavailable=['<shell>: source resistance bounds are not reported']; p.emit(report);
  assert.equal(t()[0].children[5].children[1].textContent,'50');
  assert.match(visibleText(s),/<shell>: source resistance bounds are not reported/);
  assert.match(visibleText(s),/missing bounds are not replaced with an exact range/);
  assert.doesNotMatch(visibleText(t()[0]),/reported range/);
  p.e.vaporRequest.handlers.change(); assert.equal(s.hidden,true);
});

test('vapor range comparisons and motion samples display Rust intervals without nominal reranking', async () => {
  const p=await pageModule(); await p.load(); scenarioInputs(p); await p.e.scenario.onclick();
  const report=scenarioReport(), initial=report.simulations.find(r=>r.key==='A').evaluation.result;
  const energy={potential_energy_j:[8,12],potential_mass_kg:[0.0000032,0.0000048]};
  const flow={covered_potential_heat_flow_w:[8,12],covered_potential_mass_flow_kg_s:[0.0000032,0.0000048]};
  const transfer={coverage:{body_area_m2:0.5,covered_area_m2:0.5,uncovered_area_m2:0,unassigned_cloth_reference_area_m2:0,
    covered_potential_heat_flow_w:10,covered_potential_mass_flow_kg_s:0.000004,reported_bounds:flow,bounds_unavailable:[]}};
  initial.vapor_coverage=outcome(transfer);
  initial.motion=outcome({duration_s:1,trials:1,samples:[{}],keyframes:[],vapor_coverage_history:outcome({
    duration_s:1,integration:'right_endpoint',totals:{potential_energy_j:10,potential_mass_kg:0.000004,
      reported_bounds:energy,bounds_unavailable:[]},initial:{time_s:0,result:transfer},samples:[{time_s:1,dt_s:1,
      result:transfer,potential_energy_j:10,potential_mass_kg:0.000004,reported_bounds:energy,bounds_unavailable:[]}]})});
  report.comparison={policy:{basis:'reported_bounds',allow_mixed_protocols:false,
    objectives:[{metric:'motion_covered_vapor_potential_energy_j',direction:'maximize'}],constraints:[]},
    frontier:['A','B'],motion_required:true,incomparable:[{alternatives:['A','B'],reason:'Reported intervals overlap'}],
    alternatives:['A','B'].map((key,i)=>({key,eligible:true,dominated_by:[],exclusions:[],readings:{
      motion_covered_vapor_potential_energy_j:{value:10+i,unit:'J',reported_bounds:[8+i,12+i]}}}))};
  p.emit(report);
  const s=section(p,'Fresh drape and heat alternatives'), t=caption=>descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent===caption);
  assert.match(visibleText(s),/Computed reported_bounds frontier: A, B/);
  assert.match(visibleText(s),/vapor potential at fixed geometry, boundaries and retained sampling/);
  assert.deepEqual(plain(t('Declared computed objectives — authoritative Rust comparison').children[2].children.map(n=>n.textContent)),
    ['A','Eligible','None reported','10 J; reported range [8, 12] J']);
  assert.equal(t('A: moving layered vapor potential').children[2].children[3].textContent,'10; reported range [8, 12]');
  assert.equal(t('A: vapor coverage at accepted motion samples').children[3].children[7].textContent,'10; reported range [8, 12]');
  assert.equal(t('Draped layered vapor coverage — covered-region potentials').children[2].children[6].textContent,
    '10; reported range [8, 12]');
  assert.match(visibleText(s),/A \/ B: Reported intervals overlap/);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden,true);
});

test('adaptive design search displays raw violations and exact trials without declaring unvisited values infeasible', async () => {
  const p = await pageModule(); await p.load(); p.select('studyRequest', '{}');
  const report = designReport(), source = ' {"schema":"fabric-pattern/1","value":-0.0,"label":"<retained>"}\n';
  const trial = (key,value,passed,selected,violation) => ({selected,max_fit_band_violation_mm:violation,
    total_fit_band_violation_mm:violation,design:{key,variables_mm:{'<length>':value},altered_pattern_json:source,
      validation:{passed},fit:outcome({passed,body_identity:{provenance:'MODEL_FIT'},
        validation:{points:[{status:passed?'ok':'tight',residual_mm:0}]}})},
    raw_fit:outcome([{id:'length',residual_mm:violation}]),
    refusal:passed?undefined:{message:'Declared fit band was missed'}});
  report.design_search = {schema:'ftx-design-adaptive-search/1',evaluations:3,qualified_designs:1,
    selected_designs:1,stop_reason:'qualified_design_limit',budget_exhausted:false,complete_search:false,
    global_optimum_claim:false,best_trial_key:'adaptive:0002',best_max_fit_band_violation_mm:0,
    best_total_fit_band_violation_mm:0,final_steps_mm:{'<length>':0.01},rows:[
      trial('adaptive:0000',30,false,false,3.69),trial('adaptive:0001',33.69,false,false,0.0001),
      trial('adaptive:0002',33.7,true,true,0)]};
  await p.e.study.onclick(); p.emit(report);
  const s = section(p,'Fixed-pattern manufacturing alternatives');
  const table = name=>descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent===name);
  assert.deepEqual(plain(table('Fit-directed adaptive alteration search').children[2].children.map(n=>n.textContent)),
    ['3','1','1','qualified design limit','0','0']);
  const rows=table('Adaptive alteration trials').children.slice(2);
  assert.deepEqual(plain(rows[1].children.map(n=>n.textContent)),
    ['adaptive:0001','<length>: 33.69','0.0001','0.0001','Refused','Not selected','Declared fit band was missed']);
  assert.equal(rows[2].children[5].textContent,'Selected');
  assert.match(visibleText(s),/does not prove that unvisited values are infeasible/);
  assert.equal(table('Declared alteration search'),undefined);
  const replay=descendants(s).find(n=>n.download==='adaptive-design-3-pattern.json');
  assert.equal(await p.blobs.get(replay.href).text(),source);
  report.design_search.stop_reason='evaluation_budget'; report.design_search.budget_exhausted=true;
  await p.e.study.onclick(); p.emit(report);
  assert.match(visibleText(s),/evaluation budget/);
  p.e.studyRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
});

test('design search retains incomplete domains and failed fit status even when residual rounds to zero', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  const report = designReport();
  report.design_search = {schema:'ftx-design-grid-search/1',generated_designs:2,total_grid_points:8,
    omitted_designs:6,complete_grid_enumeration:false,budget_exhausted:true,global_optimum_claim:false};
  const [baseline, wider, unavailable] = report.alternatives;
  baseline.design.fit = outcome({passed:false,body_identity:{provenance:'MODEL_FIT',shape_sha256:'fixed-shape'},
    fit_spec:{source:'<retained targets>'},validation:{status:'NOT_CONVERGED',points:[
      {id:'chest',label:'<chest & ease>',body_mm:100,finished_mm:102.96,ease_mm:3,
        target_mm:[3,5,8],status:'LOW',residual_mm:0}]}});
  wider.design.variables_mm = {'<ease>':4};
  wider.design.fit = outcome({passed:true,body_identity:{provenance:'MODEL_FIT',shape_sha256:'fixed-shape'},
    validation:{status:'CONVERGED',points:[{id:'chest',body_mm:100,finished_mm:104,ease_mm:4,
      target_mm:[3,5,8],status:'IN_BAND',residual_mm:0}]}});
  unavailable.design.fit = refused('Selected body measurement is unavailable');
  await p.e.study.onclick(); p.emit(report);
  const s = section(p,'Fixed-pattern manufacturing alternatives');
  assert.match(visibleText(s),/Stopped at design budget/);
  assert.match(visibleText(s),/frontier is conditional on visited designs/);
  const summary = descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent==='Declared alteration search');
  assert.deepEqual(plain(summary.children[2].children.map(n=>n.textContent)),['2','8','6','Stopped at design budget']);
  const fit = descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent==='Design variables and declared fit targets');
  assert.equal(fit.children[2].children[3].textContent,'Targets not met');
  assert.equal(fit.children[3].children[1].textContent,'<ease>: 4');
  assert.equal(fit.children[3].children[3].textContent,'Passed declared targets');
  assert.match(fit.children[4].children[3].textContent,/Selected body measurement is unavailable/);
  const points = descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent===baseline.key+': canonical flat fit measurements');
  assert.equal(points.children[2].children[5].textContent,'LOW');
  assert.equal(points.children[2].children[6].textContent,'0');
  assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  p.e.studyRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
});

test('complete design grids retain scope and unavailable points without turning enumeration into fit', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  const report = designReport();
  report.design_search = {generated_designs:2,total_grid_points:2,omitted_designs:0,
    complete_grid_enumeration:true,budget_exhausted:false,global_optimum_claim:false};
  report.alternatives[0].design.fit = outcome({passed:false,body_identity:{provenance:'MODEL_FIT'},
    validation:{status:'BLOCKED',points:[{id:'missing-hand',finished_mm:null,body_mm:null,ease_mm:null,
      target_mm:[0,5,10],residual_mm:null,status:'UNMEASURABLE'}]}});
  await p.e.study.onclick(); p.emit(report);
  const s = section(p,'Fixed-pattern manufacturing alternatives');
  assert.match(visibleText(s),/Complete declared grid/);
  assert.match(visibleText(s),/not a continuous or global optimum/);
  assert.match(visibleText(s),/UNMEASURABLE/);
  assert.match(visibleText(s),/Targets not met/);
  const points = descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent===report.alternatives[0].key+': canonical flat fit measurements');
  assert.equal(points.children[2].children[1].textContent,'Unknown');
  assert.equal(points.children[2].children[6].textContent,'Unknown');
});

test('fit screening separates rejected, selected, omitted and unvisited designs and retains exact patterns', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  const report = designReport(), source = ' {"schema":"fabric-pattern/1","coordinate":-0.0,"label":"<retained>"}\n';
  const screened = (key, value, passed, selected) => ({selected,
    design:{key,variables_mm:{'<length>':value},altered_pattern_json:source,
      validation:{passed},fit:outcome({passed,body_identity:{provenance:'MODEL_FIT',shape_sha256:'selected-shape'},
        validation:{points:[{id:'length',status:passed?'IN_BAND':'LOW',residual_mm:passed?0:20}]}})},
    refusal:passed?null:{message:'<length>: below the authored target'}});
  report.design_search = {total_grid_points:8,generated_designs:1,omitted_designs:7,
    complete_grid_enumeration:false,budget_exhausted:true,global_optimum_claim:false,
    fit_screening:{screened_designs:3,qualified_designs:2,rejected_designs:1,selected_designs:1,
      qualified_designs_omitted:1,unvisited_designs:5,complete_fit_screening:false,all_qualified_designs_selected:false,
      rows:[screened('grid:0000',0,false,false),screened('grid:0001',20,true,true),screened('grid:0002',30,true,false)]}};
  await p.e.study.onclick(); p.emit(report);
  const s = section(p,'Fixed-pattern manufacturing alternatives');
  const table = title => descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent===title);
  assert.deepEqual(plain(table('Fit screening before manufacturing and simulation').children[2].children.map(n=>n.textContent)),
    ['8','3','2','1','1','1','5']);
  const rows = table('Screened garment designs').children.slice(2);
  assert.deepEqual(plain(rows.map(row=>row.children.slice(2,4).map(n=>n.textContent))),
    [['Refused','Not selected'],['Passed declared flat gates','Selected'],['Passed declared flat gates','Not selected']]);
  assert.equal(rows[0].children[1].textContent,'<length>: 0');
  assert.equal(rows[0].children[4].textContent,'<length>: below the authored target');
  assert.match(visibleText(s),/unvisited grid points; their feasibility remains unknown/);
  assert.match(visibleText(s),/downstream results remain unknown/);
  assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  const replay = descendants(s).find(n=>n.download==='screened-design-3-pattern.json');
  assert.ok(replay); assert.equal(await p.blobs.get(replay.href).text(),source);
  p.e.studyRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
});

test('complete fit screening reports complete flat admission without certifying physical fit', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  const report = designReport();
  report.design_search = {total_grid_points:3,generated_designs:1,omitted_designs:2,
    complete_grid_enumeration:true,budget_exhausted:false,global_optimum_claim:false,
    fit_screening:{screened_designs:3,qualified_designs:1,rejected_designs:2,selected_designs:1,
      qualified_designs_omitted:0,unvisited_designs:0,complete_fit_screening:true,all_qualified_designs_selected:true,rows:[]}};
  await p.e.study.onclick(); p.emit(report);
  const text = visibleText(section(p,'Fixed-pattern manufacturing alternatives'));
  assert.match(text,/Every declared grid point received flat geometry and fit screening/);
  assert.match(text,/Every passing screened design was selected/);
  assert.match(text,/do not establish physical garment fit/);
  assert.doesNotMatch(text,/their downstream results remain unknown/);
});

test('compiled WASM fit screening reaches the same late design and named-case frontier as both native commands',
  {skip:process.env.FTX_REAL_WASM!=='1' || !process.env.FTX_DESIGN_SCREENING_REPLAY}, async()=>{
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const dir=process.env.FTX_DESIGN_SCREENING_REPLAY, input=name=>fs.readFileSync(path.join(dir,name),'utf8');
    const bytes=fs.readFileSync(path.join(dir,'model.ftxbody')), w=await workerModule({bindings:ftx});
    const previousProgress=globalThis.ftxProgress; globalThis.ftxProgress=()=>{};
    try {
      const standalone=await w.send({type:'study',request:169,onBody:true,pattern:input('pattern.json'),
        studyRequest:input('study.json'),measurements:input('body-input.json'),
        bytes:bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength)});
      assert.equal(standalone.type,'result',standalone.message);
      assert.deepEqual(standalone.result,JSON.parse(input('manufacturing-report.json')));
      assert.equal(standalone.result.design_search.fit_screening.rows.find(row=>row.selected).design.key,'grid:0006');
      assert.equal(standalone.files.find(file=>file.name==='body-input.json').text,input('body-input.json'));
      assert.equal(standalone.result.simulations,undefined);assert.equal(standalone.result.scenario,undefined);
      for(const name of ['single','matrix']) {
        const studyRequest=input('study.json'), scenarioRequest=input(`${name}-scenario.json`);
        const result=await w.send({type:'scenario',request:170,pattern:input('pattern.json'),studyRequest,scenarioRequest,
          measurements:input('body-input.json'),bytes:bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength)});
        assert.equal(result.type,'result',result.message);
        assert.deepEqual(result.result,JSON.parse(input(`${name}-report.json`)));
        const screening=result.result.manufacturing.design_search.fit_screening;
        assert.equal(screening.screened_designs,9); assert.equal(screening.selected_designs,1);
        assert.equal(screening.rows.find(row=>row.selected).design.key,'grid:0006');
        assert.equal(result.files.find(file=>file.name==='study-request.json').text,studyRequest);
        assert.equal(result.files.find(file=>file.name==='scenario-request.json').text,scenarioRequest);
      }
    } finally {globalThis.ftxProgress=previousProgress;}
  });

test('joint strip histories retain the declared network and separate loss ledgers until source changes', async () => {
  const p=await pageModule(); await p.inspect('prior-material');
  const prior=p.e.selection.value;
  const request=' {"schema":"ftx-strip-history/1","value":-0.0}\n';
  p.select('stripHistoryRequest',request); p.e.stripHistoryRequest.handlers.change();
  await p.e.stripHistory.onclick();
  const report={schema:'ftx-strip-history-report/1',input_sha256:'request-lock',
    model_scope:'Qualified parallel elastoplastic equilibrium and Maxwell branches',
    input:{material:{id:'joint-source',expected_sha256:'whole-material-lock',record:{
      membrane_relaxation:{parallel_plasticity:{model:'parallel_green_maxwell_plastic/1',
        evidence:'estimate',source:'paired-source',method:'<declared joint acquisition>',
        conditions:{specimen_id:'<same specimen>',conditioning:'declared dry state'}}}}}},
    states:[{time_s:0.2,extension_fraction:0,force_n:-0.3,mid_width_m:0.06,
      free_dynamic_residual_n:1e-8,stored_membrane_energy_j:0.004,free_kinetic_energy_j:0,
      maxwell_dissipation_j:0.003,plastic_dissipation_j:0.002}]};
  const raw=` ${JSON.stringify(report)}\n`;
  p.emit(report,[{name:'joint-history.json',text:raw,mime:'application/json'}]);
  const s=section(p,'Strip loading and recovery');
  assert.match(visibleText(s),/equilibrium branch retains permanent strain/);
  assert.match(visibleText(s),/Plastic flow uses equilibrium-branch stress/);
  const states=descendants(s).find(n=>n.tag==='table'&&n.children[0].textContent==='Loading and recovery history');
  assert.deepEqual(plain(states.children[2].children.slice(-2).map(n=>n.textContent)),['0.003','0.002']);
  const detail=descendants(s).find(n=>n.tag==='details'&&n.children[0].textContent==='Declared permanent-set and recovery network');
  detail.open=true; detail.handlers.toggle();
  assert.match(visibleText(detail),/<same specimen>/); assert.match(visibleText(detail),/whole-material-lock/);
  assert.equal(descendants(s).some(n=>'innerHTML' in n),false);
  const file=descendants(s).find(n=>n.download==='joint-history.json');
  assert.equal(await p.blobs.get(file.href).text(),raw);
  assert.equal(p.e.selection.value,prior);
  p.e.stripHistoryRequest.handlers.change();
  assert.equal(s.hidden,true); assert.equal(p.live.size,0); assert.equal(p.e.selection.value,prior);
  await p.e.stripHistory.onclick();
  const ordinary=plain(report); delete ordinary.input.material.record.membrane_relaxation.parallel_plasticity;
  p.emit(ordinary);
  assert.equal(visibleText(s).includes('Declared parallel network:'),false);
});

test('compiled WASM joint cyclic calibration replays native identification and actual fitted-material dynamics',
  {skip:process.env.FTX_REAL_WASM!=='1'}, async()=>{
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    for (const [file, simultaneous, native] of [
      ['material-parallel-calibration.json',false,process.env.FTX_JOINT_CALIBRATION_REPLAY],
      ['material-joint-calibration.json',true,process.env.FTX_SIMULTANEOUS_CALIBRATION_REPLAY]
    ]) {
    const calibrationRequest=fs.readFileSync(path.join(__dirname,'../examples',file),'utf8');
    const w=await workerModule({bindings:ftx});
    const identified=await w.send({type:'calibrate',request:231,calibrationMode:'membrane_plasticity',calibrationRequest,allowEstimates:true});
    assert.equal(identified.type,'result',identified.message);
    assert.equal(identified.result.calibration.status,'ACCEPTED');
    assert.deepEqual(identified.result,JSON.parse(ftx.material_calibrate_membrane_plasticity(calibrationRequest,true)));
    if (simultaneous) {
      assert.equal(JSON.parse(calibrationRequest).material.membrane_relaxation,undefined);
      const c=identified.result.calibration, p=c.parameters.relaxation;
      assert.ok(Math.abs(p.stiffness_ratio-1.4)<1e-6);
      assert.ok(Math.abs(p.relaxation_time_seconds-.18)<1e-6);
      assert.equal(c.diagnostics.fixed_maxwell_branch_steps,undefined);
      assert.equal(c.diagnostics.history_steps,2*c.diagnostics.joint_relaxation.branch_steps);
      assert.ok(c.diagnostics.excitation_pivot>1e-6);
      assert.equal(c.predictions.length,32);
      assert.equal(c.check_status,'ACCEPTED');
    } else {
      assert.equal(identified.result.calibration.diagnostics.fixed_maxwell_branch_steps,64);
      assert.deepEqual(identified.result.selection.record.membrane_relaxation,identified.result.calibration.input.material.membrane_relaxation);
    }
    assert.ok(identified.result.calibration.predictions.every(row=>row.parallel_history));
    if(native) assert.deepEqual(identified.result,JSON.parse(fs.readFileSync(path.join(native,'portable.json'),'utf8')));
    const fixture={gauge_length_m:0.1,width_m:0.06,warp_axis:[0,1],cells:[2,2]};
    const extension=simultaneous?0.08:0.04;
    const loading=[[0,0],[0.05,extension],[0.15,extension],[0.2,0],[0.25,0]]
      .map(([time_s,extension_fraction])=>({time_s,extension_fraction}));
    const controls={max_step_s:0.01,max_steps:32,max_newton:100,velocity_tolerance_m_s:1e-9,force_tolerance_n:1e-6};
    const stripRequest=native?fs.readFileSync(path.join(native,'forward-request.json'),'utf8'):
      `{"schema":"ftx-strip-history/1","material":${identified.result.selection_json},"fixture":${JSON.stringify(fixture)},"loading":${JSON.stringify(loading)},"controls":${JSON.stringify(controls)}}`;
    const history=await w.send({type:'stripHistory',request:232,stripRequest});
    assert.equal(history.type,'result',history.message);
    const last=history.result.states.at(-1);
    assert.ok(last.maxwell_dissipation_j>0&&last.plastic_dissipation_j>0);
    assert.deepEqual(history.result,JSON.parse(ftx.material_strip_history(history.result.replay_request_json)));
    if(native) assert.deepEqual(history.result,JSON.parse(fs.readFileSync(path.join(native,'forward-report.json'),'utf8')));
    if (simultaneous) {
      const failed=JSON.parse(calibrationRequest);
      failed.experiments.find(e=>e.partition==='check').samples.at(-1).second_piola_n_m[0]=1e6;
      const rejected=await w.send({type:'calibrate',request:233,calibrationMode:'membrane_plasticity',
        calibrationRequest:JSON.stringify(failed),allowEstimates:true});
      assert.equal(rejected.type,'result',rejected.message);
      assert.equal(rejected.result.calibration.status,'REJECTED');
      assert.deepEqual(rejected.result.calibration.parameters,identified.result.calibration.parameters);
      assert.deepEqual(rejected.result.calibration.fit,identified.result.calibration.fit);
      assert.equal(rejected.result.selection_json,null);
      const limited=JSON.parse(calibrationRequest); limited.max_evaluations=1;
      const partial=await w.send({type:'calibrate',request:234,calibrationMode:'membrane_plasticity',
        calibrationRequest:JSON.stringify(limited),allowEstimates:true});
      assert.equal(partial.type,'result',partial.message);
      assert.equal(partial.result.calibration.status,'BUDGET_EXHAUSTED');
      assert.equal(partial.result.selection_json,null);
    }
    }
  });

test('compiled WASM identifies a known plastic equilibrium and runs its frozen parallel material through the worker',
  {skip:process.env.FTX_REAL_WASM!=='1'}, async()=>{
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const raw=fs.readFileSync(path.join(__dirname,'../examples/relaxation-plastic-equilibrium.json'),'utf8');
    const input=JSON.parse(raw), w=await workerModule({bindings:ftx});
    const fitted=await w.send({type:'calibrate',request:210,calibrationMode:'relaxation',
      calibrationRequest:raw,allowEstimates:true});
    assert.equal(fitted.type,'result',fitted.message);
    const result=fitted.result, calibration=result.calibration;
    assert.deepEqual(result,JSON.parse(ftx.material_calibrate_relaxation(raw,true)));
    assert.equal(result.material.capabilities.mechanical_coefficients.ready,true);
    assert.deepEqual(calibration.material.properties,calibration.input.material.properties);
    assert.match(result.time_integration,/every recorded node, including unobserved loads/);
    assert.match(result.time_integration,/not a continuous-path plasticity solution/);
    const law=result.selection.record.membrane_relaxation, branch=law.branches[0];
    assert.deepEqual(law.parallel_plasticity,input.plastic_equilibrium.network);
    assert.ok(Math.abs(branch.stiffness_ratio.value-2)<1e-6);
    assert.ok(Math.abs(branch.relaxation_time.value-.05)<1e-7);
    assert.ok(calibration.fit.peak_scaled<1e-4);
    assert.ok(calibration.check.peak_scaled<1e-4);

    // Synthetic observations come from this independent scalar return map and exponential
    // convolution. Null forces still advance both histories, and CHECK starts pristine.
    let index=0;
    for(const experiment of input.experiments) {
      let plastic=0, accumulated=0, lag=0, previousTime=0, previousStrain=0;
      for(const sample of experiment.samples) {
        const stretch=sample.deformation[0][0], strain=.5*(stretch*stretch-1);
        const h=sample.time_seconds-previousTime;
        if(h>0) {
          const decay=Math.exp(-h/.05);
          lag=decay*lag+.05*(1-decay)/h*(strain-previousStrain);
        }
        const trial=1000*(strain-plastic), excess=Math.abs(trial)-(20+100*accumulated);
        if(excess>0) {
          const increment=excess/1100;
          plastic+=Math.sign(trial)*increment; accumulated+=increment;
        }
        const expected=stretch*(1000*(strain-plastic)+2000*lag);
        if(sample.nominal_n_m[0][0]!==null) {
          assert.ok(Math.abs(sample.nominal_n_m[0][0]-expected)<1e-10*Math.max(1,Math.abs(expected)));
        }
        const prediction=calibration.predictions[index++].plastic_equilibrium;
        assert.ok(Math.abs(prediction.plastic_strain[0]-plastic)<1e-12);
        assert.ok(Math.abs(prediction.accumulated_strain[0]-accumulated)<1e-12);
        previousTime=sample.time_seconds; previousStrain=strain;
      }
    }
    assert.equal(index,16); assert.equal(calibration.predictions.length,index);
    assert.deepEqual(calibration.predictions[2].residual_n_m,[[null,null],[null,null]]);
    assert.ok(calibration.predictions[2].plastic_equilibrium.plastic_strain[0]>.05);
    assert.equal(calibration.predictions[9].plastic_equilibrium.plastic_strain[0],0);
    assert.equal(fitted.files.find(file=>file.name==='calibration-input.json').text,raw);
    const selection=fitted.files.find(file=>file.name==='calibrated-selection.json').text;
    assert.equal(selection,result.selection_json);
    assert.deepEqual(JSON.parse(selection),result.selection);
    assert.deepEqual(JSON.parse(ftx.material_inspect(selection)),result.material);

    const pattern={schema:'fabric-pattern/1',units:'mm',seams:[],
      fabric:{areal_g_m2:200,note:'preserve unrelated metadata'},
      pieces:[{id:'swatch',material:'shell',cut:{qty:1},
        vertices:{a:[0,0],b:[100,0],c:[0,100]},
        edges:[{id:'ab',from:'a',to:'b'},{id:'bc',from:'b',to:'c'},{id:'ca',from:'c',to:'a'}],
        grain:[[0,0],[0,100]]}]};
    const bound=await w.send({type:'bind',request:211,pattern:JSON.stringify(pattern),
      changes:JSON.stringify({schema:'ftx-material-bindings/1',default:JSON.parse(selection)})});
    assert.equal(bound.type,'result',bound.message); assert.equal(bound.result.geometry_changed,false);
    const locked=JSON.parse(bound.result.pattern_json);
    assert.deepEqual(locked.pieces,pattern.pieces); assert.deepEqual(locked.seams,pattern.seams);
    assert.equal(locked.fabric.note,pattern.fabric.note);
    assert.equal(bound.files.find(file=>file.name==='pattern.json').text,bound.result.pattern_json);
    const strip={schema:'ftx-strip-history/1',material:locked.fabric.material,
      fixture:{gauge_length_m:.1,width_m:.06,warp_axis:[0,1],cells:[2,2]},
      loading:[[0,0],[.05,.02],[.15,.02],[.2,0],[.25,0]]
        .map(([time_s,extension_fraction])=>({time_s,extension_fraction})),
      controls:{max_step_s:.01,max_steps:32,max_newton:100,
        velocity_tolerance_m_s:1e-9,force_tolerance_n:1e-6}};
    const history=await w.send({type:'stripHistory',request:212,stripRequest:JSON.stringify(strip)});
    assert.equal(history.type,'result',history.message);
    assert.deepEqual(history.result.input.material,result.selection);
    assert.equal(history.result.states.length,27);
    const last=history.result.states.at(-1);
    assert.ok(last.relaxation&&last.plasticity);
    assert.ok(last.maxwell_dissipation_j>0); assert.ok(last.plastic_dissipation_j>0);
    const replay=history.files.find(file=>file.name==='strip-history-replay.json').text;
    assert.equal(replay,history.result.replay_request_json);
    const replayed=await w.send({type:'stripHistory',request:213,stripRequest:replay});
    assert.equal(replayed.type,'result',replayed.message);
    assert.deepEqual(replayed.result,history.result);

    const unqualified=plain(input), denied=plain(input), failedCheck=plain(input);
    delete unqualified.plastic_equilibrium;
    denied.plastic_equilibrium.allow_estimated_network=false;
    failedCheck.experiments[1].samples[2].nominal_n_m[0][0]=50000;
    for(const [offset,rejected] of [unqualified,denied,failedCheck].entries()) {
      const request=214+offset;
      const refused=await w.send({type:'calibrate',request,calibrationMode:'relaxation',
        calibrationRequest:JSON.stringify(rejected),allowEstimates:true});
      assert.equal(refused.type,'error'); assert.equal(refused.files,undefined);
      assert.equal(w.sent.some(message=>message.type==='result'&&message.request===request),false);
    }
  });

test('compiled WASM joint material memory preserves both histories and exact frozen replay through the worker',
  {skip:process.env.FTX_REAL_WASM!=='1'}, async()=>{
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const material=JSON.parse(fs.readFileSync(path.join(__dirname,'../tests/fixtures/materials/synthetic-parallel-membrane.json'),'utf8'));
    const input={schema:'ftx-strip-history/1',material,
      fixture:{gauge_length_m:0.1,width_m:0.06,warp_axis:[0,1],cells:[2,2]},
      loading:[[0,0],[0.05,0.08],[0.15,0.08],[0.2,0],[0.25,0]]
        .map(([time_s,extension_fraction])=>({time_s,extension_fraction})),
      controls:{max_step_s:0.01,max_steps:32,max_newton:100,
        velocity_tolerance_m_s:1e-9,force_tolerance_n:1e-6}};
    const raw=` ${JSON.stringify(input)}\n`, w=await workerModule({bindings:ftx});
    const result=await w.send({type:'stripHistory',stripRequest:raw,request:200});
    assert.equal(result.type,'result',result.message);
    assert.deepEqual(result.result,JSON.parse(ftx.material_strip_history(raw)));
    assert.match(result.result.model_scope,/parallel/);
    const last=result.result.states.at(-1);
    assert.ok(last.maxwell_dissipation_j>0); assert.ok(last.plastic_dissipation_j>0);
    assert.ok(last.relaxation&&last.plasticity);
    assert.ok(result.result.states.some(state=>state.solve?.roundoff_steps>0));
    for (const state of result.result.states.slice(1)) {
      assert.equal(state.solve.converged,true);
      assert.ok(state.solve.last_step/state.time_step_s<1e-9);
      assert.ok(state.free_dynamic_residual_n<=1e-6);
    }
    assert.equal(result.files[1].text,raw);
    const replay=result.files.find(file=>file.name==='strip-history-replay.json').text;
    assert.equal(replay,result.result.replay_request_json);
    assert.deepEqual(JSON.parse(ftx.material_strip_history(replay)),result.result);
    if(process.env.FTX_JOINT_HISTORY_REPLAY) {
      assert.deepEqual(result.result,JSON.parse(fs.readFileSync(process.env.FTX_JOINT_HISTORY_REPLAY,'utf8')));
    }
    delete input.material.record.membrane_relaxation.parallel_plasticity;
    const refused=await w.send({type:'stripHistory',stripRequest:JSON.stringify(input),request:201});
    assert.equal(refused.type,'error');
    assert.equal(w.sent.some(message=>message.type==='result'&&message.request===201),false);
  });

test('compiled WASM adaptive synthesis replays both native selected-body scenarios and exact intermediate patterns',
  {skip:process.env.FTX_REAL_WASM!=='1' || !process.env.FTX_DESIGN_ADAPTIVE_REPLAY}, async()=>{
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const dir=process.env.FTX_DESIGN_ADAPTIVE_REPLAY, input=name=>fs.readFileSync(path.join(dir,name),'utf8');
    const bytes=fs.readFileSync(path.join(dir,'model.ftxbody')), w=await workerModule({bindings:ftx});
    const previousProgress=globalThis.ftxProgress; globalThis.ftxProgress=()=>{};
    try {
      for (const name of ['single','matrix']) {
        const studyRequest=input('study.json'), scenarioRequest=input(`${name}-scenario.json`);
        const result=await w.send({type:'scenario',request:180,pattern:input('pattern.json'),studyRequest,scenarioRequest,
          measurements:input('body-input.json'),bytes:bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength)});
        assert.equal(result.type,'result',result.message);
        assert.deepEqual(result.result,JSON.parse(input(`${name}-report.json`)));
        const search=result.result.manufacturing.design_search;
        assert.equal(search.schema,'ftx-design-adaptive-search/1'); assert.equal(search.selected_designs,1);
        assert.equal(search.complete_search,false); assert.equal(search.global_optimum_claim,false);
        const selected=search.rows.find(row=>row.selected);
        assert.ok(Math.abs(selected.design.variables_mm.length-33.7)<=0.001);
        assert.equal(selected.max_fit_band_violation_mm,0);
        assert.equal(result.files.find(file=>file.name==='study-request.json').text,studyRequest);
        assert.equal(result.files.find(file=>file.name==='scenario-request.json').text,scenarioRequest);
      }
    } finally {globalThis.ftxProgress=previousProgress;}
  });

test('compiled WASM evaporating, supplied and condensing textile-water objectives replay native commands and named scenarios',
  {skip:process.env.FTX_REAL_WASM!=='1' || !(process.env.FTX_COUPLED_WATER_REPLAY || process.env.FTX_SUPPLIED_WATER_REPLAY || process.env.FTX_CONDENSED_WATER_REPLAY || process.env.FTX_VARIABLE_WATER_REPLAY)}, async()=>{
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const w=await workerModule({bindings:ftx});
    const previousProgress=globalThis.ftxProgress;globalThis.ftxProgress=()=>{};
    try {
      for(const [dir,kind] of [[process.env.FTX_COUPLED_WATER_REPLAY,'initial'],[process.env.FTX_SUPPLIED_WATER_REPLAY,'supplied'],
        [process.env.FTX_CONDENSED_WATER_REPLAY,'condensed'],[process.env.FTX_VARIABLE_WATER_REPLAY,'variable']].filter(([dir])=>dir)) {
        const input=name=>fs.readFileSync(path.join(dir,name),'utf8');
        const bytes=fs.readFileSync(path.join(dir,'model.ftxbody'));
        for(const name of ['single','matrix']) {
        const studyRequest=input('study.json'), scenarioRequest=input(`${name}-scenario.json`);
        const result=await w.send({type:'scenario',request:181,pattern:input('pattern.json'),studyRequest,scenarioRequest,
          measurements:input('body-input.json'),bytes:bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength)});
        assert.equal(result.type,'result',result.message);
        assert.deepEqual(result.result,JSON.parse(input(`${name}-report.json`)));
        assert.deepEqual(result.result.comparison.frontier,['qualified']);
        if(name==='single') {
          const readings=result.result.comparison.alternatives.find(row=>row.key==='qualified').readings;
          if(kind==='supplied') assert.ok(readings.textile_water_supplied_mass_kg.value>0);
          else if(kind==='condensed') {
            assert.ok(readings.textile_water_condensed_mass_kg.value>0);
            assert.ok(readings.textile_water_condensation_latent_energy_j.value>0);
          }
          else {
            assert.ok(readings.transient_surface_final_min_temperature_c.value<20);
            assert.equal(readings.textile_water_remaining_mass_kg.value,0);
          }
          assert.ok(readings.textile_water_evaporated_mass_kg.value>0);
          if(kind==='variable') {
            const heat=result.result.simulations.find(row=>row.key==='qualified').evaluation.result.motion.result.transient_surface.result;
            assert.ok(heat.model.columns.length>0);
            for(const column of heat.model.columns) {
              assert.equal(column.schema,'ftx-dry-thermal-model/2');
              assert.deepEqual(column.temperature_domain_c,[0,60]);
              assert.equal(column.cell_capacity_j_m2_k,undefined);
            }
          }
        }
        assert.equal(result.files.find(file=>file.name==='study-request.json').text,studyRequest);
        assert.equal(result.files.find(file=>file.name==='scenario-request.json').text,scenarioRequest);
        }
      }
    } finally {globalThis.ftxProgress=previousProgress;}
  });
