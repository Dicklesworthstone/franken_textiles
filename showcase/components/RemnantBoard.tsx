"use client";

import { useEffect, useId, useMemo, useRef, useState } from "react";

type Vec2 = [number, number];
type Placement = {
  piece: string;
  instance: string;
  copy: number;
  bounds_mm: [number, number, number, number];
  cut_mm: Vec2[];
  grain_mm: [Vec2, Vec2];
  internal_cut_mm: [Vec2[], boolean][];
  drills_mm: [Vec2, number][];
};

export type RemnantPlan = {
  config: {
    job1Garment: string;
    job2Garment: string;
    job2Copies: number;
    sheetId: string;
    sheetWidthMm: number;
    sheetLengthMm: number;
    material: string;
    stepMm: number;
  };
  stock: {
    id: string;
    outlineMm: Vec2[];
    grossAreaM2: number;
    sourceUrl: string | null;
    materialName: string;
  };
  job1: {
    garment: string;
    requestSha256: string;
    externalCutAreaM2: number;
    utilization: number;
    remainingAreaM2: number;
    workUsed: number;
    candidates: number;
    placements: Placement[];
    occupiedBasis: string;
    occupiedCount: number;
    svg: string | null;
    residualJson: string;
  };
  freshRollJob2: {
    widthMm: number;
    lengthMm: number;
    stockAreaM2: number;
    externalCutAreaM2: number;
    utilization: number;
  };
  job2: {
    stockId: string;
    requestSha256: string;
    occupiedBasisIn: string;
    occupiedBasisOut: string;
    availableAreaM2: number;
    externalCutAreaM2: number;
    utilizationOnSheet: number;
    combinedCutAreaM2: number;
    combinedUtilization: number;
    workUsed: number;
    candidates: number;
    occupiedPolygons: Vec2[][];
    placements: Placement[];
    svg: string | null;
    residualJson: string;
  } | null;
  job2PatternJson: string;
  job2RequestJson: string;
  job2Refusal: string | null;
};

type PresetId = "bolero_fez" | "bolero_fez_2x" | "bolero_bolero_refusal" | "bolero_bolero_large";

const PRESETS: {
  id: PresetId;
  label: string;
  job1Garment: string;
  job2Garment: string;
  job2Copies: number;
  sheetId: string;
  sheetLengthMm: number;
  isRefusal?: boolean;
}[] = [
  {
    id: "bolero_fez",
    label: "Bolero → Fez cap",
    job1Garment: "zouave_bolero",
    job2Garment: "fez",
    job2Copies: 1,
    sheetId: "muslin-sheet-01",
    sheetLengthMm: 720,
  },
  {
    id: "bolero_fez_2x",
    label: "Bolero → 2× Fez caps",
    job1Garment: "zouave_bolero",
    job2Garment: "fez",
    job2Copies: 2,
    sheetId: "muslin-sheet-01",
    sheetLengthMm: 720,
  },
  {
    id: "bolero_bolero_refusal",
    label: "Bolero → 2nd Bolero (refusal)",
    job1Garment: "zouave_bolero",
    job2Garment: "zouave_bolero",
    job2Copies: 1,
    sheetId: "muslin-sheet-01",
    sheetLengthMm: 720,
    isRefusal: true,
  },
  {
    id: "bolero_bolero_large",
    label: "Bolero → 2nd Bolero (1100 mm sheet)",
    job1Garment: "zouave_bolero",
    job2Garment: "zouave_bolero",
    job2Copies: 1,
    sheetId: "muslin-sheet-02",
    sheetLengthMm: 1100,
  },
];

const GARMENT_NAMES: Record<string, string> = {
  zouave_bolero: "Zouave bolero",
  fez: "Fez cap",
  zouave_trousers: "Gathered trousers",
};

