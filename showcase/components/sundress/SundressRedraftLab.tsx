"use client";

import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import SiteHeader from "../site/SiteHeader";
import SiteFooter from "../site/SiteFooter";
import Crumbs from "../site/Crumbs";
import { SUNDRESS_WEARERS } from "../../public/labs/sundress-redraft.mjs";
import "./sundress-redraft.css";

type CutKind = "dress" | "light";

type FitPoint = {
  id: string;
  label: string;
  body_mm: number | null;
  finished_mm: number | null;
  ease_mm: number | null;
  residual_mm: number | null;
  target_mm: [number, number, number];
  status: string;
};

type FileBundle = {
  bytes: Uint8Array;
  name: string;
  byteLength: number;
  fingerprint: string;
};

type StageReport = {
  status: string;
  points: FitPoint[];
  passedSeams: number;
  totalSeams: number;
  checks: string | null;
  patternJson: string | null;
  files: { pdf?: FileBundle; svg?: FileBundle };
};

type PanelDiff = {
  id: string;
  name: string;
  firstPath: string;
  secondPath: string;
  bounds: [number, number, number, number];
  firstWidthMm: number;
  secondWidthMm: number;
  deltaWidthMm: number;
  firstBustWidthMm: number | null;
  secondBustWidthMm: number | null;
  deltaBustWidthMm: number | null;
  firstWaistWidthMm: number | null;
  secondWaistWidthMm: number | null;
  deltaWaistWidthMm: number | null;
  firstAreaCm2: number;
  secondAreaCm2: number;
  deltaAreaCm2: number;
  maxVertexShiftMm: number;
  movedVertices: { id: string; from: [number, number]; to: [number, number]; shiftMm: number }[];
};

type SeamDiff = {
  seam: string;
  a: string;
  b: string;
  firstLenAMm: number;
  firstLenBMm: number;
  secondLenAMm: number;
  secondLenBMm: number;
  deltaAMm: number;
  deltaBMm: number;
  firstStatus: string;
  secondStatus: string;
};

type RedraftResult = {
  request: {
    cut: CutKind;
    chestCm: number;
    waistCm: number;
    reqBustEaseMm: number;
    reqWaistEaseMm: number;
  };
  gatherRatio: number;
  unchanged: boolean;
  outlinesIdentical: boolean;
  deltaBustMm: number;
  deltaWaistMm: number;
  maxPanelShiftMm: number;
  maxSeamDeltaMm: number;
  appliedOpsCount: number;
  history: string[];
  panels: PanelDiff[];
  seamDeltas: SeamDiff[];
  first: StageReport;
  second: StageReport;
};

type DownloadUrls = {
  firstPdf?: { url: string; name: string; fp: string };
  firstSvg?: { url: string; name: string; fp: string };
  secondPdf?: { url: string; name: string; fp: string };
  secondSvg?: { url: string; name: string; fp: string };
  secondJson?: { url: string; name: string };
};

function requestKey(cut: CutKind, chestCm: number, waistCm: number, reqBustEaseMm: number, reqWaistEaseMm: number) {
  return `${cut}|${chestCm.toFixed(1)}|${waistCm.toFixed(1)}|${reqBustEaseMm}|${reqWaistEaseMm}`;
}

function signed(n: number, digits = 1) {
  const s = n.toFixed(digits);
  return n > 0 ? `+${s}` : s;
}

