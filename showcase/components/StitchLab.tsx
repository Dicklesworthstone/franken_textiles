"use client";

import { useEffect, useRef, useState } from "react";
import "./stitch-lab.css";

const panels = [
  ["back", "Back"], ["front_R", "Right front"], ["front_L", "Left front"],
  ["sleeve_R", "Right sleeve"], ["sleeve_L", "Left sleeve"],
] as const;
type Panel = typeof panels[number][0];
type Move = { x: number; y: number; nextX: number; nextY: number; stitch: number; jump: boolean; braid: boolean };
type Design = ReturnType<typeof decode>;
type View = "panel" | "needle" | "braid";

function braidDetail(moves: Move[]) {
  const runs: Move[][] = [];
  let start = true;
  for (const move of moves) {
    if (!move.braid) continue;
    if (move.jump) { start = true; continue; }
    if (start) { runs.push([]); start = false; }
    runs[runs.length - 1].push(move);
  }
  // A closed, two-dimensional run is the exported trefoil. A sleeve carries only a hem band.
  for (const run of runs) {
    const first = run[0], last = run[run.length - 1];
    if (Math.hypot(first.x - last.nextX, first.y - last.nextY) > 18) continue;
    const xs = run.map(m => m.nextX), ys = run.map(m => m.nextY);
    const loX = Math.min(...xs), hiX = Math.max(...xs), loY = Math.min(...ys), hiY = Math.max(...ys);
    if (hiX - loX > 30 && hiY - loY > 30) return { x: (hiX+loX)/2, y: (hiY+loY)/2, span: Math.max(hiX-loX,hiY-loY) * 1.65, label: "Trefoil detail" };
  }
  const run = runs[0];
  if (!run) throw Error("Missing couching pass");
  const mid = run[Math.floor(run.length/2)];
  return {x: mid.x, y: mid.y, span: 400, label: "Hem band detail"};
}

// Decode the exported machine records directly. Coordinates are tenths of a millimetre.
// Each tuple is [byte index, distance, positive X bit, negative X bit, positive Y bit, negative Y bit].
const steps = [[0,1,1,2,128,64],[1,3,1,2,128,64],[0,9,4,8,32,16],[1,27,4,8,32,16],[2,81,4,8,32,16]];

function decode(data: Uint8Array) {
  const header = new TextDecoder().decode(data.subarray(0, 512));
  const expected = Number(/\rST:\s*(\d+)\r/.exec(header)?.[1]);
  if (!header.startsWith("LA:") || data.length !== 512 + 3 * (expected + 1)) throw Error("Incomplete DST header or records");
  const moves: Move[] = [];
  const needles: [number, number][] = [[0, 0]];
  let x = 0, y = 0, minX = 0, maxX = 0, minY = 0, maxY = 0, stop = -1;
  for (let pos = 512; pos < data.length; pos += 3) {
    const flag = data[pos + 2];
    if (data[pos] === 0 && data[pos + 1] === 0 && flag === 0xf3) {
      if (pos !== data.length - 3 || stop < 0) throw Error("Unexpected DST end");
      return { moves, needles, stop, minX, maxX, minY, maxY, detail: braidDetail(moves) };
    }
    if ((flag & 0xc0) === 0xc0) {
      // This exhibit is made from the exporter's two-pass soutache designs.
      if (stop >= 0) throw Error("Unexpected additional DST stop");
      stop = needles.length - 1;
      continue;
    }
    const jump = Boolean(flag & 0x80);
    let nextX = x, nextY = y;
    for (const [byte, distance, plusX, minusX, plusY, minusY] of steps) {
      const bits = data[pos + byte];
      nextX += distance * (Number(Boolean(bits & plusX)) - Number(Boolean(bits & minusX)));
      nextY += distance * (Number(Boolean(bits & plusY)) - Number(Boolean(bits & minusY)));
    }
    // Travel belongs to the next needle penetration, including split jumps.
    moves.push({ x, y, nextX, nextY, stitch: needles.length, jump, braid: stop >= 0 });
    if (!jump) needles.push([nextX, nextY]);
    x = nextX; y = nextY;
    minX = Math.min(minX, x); maxX = Math.max(maxX, x);
    minY = Math.min(minY, y); maxY = Math.max(maxY, y);
  }
  throw Error("Missing DST end record");
}

