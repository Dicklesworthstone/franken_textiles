// Run against the served showcase. A fresh destination protects previously rendered films.
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { mkdir, mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const base = new URL(process.argv[2] || "http://localhost:3000");
const media = fileURLToPath(new URL("../public/media/", import.meta.url));
const output = path.resolve(process.argv[3] || media);
const scratch = await mkdtemp(path.join(tmpdir(), "ftx-studio-film-"));
await mkdir(output, {recursive:true});
const fps = 24;
const chapters = [
  ["Draw the silhouette.", "Pin the front pattern and change the chest ease."],
  ["Plan the cut.", "The same checked draft enters the roll planner."],
  ["Follow the drape.", "A separate native bolero run on a synthetic body."],
  ["Trace the needle.", "Needle positions replayed from a Tajima DST file."],
];
const mediaArgs = ["-hide_banner", "-loglevel", "error", "-n"];
const encodeArgs = ["-an", "-c:v", "libx264", "-crf", "20", "-preset", "medium", "-pix_fmt", "yuv420p", "-r", String(fps)];
function run(args) {
  const result = spawnSync("ffmpeg", [...mediaArgs, ...args], {stdio:"inherit"});
  if (result.status !== 0) throw Error("FFmpeg could not complete the studio film.");
}
function plate(index) {
  const [title, detail] = chapters[index];
  const line = (text, font, size, x, y, color) =>
    `drawtext=font='${font}':text='${text}':fontsize=${size}:fontcolor=${color}:x=${x}:y=${y}`;
  return ["scale=1184:524:force_original_aspect_ratio=decrease:force_divisible_by=2",
    "pad=1280:720:(ow-iw)/2:48+(524-ih)/2:color=0x202830", "setsar=1",
    line("FRANKENTEXTILES   /   STUDIO TOUR","monospace",14,48,17,"0xe9d8aa"),
    line(`0${index+1}   /   04`,"monospace",14,1110,17,"0xe9d8aa"),
    "drawbox=x=48:y=590:w=1184:h=1:color=0x627079:t=fill",
    line(title,"Liberation Serif",46,46,610,"0xf8f3e7"),
    line(detail,"sans-serif",20,49,674,"0xd7d9d5")].join(",");
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
  const page = await browser.newPage({viewport:{width:1420,height:1000},deviceScaleFactor:1,
    reducedMotion:"reduce",userAgent:"OpenAI File Downloader, XaiImageApiFetch/1.0"});
  const route = new URL(base); route.searchParams.set("lab","draft"); route.hash = "studio";
  await page.goto(route.href);
  const draft = page.locator(".draft-lab");
  await draft.scrollIntoViewIfNeeded();
  await draft.locator(".dl-ok").waitFor();
  await page.evaluate(() => document.fonts.ready);
  await render(0, async capture => {
    await capture(draft,24);
    await draft.getByRole("button",{name:"Pin outline",exact:true}).click();
    for (const ease of [105,110,115,120]) {
      await draft.getByRole("slider",{name:"Chest ease"}).fill(String(ease));
      await draft.locator(".dl-ok").waitFor();
      await capture(draft,24);
    }
  });
  await draft.getByRole("button",{name:/Plan this cut/}).click();
  const cut = page.locator(".cut-lab");
  await render(1, async capture => {
    for (const copies of [1,2,3]) {
      await cut.getByRole("button",{name:new RegExp(`^Cut ${copies} garments?$`)}).click();
      await cut.locator(".cut-board[aria-busy=false]").waitFor();
      await capture(cut,40);
    }
  });
  run(["-ss","1.5","-t","5","-i",path.join(media,"bolero.mp4"),"-vf",
    "crop=1280:592:0:0,"+plate(2),...encodeArgs,part(2)]);
  await page.locator("#tab-stitch").click();
  const stitch = page.locator(".stitch-lab");
  const slider = stitch.locator("#stitch-timeline");
  await stitch.locator(".stitch-cloth canvas").first().waitFor();
  const count = Number(await slider.getAttribute("max"));
  if (!(count>0)) throw Error("No needle positions loaded.");
  await render(3, async capture => {
    for (let i=0; i<60; i++) {
      await slider.fill(String(Math.round(count*i/59)));
      await page.evaluate(() => new Promise(requestAnimationFrame));
      await capture(stitch,2);
    }
  });
} finally { await browser.close(); }

await writeFile(path.join(scratch,"chapters.txt"),chapters.map((_,i)=>`file 'part-${i}.mp4'`).join("\n"));
run(["-f","concat","-safe","1","-i",path.join(scratch,"chapters.txt"),"-c","copy","-movflags","+faststart",path.join(output,"studio-tour.mp4")]);
// The film poster uses the same three UI exposures, plus the existing garment-film poster.
for (const i of [0,1,3]) await writeFile(path.join(scratch,`panel-${i}.jpg`),stills[i]);
const images = [path.join(scratch,"panel-0.jpg"),path.join(scratch,"panel-1.jpg"),path.join(media,"bolero.jpg"),path.join(scratch,"panel-3.jpg")];
const panels = images.map((_,i)=>`[${i}:v]scale=626:344:force_original_aspect_ratio=decrease,pad=640:360:(ow-iw)/2:(oh-ih)/2:color=0x202830,setsar=1[p${i}]`).join(";");
run([...images.flatMap(file=>["-i",file]),"-filter_complex",panels+";[p0][p1][p2][p3]xstack=inputs=4:layout=0_0|640_0|0_360|640_360","-frames:v","1","-q:v","2",path.join(output,"studio-tour.jpg")]);
console.log(`Rendered the 20-second studio tour to ${output}.`);
