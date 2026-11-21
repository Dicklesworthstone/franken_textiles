"use client";

import dynamic from "next/dynamic";
import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import SiteHeader from "./site/SiteHeader";
import SiteFooter from "./site/SiteFooter";
import Crumbs from "./site/Crumbs";
import starterData from "../public/labs/sewing-starter.json";
import "./sewing-order.css";

const Studio = dynamic(() => import("./Studio"), { ssr: false });

type Pt = [number, number];

type SideInfo = {
  panel: string;
  reference: string;
  lengthMm: number;
  thicknessMm: number;
  allowanceMm: number;
  startMm: Pt;
  endMm: Pt;
  stations: Pt[];
};

type RunInfo = {
  phase: string;
  direction: string;
  fromStation: number;
  toStation: number;
  stitchIntervals: number;
  feedSeconds: number;
};

type SewingOp = {
  index: number;
  id: string;
  seam: string;
  title: string;
  stage: string;
  after: string[];
  unlocks: string[];
  plies: number[];
  maxStitchMm: number;
  spm: number;
  needle: string;
  thread: string;
  stackMm: number;
  stitchIntervals: number;
  feedSeconds: number;
  feedRatioRange: [number, number];
  notchMatch: string;
  sideA: SideInfo;
  sideB: SideInfo;
  runs: RunInfo[];
};

type SewingStarter = {
  schema: string;
  garment: string;
  sizeLabel: string;
  patternSha256: string;
  material: {
    id: string;
    name: string;
    thicknessMm: number;
    method: string;
    sourceUrl: string;
  };
  machineId: string;
  totalOperations: number;
  totalStitchIntervals: number;
  totalFeedSeconds: number;
  totalDirectedStitches: number;
  hardwareExecutionAuthorized: boolean;
  physicalSeamQualityValidated: boolean;
  instances: Record<string, Pt[]>;
  operations: SewingOp[];
};

const DATA = starterData as unknown as SewingStarter;

const PANEL_ORDER = ["front.L", "back", "front.R", "sleeve.L", "sleeve.R"] as const;
const PANEL_LABELS: Record<string, string> = {
  back: "BACK",
  "front.L": "FRONT · L",
  "front.R": "FRONT · R",
  "sleeve.L": "SLEEVE · L",
  "sleeve.R": "SLEEVE · R",
};

type PlacedPanel = {
  name: string;
  dx: number;
  dy: number;
  w: number;
  h: number;
  minX: number;
  maxY: number;
  outlinePath: string;
  cx: number;
  cy: number;
};

