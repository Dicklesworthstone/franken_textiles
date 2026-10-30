// Exercise the existing matrix renderer, then optionally the actual compiled Rust/WASM API.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { pathToFileURL } = require('node:url');

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
      for (const label of ['worst', 'worst-budget', 'worst-bounds', 'worst-bounds-budget']) {
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
