"use client";

import { useEffect, useRef, useState } from "react";
import "./draft-lab.css";

const garments = {
  zouave_bolero: { name: "Bolero", key: "chest_ease", label: "Chest ease", unit: "mm", min: 60, max: 140, step: 5, initial: 100, first: "front" },
  zouave_trousers: { name: "Trousers", key: "fullness", label: "Leg fullness", unit: "mm", min: 30, max: 100, step: 5, initial: 60, first: "tr_front" },
  fez: { name: "Fez", key: "height_frac", label: "Crown height / head diameter", unit: "%", min: 45, max: 80, step: 1, initial: 62, first: "crown" },
} as const;
type Garment = keyof typeof garments;
type FileBytes = { bytes: Uint8Array<ArrayBuffer>; name: string };
type Reply = { key: string; preview: string; pattern: string; flat: string; checks: string; passed: number;
  total: number; offTarget: string[]; files: Partial<Record<"pdf" | "svg", FileBytes>> };
type Selection = { key: string; program: Garment; height: number; style: Record<string, number> };
export type Piece = { id: string; name: string; quantity: string; allowance: string; cut: string[];
  sew: string[]; marks: string[]; trim: string[]; left: number; top: number; width: number; height: number };

export function readPieces(source: string): Piece[] {
  const sheet = new DOMParser().parseFromString(source, "image/svg+xml");
  const pieces = Array.from(sheet.querySelectorAll("g[data-piece]"), g => {
    const paths = (selector: string) => Array.from(g.querySelectorAll(selector), path => path.getAttribute("d") || "");
    const cut = paths(".cut path");
    // The native master sheet emits its sampled cut contours as M/L coordinate pairs in mm.
    const points = cut.join(" ").matchAll(/(?:M|L)\s*(-?[\d.]+),\s*(-?[\d.]+)/g);
    let left = Infinity, top = Infinity, right = -Infinity, bottom = -Infinity;
    for (const [, x, y] of points) {
      left = Math.min(left, Number(x)); right = Math.max(right, Number(x));
      top = Math.min(top, Number(y)); bottom = Math.max(bottom, Number(y));
    }
    if (!(right > left && bottom > top)) throw new Error("The exported cut contour has no extent.");
    const name = g.getAttribute("data-name")?.toLowerCase() || "piece";
    return { id: g.getAttribute("data-piece")!, name, quantity: "Cut " + g.getAttribute("data-qty") +
      (g.getAttribute("data-on-fold") ? " on fold" : g.getAttribute("data-mirror") === "true" ? " mirrored" : ""),
      allowance: g.getAttribute("data-sa-mm") || "0", cut, sew: paths(".sew path"),
      marks: paths(".notches path, .grain path, .fold path"), trim: paths(".decor-soutache path"),
      left, top, width: right - left, height: bottom - top };
  });
  if (!pieces.length) throw new Error("The engine returned a sheet with no cut pieces.");
  return pieces;
}

export function Drawing({ piece, miniature = false, reference }: { piece: Piece; miniature?: boolean; reference?: Piece }) {
  const width = Math.max(piece.width, reference?.width ?? 0), height = Math.max(piece.height, reference?.height ?? 0);
  const pad = Math.max(width, height) * (miniature ? 0.06 : 0.13);
  const right = piece.left + piece.width, bottom = piece.top + piece.height;
  const measureY = bottom + pad * 0.47, measureX = right + pad * 0.47;
  return <svg aria-hidden={miniature || undefined} role={miniature ? undefined : "img"}
    aria-label={miniature ? undefined : `${piece.name} pattern piece with cutting line, sewing line and grain marks`}
    className={miniature ? "dl-small-drawing" : "dl-drawing"}
    viewBox={`${piece.left - pad} ${bottom - height - pad} ${width + 2 * pad} ${height + 2 * pad}`}>
    {piece.cut.map((d,i) => <path key={i} className="dl-cut" d={d}/>)}
    {!miniature && <>
      {piece.sew.map((d,i) => <path key={i} className="dl-sew" d={d}/>)}
      {piece.marks.map((d,i) => <path key={i} className="dl-mark" d={d}/>)}
      {piece.trim.map((d,i) => <path key={i} className="dl-braid" d={d}/>)}
      {reference && <g transform={`translate(${piece.left-reference.left} ${bottom-reference.top-reference.height})`}>
        {reference.cut.map((d,i) => <path key={i} className="dl-reference" d={d}/>)}
      </g>}
      <g className="dl-measure" fontSize={pad * 0.34}>
        <path d={`M${piece.left},${measureY} H${right} M${measureX},${piece.top} V${bottom}`}/>
        {[piece.left, right].map(x => <path key={x} d={`M${x},${measureY-pad*.13} v${pad*.26}`}/>)}
        {[piece.top, bottom].map(y => <path key={y} d={`M${measureX-pad*.13},${y} h${pad*.26}`}/>)}
        <text x={piece.left+piece.width/2} y={measureY+pad*.41}>{piece.width.toFixed(1)}</text>
        <text transform={`translate(${measureX+pad*.37},${piece.top+piece.height/2}) rotate(-90)`}>{piece.height.toFixed(1)}</text>
      </g>
    </>}
  </svg>;
}

