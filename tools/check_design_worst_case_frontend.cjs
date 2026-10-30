// Exercise the existing matrix renderer, then optionally the actual compiled Rust/WASM API.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { pathToFileURL } = require('node:url');

test('compiled WASM retains signed Pareto tradeoffs under independent case guards and quotas',
  { skip: process.env.FTX_REAL_WASM !== '1' || !process.env.FTX_CONSUMER_ADAPTIVE_REPLAY }, async () => {
    const pkg = process.env.FTX_WASM_PKG ? path.resolve(process.env.FTX_WASM_PKG) : path.join(__dirname, '../web/pkg');
    const ftx = await import(pathToFileURL(path.join(pkg, 'ftx_wasm.js')).href);
    ftx.initSync({ module: fs.readFileSync(path.join(pkg, 'ftx_wasm_bg.wasm')) });
    const dir = path.join(process.env.FTX_CONSUMER_ADAPTIVE_REPLAY, 'guarded');
    const read = name => fs.readFileSync(path.join(dir, name), 'utf8');
    const bytes = fs.readFileSync(path.join(dir, 'model.ftxbody'));
    const previous = globalThis.ftxProgress;
    globalThis.ftxProgress = () => {};
    try {
      for (const label of ['pareto-guarded', 'pareto-guarded-budget',
        'matrix-pareto-guarded', 'matrix-pareto-guarded-budget']) {
        const limited = label.endsWith('budget'), allCases = label.startsWith('matrix-');
        const model = new ftx.WebBody(new Uint8Array(bytes));
        let report;
        try {
          report = JSON.parse(ftx.material_scenario_study(model, read('pattern.json'),
            read(`${label}-study.json`), read(`${label}-scenario.json`), read('body-input.json')));
        } finally { model.free(); }
        assert.deepEqual(report, JSON.parse(read(`${label}-report.json`)));
        const search = report.manufacturing.design_search, guidance = search.consumer_guidance;
        const selected = search.rows.filter(row => row.selected);
        assert.equal(selected.length, limited ? 1 : 2);
        assert.equal(guidance.objective, undefined);
        assert.equal(guidance.best_trial_key, null);
        assert.equal(guidance.best_reading, null);
        if (allCases) {
          assert.equal(guidance.case_aggregation, 'pareto');
          assert.equal(guidance.case_key, undefined);
          assert.equal(guidance.objectives, undefined);
          assert.deepEqual(guidance.case_policies.guide.objectives,
            JSON.parse(read(`${label}-study.json`)).comparison.objectives);
        } else assert.deepEqual(guidance.objectives, JSON.parse(read(`${label}-study.json`)).comparison.objectives);
        assert.deepEqual(guidance.pareto_frontier.retained_trial_keys, selected.map(row => row.design.key));
        assert.deepEqual(guidance.pareto_frontier.unretained_trial_keys, []);
        for (const row of selected) {
          const key = JSON.stringify([row.design.key, 'light']);
          const result = report.cases[0].simulations.find(item => item.key === key).evaluation.result;
          assert.equal(result.drape.settled, true);
          assert.ok(result.surface.result.steady.power_w < 0);
          assert.equal(row.consumer_cases[1].comparison.readings.independent_surface_power_w.value,
            result.surface.result.steady.power_w);
          if (allCases) assert.equal(row.consumer_comparison, undefined);
        }
        const rejected = search.rows.find(row => row.design.variables_mm.length === (limited ? -20 : -30));
        assert.equal(rejected.selected, false);
        assert.equal(rejected.consumer_cases[0].comparison.eligible, true);
        assert.equal(rejected.consumer_cases[1].comparison.eligible, false);
        if (limited) assert.equal(rejected.consumer_cases[1].refusal.code, 'budget_exceeded');
      }
    } finally { globalThis.ftxProgress = previous; }
  });