function PinnedSilhouetteBoard({
  panels,
  selectedId,
  pinFirst,
}: {
  panels: PanelDiff[];
  selectedId: string;
  pinFirst: boolean;
}) {
  const uid = useId();
  const active = selectedId === "all" ? panels : panels.filter(p => p.id === selectedId);

  const layout = useMemo(() => {
    if (active.length === 1) {
      const p = active[0];
      const padX = 48;
      const padTop = 64;
      const padBottom = 56;
      const w = p.bounds[2] - p.bounds[0] + padX * 2;
      const h = p.bounds[3] - p.bounds[1] + padTop + padBottom;
      return {
        viewBox: `${(p.bounds[0] - padX).toFixed(1)} ${(p.bounds[1] - padTop).toFixed(1)} ${w.toFixed(1)} ${h.toFixed(1)}`,
        items: [{ panel: p, tx: 0, ty: 0 }],
        barX: p.bounds[0],
        barY: p.bounds[3] + 34,
      };
    }
    // Group bodices on row 0 and skirts on row 1, computed dynamically from exact panel bounds.
    const byId = new Map(active.map(p => [p.id, p]));
    const row0 = [byId.get("bodice_front"), byId.get("bodice_back")].filter((p): p is PanelDiff => Boolean(p));
    const row1 = [byId.get("skirt_front"), byId.get("skirt_back")].filter((p): p is PanelDiff => Boolean(p));
    const rows = [row0, row1].filter(r => r.length > 0);
    const colGap = 120;
    const rowGap = 125;
    const maxColW = Math.max(...active.map(p => p.bounds[2] - p.bounds[0]));
    const items: { panel: PanelDiff; tx: number; ty: number }[] = [];
    let cursorY = 0;
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    for (const row of rows) {
      const rowH = Math.max(...row.map(p => p.bounds[3] - p.bounds[1]));
      row.forEach((panel, col) => {
        const tx = col * (maxColW + colGap);
        const ty = cursorY - panel.bounds[1];
        items.push({ panel, tx, ty });
        minX = Math.min(minX, tx + panel.bounds[0]);
        maxX = Math.max(maxX, tx + panel.bounds[2]);
        minY = Math.min(minY, ty + panel.bounds[1]);
        maxY = Math.max(maxY, ty + panel.bounds[3]);
      });
      cursorY += rowH + rowGap;
    }
    const padX = 76;
    const padTop = 92;
    const padBottom = 86;
    return {
      viewBox: `${(minX - padX).toFixed(1)} ${(minY - padTop).toFixed(1)} ${(maxX - minX + padX * 2).toFixed(1)} ${(maxY - minY + padTop + padBottom).toFixed(1)}`,
      items,
      barX: minX,
      barY: maxY + 48,
    };
  }, [active]);

  const strokeScale = active.length === 1 ? 1 : 2.1;

  return (
    <svg
      className="sr-svg"
      role="img"
      aria-label={
        active.length === 1
          ? `Pinned 1:1 millimetre outline comparison of ${active[0].name}: fixed first draft versus checked second cut`
          : "Pinned 1:1 millimetre outline comparison of all four sundress panels: fixed first draft versus checked second cut"
      }
      viewBox={layout.viewBox}
    >
      <defs>
        <pattern id={`${uid}grid`} width="50" height="50" patternUnits="userSpaceOnUse">
          <path d="M 50 0 H 0 V 50" fill="none" stroke="#d5cfc0" strokeWidth={0.75 * strokeScale} opacity="0.65" />
        </pattern>
      </defs>
      <rect x="-2400" y="-2400" width="6400" height="6400" fill="#f5f1e4" />
      <rect x="-2400" y="-2400" width="6400" height="6400" fill={`url(#${uid}grid)`} />

      {layout.items.map(({ panel, tx, ty }) => (
        <g key={panel.id} transform={`translate(${tx}, ${ty})`} data-panel-outline={panel.id}>
          {/* Waist centerline axis (0, 0) */}
          <line
            x1={0}
            y1={panel.bounds[1] - 12}
            x2={0}
            y2={panel.bounds[3] + 12}
            stroke="#9a9889"
            strokeWidth={0.9 * strokeScale}
            strokeDasharray={`${5 * strokeScale} ${4 * strokeScale}`}
          />

          {/* Second draft (checked redraft) */}
          <path
            className="sr-second-path"
            d={panel.secondPath}
            fill="#b64827"
            fillOpacity="0.14"
            stroke="#a73c1c"
            strokeWidth={2.4 * strokeScale}
            strokeLinejoin="round"
          />

          {/* Pinned first draft silhouette (always preserved untouched) */}
          {pinFirst && (
            <path
              className="sr-first-path"
              d={panel.firstPath}
              fill="#195c52"
              fillOpacity="0.08"
              stroke="#14584e"
              strokeWidth={2.1 * strokeScale}
              strokeDasharray={`${7 * strokeScale} ${4 * strokeScale}`}
              strokeLinejoin="round"
            />
          )}

          {/* Highlight moved vertices */}
          {panel.movedVertices.map(v => (
            <g key={v.id}>
              <line
                x1={v.from[0]}
                y1={-v.from[1]}
                x2={v.to[0]}
                y2={-v.to[1]}
                stroke="#1c2c31"
                strokeWidth={1.2 * strokeScale}
              />
              <circle cx={v.to[0]} cy={-v.to[1]} r={3.6 * strokeScale} fill="#a73c1c" />
            </g>
          ))}

          {/* Label */}
          <text
            x={0}
            y={panel.bounds[1] - 16}
            textAnchor="middle"
            fill="#1f3035"
            fontSize={14 * strokeScale}
            fontWeight="700"
            fontFamily="var(--mono)"
          >
            {panel.name.toUpperCase()} · {panel.maxVertexShiftMm > 0 ? `MAX SHIFT ${panel.maxVertexShiftMm.toFixed(1)} MM` : "MATCHED 0.0 MM"}
          </text>
        </g>
      ))}

      {/* 100 mm scale bar */}
      <g>
        <line
          x1={layout.barX}
          y1={layout.barY}
          x2={layout.barX + 100}
          y2={layout.barY}
          stroke="#1d2d33"
          strokeWidth={2.2 * strokeScale}
        />
        <text
          x={layout.barX + 50}
          y={layout.barY - 6 * strokeScale}
          textAnchor="middle"
          fill="#1d2d33"
          fontSize={11 * strokeScale}
          fontWeight="600"
          fontFamily="var(--mono)"
        >
          100 mm
        </text>
      </g>
    </svg>
  );
}