function layoutPanels(instances: Record<string, Pt[]>) {
  // Compute clean 2-row board placement in exact 1:1 millimetre units (y-up -> SVG y-down).
  const boxes = new Map<string, { minX: number; maxX: number; minY: number; maxY: number; w: number; h: number }>();
  for (const name of PANEL_ORDER) {
    const pts = instances[name] || [];
    const xs = pts.map(p => p[0]);
    const ys = pts.map(p => p[1]);
    const minX = Math.min(...xs);
    const maxX = Math.max(...xs);
    const minY = Math.min(...ys);
    const maxY = Math.max(...ys);
    boxes.set(name, { minX, maxX, minY, maxY, w: maxX - minX, h: maxY - minY });
  }

  const gapX = 34;
  const gapY = 44;
  const padX = 28;
  const padY = 30;
  const topNames = ["front.L", "back", "front.R"] as const;
  const botNames = ["sleeve.L", "sleeve.R"] as const;

  const topW = topNames.reduce((s, n) => s + boxes.get(n)!.w, 0) + gapX * (topNames.length - 1);
  const topH = Math.max(...topNames.map(n => boxes.get(n)!.h));
  const botW = botNames.reduce((s, n) => s + boxes.get(n)!.w, 0) + gapX * (botNames.length - 1);
  const botH = Math.max(...botNames.map(n => boxes.get(n)!.h));

  const contentW = Math.max(topW, botW);
  const vbH = padY + topH + gapY + botH + padY + 12;
  const vbW = Math.max(contentW + padX * 2, vbH * 1.34);

  const placed = new Map<string, PlacedPanel>();

  let curX = (vbW - topW) / 2;
  for (const name of topNames) {
    const b = boxes.get(name)!;
    const dx = curX - b.minX;
    const dy = padY + (topH - b.h) / 2 + b.maxY;
    const pts = instances[name];
    const outlinePath =
      pts.map(([x, y], i) => `${i === 0 ? "M" : "L"}${(x + dx).toFixed(1)},${(dy - y).toFixed(1)}`).join(" ") + " Z";
    placed.set(name, {
      name,
      dx,
      dy,
      w: b.w,
      h: b.h,
      minX: b.minX,
      maxY: b.maxY,
      outlinePath,
      cx: curX + b.w / 2,
      cy: padY + (topH - b.h) / 2 + b.h / 2,
    });
    curX += b.w + gapX;
  }

  curX = (vbW - botW) / 2;
  const botTop = padY + topH + gapY;
  for (const name of botNames) {
    const b = boxes.get(name)!;
    const dx = curX - b.minX;
    const dy = botTop + (botH - b.h) / 2 + b.maxY;
    const pts = instances[name];
    const outlinePath =
      pts.map(([x, y], i) => `${i === 0 ? "M" : "L"}${(x + dx).toFixed(1)},${(dy - y).toFixed(1)}`).join(" ") + " Z";
    placed.set(name, {
      name,
      dx,
      dy,
      w: b.w,
      h: b.h,
      minX: b.minX,
      maxY: b.maxY,
      outlinePath,
      cx: curX + b.w / 2,
      cy: botTop + (botH - b.h) / 2 + b.h / 2,
    });
    curX += b.w + gapX;
  }

  return { placed, vbW, vbH };
}

function toBoard(panel: PlacedPanel, [x, y]: Pt): Pt {
  return [panel.dx + x, panel.dy - y];
}

function polylinePath(panel: PlacedPanel, pts: Pt[]) {
  return pts
    .map((p, i) => {
      const [bx, by] = toBoard(panel, p);
      return `${i === 0 ? "M" : "L"}${bx.toFixed(1)},${by.toFixed(1)}`;
    })
    .join(" ");
}

