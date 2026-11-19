"use client";

import { useEffect, useId, useRef, useState } from "react";
import type { CutDraft as Draft } from "./DraftLab";
import "./cut-lab.css";

const garments = [["zouave_bolero", "Zouave bolero"], ["zouave_trousers", "Gathered trousers"], ["fez", "Fez cap"]];
const materials = [["dharma_habotai_8_momme", "36 in · Habotai silk"], ["dharma/cotton-muslin-3.5oz", "45 in · Cotton muslin"], ["fs_il019_natural", "58 in · Linen"]];
type Marker = {
  garment: string; material: string; copies: number; width: number; length: number;
  yield: number; area: number; bounds: [number, number, number, number][]; svg: string; source: string;
};
type Piece = { label: string; outline: string; grain: string; internal: string[]; drills: {x: number; y: number; r: number}[]; copy: number };
type Preview = { plan: Marker; pieces: Piece[]; download: string; live: boolean; custom: boolean };

function inspect(plan: Marker, live: boolean, custom: boolean): Preview {
  const document = new DOMParser().parseFromString(plan.svg, "image/svg+xml");
  const pieces = Array.from(document.querySelectorAll("g[data-instance]"), (group) => {
    const outline = group.querySelector(".cut-exterior")?.getAttribute("d");
    const grain = group.querySelector(".grain")?.getAttribute("d");
    if (!outline || !grain) throw Error("The marker has an incomplete cut path.");
    const label = group.getAttribute("data-instance")!.replace(/^tr_/, "").replace(/\.L$/, " left").replace(/\.R$/, " right");
    return { label, outline, grain, copy: Number(group.getAttribute("data-copy")) + 1,
      internal: Array.from(group.querySelectorAll(".cut-interior"), p => p.getAttribute("d") || ""),
      drills: Array.from(group.querySelectorAll("circle.drill"), p => ({
        x: Number(p.getAttribute("cx")), y: Number(p.getAttribute("cy")), r: Number(p.getAttribute("r")),
      })) };
  });
  if (!pieces.length || pieces.length !== plan.bounds.length) throw Error("The cut paths and placement count disagree.");
  return { plan, pieces, live, custom, download: URL.createObjectURL(new Blob([plan.svg], { type: "image/svg+xml" })) };
}

function MarkerDrawing({ preview, selected, select }: { preview: Preview; selected: number; select: (n: number) => void }) {
  const id = useId();
  const { width, length } = preview.plan;
  // Keep the longest stock dimension across the board. The scale stays uniform.
  const rotate = length > width;
  const w = rotate ? length : width;
  const h = rotate ? width : length;
  const offset = Math.max(w, h) * .028;
  const fill = ["#293d61", "#496181", "#374d71", "#273955"];
  return <svg className="cut-marker" role="img" aria-label={`Cutting marker with ${preview.pieces.length} physical pieces`}
    viewBox={`${-offset} ${-offset} ${w + 2*offset} ${h + 2*offset}`}>
    <defs>
      <pattern id={id+"grid"} width="25" height="25" patternUnits="userSpaceOnUse">
        <path d="M 25 0 H 0 V 25" fill="none" stroke="#a6b690" strokeWidth="1"/>
      </pattern>
      <pattern id={id+"twill"} width="6" height="6" patternUnits="userSpaceOnUse">
        <path d="M-1 1L1-1 M0 6L6 0 M5 7L7 5" stroke="#e0e8e4" strokeWidth=".9" opacity=".25"/>
      </pattern>
      <marker id={id+"arrow"} markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto" markerUnits="userSpaceOnUse">
        <path d="M0 0L8 4L0 8" fill="#f4edc5"/>
      </marker>
    </defs>
    <rect width={w} height={h} fill="#d6dfb7" stroke="#69795c" strokeWidth="2"/>
    <rect width={w} height={h} fill={`url(#${id}grid)`}/>
    <rect x="10" y="10" width={w-20} height={h-20} fill="none" stroke="#718664" strokeWidth="1.5" strokeDasharray="8 6"/>
    <g transform={rotate ? "matrix(0 1 1 0 0 0)" : `translate(0 ${length}) scale(1 -1)`}>
      {preview.pieces.map((piece, i) => <g key={i} onClick={() => select(i)} className="cut-panel">
        <title>{piece.label} · garment {piece.copy}</title>
        <path d={piece.outline} fill={selected === i ? "#805137" : fill[i % fill.length]}/>
        <path d={piece.outline} fill={`url(#${id}twill)`} stroke={selected === i ? "#e77c41" : "#1d304b"}
          strokeWidth={selected === i ? 1.8 : .8} vectorEffect="non-scaling-stroke"/>
        <path d={piece.grain} fill="none" stroke="#f4edc5" strokeWidth="2" strokeDasharray="9 5" markerEnd={`url(#${id}arrow)`}/>
        {piece.internal.map((d,j) => <path key={j} d={d} fill="none" stroke="#f0e3bc" strokeWidth="1.5"/>)}
        {piece.drills.map((drill,j) => <circle key={j} cx={drill.x} cy={drill.y} r={drill.r} fill="#f0e3bc"/>)}
      </g>)}
    </g>
  </svg>;
}

export default function CutLab({ draft }: { draft?: Draft }) {
  return <CuttingBoard key={draft?.pattern ?? "chart"} draft={draft}/>;
}

