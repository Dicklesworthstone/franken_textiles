// node scripts/render-sundress.mjs http://localhost:3000 /path/to/new-media
import { chromium } from "playwright-core";
import { mkdir, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import { once } from "node:events";
import path from "node:path";

const [base="http://localhost:3000",output="public/media"]=process.argv.slice(2);
await mkdir(output,{recursive:true});
const browser=await chromium.launch({executablePath:process.env.CHROMIUM_EXECUTABLE||chromium.executablePath(),
  args:["--no-sandbox","--use-angle=swiftshader","--enable-unsafe-swiftshader","--disable-dev-shm-usage"]});
try {
  const page=await browser.newPage({viewport:{width:960,height:540},deviceScaleFactor:1,reducedMotion:"reduce",
    userAgent:"OpenAI File Downloader, XaiImageApiFetch/1.0"});
  await page.goto(new URL("/wardrobe/sundress/?capture=1",base).href);
  await page.waitForFunction(()=>!!window.ftxDressFrame);
  await page.addStyleTag({content:`
    .sd-stage{position:fixed!important;inset:0!important;height:100vh!important;width:100vw!important;z-index:100}
    .sd-scene{width:68%!important;margin-left:32%}.sd-stage-caption,nextjs-portal{display:none!important}
    .sd-film{position:absolute;left:6%;top:21%;width:29%;pointer-events:none;color:#233d34}
    .sd-film small{font:500 11px var(--mono);letter-spacing:.13em;text-transform:uppercase}
    .sd-film h2{font:400 64px/.91 var(--serif);letter-spacing:-.045em;margin:16px 0 30px}
    .sd-film dl{margin:0;border-top:1px solid #748675}.sd-film dl div{display:flex;align-items:center;justify-content:space-between;border-bottom:1px solid #8c9b8b;padding:8px 0}
    .sd-film dt{font:400 14px var(--sans)}.sd-film dd{font:500 14px var(--mono);margin:0}.sd-film p{font:400 12px var(--sans);margin-top:14px}`});
  await page.evaluate(()=>{
    const text=document.createElement("div");text.className="sd-film";
    text.innerHTML='<small>FrankenTextiles / Wardrobe</small><h2>The garden sundress.</h2><dl><div><dt>Stature</dt><dd></dd></div><div><dt>Bust ease</dt><dd></dd></div><div><dt>Waist ease</dt><dd></dd></div></dl><p>One fixed cut · geometric pose study</p>';
    document.querySelector(".sd-stage").appendChild(text);
  });
  await page.evaluate(()=>document.fonts.ready);
  async function expose(t){
    await page.evaluate(t=>{
      const phase=Math.sin(Math.PI*t)**2, reach=Math.sin(Math.PI*t*1.5)**2*Math.sin(Math.PI*t)**2;
      const form={height:168+Math.round(10*phase),bust:90+Math.round(7*phase),waist:72+Math.round(9*phase),left:14+82*reach,right:19+22*phase};
      window.ftxDressFrame(form,.42*Math.cos(2*Math.PI*t)-.12,1.03);
      const values=[form.height,94-form.bust,78-form.waist];document.querySelectorAll(".sd-film dd").forEach((d,i)=>{d.textContent=(i&&values[i]>0?"+":"")+values[i]+" cm";});
    },t);
    return page.screenshot({type:"jpeg",quality:94,animations:"disabled"});
  }
  const poster=await expose(.13);
  await writeFile(path.join(output,"wardrobe-sundress.jpg"),poster,{flag:"wx"});
  const fps=20,frames=fps*7;
  const encoder=spawn("ffmpeg",["-hide_banner","-loglevel","error","-n","-f","image2pipe","-framerate",String(fps),"-i","pipe:0","-an","-c:v","libx264","-crf","19","-preset","medium","-pix_fmt","yuv420p","-movflags","+faststart",path.join(output,"wardrobe-sundress.mp4")],{stdio:["pipe","ignore","inherit"]});
  const ended=once(encoder,"close");encoder.stdin.on("error",()=>{});
  try{for(let i=0;i<frames;i++){const jpeg=await expose(i/frames);await new Promise((resolve,reject)=>encoder.stdin.write(jpeg,error=>error?reject(error):resolve()));}}
  finally{encoder.stdin.end();const [code]=await ended;if(code)throw Error("FFmpeg exited with "+code);}
  console.log(`Rendered ${frames} sundress states with FFmpeg (${fps} fps).`);
} finally {await browser.close();}
