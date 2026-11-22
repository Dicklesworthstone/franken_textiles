"use client";

import { useEffect, useId, useMemo, useRef, useState } from "react";
import SiteHeader from "./site/SiteHeader";
import SiteFooter from "./site/SiteFooter";
import Crumbs from "./site/Crumbs";
import "./friction-sled.css";

type SledSample = {
  step: number;
  time_s: number;
  applied_normal_n: number;
  retained_normal_n: number;
  applied_pull_n: [number, number];
  applied_pull_along_n: number;
  tangential_reaction_n: [number, number];
  tangential_reaction_along_n: number;
  sled_center_mm: [number, number, number];
  displacement_mm: number;
  velocity_mm_s: [number, number];
  speed_mm_s: number;
  min_gap_mm: number;
  regime: "stick" | "slip";
  converged: boolean;
  iterations: number;
};

type SledHistory = {
  id: string;
  state_id: string;
  label: string;
  description: string;
  units: { force: string; displacement: string; velocity: string; time: string; mu: string };
  normal_load_n: number;
  pull_direction: [number, number];
  pull_direction_label: string;
  pull_max_n: number;
  mu: number;
  coulomb_threshold_n: number;
  eps_v_mm_s: number;
  first_slip_step: number | null;
  first_slip_time_s: number | null;
  first_slip_pull_n: number | null;
  samples: SledSample[];
};

type SurfaceState = {
  id: string;
  label: string;
  evidence: string;
  synthetic: boolean;
  mu: number;
  calibration_input_sha256: string;
  fit_normal_n_range: [number, number];
  fit_speed_m_s_range: [number, number];
  fit_rms_scaled: number;
  check_rms_scaled: number | null;
  calibration_input: Record<string, unknown>;
};

type UnavailablePair = {
  id: string;
  label: string;
  available: boolean;
  reason: string;
  sheets: { id: string; name?: string; missing: string[] }[];
};

type SledReport = {
  schema: string;
  scope: string;
  report_sha256: string;
  sled: {
    footprint_mm: [number, number];
    mass_kg: number;
    mesh_cells: [number, number];
    time_step_s: number;
    steps: number;
    eps_v_mm_s: number;
    barrier_dhat_mm: number;
    contact_thickness_mm: number;
  };
  catalog_audit: {
    total_materials: number;
    materials_with_source_qualified_friction: number;
    unavailable_pairs: UnavailablePair[];
  };
  states: SurfaceState[];
  histories: SledHistory[];
};

type WasmCalResult = {
  mu: number | null;
  inputSha256: string | null;
  fit: { components: number; rms_scaled: number; peak_scaled: number } | null;
  check: { components: number; rms_scaled: number; peak_scaled: number } | null;
  fitNormalRangeN: [number, number] | null;
  fitSpeedRangeMS: [number, number] | null;
  readiness: {
    scope: string;
    ready: boolean;
    refusal: { code?: string; message?: string } | null;
  } | null;
  scope: string;
};

