// Actual Chromium + compiled WASM, compared with the real native materials executable.
// No model asset is bundled: successful body fitting/drape requires the separate existing model lane.
// tools/build_web.sh && cargo build -p ftx-cli
// NODE_PATH=$(npm root -g) node tools/check_materials_web.cjs --native target/debug/ftx-materials
// Optional CHROMIUM_EXECUTABLE chooses an already installed Chromium binary.
// --strip-proof DIR runs only strip browser checks against retained native reports (manifest.json).
// --outfit-proof checks public-prior drafted garment composition with synthetic materials, without a model.
// --compression-proof checks real history controls, heat/work answers and final material binding.
// --purchasing-proof checks bounded roll orders and direct exact finite-stock identities/pricing.
// --area-proof DIR checks six-parameter area fitting against native-input.json/native-report.json.
// --swatch-area-proof DIR checks joint swatch identification against retained native consent reports.
// --adaptive-proof DIR checks guarded Pareto studies on the real page against retained native reports.
// --sewing-selection-proof DIR checks native captured fixed-route requests through direct recipe selection.
// --mixed-cutting-proof DIR checks complete and process-refused native mixed cutting jobs on the real page.
const { chromium } = require(process.env.FTX_PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');
const { execFileSync } = require('node:child_process');
const fs = require('node:fs');
const http = require('node:http');
const os = require('node:os');
const path = require('node:path');
const { createHash } = require('node:crypto');
const root = path.resolve(__dirname, '..');
const webIndex = process.argv.indexOf('--web-dir');
if (webIndex >= 0 && !process.argv[webIndex + 1]) throw Error('--web-dir requires a staged site directory');
const web = webIndex < 0 ? path.join(root, 'web') : path.resolve(process.argv[webIndex + 1]);
const index = process.argv.indexOf('--native');
const native = path.resolve(index < 0 ? path.join(root, 'target/debug/ftx-materials') : process.argv[index + 1]);
const stripIndex = process.argv.indexOf('--strip-proof');
if (stripIndex >= 0 && !process.argv[stripIndex + 1]) throw Error('--strip-proof requires a native report directory');
const stripProof = stripIndex < 0 ? null : path.resolve(process.argv[stripIndex + 1]);
const outfitProof = process.argv.includes('--outfit-proof');
const compressionProof = process.argv.includes('--compression-proof');
const purchasingProof = process.argv.includes('--purchasing-proof');
const areaIndex = process.argv.indexOf('--area-proof');
if (areaIndex >= 0 && !process.argv[areaIndex + 1]) throw Error('--area-proof requires a native capture directory');
const areaProof = areaIndex < 0 ? null : path.resolve(process.argv[areaIndex + 1]);
const swatchAreaIndex = process.argv.indexOf('--swatch-area-proof');
if (swatchAreaIndex >= 0 && !process.argv[swatchAreaIndex + 1]) throw Error('--swatch-area-proof requires a native capture directory');
const swatchAreaProof = swatchAreaIndex < 0 ? null : path.resolve(process.argv[swatchAreaIndex + 1]);
const fitIndex = process.argv.indexOf('--fit-manufacturing-proof');
if (fitIndex >= 0 && !process.argv[fitIndex + 1]) throw Error('--fit-manufacturing-proof requires the native fit-screening replay directory');
const fitProof = fitIndex < 0 ? null : path.resolve(process.argv[fitIndex + 1]);
const adaptiveIndex = process.argv.indexOf('--adaptive-proof');
if (adaptiveIndex >= 0 && !process.argv[adaptiveIndex + 1]) throw Error('--adaptive-proof requires the native guarded capture directory');
const adaptiveProof = adaptiveIndex < 0 ? null : path.resolve(process.argv[adaptiveIndex + 1]);
const sewingSelectionIndex = process.argv.indexOf('--sewing-selection-proof');
if (sewingSelectionIndex >= 0 && !process.argv[sewingSelectionIndex + 1]) throw Error('--sewing-selection-proof requires the native route-cost capture directory');
const sewingSelectionProof = sewingSelectionIndex < 0 ? null : path.resolve(process.argv[sewingSelectionIndex + 1]);
const mixedCuttingIndex = process.argv.indexOf('--mixed-cutting-proof');
if (mixedCuttingIndex >= 0 && !process.argv[mixedCuttingIndex + 1]) throw Error('--mixed-cutting-proof requires the native finite-knife capture directory');
const mixedCuttingProof = mixedCuttingIndex < 0 ? null : path.resolve(process.argv[mixedCuttingIndex + 1]);
assert.ok([stripProof,outfitProof,compressionProof,purchasingProof,areaProof,swatchAreaProof,fitProof,adaptiveProof,sewingSelectionProof,mixedCuttingProof].filter(Boolean).length <= 1, 'Choose one focused browser check');
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
  if (!stripProof && !outfitProof && !compressionProof && !purchasingProof && !areaProof && !swatchAreaProof && !fitProof && !adaptiveProof) assert.ok(fs.existsSync(native), 'Build the native ftx-materials first');
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
    if (mixedCuttingProof) {
      const source=name=>fs.readFileSync(path.join(mixedCuttingProof,name),'utf8');
      const output=name=>page.locator(`#outputs a[download="${name}"]`).evaluate(async a=>await(await fetch(a.href)).text());
      await page.setInputFiles('#patternFile',path.join(mixedCuttingProof,'mixed-pattern.json'));
      assert.equal(await page.isDisabled('#cutJob'),true);
      await page.waitForFunction(()=>!document.getElementById('bind').disabled);
      assert.equal(await run('bind'),'');
      const pattern=await download('pattern.json'),patternFile=path.join(work,'mixed-bound-pattern.json');
      fs.writeFileSync(patternFile,pattern,{flag:'wx'});
      for(const name of ['job','refused-job']) {
        const requestFile=path.join(mixedCuttingProof,`${name}.json`),request=source(`${name}.json`);
        await page.setInputFiles('#cutJobRequest',requestFile);assert.equal(await run('cutJob'),'');
        const report=JSON.parse(await output('cut-job.json'));
        assert.deepEqual(report,JSON.parse(invoke('cut-job',patternFile,requestFile)));
        assert.equal(report.complete_inventory,true);assert.equal(report.hardware_execution_authorized,false);
        assert.equal(new Set(report.inventory.map(i=>`${i.instance}/${i.copy}`)).size,report.inventory.length);
        assert.equal(await output('original-pattern.json'),pattern);
        assert.equal(await output('original-cut-job-request.json'),request);
        assert.equal(await output('cut-job-pattern.json'),report.pattern_json);
        assert.equal(await output('cut-job-request.json'),report.replay_request_json);
        const replayPattern=path.join(work,`${name}-pattern.json`),replayRequest=path.join(work,`${name}-request.json`);
        fs.writeFileSync(replayPattern,report.pattern_json,{flag:'wx'});fs.writeFileSync(replayRequest,report.replay_request_json,{flag:'wx'});
        assert.deepEqual(JSON.parse(invoke('cut-job',replayPattern,replayRequest)),report);
        assert.equal(await page.getByRole('table',{name:'Complete cutting jobs — rolls, hides and remnants',exact:true}).locator('tr').count(),2);
        assert.ok(report.roll.program.markers[0].selected.modeled_path_seconds>0);
        if(name==='job') {
          assert.ok(report.finite_stock.program.markers[0].selected.modeled_path_seconds>0);
          assert.equal(await page.getByRole('table',{name:'Current bound pattern: finite-stock route: physical stock and machine placement',exact:true}).count(),1);
        } else {
          assert.equal(report.finite_stock.program,undefined);assert.ok(report.finite_stock.program_refusal);
          assert.match(await page.locator('#outputs').textContent(),/knife process unavailable/);
        }
      }
      const invalid=JSON.parse(source('job.json'));invalid.routes.pop();
      await input('cutJobRequest',invalid);
      assert.equal(await page.locator('a[download="cut-job.json"]').count(),0);
      assert.notEqual(await run('cutJob'),'');
      assert.equal(await page.locator('a[download="cut-job-request.json"]').count(),0);
      await page.setInputFiles('#cutJobRequest',path.join(mixedCuttingProof,'job.json'));
      await page.evaluate(()=>{document.getElementById('cutJob').click();document.getElementById('cancel').click();});
      await page.waitForFunction(()=>document.getElementById('status').textContent.startsWith('Ready'));
      assert.equal(await page.locator('a[download="cut-job.json"]').count(),0);
      assert.equal(await run('cutJob'),'');
      await page.screenshot({path:path.join(work,'mixed-cutting.png'),fullPage:true});
      await input('patternFile',{schema:'fabric-pattern/1',pieces:[]});
      assert.equal(await page.locator('a[download="cut-job.json"]').count(),0);
      assert.equal(await page.locator('a[download="cut-job-pattern.json"]').count(),0);
      assert.deepEqual(errors,[]);assert.deepEqual(external,[]);assert.ok(methods.every(m=>m==='GET'));
      console.log('PASS: direct Chromium mixed cutting matches complete native roll/finite jobs and frozen replay, retains independent knife refusals and exact downloads, and withdraws stale output on invalid requests, cancellation and source edits. Synthetic software evidence only. Artifacts:',work);
      return;
    }
    if (sewingSelectionProof) {
      const source=name=>fs.readFileSync(path.join(sewingSelectionProof,name),'utf8');
      const output=name=>page.locator(`#outputs a[download="${name}"]`).evaluate(async a=>await(await fetch(a.href)).text());
      await page.setInputFiles('#patternFile',path.join(sewingSelectionProof,'pattern.json'));
      await page.waitForFunction(()=>!document.getElementById('bind').disabled);
      assert.equal(await run('bind'),'');
      const bound=await download('pattern.json'),patternFile=path.join(work,'bound-pattern.json');
      fs.writeFileSync(patternFile,bound,{flag:'wx'});
      await page.check('#sewOptimize');
      let selectedOptions,selectedProgram;
      const table=name=>page.getByRole('table',{name,exact:true});
      for (const [filename,cost] of [['unconstrained-request.json',10],['request.json',14]]) {
        const requestPath=path.join(sewingSelectionProof,filename),request=source(filename);
        await page.setInputFiles('#sewOptions',requestPath);
        assert.equal(await run('sew'),'');
        const expected=JSON.parse(invoke('sew-select',patternFile,requestPath));
        assert.deepEqual(JSON.parse(await output('sewing-selection.json')),expected);
        assert.equal(expected.report.selected_thread_order_cost,cost);
        assert.equal(await output('sewing-selection-request.json'),request);
        assert.equal(await output('original-pattern.json'),bound);
        selectedOptions=await output('selected-sewing-options.json');
        assert.equal(selectedOptions,expected.selected_options_json);
        const optionsFile=path.join(work,`selected-${cost}.json`);fs.writeFileSync(optionsFile,selectedOptions,{flag:'wx'});
        selectedProgram=JSON.parse(invoke('sew-stitches',patternFile,optionsFile));
        assert.deepEqual(JSON.parse(await output('sewing-program.json')),selectedProgram);
        const rows=await table('Complete thread order costs by recipe and route').locator('tr').allTextContents();
        assert.equal(rows.length,expected.report.trials.length+1);
        assert.match(rows[1],/10/);assert.match(rows[2],/14/);
        if(cost===14) { assert.match(rows[1],/Misses deadline \(proved\)/);assert.match(rows[2],/Meets deadline/); }
      }
      await page.uncheck('#sewOptimize');
      assert.equal(await page.locator('a[download="selected-sewing-options.json"]').count(),0);
      await input('sewOptions',JSON.parse(selectedOptions));assert.equal(await run('sew'),'');
      assert.deepEqual(JSON.parse(await download('sewing-program.json')),selectedProgram);
      await page.check('#sewOptimize');
      const impossible=JSON.parse(source('request.json'));impossible.completion_deadline_milliseconds=0;
      await input('sewOptions',impossible);assert.equal(await run('sew'),'');
      assert.equal(JSON.parse(await download('sewing-selection.json')).selected_options_json,null);
      assert.equal(await page.locator('a[download="selected-sewing-options.json"]').count(),0);
      assert.equal(await page.locator('a[download="sewing-program.json"]').count(),0);
      assert.match(await page.locator('#outputs').textContent(),/No complete sewing candidate was selected/);
      await input('sewOptions',{schema:'unsupported'});assert.notEqual(await run('sew'),'');
      assert.equal(await page.locator('a[download="sewing-selection.json"]').count(),0);
      await page.setInputFiles('#sewOptions',path.join(sewingSelectionProof,'request.json'));
      await page.evaluate(()=>{document.getElementById('sew').click();document.getElementById('cancel').click();});
      await page.waitForFunction(()=>document.getElementById('status').textContent.startsWith('Ready'));
      assert.equal(await page.locator('a[download="sewing-program.json"]').count(),0);
      assert.equal(await run('sew'),'');
      await input('patternFile',{schema:'fabric-pattern/1',pieces:[]});
      assert.equal(await page.locator('a[download="sewing-program.json"]').count(),0);
      assert.equal(await page.locator('a[download="selected-sewing-options.json"]').count(),0);
      assert.deepEqual(errors,[]);assert.deepEqual(external,[]);assert.ok(methods.every(m=>m==='GET'));
      console.log('PASS: direct Chromium recipe/route choice matches native selection and actual stitches, retains cheaper deadline-excluded costs, exact source/options downloads and ordinary sewing replay; no incumbent, invalid input, cancellation and source edits withdraw stale programs. Synthetic software evidence only. Artifacts:',work);
      return;
    }
    if (adaptiveProof) {
      const source=name=>fs.readFileSync(path.join(adaptiveProof,name),'utf8');
      const reportName='design-scenario-matrix-study.json';
      const table=name=>page.getByRole('table',{name,exact:true});
      await page.setInputFiles('#patternFile',path.join(adaptiveProof,'pattern.json'));
      await page.selectOption('#studyBodySource','request');
      await page.setInputFiles('#body',path.join(adaptiveProof,'model.ftxbody'));
      await page.setInputFiles('#scenarioBody',path.join(adaptiveProof,'body-input.json'));
      for (const label of ['pareto-guarded','pareto-guarded-budget','matrix-pareto-guarded','matrix-pareto-guarded-budget']) {
        const allCases=label.startsWith('matrix-');
        await page.setInputFiles('#studyRequest',path.join(adaptiveProof,`${label}-study.json`));
        await page.setInputFiles('#scenarioRequest',path.join(adaptiveProof,`${label}-scenario.json`));
        await page.waitForFunction(()=>!document.getElementById('scenario').disabled);
        assert.equal(await run('scenario'),'');
        const actual=JSON.parse(await download(reportName));
        assert.deepEqual(actual,JSON.parse(source(`${label}-report.json`)));
        assert.equal(await download('original-pattern.json'),source('pattern.json'));
        assert.equal(await download('study-request.json'),source(`${label}-study.json`));
        assert.equal(await download('scenario-request.json'),source(`${label}-scenario.json`));
        assert.equal(await download('body-input.json'),source('body-input.json'));
        const search=actual.manufacturing.design_search,guidance=search.consumer_guidance;
        assert.equal(await table('Adaptive alteration trials').locator('tr').count(),search.rows.length+1);
        if (allCases) {
          const policies=await table('Adaptive guidance policies by operating case').locator('tr').allTextContents();
          assert.equal(policies.length,guidance.case_order.length+1);
          for (const [i,key] of guidance.case_order.entries()) assert.ok(policies[i+1].startsWith(key));
          assert.match(policies[1],/assembly_mass_kg \(minimize\)/);
          assert.match(policies[2],/independent_surface_power_w \(minimize\)/);
          const frontier=await table('Multi-case Pareto adaptive alteration search').textContent();
          for (const key of guidance.pareto_frontier.evaluated_trial_keys) assert.ok(frontier.includes(key));
          assert.equal(await table('Consumer-guided adaptive alteration search').count(),0);
        }
        for (const [i,row] of search.rows.entries()) {
          if (typeof row.design.altered_pattern_json==='string')
            assert.equal(await download(`adaptive-design-${i+1}-pattern.json`),row.design.altered_pattern_json);
          const cells=await table('Adaptive alteration trials').locator('tr').nth(i+1).locator('td').allTextContents();
          assert.ok(cells.includes(row.selected?'Selected':'Not selected'));
          if (allCases) assert.equal(cells[5],row.consumer_cases.every(c=>c.comparison?.eligible===true)?'Eligible':'Excluded');
        }
        await page.screenshot({path:path.join(work,`${label}.png`),fullPage:true});
        await input('scenarioRequest',{schema:'unsupported'});
        assert.equal(await page.locator(`a[download="${reportName}"]`).count(),0);
        assert.notEqual(await run('scenario'),'');
        assert.equal(await page.locator('a[download="adaptive-design-1-pattern.json"]').count(),0);
      }
      const label='matrix-pareto-guarded';
      await page.setInputFiles('#scenarioRequest',path.join(adaptiveProof,`${label}-scenario.json`));
      await page.setInputFiles('#studyRequest',path.join(adaptiveProof,`${label}-study.json`));
      assert.equal(await run('scenario'),'');
      const rejected=JSON.parse(source('body-input.json'));rejected.accepted_body_fit.status='REJECTED';
      await input('scenarioBody',rejected);
      assert.equal(await page.locator(`a[download="${reportName}"]`).count(),0);
      assert.match(await run('scenario'),/fitted-body measurement context/);
      await page.setInputFiles('#scenarioBody',path.join(adaptiveProof,'body-input.json'));
      await page.evaluate(()=>{document.getElementById('scenario').click();document.getElementById('cancel').click();});
      await page.waitForFunction(()=>document.getElementById('status').textContent.startsWith('Ready'));
      assert.equal(await page.locator(`a[download="${reportName}"]`).count(),0);
      assert.equal(await run('scenario'),'');
      assert.deepEqual(JSON.parse(await download(reportName)),JSON.parse(source(`${label}-report.json`)));
      await input('patternFile',{schema:'fabric-pattern/1',pieces:[]});
      assert.equal(await page.locator(`a[download="${reportName}"]`).count(),0);
      assert.equal(await page.locator('a[download="adaptive-design-1-pattern.json"]').count(),0);
      assert.deepEqual(errors,[]);assert.deepEqual(external,[]);assert.ok(methods.every(m=>m==='GET'));
      console.log('PASS: actual Chromium/compiled WASM matches four complete native guarded Pareto reports, independent case policies, all trial pattern bytes and source downloads; malformed scenarios, rejected bodies, cancellation and source edits withdraw stale outputs. Synthetic software evidence only. Artifacts:',work);
      return;
    }
    if (fitProof) {
      const source=name=>fs.readFileSync(path.join(fitProof,name),'utf8');
      const expected=JSON.parse(source('manufacturing-report.json'));
      const originals=new Map(['pattern.json','study.json','body-input.json','model.ftxbody'].map(name=>[name,fs.readFileSync(path.join(fitProof,name))]));
      await page.setInputFiles('#patternFile',path.join(fitProof,'pattern.json'));
      await page.waitForFunction(()=>!document.getElementById('bind').disabled);
      await page.setInputFiles('#studyRequest',path.join(fitProof,'study.json'));
      await page.check('#studyOnBody');assert.equal(await page.isDisabled('#study'),true);
      await page.setInputFiles('#body',path.join(fitProof,'model.ftxbody'));
      await page.setInputFiles('#scenarioBody',path.join(fitProof,'body-input.json'));
      assert.equal(await page.isDisabled('#scenario'),true);assert.equal(await page.isDisabled('#study'),false);
      assert.equal(await run('study'),'');
      const report=JSON.parse(await download('design-study.json'));
      assert.deepEqual(report,expected);
      const screening=report.design_search.fit_screening;
      assert.equal(screening.screened_designs,9);assert.equal(screening.rejected_designs,8);
      assert.equal(screening.selected_designs,1);assert.equal(screening.rows.find(r=>r.selected).design.key,'grid:0006');
      assert.equal(report.alternatives.find(r=>r.design.key==='grid:0006').design.variables_mm.length,30);
      assert.equal(report.simulations,undefined);assert.equal(report.scenario,undefined);
      assert.equal(await download('original-pattern.json'),source('pattern.json'));
      assert.equal(await download('study-request.json'),source('study.json'));
      assert.equal(await download('body-input.json'),source('body-input.json'));
      for (const [i,row] of report.alternatives.entries()) {
        assert.equal(await download(`alternative-${i+1}-design-pattern.json`),row.design.altered_pattern_json);
        if (row.binding.status==='available') assert.equal(await download(`alternative-${i+1}-pattern.json`),row.binding.result.pattern_json);
      }
      for (const [i,row] of screening.rows.entries())
        assert.equal(await download(`screened-design-${i+1}-pattern.json`),row.design.altered_pattern_json);
      await input('scenarioRequest',{schema:'unused'});assert.equal(await page.locator('a[download="design-study.json"]').count(),1);
      await input('measurements',{wearer:{stature_mm:999}});assert.equal(await page.locator('a[download="design-study.json"]').count(),1);
      await page.setInputFiles('#body',{name:'replacement.ftxbody',mimeType:'application/octet-stream',buffer:originals.get('model.ftxbody')});
      assert.equal(await page.locator('a[download="design-study.json"]').count(),0);
      assert.equal(await run('study'),'');assert.deepEqual(JSON.parse(await download('design-study.json')),expected);
      const rejected=JSON.parse(source('body-input.json'));rejected.accepted_body_fit.status='REJECTED';
      await input('scenarioBody',rejected);assert.equal(await page.locator('a[download="design-study.json"]').count(),0);
      assert.match(await run('study'),/accepted|ACCEPTED/);
      assert.equal(await page.locator('a[download="design-study.json"]').count(),0);
      await page.setInputFiles('#scenarioBody',path.join(fitProof,'body-input.json'));
      const noFit=JSON.parse(source('study.json'));delete noFit.design_fit;await input('studyRequest',noFit);
      assert.match(await run('study'),/design_fit/);assert.equal(await page.locator('a[download="design-study.json"]').count(),0);
      await page.setInputFiles('#studyRequest',path.join(fitProof,'study.json'));
      assert.equal(await run('study'),'');assert.deepEqual(JSON.parse(await download('design-study.json')),expected);
      await page.uncheck('#studyOnBody');assert.equal(await page.locator('a[download="design-study.json"]').count(),0);
      const legacy=load('synthetic-sewing-pattern.json');
      await page.setInputFiles('#studyRequest',{name:'legacy.json',mimeType:'application/json',buffer:fs.readFileSync(path.join(root,'examples/design-study.json'))});
      await input('patternFile',legacy);await page.waitForFunction(()=>!document.getElementById('study').disabled);
      assert.equal(await run('study'),'');assert.equal(JSON.parse(await download('design-study.json')).alternatives.length,8);
      for (const [name,raw] of originals) assert.ok(fs.readFileSync(path.join(fitProof,name)).equals(raw),name);
      assert.deepEqual(errors,[]);assert.deepEqual(external,[]);assert.ok(methods.every(m=>m==='GET'));
      console.log('PASS: actual selected-body manufacturing matches complete native report, selects +30mm after six refusals, retains exact sources, invalidates dependent edits/refusals, preserves legacy study and makes only local GET requests. Synthetic software evidence, no cloth/heat or physical fit claim. Artifacts:',work);
      return;
    }
    if (swatchAreaProof) {
      const raw=fs.readFileSync(path.join(root,'examples/area-identification/swatch.json'),'utf8');
      assert.equal(raw,fs.readFileSync(path.join(swatchAreaProof,'native-input.json'),'utf8'));
      const original=load('synthetic-sewing-pattern.json');
      await input('patternFile',original); await page.waitForFunction(()=>!document.getElementById('bind').disabled);
      await page.selectOption('#calibrationMode','nonlinear');
      assert.match(await page.locator('#calibrationMode option[value="nonlinear"]').textContent(),/optional area response/);
      const upload=async value=>page.setInputFiles('#calibrationRequest',{name:'swatch.json',mimeType:'application/json',buffer:Buffer.from(value)});
      let accepted;
      for (const consent of [false,true]) {
        await page.setChecked('#calibrationEstimates',consent); await upload(raw);
        assert.equal(await run('calibrate'),'');
        const reportRaw=await download('material-calibration.json'), report=JSON.parse(reportRaw);
        const expected=JSON.parse(fs.readFileSync(path.join(swatchAreaProof,`native-admission-${consent}-report.json`),'utf8'));
        assert.deepEqual(report.calibration,expected.calibration);
        assert.deepEqual(report.selection,expected.selection); assert.equal(report.selection_json,expected.selection_json);
        assert.equal(report.selection.allow_estimates,consent);
        assert.equal(report.material.capabilities.mechanical_coefficients.ready,false,'Missing independent mechanics remain missing');
        assert.equal(report.fit_claim,false); assert.equal(report.calibration.parameter_count,9);
        const c=report.calibration, area=c.material.membrane_response.area_response;
        assert.deepEqual(area.area_ratio_range,c.fitted_area_ratio_range);
        const coefficients=page.locator('table').filter({has:page.locator('caption',{hasText:'Area coefficients — joint identification'})});
        assert.equal(await coefficients.count(),1);
        for(const [i,q] of [area.quadratic,area.log_squared].entries()) {
          assert.deepEqual((await coefficients.locator('tr').nth(i+1).locator('td').allTextContents()).slice(1),[q.value,q.unit,q.evidence,q.source].map(String));
          assert.ok(Math.abs(q.value-[40,15][i])<1e-7);
        }
        const domain=page.locator('table').filter({has:page.locator('caption',{hasText:'Physical area ratio J — applicability'})});
        assert.equal(await domain.count(),1);
        assert.deepEqual(await domain.locator('tr').nth(1).locator('td').allTextContents(),['Admissible input domain',...c.input.area_response.area_ratio_range.map(String)]);
        assert.deepEqual(await domain.locator('tr').nth(2).locator('td').allTextContents(),['FIT observations and undeformed reference',...c.fitted_area_ratio_range.map(String)]);
        assert.deepEqual(await domain.locator('tr').nth(3).locator('td').allTextContents(),['Published material domain',...area.area_ratio_range.map(String)]);
        assert.notDeepEqual(c.input.area_response.area_ratio_range,c.fitted_area_ratio_range);
        assert.equal(await download('calibration-input.json'),raw);
        assert.equal(await download('calibrated-selection.json'),report.selection_json);
        assert.equal(await page.inputValue('#selection'),report.selection_json);
        await page.click('#assign'); assert.equal(await run('bind'),'');
        const bound=JSON.parse(await download('pattern.json'));
        assert.deepEqual(bound.pieces,original.pieces); assert.deepEqual(bound.seams,original.seams);
        assert.equal(bound.fabric.material.expected_sha256,report.selection.expected_sha256);
        fs.writeFileSync(path.join(work,`swatch-area-${consent}-report.json`),reportRaw,{flag:'wx'});
        fs.writeFileSync(path.join(work,`swatch-area-${consent}-bound.json`),await download('pattern.json'),{flag:'wx'});
        accepted=report;
      }
      await page.setChecked('#calibrationEstimates',false);
      assert.equal(await page.isDisabled('#assign'),true); assert.equal(await page.locator('a[download="calibrated-selection.json"]').count(),0);
      await page.setChecked('#calibrationEstimates',true);
      for(const mutate of [
        v=>{delete v.area_response.model; return /missing field.*model/i;},
        v=>{v.area_response.model='unsupported'; return /area|model/i;},
        v=>{v.samples.at(-1).nominal_n_m[0][0]+=1; return /residuals|budget/i;},
        v=>{
          const a=Math.sqrt(0.72),b=0.18/a,d=Math.sqrt(0.82-b*b);
          assert.ok(a*d>v.area_response.area_ratio_range[0] && a*d<accepted.calibration.fitted_area_ratio_range[0]);
          for(const [i,e] of [-0.14,-0.09,0.18].entries()) assert.ok(e>accepted.calibration.fitted_strain_range[i][0] && e<accepted.calibration.fitted_strain_range[i][1]);
          v.samples.at(-1).deformation=[[a,b],[0,d]];
          return /area-ratio domain/i;
        },
        v=>{v.allow_estimated_observations=false; return /invalid sample identity, source, evidence, observed forces or scale/;}
      ]) {
        const invalid=JSON.parse(raw), expectedError=mutate(invalid); await upload(JSON.stringify(invalid));
        assert.match(await run('calibrate'),expectedError);
        assert.equal(await page.isDisabled('#assign'),true); assert.equal(await page.locator('a[download="calibrated-selection.json"]').count(),0);
        await upload(raw); assert.equal(await run('calibrate'),'');
        assert.equal(await download('calibrated-selection.json'),accepted.selection_json);
      }
      fs.writeFileSync(path.join(work,'swatch-area-input.json'),raw,{flag:'wx'});
      await page.locator('#outputs').screenshot({path:path.join(work,'swatch-area.png')});
      const changed=structuredClone(accepted.selection); changed.record.name+=' changed';
      await page.fill('#selection',JSON.stringify(changed)); assert.notEqual(await run('inspect'),'');
      assert.equal(await page.isDisabled('#assign'),true);
      assert.deepEqual(errors,[]); assert.deepEqual(external,[]); assert.ok(methods.every(m=>m==='GET'));
      console.log('PASS: actual Chromium matches complete native joint-swatch calibration, displays authoritative coefficients and FIT-only J support, retains exact source/selection, binds unchanged geometry and withdraws failed/changed-consent/tampered selections. Synthetic software evidence only; no confidence or empirical textile/fit claim. Artifacts:',work);
      return;
    }
    if (areaProof) {
      const raw=fs.readFileSync(path.join(areaProof,'native-input.json'),'utf8');
      const expected=JSON.parse(fs.readFileSync(path.join(areaProof,'native-report.json'),'utf8'));
      assert.equal(raw,fs.readFileSync(path.join(root,'examples/area-response/identify.json'),'utf8'));
      assert.deepEqual(JSON.parse(raw),expected.input); assert.equal(expected.accepted,true);
      const original=load('synthetic-stock-pattern.json'); await input('patternFile',original);
      await page.waitForFunction(()=>!document.getElementById('bind').disabled);
      await page.locator('details').filter({has:page.locator('#stripFit')}).evaluate(e=>{e.open=true;});
      await page.setInputFiles('#stripFitRequest',{name:'joint-area.json',mimeType:'application/json',buffer:Buffer.from(raw)});
      assert.equal(await run('stripFit'),'');
      const resultRaw=await download('strip-calibration-report.json'),result=JSON.parse(resultRaw);
      assert.deepEqual(result.calibration,expected);
      assert.equal(await download('strip-calibration-input.json'),raw);
      assert.equal(await download('strip-calibrated-selection.json'),expected.selection_json);
      assert.equal(await page.inputValue('#selection'),expected.selection_json);
      const t=page.locator('table').filter({has:page.locator('caption',{hasText:'Identified directional and physical-area coefficients'})});
      assert.equal(await t.count(),1); assert.equal(await t.locator('tr').count(),7);
      for (const [i,d] of expected.parameter_descriptors.entries())
        assert.deepEqual(await t.locator('tr').nth(i+1).locator('td').allTextContents(),
          [d.label,expected.parameters[i],d.unit,...expected.parameter_bounds[i]].map(String));
      fs.writeFileSync(path.join(work,'area-input.json'),raw,{flag:'wx'});
      fs.writeFileSync(path.join(work,'area-report.json'),resultRaw,{flag:'wx'});
      await page.locator('#outputs').screenshot({path:path.join(work,'area-accepted.png')});
      const unfinished=JSON.parse(raw); unfinished.policy.max_iterations=1;
      await input('stripFitRequest',unfinished);
      assert.equal(await page.locator('a[download="strip-calibrated-selection.json"]').count(),0);
      assert.equal(await run('stripFit'),'');
      const rejected=JSON.parse(await download('strip-calibration-report.json'));
      assert.equal(rejected.calibration.accepted,false); assert.equal(rejected.selection,null);
      assert.equal(await page.isDisabled('#assign'),true);
      await page.setInputFiles('#stripFitRequest',{name:'joint-area.json',mimeType:'application/json',buffer:Buffer.from(raw)});
      assert.equal(await run('stripFit'),'');
      assert.deepEqual(JSON.parse(await download('strip-calibration-report.json')).calibration,expected);
      const bound=await assign(result.selection);
      assert.equal(bound.fabric.material.expected_sha256,result.selection.expected_sha256);
      assert.deepEqual(bound.pieces,original.pieces); assert.deepEqual(bound.seams,original.seams);
      fs.writeFileSync(path.join(work,'area-bound-pattern.json'),await download('pattern.json'),{flag:'wx'});
      assert.deepEqual(errors,[]); assert.deepEqual(external,[]); assert.ok(methods.every(m=>m==='GET'));
      console.log('PASS: actual Chromium matches the complete native joint-area report, displays all six authoritative bounds, retains exact inputs/selection, binds without geometry changes and withdraws rejected selections. Synthetic software evidence only; no empirical textile/fit claim. Artifacts:',work);
      return;
    }
    if (purchasingProof) {
      const pattern=load('synthetic-stock-pattern.json'); pattern.pieces=pattern.pieces.slice(0,1);
      pattern.pieces[0].vertices={a:[0,0],b:[40,0],c:[40,100],d:[0,100]};
      await input('patternFile',pattern); await page.waitForFunction(()=>!document.getElementById('bind').disabled);
      const bound=await assign(sheet(0.1,1));
      const options={copies:3,gap_mm:5,edge_margin_mm:5,end_margin_mm:10,seam_allowance_mm:0,maximum_marker_length_m:0.12};
      await input('cutOptions',options); await page.fill('#rollPreview','2');
      assert.equal(await run('cut'),'');
      const planRaw=await download('cut-plan.json'), plan=JSON.parse(planRaw);
      assert.equal(plan.schema,'ftx-roll-cut-plan/2'); assert.deepEqual(plan.rolls.map(r=>r.length_m),[0.12,0.12]);
      assert.equal(plan.rolls.flatMap(r=>r.placements).length,3);
      const svg=await download('roll-1.svg'); assert.match(svg,/data-copy="2"/); assert.doesNotMatch(svg,/data-copy="0"/);
      const quote={offer_id:'synthetic-whole-order',material_id:bound.fabric.material.id,
        material_sha256:bound.fabric.material.expected_sha256,supplier:'Synthetic supplier',sku:'whole-order',
        price:10,quantity:1,unit:'linear_meter',minimum_quantity:0.3,order_increment:0.1,
        currency:'USD',region:'US',quality:'test',terms:'test',observed_on:'2026-10-21',available:true,
        available_quantity:0.4,maximum_continuous_quantity:0.15,source:'test:browser-purchasing',source_sha256:'0'.repeat(64)};
      const market={currency:'USD',region:'US',quality:'test',terms:'test',as_of:'2026-10-21',max_age_days:1,
        require_observed_quantity:true,require_observed_continuous_length:true};
      await input('market',market); await input('quotes',{schema:'ftx-textile-quotes/1',quotes:[quote]});
      assert.equal(await run('price'),'');
      const raw=await download('cut-price.json'), priced=JSON.parse(raw), order=priced.material_orders[0];
      assert.equal(priced.schema,'ftx-cut-procurement/2'); assert.equal(priced.material_orders.length,1);
      assert.deepEqual(order.roll_indices,[0,1]); assert.equal(order.required_length_m,0.24);
      assert.equal(order.required_continuous_length_m,0.12);
      assert.ok(Math.abs(order.least_observed_order.order_quantity-0.3)<1e-12);
      assert.ok(Math.abs(priced.least_observed_independent_order_total-3)<1e-12);
      const t=page.locator('table').filter({has:page.locator('caption',{hasText:'Material order 1 — latest supplier observations'})});
      assert.equal(await t.count(),1); assert.equal(await t.locator('tr').count(),2);
      assert.equal(await t.locator('td').nth(4).textContent(),`${order.least_observed_order.order_quantity} linear_meter`);
      assert.equal(await t.locator('td').nth(5).textContent(),String(order.least_observed_order.order_cost));
      assert.match(await page.textContent('#outputs'),/total 0.24 m, longest continuous segment 0.12 m/);
      assert.match(await page.textContent('#outputs'),/available stock apply once to the combined material order/);
      fs.writeFileSync(path.join(work,'bounded-plan.json'),planRaw,{flag:'wx'});
      fs.writeFileSync(path.join(work,'marker-2.svg'),svg,{flag:'wx'});
      fs.writeFileSync(path.join(work,'whole-order-price.json'),raw,{flag:'wx'});
      fs.writeFileSync(path.join(work,'bound-pattern.json'),await download('pattern.json'),{flag:'wx'});
      await page.screenshot({path:path.join(work,'bounded-purchasing.png'),fullPage:true});
      // A marker choice invalidates only its preview; the whole-order purchasing scope is unchanged.
      await page.fill('#rollPreview','1'); await page.locator('#rollPreview').dispatchEvent('change');
      assert.equal(await page.locator('a[download="roll-1.svg"]').count(),0);
      assert.equal(await download('cut-price.json'),raw);
      assert.equal(await run('cut'),''); assert.match(await download('roll-0.svg'),/data-copy="0"/);
      for (const [field,value,reason] of [['available_quantity',0.2,'insufficient_quantity'],
        ['maximum_continuous_quantity',0.1,'insufficient_continuous_length']]) {
        await input('quotes',{schema:'ftx-textile-quotes/1',quotes:[{...quote,[field]:value}]});
        assert.equal(await page.locator('a[download="cut-price.json"]').count(),0);
        assert.equal(await run('price'),'');
        const refused=JSON.parse(await download('cut-price.json'));
        assert.equal(refused.least_observed_independent_order_total,null);
        assert.equal(refused.material_orders[0].sample.excluded_offers[0].reason,reason);
        assert.equal(await t.locator('td').nth(3).textContent(),`Excluded: ${reason.replaceAll('_',' ')}`);
        assert.equal(await t.locator('td').nth(5).textContent(),'Unknown');
      }
      await input('market',{...market,as_of:'2026-10-22'});
      assert.equal(await page.locator('a[download="cut-price.json"]').count(),0);
      assert.ok(fs.existsSync(native),'Finite-stock pricing replay needs the native materials executable');
      await page.setInputFiles('#patternFile',fixture('synthetic-stock-pattern.json'));
      await page.waitForFunction(()=>!document.getElementById('bind').disabled);
      assert.equal(await run('bind'),'');
      const stockPattern=await download('pattern.json'),stockPatternFile=path.join(work,'finite-price-pattern.json');
      fs.writeFileSync(stockPatternFile,stockPattern,{flag:'wx'});
      const stockRequestFile=fixture('synthetic-stock-request.json'),stockRequest=fs.readFileSync(stockRequestFile,'utf8');
      await page.setInputFiles('#stockRequest',stockRequestFile);assert.equal(await run('stockIdentities'),'');
      const identities=await download('stock-identities.json');
      assert.deepEqual(JSON.parse(identities),JSON.parse(invoke('stock-identities',stockPatternFile,stockRequestFile)));
      assert.equal(await download('stock-identities-pattern.json'),stockPattern);
      assert.equal(await download('stock-identities-stock-request.json'),stockRequest);
      const original=load('synthetic-stock-pricing.json'),missing={...original,bindings:[]};
      const latest=structuredClone(original);latest.market.as_of='2026-10-21';latest.market.max_age_days=1;
      latest.quotes.quotes.push({...latest.quotes.quotes[0],observed_on:'2026-10-21',available:false});
      for(const [name,pricing,total] of [['complete',original,17],['missing',missing,null],['latest',latest,null]]) {
        const raw=name==='complete'?fs.readFileSync(fixture('synthetic-stock-pricing.json'),'utf8'):JSON.stringify(pricing);
        const pricingFile=path.join(work,`finite-${name}-pricing.json`);fs.writeFileSync(pricingFile,raw,{flag:'wx'});
        await page.setInputFiles('#stockPricing',pricingFile);
        assert.equal(await page.locator('a[download="stock-price.json"]').count(),0);
        assert.equal(await download('stock-identities.json'),identities);
        assert.equal(await run('stockPrice'),'');
        const report=JSON.parse(await download('stock-price.json'));
        assert.deepEqual(report,JSON.parse(invoke('stock-price',stockPatternFile,stockRequestFile,pricingFile)));
        assert.equal(report.least_observed_independent_order_total,total);
        assert.equal(report.execution_authorized,false);
        assert.equal(await download('stock-price-pattern.json'),stockPattern);
        assert.equal(await download('stock-price-stock-request.json'),stockRequest);
        assert.equal(await download('stock-price-pricing.json'),raw);
        const totalTable=page.getByRole('table',{name:'Complete finite-stock order',exact:true});
        assert.equal(await totalTable.locator('td').nth(2).textContent(),total===null?'Unknown':String(total));
        if(name==='latest') {
          assert.equal(report.stocks[0].offers.length,0);
          assert.match(await page.getByRole('table',{name:'hide-001: dated whole-item offers',exact:true}).textContent(),/Excluded: unavailable/);
        }
      }
      await input('stockPricing',{});assert.notEqual(await run('stockPrice'),'');
      assert.equal(await page.locator('a[download="stock-price.json"]').count(),0);
      await page.setInputFiles('#stockPricing',fixture('synthetic-stock-pricing.json'));
      await page.evaluate(()=>{document.getElementById('stockPrice').click();document.getElementById('cancel').click();});
      await page.waitForFunction(()=>document.getElementById('status').textContent.startsWith('Ready'));
      assert.equal(await page.locator('a[download="stock-price.json"]').count(),0);
      assert.equal(await run('stockPrice'),'');await page.locator('#stockRequest').dispatchEvent('change');
      assert.equal(await page.locator('a[download="stock-price.json"]').count(),0);
      assert.equal(await page.locator('a[download="stock-identities.json"]').count(),0);
      assert.equal(await run('stockIdentities'),'');
      await input('patternFile',{schema:'fabric-pattern/1',pieces:[]});
      assert.equal(await page.locator('a[download="stock-identities.json"]').count(),0);
      assert.deepEqual(errors,[]); assert.deepEqual(external,[]); assert.ok(methods.every(m=>m==='GET'));
      console.log('PASS: real Chromium renders bounded roll orders and exact finite-stock identities/prices matching native, retains source bytes and unknown totals, excludes insufficient stock and newer unavailable offers, and withdraws stale prices on edits/refusal/cancellation. Synthetic observations only; no live purchasing, fulfillment, optimal nesting or machine claim. Artifacts:',work);
      return;
    }
    if (compressionProof) {
      const preparation=load('synthetic-compression.json'); preparation.pressure_pa=0;
      // Only this invented specimen has branch-independent reference observations. Real source
      // restrictions must never be removed to obtain acceptance.
      for (const q of Object.values(preparation.material.properties)) delete q.conditions.loading_branch;
      const stack={schema:'ftx-compression-stack/1',gap_m:0.020,reference_area_m2:0.25,
        direction:'loading',allow_estimates:true,layers:[{id:'layer',preparation}]};
      const entry=structuredClone(stack); entry.gap_m=0.010; entry.direction='unloading';
      const p=entry.layers[0].preparation; p.pressure_pa=500; p.loading_branch='unloading-after-500';
      for (const sample of p.samples) for (const q of Object.values(sample.properties)) q.conditions.loading_branch=p.loading_branch;
      for (const q of Object.values(p.invariants)) q.conditions.loading_branch=p.loading_branch;
      const change={entry,transitions:{layer:{source:'synthetic',evidence:'estimate',
        method:'Invented matched reversible branch pair after a 500 Pa peak; software verification only',
        conditions:{specimen:p.specimen,conditioning:p.conditioning,from_loading_branch:'loading',
          to_loading_branch:p.loading_branch,pressure_pa:'500'}}}};
      const hold=gap_m=>({seconds:10,gap_m,interface_resistance_m2_k_w:[],inside_c:40,outside_c:0,
        inside_resistance_m2_k_w:0,outside_resistance_m2_k_w:0});
      const setup={stack,cells_per_layer:1,initial_temperature_c:[0],
        thermal_evidence:'Synthetic constant storage and explicit reservoirs/films; no work-to-heat conversion'};
      const cycle={schema:'ftx-compressed-thermal-history/2',setup,
        legs:[{holds:[hold(0.010)]},{branch_change:change,holds:[hold(0.020)]}]};
      const pattern=load('synthetic-stock-pattern.json'); await input('patternFile',pattern);
      await page.waitForFunction(()=>!document.getElementById('bind').disabled);
      await page.fill('#compressionLayer','layer');
      const upload=async request=>{
        const raw=' '+JSON.stringify(request).replace('"reference_area_m2":0.25','"reference_area_m2":0.2500')+'\n';
        await page.setInputFiles('#compressionRequest',{name:'history.json',mimeType:'application/json',buffer:Buffer.from(raw)});
        return raw;
      };
      await upload({...stack,gap_m:0.015}); assert.equal(await run('pressure'),'');
      const preparedStack=JSON.parse(await download('compression-stack-report.json'));
      assert.equal(preparedStack.schema,'ftx-compression-stack-report/1');
      assert.ok(Math.abs(preparedStack.pressure_pa-250)<1e-9);
      assert.equal(await download('compression-layer-1-selection.json'),preparedStack.layers[0].conditioned.selection_json);
      let selected;
      for (const adaptive of [false,true]) for (const version of [1,2]) {
        const request=structuredClone(version===1?{schema:'ftx-compressed-thermal-history/1',setup,holds:[hold(0.010)]}:cycle);
        if(adaptive) request.adaptive={absolute_temperature_tolerance_k:1e-6,absolute_heat_tolerance_j_m2:1e-4,
          relative_tolerance:0,min_step_s:0.00001,max_step_s:10,max_trials:10000,max_cell_solves:262144};
        const original=await upload(request); assert.equal(await run('pressure'),'');
        const raw=await download('compression-thermal-report.json'), report=JSON.parse(raw);
        assert.equal(await page.locator('a[download="compression-stack-report.json"]').count(),0);
        assert.equal(report.schema,`ftx-compressed-thermal-history-report/${version}`);
        assert.equal(report.steps.length,version); assert.equal(await download('compression-request.json'),original);
        const t=page.locator('table').filter({has:page.locator('caption',{hasText:'Compression and heat history — chronological holds'})});
        assert.equal(await t.locator('tr').count(),version+1);
        let temperature=0, chargedCells=0;
        for (const [i,step] of report.steps.entries()) {
          const resistance=[0.25,0.5][i];
          if(adaptive) {
            temperature=20+(temperature-20)*Math.exp(-40/(100*resistance));
            assert.ok(Math.abs(step.temperature_c[0]-temperature)<0.01);
            const d=step.integration;
            assert.ok(d.max_accepted_error_ratio<=1); assert.ok(d.accepted_substeps>2);
            chargedCells+=d.cell_solves; assert.equal(d.cumulative_cell_solves,chargedCells);
            assert.ok(Math.abs(step.cumulative_stored_change_j-25*step.temperature_c[0])<1e-8);
            assert.ok(Math.abs(step.cumulative_heat_balance_error_j)<1e-8);
            const diagnostics=page.locator('table').filter({has:page.locator('caption',{hasText:'Adaptive thermal integration — local estimates and charged work'})});
            assert.deepEqual(await diagnostics.locator('tr').nth(i+1).locator('td').allTextContents(),
              [step.elapsed_s,d.accepted_substeps,d.rejected_trials,d.cell_solves,d.cumulative_cell_solves,d.max_accepted_error_ratio].map(String));
          } else {
            temperature=(10*temperature+2/resistance*40)/(10+4/resistance);
            assert.ok(Math.abs(step.temperature_c[0]-temperature)<1e-10);
            assert.ok(Math.abs(step.cumulative_stored_change_j-25*temperature)<1e-9);
            assert.ok(Math.abs(step.cumulative_heat_balance_error_j)<1e-9);
            assert.equal(step.integration,undefined);
          }
          assert.ok(Math.abs(step.cumulative_external_work_j-[0.625,0][i])<1e-10);
          assert.equal(step.elapsed_s,(i+1)*10); assert.equal(step.reference_mass_kg,0.025);
          assert.equal(await t.locator('tr').nth(i+1).locator('td').nth(3).textContent(),String(step.temperature_c[0]));
        }
        if(version===2) {assert.equal(report.transitions.length,1);assert.equal(report.transitions[0].before_hold,1);}
        selected=report.final_compression.layers[0].conditioned.selection_json;
        assert.equal(await download('compression-layer-1-selection.json'),selected);
        assert.equal(await page.inputValue('#selection'),selected);
        assert.match(await page.textContent('#outputs'),/External mechanical work is separate from sensible heat/);
        const prefix=adaptive?'adaptive-':'';
        fs.writeFileSync(path.join(work,`${prefix}history-v${version}-request.json`),original,{flag:'wx'});
        fs.writeFileSync(path.join(work,`${prefix}history-v${version}-report.json`),raw,{flag:'wx'});
        if(adaptive && version===2) {
          const limited=structuredClone(request); limited.adaptive.max_cell_solves=report.steps[0].integration.cell_solves;
          await upload(limited); assert.notEqual(await run('pressure'),'');
          assert.equal(await page.locator('a[download="compression-layer-1-selection.json"]').count(),0);
          assert.equal(await page.isDisabled('#assign'),true);
          await upload(request); assert.equal(await run('pressure'),'');
          assert.equal(await download('compression-thermal-report.json'),raw);
        }
      }
      await page.click('#assign'); assert.equal(await run('bind'),'');
      const bound=JSON.parse(await download('pattern.json'));
      assert.deepEqual(bound.pieces,pattern.pieces);
      assert.equal(bound.fabric.material.expected_sha256,JSON.parse(selected).expected_sha256);
      fs.writeFileSync(path.join(work,'history-bound-pattern.json'),await download('pattern.json'),{flag:'wx'});
      await page.screenshot({path:path.join(work,'compression-history.png'),fullPage:true});
      await page.fill('#compressionLayer','missing');
      assert.equal(await page.locator('a[download="compression-thermal-report.json"]').count(),0);
      assert.equal(await page.isDisabled('#assign'),true);
      assert.match(await run('pressure'),/layer ID/);
      await page.fill('#compressionLayer','layer');
      for (const mutate of [
        r=>{r.legs[1].branch_change.transitions={};},
        r=>{r.legs[1].holds[0].outside_c=-300;},
        r=>{r.legs[1].branch_change.heat_fraction=0.9;}
      ]) {
        const bad=structuredClone(cycle); mutate(bad); await upload(bad);
        assert.notEqual(await run('pressure'),'');
        assert.equal(await page.locator('a[download="compression-thermal-report.json"]').count(),0);
        assert.equal(await page.locator('a[download="compression-layer-1-selection.json"]').count(),0);
        assert.equal(await page.isDisabled('#assign'),true);
      }
      assert.deepEqual(errors,[]); assert.deepEqual(external,[]); assert.ok(methods.every(m=>m==='GET'));
      console.log('PASS: real Chromium runs fixed and adaptive heat in both compression history versions, matches independent RC answers, displays native local-error/work diagnostics, retains exact source and selections, binds without geometry changes and refuses aggregate exhaustion/missing links/late heat/unknown fields. Synthetic software evidence, no global-error, measured textile/contact/fit claim. Artifacts:',work);
      return;
    }
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
