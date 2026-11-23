"use client";

import { useEffect, useId, useMemo, useRef, useState } from "react";
import SiteHeader from "./site/SiteHeader";
import SiteFooter from "./site/SiteFooter";
import Crumbs from "./site/Crumbs";
import CodeBlock from "./site/CodeBlock";
import { workbench } from "./site/links";
import "./wash-shrink.css";

type SeamRow = {
  seam: string;
  a: string;
  b: string;
  len_a_mm: number;
  len_b_mm: number;
  ratio: number;
  target_ratio: string;
  residual_mm: number;
  status: string;
};

type PieceReport = {
  piece_id: string;
  transition: string;
  before_material_sha256: string;
  after_material_sha256: string;
  transition_sha256: string;
  anchor_mm: [number, number];
  scale_warp_weft: [number, number];
};

type WashResult = {
  config: {
    garment: string;
    transitionKey: string;
    mode: "treat" | "compensate";
    mismatchMaterial: boolean;
  };
  transition: {
    key: string;
    specimen: string;
    beforeId: string;
    beforeName: string;
    afterId: string;
    afterName: string;
    elastanePercent: string | null;
    warpShrinkPct: number;
    weftShrinkPct: number;
    cycles: number;
    process: string;
    conditioningBefore: string;
    conditioningAfter: string;
    method: string;
    sourceRef: string | null;
    sourceNote: string | null;
  };
  mode: "treat" | "compensate";
  scope: string;
  sourcePatternSha256: string;
  patternSha256: string;
  requestSha256: string;
  pieces: PieceReport[];
  seamsBefore: SeamRow[];
  seamsAfter: SeamRow[];
  originalSvg: string;
  revisedSvg: string;
  revisedStatus: string;
  revisedChecks: string;
  frozenRequestJson: string;
  files?: {
    pdf?: { bytes: Uint8Array; name: string };
    svg?: { bytes: Uint8Array; name: string };
  };
};

type ExtractedPiece = {
  id: string;
  name: string;
  cut: string[];
  sew: string[];
  marks: string[];
  minX: number;
  minY: number;
  width: number;
  height: number;
};

type PresetId = "denim1_treat" | "denim1_compensate" | "denim5_treat" | "muslin_refusal";

const PRESETS: {
  id: PresetId;
  label: string;
  garment: string;
  transitionKey: "denim1" | "denim5";
  mode: "treat" | "compensate";
  mismatchMaterial: boolean;
}[] = [
  {
    id: "denim1_treat",
    label: "Denim 1 (1.5% elastane) · Post-wash",
    garment: "zouave_bolero",
    transitionKey: "denim1",
    mode: "treat",
    mismatchMaterial: false,
  },
  {
    id: "denim1_compensate",
    label: "Denim 1 · Pre-wash recut (compensate)",
    garment: "zouave_bolero",
    transitionKey: "denim1",
    mode: "compensate",
    mismatchMaterial: false,
  },
  {
    id: "denim5_treat",
    label: "Denim 5 (0.5% elastane) · Post-wash",
    garment: "zouave_bolero",
    transitionKey: "denim5",
    mode: "treat",
    mismatchMaterial: false,
  },
  {
    id: "muslin_refusal",
    label: "Cotton muslin + Denim 1 wash (refusal)",
    garment: "zouave_bolero",
    transitionKey: "denim1",
    mode: "treat",
    mismatchMaterial: true,
  },
];

function parsePathBounds(paths: string[]): { minX: number; minY: number; maxX: number; maxY: number } {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const d of paths) {
    const nums = d.match(/-?\d+(?:\.\d+)?/g);
    if (!nums) continue;
    for (let i = 0; i + 1 < nums.length; i += 2) {
      const x = Number(nums[i]);
      const y = Number(nums[i + 1]);
      if (!Number.isFinite(x) || !Number.isFinite(y)) continue;
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
    }
  }
  return { minX, minY, maxX, maxY };
}

