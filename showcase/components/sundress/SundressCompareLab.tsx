"use client";

import dynamic from "next/dynamic";
import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import SiteHeader from "../site/SiteHeader";
import SiteFooter from "../site/SiteFooter";
import Crumbs from "../site/Crumbs";
import { publishedGeometry } from "../replayAsset";
import { COMMON_WORLD_SCALE_M_PER_UNIT, buildCompareMetrics } from "../../public/labs/sundress-compare.mjs";
import { COMPARE_CAMERAS, type CompareCameraPreset, type SharedCameraState } from "./SundressCompareScene";
import "./sundress-compare.css";

const SundressCompareScene = dynamic(() => import("./SundressCompareScene"), {
  ssr: false,
  loading: () => <div className="sc-loading">Loading native 3D drape…</div>,
});

type AcceptedState = {
  id: string;
  label: string;
  shortLabel: string;
  accepted: true;
  sourceBase: string;
  measures: { stature_mm: number; chest_mm: number; waist_mm: number; hip_mm: number };
  world: {
    soleZMm: number;
    headAboveSoleMm: number;
    hemAboveSoleMm: { front: number; back: number; mean: number };
    clothMinAboveSoleMm: number;
    clothMaxAboveSoleMm: number;
  };
  section: {
    heightMm: number;
    frontGapMm: number;
    backGapMm: number;
    body: [number, number, number, number][];
    cloth: [number, number, number, number][];
    hemRiseMm: { front: number; back: number };
  };
  summary: {
    settled: boolean;
    gating: string[];
    flags: string[];
    edgeStrainPercent: { min: number; max: number; p95: number };
    bodyOffsetMm: { target: number; achieved: number };
    finalResidualN: number;
    wallSeconds: number;
    sourcePc2Sha256: string;
    solverRevision?: string;
  };
};

type RefusedState = {
  id: string;
  label: string;
  shortLabel: string;
  accepted: false;
  sourceBase: null;
  measures: { stature_mm: number; chest_mm: number; waist_mm: number; hip_mm: number };
  refusal: {
    exit: number;
    code: string;
    message: string;
    repairs: string[];
    flatPoints: { id: string; label: string; finished_mm: number; body_mm: number; ease_mm: number; status: string }[];
  };
};

type CompareState = AcceptedState | RefusedState;

type CompareManifest = {
  schema: string;
  fixedPattern: {
    style: string;
    patternUrl: string;
    patternSha256: string;
    spacingMm: number;
    vertices: number;
    triangles: number;
    solverArguments: string[];
  };
  states: CompareState[];
};

const CAMERA_NAMES: CompareCameraPreset[] = ["Full length", "Hem elevation", "Waist profile", "Back"];
const COMPARE_MANIFEST = "/exhibits/sundress-compare/compare.json";
const NATIVE_REPLAYS = [
  { id: "reference", label: "Reference adult", directory: "reference-drape", source: "/exhibits/sundress", file: "dress-measures.json" },
  { id: "slender", label: "Petite adult", directory: "petite-drape", source: "/exhibits/sundress-compare", file: "slender-slender-measures.json" },
] as const;

function segmentsPath(segs: [number, number, number, number][]): string {
  if (!segs.length) return "";
  return segs.map(([a, b, c, d]) => `M${a.toFixed(1)},${b.toFixed(1)}L${c.toFixed(1)},${d.toFixed(1)}`).join("");
}