function draw(canvas: HTMLCanvasElement, design: Design, position: number, view: View, travel: boolean, thumbnail = false) {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const { width, height } = canvas.getBoundingClientRect();
  if (!width || !height) return;
  const ratio = Math.min(window.devicePixelRatio || 1, 2);
  const pixelWidth = Math.round(width * ratio), pixelHeight = Math.round(height * ratio);
  if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) {
    canvas.width = pixelWidth; canvas.height = pixelHeight;
  }
  ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
  ctx.clearRect(0, 0, width, height);
  const [needleX, needleY] = design.needles[position];
  const detail = view !== "panel";
  const pad = thumbnail ? 9 : 58;
  const fit = Math.min((width - pad) / (design.maxX - design.minX), (height - pad) / (design.maxY - design.minY));
  const scale = view === "braid" ? Math.min(width,height)/design.detail.span : fit * (view === "needle" ? 3.8 : 1);
  const midX = view === "braid" ? design.detail.x : view === "needle" ? needleX : (design.minX + design.maxX) / 2;
  const midY = view === "braid" ? design.detail.y : view === "needle" ? needleY : (design.minY + design.maxY) / 2;
  const px = (x: number) => (x - midX) * scale + width / 2;
  const py = (y: number) => (midY - y) * scale + height / 2;
  const guide = new Path2D(), placement = new Path2D(), braid = new Path2D(), skips = new Path2D();
  const glints = new Path2D(), holes = new Path2D();
  for (const move of design.moves) {
    const ax = px(move.x), ay = py(move.y), bx = px(move.nextX), by = py(move.nextY);
    if (move.jump) {
      if (travel && move.stitch <= position) { skips.moveTo(ax, ay); skips.lineTo(bx, by); }
      continue;
    }
    guide.moveTo(ax, ay); guide.lineTo(bx, by);
    if (move.stitch > position) continue;
    const path = move.braid ? braid : placement;
    path.moveTo(ax, ay); path.lineTo(bx, by);
    glints.moveTo(ax + (bx - ax) * .16, ay + (by - ay) * .16 - .45);
    glints.lineTo(ax + (bx - ax) * .68, ay + (by - ay) * .68 - .45);
    holes.moveTo(bx + (detail ? 1.1 : .58), by);
    holes.arc(bx, by, detail ? 1.1 : .58, 0, Math.PI * 2);
  }
  function stroke(path: Path2D, color: string, weight: number) {
    if (!ctx) return;
    ctx.strokeStyle = color; ctx.lineWidth = weight; ctx.stroke(path);
  }
  ctx.lineCap = "round"; ctx.lineJoin = "round";
  stroke(guide, "#8d9ba655", 1);
  ctx.setLineDash([3,5]); stroke(skips, "#a0caf3b3", 1); ctx.setLineDash([]);
  const mag = thumbnail ? .55 : 1;
  stroke(placement, "#131522", (detail ? 5 : 3.3) * mag);
  stroke(placement, "#dac9a5", (detail ? 3.3 : 1.9) * mag);
  stroke(braid, "#181322", (detail ? 9.2 : 4.8) * mag);
  stroke(braid, "#925533", (detail ? 7.5 : 3.6) * mag);
  stroke(braid, "#e9a96e", (detail ? 4.8 : 2.5) * mag);
  stroke(glints, "#fff1c6", (detail ? 1.6 : .78) * mag);
  if (!thumbnail) { ctx.fillStyle = "#2b2231"; ctx.fill(holes); }
  if (!thumbnail && (position < design.needles.length - 1 || view === "needle")) {
    ctx.strokeStyle = "#fff5d1"; ctx.lineWidth = 1.1;
    ctx.beginPath(); ctx.arc(px(needleX), py(needleY), 7, 0, 2 * Math.PI); ctx.stroke();
    ctx.fillStyle = "#fff5d1"; ctx.beginPath(); ctx.arc(px(needleX), py(needleY), 2, 0, 2 * Math.PI); ctx.fill();
  }
  if (thumbnail) return;
  const barMm = detail ? 10 : 50;
  const bar = barMm * 10 * scale;
  const start = 19, base = height - 17;
  ctx.strokeStyle = "#bcc4d3"; ctx.lineWidth = 1;
  ctx.beginPath(); ctx.moveTo(start, base - 4); ctx.lineTo(start, base);
  ctx.lineTo(start + bar, base); ctx.lineTo(start + bar, base - 4); ctx.stroke();
  ctx.fillStyle = "#d5d9e5"; ctx.font = '10px "IBM Plex Mono", monospace';
  ctx.fillText(`${barMm} mm`, start, base - 8);
}

