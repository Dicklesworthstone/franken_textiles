import assert from "node:assert/strict";
import test from "node:test";
import {chromium} from "playwright-core";

test("posture, elastic and cutaway views follow their own native results",async()=>{
  const browser=await chromium.launch({executablePath:process.env.CHROMIUM_EXECUTABLE||chromium.executablePath(),
    args:["--use-angle=swiftshader","--enable-unsafe-swiftshader","--disable-dev-shm-usage"]});
  try {
    const page=await browser.newPage({viewport:{width:390,height:844},reducedMotion:"reduce",userAgent:"OpenAI File Downloader, XaiImageApiFetch/1.0"});
    const origin=process.env.FTX_STUDIO_URL||"http://127.0.0.1:3000";
    await page.goto(new URL("/studies/trousers/",origin).href);
    const {cases}=await (await page.request.get(new URL("/exhibits/trouser-study/study.json",origin).href)).json();
    const button=name=>page.getByRole("button",{name,exact:true});
    for(const run of cases){
      const elastic=run.id.startsWith("elastic-");
      await button(elastic?"Elastic":"Posture").click();
      const choice=button(elastic?run.stiffness+" N":run.label);
      if(run.refusal){assert.equal(await choice.count(),0);continue;}
      await choice.click();
      await page.locator(`canvas[data-run="${run.id}"][data-cloth="true"]`).waitFor();
      assert.equal(await page.locator(".ts-values dd").last().textContent(),run.waistbandShift.toFixed(1)+" mm");
    }
    await button("Cross-section").click();
    for(const [label,key,span] of [["Standing","standing",318.899],["Shallow seat","sit-12",314.658],["Shallow squat","squat-12",304.131]]){
      await button(label).click();await page.locator(`canvas[data-run="${key}"][data-slice="280"]`).waitFor();
      // Widths from unrounded native garment.obj triangles. Display coordinates are quantized to 0.1 mm.
      const shown=Number((await page.locator(".ts-note").innerText()).match(/spans ([\d.]+)/)[1]);
      assert.ok(Math.abs(span-shown)<.25,`Native ${span} vs displayed ${shown}`);
    }
    await page.getByRole("checkbox",{name:"Cut away above the plane"}).check();
    await page.getByRole("slider").press("Home");await page.locator('canvas[data-slice="100"]').waitFor();
    assert.equal(await page.locator('[data-contour="cloth"]').count(),0);
    assert.equal(await page.locator('[data-contour="body"]').count(),1);
    assert.equal(await page.locator(".ts-films video").count(),3);
    assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
    const pdf=await page.request.get(await page.locator(".ts-download").getAttribute("href").then(p=>new URL(p,origin).href));
    assert.equal((await pdf.body()).subarray(0,5).toString(),"%PDF-");
  }finally{await browser.close();}
});