function SledPathSvg({
  history,
  stepIndex,
}: {
  history: SledHistory;
  stepIndex: number;
}) {
  const uid = useId();
  const sample = history.samples[Math.min(stepIndex, history.samples.length - 1)];
  const [dxDir, dyDir] = history.pull_direction;

  const geom = useMemo(() => {
    let minX = -70, maxX = 70, minY = -70, maxY = 70;
    for (const s of history.samples) {
      const x = s.sled_center_mm[0];
      const y = -s.sled_center_mm[1]; // SVG y-down
      minX = Math.min(minX, x - 62);
      maxX = Math.max(maxX, x + 62);
      minY = Math.min(minY, y - 62);
      maxY = Math.max(maxY, y + 62);
    }
    const pad = 28;
    const w = maxX - minX + pad * 2;
    const h = maxY - minY + pad * 2;
    const side = Math.max(w, h * 1.15);
    const cx = (minX + maxX) / 2;
    const cy = (minY + maxY) / 2;
    return {
      vb: `${(cx - side / 2).toFixed(1)} ${(cy - h / 2).toFixed(1)} ${side.toFixed(1)} ${h.toFixed(1)}`,
      minX: cx - side / 2,
      maxX: cx + side / 2,
      minY: cy - h / 2,
      maxY: cy + h / 2,
    };
  }, [history]);

  const curX = sample.sled_center_mm[0];
  const curY = -sample.sled_center_mm[1];
  const arrowScale = 52 / Math.max(1.2, history.pull_max_n);
  const pullTipX = curX + sample.applied_pull_n[0] * arrowScale;
  const pullTipY = curY - sample.applied_pull_n[1] * arrowScale;
  // Friction force on the cloth opposes pull: -tangential_reaction_n
  const fricTipX = curX - sample.tangential_reaction_n[0] * arrowScale;
  const fricTipY = curY + sample.tangential_reaction_n[1] * arrowScale;

  const stickPts = history.samples
    .slice(0, stepIndex + 1)
    .filter(s => s.regime === "stick")
    .map(s => `${s.sled_center_mm[0].toFixed(2)},${(-s.sled_center_mm[1]).toFixed(2)}`)
    .join(" ");
  const slipPts = history.samples
    .slice(0, stepIndex + 1)
    .filter(s => s.regime === "slip")
    .map(s => `${s.sled_center_mm[0].toFixed(2)},${(-s.sled_center_mm[1]).toFixed(2)}`)
    .join(" ");

  return (
    <svg
      className="fs-svg"
      role="img"
      aria-label={`Recorded 2D friction sled path along ${history.pull_direction_label} showing swatch displacement, applied pull, and resisting Coulomb reaction`}
      viewBox={geom.vb}
    >
      <defs>
        <pattern id={`${uid}grid`} width="20" height="20" patternUnits="userSpaceOnUse">
          <path d="M 20 0 H 0 V 20" fill="none" stroke="#d5cfc0" strokeWidth="0.6" opacity="0.7" />
        </pattern>
        <marker id={`${uid}pull`} viewBox="0 0 10 10" refX="7" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
          <path d="M 0 1 L 10 5 L 0 9 z" fill="#b84724" />
        </marker>
        <marker id={`${uid}fric`} viewBox="0 0 10 10" refX="7" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
          <path d="M 0 1 L 10 5 L 0 9 z" fill="#176a5b" />
        </marker>
      </defs>

      <rect x="-2000" y="-2000" width="4000" height="4000" fill="#f5f1e4" />
      <rect x="-2000" y="-2000" width="4000" height="4000" fill={`url(#${uid}grid)`} />

      {/* Counterface axes at start origin (0, 0) */}
      <line x1={geom.minX} y1="0" x2={geom.maxX} y2="0" stroke="#b9b2a1" strokeWidth="0.9" strokeDasharray="4 4" />
      <line x1="0" y1={geom.minY} x2="0" y2={geom.maxY} stroke="#b9b2a1" strokeWidth="0.9" strokeDasharray="4 4" />

      {/* Initial 100x100 mm swatch footprint ghost at origin */}
      <rect
        x="-50"
        y="-50"
        width="100"
        height="100"
        fill="none"
        stroke="#7b8687"
        strokeWidth="1.2"
        strokeDasharray="5 3"
      />

      {/* Full recorded track preview (faint) */}
      <polyline
        fill="none"
        stroke="#c9bdaa"
        strokeWidth="2.2"
        points={history.samples.map(s => `${s.sled_center_mm[0].toFixed(2)},${(-s.sled_center_mm[1]).toFixed(2)}`).join(" ")}
      />

      {/* Active moving 100x100 mm swatch sled */}
      <g transform={`translate(${curX.toFixed(2)}, ${curY.toFixed(2)})`} data-sled-regime={sample.regime}>
        <rect
          x="-50"
          y="-50"
          width="100"
          height="100"
          rx="3"
          fill={sample.regime === "slip" ? "#d5744c" : "#276961"}
          fillOpacity="0.2"
          stroke={sample.regime === "slip" ? "#a93d1d" : "#16574c"}
          strokeWidth="2.2"
        />
        {/* 2x2 production cloth mesh lines */}
        <line x1="-50" y1="0" x2="50" y2="0" stroke="#42585b" strokeWidth="0.6" opacity="0.5" />
        <line x1="0" y1="-50" x2="0" y2="50" stroke="#42585b" strokeWidth="0.6" opacity="0.5" />
        <line x1="-50" y1="-50" x2="50" y2="50" stroke="#42585b" strokeWidth="0.5" opacity="0.35" />
        <line x1="-50" y1="0" x2="0" y2="-50" stroke="#42585b" strokeWidth="0.5" opacity="0.35" />
        <line x1="0" y1="50" x2="50" y2="0" stroke="#42585b" strokeWidth="0.5" opacity="0.35" />
      </g>

      {/* Recorded stick path & slip path up to current step */}
      {stickPts && <polyline fill="none" stroke="#176a5b" strokeWidth="3.2" strokeLinecap="round" points={stickPts} />}
      {slipPts && <polyline fill="none" stroke="#b84724" strokeWidth="3.5" strokeLinecap="round" points={slipPts} />}

      {/* Force vectors at sled center */}
      {sample.applied_pull_along_n > 0.02 && (
        <line
          x1={curX}
          y1={curY}
          x2={pullTipX}
          y2={pullTipY}
          stroke="#b84724"
          strokeWidth="2.6"
          markerEnd={`url(#${uid}pull)`}
        />
      )}
      {sample.tangential_reaction_along_n > 0.02 && (
        <line
          x1={curX}
          y1={curY}
          x2={fricTipX}
          y2={fricTipY}
          stroke="#176a5b"
          strokeWidth="2.6"
          markerEnd={`url(#${uid}fric)`}
        />
      )}

      {/* Center of mass dot */}
      <circle cx={curX} cy={curY} r="4.2" fill="#172328" stroke="#fffef8" strokeWidth="1.5" />

      {/* Pull direction indicator */}
      {(() => {
        const midX = (geom.minX + geom.maxX) / 2;
        const spanW = geom.maxX - geom.minX;
        const fsTop = Math.max(7.0, Math.min(9.0, spanW * 0.026));
        const fsBot = Math.max(6.8, Math.min(8.4, spanW * 0.024));
        return (
          <>
            <text x={midX} y={geom.minY + 16} textAnchor="middle" fill="#1f3035" fontSize={fsTop.toFixed(1)} fontWeight="700" fontFamily="var(--mono)">
              SWATCH A ON B · 100×100 MM · PULL {history.pull_direction_label}
            </text>
            <text x={midX} y={geom.maxY - 9} textAnchor="middle" fill="#465558" fontSize={fsBot.toFixed(1)} fontFamily="var(--mono)">
              F_N {sample.retained_normal_n.toFixed(3)} N · GAP {sample.min_gap_mm.toFixed(3)} MM · DIR ({dxDir.toFixed(2)}, {dyDir.toFixed(2)})
            </text>
          </>
        );
      })()}
    </svg>
  );
}