function displayedNotes(report) {
  const source = fs.readFileSync(path.join(__dirname, '../web/materials-page.js'), 'utf8');
  const start = source.indexOf('function scenarioMatrixComparison(');
  const end = source.indexOf('\nfunction designTreatments(', start);
  assert.ok(start >= 0 && end > start);
  const node = () => ({ textContent: '', children: [], append(...items) { this.children.push(...items); },
    set innerHTML(_) { assert.fail('Source-authored case names must be displayed as text'); } });
  const parent = node();
  const context = vm.createContext({ document: { createElement: node },
    comparison() {}, detail() {}, table: node, cells() {}, metric: String });
  const basisStart = source.indexOf('function metricUsesBounds(');
  const basisEnd = source.indexOf('\nfunction objectiveComparison(', basisStart);
  assert.ok(basisStart >= 0 && basisEnd > basisStart);
  vm.runInContext(source.slice(basisStart, basisEnd), context);
  vm.runInContext(source.slice(start, end), context);
  context.scenarioMatrixComparison('scenario', parent, report);
  return parent.children.map(child => child.textContent);
}

// Run the existing design renderer without inventing frontend objective or frontier arithmetic.
function displayedDesign(study) {
  const source = fs.readFileSync(path.join(__dirname, '../web/materials-page.js'), 'utf8');
  const start = source.indexOf('function designComparison(');
  const end = source.indexOf('\nfunction useStudyPattern(', start);
  assert.ok(start >= 0 && end > start);
  const node = () => ({ textContent: '', children: [], append(...items) { this.children.push(...items); },
    set innerHTML(_) { assert.fail('Design identities must be displayed as text'); } });
  const parent = node(), tables = new Map(), downloads = new Map();
  const context = vm.createContext({ document: { createElement: node },
    available: item => item?.status === 'available' ? item.result : null,
    state: item => item?.status || 'Unknown', metric: value => value == null ? 'Unknown' : String(value),
    table: (_parent, caption, headings) => {
      const table = { headings, rows: [] }; tables.set(caption, table); return table;
    }, cells: (table, values) => table.rows.push(Array.from(values, String)),
    detail() {}, designTreatments() {}, metricUsesBounds: policy => policy?.basis === 'reported_bounds',
    link: (_op, _parent, file) => downloads.set(file.name, file.text) });
  vm.runInContext(source.slice(start, end), context);
  context.designComparison('scenario', parent, study);
  return { tables, downloads, text: parent.children.map(child => child.textContent).join('\n') };
}

test('matrix page displays authoritative limiting-case notes without recomputing a score', () => {
  const note = "Worst-case retained trial 'adaptive:0003' on material 'light': limiting case '<guard>', simulated_cloth_mass_kg = 31 kg. This is the original admitted case reading, not an average.";
  const report = { schema: 'ftx-design-scenario-matrix-study/1', cases: [], notes: [note],
    comparison: { frontier: ['trial'], case_order: ['guard'], incomparable: [],
      policies: { guard: { basis: 'reported_bounds', objectives: [{ metric: 'assembly_mass_kg' }],
        constraints: [{ metric: 'feed_seconds', basis: 'nominal', maximum: 10 }] } },
      alternatives: [{ key: 'trial', eligible: true, dominated_by: [], cases: { guard: {
        eligible: true, readings: { assembly_mass_kg: { value: 1, unit: 'kg' }, feed_seconds: { value: 5, unit: 's' } } } } }] },
    manufacturing: { design_search: { consumer_guidance: { best_reading: { value: 999 } } } } };
  assert.ok(displayedNotes(report).includes(note));
  assert.ok(!displayedNotes(report).some(text => text.includes('999')));
});

test('matrix page preserves no-incumbent explanation without a zero or invented limiting case', () => {
  const note = 'No worst-case trial was admitted in every named case. No limiting case or objective value is available; inspect the retained per-case refusals.';
  const report = { schema: 'ftx-design-scenario-matrix-study/1', cases: [], notes: [note],
    manufacturing: {} };
  assert.ok(displayedNotes(report).includes(note));
});

test('matrix page keeps distinct nominal and interval witnesses in authoritative range notes', () => {
  const note = "Worst-case retained trial 'adaptive:0003': nominal limiting case '<nominal>', assembly_mass_kg = 8 kg; reported worst-case range [7, 12] kg, with lower endpoint from '<lower>' and upper endpoint from '<upper>'. Only strictly separated ranges establish improvement; overlaps and touching endpoints remain unresolved.";
  const report = { schema: 'ftx-design-scenario-matrix-study/1', cases: [], notes: [note],
    manufacturing: { design_search: { consumer_guidance: {
      basis: 'reported_bounds', best_case_key: '<nominal>', best_reported_bound_case_keys: ['<lower>', '<upper>'],
      best_reading: { value: 8, reported_bounds: [7, 12], unit: 'kg' }
    } } } };
  assert.ok(displayedNotes(report).includes(note));
});