type Result = Omit<Reply, "files" | "preview"> & { pieces: Piece[]; files: { url: string; blob: Blob; name: string; type: string }[] };
export type CutDraft = { program: string; label: string; pattern: string; pdf: Blob };
export default function DraftLab({ enabled, onPlanCut }: { enabled: boolean; onPlanCut?: (draft: CutDraft) => void }) {
  const section = useRef<HTMLDivElement>(null);
  const [active, setActive] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [program, setProgram] = useState<Garment>("zouave_bolero");
  const [height, setHeight] = useState(92);
  const [value, setValue] = useState(100);
  const [selected, selectPiece] = useState<string>("front");
  const [result, setResult] = useState<Result>();
  const [pin, setPin] = useState<{pieces: Piece[]; label: string; key: string}>();
  const [sample, setSample] = useState<Piece[]>();
  const [problem, setProblem] = useState<{key: string; message: string}>();
  const current = useRef("");
  const submit = useRef<((request: Selection) => void) | undefined>(undefined);
  const preset = garments[program];
  const key = [program, height, value].join("/");

  useEffect(() => {
    if (!enabled) return;
    const controller = new AbortController();
    const observer = new IntersectionObserver(([entry]) => {
      if (!entry.isIntersecting) return;
      setActive(true); observer.disconnect();
      fetch("/labs/draft-bolero.svg", { signal: controller.signal })
        .then(r => r.ok ? r.text() : Promise.reject(new Error("Preview unavailable")))
        .then(raw => { if (!controller.signal.aborted) setSample(readPieces(raw)); })
        .catch(() => {}); // The live round provides the same pattern when available.
    }, { rootMargin: "120px" });
    if (section.current) observer.observe(section.current);
    return () => { observer.disconnect(); controller.abort(); };
  }, [enabled]);

  useEffect(() => {
    if (!active) return;
    let engine: Worker;
    try { engine = new Worker("/labs/draft-worker.js", { type: "module" }); }
    catch { setProblem({key:"*", message:"The browser could not start the local drafting engine."}); return; }
    let busy = false;
    let pending: Selection | undefined;
    const send = (request: Selection) => {
      if (busy) { pending = request; return; }
      busy = true;
      engine.postMessage(request);
    };
    submit.current = send;
    engine.onmessage = ({ data }: MessageEvent<Reply | {key:string; error:string}>) => {
      busy = false;
      if (data.key === current.current) {
        try {
          if ("error" in data) throw new Error(data.error);
          const pieces = readPieces(data.preview);
          const files = Object.entries(data.files).map(([type, file]) => {
            const blob = new Blob([file.bytes], { type: type === "svg" ? "image/svg+xml" : "application/pdf" });
            return { name: file.name, type, blob, url: URL.createObjectURL(blob) };
          });
          setResult({key:data.key,pieces,files,pattern:data.pattern,checks:data.checks,flat:data.flat,
            passed:data.passed,total:data.total,offTarget:data.offTarget});
          setProblem(undefined);
        } catch (error) {
          setProblem({ key: data.key, message: error instanceof Error ? error.message : String(error) });
        }
      }
      if (pending) { const next = pending; pending = undefined; send(next); }
    };
    engine.onerror = () => {
      setProblem({key:"*", message:"The local drafting engine could not load."});
      submit.current = undefined; engine.terminate();
    };
    return () => { submit.current = undefined; engine.terminate(); };
  }, [active, attempt]);

  useEffect(() => {
    current.current = key;
    if (!active) return;
    const timer = setTimeout(() => submit.current?.({ key, program, height,
      style: { [preset.key]: program === "fez" ? value / 100 : value } }), 140);
    return () => clearTimeout(timer);
  }, [active, attempt, key, program, height, value, preset.key]);
  useEffect(() => () => { result?.files.forEach(file => URL.revokeObjectURL(file.url)); }, [result]);

  const error = problem && (problem.key === key || problem.key === "*") ? problem.message : "";
  const fresh = result?.key === key && !error ? result : undefined;
  const pieces = fresh?.pieces ?? (key === "zouave_bolero/92/100" ? sample : undefined);
  const piece = pieces?.find(piece => piece.id === selected) ?? pieces?.[0];
  function choose(next: Garment) {
    if (next !== program) setPin(undefined);
    setProgram(next); setValue(garments[next].initial); selectPiece(garments[next].first); setActive(true);
  }
  return <div className="draft-lab" ref={section}>
    <div className="dl-controls">
      <fieldset><legend>Garment</legend><div className="dl-garments">
        {(Object.keys(garments) as Garment[]).map(id => <button key={id} type="button" aria-pressed={id === program}
          onClick={() => choose(id)}>{garments[id].name}</button>)}
      </div></fieldset>
      <label className="dl-slider">Chart stature <output>{height} cm</output>
        <input type="range" min="80" max="128" step="2" value={height} aria-label="Chart stature in centimetres"
          onChange={e => {setHeight(+e.target.value); setActive(true);}}/>
      </label>
      <label className="dl-slider">{preset.label} <output>{value}{preset.unit === "%" ? "%" : " mm"}</output>
        <input type="range" min={preset.min} max={preset.max} step={preset.step} value={value} aria-label={preset.label}
          onChange={e => {setValue(+e.target.value); setActive(true);}}/>
      </label>
      <div className="dl-results" aria-live="polite">
        {error ? <><p role="alert">{error}</p><button className="dl-retry" onClick={() => {
          setProblem(undefined); setAttempt(n => n+1); setActive(true);
        }}>Retry draft</button></> : fresh ? <>
          <p className={fresh.flat === "CONVERGED" ? "dl-ok" : "dl-review"}>
            <i/>{fresh.flat === "CONVERGED" ? "Flat checks passed" : "Review this draft"}</p>
          <p>{fresh.passed} of {fresh.total} seams pass · {fresh.offTarget.length ? fresh.offTarget.join(", ") + " outside target" : fresh.checks.replace(/^PASS: /,"") + " on exports"}</p>
        </> : <p role="status">Drafting and checking locally…</p>}
      </div>
      <div className="dl-downloads">{fresh?.files.map(f => <a href={f.url} download={f.name} key={f.type}>
        {f.type === "pdf" ? "A4 pattern PDF" : "Master SVG"} <span aria-hidden="true">↓</span>
      </a>)}</div>
      <a className="dl-watch-link" href={"/?garment="+({zouave_bolero:"bolero",zouave_trousers:"trousers",fez:"fez"}[program])+"#construction"}>Watch the reference {preset.name.toLowerCase()} take shape <span aria-hidden="true">↗</span></a>
      <a className="dl-watch-link" href="/studies/tiling/">See how the A4 tiles tape together <span aria-hidden="true">→</span></a>
      {onPlanCut && fresh?.flat === "CONVERGED" && fresh.pattern && fresh.files.some(f => f.type === "pdf") &&
        <button className="dl-plan-cut" type="button" onClick={() => onPlanCut({program,
          label:`${preset.name} · ${height} cm · ${preset.label.toLowerCase()} ${value}${preset.unit}`, pattern:fresh.pattern,
          pdf:fresh.files.find(f => f.type === "pdf")!.blob})}>
          Plan this cut <span aria-hidden="true">→</span>
        </button>}
    </div>
    <div className="dl-board">
      <div className="dl-plate" aria-busy={!fresh && !error}>
        <div className="dl-plate-top"><span>{piece ? piece.name : "Cut piece"}</span><span>{piece?.quantity}</span></div>
        {piece ? <Drawing piece={piece} reference={pin?.key !== key ? pin?.pieces.find(p => p.id === piece.id) : undefined}/> :
          <div className="dl-empty">{error ? "Draft unavailable" : "Preparing the pattern…"}</div>}
        <div className="dl-plate-foot"><span><i/> Cut <i/> Stitch</span>{piece && <span>mm · {piece.allowance} mm allowance</span>}</div>
      </div>
      <div className="dl-pieces" role="group" aria-label="Inspect a cut piece">
        {pieces?.map(p => <button type="button" key={p.id} aria-pressed={piece?.id === p.id}
          onClick={() => selectPiece(p.id)}><Drawing piece={p} miniature/>{p.name}</button>)}
      </div>
      <div className="dl-compare"><button type="button" disabled={!pin && !fresh} aria-pressed={!!pin}
        onClick={() => pin ? setPin(undefined) : fresh && setPin({pieces:fresh.pieces, key, label:`${height} cm · ${value}${preset.unit}`})}>
        {pin ? "Unpin outline" : "Pin outline"}</button>
        <span>{pin ? pin.key === key ? "Move a slider to compare." : `Pinned ${pin.label} · aligned at lower left` : "Compare two drafts at the same scale."}</span>
      </div>
    </div>
  </div>;
}
