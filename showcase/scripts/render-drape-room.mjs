// Capture the two independent native endpoints at a fixed chest target. Pass a new output directory.
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { chromium } from "playwright-core";

const destination=resolve(process.argv[2]);
await mkdir(destination);
const browser=await chromium.launch({executablePath:process.env.CHROMIUM_EXECUTABLE||chromium.executablePath(),
  args:["--use-angle=swiftshader","--enable-unsafe-swiftshader","--disable-dev-shm-usage"]});
try {
  const page=await browser.newPage({viewport:{width:960,height:540},reducedMotion:"reduce",
    userAgent:"OpenAI File Downloader, XaiImageApiFetch/1.0"});
  const origin=process.env.FTX_STUDIO_URL||"http://127.0.0.1:3000";
  await page.goto(new URL("/?capture=pose-room",origin).href);
  await page.locator('canvas[data-run="520-35"]').waitFor();
  await page.evaluate(()=>document.fonts.ready);
  await page.addStyleTag({content:`
    .dr-stage{position:fixed!important;inset:0!important;width:960px!important;height:540px!important;z-index:12}
    .dr-room,.dr-display{overflow:visible!important;isolation:auto!important}
    .dr-mount{left:24%!important;width:76%!important}
    .dr-stage-head,.dr-stage-foot,.dr-flat,.dr-reverse,.dr-legend,.ft-head,nextjs-portal{display:none!important}
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
  const encoder=spawn("ffmpeg",["-v","error","-f","image2pipe","-framerate","20","-i","pipe:0","-an","-c:v","libx264","-crf","19","-pix_fmt","yuv420p","-movflags","+faststart",resolve(destination,"drape-room.mp4")],
    {stdio:["pipe","ignore","inherit"]});
  const finished=once(encoder,"close");
  try {
    for(let f=0;f<160;f++){
      const angle=f<60?35:75, contact=f>=112;
      if(f===60)await page.getByRole("button",{name:"75 degree arm pose"}).evaluate(b=>b.click());
      if(f===112)await page.locator(".dr-modes button").filter({hasText:"Contact"}).evaluate(b=>b.click());
      await page.locator(`canvas[data-run="520-${angle}"][data-mode="${contact?"contact":"cloth"}"]`).waitFor();
      await page.evaluate(({angle,contact,f})=>{
        const readings=document.querySelectorAll(".dr-readings strong");
        document.querySelector(".film-slate p").textContent=angle+"° arm pose · 52 cm chest";
        document.querySelector(".film-slate span").textContent=contact?"Peak contact force "+readings[2].textContent:"Mean chest gap "+readings[1].textContent;
        window.ftxPoseCamera(.24+.49*Math.sin(f/159*Math.PI),.17,.36,.067);
      },{angle,contact,f});
      const image=await stage.screenshot({type:"jpeg",quality:92});
      if(f===32)await writeFile(resolve(destination,"drape-room.jpg"),image,{flag:"wx"});
      if(!encoder.stdin.write(image))await once(encoder.stdin,"drain");
    }
  } finally {encoder.stdin.end();}
  const [code]=await finished;
  if(code!==0)throw Error("FFmpeg exited with "+code);
} finally {await browser.close();}