function WaistClearanceOverlaySvg({
  left,
  right,
}: {
  left: CompareState;
  right: CompareState;
}) {
  const uid = useId();
  const hasLeft = left.accepted;
  const hasRight = right.accepted;
  const allSegs = [
    ...(hasLeft ? [...left.section.body, ...left.section.cloth] : []),
    ...(hasRight ? [...right.section.body, ...right.section.cloth] : []),
  ];
  let minX = -180, maxX = 180, minY = -160, maxY = 160;
  if (allSegs.length) {
    minX = Infinity;
    maxX = -Infinity;
    minY = Infinity;
    maxY = -Infinity;
    for (const [a, b, c, d] of allSegs) {
      if (a < minX) minX = a;
      if (c < minX) minX = c;
      if (a > maxX) maxX = a;
      if (c > maxX) maxX = c;
      if (b < minY) minY = b;
      if (d < minY) minY = d;
      if (b > maxY) maxY = b;
      if (d > maxY) maxY = d;
    }
  }
  const padX = 36;
  const padTop = 34;
  const padBot = 36;
  const vx = minX - padX;
  const vy = minY - padTop;
  const vw = Math.max(300, maxX - minX + padX * 2);
  const vh = Math.max(240, maxY - minY + padTop + padBot);
  const cx = (minX + maxX) / 2;

  return (
    <svg
      className="sc-slice-svg"
      role="img"
      aria-label="1:1 mm horizontal waist cross-section overlay comparing geometric cloth-to-body clearance"
      viewBox={`${vx.toFixed(1)} ${vy.toFixed(1)} ${vw.toFixed(1)} ${vh.toFixed(1)}`}
    >
      <defs>
        <pattern id={`${uid}g`} width="40" height="40" patternUnits="userSpaceOnUse">
          <path d="M 40 0 H 0 V 40" fill="none" stroke="#d5cfc0" strokeWidth="0.7" opacity="0.7" />
        </pattern>
      </defs>
      <rect x={vx - 200} y={vy - 200} width={vw + 400} height={vh + 400} fill="#f5f1e4" />
      <rect x={vx - 200} y={vy - 200} width={vw + 400} height={vh + 400} fill={`url(#${uid}g)`} />

      {/* Centreline reference */}
      <line x1={cx} y1={minY - 8} x2={cx} y2={maxY + 8} stroke="#9aa49e" strokeWidth="1.2" strokeDasharray="4 4" />

      {hasLeft && (
        <g>
          <path d={segmentsPath(left.section.body)} fill="none" stroke="#537a76" strokeWidth="1.8" strokeDasharray="4 3" />
          <path d={segmentsPath(left.section.cloth)} fill="none" stroke="#0e6766" strokeWidth="2.8" />
        </g>
      )}
      {hasRight && (
        <g>
          <path d={segmentsPath(right.section.body)} fill="none" stroke="#a3624d" strokeWidth="1.8" strokeDasharray="4 3" />
          <path
            d={segmentsPath(right.section.cloth)}
            fill="none"
            stroke="#b84724"
            strokeWidth="2.8"
            strokeDasharray={left.id === right.id ? "6 4" : undefined}
          />
        </g>
      )}

      <text x={cx} y={minY - 14} textAnchor="middle" fill="#1b2d34" fontSize="11" fontWeight="700" fontFamily="var(--mono)">
        FRONT · {hasLeft ? `L GAP ${left.section.frontGapMm} MM` : "L REFUSED"} ·{" "}
        {hasRight ? `R GAP ${right.section.frontGapMm} MM` : "R REFUSED"}
      </text>
      <text x={cx} y={maxY + 22} textAnchor="middle" fill="#465558" fontSize="10.5" fontWeight="700" fontFamily="var(--mono)">
        BACK · {hasLeft ? `L GAP ${left.section.backGapMm} MM` : "L REFUSED"} ·{" "}
        {hasRight ? `R GAP ${right.section.backGapMm} MM` : "R REFUSED"} (1:1 MM WAIST SECTION)
      </text>
    </svg>
  );
}