export default function SundressRedraftLab() {
  const [cut, setCut] = useState<CutKind>("dress");
  const [chestCm, setChestCm] = useState(93.5);
  const [waistCm, setWaistCm] = useState(77.0);
  const [reqBustEaseMm, setReqBustEaseMm] = useState(40);
  const [reqWaistEaseMm, setReqWaistEaseMm] = useState(60);
  const [selectedPanel, setSelectedPanel] = useState<string>("bodice_front");
  const [pinFirst, setPinFirst] = useState(true);

  const [result, setResult] = useState<RedraftResult | null>(null);
  const [appliedKey, setAppliedKey] = useState<string>("");
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState("");
  const [urls, setUrls] = useState<DownloadUrls>({});

  const workerRef = useRef<Worker | null>(null);
  const reqIdRef = useRef(0);
  const urlsRef = useRef<DownloadUrls>({});

  const revokeUrls = useCallback((bag: DownloadUrls) => {
    for (const item of Object.values(bag)) {
      if (item?.url) URL.revokeObjectURL(item.url);
    }
  }, []);

  const currentKey = requestKey(cut, chestCm, waistCm, reqBustEaseMm, reqWaistEaseMm);
  const isStale = !result || busy || Boolean(error) || appliedKey !== currentKey;

  const dispatchRedraft = useCallback(
    (params: { cut: CutKind; chestCm: number; waistCm: number; reqBustEaseMm: number; reqWaistEaseMm: number }) => {
      const worker = workerRef.current;
      if (!worker) return;
      const id = ++reqIdRef.current;
      setBusy(true);
      setError("");
      worker.postMessage({ id, ...params });
    },
    [],
  );

  useEffect(() => {
    const worker = new Worker(new URL("../../public/labs/sundress-redraft.mjs", import.meta.url), {
      type: "module",
    });
    workerRef.current = worker;

    worker.onmessage = ({ data }: MessageEvent<{ id: number; result?: RedraftResult; error?: string }>) => {
      if (data.id !== reqIdRef.current) return;
      if (data.error) {
        setError(data.error);
        setBusy(false);
        return;
      }
      if (!data.result) return;
      const next = data.result;
      const nextUrls: DownloadUrls = {};
      if (next.first.files.pdf) {
        nextUrls.firstPdf = {
          url: URL.createObjectURL(new Blob([new Uint8Array(next.first.files.pdf.bytes)], { type: "application/pdf" })),
          name: next.first.files.pdf.name,
          fp: next.first.files.pdf.fingerprint,
        };
      }
      if (next.first.files.svg) {
        nextUrls.firstSvg = {
          url: URL.createObjectURL(new Blob([new Uint8Array(next.first.files.svg.bytes)], { type: "image/svg+xml" })),
          name: next.first.files.svg.name,
          fp: next.first.files.svg.fingerprint,
        };
      }
      if (next.second.files.pdf) {
        nextUrls.secondPdf = {
          url: URL.createObjectURL(new Blob([new Uint8Array(next.second.files.pdf.bytes)], { type: "application/pdf" })),
          name: next.second.files.pdf.name,
          fp: next.second.files.pdf.fingerprint,
        };
      }
      if (next.second.files.svg) {
        nextUrls.secondSvg = {
          url: URL.createObjectURL(new Blob([new Uint8Array(next.second.files.svg.bytes)], { type: "image/svg+xml" })),
          name: next.second.files.svg.name,
          fp: next.second.files.svg.fingerprint,
        };
      }
      if (next.second.patternJson) {
        nextUrls.secondJson = {
          url: URL.createObjectURL(new Blob([next.second.patternJson], { type: "application/json" })),
          name: "SUNDRESS_REDRAFT_pattern.json",
        };
      }
      revokeUrls(urlsRef.current);
      urlsRef.current = nextUrls;
      setUrls(nextUrls);
      setResult(next);
      setAppliedKey(
        requestKey(
          next.request.cut,
          next.request.chestCm,
          next.request.waistCm,
          next.request.reqBustEaseMm,
          next.request.reqWaistEaseMm,
        ),
      );
      setBusy(false);
    };

    worker.onerror = ev => {
      setError(ev.message || "Redraft worker failed.");
      setBusy(false);
    };

    // Initial checked redraft on the "Close (93.5 / 77.0 cm)" warning case.
    dispatchRedraft({ cut: "dress", chestCm: 93.5, waistCm: 77.0, reqBustEaseMm: 40, reqWaistEaseMm: 60 });

    return () => {
      worker.terminate();
      workerRef.current = null;
      revokeUrls(urlsRef.current);
      urlsRef.current = {};
    };
  }, [dispatchRedraft, revokeUrls]);

  const firstBust = result?.first.points.find(p => p.id === "bust");
  const firstWaist = result?.first.points.find(p => p.id === "waist");
  const secondBust = result?.second.points.find(p => p.id === "bust");
  const secondWaist = result?.second.points.find(p => p.id === "waist");

  return (
    <>
      <SiteHeader />
      <main id="main" className="sr-page">
        <Crumbs trail={[["Exhibition", "/"], ["The garden sundress", "/wardrobe/sundress/"], ["Ease redraft"]]} />
        <div className="wrap sr-wrap">
          <div className="sr-heading">
            <div>
              <p className="kicker">Flat ease check · Native Rust WebAssembly alteration</p>
              <h1>Redraft after an ease <em>warning.</em></h1>
            </div>
            <p>
              When the fixed Garden Sundress reports insufficient bust or waist ease, request a second checked pattern
              computed live with native <code>alter_pattern</code> and flat validation. Pin both silhouettes at 1:1
              scale, inspect every recalculated seam length, and keep the original first draft untouched.
            </p>
          </div>

          {/* Controls bar */}
          <section className="sr-controls" aria-label="Wearer measurements and ease targets">
            <div className="sr-presets">
              <div className="sr-group" role="group" aria-label="Choose synthetic wearer preset">
                {SUNDRESS_WEARERS.map(w => (
                  <button
                    key={w.id}
                    type="button"
                    data-wearer={w.id}
                    aria-pressed={chestCm === w.chestCm && waistCm === w.waistCm}
                    onClick={() => {
                      setChestCm(w.chestCm);
                      setWaistCm(w.waistCm);
                    }}
                  >
                    {w.label}
                  </button>
                ))}
              </div>
              <div className="sr-group" role="group" aria-label="Choose sundress skirt cut">
                <button type="button" aria-pressed={cut === "dress"} onClick={() => setCut("dress")}>
                  Gathered skirt (1.30×)
                </button>
                <button type="button" aria-pressed={cut === "light"} onClick={() => setCut("light")}>
                  Lightly eased (1.05×)
                </button>
              </div>
            </div>

            <div className="sr-sliders">
              <label>
                <span>Wearer bust girth</span>
                <output>{chestCm.toFixed(1)} cm</output>
                <input
                  type="range"
                  min="84"
                  max="104"
                  step="0.5"
                  value={chestCm}
                  aria-label="Wearer bust girth in centimetres"
                  onChange={e => setChestCm(Number(e.target.value))}
                />
              </label>
              <label>
                <span>Wearer waist girth</span>
                <output>{waistCm.toFixed(1)} cm</output>
                <input
                  type="range"
                  min="64"
                  max="90"
                  step="0.5"
                  value={waistCm}
                  aria-label="Wearer waist girth in centimetres"
                  onChange={e => setWaistCm(Number(e.target.value))}
                />
              </label>
              <label>
                <span>Required bust ease</span>
                <output>+{reqBustEaseMm} mm</output>
                <input
                  type="range"
                  min="20"
                  max="80"
                  step="5"
                  value={reqBustEaseMm}
                  aria-label="Required bust ease in millimetres"
                  onChange={e => setReqBustEaseMm(Number(e.target.value))}
                />
              </label>
              <label>
                <span>Required waist ease</span>
                <output>+{reqWaistEaseMm} mm</output>
                <input
                  type="range"
                  min="25"
                  max="90"
                  step="5"
                  value={reqWaistEaseMm}
                  aria-label="Required waist ease in millimetres"
                  onChange={e => setReqWaistEaseMm(Number(e.target.value))}
                />
              </label>
            </div>

            <div className="sr-action-row">
              <button
                type="button"
                className="sr-submit"
                disabled={busy}
                onClick={() => dispatchRedraft({ cut, chestCm, waistCm, reqBustEaseMm, reqWaistEaseMm })}
              >
                {busy ? "Checking & exporting in WASM…" : "Request checked second cut"}
              </button>
              {isStale && !busy && !error && (
                <p className="sr-stale-banner" role="status" data-stale="true">
                  Inputs changed since the last checked export — request a fresh redraft before downloading the second cut.
                </p>
              )}
              {error && (
                <p className="sr-error-banner" role="alert">
                  {error}
                </p>
              )}
              {!isStale && result && (
                <p className="sr-fresh-banner" role="status" data-stale="false">
                  {result.outlinesIdentical
                    ? "Unchanged request — second cut matches the fixed first draft with 0.0 mm shift."
                    : `Checked second cut ready · bust ${signed(result.deltaBustMm, 0)} mm, waist ${signed(result.deltaWaistMm, 0)} mm · ${result.second.checks}`}
                </p>
              )}
            </div>
          </section>

          {/* Main stage: Pinned 1:1 SVG Silhouette Comparison + Verdicts & Downloads */}
          <div className="sr-stage">
            <section className="sr-board-card" aria-label="Pinned silhouette outline comparison">
              <div className="sr-card-head">
                <div className="sr-piece-tabs" role="group" aria-label="Inspect pattern piece">
                  {(
                    [
                      ["bodice_front", "Front bodice"],
                      ["bodice_back", "Back bodice"],
                      ["skirt_front", "Front skirt"],
                      ["skirt_back", "Back skirt"],
                      ["all", "All 4 panels"],
                    ] as const
                  ).map(([id, label]) => (
                    <button
                      key={id}
                      type="button"
                      aria-pressed={selectedPanel === id}
                      onClick={() => setSelectedPanel(id)}
                    >
                      {label}
                    </button>
                  ))}
                </div>
                <label className="sr-pin-toggle">
                  <input type="checkbox" checked={pinFirst} onChange={e => setPinFirst(e.target.checked)} />
                  Pin first draft silhouette
                </label>
              </div>
              <div className="sr-svg-wrap" data-stale={isStale ? "true" : "false"}>
                {result ? (
                  <PinnedSilhouetteBoard panels={result.panels} selectedId={selectedPanel} pinFirst={pinFirst} />
                ) : (
                  <p className="sr-loading" role="status">
                    {error || "Running native flat validation and alteration in WebAssembly…"}
                  </p>
                )}
              </div>
              <div className="sr-legend">
                <span>
                  <i className="sw-first" /> Pinned first draft (fixed <code>GARDEN_SUNDRESS</code>)
                </span>
                <span>
                  <i className="sw-second" /> Checked second cut (<code>SUNDRESS_REDRAFT</code>)
                </span>
                <span>
                  <i className="sw-dot" /> Shifted vertices (aligned at waist centerline)
                </span>
              </div>
            </section>

            <aside className="sr-side" aria-label="First draft versus second draft verdicts and exports">
              {/* First Draft (Always Preserved) */}
              <div className="sr-draft-card sr-first-card">
                <div className="sr-draft-head">
                  <h2>1. First draft (preserved)</h2>
                  <span
                    className="sr-badge"
                    data-status={result?.first.status ?? "PENDING"}
                    data-metric="first-status"
                  >
                    {result?.first.status ?? "Loading"}
                  </span>
                </div>
                {isStale && result && (
                  <p className="sr-stale-note">
                    Showing last check ({result.request.chestCm.toFixed(1)}/{result.request.waistCm.toFixed(1)} cm ·{" "}
                    {result.request.cut === "light" ? "1.05× eased" : "1.30× gathered"}). Request a redraft to re-check{" "}
                    {chestCm.toFixed(1)}/{waistCm.toFixed(1)} cm.
                  </p>
                )}
                <dl className="sr-fit-list">
                  <div>
                    <dt>Bust ease</dt>
                    <dd data-metric="first-bust">
                      {firstBust?.ease_mm != null
                        ? `${signed(firstBust.ease_mm)} mm (${firstBust.status})`
                        : "—"}
                    </dd>
                    <small>
                      Garment {firstBust?.finished_mm?.toFixed(0) ?? "—"} vs body {firstBust?.body_mm?.toFixed(0) ?? "—"} mm
                    </small>
                  </div>
                  <div>
                    <dt>Waist ease</dt>
                    <dd data-metric="first-waist">
                      {firstWaist?.ease_mm != null
                        ? `${signed(firstWaist.ease_mm)} mm (${firstWaist.status})`
                        : "—"}
                    </dd>
                    <small>
                      Garment {firstWaist?.finished_mm?.toFixed(0) ?? "—"} vs body {firstWaist?.body_mm?.toFixed(0) ?? "—"} mm
                    </small>
                  </div>
                </dl>
                <div className="sr-dl-row">
                  {urls.firstPdf && (
                    <a className="sr-dl-sec" href={urls.firstPdf.url} download={urls.firstPdf.name} data-download="first-pdf">
                      {urls.firstPdf.name} ↓
                    </a>
                  )}
                  {urls.firstSvg && (
                    <a className="sr-dl-sec" href={urls.firstSvg.url} download={urls.firstSvg.name} data-download="first-svg">
                      {urls.firstSvg.name} ↓
                    </a>
                  )}
                </div>
              </div>

              {/* Second Draft (Locked whenever Stale!) */}
              <div className="sr-draft-card sr-second-card" data-stale={isStale ? "true" : "false"}>
                <div className="sr-draft-head">
                  <h2>2. Checked second cut</h2>
                  <span
                    className="sr-badge"
                    data-status={isStale ? "STALE" : result?.second.status ?? "PENDING"}
                    data-metric="second-status"
                  >
                    {isStale ? "STALE · REDRAFT NEEDED" : result?.second.status ?? "Loading"}
                  </span>
                </div>
                <dl className="sr-fit-list">
                  <div>
                    <dt>Bust ease</dt>
                    <dd data-metric="second-bust">
                      {secondBust?.ease_mm != null
                        ? `${signed(secondBust.ease_mm)} mm (${secondBust.status})`
                        : "—"}
                    </dd>
                    <small>
                      Garment {secondBust?.finished_mm?.toFixed(0) ?? "—"} vs body {secondBust?.body_mm?.toFixed(0) ?? "—"} mm
                    </small>
                  </div>
                  <div>
                    <dt>Waist ease</dt>
                    <dd data-metric="second-waist">
                      {secondWaist?.ease_mm != null
                        ? `${signed(secondWaist.ease_mm)} mm (${secondWaist.status})`
                        : "—"}
                    </dd>
                    <small>
                      Garment {secondWaist?.finished_mm?.toFixed(0) ?? "—"} vs body {secondWaist?.body_mm?.toFixed(0) ?? "—"} mm
                    </small>
                  </div>
                </dl>
                <p className="sr-export-meta" data-metric="export-fp">
                  {isStale
                    ? "Downloads locked until the updated request finishes checking."
                    : `Seams ${result?.second.passedSeams}/${result?.second.totalSeams} PASS · ${result?.second.checks} · PDF #${urls.secondPdf?.fp ?? "—"}`}
                </p>
                {isStale ? (
                  <p className="sr-dl-locked" data-download-locked="true">
                    Stale result — click <strong>Request checked second cut</strong> to generate fresh exports.
                  </p>
                ) : (
                  <div className="sr-dl-row">
                    {urls.secondPdf && (
                      <a
                        className="sr-dl-pri"
                        href={urls.secondPdf.url}
                        download={urls.secondPdf.name}
                        data-download="second-pdf"
                        data-fingerprint={urls.secondPdf.fp}
                      >
                        {urls.secondPdf.name} ↓
                      </a>
                    )}
                    {urls.secondSvg && (
                      <a
                        className="sr-dl-sec"
                        href={urls.secondSvg.url}
                        download={urls.secondSvg.name}
                        data-download="second-svg"
                        data-fingerprint={urls.secondSvg.fp}
                      >
                        {urls.secondSvg.name} ↓
                      </a>
                    )}
                    {urls.secondJson && (
                      <a className="sr-dl-sec" href={urls.secondJson.url} download={urls.secondJson.name} data-download="second-json">
                        {urls.secondJson.name} ↓
                      </a>
                    )}
                  </div>
                )}
              </div>
            </aside>
          </div>

          {/* Per-panel movement & recalculated seam tables */}
          {result && (
            <div className="sr-tables">
              <section className="sr-table-card" aria-label="Per-panel geometry movement">
                <h2>How much each panel moved</h2>
                <p className="sr-sub">
                  Exact drafted mm coordinates before and after native <code>ftx.alter_pattern</code>. Max vertex shift:{" "}
                  <strong data-metric="max-panel-shift">{result.maxPanelShiftMm.toFixed(2)} mm</strong>.
                </p>
                <div className="sr-table-scroll">
                  <table>
                    <thead>
                      <tr>
                        <th>Panel</th>
                        <th>Bust width</th>
                        <th>Waist width</th>
                        <th>Max shift</th>
                        <th>Area Δ</th>
                      </tr>
                    </thead>
                    <tbody>
                      {result.panels.map(p => (
                        <tr key={p.id}>
                          <td>
                            <strong>{p.name}</strong>
                          </td>
                          <td>
                            {p.secondBustWidthMm !== null
                              ? `${p.firstBustWidthMm} → ${p.secondBustWidthMm} mm (${signed(p.deltaBustWidthMm ?? 0)} mm)`
                              : "—"}
                          </td>
                          <td>
                            {p.secondWaistWidthMm !== null
                              ? `${p.firstWaistWidthMm} → ${p.secondWaistWidthMm} mm (${signed(p.deltaWaistWidthMm ?? 0)} mm)`
                              : "—"}
                          </td>
                          <td>{p.maxVertexShiftMm.toFixed(2)} mm</td>
                          <td>{signed(p.deltaAreaCm2, 1)} cm²</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </section>

              <section className="sr-table-card" aria-label="Recalculated seam length comparison">
                <h2>Recalculated seam lengths ({result.second.passedSeams}/{result.second.totalSeams} PASS)</h2>
                <p className="sr-sub">
                  Every seam pair is re-walked by native flat validation. Skirt gather ratio stays locked at{" "}
                  <code>{result.gatherRatio.toFixed(2)}×</code>.
                </p>
                <div className="sr-table-scroll">
                  <table>
                    <thead>
                      <tr>
                        <th>Seam</th>
                        <th>First (A / B)</th>
                        <th>Second (A / B)</th>
                        <th>Δ A</th>
                        <th>Status</th>
                      </tr>
                    </thead>
                    <tbody>
                      {result.seamDeltas.map(s => (
                        <tr key={s.seam} data-seam-row={s.seam}>
                          <td>
                            <code>{s.seam}</code>
                          </td>
                          <td>
                            {s.firstLenAMm.toFixed(1)} / {s.firstLenBMm.toFixed(1)} mm
                          </td>
                          <td>
                            {s.secondLenAMm.toFixed(1)} / {s.secondLenBMm.toFixed(1)} mm
                          </td>
                          <td>{signed(s.deltaAMm)} mm</td>
                          <td>
                            <span className="sr-pass-pill">{s.secondStatus}</span>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </section>
            </div>
          )}

          {/* Native history & honest scope notice */}
          <section className="sr-note-card" aria-label="Alteration provenance and scope">
            <div>
              <h2>Native alteration trail &amp; honesty boundary</h2>
              <p>
                Flat pattern validation confirms geometric ease against the synthetic wearer’s bust and waist tapes,
                paired seam agreement, and printable A4/SVG export integrity. It does not simulate cloth gravity on a
                body or prove physical fit without a sewn muslin.
              </p>
              <p className="sr-back">
                <a href="/wardrobe/sundress/">← Return to The garden sundress</a>
              </p>
            </div>
            {result && (
              <details className="sr-history">
                <summary>
                  Native <code>alter_pattern</code> history ({result.appliedOpsCount} operations)
                </summary>
                {result.history.length > 0 ? (
                  <ol>
                    {result.history.map((line, idx) => (
                      <li key={idx}>{line}</li>
                    ))}
                  </ol>
                ) : (
                  <p>No alterations applied — fixed pattern already satisfies the requested bust and waist ease.</p>
                )}
              </details>
            )}
          </section>
        </div>
      </main>
      <SiteFooter />
    </>
  );
}
