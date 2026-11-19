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

test("visible preview plays, preserves a deliberate pause, and leaves the screen quiet", async () => {
  const p = await browser.newPage({ ...opts, reducedMotion:"no-preference" });
  try {
    await p.goto(site);
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

test("wearer changes keep the sundress cut fixed and show the native signed ease", async () => {
  const p=await browser.newPage({...opts,reducedMotion:"reduce",viewport:{width:390,height:844}});
  try {
    await p.goto(new URL("/wardrobe/sundress/",site).href);
    await p.locator("canvas[data-ready]").waitFor();
    const cases=await p.evaluate(async()=>(await (await fetch("/sundress/fit-comparison.json")).json()).runs);
    for(const item of cases) {
      await p.getByRole("button",{name:item.form,exact:true}).click();
      const shown=await p.locator(".sd-table b").allTextContents();
      assert.deepEqual(shown.map(Number),item.points.map(point=>point.ease_mm/10));
    }
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