function FlatPatternBoard({
  activeIndex,
  onSelectOp,
}: {
  activeIndex: number;
  onSelectOp: (idx: number) => void;
}) {
  const uid = useId();
  const { placed, vbW, vbH } = useMemo(() => layoutPanels(DATA.instances), []);
  const active = DATA.operations[activeIndex];
  const panelA = active.sideA.panel;
  const panelB = active.sideB.panel;
  const samePanel = panelA === panelB;

  const pA = placed.get(panelA)!;
  const pB = placed.get(panelB)!;
  const aStart = toBoard(pA, active.sideA.startMm);
  const aEnd = toBoard(pA, active.sideA.endMm);
  const bStart = toBoard(pB, active.sideB.startMm);
  const bEnd = toBoard(pB, active.sideB.endMm);

  return (
    <svg
      className="so-flat-svg"
      role="img"
      aria-label={`Flat Zouave bolero pattern lighting operation ${activeIndex + 1} (${active.title}): Side A ${active.sideA.reference} paired with Side B ${active.sideB.reference}`}
      viewBox={`0 0 ${vbW.toFixed(1)} ${vbH.toFixed(1)}`}
    >
      <defs>
        <pattern id={`${uid}grid`} width="25" height="25" patternUnits="userSpaceOnUse">
          <path d="M 25 0 H 0 V 25" fill="none" stroke="#d5ceb8" strokeWidth="0.7" opacity="0.65" />
        </pattern>
      </defs>

      <rect width={vbW} height={vbH} fill="#f4efe0" />
      <rect width={vbW} height={vbH} fill={`url(#${uid}grid)`} />

      {/* 5 flat pattern pieces */}
      {PANEL_ORDER.map(name => {
        const panel = placed.get(name)!;
        const isA = name === panelA;
        const isB = name === panelB;
        const fill =
          isA && isB ? "#efe2cf" : isA ? "#d6ebe4" : isB ? "#f5dfd5" : "#fcfaf2";
        const stroke =
          isA && isB ? "#7d4628" : isA ? "#197562" : isB ? "#b84724" : "#6c7678";
        return (
          <g key={name} className="so-panel" data-panel={name} data-active={isA || isB ? "true" : "false"}>
            <path d={panel.outlinePath} fill={fill} stroke={stroke} strokeWidth={isA || isB ? 2.2 : 1.15} />
            <text
              x={panel.cx}
              y={panel.cy}
              textAnchor="middle"
              fill={isA || isB ? "#1d2c31" : "#687274"}
              fontSize="13.5"
              fontWeight="700"
              fontFamily="var(--mono)"
            >
              {PANEL_LABELS[name]}
            </text>
            {(isA || isB) && (
              <text
                x={panel.cx}
                y={panel.cy + 16}
                textAnchor="middle"
                fill={samePanel ? "#7d4628" : isA ? "#135d4e" : "#9c3718"}
                fontSize="10.5"
                fontWeight="700"
                fontFamily="var(--mono)"
              >
                {samePanel ? "SIDE A + B" : isA ? "SIDE A" : "SIDE B"}
              </text>
            )}
          </g>
        );
      })}

      {/* All non-active operations (completed vs upcoming) */}
      {DATA.operations.map(op => {
        if (op.index === activeIndex) return null;
        const done = op.index < activeIndex;
        const panA = placed.get(op.sideA.panel)!;
        const panB = placed.get(op.sideB.panel)!;
        return (
          <g
            key={op.id}
            className="so-op-trace"
            data-op={op.id}
            data-state={done ? "done" : "pending"}
            onClick={() => onSelectOp(op.index)}
            style={{ cursor: "pointer" }}
          >
            <path
              d={polylinePath(panA, op.sideA.stations)}
              fill="none"
              stroke={done ? "#2e7d5e" : "#8b9394"}
              strokeWidth={done ? 2.2 : 1.3}
              strokeDasharray={done ? undefined : "4 3"}
              opacity={done ? 0.7 : 0.45}
            />
            <path
              d={polylinePath(panB, op.sideB.stations)}
              fill="none"
              stroke={done ? "#2e7d5e" : "#8b9394"}
              strokeWidth={done ? 2.2 : 1.3}
              strokeDasharray={done ? undefined : "4 3"}
              opacity={done ? 0.7 : 0.45}
            />
          </g>
        );
      })}

      {/* Active operation paired stitch paths + physical start/end markers */}
      <g className="so-active-seam" data-active-op={active.id}>
        <path
          d={polylinePath(pA, active.sideA.stations)}
          fill="none"
          stroke="#197562"
          strokeWidth="4.4"
          strokeLinecap="round"
        />
        <path
          d={polylinePath(pB, active.sideB.stations)}
          fill="none"
          stroke="#c44b24"
          strokeWidth="4.4"
          strokeLinecap="round"
        />
        {/* Start & end markers on Side A */}
        <circle cx={aStart[0]} cy={aStart[1]} r="6.5" fill="#197562" stroke="#fffef8" strokeWidth="2" />
        <text x={aStart[0]} y={aStart[1] + 3.5} textAnchor="middle" fill="#fffef8" fontSize="8.5" fontWeight="700" fontFamily="var(--mono)">
          S
        </text>
        <circle cx={aEnd[0]} cy={aEnd[1]} r="6.5" fill="#134e42" stroke="#fffef8" strokeWidth="2" />
        <text x={aEnd[0]} y={aEnd[1] + 3.5} textAnchor="middle" fill="#fffef8" fontSize="8.5" fontWeight="700" fontFamily="var(--mono)">
          E
        </text>

        {/* Start & end markers on Side B */}
        <circle cx={bStart[0]} cy={bStart[1]} r="6.5" fill="#c44b24" stroke="#fffef8" strokeWidth="2" />
        <text x={bStart[0]} y={bStart[1] + 3.5} textAnchor="middle" fill="#fffef8" fontSize="8.5" fontWeight="700" fontFamily="var(--mono)">
          S
        </text>
        <circle cx={bEnd[0]} cy={bEnd[1]} r="6.5" fill="#8f2f12" stroke="#fffef8" strokeWidth="2" />
        <text x={bEnd[0]} y={bEnd[1] + 3.5} textAnchor="middle" fill="#fffef8" fontSize="8.5" fontWeight="700" fontFamily="var(--mono)">
          E
        </text>
      </g>
    </svg>
  );
}

