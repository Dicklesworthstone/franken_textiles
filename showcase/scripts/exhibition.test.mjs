// Test the served static export, including real media and native specimen reports.
import assert from "node:assert/strict";
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
        run.flags.length||run.trousers.flags?.length?(run.settled?"Contact thickness incomplete":"Cloth still moving"):"Settled");
      await page.getByRole("button",{name:"Contact",exact:true}).click();
      await page.locator('canvas[data-mode="contact"][data-frame="135"]').waitFor();
      assert.equal(await page.locator(".dr-readings strong").last().textContent(),run.peakBodyForceN.toFixed(2)+" N");
    }
    await page.getByRole("button",{name:"Lay the five cut pieces flat"}).click();
    await page.locator('canvas[data-mode="construction"][data-frame="0"]').waitFor();
    await page.getByRole("button",{name:"Play sewing replay"}).click();
    await page.waitForFunction(()=>Number(document.querySelector(".dr-stage canvas")?.dataset.frame)>2);
    await page.getByRole("button",{name:"Pause sewing replay"}).click();
    await page.setViewportSize({width:390,height:844});
    await page.getByRole("button",{name:"52 centimetre chest"}).click();
    await page.getByRole("button",{name:"35 degree arm pose"}).click();
    await page.getByRole("button",{name:"Full figure"}).click();
    await page.locator('canvas[data-run="520-35"][data-full="true"]').waitFor();
    assert.ok(await page.locator(".dr-settings a[download]").count()===2);
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
  } finally {await p.close();}
});

test("sundress ease uses the fitted Anny tapes and preserves the fixed cut", async () => {
  const p=await browser.newPage({...opts,reducedMotion:"reduce",viewport:{width:390,height:844}});
  try {
    await p.goto(new URL("/wardrobe/sundress/",site).href);
    await p.locator("canvas[data-ready]").waitFor();
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
    assert.equal(await p.evaluate(()=>document.documentElement.scrollWidth),390);
  } finally {await p.close();}
});

test("regiment, layer and camera selections reach the hussar scene", async () => {
  const p=await browser.newPage({...opts,reducedMotion:"reduce"});
  try {
    await p.goto(new URL("/wardrobe/hussar/",site).href);
    await p.getByRole("button",{name:"Compare",exact:true}).click();
    await p.getByRole("button",{name:"Pelisse",exact:true}).click();
    await p.getByRole("button",{name:"Chest",exact:true}).click();
    await p.locator("canvas[data-ready='Compare-Pelisse']").waitFor();
    const file=p.getByRole("link",{name:"Download DST"});
    const dst=await file.evaluate(async a=>new Uint8Array(await (await fetch(a.href)).arrayBuffer()).slice(0,3).join(","));
    assert.equal(dst,"76,65,58"); // Tajima header begins with "LA:".
  } finally {await p.close();}
});
