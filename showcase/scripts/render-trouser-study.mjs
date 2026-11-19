// Record the website's own meshes, controls and cross-sections into three short H.264 films.
import {spawn} from "node:child_process";
import {once} from "node:events";
import {mkdir,writeFile} from "node:fs/promises";
import {resolve} from "node:path";
import {chromium} from "playwright-core";

const out=resolve(process.argv[2]);
await mkdir(out);
const origin=process.env.FTX_STUDIO_URL||"http://127.0.0.1:3000";
const browser=await chromium.launch({executablePath:process.env.CHROMIUM_EXECUTABLE||chromium.executablePath(),
  args:["--use-angle=swiftshader","--enable-unsafe-swiftshader","--disable-dev-shm-usage"]});
const productions=[
  {id:"trouser-posture",tab:"Posture",title:"Bending the knee",cuts:[["Standing","standing"],["Shallow squat","squat-12"],["Full squat","squat"]]},
  {id:"trouser-elastic",tab:"Elastic",title:"Holding the waist",cuts:[["5 N","elastic-5"],["15 N","standing"],["60 N","elastic-60"]]},
  {id:"trouser-section",tab:"Cross-section",title:"Inside the folds",cuts:[["Standing","standing"]]},
];
try {
  const page=await browser.newPage({viewport:{width:960,height:540},reducedMotion:"reduce",userAgent:"OpenAI File Downloader, XaiImageApiFetch/1.0"});
  await page.goto(new URL("/studies/trousers/?capture=film",origin).href);
  await page.locator('canvas[data-run="standing"]').waitFor();
  await page.evaluate(()=>document.fonts.ready);
  const clean=await page.addStyleTag({content:".ts-stage{width:680px!important;height:620px!important}.ts-stage .ts-mark,.ts-cameras{visibility:hidden}"});
  await page.locator(".ts-stage").screenshot({path:resolve(out,"trouser-form.jpg"),type:"jpeg",quality:94});
  await clean.evaluate(node=>{node.textContent="";});
  await page.addStyleTag({content:`
    .ts-stage{position:fixed!important;inset:0!important;width:960px!important;height:540px!important;z-index:100;border-radius:0}
    .ts-canvas{left:34%;width:66%}.ts-mark,.ts-cameras,nextjs-portal{display:none!important}
    .ts-film-type{position:absolute;inset:0 auto 0 0;width:31%;padding:75px 22px 25px 32px;background:#f3eddb;color:#2c4437}
    .ts-film-type small,.ts-film-type label{font:500 11px var(--mono);letter-spacing:.06em;text-transform:uppercase}
    .ts-film-type h2{font:400 68px/.91 var(--serif);letter-spacing:-.045em;margin:26px 0}.ts-film-type h2 em{font-weight:400;color:#986440}
    .ts-film-type p{border-top:1px solid #9e9c87;padding-top:12px;font:600 16px var(--sans);margin:0 0 8px}
    .ts-film-type .ts-diagram{margin-top:18px}.ts-film-type .ts-diagram svg{height:120px}.ts-film-type .ts-figure-label{display:none}
  `});
  await page.evaluate(()=>{
    const slate=document.createElement("div");slate.className="ts-film-type";
    slate.innerHTML="<small>FrankenTextiles / Studies</small><h2></h2><p></p><label></label><div class='ts-inset'></div>";
    document.querySelector(".ts-stage").append(slate);
  });
  const stage=page.locator(".ts-stage");
  for(const {id,tab,title,cuts} of productions){
    await page.getByRole("button",{name:tab,exact:true}).evaluate(b=>b.click());
    const isSlice=tab==="Cross-section";
    const encoder=spawn("ffmpeg",["-v","error","-f","image2pipe","-framerate","16","-i","pipe:0","-an","-c:v","libx264",
      "-crf","19","-pix_fmt","yuv420p","-movflags","+faststart",resolve(out,id+".mp4")],{stdio:["pipe","ignore","inherit"]});
    const finished=once(encoder,"close");
    try {
      for(let i=0;i<96;i++){
        const index=isSlice?0:Math.floor(i/32),[button,key]=cuts[index];
        if(i%32===0){
          await page.getByRole("button",{name:button,exact:true}).evaluate(b=>b.click());
          await page.locator('canvas[data-run="'+key+'"]').waitFor();
        }
        const height=Math.round(150+312*(.5-.5*Math.cos(i/95*Math.PI)));
        if(isSlice){
          await page.locator('input[type=range]').evaluate((slider,h)=>{
            Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value").set.call(slider,String(h));
            slider.dispatchEvent(new Event("input",{bubbles:true}));
          },height);
          await page.locator('canvas[data-slice="'+height+'"]').waitFor();
        }
        await page.evaluate(({tab,title,key,button,i,height})=>{
          const desk=document.querySelector(".ts-film-type"),words=title.split(" ");
          desk.querySelector("h2").innerHTML=words.slice(0,-1).join(" ")+"<br><em>"+words.at(-1)+".</em>";
          desk.querySelector("p").textContent=tab==="Cross-section"?height+" mm above sole":tab==="Elastic"?button+" elastic stiffness":button+" posture";
          const metric=document.querySelectorAll(".ts-values dd");
          desk.querySelector("label").textContent=tab==="Cross-section"?"Plane through the native mesh":tab==="Elastic"?"Waistband shift "+metric[1].textContent:key==="squat"?"Body only · cloth solve refused":"Mean seat gap "+metric[0].textContent;
          const inset=desk.querySelector(".ts-inset");inset.replaceChildren();
          if(tab==="Cross-section"){
            const chart=document.querySelector(".ts-panel .ts-diagram").cloneNode(true);
            chart.querySelector("pattern").id="film-grid";chart.querySelector("rect").setAttribute("fill","url(#film-grid)");inset.append(chart);
          }
          window.ftxTrouserCamera(tab==="Posture"?.46+1.04*i/95:.42+.56*Math.sin(i/95*Math.PI));
        },{tab,title,key,button,i,height});
        const frame=await stage.screenshot({type:"jpeg",quality:92});
        if(i===28)await writeFile(resolve(out,id+".jpg"),frame,{flag:"wx"});
        if(!encoder.stdin.write(frame))await once(encoder.stdin,"drain");
      }
    } finally {encoder.stdin.end();}
    const [status]=await finished;if(status!==0)throw Error("FFmpeg failed for "+id);
  }
} finally {await browser.close();}