test('Pareto renderer preserves the evaluated frontier separately from retained geometry', () => {
  const study = { alternatives: [{key:'bound',design:{key:'first'}}], design_search: {
    schema:'ftx-design-adaptive-search/1',evaluations:2,selected_designs:1,stop_reason:'minimum_steps',
    consumer_guidance:{material_key:'<material>',evaluations:2,reserved_evaluations:4,
      objectives:[{metric:'assembly_mass_kg',direction:'minimize'},{metric:'independent_surface_power_w',direction:'maximize'}],
      pareto_frontier:{evaluated_trial_keys:['first','<omitted>'],retained_trial_keys:['first'],
        unretained_trial_keys:['<omitted>'],retention_capacity:1}},rows:[] } };
  const rendered=displayedDesign(study),summary=rendered.tables.get('Pareto-guided adaptive alteration search');
  assert.deepEqual(summary.rows[0].slice(5),['first, <omitted>','first','<omitted>']);
  assert.equal(rendered.tables.has('Consumer-guided adaptive alteration search'),false);
  assert.match(rendered.text,/assembly_mass_kg \(minimize\); independent_surface_power_w \(maximize\)/);
  assert.match(rendered.text,/retained subset is not a complete frontier or a single winner/);
});

test('compiled WASM Pareto mass and cooling tradeoffs replay native results through the worker and renderer',
  { skip: process.env.FTX_REAL_WASM !== '1' || !process.env.FTX_CONSUMER_ADAPTIVE_REPLAY }, async () => {
    const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(__dirname,'../web/pkg');
    const ftx=await import(pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const input=name=>fs.readFileSync(path.join(process.env.FTX_CONSUMER_ADAPTIVE_REPLAY,name),'utf8');
    const bytes=fs.readFileSync(path.join(process.env.FTX_CONSUMER_ADAPTIVE_REPLAY,'model.ftxbody'));
    const sent=[],context=vm.createContext({Uint8Array,TextEncoder,WebAssembly,JSON,Error,self:{},postMessage:message=>sent.push(message)});
    const bindings=new vm.SyntheticModule(Object.keys(ftx),function(){
      for(const [name,value] of Object.entries(ftx))this.setExport(name,value);
    },{context});
    const worker=new vm.SourceTextModule(fs.readFileSync(path.join(__dirname,'../web/materials-worker.js'),'utf8'),{context});
    await worker.link(()=>bindings);await worker.evaluate();
    const previous=globalThis.ftxProgress;
    globalThis.ftxProgress=line=>context.self.ftxProgress(line);
    try {
      for(const [index,label] of ['pareto','pareto-budget'].entries()) {
        const studyRequest=input(`${label}-study.json`),scenarioRequest=input(`${label}-scenario.json`);
        await context.self.onmessage({data:{type:'scenario',request:270+index,pattern:input('pattern.json'),
          studyRequest,scenarioRequest,measurements:input('body-input.json'),
          bytes:bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength)}});
        const result=sent.at(-1);assert.equal(result.type,'result',result.message);
        assert.deepEqual(result.result,JSON.parse(input(`${label}-report.json`)));
        const search=result.result.manufacturing.design_search,guidance=search.consumer_guidance,
          frontier=guidance.pareto_frontier,limited=label.endsWith('budget');
        assert.deepEqual(guidance.objectives,JSON.parse(studyRequest).comparison.objectives);
        assert.equal(guidance.objective,undefined);assert.equal(guidance.best_reading,null);
        assert.equal(guidance.best_trial_key,null);assert.equal(search.global_optimum_claim,false);
        assert.equal(frontier.retained_trial_keys.length,limited?1:2);
        assert.equal(frontier.evaluated_trial_keys.length,frontier.retained_trial_keys.length+frontier.unretained_trial_keys.length);
        if(limited) assert.ok(search.rows.some(row=>row.consumer_refusal?.code==='budget_exceeded'&&!row.selected));
        else {
          const shortest=search.rows.find(row=>row.design.variables_mm.length===-30);
          assert.ok(frontier.unretained_trial_keys.includes(shortest.design.key));
          const initial=search.rows[0].consumer_comparison.readings;
          for(const metric of ['assembly_mass_kg','independent_surface_power_w'])
            assert.ok(shortest.consumer_comparison.readings[metric].value<initial[metric].value);
        }
        const rendered=displayedDesign(result.result.manufacturing);
        const summary=rendered.tables.get('Pareto-guided adaptive alteration search');
        assert.deepEqual(summary.rows[0].slice(5),[frontier.evaluated_trial_keys.join(', '),
          frontier.retained_trial_keys.join(', '),frontier.unretained_trial_keys.join(', ')||'None']);
        assert.equal(rendered.tables.has('Consumer-guided adaptive alteration search'),false);
        assert.match(rendered.text,/assembly_mass_kg \(minimize\); independent_surface_power_w \(maximize\)/);
        const rows=rendered.tables.get('Adaptive alteration trials').rows;
        for(const [index,trial] of search.rows.entries()) {
          const displayed=rows.find(row=>row[0]===trial.design.key);
          assert.equal(displayed[8],trial.selected?'Selected':'Not selected');
          assert.match(displayed[6],/assembly_mass_kg:/);assert.match(displayed[6],/independent_surface_power_w:/);
          if(frontier.evaluated_trial_keys.includes(trial.design.key))assert.equal(displayed[7],'Nondominated among evaluated trials');
          assert.equal(rendered.downloads.get(`adaptive-design-${index+1}-pattern.json`),trial.design.altered_pattern_json);
          if(!trial.selected)continue;
          const key=JSON.stringify([trial.design.key,'light']);
          const simulation=result.result.cases[0].simulations.find(row=>row.key===key).evaluation;
          assert.equal(simulation.status,'available');assert.equal(simulation.result.surface.status,'available');
          const heat=simulation.result.surface.result.steady;
          assert.equal(heat.power_w,trial.consumer_comparison.readings.independent_surface_power_w.value);
          assert.ok(Math.abs(heat.power_w-heat.area_m2*30/(0.1+0.2+0.0004/0.05))<1e-10);
        }
        assert.equal(result.result.evaluations,guidance.evaluations+
          result.result.manufacturing.alternatives.length-frontier.retained_trial_keys.length);
        assert.deepEqual(JSON.parse(result.files.find(file=>file.name==='design-scenario-matrix-study.json').text),result.result);
        assert.equal(result.files.find(file=>file.name==='study-request.json').text,studyRequest);
        assert.equal(result.files.find(file=>file.name==='scenario-request.json').text,scenarioRequest);
        assert.equal(result.files.find(file=>file.name==='body-input.json').text,input('body-input.json'));
      }
    } finally {
      if(previous===undefined)delete globalThis.ftxProgress;else globalThis.ftxProgress=previous;
    }
  });