function extractPieces(svgText: string): ExtractedPiece[] {
  if (typeof DOMParser === "undefined" || !svgText) return [];
  const doc = new DOMParser().parseFromString(svgText, "image/svg+xml");
  return Array.from(doc.querySelectorAll("g[data-piece]"), g => {
    const id = g.getAttribute("data-piece") || "";
    const name = g.getAttribute("data-name") || id.toUpperCase();
    const cut = Array.from(g.querySelectorAll("g.cut path"), p => p.getAttribute("d") || "").filter(Boolean);
    const sew = Array.from(g.querySelectorAll("g.sew path"), p => p.getAttribute("d") || "").filter(Boolean);
    const marks = Array.from(g.querySelectorAll("g.marks path, g.marks line"), p => {
      if (p.tagName.toLowerCase() === "line") {
        return `M${p.getAttribute("x1")},${p.getAttribute("y1")} L${p.getAttribute("x2")},${p.getAttribute("y2")}`;
      }
      return p.getAttribute("d") || "";
    }).filter(Boolean);
    const b = parsePathBounds(cut.length ? cut : sew);
    return {
      id,
      name,
      cut,
      sew,
      marks,
      minX: Number.isFinite(b.minX) ? b.minX : 0,
      minY: Number.isFinite(b.minY) ? b.minY : 0,
      width: Number.isFinite(b.maxX - b.minX) ? b.maxX - b.minX : 100,
      height: Number.isFinite(b.maxY - b.minY) ? b.maxY - b.minY : 100,
    };
  });
}

function OverlayStage({
  original,
  revised,
  pieceId,
  mode,
}: {
  original: ExtractedPiece[];
  revised: ExtractedPiece[];
  pieceId: string;
  mode: "treat" | "compensate";
}) {
  const uid = useId();
  const orig = original.find(p => p.id === pieceId) ?? original[0];
  const rev = revised.find(p => p.id === pieceId) ?? revised[0];
  if (!orig || !rev) return null;

  const maxW = Math.max(orig.width, rev.width);
  const maxH = Math.max(orig.height, rev.height);
  const padY = Math.max(30, maxH * 0.13);
  const vbH = maxH + padY * 2;
  const padX = Math.max(44, maxW * 0.18);
  const vbW = Math.max(maxW + padX * 2, vbH * 1.48);

  // Center both outlines on the same mm origin so anisotropic warp (vertical) vs weft (horizontal)
  // shrinkage or compensation is directly legible around the entire perimeter.
  const cx = vbW / 2;
  const cy = vbH / 2;
  const origTx = cx - (orig.minX + orig.width / 2);
  const origTy = cy - (orig.minY + orig.height / 2);
  const revTx = cx - (rev.minX + rev.width / 2);
  const revTy = cy - (rev.minY + rev.height / 2);
  const accent = mode === "compensate" ? "#1e6e60" : "#b24322";

  return (
    <svg
      className="ws-svg"
      role="img"
      aria-label={`Superimposed original and ${mode === "compensate" ? "pre-wash compensated" : "post-wash shrunk"} cutting outlines for ${orig.name}`}
      viewBox={`0 0 ${vbW.toFixed(1)} ${vbH.toFixed(1)}`}
    >
      <defs>
        <pattern id={`${uid}grid`} width="20" height="20" patternUnits="userSpaceOnUse">
          <path d="M 20 0 H 0 V 20" fill="none" stroke="#c8c5b6" strokeWidth="0.6" />
        </pattern>
        <marker id={`${uid}arr`} markerWidth="7" markerHeight="7" refX="6" refY="3.5" orient="auto" markerUnits="userSpaceOnUse">
          <path d="M0 0L7 3.5L0 7Z" fill="#2a3d45" />
        </marker>
      </defs>
      <rect width={vbW} height={vbH} fill="#f4f1e6" />
      <rect width={vbW} height={vbH} fill={`url(#${uid}grid)`} />

      {/* Grain / warp-weft orientation compass in top-left corner */}
      <g transform="translate(14 14)">
        <line x1="12" y1="36" x2="12" y2="8" stroke="#2a3d45" strokeWidth="1.5" markerEnd={`url(#${uid}arr)`} />
        <line x1="12" y1="36" x2="40" y2="36" stroke="#2a3d45" strokeWidth="1.5" markerEnd={`url(#${uid}arr)`} />
        <text x="12" y="5" textAnchor="middle" fill="#2a3d45" fontSize="8" fontWeight="700" fontFamily="var(--mono)">
          WARP
        </text>
        <text x="45" y="38.5" fill="#2a3d45" fontSize="8" fontWeight="700" fontFamily="var(--mono)">
          WEFT
        </text>
      </g>

      {/* Original cut & sewing outline (dashed slate) */}
      <g className="ws-orig-group" transform={`translate(${origTx.toFixed(2)} ${origTy.toFixed(2)})`}>
        {orig.cut.map((d, i) => (
          <path
            key={`oc-${i}`}
            d={d}
            fill="#28424d"
            fillOpacity="0.06"
            stroke="#28424d"
            strokeWidth="1.5"
            strokeDasharray="5 4"
            vectorEffect="non-scaling-stroke"
          />
        ))}
        {orig.sew.map((d, i) => (
          <path
            key={`os-${i}`}
            d={d}
            fill="none"
            stroke="#516872"
            strokeWidth="0.9"
            strokeDasharray="3 3"
            vectorEffect="non-scaling-stroke"
          />
        ))}
      </g>

      {/* Revised cut & sewing outline (solid terracotta for treat, deep teal for compensate) */}
      <g className="ws-rev-group" transform={`translate(${revTx.toFixed(2)} ${revTy.toFixed(2)})`}>
        {rev.cut.map((d, i) => (
          <path
            key={`rc-${i}`}
            d={d}
            fill={accent}
            fillOpacity="0.11"
            stroke={accent}
            strokeWidth="2.1"
            vectorEffect="non-scaling-stroke"
          />
        ))}
        {rev.sew.map((d, i) => (
          <path
            key={`rs-${i}`}
            d={d}
            fill="none"
            stroke={accent}
            strokeWidth="1.15"
            strokeDasharray="6 3"
            vectorEffect="non-scaling-stroke"
          />
        ))}
        {rev.marks.map((d, i) => (
          <path
            key={`rm-${i}`}
            d={d}
            fill="none"
            stroke="#2a3d45"
            strokeWidth="1"
            strokeOpacity="0.55"
            vectorEffect="non-scaling-stroke"
          />
        ))}
      </g>
    </svg>
  );
}