function SideViewportCard({
  side,
  state,
  otherState,
  accentHex,
  cameraRef,
  onCameraDrag,
}: {
  side: "left" | "right";
  state: CompareState;
  otherState: CompareState;
  accentHex: string;
  cameraRef: React.MutableRefObject<SharedCameraState>;
  onCameraDrag: (dx: number, dy: number, wheelDelta?: number) => void;
}) {
  return (
    <section
      className="sc-side-card"
      data-side={side}
      data-state-id={state.id}
      data-accepted={state.accepted ? "true" : "false"}
      data-world-scale={COMMON_WORLD_SCALE_M_PER_UNIT}
      aria-label={`${side === "left" ? "Left" : "Right"} native drape viewport: ${state.label}`}
    >
      <div className="sc-side-head">
        <span>
          {side === "left" ? "LEFT FORM" : "RIGHT FORM"} · {state.shortLabel.toUpperCase()}
        </span>
        <span>
          {state.accepted
            ? `HEM ${state.world.hemAboveSoleMm.mean.toFixed(1)} MM · SCALE 1:1 M`
            : `REFUSED (EXIT ${state.refusal.exit})`}
        </span>
      </div>

      {state.accepted ? (
        <SundressCompareScene
          key={state.id}
          sourceBase={state.sourceBase}
          ownHemMm={state.world.hemAboveSoleMm.mean}
          otherHemMm={otherState.accepted ? otherState.world.hemAboveSoleMm.mean : null}
          accentHex={accentHex}
          cameraRef={cameraRef}
          onCameraDrag={onCameraDrag}
          label={`Synchronized 3D native drape of fixed sundress on ${state.label} at 1:1 metre common world scale`}
        />
      ) : (
        <div className="sc-refused-stage" data-refused-garment="true" role="alert">
          <span className="sc-refused-badge">NATIVE SOLVER REFUSAL · NO COMPLETED GARMENT</span>
          <h3>Refused: <code>{state.refusal.code}</code> (exit {state.refusal.exit})</h3>
          <p className="sc-refused-msg">{state.refusal.message}</p>
          <div className="sc-refused-points">
            <strong>Flat pattern ease on {state.measures.chest_mm}/{state.measures.waist_mm} mm body:</strong>
            <ul>
              {state.refusal.flatPoints.map(pt => (
                <li key={pt.id}>
                  <code>{pt.id}</code>: finished {pt.finished_mm.toFixed(1)} mm vs body {pt.body_mm.toFixed(1)} mm (
                  <strong>ease {pt.ease_mm.toFixed(1)} mm · {pt.status}</strong>)
                </li>
              ))}
            </ul>
          </div>
          {state.refusal.repairs[0] && (
            <p className="sc-refused-repair">
              <strong>Native repair note:</strong> {state.refusal.repairs[0]}
            </p>
          )}
        </div>
      )}

      {/* Per-side report strip (preserved verbatim across swaps!) */}
      <div className="sc-side-report" data-report-side={side}>
        {state.accepted ? (
          <>
            <div className="sc-mini-kpis">
              <div>
                <span>Hem (front / back)</span>
                <strong data-metric={`hem-${side}`}>
                  {state.world.hemAboveSoleMm.front.toFixed(1)} / {state.world.hemAboveSoleMm.back.toFixed(1)} mm
                </strong>
              </div>
              <div>
                <span>Waist gap (F / B)</span>
                <strong data-metric={`gap-${side}`}>
                  {state.section.frontGapMm.toFixed(1)} / {state.section.backGapMm.toFixed(1)} mm
                </strong>
              </div>
              <div>
                <span>Strain P95</span>
                <strong data-metric={`strain-${side}`}>{state.summary.edgeStrainPercent.p95.toFixed(2)}%</strong>
              </div>
              <div>
                <span>PC2 SHA-256</span>
                <code data-metric={`sha-${side}`}>{state.summary.sourcePc2Sha256.slice(0, 12)}</code>
              </div>
            </div>
            <details className="sc-flags">
              <summary>
                {state.summary.settled ? "Settled" : "Unsettled"} · {state.summary.gating.length} gating ·{" "}
                {state.summary.flags.length} solver notes
              </summary>
              <ul>
                {state.summary.flags.map((f, i) => (
                  <li key={i}>{f}</li>
                ))}
              </ul>
            </details>
          </>
        ) : (
          <div className="sc-mini-kpis">
            <div>
              <span>Drape admission</span>
              <strong data-metric={`hem-${side}`}>REFUSED</strong>
            </div>
            <div>
              <span>Refusal code</span>
              <code data-metric={`sha-${side}`}>{state.refusal.code}</code>
            </div>
          </div>
        )}
      </div>
    </section>
  );
}