function CuttingBoard({ draft }: { draft?: Draft }) {
  const [garment, setGarment] = useState(draft ? "custom" : garments[0][0]);
  const custom = garment === "custom" ? draft : undefined;
  const program = custom?.program ?? garment;
  const [customPdf, setCustomPdf] = useState("");
  useEffect(() => {
    if (!draft) return;
    const url = URL.createObjectURL(draft.pdf);
    setCustomPdf(url);
    return () => URL.revokeObjectURL(url);
  }, [draft]);
  const [material, setMaterial] = useState(materials[1][0]);
  const [copies, setCopies] = useState(2);
  const [selected, select] = useState(0);
  const [preview, setPreview] = useState<Preview | null>(null);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  const worker = useRef<Worker | null>(null);
  const serial = useRef(0);
  useEffect(() => () => { worker.current?.terminate(); worker.current = null; }, []);
  useEffect(() => {
    const id = ++serial.current;
    const controller = new AbortController();
    const fail = (reason: unknown) => {
      if (!controller.signal.aborted) setError(reason instanceof Error ? reason.message : String(reason));
    };
    const accept = (result: Marker, live: boolean) => {
      if (controller.signal.aborted) return;
      try { setPreview(inspect(result, live, Boolean(custom))); select(0); setError(""); } catch (reason) { fail(reason); }
    };
    setError("");
    if (!retry && copies === 2 && garment === garments[0][0] && material === materials[1][0]) {
      fetch("/labs/cut-starter.json", { signal: controller.signal }).then(async response => {
        if (!response.ok) throw Error("The example marker could not load.");
        accept(await response.json(), false);
      }).catch(fail);
    } else {
      try {
        worker.current ??= new Worker("/labs/cut-worker.mjs", { type: "module" });
        worker.current.onmessage = ({ data }) => {
          if (data.id !== id) return;
          if (data.error) fail(data.error); else accept(data.result, true);
        };
        worker.current.onerror = () => {
          worker.current?.terminate(); worker.current = null;
          fail("The cutting engine could not load. Try again or open the full workbench.");
        };
        worker.current.postMessage({ id, garment: program, material, copies, pattern: custom?.pattern });
      } catch (reason) { fail(reason); }
    }
    return () => controller.abort();
  }, [garment, program, custom, material, copies, retry]);
  useEffect(() => {
    if (!preview) return;
    return () => URL.revokeObjectURL(preview.download);
  }, [preview]);
  const current = preview?.plan;
  const ready = current?.material === material && current?.garment === program && current?.copies === copies && preview?.custom === Boolean(custom) && !error;
  const piece = preview?.pieces[selected];
  const bounds = current?.bounds[selected];
  return <div className="cut-lab">
    <div className="cut-board" aria-busy={!ready && !error}>
      <div className="cut-board-top"><span>{preview?.live ? "Recalculated marker" : "Saved example marker"}</span><span>One-way grain</span></div>
      <div className="cut-board-view">{preview && <MarkerDrawing preview={preview} selected={selected} select={select}/>}
        {!ready && <div className="cut-progress" role="status">{error || "Planning the marker…"}
          {error && <button onClick={() => setRetry(n => n+1)}>Retry</button>}</div>}
      </div>
      <div className="cut-inspect">{preview && piece && bounds && <>
        <label>Inspect piece <select aria-label="Inspect piece" value={selected} onChange={e => select(Number(e.target.value))}>
          {preview.pieces.map((p,i) => <option value={i} key={i}>{p.label}{current!.copies > 1 ? ` · garment ${p.copy}` : ""}</option>)}
        </select></label><span>{Math.round(bounds[2]-bounds[0])} × {Math.round(bounds[3]-bounds[1])} mm</span>
      </>}</div>
    </div>
    <div className="cut-controls">
      <h3>Plan a cutting marker</h3><p>Change the roll or batch size to see how much fabric the pieces use.</p>
      <label>Pattern <select aria-label="Cutting pattern" value={garment} onChange={e => setGarment(e.target.value)}>
        {draft && <option value="custom">Your draft · {draft.label}</option>}
        {garments.map(([id,name])=><option key={id} value={id}>{name}</option>)}</select></label>
      <label>Fabric roll <select aria-label="Fabric roll" value={material} onChange={e => setMaterial(e.target.value)}>{materials.map(([id,name])=><option key={id} value={id}>{name}</option>)}</select></label>
      <fieldset><legend>Garments to cut</legend><div className="cut-quantity">{[1,2,3,4].map(n=>
        <button key={n} type="button" aria-pressed={n===copies} aria-label={`Cut ${n} garment${n===1?"":"s"}`} onClick={()=>setCopies(n)}>{n}</button>)}</div></fieldset>
      <dl className="cut-metrics" aria-live="polite">
        <div><dt>Roll length</dt><dd>{ready ? (Math.ceil(current.length)/1000).toFixed(3)+" m" : "—"}</dd></div>
        <div><dt>Cloth used by pieces</dt><dd>{ready ? (current.yield*100).toFixed(1)+"%" : "—"}</dd></div>
      </dl>
      {ready && preview && <a className="cut-save" href={preview.download} download={`${custom ? "custom-" : ""}${program}-${Math.round(current.width)}mm-${copies}x-marker.svg`}>Download marker SVG <span aria-hidden="true">↓</span></a>}
      {ready && (!custom || customPdf) && <a className="cut-save" href={custom ? customPdf : `/patterns/chart-${program}.pdf`}
        download={`${program}-A4.pdf`}>Full-size pattern PDF ↓</a>}
      <p className="cut-source">{ready && <a href={current.source} target="_blank" rel="noreferrer">Supplier width ↗</a>} Single-ply plan with a 4 mm cutting gap and 10 mm margins. SVG dimensions are full scale.</p>
    </div>
  </div>;
}
