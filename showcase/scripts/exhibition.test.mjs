// Test the served static export, including real media and native specimen reports.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { after, before, test } from "node:test";
import { chromium } from "playwright-core";

const site = process.env.FTX_STUDIO_URL || "http://127.0.0.1:3000";
let browser;
before(async () => {
  browser = await chromium.launch({ executablePath: process.env.CHROMIUM_EXECUTABLE || chromium.executablePath(),
    args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"] });
});
after(async () => { await browser?.close(); });
const opts = { userAgent: "OpenAI File Downloader, XaiImageApiFetch/1.0", viewport: { width: 1120, height: 820 } };

test("every construction study pairs a full-size cut with its three film chapters",async()=>{
  const page=await browser.newPage({...opts,reducedMotion:"reduce",viewport:{width:390,height:844}});
  try{
    await page.goto(site);
    const stage=page.locator("#construction"),select=stage.getByRole("combobox");
    const choices=await select.locator("option").evaluateAll(options=>options.map(o=>o.value));
    assert.equal(choices.length,10);
    for(const garment of choices){
      await select.selectOption(garment);
      const motion=stage.locator(`[data-garment="${garment}"]`);
      for(const chapter of ["Flat pattern","Match the edges","Sewn article"]){
        await motion.getByRole("button",{name:chapter,exact:true}).click();
        await motion.locator(".construction-still").evaluate(img=>img.decode());
        assert.equal(await motion.locator("video").evaluate(v=>v.paused),true);
      }
      await motion.getByRole("button",{name:"Play construction film"}).click();
      await page.waitForFunction(()=>document.querySelector("#construction video")?.currentTime>0.05);
      assert.equal(await motion.locator("video").evaluate(v=>Math.round(v.duration*16)),96);
      for(const url of await stage.locator("a[download]").evaluateAll(links=>links.map(l=>l.href))){
        const pdf=await page.request.get(url);
        assert.equal((await pdf.body()).subarray(0,5).toString(),"%PDF-");
      }
    }
    assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  }finally{await page.close();}
});

test("the skirt atelier displays the selected native trajectory and its matching cut", async () => {
  const page=await browser.newPage({...opts,reducedMotion:"reduce"});
  try {
    await page.goto(new URL("/studies/skirt/?cut=column&waist=800",site).href,{waitUntil:"domcontentloaded"});
    await page.locator('canvas[data-run="column-800"]').waitFor();
    assert.ok(await page.getByRole("button",{name:"Flared",exact:true}).isEnabled());
    const data=await (await page.request.get(new URL("/atelier/study.json",site).href)).json();
    for(const cut of ["column","flare"]){
      await page.getByRole("button",{name:"68 centimetre form waist"}).click();
      await page.getByRole("button",{name:cut==="column"?"Column":"Flared",exact:true}).click();
      for(const [key,run] of Object.entries(data.runs).filter(([k])=>k.startsWith(cut+"-"))){
        await page.getByRole("button",{name:run.waist/10+" centimetre form waist"}).click();
        await page.locator('canvas[data-run="'+key+'"]').waitFor();
        await page.locator(".sa-intro").getByRole("button",{name:"Flat pattern"}).click();
        await page.locator('canvas[data-run="'+key+'"][data-native-frame="0"]').waitFor();
        await page.getByRole("button",{name:"Join gores"}).click();
        await page.locator('canvas[data-run="'+key+'"][data-native-frame="87"]').waitFor();
        await page.getByRole("button",{name:"Sewn skirt"}).click();
        const mm=Object.values(run.hemHeights);
        assert.equal(await page.locator(".sa-reading strong").nth(1).innerText(),(Math.max(...mm)-Math.min(...mm)).toFixed(1)+" mm");
        const pattern=await page.locator(".sa-pdf").getAttribute("href");
        assert.equal(pattern,"/atelier/"+run.patternStem+".pdf");
        const pdf=await page.request.get(new URL(pattern,site).href);
        assert.equal(pdf.status(),200);
        assert.equal((await pdf.body()).subarray(0,5).toString(),"%PDF-");
        assert.match(await page.locator(".sa-section").innerText(),/Centre-front gap/);
      }
    }
    await page.getByRole("button",{name:"Compare hems"}).click();
    await page.locator('canvas[data-surface="Hems"]').waitFor();
    await page.getByRole("button",{name:"Strain",exact:true}).click();
    await page.locator('canvas[data-surface="Strain"][data-step="100"]').waitFor();
    await page.getByRole("button",{name:"Replay drape"}).click();
    await page.waitForFunction(()=>Number(document.querySelector(".sa-canvas canvas").dataset.step)<95);
    await page.setViewportSize({width:375,height:812});
    assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  } finally {await page.close();}
});

test("the drape room pairs each body and pose with its native cloth and endpoint loads", async () => {
  const page=await browser.newPage({...opts,reducedMotion:"reduce"});
  try {
    await page.goto(site);
    const reports=await (await page.request.get(new URL("/exhibits/poses/runs.json",site).href)).json();
    for(const chest of [48,52,54])for(const angle of [35,75,90]){
      const id=chest*10+"-"+angle,run=reports.cases[id];
      await page.getByRole("button",{name:chest+" centimetre chest"}).click();
      await page.getByRole("button",{name:angle+" degree arm pose"}).click();
      await page.locator('canvas[data-run="'+id+'"]').waitFor({timeout:30000});
      assert.equal(await page.locator(".dr-readings strong").nth(1).textContent(),run.chestClearance.toFixed(1)+" mm");
      assert.equal(await page.locator(".dr-method summary span").textContent(),
        !run.settled?"Cloth still moving":run.flags.length?"Review solver flags":"Settled");
      const method=page.locator(".dr-method");
      const tapes=await method.locator('a[download][href*="/chest-"]').getAttribute("href");
      assert.equal(tapes,`/exhibits/poses/chest-${chest*10}.json`);
      if(angle===35){
        const input=await (await page.request.get(new URL(tapes,site).href)).json();
        assert.equal(input.records.find(r=>r.name==="chest").value_mm,chest*10);
        assert.equal(input.records.find(r=>r.name==="hand_pass").value_mm,145);
      }
      const command=await method.locator("pre").textContent();
      assert.ok(command.includes(`--measurements chest-${chest*10}.json --fit fit-spec.json`));
      assert.ok(command.includes(`--arm-deg ${angle} `) && command.includes(`ftx viewer bolero-${id} --check-contact`));
      assert.ok((await method.textContent()).includes(run.sourcePc2Sha256));
      await page.getByRole("button",{name:"Contact",exact:true}).click();
      await page.locator(`canvas[data-mode="contact"][data-frame="${run.sourceFrames-1}"]`).waitFor();
      assert.equal(await page.locator(".dr-readings strong").last().textContent(),run.peakBodyForceN.toFixed(2)+" N");
    }
    await page.getByRole("button",{name:"Lay the five cut pieces flat"}).click();
    await page.locator('canvas[data-mode="construction"][data-frame="0"]').waitFor();
    const timeline=page.getByRole("slider",{name:"Sewing replay frame"});
    await timeline.fill("86");
    await page.locator('canvas[data-frame="86"][data-native-frame="69"]').waitFor();
    await timeline.fill("87");
    await page.locator('canvas[data-frame="87"][data-native-frame="87"]').waitFor();
    await timeline.fill("88");
    await page.locator('canvas[data-frame="88"][data-native-frame="88"]').waitFor();
    await timeline.press("Home");
    await page.getByRole("button",{name:"Play sewing replay"}).click();
    await page.waitForFunction(()=>Number(document.querySelector(".dr-stage canvas")?.dataset.frame)>2);
    await page.getByRole("button",{name:"Pause sewing replay"}).click();
    await page.setViewportSize({width:390,height:844});
    await page.getByRole("button",{name:"52 centimetre chest"}).click();
    await page.getByRole("button",{name:"35 degree arm pose"}).click();
    await page.getByRole("button",{name:"Coupled outfit"}).click();
    await page.locator('canvas[data-run="520-35"][data-coupled="true"][data-native-frame="135"]').waitFor();
    assert.equal(await page.locator(".dr-readings strong").nth(1).textContent(),"-2.2 mm");
    await page.getByRole("slider",{name:"Sewing replay frame"}).press("Home");
    await page.locator('canvas[data-coupled="true"][data-native-frame="0"]').waitFor();
    assert.ok(await page.locator(".dr-settings a[download]").count()===2);
    assert.equal(await page.locator('.dr-method a[href*="/chest-"]').count(),0);
    assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  } finally {await page.close();}
});

test("the photo workbench detects the demo sheet, measures the marks and clears the demo on upload", async () => {
  const page=await browser.newPage({...opts,viewport:{width:390,height:844}});
  try {
    await page.goto(new URL("/workbench/photo.html",site).href);
    await page.locator("#values tr").first().waitFor({timeout:60000});
    const cells=await page.locator("#values tr").first().locator("td").allTextContents();
    assert.equal(cells[0],"chalk_span");
    assert.ok(Math.abs(Number(cells[1])-200)<.5);
    assert.ok(Number(cells[2])>0);
    assert.equal(await page.locator("#image").evaluate(canvas=>canvas.width),1200);
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
    const source=await page.request.get(new URL("/workbench/atelier-assets/photo-example-sleeve.jpg",site).href);
    await page.locator("#photo").setInputFiles({name:"own-sleeve.jpg",mimeType:"image/jpeg",buffer:await source.body()});
    assert.ok(await page.locator("#result").isHidden());
    assert.equal(await page.locator("#points").inputValue(),"{}");
    await page.locator("#analyze").click();
    await page.locator("#preview").waitFor();
    assert.equal(await page.locator("#points").inputValue(),"{}");
  } finally { await page.close(); }
});

test("each gallery garment has a downloadable full-size sewing PDF", async () => {
  const page=await browser.newPage(opts);
  try {
    for (const [route,expected] of [
      ["/sections/wardrobe/",["/sundress/pattern-A4.pdf","/patterns/hussar-blue.pdf","/patterns/hussar-yellow.pdf"]],
      ["/sections/studies/",["/patterns/bolero.pdf","/patterns/trousers.pdf"]],
      ["/sections/tracing/",["/patterns/bolero.pdf"]],
    ]) {
      await page.goto(new URL(route,site).href);
      for(const path of expected) {
        assert.ok(await page.locator('a[download][href="'+path+'"]').first().isVisible());
        const pdf=await page.request.get(new URL(path,site).href);
        assert.equal(pdf.status(),200);
        assert.equal((await pdf.body()).subarray(0,5).toString(),"%PDF-");
      }
    }
    await page.goto(new URL("/sections/films/",site).href);
    await page.getByRole("button",{name:/Lightly eased sundress/}).click();
    assert.ok(await page.locator('.screening a[href="/exhibits/sundress/light-A4.pdf"]').isVisible());
    await page.getByRole("button",{name:/The hussar’s cabinet/}).click();
    assert.ok(await page.locator('.screening a[href="/patterns/hussar-yellow.pdf"]').isVisible());
    await page.getByRole("button",{name:/Anatomy of a seam/}).click();
    assert.ok(await page.locator('.screening a[href="/patterns/bolero.pdf"]').isVisible());
  } finally {await page.close();}
});

test("visible preview plays, preserves a deliberate pause, and leaves the screen quiet", async () => {
  const p = await browser.newPage({ ...opts, reducedMotion:"no-preference" });
  try {
    await p.goto(site);
    await p.locator(".hero-screen").scrollIntoViewIfNeeded();
    await p.waitForFunction(()=>document.querySelector(".hero-screen video")?.currentTime>.1);
    await p.getByRole("button",{name:"Pause Inside the wardrobe preview"}).click();
    await p.locator("#tracing").scrollIntoViewIfNeeded();
    await p.locator(".hero-screen").scrollIntoViewIfNeeded();
    assert.equal(await p.locator(".hero-screen video").evaluate(v=>v.paused),true);
    await p.getByRole("button",{name:"Play Inside the wardrobe preview"}).click();
    await p.waitForFunction(()=>document.querySelector(".hero-screen video")?.paused===false);
    await p.locator("#materials").scrollIntoViewIfNeeded();
    await p.waitForFunction(()=>document.querySelector(".hero-screen video")?.paused===true);
    await p.waitForFunction(()=>Array.from(document.querySelectorAll("#materials video")).some(v=>v.currentTime>.1));
  } finally { await p.close(); }
});

test("reduced motion leaves the film paused until requested", async () => {
  const p=await browser.newPage({...opts,reducedMotion:"reduce",viewport:{width:390,height:844}});
  try {
    await p.goto(site);await p.getByRole("button",{name:"Play Inside the wardrobe preview"}).waitFor();
    const hero=p.locator(".hero-screen video");
    assert.equal(await hero.evaluate(v=>v.paused && v.getAttribute("src")===null),true);
    await p.getByRole("button",{name:"Play Inside the wardrobe preview"}).click();
    await p.waitForFunction(()=>document.querySelector(".hero-screen video")?.currentTime>.1);
    assert.equal(await p.evaluate(()=>document.documentElement.scrollWidth),390);
  } finally {await p.close();}
});

test("strip controls and readouts agree with the exported Rust states", async () => {
  const p=await browser.newPage({...opts,reducedMotion:"reduce"});
  try {
    await p.goto(new URL("/experiments/strip/",site).href);
    await p.getByRole("button",{name:"4% pull"}).click();
    const rows=await p.locator("tbody tr td:first-of-type").allTextContents();
    const expected=await p.evaluate(async()=>{
      const samples=await (await fetch("/materials/strip.json")).json();
      return samples.map(({report})=>report.states.find(s=>s.extension_fraction===.04).force_n.toFixed(3)+" N");
    });
    assert.deepEqual(rows,expected);
    await p.locator(".rig-scene canvas[data-step='4']").waitFor();
    assert.deepEqual(await p.locator(".rig-measured strong").allTextContents(),["0.12 N","-0.01 N","0.04 N"]);
    await p.getByRole("button",{name:"Weft",exact:true}).click();
    await p.locator('canvas[data-specimen="Weft"]').waitFor();
    await p.goto(new URL("/experiments/cylinder/",site).href);
    await p.getByRole("button",{name:"Compressed",exact:true}).click();
    assert.deepEqual(await p.locator(".rig-refinement b").allTextContents(),["-0.63 N","-0.97 N"]);
    await p.getByText("Solver notes and residuals",{exact:true}).click();
    assert.ok(await p.getByText("48 × 12 mesh: the solve did not converge.",{exact:true}).isVisible());
    await p.getByRole("button",{name:"Mid-section",exact:true}).click();
    assert.equal(await p.getByRole("button",{name:"Mid-section",exact:true}).getAttribute("aria-pressed"),"true");
  } finally {await p.close();}
});

test("sundress ease uses the fitted Anny tapes and preserves the fixed cut", async () => {
  const p=await browser.newPage({...opts,reducedMotion:"reduce",viewport:{width:390,height:844}});
  try {
    await p.goto(new URL("/wardrobe/sundress/",site).href);
    await p.locator(".sd-scene canvas[data-ready]").waitFor();
    const [cases,body]=await p.evaluate(()=>Promise.all(["/sundress/fit-comparison.json","/fitted-bodies/sundress.json"].map(async url=>(await fetch(url)).json())));
    const reference=body.samples.find(sample=>sample.name==="reference");
    for(const item of cases.runs) {
      await p.getByRole("button",{name:item.form,exact:true}).click();
      for(const point of item.points){
        let tape=reference.outcomes[point.id];
        for(const target of item.points){
          const sample=body.samples.find(s=>s.name===target.id+"High");
          const t=(target.body_mm/10-reference.targets[target.id])/(sample.targets[target.id]-reference.targets[target.id]);
          tape+=t*(sample.outcomes[point.id]-reference.outcomes[point.id]);
        }
        const row=p.locator(".sd-table>div").filter({hasText:point.id});
        await p.waitForFunction(({id,v})=>document.querySelector('[data-fitted="'+id+'"]').textContent===v,{id:point.id,v:tape.toFixed(1)});
        assert.equal(Number(await row.locator("span").first().textContent()),point.finished_mm/10);
        assert.equal(Number(await row.locator("b").textContent()),Number((point.finished_mm/10-tape).toFixed(1)));
      }
    }
    await p.getByRole("button",{name:"Reset",exact:true}).click();
    await p.getByRole("slider",{name:"Body mass target"}).press("End");
    const fittedWeight=body.samples.find(s=>s.name==="weightHigh").outcomes.weight.toFixed(1);
    await p.locator('[data-model-mass="'+fittedWeight+'"]').waitFor();
    assert.notEqual(Number(fittedWeight),body.domains.weight[1]);
    const stage=p.locator(".sd-scene canvas"),before=await stage.screenshot();
    await p.getByRole("button",{name:"Raised",exact:true}).click();
    assert.equal(await p.getByRole("button",{name:"Raised",exact:true}).getAttribute("aria-pressed"),"true");
    assert.notDeepEqual(await stage.screenshot(),before);
    assert.equal(await p.getByRole("link",{name:"Three native flat checks"}).getAttribute("href"),"/sundress/fit-comparison.json");
    // The solver's drape of the same pattern replays the packaged frames: the settled endpoint first, the flat cut at frame zero.
    const sewn=p.locator(".sd-making canvas[data-ready]"),summary=await (await p.request.get(new URL("/exhibits/sundress/dress-summary.json",site).href)).json();
    await sewn.waitFor();
    // The band can shorten while each skirt stays held at the waist.
    assert.equal(summary.settled,true);
    assert.deepEqual(summary.gating,[]);
    for(const side of ["skirt_front","skirt_back"])assert.ok(summary.flags.some(flag=>flag.startsWith(side)&&flag.includes("did not slide")));
    const endpoint=await sewn.getAttribute("data-frame");
    assert.equal(Number(endpoint),Object.values(summary.phases).reduce((a,b)=>a+b)-1);
    await p.getByRole("slider",{name:"Sewing replay frame"}).fill("0");
    await p.waitForFunction(()=>document.querySelector(".sd-making canvas").dataset.frame==="0");
    // The flat pattern is shown under the stage, one outline per cut piece, and the four stages are named.
    assert.equal(await p.locator(".sd-making .mk-pattern path").count(),summary.instances.length);
    assert.deepEqual(await p.locator(".sd-making .mk-steps button").allTextContents(),["1Cut flat","2Shape the panels","3Seams sewn","4Hung under gravity"]);
    assert.equal(await p.locator(".sd-making .mk-steps .is-now").textContent(),"1Cut flat");
    const cutSelector=p.getByRole("group",{name:"Skirt waist cut"});
    const sections=[];
    for(const [id,title] of [["light","Lightly eased waist"],["dress","Gathered waist"]]){
      await cutSelector.getByRole("button",{name:new RegExp(title)}).click();
      const base=new URL("/exhibits/sundress/"+id,site).href;
      const native=await (await p.request.get(base+"-summary.json")).json();
      await sewn.waitFor(); await p.locator(".sd-making .mk-steps button").nth(3).click();
      await p.waitForFunction(frame=>document.querySelector(".sd-making canvas")?.dataset.frame===String(frame),
        Object.values(native.phases).reduce((a,b)=>a+b)-1);
      assert.equal(await p.locator("#sewn h3").textContent(),title);
      const section=await (await p.request.get(base+"-section.json")).json();
      assert.equal(native.settled,true);
      assert.deepEqual(native.gating,[]);
      for(const flag of native.flags){
        const reading=flag.startsWith("the body rides up ")
          ? `The front hem settled ${section.hemRiseMm.front} mm above its starting height, and the back hem settled ${section.hemRiseMm.back} mm above its starting height.`
          : flag;
        await p.locator(".sd-sewn .sd-flag",{hasText:reading}).waitFor();
      }
      sections.push(section); assert.ok((await p.locator(".sd-slice").textContent()).includes(section.frontGapMm+" mm"));
      const href=await p.getByRole("link",{name:"Full-size A4 pattern for this cut"}).getAttribute("href");
      const pdf=await p.request.get(new URL(href,site).href);
      assert.equal((await pdf.body()).subarray(0,5).toString(),"%PDF-");
    }
    assert.equal(sections[0].heightMm,sections[1].heightMm);
    assert.deepEqual(sections[0].body,sections[1].body);
    assert.ok(sections[0].frontGapMm<sections[1].frontGapMm);
    assert.notDeepEqual(sections[0].cloth,sections[1].cloth);
    assert.equal(await p.evaluate(()=>document.documentElement.scrollWidth),390);
  } finally {await p.close();}
});

test("a sewist can compare two muslins privately and retrieve the instructions for the selected cut", async () => {
  const page = await browser.newPage({...opts, reducedMotion:"reduce", viewport:{width:390,height:844}});
  try {
    await page.goto(new URL("/wardrobe/sundress/?garment=eased#muslin", site).href);
    const trial = page.locator("#muslin"), notes = page.getByLabel("Form, cloth, repeat readings and discrepancies");
    const slice = trial.getByRole("img", {name:/Both waist cuts/});
    assert.equal(await slice.locator("path").count(), 3);
    assert.equal(await slice.locator("line").count(), 1);
    const save = trial.getByRole("button", {name:"Save worksheet"});
    const download = async () => {
      const pending = page.waitForEvent("download");
      await save.click();
      return readFile(await (await pending).path(), "utf8");
    };
    assert.match(await download(), /observed ____ mm/);
    const front = trial.getByRole("group", {name:"Centre front"});
    const [gathered, eased] = await front.getByRole("textbox").all();
    await gathered.fill("0"); await eased.fill("7,5");
    assert.match(await front.locator("output").innerText(), /predicted -59.6 mm · observed \+7.5 mm · residual \+67.1 mm/);
    await notes.fill("Synthetic browser check only.");
    const sheet = await download();
    assert.match(sheet, /predicted 90.7 mm, observed 0 mm, residual -90.7 mm/);
    assert.match(sheet, /predicted -59.6 mm, observed \+7.5 mm, residual \+67.1 mm/);
    assert.match(sheet, /Synthetic browser check only/);
    assert.match(sheet, /Native trajectory SHA-256 [a-f0-9]{64}/);
    await eased.fill("-1"); assert.equal(await save.isDisabled(), true);
    await eased.fill(""); assert.equal(await save.isEnabled(), true);
    assert.doesNotMatch(await front.locator("output").innerText(), /observed/);
    const instructions = page.locator("details").filter({has:page.getByText("Cut and sew the dress", {exact:true})});
    await instructions.locator("summary").click();
    for (const [id, name, tiles, edge, svg] of [["light","Lightly eased waist",36,"409.5","/exhibits/sundress/light-master.svg"],["dress","Gathered waist",37,"507","/sundress/master.svg"]]) {
      await page.getByRole("group",{name:"Skirt waist cut"}).getByRole("button",{name:new RegExp(name)}).click();
      const body = await instructions.innerText();
      assert.ok(body.includes(`${tiles} tiles`) && body.includes(`${edge} mm skirt waist`));
      assert.equal(await page.getByRole("link", {name:"Download FrankenTextiles pattern"}).getAttribute("href"), `/exhibits/sundress/${id}-pattern.json`);
      assert.equal(await page.getByRole("link", {name:"Download full-scale pattern SVG"}).getAttribute("href"), svg);
      const drawing = await page.request.get(new URL(svg,site).href);
      assert.match(await drawing.text(), /fp:schema="fabric-pattern\/1"/);
    }
    await page.reload();
    assert.equal(await notes.inputValue(), ""); assert.equal(await gathered.inputValue(), "");
  } finally { await page.close(); }
});

test("the hussar wardrobe connects the dressed figure, pattern front and native sewing replay", async () => {
  const p=await browser.newPage({...opts,reducedMotion:"reduce"});
  try {
    await p.goto(new URL("/wardrobe/hussar/",site).href);
    const recipe = p.locator("details.hu-flags").filter({ has: p.getByText("Replay this jacket with FrankenTextiles", { exact: true }) });
    await recipe.locator("summary").click();
    const summary = await (await p.request.get(new URL("/exhibits/hussar/dolman-summary.json", site).href)).json();
    const measurements = recipe.getByRole("link", { name: "synthetic body measurements" });
    const tapes = await (await p.request.get(new URL(await measurements.getAttribute("href"), site).href)).json();
    assert.equal(tapes.fused.chest.value_mm, 1000);
    const command = await recipe.textContent();
    assert.ok(command.includes(summary.solverArguments.join(" ")) && command.includes(summary.sourcePc2Sha256));
    assert.ok(command.includes("ftx viewer dolman --check-contact"));
    assert.ok((await p.locator(".hu-readings").textContent()).includes(`${summary.bodyOffsetMm.achieved.toFixed(3)} / ${summary.bodyOffsetMm.target} mm`));
    const book=p.locator(".regimental-cuts");
    await book.scrollIntoViewIfNeeded();
    for(const regiment of ["blue","yellow"]){
      await book.getByRole("combobox",{name:"Regiment",exact:true}).selectOption(regiment);
      await book.getByRole("button",{name:"Cap",exact:true}).click();
      await book.locator('option[value="'+(regiment==="blue"?"mirliton_wall":"busby_wall")+'"]').waitFor({state:"attached"});
      const seen=new Set();
      for(const article of await book.locator(".regimental-items button").all()){
        await article.click();
        for(const id of await book.getByLabel("Flat piece").locator("option").evaluateAll(o=>o.map(x=>x.value)))seen.add(id);
        assert.ok(await book.locator(".regimental-cut svg path.dl-cut").count()>0);
      }
      assert.equal(seen.size,49);
    }
    const cabinet=p.locator("#dress-study");
    await cabinet.scrollIntoViewIfNeeded();
    await cabinet.locator("canvas[data-ready='Compare-Dressed']").waitFor({timeout:90000});
    await cabinet.getByRole("radio",{name:"Yellow",exact:true}).check();
    await cabinet.getByRole("radio",{name:"Side",exact:true}).check();
    await cabinet.getByRole("radio",{name:"Pelisse",exact:true}).check();
    await cabinet.getByRole("radio",{name:"Flat",exact:true}).check();
    await cabinet.locator("canvas[data-ready='Yellow-Pelisse'][data-piece='pelisse_front']").waitFor();
    const pattern=await (await p.request.get(new URL("/patterns/hussar-yellow.json",site).href)).json();
    const front=pattern.pieces.find(piece=>piece.id==="pelisse_front");
    assert.equal(await cabinet.locator("dt",{hasText:"Cord paths per cut"}).locator("..").locator("dd").textContent(),
      String(front.decor.filter(row=>row.kind==="soutache").length));
    await cabinet.getByRole("link",{name:"Watch the dolman sew together"}).click();
    const replay=p.locator("#native-dolman");
    await replay.getByRole("button",{name:"Yellow",exact:true}).click();
    await replay.getByRole("slider",{name:"Sewing replay frame"}).press("Home");
    await replay.locator("canvas[data-ready='Yellow'][data-frame='0']").waitFor();
    for (const width of [1120,375]) {
      await p.setViewportSize({width,height:820});
      const stage=replay.locator(".mk-stage");
      const [controls, canvas, caption]=await Promise.all([".mk-steps","canvas",".hu-tag"].map(selector=>stage.locator(selector).boundingBox()));
      assert.ok(controls.y+controls.height<=canvas.y+1 && canvas.y+canvas.height<=caption.y+1,
        "The entire cut has its own viewport between the controls and the caption");
    }
    const file=p.getByRole("link",{name:"Download DST"});
    const dst=await file.evaluate(async a=>new Uint8Array(await (await fetch(a.href)).arrayBuffer()).slice(0,3).join(","));
    assert.equal(dst,"76,65,58"); // Tajima header begins with "LA:".
  } finally {await p.close();}
});
