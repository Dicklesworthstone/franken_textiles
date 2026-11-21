// Run against the served showcase. A fresh destination protects previously rendered films.
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { mkdir, mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { captureOptions, enableFilmResolution, encodeUhd, filmWidth, filmHeight, fps } from "./film-capture.mjs";

const base = new URL(process.argv[2] || "http://localhost:3000");
const media = process.env.FTX_FILM_SOURCE_DIR || fileURLToPath(new URL("../public/media/", import.meta.url));
const output = path.resolve(process.argv[3] || media);
const scratch = await mkdtemp(path.join(tmpdir(), "ftx-studio-film-"));
await mkdir(output, {recursive:true});
const chapters = [
  ["Draw the silhouette.", "Pin the front pattern and change the chest ease."],
  ["Plan the cut.", "The same checked draft enters the roll planner."],
  ["Follow the drape.", "A separate native bolero run on a synthetic body."],
  ["Trace the needle.", "Needle positions replayed from a Tajima DST file."],
];
const mediaArgs = ["-hide_banner", "-loglevel", "error", "-n", "-filter_threads", "2"];
const encodeArgs = [...encodeUhd, "-r", String(fps)];
function run(args) {
  const result = spawnSync("ffmpeg", [...mediaArgs, ...args], {stdio:"inherit"});
  if (result.status !== 0) throw Error("FFmpeg could not complete the studio film.");
}
function plate(index) {
  const [title, detail] = chapters[index];
  const line = (text, font, size, x, y, color) =>
    `drawtext=font='${font}':text='${text}':fontsize=${size*3}:fontcolor=${color}:x=${x*3}:y=${y*3}`;
  return ["scale=3552:1572:force_original_aspect_ratio=decrease:force_divisible_by=2:out_range=tv",
    "pad=3840:2160:(ow-iw)/2:144+(1572-ih)/2:color=0x202830", "setsar=1",
    line("FRANKENTEXTILES   /   STUDIO TOUR","monospace",14,48,17,"0xe9d8aa"),
    line(`0${index+1}   /   04`,"monospace",14,1110,17,"0xe9d8aa"),
    "drawbox=x=144:y=1770:w=3552:h=3:color=0x627079:t=fill",
    line(title,"Liberation Serif",46,46,610,"0xf8f3e7"),
    line(detail,"sans-serif",20,49,674,"0xd7d9d5")].join(",");
}
const source = path.join(media,"bolero.mp4");
const probe = spawnSync("ffprobe",["-v","error","-select_streams","v:0","-show_entries","stream=width,height,avg_frame_rate","-of","json",source],{encoding:"utf8"});
const stream = probe.status===0 && JSON.parse(probe.stdout).streams[0];
if(!stream || stream.width!==filmWidth || stream.height!==filmHeight || stream.avg_frame_rate!==`${fps}/1`){
  throw Error("Render bolero.mp4 at 3840×2160 and 60 fps before assembling the tour.");
}
const part = index => path.join(scratch,`part-${index}.mp4`);
const stills = [];
async function render(index, collect) {
  const ffmpeg = spawn("ffmpeg", [...mediaArgs, "-f", "image2pipe", "-framerate", String(fps),
    "-i", "pipe:0", "-vf", plate(index), ...encodeArgs, part(index)], {stdio:["pipe","ignore","inherit"]});
  const closed = once(ffmpeg, "close");
  ffmpeg.stdin.on("error", () => {});
  const capture = async (element, repeat) => {
    const image = await element.screenshot({type:"jpeg",quality:92,animations:"disabled"});
    if (!stills[index] || index === 3) stills[index] = image;
    for (let i = 0; i < repeat; i++) {
      await new Promise((resolve,reject) => ffmpeg.stdin.write(image, error => error ? reject(error) : resolve()));
    }
  };
  try { await collect(capture); }
  finally {
    ffmpeg.stdin.end();
    const [code] = await closed;
    if (code !== 0) throw Error(`FFmpeg failed on chapter ${index+1}.`);
  }
}

const browser = await chromium.launch({executablePath:process.env.CHROMIUM_EXECUTABLE || chromium.executablePath(),
  args:["--disable-dev-shm-usage","--use-angle=swiftshader","--enable-unsafe-swiftshader"]});
try {
  const page = await browser.newPage(captureOptions({width:1420,height:1000}, 3.25));
  await enableFilmResolution(page);
  const route = new URL(base); route.searchParams.set("lab","draft"); route.hash = "studio";
  await page.goto(route.href);
  const draft = page.locator(".draft-lab");
  await draft.scrollIntoViewIfNeeded();
  await draft.locator(".dl-ok").waitFor();
  await page.evaluate(() => document.fonts.ready);
  await render(0, async capture => {
    await capture(draft,fps);
    await draft.getByRole("button",{name:"Pin outline",exact:true}).click();
    for (const ease of [105,110,115,120]) {
      await draft.getByRole("slider",{name:"Chest ease"}).fill(String(ease));
      await draft.locator(".dl-ok").waitFor();
      await capture(draft,fps);
    }
  });
  await draft.getByRole("button",{name:/Plan this cut/}).click();
  const cut = page.locator(".cut-lab");
  await render(1, async capture => {
    for (const copies of [1,2,3]) {
      await cut.getByRole("button",{name:new RegExp(`^Cut ${copies} garments?$`)}).click();
      await cut.locator(".cut-board[aria-busy=false]").waitFor();
      await capture(cut,5*fps/3);
    }
  });
  run(["-ss","1.5","-t","5","-i",source,"-vf",
    "crop=iw:trunc(ih*592/720/2)*2:0:0,"+plate(2),...encodeArgs,part(2)]);
  await page.locator("#tab-stitch").click();
  const stitch = page.locator(".stitch-lab");
  const slider = stitch.locator("#stitch-timeline");
  await stitch.locator(".stitch-cloth canvas").first().waitFor();
  const count = Number(await slider.getAttribute("max"));
  if (!(count>0)) throw Error("No needle positions loaded.");
  await render(3, async capture => {
    const frames=5*fps;
    for (let i=0; i<frames; i++) {
      await slider.fill(String(Math.round(count*i/(frames-1))));
      await page.evaluate(() => new Promise(requestAnimationFrame));
      await capture(stitch,1);
    }
  });
} finally { await browser.close(); }

await writeFile(path.join(scratch,"chapters.txt"),chapters.map((_,i)=>`file 'part-${i}.mp4'`).join("\n"));
run(["-f","concat","-safe","1","-i",path.join(scratch,"chapters.txt"),"-c","copy","-movflags","+faststart",path.join(output,"studio-tour.mp4")]);
// The film poster uses the same three UI exposures, plus the existing garment-film poster.
for (const i of [0,1,3]) await writeFile(path.join(scratch,`panel-${i}.jpg`),stills[i]);
const images = [path.join(scratch,"panel-0.jpg"),path.join(scratch,"panel-1.jpg"),path.join(media,"bolero.jpg"),path.join(scratch,"panel-3.jpg")];
const panels = images.map((_,i)=>`[${i}:v]scale=1878:1032:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2:color=0x202830,setsar=1[p${i}]`).join(";");
run([...images.flatMap(file=>["-i",file]),"-filter_complex",panels+";[p0][p1][p2][p3]xstack=inputs=4:layout=0_0|1920_0|0_1080|1920_1080","-frames:v","1","-q:v","2",path.join(output,"studio-tour.jpg")]);
console.log(`Rendered the 20-second studio tour to ${output}.`);