export default function WashShrinkLab() {
  const [garment, setGarment] = useState<"zouave_bolero" | "zouave_trousers">("zouave_bolero");
  const [transitionKey, setTransitionKey] = useState<"denim1" | "denim5">("denim1");
  const [mode, setMode] = useState<"treat" | "compensate">("treat");
  const [mismatchMaterial, setMismatchMaterial] = useState(false);
  const [selectedPiece, setSelectedPiece] = useState("back");

  const [result, setResult] = useState<WashResult | null>(null);
  const [sourceTag, setSourceTag] = useState<"starter" | "live" | "loading" | "refusal" | "unavailable">("loading");
  const [error, setError] = useState("");
  const workerRef = useRef<Worker | null>(null);
  const serial = useRef(0);

  useEffect(() => () => {
    workerRef.current?.terminate();
    workerRef.current = null;
  }, []);

  useEffect(() => {
    const id = ++serial.current;
    const controller = new AbortController();
    setError("");
    setSourceTag("loading");

    const isDefault =
      garment === "zouave_bolero" &&
      transitionKey === "denim1" &&
      mode === "treat" &&
      !mismatchMaterial;

    const runWorker = () => {
      try {
        workerRef.current ??= new Worker("/labs/wash-worker.mjs", { type: "module" });
        workerRef.current.onmessage = ({ data }) => {
          if (controller.signal.aborted || data.id !== id) return;
          if (data.loadError) {
            setResult(null);
            setError(data.loadError);
            setSourceTag("unavailable");
          } else if (data.error) {
            setResult(null);
            setError(data.error);
            setSourceTag("refusal");
          } else {
            setResult(data.result);
            setError("");
            setSourceTag("live");
          }
        };
        workerRef.current.onerror = () => {
          if (controller.signal.aborted) return;
          workerRef.current?.terminate();
          workerRef.current = null;
          setResult(null);
          setError("The local Rust WebAssembly treatment engine could not load.");
          setSourceTag("unavailable");
        };
        workerRef.current.postMessage({
          id,
          params: { garment, transitionKey, mode, mismatchMaterial },
        });
      } catch (err) {
        if (!controller.signal.aborted) {
          setResult(null);
          setError(err instanceof Error ? err.message : String(err));
          setSourceTag("unavailable");
        }
      }
    };

    if (isDefault) {
      fetch("/labs/wash-starter.json", { signal: controller.signal })
        .then(async res => {
          if (!res.ok) throw new Error("starter-missing");
          const data: WashResult = await res.json();
          if (controller.signal.aborted) return;
          setResult(data);
          setSourceTag("starter");
          // Also warm the live worker so PDF/SVG bytes are downloadable immediately on interaction
          runWorker();
        })
        .catch(() => {
          if (!controller.signal.aborted) runWorker();
        });
    } else {
      runWorker();
    }

    return () => controller.abort();
  }, [garment, transitionKey, mode, mismatchMaterial]);

  const origPieces = useMemo(() => (result ? extractPieces(result.originalSvg) : []), [result]);
  const revPieces = useMemo(() => (result ? extractPieces(result.revisedSvg) : []), [result]);

  useEffect(() => {
    if (origPieces.length && !origPieces.some(p => p.id === selectedPiece)) {
      setSelectedPiece(origPieces[0].id);
    }
  }, [origPieces, selectedPiece]);

  const downloads = useMemo(() => {
    if (!result) return null;
    const svgPart: BlobPart = result.files?.svg?.bytes
      ? new Uint8Array(result.files.svg.bytes)
      : result.revisedSvg;
    const svgUrl = URL.createObjectURL(new Blob([svgPart], { type: "image/svg+xml" }));
    const pdfUrl = result.files?.pdf?.bytes
      ? URL.createObjectURL(new Blob([new Uint8Array(result.files.pdf.bytes)], { type: "application/pdf" }))
      : null;
    const reqUrl = URL.createObjectURL(new Blob([result.frozenRequestJson], { type: "application/json" }));
    return {
      svgUrl,
      svgName: result.files?.svg?.name ?? `${result.config.garment}-${result.mode}_master_mm.svg`,
      pdfUrl,
      pdfName: result.files?.pdf?.name ?? `${result.config.garment}-${result.mode}_A4.pdf`,
      reqUrl,
    };
  }, [result]);

  useEffect(() => {
    if (!downloads) return;
    return () => {
      URL.revokeObjectURL(downloads.svgUrl);
      if (downloads.pdfUrl) URL.revokeObjectURL(downloads.pdfUrl);
      URL.revokeObjectURL(downloads.reqUrl);
    };
  }, [downloads]);

  const activePreset = PRESETS.find(
    p =>
      p.garment === garment &&
      p.transitionKey === transitionKey &&
      p.mode === mode &&
      p.mismatchMaterial === mismatchMaterial,
  )?.id;

  const firstPiece = result?.pieces[0];
  const warpFactor = firstPiece?.scale_warp_weft[0];
  const weftFactor = firstPiece?.scale_warp_weft[1];
  const seamPairs = useMemo(() => {
    if (!result) return [];
    return result.seamsBefore.map((before, i) => ({
      before,
      after: result.seamsAfter[i],
    }));
  }, [result]);

  const cliSnippet = [
    "# Replay this exact wash shrinkage / compensation transition natively",
    `ftx materials treatment-pattern pattern.json ${transitionKey}-${mode}-request.json revised.json --json`,
  ].join("\n");

  return (
    <>
      <SiteHeader />
      <main id="main" className="ws-page">
        <Crumbs trail={[["Exhibition", "/"], ["Material studies", "/sections/materials/"], ["Wash, shrink & recut"]]} />
        <div className="wrap ws-wrap">
          <div className="ws-heading">
            <div>
              <p className="kicker">Source-qualified laundering · Live WASM</p>
              <h1>Wash, shrink &amp; <em>recut.</em></h1>
            </div>
            <p>
              Apply peer-reviewed ISO 6330 laundering shrinkage from Kanat &amp; Ozdil (2026) directly along each
              pattern piece&apos;s grainline, inspect post-wash seam contraction or compute an oversized pre-wash
              compensation cut, and see why unsupported material revisions are refused.
            </p>
          </div>

          <div className="ws-desk">
            <section className="ws-stage-card" aria-label="Original vs washed or compensated pattern overlay">
              <div className="ws-stage-head">
                <span>
                  {sourceTag === "starter"
                    ? "Verified native fixture · Kanat-Ozdil 2026"
                    : sourceTag === "loading"
                      ? "Evaluating in local Rust WASM…"
                      : sourceTag === "refusal"
                        ? "Native Rust WASM · Refused"
                        : sourceTag === "unavailable"
                          ? "Local Rust WASM unavailable"
                          : "Solved live in local Rust WASM"}
                  {" · "}
                  <strong>{mode === "compensate" ? "Pre-wash recut (compensate)" : "Post-wash (treat)"}</strong>
                </span>
                {origPieces.length > 0 && (
                  <label className="ws-piece-pick">
                    Panel
                    <select
                      aria-label="Inspect pattern panel"
                      value={selectedPiece}
                      onChange={e => setSelectedPiece(e.target.value)}
                    >
                      {origPieces.map(p => (
                        <option key={p.id} value={p.id}>
                          {p.name}
                        </option>
                      ))}
                    </select>
                  </label>
                )}
              </div>

              <div className="ws-stage-body">
                {result && origPieces.length > 0 ? (
                  <OverlayStage
                    original={origPieces}
                    revised={revPieces}
                    pieceId={selectedPiece}
                    mode={result.mode}
                  />
                ) : error ? (
                  <div className="ws-refusal-box" role="alert">
                    <h3>
                      {sourceTag === "unavailable"
                        ? "The local solver could not start"
                        : "Unsupported material / treatment pair refused"}
                    </h3>
                    <p>
                      <code>{error}</code>
                    </p>
                    {sourceTag === "refusal" && (
                      <p>
                        FrankenTextiles locks every treatment transition to the exact SHA-256 of its measured{" "}
                        <code>BEFORE</code> material snapshot. Cotton muslin cannot inherit denim wash shrinkage.
                      </p>
                    )}
                  </div>
                ) : (
                  <p className="ws-pending" role="status">
                    Applying dimensional treatment in local Rust WebAssembly…
                  </p>
                )}
              </div>

              {result && origPieces.length > 0 && (() => {
                const origP = origPieces.find(p => p.id === selectedPiece) ?? origPieces[0];
                const revP = revPieces.find(p => p.id === selectedPiece) ?? revPieces[0];
                const dw = revP.width - origP.width;
                const dh = revP.height - origP.height;
                return (
                  <div className="ws-dim-bar">
                    <span><strong>{origP.name}</strong></span>
                    <span>
                      Weft {origP.width.toFixed(1)} → {revP.width.toFixed(1)} mm ({dw >= 0 ? `+${dw.toFixed(1)}` : dw.toFixed(1)} mm)
                    </span>
                    <span>
                      Warp {origP.height.toFixed(1)} → {revP.height.toFixed(1)} mm ({dh >= 0 ? `+${dh.toFixed(1)}` : dh.toFixed(1)} mm)
                    </span>
                  </div>
                );
              })()}

              {result && (
                <div className="ws-legend">
                  <span><i className="sw-orig" /> Original drafted outline (1:1 mm)</span>
                  <span>
                    <i className={result.mode === "compensate" ? "sw-comp" : "sw-treat"} />{" "}
                    {result.mode === "compensate"
                      ? "Compensated pre-wash cut (shrinks back to original)"
                      : "Shrunk post-wash outline (after 5 cycles)"}
                  </span>
                </div>
              )}

              <dl className="ws-kpis" aria-live="polite">
                <div>
                  <dt>Warp scale (grain)</dt>
                  <dd>
                    {result && warpFactor !== undefined
                      ? `${((warpFactor - 1) * 100 >= 0 ? "+" : "")}${((warpFactor - 1) * 100).toFixed(2)}%`
                      : "—"}
                  </dd>
                  <small>
                    {result ? `Measured −${result.transition.warpShrinkPct.toFixed(2)}% contraction` : "Awaiting valid pair"}
                  </small>
                </div>
                <div>
                  <dt>Weft scale (crossgrain)</dt>
                  <dd>
                    {result && weftFactor !== undefined
                      ? `${((weftFactor - 1) * 100 >= 0 ? "+" : "")}${((weftFactor - 1) * 100).toFixed(2)}%`
                      : "—"}
                  </dd>
                  <small>
                    {result ? `Measured −${result.transition.weftShrinkPct.toFixed(2)}% contraction` : "Awaiting valid pair"}
                  </small>
                </div>
                <div>
                  <dt>First seam shift</dt>
                  <dd>
                    {result && seamPairs[0]
                      ? `${seamPairs[0].before.len_a_mm.toFixed(1)} → ${seamPairs[0].after.len_a_mm.toFixed(1)} mm`
                      : "—"}
                  </dd>
                  <small>
                    {result && seamPairs[0]
                      ? `${seamPairs[0].before.seam} · residual ${seamPairs[0].after.residual_mm.toFixed(1)} mm`
                      : "Seam walk"}
                  </small>
                </div>
                <div>
                  <dt>Flat pattern check</dt>
                  <dd>{result ? result.revisedStatus : sourceTag === "refusal" ? "REFUSED" : "—"}</dd>
                  <small>
                    {result
                      ? result.mode === "treat"
                        ? "Post-wash dimensions vs fit target"
                        : "Oversized cut before washing"
                      : "Source lock check"}
                  </small>
                </div>
              </dl>
            </section>

            <aside className="ws-console" aria-label="Treatment controls">
              <h2>Treatment &amp; mode</h2>
              <div className="ws-presets" role="group" aria-label="Measured wash presets">
                {PRESETS.map(p => (
                  <button
                    key={p.id}
                    type="button"
                    aria-pressed={activePreset === p.id}
                    data-refusal={p.mismatchMaterial ? "true" : undefined}
                    onClick={() => {
                      setGarment(p.garment as "zouave_bolero" | "zouave_trousers");
                      setTransitionKey(p.transitionKey);
                      setMode(p.mode);
                      setMismatchMaterial(p.mismatchMaterial);
                    }}
                  >
                    {p.label}
                  </button>
                ))}
              </div>

              <label className="ws-field">
                Garment cut
                <select
                  aria-label="Garment cut"
                  value={garment}
                  onChange={e => setGarment(e.target.value as "zouave_bolero" | "zouave_trousers")}
                >
                  <option value="zouave_bolero">Zouave bolero (92 cm chart)</option>
                  <option value="zouave_trousers">Gathered trousers (92 cm chart)</option>
                </select>
              </label>

              <label className="ws-field">
                Measured fabric &amp; wash transition
                <select
                  aria-label="Measured fabric and wash transition"
                  value={mismatchMaterial ? "muslin_mismatch" : transitionKey}
                  onChange={e => {
                    if (e.target.value === "muslin_mismatch") {
                      setTransitionKey("denim1");
                      setMismatchMaterial(true);
                    } else {
                      setTransitionKey(e.target.value as "denim1" | "denim5");
                      setMismatchMaterial(false);
                    }
                  }}
                >
                  <option value="denim1">Kanat-Ozdil 2026 Denim 1 · 1.50% elastane (−4.83% warp / −5.50% weft)</option>
                  <option value="denim5">Kanat-Ozdil 2026 Denim 5 · 0.50% elastane (−1.60% warp / −3.00% weft)</option>
                  <option value="muslin_mismatch">Cotton muslin 3.5 oz + Denim 1 wash (unsupported — refuses)</option>
                </select>
              </label>

              <fieldset className="ws-mode-set">
                <legend>Pattern geometry direction</legend>
                <div className="ws-mode-row" role="group" aria-label="Treatment geometry mode">
                  <button
                    type="button"
                    aria-pressed={mode === "treat"}
                    onClick={() => setMode("treat")}
                  >
                    Shrink (treat)
                  </button>
                  <button
                    type="button"
                    aria-pressed={mode === "compensate"}
                    onClick={() => setMode("compensate")}
                  >
                    Recut oversized (compensate)
                  </button>
                </div>
              </fieldset>

              {result && downloads && (
                <div className="ws-downloads">
                  <a className="ws-dl-primary" href={downloads.svgUrl} download={downloads.svgName}>
                    Download revised master SVG ↓
                  </a>
                  {downloads.pdfUrl && (
                    <a className="ws-dl-secondary" href={downloads.pdfUrl} download={downloads.pdfName}>
                      Download revised A4 PDF ↓
                    </a>
                  )}
                  <a
                    className="ws-dl-secondary"
                    href={downloads.reqUrl}
                    download={`${transitionKey}-${mode}-request.json`}
                  >
                    Frozen treatment request JSON ↓
                  </a>
                </div>
              )}

              <p className="ws-meta-note">
                Only measured transitions sharing the exact SHA-256 of the piece&apos;s bound{" "}
                <code>BEFORE</code> material are admitted. Units remain millimetres along each piece&apos;s authored
                grainline.
              </p>
            </aside>
          </div>

          <div className="ws-details">
            <section className="ws-table-card" aria-label="Seam walk before and after laundering">
              <h2>Seam walk &amp; residuals</h2>
              {result ? (
                <div className="ws-table-scroll">
                  <table className="ws-table">
                    <thead>
                      <tr>
                        <th>Seam</th>
                        <th>Before (mm)</th>
                        <th>{result.mode === "compensate" ? "Compensated (mm)" : "After 5 washes (mm)"}</th>
                        <th>Δ Length</th>
                        <th>Residual</th>
                        <th>Status</th>
                      </tr>
                    </thead>
                    <tbody>
                      {seamPairs.map(({ before, after }) => {
                        const diff = after.len_a_mm - before.len_a_mm;
                        return (
                          <tr key={before.seam}>
                            <td>
                              <strong>{before.seam}</strong>
                              <br />
                              <span>{before.a} ↔ {before.b}</span>
                            </td>
                            <td className="num">{before.len_a_mm.toFixed(1)}</td>
                            <td className="num">{after.len_a_mm.toFixed(1)}</td>
                            <td className="num">{diff >= 0 ? `+${diff.toFixed(1)}` : diff.toFixed(1)} mm</td>
                            <td className="num">{after.residual_mm.toFixed(1)} mm</td>
                            <td>
                              <span className="ws-pill" data-ok={after.status === "PASS"}>
                                {after.status}
                              </span>
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </div>
              ) : (
                <p>
                  {sourceTag === "loading"
                    ? "Computing treated seam walks in local Rust WebAssembly…"
                    : sourceTag === "unavailable"
                      ? "Seam table unavailable while local Rust WebAssembly cannot load."
                      : "Seam table unavailable because the requested material/treatment pair was refused."}
                </p>
              )}
              <div style={{ marginTop: 18 }}>
                <CodeBlock title="Reproduce identically on the CLI" code={cliSnippet} />
              </div>
            </section>

            <aside className="ws-scope" aria-label="Published source and scope boundary">
              <h2>Published source &amp; scope</h2>
              {result ? (
                <>
                  <p>
                    <strong>Specimen:</strong> <code>{result.transition.specimen}</code> · {result.transition.cycles}{" "}
                    washing cycles.
                  </p>
                  <p>
                    <strong>Process:</strong> {result.transition.process}.
                  </p>
                  <p>
                    <strong>Measurement:</strong> {result.transition.method}.
                  </p>
                  {result.transition.sourceRef && (
                    <p>
                      <a href={result.transition.sourceRef} target="_blank" rel="noreferrer">
                        Kanat &amp; Ozdil (2026), Materials 19(15), 3264 (CC BY) ↗
                      </a>
                    </p>
                  )}
                  <p className="ws-scope-text">{result.scope}</p>
                  <p className="ws-sha">
                    <code>source_pattern_sha256</code>: {result.sourcePatternSha256}
                    <br />
                    <code>pattern_sha256</code>: {result.patternSha256}
                  </p>
                </>
              ) : (
                <p>
                  Every admitted wash transition requires a published or measured before/after material pair whose{" "}
                  <code>BEFORE</code> SHA-256 matches the pattern&apos;s bound material.
                </p>
              )}
              <div className="ws-links">
                <a href={workbench("materials")}>Open Material Workbench ↗</a>
                <a href="/experiments/thermal/">Thermal sandwich →</a>
                <a href="/sections/cut/?mode=remnant">Cut from remnant →</a>
              </div>
            </aside>
          </div>
        </div>
      </main>
      <SiteFooter />
    </>
  );
}