export default function SundressCompareLab() {
  const [comparison, setComparison] = useState<{ manifest: CompareManifest; downloadUrl: string }>();
  const [manifestAttempt, setManifestAttempt] = useState(0);
  const [error, setError] = useState("");
  const [leftId, setLeftId] = useState("reference");
  const [rightId, setRightId] = useState("slender");
  const [camPreset, setCamPreset] = useState<CompareCameraPreset>("Full length");

  // Single shared Three.js camera state driving BOTH left & right canvases identically!
  const cameraRef = useRef<SharedCameraState>({ ...COMPARE_CAMERAS["Full length"] });

  const handleCameraDrag = useCallback((dx: number, dy: number, wheelDelta?: number) => {
    const st = cameraRef.current;
    if (wheelDelta) {
      st.dist = Math.max(0.9, Math.min(3.4, st.dist + wheelDelta * 0.0012));
    } else {
      st.yaw -= dx * 0.008;
      st.pitch = Math.max(-0.45, Math.min(0.65, st.pitch + dy * 0.006));
    }
  }, []);

  const applyPreset = (name: CompareCameraPreset) => {
    setCamPreset(name);
    const p = COMPARE_CAMERAS[name];
    cameraRef.current.yaw = p.yaw;
    cameraRef.current.pitch = p.pitch;
    cameraRef.current.dist = p.dist;
    cameraRef.current.targetY = p.targetY;
  };

  useEffect(() => {
    const controller = new AbortController();
    let downloadUrl: string | undefined;
    async function loadManifest() {
      try {
        const response = await publishedGeometry(COMPARE_MANIFEST, controller.signal, manifestAttempt ? "reload" : "no-cache");
        const bytes = await response.blob();
        const manifest: CompareManifest = JSON.parse(await bytes.text());
        if (controller.signal.aborted) return;
        downloadUrl = URL.createObjectURL(bytes);
        setComparison({ manifest, downloadUrl });
      } catch {
        if (!controller.signal.aborted) setError("Could not load the published comparison data.");
      }
    }
    void loadManifest();
    return () => {
      controller.abort();
      if (downloadUrl) URL.revokeObjectURL(downloadUrl);
    };
  }, [manifestAttempt]);

  const manifest = comparison?.manifest;
  const manifestDownload = comparison?.downloadUrl;
  const leftState = manifest?.states.find(s => s.id === leftId) ?? null;
  const rightState = manifest?.states.find(s => s.id === rightId) ?? null;

  const metrics = useMemo(() => {
    if (!leftState || !rightState) return null;
    return buildCompareMetrics(leftState, rightState);
  }, [leftState, rightState]);

  const swapSides = () => {
    setLeftId(rightId);
    setRightId(leftId);
  };

  return (
    <>
      <SiteHeader />
      <main id="main" className="sc-page">
        <Crumbs
          trail={[
            ["Exhibition", "/"],
            ["Wardrobe", "/sections/wardrobe/"],
            ["Garden sundress", "/wardrobe/sundress/"],
            ["Compare two drapes"],
          ]}
        />
        <div className="wrap sc-wrap">
          <div className="sc-heading">
            <div>
              <p className="kicker">Identical cut · Synchronized camera &amp; 1:1 metre scale</p>
              <h1>Compare two native drapes of <em>one fixed dress.</em></h1>
            </div>
            <p>
              Hold the Garden Sundress pattern, <code>20 mm</code> triangulation (<code>3,458</code> vertices), and
              solver flags strictly identical while comparing independently accepted native drapes on two synthetic
              adult forms side by side — or inspect why an oversized form refuses natively instead of rendering a fake
              garment.
            </p>
          </div>

          {error ? (
            <div className="sc-error">
              <p role="alert">{error}</p>
              <button className="sc-dl-sec" type="button" onClick={() => {
                setError("");
                setManifestAttempt(attempt => attempt + 1);
              }}>Retry comparison</button>
            </div>
          ) : !manifest && <p className="sc-note" role="status">Loading comparison data…</p>}

          {manifest && manifestDownload && leftState && rightState && metrics && (
            <>
              {/* Selector & Swap Bar */}
              <section className="sc-bar" aria-label="Choose left and right synthetic adult forms and shared camera">
                <div className="sc-bar-row">
                  <div className="sc-side-picker">
                    <span>Left viewport:</span>
                    <div className="sc-btn-group" role="group" aria-label="Select left synthetic adult form">
                      {manifest.states.map(s => (
                        <button
                          key={s.id}
                          type="button"
                          data-pick-left={s.id}
                          aria-pressed={leftId === s.id}
                          onClick={() => setLeftId(s.id)}
                        >
                          {s.shortLabel}
                        </button>
                      ))}
                    </div>
                  </div>

                  <div className="sc-swap-actions">
                    <button type="button" className="sc-swap-btn" data-action="swap" onClick={swapSides}>
                      ⇄ Swap sides
                    </button>
                    <button
                      type="button"
                      data-action="match-same"
                      onClick={() => setRightId(leftId)}
                    >
                      Match same state
                    </button>
                  </div>

                  <div className="sc-side-picker">
                    <span>Right viewport:</span>
                    <div className="sc-btn-group" role="group" aria-label="Select right synthetic adult form">
                      {manifest.states.map(s => (
                        <button
                          key={s.id}
                          type="button"
                          data-pick-right={s.id}
                          aria-pressed={rightId === s.id}
                          onClick={() => setRightId(s.id)}
                        >
                          {s.shortLabel}
                        </button>
                      ))}
                    </div>
                  </div>
                </div>

                <div className="sc-cam-row">
                  <span>Shared synchronized camera (drag either viewport to turn both):</span>
                  <div className="sc-btn-group" role="group" aria-label="Shared 3D camera presets">
                    {CAMERA_NAMES.map(name => (
                      <button key={name} type="button" aria-pressed={camPreset === name} onClick={() => applyPreset(name)}>
                        {name}
                      </button>
                    ))}
                  </div>
                </div>
              </section>

              {/* Side-by-side synchronized viewports under one camera and common world scale */}
              <div className="sc-viewports" data-same-state={metrics.sameState ? "true" : "false"}>
                <SideViewportCard
                  side="left"
                  state={leftState}
                  otherState={rightState}
                  accentHex="#0e6766"
                  cameraRef={cameraRef}
                  onCameraDrag={handleCameraDrag}
                />
                <SideViewportCard
                  side="right"
                  state={rightState}
                  otherState={leftState}
                  accentHex="#b84724"
                  cameraRef={cameraRef}
                  onCameraDrag={handleCameraDrag}
                />
              </div>

              {/* Lower: Hem & Geometric Clearance Delta Table + Waist Section Overlay + Identical Solver Provenance */}
              <div className="sc-lower">
                <section className="sc-card-light" aria-label="Hem elevation and waist geometric clearance comparison">
                  <h2>Hem elevation &amp; waist clearance (Right − Left)</h2>
                  {metrics.deltas ? (
                    <>
                      <p className="sc-note" data-metric="delta-summary">
                        {metrics.sameState ? (
                          <strong>
                            Exact match: identical state ({leftState.shortLabel}) loaded on both sides — all hem,
                            clearance, and strain deltas are 0.0.
                          </strong>
                        ) : (
                          <>
                            Both forms stand on the exact same world sole plane (<code>Y = 0.000 m</code>). Mean hem
                            shifts by{" "}
                            <strong>
                              {metrics.deltas.hemMeanMm > 0 ? "+" : ""}
                              {metrics.deltas.hemMeanMm.toFixed(1)} mm
                            </strong>{" "}
                            and waist centre-back clearance shifts by{" "}
                            <strong>
                              {metrics.deltas.backGapMm > 0 ? "+" : ""}
                              {metrics.deltas.backGapMm.toFixed(1)} mm
                            </strong>
                            .
                          </>
                        )}
                      </p>
                      <div className="sc-table-scroll">
                        <table>
                          <thead>
                            <tr>
                              <th>Measurement (common world frame)</th>
                              <th>Left ({leftState.shortLabel})</th>
                              <th>Right ({rightState.shortLabel})</th>
                              <th>Δ (R − L)</th>
                            </tr>
                          </thead>
                          <tbody>
                            <tr>
                              <td>Front hem above sole</td>
                              <td>{leftState.accepted ? `${leftState.world.hemAboveSoleMm.front.toFixed(1)} mm` : "—"}</td>
                              <td>{rightState.accepted ? `${rightState.world.hemAboveSoleMm.front.toFixed(1)} mm` : "—"}</td>
                              <td data-metric="delta-hem-front">
                                <strong>{metrics.deltas.hemFrontMm.toFixed(1)} mm</strong>
                              </td>
                            </tr>
                            <tr>
                              <td>Back hem above sole</td>
                              <td>{leftState.accepted ? `${leftState.world.hemAboveSoleMm.back.toFixed(1)} mm` : "—"}</td>
                              <td>{rightState.accepted ? `${rightState.world.hemAboveSoleMm.back.toFixed(1)} mm` : "—"}</td>
                              <td data-metric="delta-hem-back">
                                <strong>{metrics.deltas.hemBackMm.toFixed(1)} mm</strong>
                              </td>
                            </tr>
                            <tr>
                              <td>Waist centre-front clearance</td>
                              <td>{leftState.accepted ? `${leftState.section.frontGapMm.toFixed(1)} mm` : "—"}</td>
                              <td>{rightState.accepted ? `${rightState.section.frontGapMm.toFixed(1)} mm` : "—"}</td>
                              <td data-metric="delta-front-gap">
                                <strong>{metrics.deltas.frontGapMm.toFixed(1)} mm</strong>
                              </td>
                            </tr>
                            <tr>
                              <td>Waist centre-back clearance</td>
                              <td>{leftState.accepted ? `${leftState.section.backGapMm.toFixed(1)} mm` : "—"}</td>
                              <td>{rightState.accepted ? `${rightState.section.backGapMm.toFixed(1)} mm` : "—"}</td>
                              <td data-metric="delta-back-gap">
                                <strong>{metrics.deltas.backGapMm.toFixed(1)} mm</strong>
                              </td>
                            </tr>
                            <tr>
                              <td>Edge strain (P95)</td>
                              <td>{leftState.accepted ? `${leftState.summary.edgeStrainPercent.p95.toFixed(2)}%` : "—"}</td>
                              <td>{rightState.accepted ? `${rightState.summary.edgeStrainPercent.p95.toFixed(2)}%` : "—"}</td>
                              <td data-metric="delta-strain">
                                <strong>{metrics.deltas.strainP95Pct.toFixed(2)}%</strong>
                              </td>
                            </tr>
                          </tbody>
                        </table>
                      </div>
                    </>
                  ) : (
                    <p className="sc-note" data-metric="delta-summary">
                      One or both sides refused natively — geometric comparison deltas are withheld because a refused
                      solve never produces a completed garment surface.
                    </p>
                  )}

                  <div className="sc-slice-wrap">
                    <WaistClearanceOverlaySvg left={leftState} right={rightState} />
                  </div>
                </section>

                <aside className="sc-card-light" aria-label="Identical solver configuration and native artifacts">
                  <h2>Identical solver &amp; mesh config</h2>
                  <p className="sc-note">
                    Both accepted drapes solve the exact same fixed pattern (<code>{manifest.fixedPattern.style}</code>,{" "}
                    <code>{manifest.fixedPattern.vertices.toLocaleString("en")}</code> vertices,{" "}
                    <code>{manifest.fixedPattern.triangles.toLocaleString("en")}</code> triangles,{" "}
                    <code>{manifest.fixedPattern.spacingMm} mm</code> spacing) with identical solver settings.
                  </p>
                  <p className="sc-sha">
                    Fixed pattern SHA-256: <code>{manifest.fixedPattern.patternSha256.slice(0, 20)}…</code>
                  </p>
                  <div className="sc-downloads">
                    <a className="sc-dl-pri" href="/sundress/pattern-A4.pdf" download>
                      Full-size sewing pattern PDF ↓
                    </a>
                    <a className="sc-dl-sec" href={manifestDownload} download="compare.json">
                      Download comparison manifest (JSON) ↓
                    </a>
                    <a className="sc-dl-sec" href={manifest.fixedPattern.patternUrl} download="dress-pattern.json">
                      Fixed sundress pattern (fabric-pattern/1) ↓
                    </a>
                    <a className="sc-dl-sec" href="/exhibits/sundress/dress-summary.json" download>
                      Reference adult solver report ↓
                    </a>
                    <a className="sc-dl-sec" href="/exhibits/sundress-compare/slender-summary.json" download>
                      Petite adult solver report ↓
                    </a>
                    <a className="sc-dl-sec" href="/wardrobe/sundress/redraft/">
                      Redraft sundress after an ease warning →
                    </a>
                  </div>
                  <details className="sc-flags">
                    <summary>Rerun the native drapes</summary>
                    <p className="sc-note">
                      Save the fixed pattern, fit specification and the form’s measurements together. Set <code>FTX_ANNY_MODEL</code>{" "}
                      to your converted Anny model and run the command from that directory.
                    </p>
                    <a className="sc-dl-sec" href="/exhibits/sundress/dress-fit.json" download="dress-fit.json">
                      Shared fit specification (JSON) ↓
                    </a>
                    {NATIVE_REPLAYS.map(run => {
                      const state = manifest.states.find(state => state.id === run.id);
                      if (!state?.accepted) return null;
                      const command = `ftx drape dress-pattern.json --measurements ${run.file} --fit dress-fit.json` +
                        ` --model "$FTX_ANNY_MODEL" --out ${run.directory} ${manifest.fixedPattern.solverArguments.join(" ")} --json`;
                      return <div key={run.id} data-native-replay={run.id}>
                        <p className="sc-note">
                          {run.label}{state.summary.solverRevision && <> · Archived solver <code>{state.summary.solverRevision.slice(0, 12)}</code></>}
                        </p>
                        <a className="sc-dl-sec" href={`${run.source}/${run.file}`} download={run.file}>
                          {run.label} measurements (JSON) ↓
                        </a>
                        <pre className="sc-cli"><code>{command}</code></pre>
                      </div>;
                    })}
                  </details>
                </aside>
              </div>
            </>
          )}
        </div>
      </main>
      <SiteFooter />
    </>
  );
}