function ReactionCurveSvg({
  history,
  stepIndex,
  onSelectStep,
}: {
  history: SledHistory;
  stepIndex: number;
  onSelectStep: (step: number) => void;
}) {
  const w = 520;
  const h = 300;
  const padL = 52;
  const padR = 22;
  const padT = 26;
  const padB = 42;
  const plotW = w - padL - padR;
  const plotH = h - padT - padB;

  const maxTime = history.samples[history.samples.length - 1].time_s || 0.44;
  const maxForce = Math.max(history.normal_load_n, history.pull_max_n) * 1.12;

  const xOf = (t: number) => padL + (t / maxTime) * plotW;
  const yOf = (f: number) => padT + plotH - (f / maxForce) * plotH;

  const pullPath = history.samples
    .map((s, i) => `${i === 0 ? "M" : "L"}${xOf(s.time_s).toFixed(1)},${yOf(s.applied_pull_along_n).toFixed(1)}`)
    .join(" ");
  const tangPath = history.samples
    .map((s, i) => `${i === 0 ? "M" : "L"}${xOf(s.time_s).toFixed(1)},${yOf(s.tangential_reaction_along_n).toFixed(1)}`)
    .join(" ");
  const normPath = history.samples
    .map((s, i) => `${i === 0 ? "M" : "L"}${xOf(s.time_s).toFixed(1)},${yOf(s.retained_normal_n).toFixed(1)}`)
    .join(" ");

  const cur = history.samples[Math.min(stepIndex, history.samples.length - 1)];
  const slipX = history.first_slip_time_s != null ? xOf(history.first_slip_time_s) : null;
  const threshY = yOf(history.coulomb_threshold_n);

  const yTicks = [0, maxForce * 0.25, maxForce * 0.5, maxForce * 0.75, maxForce];

  return (
    <svg
      className="fs-chart-svg"
      role="img"
      aria-label="Retained normal reaction, applied tangential pull, and tangential friction reaction versus time showing the stick-to-slip threshold"
      viewBox={`0 0 ${w} ${h}`}
    >
      <rect x="0" y="0" width={w} height={h} fill="#faf7ee" rx="4" />

      {/* Horizontal grid & Y axis ticks (N) */}
      {yTicks.map((val, idx) => (
        <g key={idx}>
          <line x1={padL} y1={yOf(val)} x2={w - padR} y2={yOf(val)} stroke="#e3ded0" strokeWidth="1" />
          <text x={padL - 8} y={yOf(val) + 3.5} textAnchor="end" fill="#526063" fontSize="10" fontFamily="var(--mono)">
            {val.toFixed(1)}
          </text>
        </g>
      ))}

      {/* Slip zone background */}
      {slipX != null && (
        <rect
          x={slipX}
          y={padT}
          width={w - padR - slipX}
          height={plotH}
          fill="#b84724"
          fillOpacity="0.08"
        />
      )}

      {/* Coulomb threshold line mu * Fn */}
      <line x1={padL} y1={threshY} x2={w - padR} y2={threshY} stroke="#a66814" strokeWidth="1.2" strokeDasharray="5 3" />
      <text x={w - padR - 4} y={threshY - 5} textAnchor="end" fill="#8b550d" fontSize="10" fontWeight="700" fontFamily="var(--mono)">
        μ·Fₙ = {history.coulomb_threshold_n.toFixed(2)} N
      </text>

      {/* Stick-to-slip vertical line */}
      {slipX != null && (
        <g>
          <line x1={slipX} y1={padT} x2={slipX} y2={padT + plotH} stroke="#b84724" strokeWidth="1.3" strokeDasharray="3 3" />
          <text x={slipX + 4} y={padT + 13} fill="#9c3214" fontSize="10" fontWeight="700" fontFamily="var(--mono)">
            SLIP (t={history.first_slip_time_s?.toFixed(2)} s)
          </text>
        </g>
      )}

      {/* Retained normal reaction F_n */}
      <path d={normPath} fill="none" stroke="#2c5875" strokeWidth="2.1" />

      {/* Applied tangential pull */}
      <path d={pullPath} fill="none" stroke="#7d8487" strokeWidth="1.9" strokeDasharray="5 3" />

      {/* Retained tangential friction reaction F_t */}
      <path d={tangPath} fill="none" stroke="#b84724" strokeWidth="2.8" />

      {/* Clickable sample columns for direct chart scrubbing */}
      {history.samples.map(s => (
        <circle
          key={s.step}
          cx={xOf(s.time_s)}
          cy={yOf(s.tangential_reaction_along_n)}
          r="5"
          fill="transparent"
          style={{ cursor: "pointer" }}
          onClick={() => onSelectStep(s.step)}
        >
          <title>{`Step ${s.step} (t=${s.time_s.toFixed(2)} s): Fn=${s.retained_normal_n.toFixed(2)} N, Ft=${s.tangential_reaction_along_n.toFixed(2)} N (${s.regime})`}</title>
        </circle>
      ))}

      {/* Active cursor vertical line & dots */}
      <line x1={xOf(cur.time_s)} y1={padT} x2={xOf(cur.time_s)} y2={padT + plotH} stroke="#172328" strokeWidth="1.2" />
      <circle cx={xOf(cur.time_s)} cy={yOf(cur.retained_normal_n)} r="4" fill="#2c5875" stroke="#fffef8" strokeWidth="1.2" />
      <circle cx={xOf(cur.time_s)} cy={yOf(cur.tangential_reaction_along_n)} r="4.8" fill="#b84724" stroke="#fffef8" strokeWidth="1.4" />

      {/* Axes & labels */}
      <line x1={padL} y1={padT} x2={padL} y2={padT + plotH} stroke="#1d2c31" strokeWidth="1.2" />
      <line x1={padL} y1={padT + plotH} x2={w - padR} y2={padT + plotH} stroke="#1d2c31" strokeWidth="1.2" />
      <text x={padL + plotW / 2} y={h - 8} textAnchor="middle" fill="#3a4a4e" fontSize="10.5" fontWeight="600" fontFamily="var(--mono)">
        Time (s) · {history.samples.length} recorded native steps
      </text>
      <text
        x="14"
        y={padT + plotH / 2}
        textAnchor="middle"
        fill="#3a4a4e"
        fontSize="10.5"
        fontWeight="600"
        fontFamily="var(--mono)"
        transform={`rotate(-90 14 ${padT + plotH / 2})`}
      >
        Force (N)
      </text>
    </svg>
  );
}

