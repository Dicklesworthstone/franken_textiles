// Record the same camera, body meshes, panel coordinates and sewn cloth used in the browser.
import {spawn} from "node:child_process";
import {once} from "node:events";
import {mkdir,writeFile} from "node:fs/promises";
import {resolve} from "node:path";
import {chromium} from "playwright-core";
import sharp from "sharp";

const out=resolve(process.argv[2]);await mkdir(out);
const browser=await chromium.launch({executablePath:process.env.CHROMIUM_EXECUTABLE||chromium.executablePath(),
  args:["--use-angle=swiftshader","--enable-unsafe-swiftshader","--disable-dev-shm-usage"]});
try{
  const page=await browser.newPage({viewport:{width:1104,height:620},reducedMotion:"reduce",userAgent:"OpenAI File Downloader, XaiImageApiFetch/1.0"});
  await page.goto(new URL("/studies/skirt/?capture=film",process.env.FTX_STUDIO_URL||"http://127.0.0.1:3000").href);
  const room=page.locator(".sa-room"),shot=page.locator(".sa-canvas canvas");
  await page.locator('canvas[data-run="flare-680"]').waitFor();await page.evaluate(()=>document.fonts.ready);
  const alpha=await page.evaluate(()=>{window.ftxSkirtCamera(.34);return document.querySelector(".sa-canvas canvas").toDataURL("image/png").split(",")[1];});
  const buf=Buffer.from(alpha,"base64"),{width,height}=await sharp(buf).metadata();
  await sharp(buf).extract({left:Math.floor((width-height*.81)/2),top:0,width:Math.floor(height*.81),height}).webp({quality:94}).toFile(resolve(out,"form.webp"));
  await page.addStyleTag({content:`
    .sa-room{position:fixed!important;inset:0!important;z-index:1000}.sa-inside{height:100%;max-width:none;padding:0 50px;grid-template-columns:43% 57%}
    .sa-intro{display:block;padding:0}.sa-intro fieldset,.sa-form-toggle,.sa-deck,.sa-fit,.sa-display figcaption,.sa-measure,nextjs-portal{display:none}
    .sa-intro h1{font-size:94px}.sa-intro .sa-label{font-size:12px}.sa-intro .film-reading{margin-top:24px;color:#fff0ce;font:18px var(--sans);line-height:1.5}
  `});
  await page.locator(".sa-intro").evaluate(el=>{
    const reading=document.createElement("div");reading.className="film-reading";el.append(reading);
    el.style.visibility="hidden";
  });
  await room.screenshot({path:resolve(out,"cover.jpg"),type:"jpeg",quality:94});
  await page.locator(".sa-intro").evaluate(el=>{el.style.visibility="";});
  const encoder=spawn("ffmpeg",["-v","error","-f","image2pipe","-framerate","16","-i","pipe:0","-an","-c:v","libx264",
    "-crf","18","-pix_fmt","yuv420p","-movflags","+faststart",resolve(out,"skirt-atelier.mp4")],{stdio:["pipe","ignore","inherit"]});
  const finished=once(encoder,"close");
  try{for(const cut of ["column","flare"]){
    await page.getByRole("button",{name:cut==="flare"?"Flared":"Column",exact:true,includeHidden:true}).evaluate(b=>b.click());
    await page.locator('canvas[data-run="'+cut+'-680"]').waitFor();
    await page.evaluate(c=>{
      document.querySelector(".sa-intro h1").innerHTML=c==="flare"?"The<br><em>flare.</em>":"The<br><em>column.</em>";
      document.querySelector(".film-reading").textContent=(c==="flare"?"168":"126")+" cm hem · 68 cm form";
    },cut);
    for(let i=0;i<64;i++){
      const pct=Math.min(100,Math.round(i*100/44));
      await page.locator(".sa-scrub input").evaluate((slider,p)=>{
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value").set.call(slider,String(p));
        slider.dispatchEvent(new Event("input",{bubbles:true}));
      },pct);
      await page.waitForFunction(p=>document.querySelector(".sa-canvas canvas").dataset.step===String(p),pct);
      await page.evaluate(a=>window.ftxSkirtCamera(a),.20+.94*i/63);
      const frame=await room.screenshot({type:"jpeg",quality:91});
      if(cut==="flare"&&i===45)await writeFile(resolve(out,"skirt-atelier.jpg"),frame,{flag:"wx"});
      if(!encoder.stdin.write(frame))await once(encoder.stdin,"drain");
    }
  }}finally{encoder.stdin.end();}
  const [status]=await finished;if(status!==0 || !await shot.count())throw Error("Skirt film recording failed");
}finally{await browser.close();}
