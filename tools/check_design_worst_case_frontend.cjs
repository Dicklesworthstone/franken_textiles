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
  vm.runInContext(source.slice(start, end), context);
  context.scenarioMatrixComparison('scenario', parent, report);
  return parent.children.map(child => child.textContent);
}

test('matrix page displays authoritative limiting-case notes without recomputing a score', () => {
  const note = "Worst-case retained trial 'adaptive:0003' on material 'light': limiting case '<guard>', simulated_cloth_mass_kg = 31 kg. This is the original admitted case reading, not an average.";
  const report = { schema: 'ftx-design-scenario-matrix-study/1', cases: [], notes: [note],
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
      for (const label of ['worst', 'worst-budget']) {
        const model = new ftx.WebBody(new Uint8Array(bytes));
        let report;
        try {
          report = JSON.parse(ftx.material_scenario_study(model, read('pattern.json'),
            read(`${label}-study.json`), read(`${label}-scenario.json`), read('body-input.json')));
        } finally { model.free(); }
        assert.deepEqual(report, JSON.parse(read(`${label}-report.json`)));
        const search = report.manufacturing.design_search;
        const selected = search.rows.find(row => row.selected);
        assert.equal(search.consumer_guidance.case_aggregation, 'worst_case');
        assert.equal(search.consumer_guidance.best_case_key, 'weight');
        assert.equal(selected.design.variables_mm.length, label === 'worst' ? -20 : 0);
        assert.equal(search.global_optimum_claim, false);
        const note = report.notes.find(text => text.startsWith('Worst-case retained trial'));
        assert.ok(note.includes("limiting case 'weight'"));
        assert.ok(displayedNotes(report).includes(note));
        if (label === 'worst-budget') assert.ok(search.rows.some(row =>
          row.consumer_cases?.some(item => item.refusal?.code === 'budget_exceeded')));
      }
    } finally { globalThis.ftxProgress = previous; }
  });
