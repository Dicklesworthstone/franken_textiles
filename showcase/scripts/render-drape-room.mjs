// Capture the two independent native endpoints at a fixed chest target. Pass a new output directory.
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { chromium } from "playwright-core";
import sharp from "sharp";
import { captureOptions, enableFilmResolution, filmArgs, filmBrowserOptions, fps, frameCapture } from "./film-capture.mjs";

const destination=resolve(process.argv[2]);
await mkdir(destination);
const browser=await chromium.launch(filmBrowserOptions(process.env.CHROMIUM_EXECUTABLE || chromium.executablePath()));
try {
  const page=await browser.newPage(captureOptions({width:960,height:540}));
  await enableFilmResolution(page);
  page.setDefaultTimeout(120000);
  const origin=process.env.FTX_STUDIO_URL||"http://127.0.0.1:3000";
  await page.goto(new URL("/?capture=pose-room",origin).href);
  await page.locator('canvas[data-run="520-35"]').waitFor();
  await page.evaluate(()=>document.fonts.ready);
  await page.addStyleTag({content:`
    .dr-stage{position:fixed!important;inset:0!important;width:960px!important;height:540px!important;z-index:12}
    .dr-room,.dr-display{overflow:visible!important;isolation:auto!important}
    .dr-mount{left:24%!important;width:76%!important}
    .dr-stage-head,.dr-stage-foot,.dr-flat,.dr-legend,.ft-head,nextjs-portal{display:none!important}
    .film-slate{position:absolute;left:4%;top:23%;width:29%;pointer-events:none;color:#332d28}
    .film-slate small,.film-slate span{font:500 11px/1.5 var(--mono);letter-spacing:.08em;text-transform:uppercase}
    .film-slate h1{font:400 69px/.9 var(--serif);letter-spacing:-.035em;margin:17px 0 25px}.film-slate em{color:#805334}
    .film-slate p{border-top:1px solid #8d816e;margin:0 0 8px;padding:8px 0 0;font:600 17px var(--sans)}
  `});
  await page.evaluate(()=>{
    const slate=document.createElement("aside");slate.className="film-slate";
    slate.innerHTML="<small>FrankenTextiles / Drape room</small><h1>Raising the<br><em>sleeve.</em></h1><p></p><span></span>";
    document.querySelector(".dr-stage").appendChild(slate);
  });
  const stage=page.locator(".dr-stage");
  const encoder=spawn("ffmpeg",filmArgs(resolve(destination,"drape-room.mp4")),
    {stdio:["pipe","ignore","inherit"]});
  const finished=once(encoder,"close");
  const screenshot=await frameCapture(page);
  try {
    const frames=8*fps, poseChange=3*fps, contactChange=Math.round(5.6*fps);
    for(let f=0;f<frames;f++){
      const angle=f<poseChange?35:75, contact=f>=contactChange;
      if(f===poseChange)await page.getByRole("button",{name:"75 degree arm pose"}).evaluate(b=>b.click());
      if(f===contactChange)await page.locator(".dr-modes button").filter({hasText:"Contact"}).evaluate(b=>b.click());
      await page.locator(`canvas[data-run="520-${angle}"][data-mode="${contact?"contact":"cloth"}"]`).waitFor();
      await page.evaluate(({angle,contact,progress})=>{
        const readings=document.querySelectorAll(".dr-readings strong");
        document.querySelector(".film-slate p").textContent=angle+"° arm pose · 52 cm chest";
        document.querySelector(".film-slate span").textContent=contact?"Peak contact force "+readings[2].textContent:"Mean chest gap "+readings[1].textContent;
        window.ftxPoseCamera(.24+.49*Math.sin(progress*Math.PI),.17,.36,.067);
      },{angle,contact,progress:f/(frames-1)});
      const image=await screenshot();
      if(f===Math.round(1.6*fps))await writeFile(resolve(destination,"drape-room.jpg"),image,{flag:"wx"});
      if(!encoder.stdin.write(image))await once(encoder.stdin,"drain");
    }
  } finally {encoder.stdin.end();}
  const [code]=await finished;
  if(code!==0)throw Error("FFmpeg exited with "+code);
  await page.addStyleTag({content:`
    .dr-stage{width:100vw!important;height:100vh!important}
    .dr-mount{left:0!important;width:100%!important}
    .film-slate{display:none}
  `});
  for(const [name,width,height,angle,mode] of [
    ["drape-room-contact",692,542,75,"contact"],
    ["drape-room-mobile",692,542,35,"cloth"],
    ["drape-room",1170,719,35,"cloth"],
  ]){
    await page.setViewportSize({width,height});
    await page.getByRole("button",{name:angle+" degree arm pose"}).evaluate(button=>button.click());
    await page.locator(`canvas[data-run="520-${angle}"]`).waitFor();
    await page.locator(".dr-modes button").filter({hasText:mode==="contact"?/^Contact$/:/^Wool$/}).evaluate(button=>button.click());
    await page.locator(`canvas[data-run="520-${angle}"][data-mode="${mode}"]`).waitFor();
    await page.evaluate(()=>new Promise(done=>requestAnimationFrame(()=>requestAnimationFrame(()=>{
      window.ftxPoseCamera(.32,.17);done();
    }))));
    await sharp(await stage.screenshot()).webp({quality:90}).toFile(resolve(destination,name+".webp"));
  }
} finally {await browser.close();}
