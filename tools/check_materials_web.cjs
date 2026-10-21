// Actual Chromium + compiled WASM, compared with the real native materials executable.
// No model asset is bundled: successful body fitting/drape requires the separate existing model lane.
// tools/build_web.sh && cargo build -p ftx-cli
// NODE_PATH=$(npm root -g) node tools/check_materials_web.cjs --native target/debug/ftx-materials
// Optional CHROMIUM_EXECUTABLE chooses an already installed Chromium binary.
// --strip-proof DIR runs only strip browser checks against retained native reports (manifest.json).
// --outfit-proof checks public-prior drafted garment composition with synthetic materials, without a model.
const { chromium } = require(process.env.FTX_PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');
const { execFileSync } = require('node:child_process');
const fs = require('node:fs');
const http = require('node:http');
const os = require('node:os');
const path = require('node:path');
const { createHash } = require('node:crypto');
const root = path.resolve(__dirname, '..'), web = path.join(root, 'web');
const index = process.argv.indexOf('--native');
const native = path.resolve(index < 0 ? path.join(root, 'target/debug/ftx-materials') : process.argv[index + 1]);
const stripIndex = process.argv.indexOf('--strip-proof');
if (stripIndex >= 0 && !process.argv[stripIndex + 1]) throw Error('--strip-proof requires a native report directory');
const stripProof = stripIndex < 0 ? null : path.resolve(process.argv[stripIndex + 1]);
const outfitProof = process.argv.includes('--outfit-proof');
assert.ok(!(stripProof && outfitProof), 'Choose one focused browser check');
const fixture = n => path.join(root, 'tests/fixtures/materials', n);
const load = n => JSON.parse(fs.readFileSync(fixture(n), 'utf8'));
const work = fs.mkdtempSync(path.join(os.tmpdir(), 'ftx-materials-browser-'));
const write = (name, value) => { const file = path.join(work, name); fs.writeFileSync(file, JSON.stringify(value)); return file; };
const invoke = (...args) => execFileSync(native, args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
function sheet(width, thickness) {
  const s = load('synthetic-sewing-pattern.json').fabric.material;
  for (const [key, value, unit] of [
    ['width',width,'m'], ['thickness',thickness,'mm'], ['areal_density',0.2,'kg/m2'],
    ['warp_stiffness',8000,'N/m'], ['weft_stiffness',4000,'N/m'], ['poisson_warp_weft',0.2,'1'],
    ['shear_stiffness',1000,'N/m'], ['bending_rigidity',1e-4,'N*m'], ['friction_skin',0.3,'1'], ['friction_self',0.3,'1']
  ]) s.record.properties[key] = { value, unit, evidence:'estimate', source:'test', method:'Artificial browser software test, not measured data' };
  return s;
}
const server = http.createServer((req, res) => {
  if (req.url === '/favicon.ico') { res.writeHead(204); return res.end(); }
  const target = path.resolve(web, '.' + new URL(req.url, 'http://localhost').pathname);
  if (!target.startsWith(web + path.sep) && target !== web) { res.writeHead(403); return res.end(); }
  const file = target === web ? path.join(web, 'index.html') : target;
  const mime = { '.html':'text/html', '.js':'text/javascript', '.wasm':'application/wasm' }[path.extname(file)] || 'application/octet-stream';
  fs.readFile(file, (e, data) => { res.writeHead(e ? 404 : 200, { 'Content-Type':mime }); res.end(e ? 'not found' : data); });
});
(async () => {
  assert.ok(fs.existsSync(path.join(web, 'pkg/ftx_wasm_bg.wasm')), 'Build web/pkg first');
  if (!stripProof && !outfitProof) assert.ok(fs.existsSync(native), 'Build the native ftx-materials first');
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_EXECUTABLE || undefined, args:['--no-sandbox'] });
  const page = await browser.newPage({ viewport:{width:1280,height:900},
    userAgent:'OpenAI File Downloader, XaiImageApiFetch/1.0' }), errors = [], external = [], methods = [];
  page.on('pageerror', e => errors.push(String(e)));
  page.on('console', m => { if (m.type() === 'error') errors.push(m.text()); });
  page.on('request', r => { methods.push(r.method()); if (!r.url().startsWith(origin) && !r.url().startsWith('blob:')) external.push(r.url()); });
  const input = async (id, value) => page.setInputFiles('#'+id, { name:id+'.json', mimeType:'application/json', buffer:Buffer.from(JSON.stringify(value)) });
  const run = async id => {
    await page.click('#'+id);
    const op = id === 'search' ? 'catalog' : id;
    await page.waitForFunction(op => new RegExp(`^${op} (complete|refused)`).test(document.getElementById('status').textContent), op, { timeout:120000 });
    return await page.locator('#error').isVisible() ? page.textContent('#error') : '';
  };
  const download = name => page.locator(`a[download="${name}"]`).evaluate(async a => await (await fetch(a.href)).text());
  const assign = async selection => {
    await page.fill('#selection', JSON.stringify(selection)); assert.equal(await run('inspect'), '');
    await page.click('#assign'); assert.equal(await page.locator('a[download="cut-plan.json"]').count(), 0);
    assert.equal(await run('bind'), ''); return JSON.parse(await download('pattern.json'));
  };
  try {
    await page.goto(origin); await page.click('a[href="materials.html"]');
    await page.waitForFunction(() => document.getElementById('status').textContent.startsWith('Ready'), null, {timeout:60000});
    if (outfitProof) {
      const drafts = JSON.parse(fs.readFileSync(path.join(root,'tests/fixtures/oracle/pattern/drafts.json'),'utf8')).cases;
      const request = load('synthetic-joint-outfit.json'), fabric = request.garments[0].pattern.fabric;
      for (const [i,id] of ['zouave_bolero-920','zouave_trousers-920'].entries()) {
        request.garments[i].id = ['jacket','trousers'][i];
        request.garments[i].pattern = structuredClone(drafts.find(c => c.id === id).input.pattern);
        request.garments[i].pattern.fabric = structuredClone(fabric);
      }
      request.interfaces = [];
      const original = JSON.stringify(request);
      await input('patternFile',request);
      await page.waitForFunction(() => /^outfit (complete|refused)/.test(document.getElementById('status').textContent));
      assert.equal(await page.locator('#error').isVisible(),false,await page.textContent('#error'));
      assert.match(await page.textContent('#patternName'),/2 garments in one composed outfit/);
      assert.equal(await page.isChecked('#selfContact'),true); assert.equal(await page.isDisabled('#selfContact'),true);
      assert.equal(await download('outfit-request.json'),original);
      const raw = await download('outfit-pattern.json'), pattern = JSON.parse(raw);
      const casings = pattern.meta.solved.elastic_casings, source = request.garments[1].pattern.meta;
      assert.equal(casings.length,1); assert.ok(casings[0].instance.startsWith('trousers::'));
      assert.ok(casings[0].load_instances.length > 0 && casings[0].load_instances.every(i => i.startsWith('trousers::')));
      assert.equal(casings[0].relaxed_m,(source.solved.elastic_cut_mm-source.style.elastic_overlap)*0.001);
      fs.writeFileSync(path.join(work,'outfit-pattern.json'),raw,{flag:'wx'});
      fs.writeFileSync(path.join(work,'outfit-request.json'),original,{flag:'wx'});
      await page.screenshot({path:path.join(work,'drafted-outfit-import.png'),fullPage:true});
      assert.deepEqual(errors,[]); assert.deepEqual(external,[]); assert.ok(methods.every(m => m==='GET'));
      console.log('PASS: real Chromium imports drafted bolero/trousers, retains exact original request and independent casing rest/load scope, and locks required self-contact; no errors or uploads. Composition only, synthetic material; no drape or fit claim. Artifacts:',work);
      return;
    }
    if (stripProof) {
      const sha = bytes => createHash('sha256').update(bytes).digest('hex');
      const manifest = JSON.parse(fs.readFileSync(path.join(stripProof,'manifest.json'),'utf8'));
      assert.match(manifest.source_commit,/^[a-f0-9]{40}$/);
      assert.match(manifest.binary_sha256,/^[a-f0-9]{64}$/);
      for (const [file, digest] of Object.entries(manifest.inputs))
        assert.equal(sha(fs.readFileSync(path.join(root,file))),digest,`Changed native input: ${file}`);
      const reference = name => {
        const raw = fs.readFileSync(path.join(stripProof,name));
        assert.equal(sha(raw),manifest.reports[name],`Changed native report: ${name}`);
        return JSON.parse(raw);
      };
      const retain = (name, raw) => fs.writeFileSync(path.join(work,name),raw,{flag:'wx'});
      await page.fill('#query','mit.raw-2013.');
      assert.equal(await run('search'),'');
      assert.equal(await page.locator('#catalog option').count(),52);
      const acquisition = path.join(root,'crates/ftx-pattern/src/material_catalog/knit-strip-measurements.json');
      await page.locator('details').filter({has:page.locator('#stripAcquisition')}).evaluate(e => {e.open=true;});
      await page.setInputFiles('#stripAcquisitionRequest',acquisition);
      assert.equal(await run('stripAcquisition'),'');
      const acquiredRaw = await download('strip-acquisition-report.json'), acquired = JSON.parse(acquiredRaw);
      assert.deepEqual(acquired,reference('acquisition.json'));
      assert.equal(await download('strip-acquisition-input.json'),fs.readFileSync(acquisition,'utf8'));
      assert.equal(acquired.curves.length,9); assert.equal(acquired.curves[0].observations.length,916);
      assert.equal(acquired.curves[0].observations.at(-1).branch,'unloading');
      assert.equal(acquired.transverse_curves.length,9); assert.equal(acquired.selection,undefined);
      assert.match(await page.locator('#outputs').textContent(),/not converted to widths, local strains or Poisson ratios/);
      retain('acquisition.json',acquiredRaw);
      await page.setInputFiles('#patternFile',fixture('synthetic-sewing-pattern.json'));
      await page.waitForFunction(() => document.getElementById('patternName').textContent.includes('Original retained'));
      await page.selectOption('#calibrationMode','clamped_strip');
      await page.setInputFiles('#calibrationRequest',fixture('synthetic-clamped-strip.json'));
      for (const admission of [false,true]) {
        await page.setChecked('#calibrationEstimates',admission);
        assert.equal(await page.isDisabled('#assign'),true);
        assert.equal(await page.locator('a[download="material-calibration.json"]').count(),0);
        assert.equal(await run('calibrate'),'');
        const raw = await download('material-calibration.json'), report = JSON.parse(raw);
        const expected = reference(admission ? 'admitted.json' : 'denied.json');
        for (const field of ['calibration','selection','selection_json']) assert.deepEqual(report[field],expected[field],field);
        assert.equal(report.calibration.accepted,true); assert.equal(report.calibration.domain_replay_verified,true);
        assert.equal(report.calibration.fit.components,12); assert.equal(report.calibration.check.components,2);
        assert.equal(report.selection.allow_estimates,admission);
        assert.equal(report.material.capabilities.mechanical_coefficients.ready,false);
        assert.equal(await download('calibration-input.json'),fs.readFileSync(fixture('synthetic-clamped-strip.json'),'utf8'));
        assert.equal(await download('calibrated-selection.json'),report.selection_json);
        assert.equal(await page.inputValue('#selection'),report.selection_json);
        for (const caption of ['Clamped strip observations — independent partitions',
          'Identified orthotropic sheet coefficients','Actual strip geometry and declared width comparison',
          'Actual refined strip predictions and observation residuals'])
          assert.equal(await page.locator('caption').filter({hasText:caption}).count(),1);
        const observed = page.locator('table').filter({has:page.locator('caption',{
          hasText:'Clamped strip observations — independent partitions'})});
        for (const [i,key] of ['fit','check'].entries()) {
          const q = report.calibration[key];
          assert.deepEqual(await observed.locator('tr').nth(i+1).locator('td').allTextContents(),
            [key.toUpperCase(),q.components,q.rms_scaled,q.peak_scaled].map(String));
        }
        await page.click('#assign'); assert.equal(await run('bind'),'');
        const bound = JSON.parse(await download('pattern.json')), original = load('synthetic-sewing-pattern.json');
        assert.deepEqual(bound.pieces,original.pieces); assert.deepEqual(bound.seams,original.seams);
        assert.equal(bound.fabric.material.expected_sha256,report.selection.expected_sha256);
        const binding = JSON.parse(await download('material-bindings.json'));
        assert.equal(binding.geometry_changed,false); assert.equal(binding.drape_materials_with_self_contact.ready,false);
        retain(`accepted-${admission}.json`,raw); retain(`pattern-${admission}.json`,await download('pattern.json'));
      }
      // The dedicated input takes estimate policy from its file, independently of the generic checkbox.
      await page.setChecked('#calibrationEstimates',false);
      await page.setInputFiles('#stripFitRequest',fixture('synthetic-clamped-strip.json'));
      assert.equal(await run('stripFit'),'');
      const dedicatedRaw = await download('strip-calibration-report.json'), dedicated = JSON.parse(dedicatedRaw);
      for (const field of ['calibration','selection','selection_json'])
        assert.deepEqual(dedicated[field],reference('admitted.json')[field],`Dedicated strip ${field}`);
      assert.equal(await download('strip-calibration-input.json'),fs.readFileSync(fixture('synthetic-clamped-strip.json'),'utf8'));
      assert.equal(await download('strip-calibrated-selection.json'),dedicated.selection_json);
      assert.equal(await page.inputValue('#selection'),dedicated.selection_json);
      assert.equal(await page.isDisabled('#assign'),false);
      retain('dedicated-strip.json',dedicatedRaw);
      const nonlinearInput = path.join(root,'examples/strip-identification/synthetic-nonlinear-calibration.json');
      await page.setInputFiles('#stripFitRequest',nonlinearInput);
      assert.equal(await run('stripFit'),'');
      const nonlinearRaw = await download('strip-calibration-report.json'), nonlinear = JSON.parse(nonlinearRaw);
      for (const field of ['calibration','selection','selection_json'])
        assert.deepEqual(nonlinear[field],reference('nonlinear.json')[field],`Nonlinear strip ${field}`);
      assert.equal(nonlinear.calibration.accepted,true);
      assert.equal(await download('strip-calibration-input.json'),fs.readFileSync(nonlinearInput,'utf8'));
      assert.equal(await download('strip-calibrated-selection.json'),nonlinear.selection_json);
      const coefficients = page.locator('table').filter({has:page.locator('caption',{
        hasText:'Identified nonlinear shape scales and coupling'})});
      for (const [i,d] of nonlinear.calibration.parameter_descriptors.entries())
        assert.deepEqual(await coefficients.locator('tr').nth(i+1).locator('td').allTextContents(),
          [d.label,nonlinear.calibration.parameters[i],d.unit,...nonlinear.calibration.input.parameter_bounds[i]].map(String));
      assert.match(await page.locator('#outputs').textContent(),/does not identify the curve shapes or impose area preservation/);
      retain('nonlinear-strip.json',nonlinearRaw);
      await page.screenshot({path:path.join(work,'accepted.png'),fullPage:true});
      const unfinished = load('synthetic-clamped-strip.json'); unfinished.policy.max_iterations=1;
      await input('stripFitRequest',unfinished);
      assert.equal(await page.isDisabled('#assign'),true);
      assert.equal(await page.locator('a[download="strip-calibrated-selection.json"]').count(),0);
      await input('calibrationRequest',unfinished);
      assert.equal(await page.isDisabled('#assign'),true);
      assert.equal(await page.locator('a[download="calibrated-selection.json"]').count(),0);
      assert.equal(await run('calibrate'),'');
      const unfinishedRaw = await download('material-calibration.json'), rejected = JSON.parse(unfinishedRaw);
      assert.equal(rejected.calibration.accepted,false); assert.ok(rejected.calibration.reasons.length>0);
      assert.ok(rejected.calibration.starts.length>0); assert.equal(rejected.selection_json,null);
      assert.equal(await page.isDisabled('#assign'),true);
      assert.equal(await page.locator('a[download="calibrated-selection.json"]').count(),0);
      assert.match(await page.locator('#outputs').textContent(),/NOT ACCEPTED/);
      retain('unaccepted.json',unfinishedRaw);
      await page.screenshot({path:path.join(work,'unaccepted.png'),fullPage:true});
      const exhausted = load('synthetic-clamped-strip.json'); exhausted.policy.max_forward_solves=1;
      await input('calibrationRequest',exhausted);
      assert.equal(await page.locator('a[download="material-calibration.json"]').count(),0);
      assert.match(await run('calibrate'),/budget/i);
      assert.equal(await page.isDisabled('#assign'),true);
      assert.equal(await page.locator('a[download="calibrated-selection.json"]').count(),0);
      assert.deepEqual(errors,[]); assert.deepEqual(external,[]); assert.ok(methods.every(m => m==='GET'));
      const fingerprint = {native:manifest,browser:browser.version(),node:process.version,files:{}};
      for (const file of ['web/pkg/ftx_wasm_bg.wasm','web/pkg/ftx_wasm.js','web/materials-page.js',
        'web/materials-worker.js','web/materials.html','tools/check_materials_web.cjs'])
        fingerprint.files[file]=sha(fs.readFileSync(path.join(root,file)));
      retain('fingerprint.json',JSON.stringify(fingerprint,null,2));
      console.log('PASS: real Chromium source acquisitions, complete native strip equality under both estimate policies, exact downloads, frozen pattern binding, unaccepted diagnostics, budget refusal and stale-result withdrawal.');
      console.log('Strip-only lane; no measured textile, garment fit, benchmark or full workbench claim. Artifacts:',work);
      return;
    }
    await page.fill('#query','cotton'); assert.equal(await run('search'), '');
    assert.ok(await page.locator('#catalog option').count() > 0);
    await page.fill('#selection', JSON.stringify({id:'fsri/hfm/cotton-sheet/dry/220330-r1',allow_estimates:true}));
    assert.equal(await run('inspect'), '');
    const caps = await page.locator('#capabilities').textContent();
    assert.match(caps, /steady conductionCoefficient inputs available/);
    assert.match(caps, /specific_heat|areal_density|characterization/);
    await page.setInputFiles('#patternFile', fixture('synthetic-sewing-pattern.json'));
    await page.waitForFunction(() => !document.getElementById('assign').disabled);
    for (const [mode, filename] of [['nonlinear','synthetic-nonlinear-swatch.json'],
      ['relaxation','synthetic-relaxation-history.json'], ['relaxation','synthetic-orthotropic-relaxation-history.json'],
      ['relaxation','synthetic-orthotropic-relaxation-search.json']]) {
      await page.selectOption('#calibrationMode',mode);
      await page.setInputFiles('#calibrationRequest',fixture(filename));
      for (const admission of [false,true]) {
        await page.setChecked('#calibrationEstimates',admission);
        assert.equal(await run('calibrate'),'');
        const report=JSON.parse(await download('material-calibration.json'));
        const expected=JSON.parse(invoke('calibrate-'+mode,fixture(filename),...(admission ? ['--allow-estimates'] : [])));
        assert.deepEqual(report.calibration,expected.calibration);
        assert.deepEqual(report.selection,expected.selection);
        assert.equal(report.material.capabilities.mechanical_coefficients.ready,admission);
        assert.equal(await download('calibration-input.json'),fs.readFileSync(fixture(filename),'utf8'));
        assert.equal(await download('calibrated-selection.json'),report.selection_json);
        assert.equal(await page.inputValue('#selection'),report.selection_json);
        assert.match(await page.locator('#outputs').textContent(),/Observed force errors/);
        if (report.calibration.orthotropic_fit) {
          const c=report.calibration, search=c.orthotropic_fit.time_search;
          assert.match(await page.locator('#outputs').textContent(),search ?
            /Directional recovery — published stiffness at identified times/ : /Directional recovery — published stiffness at supplied times/);
          if (search) {
            const windows=page.locator('table').filter({has:page.locator('caption',{
              hasText:'Directional time search — declared windows and local sensitivity'})});
            for (const [i, row] of c.orthotropic_fit.candidates.entries()) {
              const bounds=c.input.orthotropic.time_bounds_seconds[i];
              assert.deepEqual(await windows.locator('tr').nth(i+1).locator('td').allTextContents(),
                [bounds[0],row.time_seconds,bounds[1],search.time_sensitivity[i],search.boundary_distance_fraction[i]].map(String));
            }
            assert.ok(search.starts_converged>0);
            assert.equal(search.starts_attempted,c.starts_attempted);
            assert.match(await page.locator('#outputs').textContent(),/Directional time search — local optimizer diagnostics/);
          } else assert.match(await page.locator('#outputs').textContent(),/Inactive/);
        }
        await page.click('#assign'); assert.equal(await run('bind'),'');
        const selected=JSON.parse(await download('pattern.json'));
        assert.equal(selected.fabric.material.expected_sha256,report.selection.expected_sha256);
        assert.deepEqual(selected.pieces,load('synthetic-sewing-pattern.json').pieces);
        const binding=JSON.parse(await download('material-bindings.json'));
        assert.equal(binding.drape_materials_with_self_contact.ready,admission);
      }
    }
    await input('calibrationRequest',{}); assert.equal(await page.isDisabled('#assign'),true);
    assert.equal(await page.locator('a[download="material-calibration.json"]').count(),0);
    assert.notEqual(await run('calibrate'),''); assert.equal(await page.isDisabled('#assign'),true);
    const permanent=load('synthetic-membrane-plasticity.json'), permanentLaws=[];
    for (const yieldWarp of [0.2,0.6]) {
      const selected=structuredClone(permanent);
      selected.record.properties.membrane_yield_warp.value=yieldWarp;
      const bound=await assign(selected);
      assert.deepEqual(bound.pieces,load('synthetic-sewing-pattern.json').pieces);
      const binding=JSON.parse(await download('material-bindings.json'));
      assert.equal(binding.drape_materials_without_self_contact.ready,true);
      const filename=path.join(work,`permanent-${yieldWarp}.json`);
      fs.writeFileSync(filename,await download('pattern.json'));
      const expected=JSON.parse(invoke('resolve',filename));
      assert.deepEqual(binding.drape_materials_without_self_contact.value,expected);
      assert.match(expected.membrane_plasticity_law_sha256,/^[a-f0-9]{64}$/);
      permanentLaws.push(expected.membrane_plasticity_law_sha256);
    }
    assert.notEqual(...permanentLaws);
    delete permanent.record.properties.membrane_yield_weft;
    await assign(permanent);
    assert.equal(JSON.parse(await download('material-bindings.json')).drape_materials_without_self_contact.ready,false);
    let bound = await assign(sheet(0.1,0.4));
    assert.deepEqual(bound.pieces, load('synthetic-sewing-pattern.json').pieces);
    const boundPath = path.join(work,'bound.json'), selectionPath = write('selection.json',sheet(0.1,0.4));
    // Native consumers receive the same frozen number spellings as the browser consumers.
    fs.writeFileSync(boundPath,await download('pattern.json'));
    invoke('bind',fixture('synthetic-sewing-pattern.json'),selectionPath,path.join(work,'native-bound.json'));
    assert.deepEqual(bound,JSON.parse(fs.readFileSync(path.join(work,'native-bound.json'),'utf8')));
    const cut = {copies:1,gap_mm:2,edge_margin_mm:0,end_margin_mm:0};
    await input('cutOptions',cut); const cutPath = write('cut.json',cut);
    assert.equal(await run('cut'),''); const marker = JSON.parse(await download('cut-plan.json'));
    assert.deepEqual(marker,JSON.parse(invoke('cut-plan',boundPath,cutPath)));
    invoke('cut-svg',boundPath,cutPath,'0',path.join(work,'native-roll.svg'));
    assert.equal(await download('roll-0.svg'),fs.readFileSync(path.join(work,'native-roll.svg'),'utf8'));
    await page.setInputFiles('#sewOptions',fixture('synthetic-sewing-options.json')); assert.equal(await run('sew'),'');
    assert.deepEqual(JSON.parse(await download('sewing-program.json')),JSON.parse(invoke('sew-stitches',boundPath,fixture('synthetic-sewing-options.json'))));
    const quotes = {schema:'ftx-textile-quotes/1',quotes:[]}, market = {currency:'USD',region:'US',quality:'test',terms:'test',as_of:'2026-10-15',max_age_days:30};
    await input('quotes',quotes); await input('market',market); assert.equal(await run('price'),'');
    const cost = JSON.parse(await download('cut-price.json'));
    assert.equal(cost.complete_offer_coverage,false); assert.equal(cost.least_observed_independent_order_total,null);
    assert.deepEqual(cost,JSON.parse(invoke('cut-price',boundPath,cutPath,write('quotes.json',quotes),write('market.json',market))));
    await page.screenshot({path:path.join(work,'workbench.png'),fullPage:true});
    bound = await assign(sheet(0.06,2));
    assert.deepEqual(bound.pieces,load('synthetic-sewing-pattern.json').pieces);
    assert.equal(await page.locator('a[download="sewing-program.json"]').count(),0);
    assert.equal(await run('cut'),''); const changed = JSON.parse(await download('cut-plan.json'));
    assert.ok(changed.rolls[0].length_m > marker.rolls[0].length_m);
    assert.match(await run('sew'),/stack/); assert.equal(await page.locator('a[download="sewing-program.json"]').count(),0);
    // Execute the actual thermal worker on an explicitly synthetic frozen report, not a claimed body drape.
    const worker = async msg => page.evaluate(msg => new Promise((resolve,reject) => {
      const w = new Worker('./materials-worker.js',{type:'module'});
      const timer = setTimeout(() => {w.terminate();reject(Error('worker timeout'));},60000);
      w.onmessage = ({data}) => { if(data.type==='ready') w.postMessage({...msg,request:99});
        else if(data.type==='result'||data.type==='error'||data.type==='loadError') {clearTimeout(timer);w.terminate();resolve(data);} };
      w.onerror = e => {clearTimeout(timer);w.terminate();reject(Error(e.message));};
    }), msg);
    const report = fs.readFileSync(fixture('synthetic-steady-surface-report.json'),'utf8');
    const conditions = fs.readFileSync(fixture('steady-surface-conditions.json'),'utf8');
    const heat = await worker({type:'heat',report,conditions});
    assert.equal(heat.type,'result');
    assert.deepEqual(heat.result,JSON.parse(invoke('thermal-steady-surface',fixture('synthetic-steady-surface-report.json'),fixture('steady-surface-conditions.json'))));
    const coverage = await worker({type:'coverage',report,
      map:fs.readFileSync(fixture('steady-coverage-map.json'),'utf8'),
      conditions:fs.readFileSync(fixture('steady-coverage-conditions.json'),'utf8')});
    assert.equal(coverage.type,'result');
    assert.deepEqual(coverage.result,JSON.parse(invoke('thermal-steady-coverage',fixture('synthetic-steady-surface-report.json'),fixture('steady-coverage-map.json'),fixture('steady-coverage-conditions.json'))));
    // Actual compiled browser solver: explicit source-normal assignment changes the selected finish.
    const opticalStack = load('synthetic-asymmetric-exterior.json'), opticalResults = [];
    const browserSteady = request => page.evaluate(async request => {
      const ftx = await import('./pkg/ftx_wasm.js'); await ftx.default();
      try { return {result:JSON.parse(ftx.thermal_steady(request))}; }
      catch (e) { return {error:String(e)}; }
    }, JSON.stringify(request));
    for (const reversed of [false,true]) {
      const input = structuredClone(opticalStack), faces = input.layers[0].record.optical_faces;
      if (reversed) [faces.positive,faces.negative] = [faces.negative,faces.positive];
      const computed = await browserSteady(input);
      assert.equal(computed.error,undefined);
      assert.deepEqual(computed.result,JSON.parse(invoke('thermal-steady',write(`optical-${reversed}.json`,input))));
      const selected = computed.result.exterior_exchange.material_face;
      assert.equal(selected.side,'positive');
      assert.equal(selected.face,reversed ? 'reverse' : 'finish');
      assert.equal(selected.emissivity_property,reversed ? 'reverse_emissivity' : 'finish_emissivity');
      opticalResults.push(computed.result);
    }
    assert.notEqual(opticalResults[0].power_w,opticalResults[1].power_w);
    assert.notEqual(opticalResults[0].request_sha256,opticalResults[1].request_sha256);
    delete opticalStack.layers[0].record.optical_faces;
    assert.match((await browserSteady(opticalStack)).error,/no source-sheet optical face mapping/);
    const badModel = await worker({type:'fold',bytes:new ArrayBuffer(8),pattern:JSON.stringify(bound),fit:'{}',measurements:'{}',selfContactMm:2});
    assert.equal(badModel.type,'error');
    await page.setInputFiles('#patternFile',{name:'bad.json',mimeType:'application/json',buffer:Buffer.from('{')});
    await page.waitForFunction(() => !document.getElementById('error').hidden);
    assert.equal(await page.locator('a[download="pattern.json"]').count(),0); assert.equal(await page.isDisabled('#bind'),true);
    assert.deepEqual(errors,[]); assert.deepEqual(external,[]); assert.ok(methods.every(m => m==='GET'));
    console.log('PASS: real browser nonlinear/proportional/directional calibration with fixed and identified times, estimate admission, catalog, frozen binding, permanent membrane law selection, material substitution, cutting/SVG, sewing, cost, source-locked thermal worker/native equality, automatic asymmetric optical faces, invalidation and model refusal.');
    console.log('No successful model-backed browser drape or real textile validation asserted. Artifacts:',work);
  } finally { await browser.close(); await new Promise(resolve => server.close(resolve)); }
})().catch(e => { console.error(e); server.close(); process.exitCode=1; });