export default function SewingOrderLab() {
  const [step, setStep] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [spread, setSpread] = useState(0.18);
  const [wire, setWire] = useState(false);
  const [resetKey, setResetKey] = useState(0);
  const [webglReady, setWebglReady] = useState(false);
  const [webglFailed, setWebglFailed] = useState(false);
  const [copied, setCopied] = useState(false);
  const copyTimer = useRef<number | null>(null);

  useEffect(() => {
    return () => {
      if (copyTimer.current !== null) window.clearTimeout(copyTimer.current);
    };
  }, []);

  const total = DATA.operations.length;
  const active = DATA.operations[step];
  const opById = useMemo(() => new Map(DATA.operations.map(o => [o.id, o])), []);

  const goPrev = useCallback(() => {
    setPlaying(false);
    setStep(s => (s - 1 + total) % total);
  }, [total]);

  const goNext = useCallback(() => {
    setPlaying(false);
    setStep(s => (s + 1) % total);
  }, [total]);

  // Step-through autoplay timer
  useEffect(() => {
    if (!playing) return;
    const id = window.setInterval(() => {
      setStep(s => {
        if (s + 1 >= total) {
          setPlaying(false);
          return s;
        }
        return s + 1;
      });
    }, 2200);
    return () => window.clearInterval(id);
  }, [playing, total]);

  // Keyboard ArrowLeft / ArrowRight step navigation (never steals arrows from inputs or the 3D orbit canvas)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.altKey || e.ctrlKey || e.metaKey) return;
      const target = e.target as HTMLElement | null;
      if (target && (target.tagName === "INPUT" || target.tagName === "SELECT" || target.tagName === "TEXTAREA" || target.tagName === "CANVAS")) {
        return;
      }
      if (e.key === "ArrowRight") {
        e.preventDefault();
        goNext();
      } else if (e.key === "ArrowLeft") {
        e.preventDefault();
        goPrev();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [goNext, goPrev]);

  const onReady = useCallback(() => setWebglReady(true), []);
  const onError = useCallback(() => setWebglFailed(true), []);

  const cliCommand = [
    "# Replay this exact 10-operation sewing plan and directed stitch program natively",
    "ftx-materials sew-plan bolero-duck.json bolero-sew-options.json",
    "ftx-materials sew-stitches bolero-duck.json bolero-sew-options.json",
  ].join("\n");

  const copyCli = () => {
    navigator.clipboard?.writeText(cliCommand).then(
      () => {
        setCopied(true);
        if (copyTimer.current !== null) window.clearTimeout(copyTimer.current);
        copyTimer.current = window.setTimeout(() => setCopied(false), 1800);
      },
      () => {},
    );
  };

  return (
    <>
      <SiteHeader />
      <main id="main" className="so-page">
        <Crumbs trail={[["Exhibition", "/"], ["Drape gallery", "/#studies"], ["Jacket sewing order"]]} />
        <div className="wrap so-wrap">
          <div className="so-heading">
            <div>
              <p className="kicker">Native sewing planner · {DATA.schema}</p>
              <h1>Play the actual <em>sewing order.</em></h1>
            </div>
            <p>
              Step through all {total} accepted operations for the 92 cm Zouave bolero jacket on NISTIR 7282 Cotton Duck
              ({DATA.material.thicknessMm} mm micrometer thickness). Each step lights the exact paired edges on both the
              flat cut pieces and the three-dimensional jacket, with its prerequisites, unlocked steps, and physical endpoints.
            </p>
          </div>

          {/* Step transport bar */}
          <div className="so-transport" role="region" aria-label="Sewing order playback controls">
            <div className="so-transport-btns">
              <button type="button" onClick={goPrev} aria-label="Previous sewing operation">
                ← Previous
              </button>
              <button
                type="button"
                className="so-play"
                aria-pressed={playing}
                onClick={() => {
                  if (!playing && step === total - 1) setStep(0);
                  setPlaying(p => !p);
                }}
              >
                {playing ? "Pause" : "Play order"}
              </button>
              <button type="button" onClick={goNext} aria-label="Next sewing operation">
                Next →
              </button>
            </div>
            <div className="so-step-status" aria-live="polite">
              <span className="so-step-pill">
                Step {step + 1} / {total}
              </span>
              <strong>{active.title}</strong>
              <code>{active.id}</code>
            </div>
            <span className="so-key-hint">Keys: ← / →</span>
          </div>

          {/* Dual synchronized stage: Flat Pattern Board + 3D Jacket */}
          <div className="so-dual-stage">
            <section className="so-card so-flat-card" aria-label="Flat pattern operation view">
              <div className="so-card-head">
                <span>FLAT PATTERN · MILLIMETRES (1:1)</span>
                <span>
                  {active.sideA.reference} ↔ {active.sideB.reference}
                </span>
              </div>
              <div className="so-flat-frame">
                <FlatPatternBoard activeIndex={step} onSelectOp={i => { setPlaying(false); setStep(i); }} />
              </div>
              <div className="so-legend">
                <span><i className="lg-a" /> Side A ({active.sideA.panel})</span>
                <span><i className="lg-b" /> Side B ({active.sideB.panel})</span>
                <span><i className="lg-done" /> Sewn ({step})</span>
                <span><i className="lg-pend" /> Remaining ({total - step - 1})</span>
                <span><strong>S → E</strong> Stitch direction</span>
              </div>
            </section>

            <section className="so-card so-3d-card" aria-label="3D jacket panel view">
              <div className="so-card-head">
                <span>3D JACKET · SEAM {active.seam.toUpperCase()}</span>
                <div className="so-3d-tools">
                  <label>
                    Explode
                    <input
                      type="range"
                      min="0"
                      max="0.7"
                      step="0.02"
                      value={spread}
                      aria-label="Explode 3D jacket panels"
                      onChange={e => setSpread(Number(e.target.value))}
                    />
                  </label>
                  <label className="so-mesh-check">
                    <input type="checkbox" checked={wire} onChange={e => setWire(e.target.checked)} />
                    Mesh
                  </label>
                  <button type="button" onClick={() => setResetKey(k => k + 1)}>
                    Reset
                  </button>
                </div>
              </div>
              <div className="so-3d-frame">
                {webglFailed ? (
                  <p className="so-3d-fallback" role="status">
                    WebGL is unavailable on this device. Inspect the flat pattern board, physical endpoints, and
                    downloadable stitch program below.
                  </p>
                ) : (
                  <>
                    <Studio
                      garment="bolero"
                      mode="construction"
                      frame={1}
                      wire={wire}
                      seam={active.seam}
                      spread={spread}
                      resetKey={resetKey}
                      onReady={onReady}
                      onError={onError}
                    />
                    {!webglReady && (
                      <span className="so-3d-loading" role="status">
                        Loading 3D bolero…
                      </span>
                    )}
                  </>
                )}
              </div>
              <div className="so-legend">
                <span><i className="lg-3d-a" /> Cyan: Side A</span>
                <span><i className="lg-3d-b" /> Gold: Side B</span>
                <span>Drag to orbit · Scroll to zoom</span>
              </div>
            </section>
          </div>

          {/* Operation list + Detailed inspector */}
          <div className="so-lower">
            <aside className="so-op-list-card" aria-label="Accepted sewing operations in native order">
              <h2>Native order ({total} operations)</h2>
              <p className="so-sub">
                {DATA.totalStitchIntervals} stitch intervals · {DATA.totalFeedSeconds.toFixed(1)} s feed time total
              </p>
              <ol className="so-op-list">
                {DATA.operations.map((op, i) => {
                  const isActive = i === step;
                  const isDone = i < step;
                  return (
                    <li key={op.id}>
                      <button
                        type="button"
                        className="so-op-btn"
                        data-op-id={op.id}
                        data-state={isActive ? "active" : isDone ? "done" : "pending"}
                        aria-current={isActive ? "step" : undefined}
                        onClick={() => {
                          setPlaying(false);
                          setStep(i);
                        }}
                      >
                        <span className="so-op-num">{i + 1}</span>
                        <span className="so-op-main">
                          <strong>{op.title}</strong>
                          <small>
                            {op.id} · {op.stitchIntervals} stitches · {op.feedSeconds.toFixed(2)} s
                          </small>
                        </span>
                        <span className="so-op-stage">{op.stage}</span>
                      </button>
                    </li>
                  );
                })}
              </ol>
            </aside>

            <section className="so-inspector" aria-label="Active sewing operation details">
              <div className="so-inspector-head">
                <div>
                  <p className="so-stage-kicker">
                    Operation {step + 1} of {total} · {active.stage}
                  </p>
                  <h2>{active.title}</h2>
                </div>
                <div className="so-op-pills">
                  <span>{active.stitchIntervals} intervals</span>
                  <span>{active.feedSeconds.toFixed(2)} s feed</span>
                  <span>Stack {active.stackMm.toFixed(2)} mm</span>
                </div>
              </div>

              {/* Prerequisites & next dependencies */}
              <div className="so-deps">
                <div className="so-dep-group">
                  <h3>Requires before sewing</h3>
                  <div className="so-dep-chips" data-deps="after">
                    {active.after.length ? (
                      active.after.map(depId => {
                        const target = opById.get(depId);
                        return (
                          <button
                            key={depId}
                            type="button"
                            className="so-chip"
                            onClick={() => target && (setPlaying(false), setStep(target.index))}
                          >
                            #{target ? target.index + 1 : "?"} {depId}
                          </button>
                        );
                      })
                    ) : (
                      <span className="so-chip-none">None — ready at start</span>
                    )}
                  </div>
                </div>
                <div className="so-dep-group">
                  <h3>Unlocks next</h3>
                  <div className="so-dep-chips" data-deps="unlocks">
                    {active.unlocks.length ? (
                      active.unlocks.map(nextId => {
                        const target = opById.get(nextId);
                        return (
                          <button
                            key={nextId}
                            type="button"
                            className="so-chip so-chip-next"
                            onClick={() => target && (setPlaying(false), setStep(target.index))}
                          >
                            → #{target ? target.index + 1 : "?"} {nextId}
                          </button>
                        );
                      })
                    ) : (
                      <span className="so-chip-none">Terminal seam on this sleeve</span>
                    )}
                  </div>
                </div>
              </div>

              {/* Physical endpoints table */}
              <div className="so-block">
                <h3>Physical endpoints &amp; paired allowances</h3>
                <div className="so-table-scroll">
                  <table className="so-table">
                    <thead>
                      <tr>
                        <th>Side</th>
                        <th>Panel / Edge</th>
                        <th>Arc length</th>
                        <th>Start (mm)</th>
                        <th>End (mm)</th>
                        <th>SA / Ply</th>
                      </tr>
                    </thead>
                    <tbody>
                      <tr>
                        <td><span className="so-badge-a">Side A</span></td>
                        <td><code>{active.sideA.reference}</code></td>
                        <td className="num">{active.sideA.lengthMm.toFixed(2)} mm</td>
                        <td className="num">({active.sideA.startMm[0].toFixed(1)}, {active.sideA.startMm[1].toFixed(1)})</td>
                        <td className="num">({active.sideA.endMm[0].toFixed(1)}, {active.sideA.endMm[1].toFixed(1)})</td>
                        <td className="num">{active.sideA.allowanceMm} mm · {active.sideA.thicknessMm} mm</td>
                      </tr>
                      <tr>
                        <td><span className="so-badge-b">Side B</span></td>
                        <td><code>{active.sideB.reference}</code></td>
                        <td className="num">{active.sideB.lengthMm.toFixed(2)} mm</td>
                        <td className="num">({active.sideB.startMm[0].toFixed(1)}, {active.sideB.startMm[1].toFixed(1)})</td>
                        <td className="num">({active.sideB.endMm[0].toFixed(1)}, {active.sideB.endMm[1].toFixed(1)})</td>
                        <td className="num">{active.sideB.allowanceMm} mm · {active.sideB.thicknessMm} mm</td>
                      </tr>
                    </tbody>
                  </table>
                </div>
                <p className="so-feed-note">
                  Differential feed ratio <code>b / a</code>:{" "}
                  <strong>
                    {active.feedRatioRange[0] === active.feedRatioRange[1]
                      ? active.feedRatioRange[0].toFixed(3)
                      : `${active.feedRatioRange[0].toFixed(3)} – ${active.feedRatioRange[1].toFixed(3)}`}
                  </strong>{" "}
                  · Max stitch <code>{active.maxStitchMm} mm</code> · Speed <code>{active.spm} spm</code> · Needle{" "}
                  <code>{active.needle}</code> · Thread <code>{active.thread}</code>
                </p>
              </div>

              {/* Stitch runs (backtacks + main seam) */}
              <div className="so-block">
                <h3>Directed stitch runs (backtack + seam)</h3>
                <div className="so-runs">
                  {active.runs.map((r, ri) => (
                    <div key={ri} className="so-run-chip">
                      <strong>{r.phase.replaceAll("_", " ")}</strong>
                      <span>
                        {r.direction} · st {r.fromStation}→{r.toStation} · {r.stitchIntervals} int ·{" "}
                        {r.feedSeconds.toFixed(2)} s
                      </span>
                    </div>
                  ))}
                </div>
              </div>

              {/* Downloads & machine-file links */}
              <div className="so-downloads-block">
                <h3>Machine &amp; planning files</h3>
                <div className="so-downloads">
                  <a className="so-dl-primary" href="/labs/bolero-sew-program.json" download="bolero-sew-program.json">
                    Directed stitch program (JSON · {DATA.totalDirectedStitches} stitches) ↓
                  </a>
                  <a className="so-dl-secondary" href="/labs/bolero-sew-plan.json" download="bolero-sew-plan.json">
                    Native sewing plan ({DATA.schema}) ↓
                  </a>
                  <a className="so-dl-secondary" href="/labs/stitch-back.dst" download="frankentextiles-bolero-back.dst">
                    Tajima embroidery machine file (.dst) ↓
                  </a>
                  <a className="so-dl-secondary" href="/patterns/chart-zouave_bolero.pdf" download="frankentextiles-zouave_bolero-A4.pdf">
                    Full-size A4 jacket pattern (.pdf) ↓
                  </a>
                </div>
              </div>

              {/* Honest scope & qualification receipts */}
              <div className="so-scope">
                <p>
                  <strong>Material thickness source:</strong> {DATA.material.name} ({DATA.material.thicknessMm} mm;{" "}
                  {DATA.material.method}).{" "}
                  <a href={DATA.material.sourceUrl} target="_blank" rel="noreferrer">
                    NISTIR 7282 (2005) ↗
                  </a>
                </p>
                <p className="so-flags">
                  <code>hardware_execution_authorized: {String(DATA.hardwareExecutionAuthorized)}</code> ·{" "}
                  <code>physical_seam_quality_validated: {String(DATA.physicalSeamQualityValidated)}</code> ·{" "}
                  <code>pattern_sha256: {DATA.patternSha256}</code>
                </p>
                <div className="so-cli">
                  <div className="so-cli-head">
                    <span>Reproduce identically with native CLI</span>
                    <button type="button" onClick={copyCli}>
                      {copied ? "Copied" : "Copy"}
                    </button>
                  </div>
                  <pre>{cliCommand}</pre>
                </div>
              </div>
            </section>
          </div>
        </div>
      </main>
      <SiteFooter />
    </>
  );
}
