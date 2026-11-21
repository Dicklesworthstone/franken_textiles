// With Next running, capture the same WebGL scene and native readings used in the studio.
import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { captureOptions, enableFilmResolution, filmArgs, filmBrowserOptions, fps, frameCapture } from "./film-capture.mjs";

const base = process.argv[2] || "http://localhost:3000";
const dest = path.resolve(process.argv[3] || fileURLToPath(new URL("../public/media/", import.meta.url)));
const kinds = process.argv[4] ? [process.argv[4]] : ["cylinder","strip"];
const frames = fps * 7;
await mkdir(dest,{recursive:true});
const browser = await chromium.launch(filmBrowserOptions(process.env.CHROMIUM_EXECUTABLE || chromium.executablePath()));
try {
  for(const kind of kinds) {
    if(!["cylinder","strip"].includes(kind)) throw Error("Choose cylinder or strip.");
    const page = await browser.newPage(captureOptions({width:1280,height:720}));
    await enableFilmResolution(page);
    await page.goto(new URL(`/experiments/${kind}/?capture=1`,base).href);
    await page.waitForFunction(()=>typeof window.ftxRigView==="function" && document.querySelector("canvas[data-step]"));
    await page.evaluate(()=>document.fonts.ready);
    await page.addStyleTag({content:".rig-stage{position:fixed!important;inset:0!important;width:100vw!important;height:100vh!important;z-index:90}.rig-plate,.rig-annot,nextjs-portal{display:none!important}"});
    const slider=page.locator(".rig-slider input");
    const last=Number(await slider.getAttribute("max"));
    async function pose(i,angle) {
      // The fixed capture surface covers the slider, so dispatch the same native input event directly.
      await slider.evaluate((el,value)=>{
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value").set.call(el,String(value));
        el.dispatchEvent(new Event("input",{bubbles:true}));
      },i);
      await page.waitForFunction(k=>document.querySelector("canvas")?.dataset.step===String(k),i);
      await page.evaluate(yaw=>window.ftxRigView(yaw),angle);
    }
    await pose(kind==="cylinder"?Math.round(last*2.5/3.5):last,kind==="cylinder"?-.38:.17);
    await writeFile(path.join(dest,`rig-${kind}.jpg`),await page.screenshot({type:"jpeg",quality:94}),{flag:"wx"});
    await page.evaluate(()=>{
      const plate=document.createElement("div");plate.id="film-reading";
      plate.style.cssText="position:fixed;z-index:95;inset:0 0 auto;padding:18px 30px;display:flex;justify-content:space-between;align-items:center;background:#203333ed;color:#fff4db;font:500 14px monospace";
      const name=document.createElement("strong");name.style.cssText="font:400 30px Georgia,serif";name.textContent=document.querySelector('h1').textContent;
      const value=document.createElement("span");plate.append(name,value);document.body.append(plate);
    });
    const encoder=spawn("ffmpeg",filmArgs(path.join(dest,`rig-${kind}.mp4`)),{stdio:["pipe","ignore","inherit"]});
    const done=once(encoder,"close");encoder.stdin.on("error",()=>{});
    const screenshot=await frameCapture(page);
    try {
      for(let k=0;k<frames;k++){
        const t=k/(frames-1),cut=Math.min(2,Math.floor(t*3));
        if(kind==="strip"){
          const target=["Warp","Weft","Bias"][cut];
          await page.getByRole("button",{name:target,exact:true}).evaluate(el=>{if(el.getAttribute("aria-pressed")!=="true")el.click();});
        }
        const i=Math.round((kind==="cylinder"?t:Math.min(1,(t*3-cut)*1.4))*last);
        await pose(i, (kind==="cylinder"?.35:.05)+Math.sin(t*2*Math.PI)*(kind==="cylinder"?.89:.30));
        await page.evaluate(kind=>{
          const time=document.querySelector(".rig-slider output").textContent;
          const cut=document.querySelector(".rig-scene canvas").dataset.specimen;
          const value=kind==="cylinder"?document.querySelector(".rig-values dd").textContent:[...document.querySelectorAll(".rig-values tbody tr")].find(row=>row.querySelector("th").textContent===cut).querySelector("td").textContent;
          document.querySelector("#film-reading span").textContent=kind==="cylinder"?time+"   /   UPPER GRIP  "+value:time+" EXTENSION   /   "+cut.toUpperCase()+"  "+value;
        },kind);
        const frame=await screenshot(93);
        await new Promise((resolve,reject)=>encoder.stdin.write(frame,error=>error?reject(error):resolve()));
      }
    } finally {
      encoder.stdin.end();const [code]=await done;if(code!==0) throw Error("FFmpeg failed for "+kind);
      await page.close();
    }
    console.log(`Rendered ${kind}, ${frames} frames.`);
  }
} finally {await browser.close();}