test('compiled WASM worst-case sewn-body studies reproduce native constraints and cumulative budget refusals',
  { skip: process.env.FTX_REAL_WASM !== '1' || !process.env.FTX_CONSUMER_ADAPTIVE_REPLAY }, async () => {
    const pkg = process.env.FTX_WASM_PKG ? path.resolve(process.env.FTX_WASM_PKG) : path.join(__dirname, '../web/pkg');
    const ftx = await import(pathToFileURL(path.join(pkg, 'ftx_wasm.js')).href);
    ftx.initSync({ module: fs.readFileSync(path.join(pkg, 'ftx_wasm_bg.wasm')) });
    const read = name => fs.readFileSync(path.join(process.env.FTX_CONSUMER_ADAPTIVE_REPLAY, name), 'utf8');
    const bytes = fs.readFileSync(path.join(process.env.FTX_CONSUMER_ADAPTIVE_REPLAY, 'model.ftxbody'));
    const previous = globalThis.ftxProgress;
    globalThis.ftxProgress = () => {};
    try {
      for (const label of ['worst', 'worst-budget', 'worst-bounds', 'worst-bounds-budget',
        'restart-single', 'restart-complete', 'restart-budget']) {
        const bounded = label.includes('bounds'), limited = label.endsWith('budget');
        const model = new ftx.WebBody(new Uint8Array(bytes));
        let report;
        try {
          report = JSON.parse(ftx.material_scenario_study(model, read('pattern.json'),
            read(`${label}-study.json`), read(`${label}-scenario.json`), read('body-input.json')));
        } finally { model.free(); }
        assert.deepEqual(report, JSON.parse(read(`${label}-report.json`)));
        const search = report.manufacturing.design_search;
        const selected = search.rows.find(row => row.selected);
        const guidance = search.consumer_guidance;
        assert.equal(guidance.case_aggregation, 'worst_case');
        if (label.startsWith('restart-')) {
          assert.equal(search.global_optimum_claim, false);
          assert.equal(search.complete_search, false);
          assert.equal(guidance.reserved_evaluations, 24);
          if (label === 'restart-single') {
            assert.equal(search.search_starts, undefined);
            assert.equal(search.evaluations, 2);
          } else {
            assert.equal(search.search_starts.length, 3);
            assert.equal(search.search_starts.reduce((n, start) => n + start.evaluations, 0), search.evaluations);
            assert.equal(search.search_starts[0].stop_reason, 'minimum_steps');
          }
          if (label === 'restart-complete') {
            assert.equal(search.evaluations, 5);
            assert.equal(selected.design.variables_mm.length, -5);
            assert.equal(search.search_starts[2].evaluations, 0);
            assert.equal(search.rows[4].design.variables_mm.length, -10);
            assert.equal(search.rows[4].consumer_cases[0].comparison.eligible, false);
            assert.match(search.rows[4].refusal.message, /geometrically settled/);
          } else {
            assert.equal(selected, undefined);
            assert.equal(guidance.best_reading, null);
            if (limited) assert.equal(search.rows[2].consumer_cases[1].refusal.code, 'budget_exceeded');
          }
          continue;
        }
        assert.equal(guidance.best_case_key, 'weight');
        assert.equal(selected.design.variables_mm.length, limited ? 0 : -20);
        assert.equal(search.global_optimum_claim, false);
        if (bounded) {
          assert.equal(guidance.basis, 'reported_bounds');
          assert.deepEqual(guidance.best_reported_bound_case_keys, ['weight', 'weight']);
          assert.deepEqual(selected.consumer_worst_case.reported_bound_case_keys, guidance.best_reported_bound_case_keys);
          assert.deepEqual(guidance.best_reading, selected.consumer_worst_case.reading);
          assert.ok(guidance.best_reading.reported_bounds[0] < guidance.best_reading.value);
          assert.ok(guidance.best_reading.value < guidance.best_reading.reported_bounds[1]);
          assert.deepEqual(selected.consumer_comparison, selected.consumer_cases[0].comparison);
          const expectedArea = Math.PI * 2 * 180 * (100 + selected.design.variables_mm.length) * 1e-6;
          for (const [i, density] of [0.19, 0.21].entries()) assert.ok(
            Math.abs(guidance.best_reading.reported_bounds[i] - expectedArea * density) < 1e-6);
          const guard = selected.consumer_cases[1].comparison.readings.assembly_mass_kg;
          assert.ok(guard.reported_bounds[0] >= 0.0155);
        } else {
          assert.equal(guidance.best_reported_bound_case_keys, undefined);
          assert.equal(selected.consumer_worst_case, undefined);
        }
        const note = report.notes.find(text => text.startsWith('Worst-case retained trial'));
        assert.ok(note.includes("limiting case 'weight'"));
        assert.ok(displayedNotes(report).includes(note));
        if (bounded) {
          assert.ok(note.includes("lower endpoint from 'weight'"));
          assert.ok(note.includes("upper endpoint from 'weight'"));
          assert.ok(note.includes('overlaps and touching endpoints remain unresolved'));
        }
        if (limited) assert.ok(search.rows.some(row =>
          row.consumer_cases?.some(item => item.refusal?.code === 'budget_exceeded')));
      }
    } finally { globalThis.ftxProgress = previous; }
  });