export default function StitchLab({ active }: { active: boolean }) {
  const [panel, setPanel] = useState<Panel>("back");
  const [design, setDesign] = useState<Design | null>(null);
  const [error, setError] = useState(false);
  const [retry, setRetry] = useState(0);
  const [position, setPosition] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [view, setView] = useState<View>("braid");
  const [travel, setTravel] = useState(false);
  const canvas = useRef<HTMLCanvasElement>(null);
  const inset = useRef<HTMLCanvasElement>(null);
  const cursor = useRef(0);
  const file = `/labs/stitch-${panel}.dst`;
  const count = design ? design.needles.length - 1 : 0;

  useEffect(() => {
    if (!active) setPlaying(false);
  }, [active]);

  useEffect(() => {
    const abort = new AbortController();
    setError(false); setDesign(null); setPlaying(false);
    async function load() {
      const response = await fetch(file, { signal: abort.signal });
      if (!response.ok) throw Error("Unavailable stitch file");
      const decoded = decode(new Uint8Array(await response.arrayBuffer()));
      if (!abort.signal.aborted) {
        setPosition(decoded.needles.length - 1); cursor.current = decoded.needles.length - 1;
        setDesign(decoded);
      }
    }
    load().catch(() => { if (!abort.signal.aborted) setError(true); });
    return () => abort.abort();
  }, [file, retry]);

  useEffect(() => {
    if (!active || !playing || !design) return;
    let id = 0;
    const start = performance.now(), from = cursor.current;
    const to = from < design.stop ? design.stop : count;
    const frame = (now: number) => {
      const next = Math.min(to, from + Math.floor((now - start) * .058));
      cursor.current = next; setPosition(next);
      if (next === to) setPlaying(false);
      else id = requestAnimationFrame(frame);
    };
    id = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(id);
  }, [active, playing, design, count]);

  useEffect(() => {
    const el = canvas.current;
    if (!active || !design || !el) return;
    const paint = () => {
      draw(el, design, position, view, travel);
      if (inset.current) draw(inset.current, design, position, view === "panel" ? "braid" : "panel", travel, true);
    };
    paint();
    const observer = new ResizeObserver(paint);
    observer.observe(el);
    return () => observer.disconnect();
  }, [active, design, position, view, travel]);

  function seek(value: number) { setPlaying(false); cursor.current = value; setPosition(value); }
  function play() {
    if (!playing && position === count) { cursor.current = 0; setPosition(0); setView("panel"); }
    setPlaying(!playing);
  }
  const stopped = design && position === design.stop;
  const phase = !design ? "Loading design" : position === 0 ? "Hoop centre"
    : stopped ? "Stop · lay the braid" : position < design.stop ? "Placement pass" : position === count ? "Complete" : "Couching pass";
  return <div className="stitch-lab">
    <div className="stitch-figure">
      <div className="stitch-cloth">
        <span className="stitch-caption">{`Bolero / ${panels.find(([id]) => id === panel)?.[1]}`}
          {design && <b>{view === "braid" ? design.detail.label : view === "needle" ? "Needle detail" : "Full panel"}</b>}</span>
        {design && <><canvas ref={canvas} role="img" aria-label={`Embroidery path for the bolero ${panel}. ${position} of ${count} stitches.`}/>
          <button className="stitch-inset" onClick={() => setView(view === "panel" ? "braid" : "panel")}>
            <canvas ref={inset} aria-hidden="true"/><span>{view === "panel" ? "Braid detail" : "Full panel"} ↗</span>
          </button></>}
        {error && <div className="stitch-error" role="alert">Couldn’t load this design. <button onClick={() => setRetry(r => r + 1)}>Retry</button></div>}
        {!error && !design && <div className="stitch-error" role="status">Reading stitch file…</div>}
      </div>
      <div className="stitch-key"><span><i/> Placement</span><span><i/> Couching</span><span className="stitch-phase">{phase}</span></div>
    </div>
    <div className="stitch-desk">
      <div className="stitch-top"><span className="stitch-label">Tajima DST</span><h3>Stitch the braid.</h3>
        <p>Follow the needle around the panel, then lay the braid at the machine stop and watch the second pass secure it.</p></div>
      <label className="stitch-field">Bolero panel<select value={panel} onChange={e => {
        setPlaying(false); setDesign(null); setPosition(0); setView("braid"); setPanel(e.target.value as Panel);
      }}>{panels.map(([id, name]) => <option key={id} value={id}>{name}</option>)}</select></label>
      <div className="stitch-dim"><span>Design size</span><strong>{design ? `${((design.maxX-design.minX)/10).toFixed(1)} × ${((design.maxY-design.minY)/10).toFixed(1)} mm` : "—"}</strong></div>
      <div className="stitch-progress"><label htmlFor="stitch-timeline">Needle position</label><output>{position} / {count}</output></div>
      <input id="stitch-timeline" type="range" min="0" max={count} step="1" value={position} disabled={!design}
        aria-valuetext={`Stitch ${position} of ${count}. ${phase}`} onChange={e => seek(Number(e.target.value))}/>
      <div className="stitch-buttons"><button disabled={!design} className="stitch-play" onClick={play}>
        <span aria-hidden="true">{playing ? "Ⅱ" : "▶"}</span> {playing ? "Pause" : stopped ? "Stitch braid" : position === count ? "Replay stitches" : "Play stitches"}</button>
        <button className="stitch-skip" disabled={!design || Boolean(stopped)} onClick={() => { if (design) { seek(design.stop); setView("panel"); } }}>Braid stop</button></div>
      <div className="stitch-options"><label><input type="checkbox" checked={view === "needle"} onChange={e => setView(e.target.checked ? "needle" : "panel")}/> Needle loupe</label>
        <label><input type="checkbox" checked={travel} onChange={e => setTravel(e.target.checked)}/> Jump moves</label></div>
      {design && <a className="stitch-download" download={`ZOUAVE_BOLERO_${panel}_920_mm_stature_soutache.dst`} href={file}>Download this DST <span aria-hidden="true">↓</span></a>}
      <a className="stitch-download" href="/patterns/bolero.pdf" download>Full-size bolero pattern PDF ↓</a>
    </div>
  </div>;
}