function ringPath(pts: Vec2[], closed = true): string {
  if (!pts.length) return "";
  let d = `M${pts[0][0].toFixed(2)} ${pts[0][1].toFixed(2)}`;
  for (let i = 1; i < pts.length; i++) {
    d += ` L${pts[i][0].toFixed(2)} ${pts[i][1].toFixed(2)}`;
  }
  return closed ? `${d} Z` : d;
}

function prettyInstance(id: string): string {
  return id.replace(/^tr_/, "").replace(/\.L$/, " left").replace(/\.R$/, " right");
}

function shortHash(basis: string): string {
  return basis.replace(/ftx-finite-stock-plan\/1:([0-9a-f]{10})[0-9a-f]+/g, "plan:$1…");
}

function RemnantSvg({
  plan,
  selected,
  onSelect,
}: {
  plan: RemnantPlan;
  selected: string;
  onSelect: (key: string) => void;
}) {
  const id = useId();
  const w = plan.config.sheetWidthMm;
  const h = plan.config.sheetLengthMm;
  const pad = Math.max(w, h) * 0.032;
  const job1Polys = plan.job2 ? plan.job2.occupiedPolygons : plan.job1.placements.map(p => p.cut_mm);

  return (
    <svg
      className="cut-marker remnant-svg"
      role="img"
      aria-label={`Finite stock ${plan.stock.id} (${w} by ${h} mm) with ${job1Polys.length} occupied Job 1 pieces and ${plan.job2 ? plan.job2.placements.length : 0} Job 2 remnant pieces`}
      viewBox={`${-pad} ${-pad} ${w + 2 * pad} ${h + 2 * pad}`}
    >
      <defs>
        <pattern id={`${id}grid`} width="25" height="25" patternUnits="userSpaceOnUse">
          <path d="M 25 0 H 0 V 25" fill="none" stroke="#a6b690" strokeWidth="1" />
        </pattern>
        <pattern id={`${id}hatch`} width="10" height="10" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
          <line x1="0" y1="0" x2="0" y2="10" stroke="#e4ebf2" strokeWidth="1.8" opacity="0.34" />
        </pattern>
        <pattern id={`${id}twill`} width="7" height="7" patternUnits="userSpaceOnUse">
          <path d="M-1 1L1-1 M0 7L7 0 M6 8L8 6" stroke="#fff2dd" strokeWidth="1" opacity="0.28" />
        </pattern>
        <marker id={`${id}arrow`} markerWidth="9" markerHeight="9" refX="8" refY="4.5" orient="auto" markerUnits="userSpaceOnUse">
          <path d="M0 0L9 4.5L0 9" fill="#f7efc8" />
        </marker>
      </defs>

      {/* Surviving usable stock mat */}
      <rect width={w} height={h} fill="#d6dfb7" stroke="#5b6d50" strokeWidth="2.5" />
      <rect width={w} height={h} fill={`url(#${id}grid)`} />
      <rect
        x="10"
        y="10"
        width={w - 20}
        height={h - 20}
        fill="none"
        stroke="#718664"
        strokeWidth="1.6"
        strokeDasharray="8 6"
      />

      {/* Real mm geometry flipped from y-up marker frame into SVG y-down */}
      <g transform={`translate(0 ${h}) scale(1 -1)`}>
        {/* Stock outline from exact returned marker */}
        <path d={ringPath(plan.stock.outlineMm)} fill="none" stroke="#45563d" strokeWidth="2" />

        {/* Job 1 locked occupied_mm polygons */}
        {job1Polys.map((poly, idx) => {
          const p = plan.job1.placements[idx];
          const key = `job1-${idx}`;
          const isSel = selected === key;
          const d = ringPath(poly);
          return (
            <g
              key={key}
              className="cut-panel remnant-occupied"
              data-job="1"
              data-instance={p?.instance ?? `occupied-${idx}`}
              onClick={() => onSelect(key)}
            >
              <title>
                Job 1 occupied · {p ? prettyInstance(p.instance) : `cut ${idx + 1}`} (locked in residual receipt)
              </title>
              <path d={d} fill={isSel ? "#3b567e" : "#2c4160"} fillOpacity="0.88" />
              <path
                d={d}
                fill={`url(#${id}hatch)`}
                stroke={isSel ? "#f7efc8" : "#182840"}
                strokeWidth={isSel ? 2.2 : 1.1}
                vectorEffect="non-scaling-stroke"
              />
            </g>
          );
        })}

        {/* Job 2 newly cut pieces inside surviving remnant */}
        {plan.job2?.placements.map((piece, idx) => {
          const key = `job2-${idx}`;
          const isSel = selected === key;
          const d = ringPath(piece.cut_mm);
          const grainD = ringPath(piece.grain_mm, false);
          return (
            <g
              key={key}
              className="cut-panel remnant-new"
              data-job="2"
              data-instance={piece.instance}
              data-copy={piece.copy}
              onClick={() => onSelect(key)}
            >
              <title>
                Job 2 remnant cut · {prettyInstance(piece.instance)}
                {plan.config.job2Copies > 1 ? ` · garment ${piece.copy + 1}` : ""}
              </title>
              <path d={d} fill={isSel ? "#c4562a" : "#9c4120"} />
              <path
                d={d}
                fill={`url(#${id}twill)`}
                stroke={isSel ? "#fff2c6" : "#5e220e"}
                strokeWidth={isSel ? 2.2 : 1.1}
                vectorEffect="non-scaling-stroke"
              />
              <path
                d={grainD}
                fill="none"
                stroke="#f7efc8"
                strokeWidth="2.2"
                strokeDasharray="9 5"
                markerEnd={`url(#${id}arrow)`}
              />
              {piece.internal_cut_mm.map(([pts, closed], j) => (
                <path key={j} d={ringPath(pts, closed)} fill="none" stroke="#f7e6b8" strokeWidth="1.6" />
              ))}
              {piece.drills_mm.map(([pt, diam], j) => (
                <circle key={j} cx={pt[0]} cy={pt[1]} r={diam / 2} fill="#f7e6b8" />
              ))}
            </g>
          );
        })}
      </g>
    </svg>
  );
}

