// Every film the site serves is UHD at 60 frames a second, with a poster to match, a 1080p60 copy for devices
// that cannot decode it, and small enough to deploy.
//
//   node --test scripts/films.test.mjs                 the files in public/media
//   FTX_FILM_MOTION=0 node --test scripts/films.test.mjs   skip the slower distinct-frame count
//
// Needs ffprobe and ffmpeg on PATH. It reads the files; it renders nothing.
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const media = fileURLToPath(new URL("../public/media/", import.meta.url));
const WIDTH = 3840, HEIGHT = 2160, FPS = 60;
// Cloudflare refuses a static asset larger than 25 MiB.
const DEPLOY_LIMIT = 25 * 1024 * 1024;
// A film re-timed from 16, 20 or 24 frames a second by repeating frames keeps at most 27, 33 or 40 % of its
// frames distinct. Apply a stronger check to a named continuous shot when the edit also contains longer holds.
const MIN_DISTINCT = 0.45;
const continuousShots = {
  "bolero.mp4": { start: 5.2, seconds: 2.8 },
  "construction.mp4": { start: 5.2, seconds: 2.8 },
  "trousers.mp4": { start: 5.2, seconds: 2.8 },
  "studio-tour.mp4": { start: 15, seconds: 5 }, // The needle advances throughout the final chapter.
};

const names = readdirSync(media).sort();
// <film>-1080.mp4 is the copy scripts/film-renditions.sh derives from <film>.mp4; it is checked with its film.
const films = names.filter(name => name.endsWith(".mp4") && !name.endsWith("-1080.mp4"));
// Stills shown in a film's place: its poster, and the cut and join chapters of a construction study.
const stills = names.filter(name => name.endsWith(".jpg") &&
  (films.includes(name.replace(/\.jpg$/, ".mp4")) || films.includes(name.replace(/-(cut|join)\.jpg$/, ".mp4"))));

function probe(file) {
  const out = execFileSync("ffprobe", ["-v", "error", "-select_streams", "v:0", "-show_entries",
    "stream=codec_name,pix_fmt,width,height,r_frame_rate,avg_frame_rate,nb_frames:format=duration:format_tags=comment",
    "-of", "json", file], { encoding: "utf8" });
  const { streams: [stream], format } = JSON.parse(out);
  return { ...stream, duration: Number(format.duration), comment: format.tags?.comment ?? "" };
}

test("the site has films and each has its stills", () => {
  assert.ok(films.length >= 24, `${films.length} films`);
  for (const film of films) assert.ok(existsSync(media + film.replace(/\.mp4$/, ".jpg")), film + " has no poster");
});

for (const film of films) {
  test(`${film} is ${WIDTH}×${HEIGHT} at ${FPS} fps and fits the deploy limit`, () => {
    const s = probe(media + film);
    assert.equal(s.codec_name, "h264");
    assert.equal(s.pix_fmt, "yuv420p");
    assert.deepEqual([s.width, s.height], [WIDTH, HEIGHT]);
    assert.equal(s.r_frame_rate, `${FPS}/1`);
    assert.equal(s.avg_frame_rate, `${FPS}/1`);
    assert.equal(Number(s.nb_frames), Math.round(s.duration * FPS), "frame count against duration");
    const size = statSync(media + film).size;
    assert.ok(size <= DEPLOY_LIMIT, `${(size / 2 ** 20).toFixed(1)} MiB is over the 25 MiB asset limit`);
  });

  test(`${film} has a 1080p copy at ${FPS} fps made from this film`, () => {
    const copy = media + film.replace(/\.mp4$/, "-1080.mp4");
    assert.ok(existsSync(copy), "no 1080p copy: run sh scripts/film-renditions.sh");
    const s = probe(copy);
    assert.equal(s.codec_name, "h264");
    assert.equal(s.pix_fmt, "yuv420p");
    assert.deepEqual([s.width, s.height], [1920, 1080]);
    assert.equal(s.r_frame_rate, `${FPS}/1`);
    assert.equal(Number(s.nb_frames), Number(probe(media + film).nb_frames), "frame count against the film");
    // A re-rendered film with an old copy would show phones a different cut.
    const sha = createHash("sha256").update(readFileSync(media + film)).digest("hex");
    assert.equal(s.comment, `FrankenTextiles 1080p copy of sha256 ${sha}`);
  });

  test(`${film} shows distinct frames during ${FPS} fps motion`, { skip: process.env.FTX_FILM_MOTION === "0" }, () => {
    const shot = continuousShots[film];
    const segment = shot ? ["-ss", String(shot.start), "-t", String(shot.seconds)] : [];
    const total = shot ? Math.round(shot.seconds * FPS) : Number(probe(media + film).nb_frames);
    const minimum = shot ? 0.90 : MIN_DISTINCT;
    // mpdecimate drops a frame that repeats the one before it; the null muxer reports how many were kept.
    const run = spawnSync("ffmpeg", ["-hide_banner", "-nostats", "-threads", "2", ...segment, "-i", media + film, "-vf", "mpdecimate", "-fps_mode", "vfr",
      "-an", "-f", "null", "-"], { encoding: "utf8", maxBuffer: 64 * 2 ** 20 });
    assert.equal(run.status, 0, run.stderr.slice(-400));
    const kept = Number(run.stderr.match(/frame=\s*(\d+)(?![\s\S]*frame=)/)?.[1]);
    assert.ok(kept / total >= minimum, `${kept} of ${total} frames are distinct (${(100 * kept / total).toFixed(0)} %)`);
  });
}

for (const still of stills) {
  test(`${still} is at least ${WIDTH}×${HEIGHT}`, () => {
    const s = probe(media + still);
    assert.ok(s.width >= WIDTH && s.height >= HEIGHT, `${s.width}×${s.height}`);
  });
}