export default function FrictionSledLab() {
  const [report, setReport] = useState<SledReport | null>(null);
  const [loadError, setLoadError] = useState("");
  const [mode, setMode] = useState<"synthetic_pair_040" | "synthetic_pair_075" | "catalog_unavailable">("synthetic_pair_040");
  const [historyId, setHistoryId] = useState<string>("state_a_2n_pos_x");
  const [stepIndex, setStepIndex] = useState<number>(28);
  const [playing, setPlaying] = useState(false);
  const [allowEstimates, setAllowEstimates] = useState(true);

  const [wasmResult, setWasmResult] = useState<WasmCalResult | null>(null);
  const [wasmError, setWasmError] = useState("");
  const [wasmBusy, setWasmBusy] = useState(false);

  const workerRef = useRef<Worker | null>(null);
  const reqIdRef = useRef(0);

  useEffect(() => {
    const controller = new AbortController();
    fetch("/materials/friction-sled.json", { signal: controller.signal })
      .then(r => {
        if (!r.ok) throw new Error(`Could not load native friction-sled report (${r.status}).`);
        return r.json() as Promise<SledReport>;
      })
      .then(data => {
        if (controller.signal.aborted) return;
        setReport(data);
        const firstHist = data.histories[0];
        if (firstHist) {
          setHistoryId(firstHist.id);
          setStepIndex(firstHist.first_slip_step ?? Math.floor(firstHist.samples.length / 2));
        }
      })
      .catch(err => {
        if (!controller.signal.aborted) setLoadError(err instanceof Error ? err.message : String(err));
      });
    return () => controller.abort();
  }, []);

  const activeState = useMemo(
    () => (mode === "catalog_unavailable" ? null : report?.states.find(s => s.id === mode) ?? null),
    [report, mode],
  );
  const stateHistories = useMemo(
    () => (mode === "catalog_unavailable" ? [] : report?.histories.filter(h => h.state_id === mode) ?? []),
    [report, mode],
  );
  const activeHistory = useMemo(
    () => stateHistories.find(h => h.id === historyId) ?? stateHistories[0] ?? null,
    [stateHistories, historyId],
  );

  // Live WASM calibration verification whenever active synthetic surface state or consent changes.
  useEffect(() => {
    const worker = new Worker(new URL("../public/labs/friction-sled-worker.mjs", import.meta.url), {
      type: "module",
    });
    workerRef.current = worker;
    worker.onmessage = ({ data }: MessageEvent<{ id: number; result?: WasmCalResult; error?: string }>) => {
      if (data.id !== reqIdRef.current) return;
      setWasmBusy(false);
      if (data.error) {
        setWasmError(data.error);
        setWasmResult(null);
      } else if (data.result) {
        setWasmError("");
        setWasmResult(data.result);
      }
    };
    worker.onerror = ev => {
      setWasmBusy(false);
      setWasmError(ev.message || "Friction calibration worker failed.");
    };
    return () => {
      worker.terminate();
      workerRef.current = null;
    };
  }, []);

  useEffect(() => {
    const worker = workerRef.current;
    if (!worker || !activeState) {
      setWasmResult(null);
      setWasmError("");
      setWasmBusy(false);
      return;
    }
    const id = ++reqIdRef.current;
    setWasmBusy(true);
    setWasmError("");
    worker.postMessage({
      id,
      calibrationInput: activeState.calibration_input,
      allowEstimates,
    });
  }, [activeState, allowEstimates]);

  // Smooth step playback without side effects inside React state updaters.
  useEffect(() => {
    if (!playing || !activeHistory) return;
    const maxIdx = activeHistory.samples.length - 1;
    if (stepIndex >= maxIdx) {
      setPlaying(false);
      return;
    }
    const id = window.setTimeout(() => {
      if (stepIndex + 1 >= maxIdx) {
        setStepIndex(maxIdx);
        setPlaying(false);
      } else {
        setStepIndex(stepIndex + 1);
      }
    }, 65);
    return () => window.clearTimeout(id);
  }, [playing, stepIndex, activeHistory]);

  const activeSample = activeHistory
    ? activeHistory.samples[Math.min(stepIndex, activeHistory.samples.length - 1)]
    : null;

  const selectSurfaceMode = (nextMode: "synthetic_pair_040" | "synthetic_pair_075" | "catalog_unavailable") => {
    setPlaying(false);
    setMode(nextMode);
    if (report && nextMode !== "catalog_unavailable") {
      const nextHist = report.histories.find(h => h.state_id === nextMode);
      if (nextHist) {
        setHistoryId(nextHist.id);
        setStepIndex(nextHist.first_slip_step ?? 0);
      }
    }
  };

  return (
    <>
      <SiteHeader />
      <main id="main" className="fs-page">
        <Crumbs trail={[["Exhibition", "/"], ["Materials", "/sections/materials/"], ["Friction sled"]]} />
        <div className="wrap fs-wrap">
          <div className="fs-heading">
            <div>
              <p className="kicker">Contact &amp; Coulomb friction · Native sliding fixture</p>
              <h1>Put two swatches on a <em>friction sled.</em></h1>
            </div>
            <p>
              Step through recorded native <code>ftx-drape</code> sliding-sled trajectories between two declared
              synthetic surface states. Inspect retained IPC normal reactions, the tangential stick-to-slip plateau at{" "}
              <code>μ·Fₙ</code>, live Rust WebAssembly friction calibration, and why uncalibrated real catalog textiles
              remain unavailable.
            </p>
          </div>

          {/* Surface State & Declared Load History Bar */}
          <section className="fs-bar" aria-label="Surface state and declared load history controls">
            <div className="fs-row">
              <div className="fs-group" role="group" aria-label="Choose surface state">
                <button
                  type="button"
                  data-surface="synthetic_pair_040"
                  aria-pressed={mode === "synthetic_pair_040"}
                  onClick={() => selectSurfaceMode("synthetic_pair_040")}
                >
                  Synthetic pair A/B (μ = 0.40)
                </button>
                <button
                  type="button"
                  data-surface="synthetic_pair_075"
                  aria-pressed={mode === "synthetic_pair_075"}
                  onClick={() => selectSurfaceMode("synthetic_pair_075")}
                >
                  Higher-grip synthetic (μ = 0.75)
                </button>
                <button
                  type="button"
                  data-surface="catalog_unavailable"
                  aria-pressed={mode === "catalog_unavailable"}
                  onClick={() => selectSurfaceMode("catalog_unavailable")}
                >
                  Catalog textiles (missing μ · unavailable)
                </button>
              </div>
            </div>

            {stateHistories.length > 0 && (
              <div className="fs-row">
                <div className="fs-group" role="group" aria-label="Choose declared load history">
                  {stateHistories.map(h => (
                    <button
                      key={h.id}
                      type="button"
                      data-history={h.id}
                      aria-pressed={activeHistory?.id === h.id}
                      onClick={() => {
                        setPlaying(false);
                        setHistoryId(h.id);
                        setStepIndex(h.first_slip_step ?? 0);
                      }}
                    >
                      {h.label}
                    </button>
                  ))}
                </div>
                <div className="fs-transport">
                  <button
                    type="button"
                    className="fs-play"
                    disabled={!activeHistory}
                    aria-pressed={playing}
                    onClick={() => {
                      if (!activeHistory) return;
                      if (!playing && stepIndex >= activeHistory.samples.length - 1) setStepIndex(0);
                      setPlaying(p => !p);
                    }}
                  >
                    {playing ? "Pause" : stepIndex >= (activeHistory?.samples.length ?? 1) - 1 ? "Replay ramp" : "Play ramp"}
                  </button>
                  <button
                    type="button"
                    disabled={!activeHistory || activeHistory.first_slip_step == null}
                    onClick={() => {
                      if (!activeHistory || activeHistory.first_slip_step == null) return;
                      setPlaying(false);
                      setStepIndex(activeHistory.first_slip_step);
                    }}
                  >
                    Jump to breakaway
                  </button>
                  <span className="fs-step-pair">
                    <button
                      type="button"
                      disabled={!activeHistory || stepIndex <= 0}
                      aria-label="Previous recorded step"
                      onClick={() => { setPlaying(false); setStepIndex(s => Math.max(0, s - 1)); }}
                    >
                      ←
                    </button>
                    <button
                      type="button"
                      disabled={!activeHistory || stepIndex >= activeHistory.samples.length - 1}
                      aria-label="Next recorded step"
                      onClick={() => { setPlaying(false); setStepIndex(s => Math.min((activeHistory?.samples.length ?? 1) - 1, s + 1)); }}
                    >
                      →
                    </button>
                  </span>
                </div>
              </div>
            )}
          </section>

          {loadError && (
            <p className="fs-error" role="alert">
              {loadError}
            </p>
          )}

          {/* Unavailable Catalog State vs Active Synthetic Sled Stage */}
          {mode === "catalog_unavailable" && report ? (
            <section className="fs-unavail-card" aria-label="Missing source-qualified catalog friction refusal" data-unavailable="true">
              <div className="fs-unavail-head">
                <h2>Unavailable: missing source-qualified friction coefficients</h2>
                <span className="fs-refusal-pill">0 / {report.catalog_audit.total_materials} CATALOG MATERIALS QUALIFIED</span>
              </div>
              <p>
                FrankenTextiles never invents generic fabric friction presets. Across all{" "}
                <strong>{report.catalog_audit.total_materials}</strong> named catalog records,{" "}
                <strong>{report.catalog_audit.materials_with_source_qualified_friction}</strong> currently carry a
                source-qualified <code>friction_self</code> or <code>friction_skin</code> observation. Until real
                counterface acquisitions are recorded, named textile pairs remain unavailable on the sled.
              </p>
              <div className="fs-unavail-grid">
                {report.catalog_audit.unavailable_pairs.map(pair => (
                  <article key={pair.id} className="fs-unavail-item">
                    <h3>{pair.label}</h3>
                    <p>{pair.reason}</p>
                    <ul>
                      {pair.sheets.map(sh => (
                        <li key={sh.id}>
                          <code>{sh.id}</code> — missing: <strong>{sh.missing.join(", ")}</strong>
                        </li>
                      ))}
                    </ul>
                  </article>
                ))}
              </div>
            </section>
          ) : (
            activeHistory &&
            activeSample && (
              <>
                {/* Scrubber */}
                <div className="fs-scrub-card">
                  <div className="fs-scrub-meta" aria-live="polite">
                    <span className="fs-regime-badge" data-regime={activeSample.regime} data-metric="regime">
                      {activeSample.regime === "slip" ? "SLIP · SLIDING PAST μ·Fₙ" : "STICK · HELD BELOW μ·Fₙ"}
                    </span>
                    <strong>
                      Step {activeSample.step} / {activeHistory.samples.length - 1} · t = {activeSample.time_s.toFixed(2)} s
                    </strong>
                    <span>
                      Threshold μ·Fₙ = {activeHistory.coulomb_threshold_n.toFixed(2)} N (first slip at step #
                      {activeHistory.first_slip_step}, F_pull = {activeHistory.first_slip_pull_n?.toFixed(2)} N)
                    </span>
                  </div>
                  <input
                    type="range"
                    min="0"
                    max={activeHistory.samples.length - 1}
                    step="1"
                    value={stepIndex}
                    aria-label="Recorded friction sled step cursor"
                    onChange={e => { setPlaying(false); setStepIndex(Number(e.target.value)); }}
                  />
                </div>

                {/* Dual synchronized stage: Recorded 2D Sled Track + Original Reaction Curves */}
                <div className="fs-dual">
                  <section className="fs-card" aria-label="Recorded 2D friction sled path">
                    <div className="fs-card-head">
                      <span>RECORDED SLED PATH · {activeHistory.pull_direction_label}</span>
                      <span>μ = {activeHistory.mu.toFixed(2)} (SYNTHETIC)</span>
                    </div>
                    <div className="fs-svg-wrap">
                      <SledPathSvg history={activeHistory} stepIndex={stepIndex} />
                    </div>
                    <div className="fs-legend">
                      <span><i className="sw-stick" /> Stick path &amp; resisting friction</span>
                      <span><i className="sw-slip" /> Slip path &amp; applied pull</span>
                      <span><i className="sw-ghost" /> Initial 100×100 mm swatch footprint</span>
                    </div>
                  </section>

                  <section className="fs-card" aria-label="Retained normal and tangential reaction curves">
                    <div className="fs-card-head">
                      <span>RETAINED NORMAL &amp; TANGENTIAL REACTIONS (N)</span>
                      <span>ε_v = {activeHistory.eps_v_mm_s.toFixed(1)} mm/s</span>
                    </div>
                    <div className="fs-chart-wrap">
                      <ReactionCurveSvg
                        history={activeHistory}
                        stepIndex={stepIndex}
                        onSelectStep={s => { setPlaying(false); setStepIndex(s); }}
                      />
                    </div>
                    <div className="fs-legend">
                      <span><i className="sw-norm" /> Retained normal Fₙ (N)</span>
                      <span><i className="sw-slip" /> Tangential friction Fₜ (N)</span>
                      <span><i className="sw-pull" /> Applied pull ramp (N)</span>
                      <span><i className="sw-thresh" /> Coulomb threshold μ·Fₙ</span>
                    </div>
                  </section>
                </div>

                {/* Live Recorded Telemetry KPIs */}
                <dl className="fs-kpis" aria-live="polite">
                  <div>
                    <dt>Retained normal Fₙ</dt>
                    <dd data-metric="normal">{activeSample.retained_normal_n.toFixed(3)} N</dd>
                    <small>IPC barrier sum (target {activeHistory.normal_load_n.toFixed(1)} N)</small>
                  </div>
                  <div>
                    <dt>Tangential reaction Fₜ</dt>
                    <dd data-metric="tangential">{activeSample.tangential_reaction_along_n.toFixed(3)} N</dd>
                    <small>Applied pull {activeSample.applied_pull_along_n.toFixed(3)} N</small>
                  </div>
                  <div>
                    <dt>Coulomb threshold μ·Fₙ</dt>
                    <dd data-metric="threshold">{activeHistory.coulomb_threshold_n.toFixed(3)} N</dd>
                    <small>μ = {activeHistory.mu.toFixed(2)} (dimensionless)</small>
                  </div>
                  <div>
                    <dt>Sled travel</dt>
                    <dd data-metric="displacement">{activeSample.displacement_mm.toFixed(2)} mm</dd>
                    <small>
                      ({activeSample.sled_center_mm[0].toFixed(1)}, {activeSample.sled_center_mm[1].toFixed(1)}) mm along{" "}
                      {activeHistory.pull_direction_label}
                    </small>
                  </div>
                  <div>
                    <dt>Tangential speed</dt>
                    <dd data-metric="speed">{activeSample.speed_mm_s.toFixed(2)} mm/s</dd>
                    <small>Stick band ≤ {activeHistory.eps_v_mm_s.toFixed(1)} mm/s</small>
                  </div>
                  <div>
                    <dt>Minimum contact gap</dt>
                    <dd data-metric="gap">{activeSample.min_gap_mm.toFixed(3)} mm</dd>
                    <small>Strictly positive · {activeSample.iterations} Newton its</small>
                  </div>
                </dl>
              </>
            )
          )}

          {/* Live WASM Friction Calibration Proof & Native Replay */}
          {report && (
            <div className="fs-lower">
              <section className="fs-panel-card" aria-label="Live Rust WebAssembly friction calibration">
                <div className="fs-panel-head">
                  <h2>Live WASM <code>material_calibrate_friction</code></h2>
                  <label className="fs-consent-check">
                    <input
                      type="checkbox"
                      checked={allowEstimates}
                      onChange={e => setAllowEstimates(e.target.checked)}
                    />
                    Admit synthetic estimate (<code>--allow-estimates</code>)
                  </label>
                </div>

                {activeState ? (
                  <>
                    <p className="fs-note">
                      Identifying kinetic <code>μ</code> directly in your browser from the declared synthetic
                      steady-pull acquisitions (<code>fit</code> and held-out <code>check</code> partitions).
                    </p>
                    {wasmBusy && <p className="fs-wasm-status">Running Rust/WASM calibration…</p>}
                    {wasmError && (
                      <p className="fs-wasm-refusal" role="alert" data-wasm-refusal="true">
                        Refused: {wasmError}
                      </p>
                    )}
                    {wasmResult && (
                      <div className="fs-wasm-grid">
                        <div>
                          <span>Identified μ (WASM)</span>
                          <code data-metric="wasm-mu">{wasmResult.mu?.toFixed(4) ?? "—"} (1)</code>
                        </div>
                        <div>
                          <span>Interface admission</span>
                          <code data-metric="wasm-ready">
                            {wasmResult.readiness?.ready
                              ? "READY (interface_only)"
                              : `REFUSED · ${wasmResult.readiness?.refusal?.message ?? "consent required"}`}
                          </code>
                        </div>
                        <div>
                          <span>Fit / Check RMS scaled</span>
                          <code>
                            {wasmResult.fit?.rms_scaled.toExponential(2)} /{" "}
                            {wasmResult.check?.rms_scaled.toExponential(2) ?? "none"}
                          </code>
                        </div>
                        <div>
                          <span>Fit load &amp; speed envelope</span>
                          <code>
                            {wasmResult.fitNormalRangeN?.join("–")} N · {wasmResult.fitSpeedRangeMS?.join("–")} m/s
                          </code>
                        </div>
                      </div>
                    )}
                  </>
                ) : (
                  <p className="fs-note">
                    Select either synthetic surface state above to run live WASM friction identification. Uncalibrated
                    catalog textiles have no sliding acquisition to fit.
                  </p>
                )}
              </section>

              <aside className="fs-panel-card" aria-label="Original native report and honesty scope">
                <h2>Native report &amp; scope</h2>
                <p className="fs-note">{report.scope}</p>
                <p className="fs-digest">
                  Report SHA-256: <code>{report.report_sha256.slice(0, 16)}…</code> · Catalog qualified friction:{" "}
                  <strong>
                    {report.catalog_audit.materials_with_source_qualified_friction} /{" "}
                    {report.catalog_audit.total_materials}
                  </strong>
                </p>
                <div className="fs-downloads">
                  <a className="fs-dl-pri" href="/materials/friction-sled.json" download="frankentextiles-friction-sled.json">
                    Download native sled report (JSON) ↓
                  </a>
                </div>
                <pre className="fs-cli">
                  <code>
                    ftx-materials calibrate-friction tests/fixtures/materials/synthetic-friction-sliding.json
                    --allow-estimates
                  </code>
                </pre>
              </aside>
            </div>
          )}
        </div>
      </main>
      <SiteFooter />
    </>
  );
}
