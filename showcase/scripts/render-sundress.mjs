// node scripts/render-sundress.mjs http://localhost:3000 /path/to/new-media [/path/to/new-share-art]
import { chromium } from "playwright-core";
import { mkdir, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import { once } from "node:events";
import path from "node:path";
import { captureOptions, enableFilmResolution, filmArgs, fps, frameCapture } from "./film-capture.mjs";

const [base="http://localhost:3000",output="public/media",art]=process.argv.slice(2);
await mkdir(output,{recursive:true});
const browser=await chromium.launch({executablePath:process.env.CHROMIUM_EXECUTABLE||chromium.executablePath(),
  args:["--no-sandbox","--use-angle=swiftshader","--enable-unsafe-swiftshader","--disable-dev-shm-usage"]});
try {
  const page=await browser.newPage(captureOptions({width:1120,height:630}));
  await enableFilmResolution(page);
  page.setDefaultTimeout(120000);
  await page.goto(new URL("/wardrobe/sundress/?capture=1",base).href);
  await page.evaluate(()=>document.querySelector(".sd-stage").scrollIntoView());
  await page.waitForFunction(()=>typeof window.ftxDressFrame==="function" && document.querySelector(".sd-scene canvas[data-ready]"));
  if(art){
    await mkdir(art,{recursive:true});await page.setViewportSize({width:1200,height:1200});
    const style=await page.addStyleTag({content:`
      .sd-stage{position:fixed!important;inset:0!important;min-height:0!important;height:100vh!important;width:100vw!important;z-index:100}
      .sd-scene{width:100%!important}.sd-stage-caption,nextjs-portal{display:none!important}
    `});
    await page.evaluate(()=>window.ftxDressFrame({height:168,bust:90,waist:72,weight:52,left:14,right:22},-.32,2.02,127,true));
    await writeFile(path.join(art,"sundress.jpg"),await page.screenshot({type:"jpeg",quality:95}),{flag:"wx"});
    await style.evaluate(element=>element.remove());await page.setViewportSize({width:1120,height:630});
  }
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
    text.innerHTML='<small>FrankenTextiles / Fitting room</small><h2>The garden sundress.</h2><dl><div><dt>Model stature</dt><dd></dd></div><div><dt>Model mass</dt><dd></dd></div><div><dt>Bust ease</dt><dd></dd></div><div><dt>Waist ease</dt><dd></dd></div></dl><p>Fitted Anny body · geometric garment</p>';
    document.querySelector(".sd-stage").appendChild(text);
  });
  await page.evaluate(()=>document.fonts.ready);
  const screenshot=await frameCapture(page);
  async function expose(t){
    await page.evaluate(t=>{
      const phase=Math.sin(Math.PI*t)**2, reach=Math.sin(Math.PI*t*1.5)**2*Math.sin(Math.PI*t)**2;
      const form={height:168+7*phase,bust:90+8*phase,waist:72+12*phase,weight:52+8*phase,left:14+82*reach,right:19+24*phase};
      const v=window.ftxDressFrame(form,.42*Math.cos(2*Math.PI*t)-.12,1.03+.33*phase,89+21*phase);
      const values=[v.height,v.weight,94-v.bust,78-v.waist];document.querySelectorAll(".sd-film dd").forEach((d,i)=>{d.textContent=(i>=2&&values[i]>0?"+":"")+values[i].toFixed(1)+(i===1?" kg":" cm");});
    },t);
    return screenshot();
  }
  const poster=await expose(.13);
  await writeFile(path.join(output,"wardrobe-sundress.jpg"),poster,{flag:"wx"});
  const frames=fps*7;
  const encoder=spawn("ffmpeg",filmArgs(path.join(output,"wardrobe-sundress.mp4")),{stdio:["pipe","ignore","inherit"]});
  const ended=once(encoder,"close");encoder.stdin.on("error",()=>{});
  try{for(let i=0;i<frames;i++){const jpeg=await expose(i/frames);await new Promise((resolve,reject)=>encoder.stdin.write(jpeg,error=>error?reject(error):resolve()));}}
  finally{encoder.stdin.end();const [code]=await ended;if(code)throw Error("FFmpeg exited with "+code);}
  console.log(`Rendered ${frames} sundress states with FFmpeg (${fps} fps).`);
} finally {await browser.close();}
