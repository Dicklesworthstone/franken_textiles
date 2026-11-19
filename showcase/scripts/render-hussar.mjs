// Capture the two wardrobe models through their real controls, then encode a silent H.264 loop.
import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const base=new URL(process.argv[2] || "http://localhost:3000");
const dest=path.resolve(process.argv[3] || fileURLToPath(new URL("../public/media/",import.meta.url)));
const fps=24, frames=fps*9, poster=path.join(dest,"wardrobe-hussar.jpg"), output=path.join(dest,"wardrobe-hussar.mp4");
const startFrame = Number(process.env.HUSSAR_START_FRAME ?? 0);
const endFrame = Number(process.env.HUSSAR_END_FRAME ?? frames);
if (!Number.isInteger(startFrame) || startFrame < 0 || startFrame >= frames) {
  throw new Error("HUSSAR_START_FRAME must be an integer from 0 through " + (frames - 1));
}
if (!Number.isInteger(endFrame) || endFrame <= startFrame || endFrame > frames) {
  throw new Error("HUSSAR_END_FRAME must follow HUSSAR_START_FRAME and be at most " + frames);
}
const ease=t=>t*t*(3-2*t);
await mkdir(dest,{recursive:true});
const browser=await chromium.launch({executablePath:process.env.CHROMIUM_EXECUTABLE || chromium.executablePath(),
  args:["--use-angle=swiftshader","--enable-unsafe-swiftshader","--disable-dev-shm-usage"]});
try{
  const page=await browser.newPage({viewport:{width:1280,height:720},deviceScaleFactor:1,reducedMotion:"reduce",
    userAgent:"OpenAI File Downloader, XaiImageApiFetch/1.0"});
  await page.goto(new URL("/wardrobe/hussar/?capture=1",base).href);
  await page.waitForFunction(()=>typeof window.ftxHussarFrame==="function" && document.querySelector("canvas[data-ready]"),undefined,{timeout:120000});
  await page.evaluate(()=>document.fonts.ready);
  async function select(reg,layer){
    // The instrument fills the viewport during capture, so drive the original buttons programmatically.
    await page.evaluate(([reg,layer])=>{
      for(const text of [reg,layer]){
        const button=[...document.querySelectorAll(".hu-choice button")].find(b=>b.textContent===text);
        if(!button)throw Error("Missing wardrobe control: "+text);button.click();
      }
    },[reg,layer]);
    await page.waitForFunction(wanted=>document.querySelector("canvas")?.dataset.ready===wanted,reg+"-"+layer);
  }
  await select("Compare","Dressed");
  await page.addStyleTag({content:".hu-stage{position:fixed!important;inset:0!important;width:100vw!important;height:100vh!important;z-index:99}.hu-tag,.hu-plate,nextjs-portal{display:none!important}"});
  async function pose(yaw,y,span){
    await page.evaluate(async ([angle,y,span])=>{window.ftxHussarFrame(angle,y,span);await new Promise(r=>requestAnimationFrame(r));},[yaw,y,span]);
  }
  await pose(-.17,136,75);
  await writeFile(poster,await page.screenshot({type:"jpeg",quality:94}),{flag:"wx"});
  await select("Blue","Dressed");
  await page.setViewportSize({width:1000,height:850});
  await pose(.24,154,56);
  await writeFile(path.join(dest,"hussar.jpg"),await page.screenshot({type:"jpeg",quality:95}),{flag:"wx"});
  if(process.env.HUSSAR_STILLS_ONLY==="1") {
    console.log("Rendered the film poster and clean portrait in "+dest);
  } else {
  await page.setViewportSize({width:1280,height:720});
  await page.evaluate(()=>{
    const plate=document.createElement("aside");plate.id="hussar-film-plate";
    plate.style.cssText="position:fixed;inset:auto 3.6% 3.8%;z-index:100;pointer-events:none;color:#f5efd9;display:flex;justify-content:space-between;align-items:baseline;border-top:1px solid #d8c89d80;padding:8px 0 0;text-shadow:0 1px 4px #23332b;font:500 12px ui-monospace,monospace;letter-spacing:1.4px";
    const title=document.createElement("b");title.style.cssText="font:normal 28px Georgia,serif;letter-spacing:-.4px";title.textContent="The hussar’s cabinet";
    plate.append(title,document.createElement("span"));document.body.append(plate);
  });
  const encoder=spawn("ffmpeg",["-v","error","-n","-f","image2pipe","-framerate",String(fps),"-i","pipe:0","-an","-c:v","libx264","-preset","fast","-crf","20","-pix_fmt","yuv420p","-movflags","+faststart",output],{stdio:["pipe","ignore","inherit"]});
  const done=once(encoder,"close");encoder.stdin.on("error",()=>{});
  const shots=[
    {reg:"Blue",layer:"Dressed",label:"PUTBUS   /   BLUE TROOPER 1761",start:[.32,154,56],end:[-.21,149,61]},
    {reg:"Compare",layer:"Dressed",label:"BLUE AND YELLOW HUSSARS   /   1761 UNIFORM",start:[-.18,127,84],end:[.12,101,112]},
    {reg:"Compare",layer:"Pelisse",label:"TWO PELISSES   /   FUR-TRIMMED FRONTS",start:[-.37,118,49],end:[.34,117,54]},
    {reg:"Yellow",layer:"Dressed",label:"WRANGEL   /   YELLOW TROOPER 1765",start:[.42,143,67],end:[.1,154,56]},
  ];
  try{
    for(const [shotIndex, shot] of shots.entries()){
      const shotLength = frames / shots.length;
      const first = Math.max(0, startFrame - shotIndex * shotLength);
      const last = Math.min(shotLength, endFrame - shotIndex * shotLength);
      if (first >= last) continue;
      await select(shot.reg,shot.layer);
      await page.evaluate(text=>document.querySelector("#hussar-film-plate span").textContent=text,shot.label);
      for(let i=first;i<last;i++){
        const t=ease(i/(shotLength-1));const [a,y,span]=shot.start.map((v,k)=>v+(shot.end[k]-v)*t);
        await pose(a,y,span);
        const bytes=await page.screenshot({type:"jpeg",quality:92});
        await new Promise((resolve,reject)=>encoder.stdin.write(bytes,error=>error?reject(error):resolve()));
      }
    }
  } finally {encoder.stdin.end();const [code]=await done;if(code!==0)throw Error("FFmpeg failed: "+code);}
  console.log(`Rendered ${endFrame-startFrame} frames to ${output}`);
  }
}finally{await browser.close();}
