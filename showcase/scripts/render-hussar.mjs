// Capture the two wardrobe models through their real controls, then encode a silent H.264 loop.
import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { captureOptions, enableFilmResolution, filmArgs, fps, frameCapture } from "./film-capture.mjs";

const base=new URL(process.argv[2] || "http://localhost:3000");
const dest=path.resolve(process.argv[3] || fileURLToPath(new URL("../public/media/",import.meta.url)));
const frames=fps*10, poster=path.join(dest,"wardrobe-hussar.jpg"), output=path.join(dest,"wardrobe-hussar.mp4");
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
  const page=await browser.newPage(captureOptions({width:960,height:540}));
  await enableFilmResolution(page);
  page.setDefaultTimeout(120000);
  await page.routeWebSocket(/\/_next\/webpack-hmr/,()=>{});
  await page.goto(new URL("/wardrobe/hussar/?costumeCapture=1",base).href);
  await page.waitForFunction(()=>typeof window.ftxCostumeFrame==="function" && document.querySelector(".costume-stage canvas[data-ready]"),undefined,{timeout:120000});
  await page.evaluate(()=>document.fonts.ready);
  async function select(reg,layer){
    await page.evaluate(([reg,layer])=>{
      for(const val of [reg,layer])document.querySelector('.costume-choices input[value="'+val+'"]').click();
    },[reg,layer]);
    await page.waitForFunction(wanted=>document.querySelector(".costume-stage canvas")?.dataset.ready===wanted,reg+"-"+layer);
  }
  await select("Compare","Dressed");
  await page.addStyleTag({content:".costume-stage{position:fixed!important;inset:0!important;width:100vw!important;height:100vh!important;z-index:99}.hu-tag,.hu-plate,nextjs-portal{display:none!important}"});
  async function pose(yaw,y,span){
    await page.evaluate(async ([angle,y,span])=>{
      await new Promise(r=>requestAnimationFrame(r));
      const canvas=document.querySelector(".costume-stage canvas[data-ready]");
      if(!canvas||!window.ftxCostumeFrame)throw Error("The wardrobe camera is unavailable");
      window.ftxCostumeFrame(angle,y,span);
      const gl=canvas.getContext("webgl2"),width=gl.drawingBufferWidth, pixels=new Uint8Array(width*4);
      gl.readPixels(0,Math.floor(gl.drawingBufferHeight*.5),width,1,gl.RGBA,gl.UNSIGNED_BYTE,pixels);
      if(!pixels.some((value,index)=>index%4===3&&value>64))throw Error("Empty wardrobe render at "+angle);
    },[yaw,y,span]);
  }
  await pose(.045,137,77);
  await writeFile(poster,await page.screenshot({type:"jpeg",quality:94}),{flag:"wx"});
  await select("Blue","Dressed");
  await page.setViewportSize({width:1000,height:850});
  await pose(.04,150,61);
  await writeFile(path.join(dest,"hussar.jpg"),await page.screenshot({type:"jpeg",quality:95}),{flag:"wx"});
  if(process.env.HUSSAR_STILLS_ONLY==="1") {
    console.log("Rendered the film poster and clean portrait in "+dest);
  } else {
  await page.setViewportSize({width:960,height:540});
  const screenshot = await frameCapture(page);
  await page.evaluate(()=>{
    const plate=document.createElement("aside");plate.id="hussar-film-plate";
    plate.style.cssText="position:fixed;inset:auto 3.6% 3.8%;z-index:100;pointer-events:none;color:#f5efd9;display:flex;justify-content:space-between;align-items:baseline;border-top:1px solid #d8c89d80;padding:8px 0 0;text-shadow:0 1px 4px #23332b;font:500 12px ui-monospace,monospace;letter-spacing:1.4px";
    const title=document.createElement("b");title.style.cssText="font:normal 28px Georgia,serif;letter-spacing:-.4px";title.textContent="The hussar’s cabinet";
    plate.append(title,document.createElement("span"));document.body.append(plate);
  });
  const encoder=spawn("ffmpeg",filmArgs(output),{stdio:["pipe","ignore","inherit"]});
  const done=once(encoder,"close");encoder.stdin.on("error",()=>{});
  const shots=[
    {reg:"Blue",layer:"Dressed",label:"PUTBUS · 1761   /   COSTUME STUDY",start:[-.14,149,64],end:[.13,143,71]},
    {reg:"Compare",layer:"Dressed",label:"THE TWO REGIMENTS   /   COSTUME STUDY",start:[.04,103,113],end:[.28,105,111]},
    {reg:"Blue",layer:"Dolman",label:"DOLMAN FRONTS   /   THE PRINTABLE CUT",start:[-.25,126,30],end:[.36,126,27]},
    {reg:"Compare",layer:"Pelisse",label:"PELISSE FRONTS   /   BRAID PLACEMENT",start:[-.14,125,38],end:[.14,124,32]},
    {reg:"Yellow",layer:"Dressed",label:"WRANGEL · 1765   /   COSTUME STUDY",start:[.89,144,68],end:[.70,153,57]},
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
        const bytes=await screenshot(94);
        await new Promise((resolve,reject)=>encoder.stdin.write(bytes,error=>error?reject(error):resolve()));
      }
    }
  } finally {encoder.stdin.end();const [code]=await done;if(code!==0)throw Error("FFmpeg failed: "+code);}
  console.log(`Rendered ${endFrame-startFrame} frames to ${output}`);
  }
}finally{await browser.close();}