export default function RemnantBoard() {
  const [presetId, setPresetId] = useState<PresetId>("bolero_fez");
  const preset = PRESETS.find(p => p.id === presetId) ?? PRESETS[0];
  const [plan, setPlan] = useState<RemnantPlan | null>(null);
  const [selected, setSelected] = useState("job2-0");
  const [loadError, setLoadError] = useState("");
  const [solverError, setSolverError] = useState("");
  const [retry, setRetry] = useState(0);
  const [downloads, setDownloads] = useState<{
    plan: RemnantPlan;
    svgUrl: string | null;
    residualUrl: string;
    patternUrl: string;
    replayUrl: string;
  }>();
  const workerRef = useRef<Worker | null>(null);
  const serial = useRef(0);

  useEffect(() => () => {
    workerRef.current?.terminate();
    workerRef.current = null;
  }, []);

  useEffect(() => {
    const id = ++serial.current;
    const controller = new AbortController();
    setLoadError("");
    setSolverError("");

    const accept = (result: RemnantPlan) => {
      if (controller.signal.aborted) return;
      setPlan(result);
      setSelected(result.job2?.placements.length ? "job2-0" : "job1-0");
      setLoadError("");
      setSolverError("");
    };

    if (!retry && presetId === "bolero_fez") {
      fetch("/labs/remnant-starter.json", { signal: controller.signal })
        .then(async res => {
          if (!res.ok) throw new Error("starter-missing");
          accept(await res.json());
        })
        .catch(err => {
          if (controller.signal.aborted) return;
          // Fall back cleanly to live WASM worker if starter snapshot is absent.
          runWorker(id, controller.signal, preset, accept);
        });
    } else {
      runWorker(id, controller.signal, preset, accept);
    }

    return () => controller.abort();
  }, [presetId, preset, retry]);

  function runWorker(
    id: number,
    signal: AbortSignal,
    activePreset: (typeof PRESETS)[number],
    accept: (r: RemnantPlan) => void,
  ) {
    try {
      workerRef.current ??= new Worker("/labs/remnant-worker.mjs", { type: "module" });
      workerRef.current.onmessage = ({ data }) => {
        if (signal.aborted || data.id !== id) return;
        if (data.loadError) {
          setLoadError(data.loadError);
        } else if (data.error) {
          setSolverError(data.error);
        } else {
          accept(data.result);
        }
      };
      workerRef.current.onerror = () => {
        if (signal.aborted) return;
        workerRef.current?.terminate();
        workerRef.current = null;
        setLoadError("The remnant cutting engine could not load. Try again or open the full workbench.");
      };
      workerRef.current.postMessage({
        id,
        params: {
          job1Garment: activePreset.job1Garment,
          job2Garment: activePreset.job2Garment,
          job2Copies: activePreset.job2Copies,
          sheetId: activePreset.sheetId,
          sheetLengthMm: activePreset.sheetLengthMm,
        },
      });
    } catch (reason) {
      if (!signal.aborted) {
        setLoadError(reason instanceof Error ? reason.message : String(reason));
      }
    }
  }

  const ready =
    Boolean(plan) &&
    plan!.config.job1Garment === preset.job1Garment &&
    plan!.config.job2Garment === preset.job2Garment &&
    plan!.config.job2Copies === preset.job2Copies &&
    plan!.config.sheetId === preset.sheetId &&
    plan!.config.sheetLengthMm === preset.sheetLengthMm &&
    !loadError &&
    !solverError;

  useEffect(() => {
    if (!ready || !plan) return;
    const svgText = plan.job2 ? plan.job2.svg : plan.job1.svg;
    const urls = {
      plan,
      svgUrl: svgText ? URL.createObjectURL(new Blob([svgText], { type: "image/svg+xml" })) : null,
      residualUrl: URL.createObjectURL(new Blob([plan.job2?.residualJson ?? plan.job1.residualJson], { type: "application/json" })),
      patternUrl: URL.createObjectURL(new Blob([plan.job2PatternJson], { type: "application/json" })),
      replayUrl: URL.createObjectURL(new Blob([plan.job2RequestJson], { type: "application/json" })),
    };
    setDownloads(urls);
    return () => {
      if (urls.svgUrl) URL.revokeObjectURL(urls.svgUrl);
      URL.revokeObjectURL(urls.residualUrl);
      URL.revokeObjectURL(urls.patternUrl);
      URL.revokeObjectURL(urls.replayUrl);
    };
  }, [ready, plan]);

  const inspectOptions = useMemo(() => {
    if (!plan) return [];
    const list: { key: string; label: string; bounds: [number, number, number, number] }[] = [];
    plan.job2?.placements.forEach((p, i) => {
      list.push({
        key: `job2-${i}`,
        label: `Job 2 · ${prettyInstance(p.instance)}${plan.config.job2Copies > 1 ? ` (copy ${p.copy + 1})` : ""}`,
        bounds: p.bounds_mm,
      });
    });
    plan.job1.placements.forEach((p, i) => {
      list.push({
        key: `job1-${i}`,
        label: `Job 1 locked · ${prettyInstance(p.instance)}`,
        bounds: p.bounds_mm,
      });
    });
    return list;
  }, [plan]);

  const currentPiece = inspectOptions.find(o => o.key === selected) ?? inspectOptions[0];
  const refused = Boolean(ready && plan?.job2Refusal);
  const errorMsg = loadError || solverError;

  return (
    <div className="cut-lab remnant-lab">
      <div className="cut-board" aria-busy={!ready && !errorMsg}>
        <div className="cut-board-top">
          <span>
            {ready && plan
              ? `Stock ${plan.stock.id} · ${plan.config.sheetWidthMm} × ${plan.config.sheetLengthMm} mm`
              : "Finite remnant planner · Rust WASM"}
          </span>
          <span>{refused ? "Job 2 refused · remnant preserved" : "Two-job receipt chain"}</span>
        </div>

        <div className="cut-board-view">
          {plan && <RemnantSvg plan={plan} selected={selected} onSelect={setSelected} />}
          {!ready && (
            <div className="cut-progress" role="status">
              {errorMsg || "Running Job 1 → residual receipt → Job 2 in local Rust WASM…"}
              {errorMsg && (
                <button type="button" onClick={() => setRetry(n => n + 1)}>
                  Retry
                </button>
              )}
            </div>
          )}
        </div>

        <div className="remnant-legend" aria-label="Remnant map legend">
          <span><i className="sw-remnant" /> Usable sheet</span>
          <span><i className="sw-job1" /> Job 1 · Occupied (locked)</span>
          <span><i className="sw-job2" /> Job 2 · Cut from remnant</span>
        </div>

        <div className="cut-inspect">
          {plan && currentPiece && (
            <>
              <label>
                Inspect piece
                <select
                  aria-label="Inspect remnant piece"
                  value={currentPiece.key}
                  onChange={e => setSelected(e.target.value)}
                >
                  {inspectOptions.map(o => (
                    <option key={o.key} value={o.key}>
                      {o.label}
                    </option>
                  ))}
                </select>
              </label>
              <span>
                {Math.round(currentPiece.bounds[2] - currentPiece.bounds[0])} ×{" "}
                {Math.round(currentPiece.bounds[3] - currentPiece.bounds[1])} mm
              </span>
            </>
          )}
        </div>
      </div>

      <div className="cut-controls remnant-controls">
        <h3>Cut a second garment from the real remnant</h3>
        <p>
          Job 1 cuts a garment on an individually identified finite sheet and emits a frozen{" "}
          <code>residual_json</code> receipt carrying the exact stock ID and exterior cut polygons as{" "}
          <code>occupied_mm</code>. Job 2 plans strictly inside the surviving cloth — never cutting
          inside Job 1 or silently pulling a fresh roll.
        </p>

        <fieldset>
          <legend>Two-job finite-stock scenario</legend>
          <div className="remnant-presets" role="group" aria-label="Remnant cutting scenarios">
            {PRESETS.map(p => (
              <button
                key={p.id}
                type="button"
                aria-pressed={presetId === p.id}
                data-refusal={p.isRefusal ? "true" : undefined}
                onClick={() => setPresetId(p.id)}
              >
                {p.label}
              </button>
            ))}
          </div>
        </fieldset>

        {refused && plan?.job2Refusal && (
          <div className="remnant-refusal" role="alert">
            <strong>Job 2 refused on {plan.stock.id}</strong>
            <p>
              <code>{plan.job2Refusal}</code>
            </p>
            <p>
              Job 1&apos;s {plan.job1.occupiedCount} occupied cutouts remain locked on{" "}
              <code>{plan.stock.id}</code>; Job 2 cannot overlap them or assume a fresh roll.
            </p>
          </div>
        )}

        <dl className="cut-metrics remnant-metrics" aria-live="polite">
          <div>
            <dt>Physical stock ID</dt>
            <dd className="remnant-dd-mono">{ready && plan ? plan.stock.id : "—"}</dd>
          </div>
          <div>
            <dt>Fresh roll avoided</dt>
            <dd>
              {ready && plan
                ? plan.job2
                  ? `${(Math.ceil(plan.freshRollJob2.lengthMm) / 1000).toFixed(3)} m`
                  : "Refused"
                : "—"}
            </dd>
          </div>
          <div>
            <dt>Sheet utilization</dt>
            <dd>
              {ready && plan
                ? plan.job2
                  ? `${(plan.job1.utilization * 100).toFixed(1)}% → ${(plan.job2.combinedUtilization * 100).toFixed(1)}%`
                  : `${(plan.job1.utilization * 100).toFixed(1)}%`
                : "—"}
            </dd>
          </div>
          <div>
            <dt>Fresh roll area avoided</dt>
            <dd>
              {ready && plan
                ? plan.job2
                  ? `${plan.freshRollJob2.stockAreaM2.toFixed(3)} m²`
                  : "0.000 m²"
                : "—"}
            </dd>
          </div>
        </dl>

        {ready && plan && (
          <div className="remnant-receipt">
            <div>
              <strong>Job 1</strong> ({GARMENT_NAMES[plan.job1.garment] ?? plan.job1.garment}):{" "}
              {plan.job1.occupiedCount} pieces locked ({plan.job1.externalCutAreaM2.toFixed(3)} m²)
            </div>
            <div>
              <strong>Job 2</strong> ({plan.config.job2Copies}×{" "}
              {GARMENT_NAMES[plan.config.job2Garment] ?? plan.config.job2Garment}):{" "}
              {plan.job2
                ? `${plan.job2.placements.length} pieces cut inside surviving ${plan.job2.availableAreaM2.toFixed(3)} m²`
                : "0 pieces placed (refused — see diagnostic above)"}
            </div>
            <div className="remnant-basis">
              <code>occupied_basis</code>:{" "}
              <code>{shortHash(plan.job2 ? plan.job2.occupiedBasisOut : plan.job1.occupiedBasis)}</code>
            </div>
          </div>
        )}

        {ready && plan && downloads?.plan === plan && (
          <div className="remnant-downloads">
            {downloads.svgUrl && (
              <a
                className="cut-save"
                href={downloads.svgUrl}
                download={`${plan.stock.id}-${plan.job2 ? `job2-${plan.config.job2Garment}` : `job1-${plan.config.job1Garment}`}.svg`}
              >
                {plan.job2 ? "Download Job 2 remnant SVG" : "Download Job 1 stock SVG"}{" "}
                <span aria-hidden="true">↓</span>
              </a>
            )}
            <a
              className="cut-save remnant-save-secondary"
              href={downloads.residualUrl}
              download={`${plan.stock.id}-residual-request.json`}
            >
              Download updated remnant <span aria-hidden="true">↓</span>
            </a>
            <a
              className="cut-save remnant-save-secondary"
              href={downloads.replayUrl}
              download={`${plan.stock.id}-job2-request.json`}
            >
              Download Job 2 request <span aria-hidden="true">↓</span>
            </a>
          </div>
        )}

        <p className="cut-source">
          {ready && plan?.stock.sourceUrl && (
            <>
              <a href={plan.stock.sourceUrl} target="_blank" rel="noreferrer">
                Supplier width ↗
              </a>{" "}
            </>
          )}
          Single-ply {ready && plan ? plan.stock.materialName : "cotton muslin"}, 4 mm cut gap and
          10 mm boundary margin.{ready && plan && <> Search grid {plan.config.stepMm} mm.</>}
          <br />
          Full-size A4 patterns{" "}
          <a href="/patterns/chart-zouave_bolero.pdf" download>Bolero</a>
          {preset.job2Garment === "fez" && <>{" · "}<a href="/patterns/chart-fez.pdf" download>Fez</a></>}
          {ready && plan && downloads?.plan === plan && (
            <>
              <br />
              <a href={downloads.patternUrl} download={`${plan.config.job2Garment}-pattern.json`}>
                Job 2 pattern JSON
              </a>{" and the Job 2 request replay with "}
              <code>ftx-materials stock-plan PATTERN REQUEST</code>.
            </>
          )}
        </p>
      </div>
    </div>
  );
}
