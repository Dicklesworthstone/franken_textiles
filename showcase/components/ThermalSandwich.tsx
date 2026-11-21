"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import SiteHeader from "./site/SiteHeader";
import SiteFooter from "./site/SiteFooter";
import Crumbs from "./site/Crumbs";
import CodeBlock from "./site/CodeBlock";
import { workbench } from "./site/links";
import "./thermal-sandwich.css";

interface MaterialOption {
  id: string;
  short: string;
  construction: string;
  rct: number;
  thicknessMm: number;
  citation: string;
  swatch: string;
}

// Verified built-in catalog records that pass `ftx materials thermal-steady` with `allow_estimates: false`.
const sheets: readonly MaterialOption[] = [
  { id: "zagreb/neoprene-2025/m3nob5", short: "M3NOb5 · Unlined neoprene (5.52 mm)", construction: "Closed-cell neoprene sheet", rct: 0.172, thicknessMm: 5.5167, citation: "Potočić Matković et al. 2025", swatch: "#365063" },
  { id: "zagreb/neoprene-2025/m1no5", short: "M1NO5 · Unlined neoprene (5.15 mm)", construction: "Closed-cell neoprene sheet", rct: 0.0915, thicknessMm: 5.1467, citation: "Potočić Matković et al. 2025", swatch: "#476972" },
  { id: "zagreb/neoprene-2025/m2bs5", short: "M2BS5 · Double-lined neoprene (6.98 mm)", construction: "Both-sided lined laminate", rct: 0.1175, thicknessMm: 6.9833, citation: "Potočić Matković et al. 2025", swatch: "#5a6b52" },
  { id: "zagreb/neoprene-2025/m3noa3", short: "M3NOa3 · Unlined neoprene (3.29 mm)", construction: "Closed-cell neoprene sheet", rct: 0.0535, thicknessMm: 3.2933, citation: "Potočić Matković et al. 2025", swatch: "#557180" },
  { id: "zagreb/neoprene-2025/m4os7", short: "M4OS7 · One-sided lined neoprene (7.20 mm)", construction: "One-sided lined laminate", rct: 0.198, thicknessMm: 7.1967, citation: "Potočić Matković et al. 2025", swatch: "#2c4654" },
  { id: "zagreb/leather-2022/cr-d-h-back", short: "Cr-D-H · Bovine leather, back (1.71 mm)", construction: "Chrome-tanned cowhide", rct: 0.019, thicknessMm: 1.71, citation: "Zagreb bovine leather 2022", swatch: "#8a5638" },
];

type FrozenLayer = {
  id: string;
  allow_estimates: boolean;
  expected_sha256: string;
  record: {
    id: string;
    name: string;
    form: string;
    properties: {
      thermal_resistance?: { value: number; unit: string; method: string; source: string };
      thickness?: { value: number; unit: string; method: string };
      areal_density?: { value: number; unit: string };
    };
    sources?: Record<string, { reference: string; note: string }>;
  };
};

interface SteadyResult {
  schema: string;
  model: string;
  power_w: number;
  request_sha256: string;
  comfort_claim: boolean;
  empirical_validation_claim: boolean;
  storage_required: boolean;
  uncertainty_propagated: boolean;
  not_simulated: string[];
  request: {
    schema: string;
    area_m2: number;
    inside_c: number;
    outside_c: number;
    inside_resistance_m2_k_w: number;
    outside_resistance_m2_k_w: number;
    interface_resistance_m2_k_w: number[];
    layers: FrozenLayer[];
  };
  state: {
    flux_w_m2: number;
    layer_faces_c: [number, number][];
    max_flux_residual_w_m2: number;
    max_temperature_residual_c: number;
  };
}

const AREA_M2 = 0.6;
const R_IN = 0.03;
const R_OUT = 0.05;

function tempColor(temp: number, lo: number, hi: number): string {
  const span = Math.max(1e-6, hi - lo);
  const t = Math.max(0, Math.min(1, (temp - lo) / span));
  // Cool slate (#4c85a6) -> parchment (#dfd2b8) -> warm copper (#d46a3f)
  const r = Math.round(76 + (212 - 76) * t);
  const g = Math.round(133 + (106 - 133) * t);
  const b = Math.round(166 + (63 - 166) * t);
  return `rgb(${r}, ${g}, ${b})`;
}

function fmt(n: number, digits = 2): string {
  return (Math.abs(n) < 0.5 * 10 ** -digits ? 0 : n).toFixed(digits);
}

function IsometricStack({
  layerIds,
  result,
  refusal,
  insideC,
  outsideC,
  hasInterface,
  interfaceR,
  explode,
}: {
  layerIds: string[];
  result?: SteadyResult;
  refusal?: string;
  insideC: number;
  outsideC: number;
  hasInterface: boolean;
  interfaceR: number;
  explode: number;
}) {
  const lo = Math.min(insideC, outsideC);
  const hi = Math.max(insideC, outsideC);
  const gap = 14 + explode * 34;
  const cx = 190;
  const rx = 112;
  const ry = 34;

  const isoDiamond = (y: number) =>
    `M ${cx} ${y - ry} L ${cx + rx} ${y} L ${cx} ${y + ry} L ${cx - rx} ${y} Z`;

  const slab = (yTop: number, thicknessPx: number, topFill: string, leftFill: string, rightFill: string) => {
    const yBot = yTop + thicknessPx;
    return (
      <>
        <path d={`M ${cx - rx} ${yTop} L ${cx} ${yTop + ry} L ${cx} ${yBot + ry} L ${cx - rx} ${yBot} Z`} fill={leftFill} stroke="#101a20" strokeWidth="1.2" />
        <path d={`M ${cx + rx} ${yTop} L ${cx} ${yTop + ry} L ${cx} ${yBot + ry} L ${cx + rx} ${yBot} Z`} fill={rightFill} stroke="#101a20" strokeWidth="1.2" />
        <path d={isoDiamond(yTop)} fill={topFill} stroke="#f3ede0" strokeOpacity="0.45" strokeWidth="1.1" />
      </>
    );
  };

  // Arrange stack top-to-bottom: Outside reservoir (top) -> Layer 2 (optional) -> Interface -> Layer 1 -> Inside reservoir (bottom)
  const layers = layerIds.map((id, idx) => {
    const meta = sheets.find(s => s.id === id) ?? sheets[0];
    const faces = result?.state.layer_faces_c[idx];
    return { meta, idx, faces };
  });

  // Visual top-to-bottom order puts outer layer at top, inner layer at bottom.
  const ordered = [...layers].reverse();
  const baseTop = ordered.length === 2 ? 78 - explode * 18 : 122 - explode * 10;
  const topSlabH = ordered[0] ? Math.max(12, Math.min(34, ordered[0].meta.thicknessMm * 4.2)) : 20;
  const botSlabH = ordered[1] ? Math.max(12, Math.min(34, ordered[1].meta.thicknessMm * 4.2)) : 0;

  const yOuterFilm = baseTop;
  const yTopLayer = yOuterFilm + 18 + gap * 0.65;
  const yInterface = yTopLayer + topSlabH + gap * 0.9;
  const yBotLayer = ordered.length === 2 ? yInterface + gap * 0.9 : yTopLayer;
  const yInnerFilm = (ordered.length === 2 ? yBotLayer + botSlabH : yTopLayer + topSlabH) + 18 + gap * 0.65;

  return (
    <svg
      className="th-iso-svg"
      viewBox="0 0 430 370"
      role="img"
      aria-label={`Explodable thermal stack with ${layerIds.length} measured sheet${layerIds.length > 1 ? "s" : ""}`}
    >
      {/* Outside air / boundary film plane */}
      <g opacity="0.88">
        <path d={isoDiamond(yOuterFilm)} fill={tempColor(outsideC, lo, hi)} fillOpacity="0.22" stroke={tempColor(outsideC, lo, hi)} strokeDasharray="5 4" strokeWidth="1.4" />
        <text x={cx + rx + 10} y={yOuterFilm + 4} fill="#d7e0dc" fontSize="10.5" fontFamily="var(--mono)">
          OUTSIDE {fmt(outsideC, 1)} °C
        </text>
        <text x={cx + rx + 10} y={yOuterFilm + 17} fill="#94a8a8" fontSize="9.5" fontFamily="var(--mono)">
          film {R_OUT} m²·K/W
        </text>
      </g>

      {/* Bottom (Inside) reservoir film plane drawn behind lower slab when collapsed, or clearly beneath */}
      <g opacity="0.9">
        <path d={isoDiamond(yInnerFilm)} fill={tempColor(insideC, lo, hi)} fillOpacity="0.22" stroke={tempColor(insideC, lo, hi)} strokeDasharray="5 4" strokeWidth="1.4" />
        <text x={cx + rx + 10} y={yInnerFilm + 4} fill="#ffd4b3" fontSize="10.5" fontFamily="var(--mono)">
          INSIDE {fmt(insideC, 1)} °C
        </text>
        <text x={cx + rx + 10} y={yInnerFilm + 17} fill="#94a8a8" fontSize="9.5" fontFamily="var(--mono)">
          film {R_IN} m²·K/W
        </text>
      </g>

      {/* Lower layer (Layer 1 — inside sheet when 2 layers are active) */}
      {ordered.length === 2 && (() => {
        const l1 = ordered[1];
        const tIn = l1.faces?.[0];
        const tOut = l1.faces?.[1];
        const topCol = tOut !== undefined ? tempColor(tOut, lo, hi) : l1.meta.swatch;
        const sideCol = tIn !== undefined ? tempColor((tIn + (tOut ?? tIn)) / 2, lo, hi) : l1.meta.swatch;
        return (
          <g>
            {slab(yBotLayer, botSlabH, topCol, sideCol, "#25343d")}
            <text x={cx} y={yBotLayer + 4} textAnchor="middle" fill="#fffdf6" fontSize="11" fontWeight="600" fontFamily="var(--sans)">
              Layer 1 · {l1.meta.id.split("/").pop()?.toUpperCase()}
            </text>
            <text x={cx + rx + 10} y={yBotLayer + 6} fill="#fff7ea" fontSize="10.5" fontFamily="var(--mono)">
              {tOut !== undefined ? `${fmt(tOut, 2)} °C` : "—"}
            </text>
            <text x={cx + rx + 10} y={yBotLayer + botSlabH + 10} fill="#ffbd95" fontSize="10.5" fontFamily="var(--mono)">
              {tIn !== undefined ? `${fmt(tIn, 2)} °C` : "—"}
            </text>
            <text x={12} y={yBotLayer + botSlabH * 0.6} fill="#c8d2d0" fontSize="10" fontFamily="var(--mono)">
              {l1.meta.thicknessMm.toFixed(2)} mm
            </text>
          </g>
        );
      })()}

      {/* Interface gap marker between Layer 1 and Layer 2 */}
      {ordered.length === 2 && (
        <g>
          <path
            d={isoDiamond(yInterface)}
            fill={hasInterface ? "#ffba8c" : "#d94e34"}
            fillOpacity={hasInterface ? "0.14" : "0.28"}
            stroke={hasInterface ? "#ffba8c" : "#ff8d75"}
            strokeWidth="1.5"
            strokeDasharray={hasInterface ? "3 3" : "none"}
          />
          <text
            x={cx}
            y={yInterface + 4}
            textAnchor="middle"
            fill={hasInterface ? "#ffe1c4" : "#ffd6cc"}
            fontSize="10.5"
            fontWeight="600"
            fontFamily="var(--mono)"
          >
            {hasInterface
              ? `INTERFACE R = ${interfaceR.toFixed(3)} m²·K/W`
              : "INTERFACE OMITTED · BLOCKED"}
          </text>
        </g>
      )}

      {/* Top layer (Layer 2 if 2 layers, else Layer 1) */}
      {(() => {
        const top = ordered[0];
        const tIn = top.faces?.[0];
        const tOut = top.faces?.[1];
        const topCol = tOut !== undefined ? tempColor(tOut, lo, hi) : top.meta.swatch;
        const sideCol = tIn !== undefined ? tempColor((tIn + (tOut ?? tIn)) / 2, lo, hi) : top.meta.swatch;
        return (
          <g>
            {slab(yTopLayer, topSlabH, topCol, sideCol, "#25343d")}
            <text x={cx} y={yTopLayer + 4} textAnchor="middle" fill="#fffdf6" fontSize="11" fontWeight="600" fontFamily="var(--sans)">
              Layer {top.idx + 1} · {top.meta.id.split("/").pop()?.toUpperCase()}
            </text>
            <text x={cx + rx + 10} y={yTopLayer + 6} fill="#d7e6ec" fontSize="10.5" fontFamily="var(--mono)">
              {tOut !== undefined ? `${fmt(tOut, 2)} °C` : "—"}
            </text>
            <text x={cx + rx + 10} y={yTopLayer + topSlabH + 10} fill="#ffbd95" fontSize="10.5" fontFamily="var(--mono)">
              {tIn !== undefined ? `${fmt(tIn, 2)} °C` : "—"}
            </text>
            <text x={12} y={yTopLayer + topSlabH * 0.6} fill="#c8d2d0" fontSize="10" fontFamily="var(--mono)">
              {top.meta.thicknessMm.toFixed(2)} mm
            </text>
          </g>
        );
      })()}

      {/* Heat flux direction arrow */}
      {!refusal && result && Math.abs(result.state.flux_w_m2) > 1e-6 && (
        <g>
          <line
            x1="44"
            y1={result.state.flux_w_m2 > 0 ? yInnerFilm - 10 : yOuterFilm + 10}
            x2="44"
            y2={result.state.flux_w_m2 > 0 ? yOuterFilm + 10 : yInnerFilm - 10}
            stroke="#ffba8c"
            strokeWidth="2"
          />
          <polygon
            points={
              result.state.flux_w_m2 > 0
                ? `44,${yOuterFilm + 4} 38,${yOuterFilm + 15} 50,${yOuterFilm + 15}`
                : `44,${yInnerFilm - 4} 38,${yInnerFilm - 15} 50,${yInnerFilm - 15}`
            }
            fill="#ffba8c"
          />
          <text x="12" y={yOuterFilm + 3} fill="#ffba8c" fontSize="9.5" fontFamily="var(--mono)">
            q = {fmt(result.state.flux_w_m2, 1)} W/m²
          </text>
        </g>
      )}
    </svg>
  );
}

function TemperatureProfile({
  result,
  insideC,
  outsideC,
}: {
  result?: SteadyResult;
  insideC: number;
  outsideC: number;
}) {
  if (!result) {
    return (
      <div className="th-profile-wrap">
        <h2>Steady temperature profile</h2>
        <p className="th-profile-note">
          No profile is plotted while the request is refused. Supply an explicit interface resistance to solve a layered stack.
        </p>
      </div>
    );
  }
  const faces = result.state.layer_faces_c;
  const pts: { x: number; t: number; label: string }[] = [{ x: 0, t: insideC, label: "In" }];
  if (faces.length === 1) {
    pts.push({ x: 0.18, t: faces[0][0], label: "L1 in" });
    pts.push({ x: 0.82, t: faces[0][1], label: "L1 out" });
  } else {
    pts.push({ x: 0.14, t: faces[0][0], label: "L1 in" });
    pts.push({ x: 0.44, t: faces[0][1], label: "L1 out" });
    pts.push({ x: 0.56, t: faces[1][0], label: "L2 in" });
    pts.push({ x: 0.86, t: faces[1][1], label: "L2 out" });
  }
  pts.push({ x: 1, t: outsideC, label: "Out" });

  const lo = Math.min(...pts.map(p => p.t)) - 2;
  const hi = Math.max(...pts.map(p => p.t)) + 2;
  const span = Math.max(1, hi - lo);
  const px = (x: number) => 34 + x * 244;
  const py = (t: number) => 178 - ((t - lo) / span) * 144;
  const poly = pts.map(p => `${px(p.x).toFixed(1)},${py(p.t).toFixed(1)}`).join(" ");

  return (
    <div className="th-profile-wrap">
      <h2>Layer-by-layer temperature drop (°C)</h2>
      <svg
        className="th-profile-svg"
        viewBox="0 0 300 218"
        role="img"
        aria-label="Steady temperature profile from inside reservoir to outside reservoir"
      >
        {/* Shaded layer bands */}
        {faces.length === 1 ? (
          <rect x={px(0.18)} y="24" width={px(0.82) - px(0.18)} height="158" fill="#2c4452" fillOpacity="0.45" />
        ) : (
          <>
            <rect x={px(0.14)} y="24" width={px(0.44) - px(0.14)} height="158" fill="#2c4452" fillOpacity="0.45" />
            <rect x={px(0.44)} y="24" width={px(0.56) - px(0.44)} height="158" fill="#a85532" fillOpacity="0.24" />
            <rect x={px(0.56)} y="24" width={px(0.86) - px(0.56)} height="158" fill="#3a5248" fillOpacity="0.45" />
          </>
        )}
        <line x1="34" y1="182" x2="278" y2="182" stroke="#425560" strokeWidth="1" />
        <line x1="34" y1="24" x2="34" y2="182" stroke="#425560" strokeWidth="1" />
        <polyline points={poly} fill="none" stroke="#ffba8c" strokeWidth="2.4" strokeLinejoin="round" />
        {pts.map((p, idx) => (
          <g key={p.label}>
            <circle cx={px(p.x)} cy={py(p.t)} r="3.6" fill="#fff7ea" stroke="#c85f33" strokeWidth="1.5" />
            <text
              x={px(p.x)}
              y={py(p.t) + (idx % 2 === 0 ? -9 : 14)}
              textAnchor="middle"
              fill="#f3efe3"
              fontSize="9.5"
              fontFamily="var(--mono)"
            >
              {fmt(p.t, 1)}°
            </text>
            <text x={px(p.x)} y="198" textAnchor="middle" fill="#9fb0b0" fontSize="8.5" fontFamily="var(--mono)">
              {p.label}
            </text>
          </g>
        ))}
      </svg>
      <p className="th-profile-note">
        Inside film ({R_IN} m²·K/W) → {faces.length === 2 ? "Layer 1 → explicit contact jump → Layer 2" : "measured sheet"} → outside film ({R_OUT} m²·K/W).
      </p>
    </div>
  );
}

export default function ThermalSandwich() {
  const [layer1, setLayer1] = useState(sheets[0].id);
  const [layer2, setLayer2] = useState<string>("none");
  const [hasInterface, setHasInterface] = useState(true);
  const [interfaceR, setInterfaceR] = useState(0.02);
  const [insideC, setInsideC] = useState(35);
  const [outsideC, setOutsideC] = useState(20);
  const [explode, setExplode] = useState(0.45);
  const request = useMemo(() => {
    const layers =
      layer2 === "none"
        ? [{ id: layer1, allow_estimates: false }]
        : [
            { id: layer1, allow_estimates: false },
            { id: layer2, allow_estimates: false },
          ];
    return {
      schema: "ftx-thermal-steady/1",
      layers,
      interface_resistance_m2_k_w:
        layer2 === "none" ? [] : hasInterface ? [Number(interfaceR.toFixed(4))] : [],
      area_m2: AREA_M2,
      inside_c: insideC,
      outside_c: outsideC,
      inside_resistance_m2_k_w: R_IN,
      outside_resistance_m2_k_w: R_OUT,
    };
  }, [layer1, layer2, hasInterface, interfaceR, insideC, outsideC]);
  const [reply, setReply] = useState<{
    request: typeof request;
    source: "starter" | "wasm";
    result?: SteadyResult;
    refusal?: string;
  }>();
  const active = reply?.request === request ? reply : undefined;
  const result = active?.result;
  const refusal = active?.refusal ?? "";
  const sourceTag = active?.source ?? "loading";
  const [downloads, setDownloads] = useState<{ result: SteadyResult; requestUrl: string; reportUrl: string }>();

  const worker = useRef<Worker | null>(null);
  const serial = useRef(0);
  const isFixtureDefault = layer1 === sheets[0].id && layer2 === "none" && insideC === 35 && outsideC === 20;

  useEffect(() => () => { worker.current?.terminate(); worker.current = null; }, []);

  useEffect(() => {
    const id = ++serial.current;
    const controller = new AbortController();

    if (isFixtureDefault) {
      fetch("/labs/thermal-starter.json", { signal: controller.signal })
        .then(r => (r.ok ? r.json() : Promise.reject(new Error("Starter fixture unavailable"))))
        .then((data: SteadyResult) => {
          if (controller.signal.aborted || id !== serial.current) return;
          setReply({ request, result: data, source: "starter" });
        })
        .catch(() => {
          if (!controller.signal.aborted && id === serial.current) dispatchWasm();
        });
    } else {
      dispatchWasm();
    }

    function dispatchWasm() {
      try {
        worker.current ??= new Worker("/labs/thermal-worker.mjs", { type: "module" });
        worker.current.onmessage = ({ data }: MessageEvent<{ id: number; result?: SteadyResult; error?: string }>) => {
          if (data.id !== serial.current) return;
          if (data.error) {
            setReply({ request, refusal: data.error, source: "wasm" });
          } else if (data.result) {
            setReply({ request, result: data.result, source: "wasm" });
          }
        };
        worker.current.onerror = () => {
          worker.current?.terminate();
          worker.current = null;
          if (id === serial.current) {
            setReply({ request, refusal: "The local thermal WASM engine could not load.", source: "wasm" });
          }
        };
        worker.current.postMessage({ id, request });
      } catch (err) {
        if (id === serial.current) {
          setReply({ request, refusal: err instanceof Error ? err.message : String(err), source: "wasm" });
        }
      }
    }

    return () => controller.abort();
  }, [request, isFixtureDefault]);

  useEffect(() => {
    if (!result) {
      setDownloads(undefined);
      return;
    }
    const requestUrl = URL.createObjectURL(
      new Blob([JSON.stringify(result.request, null, 2) + "\n"], { type: "application/json" }),
    );
    const reportUrl = URL.createObjectURL(
      new Blob([JSON.stringify(result, null, 2) + "\n"], { type: "application/json" }),
    );
    setDownloads({ result, requestUrl, reportUrl });
    return () => {
      URL.revokeObjectURL(requestUrl);
      URL.revokeObjectURL(reportUrl);
    };
  }, [result]);

  const activeLayerIds = layer2 === "none" ? [layer1] : [layer1, layer2];
  const preset =
    isFixtureDefault
      ? "fixture"
      : layer1 === "zagreb/neoprene-2025/m3nob5" && layer2 === "zagreb/neoprene-2025/m1no5" && hasInterface && interfaceR === 0.02 && insideC === 35 && outsideC === 20
        ? "two-neoprene"
        : layer1 === "zagreb/neoprene-2025/m3nob5" && layer2 === "zagreb/leather-2022/cr-d-h-back" && hasInterface && interfaceR === 0.015 && insideC === 35 && outsideC === 20
          ? "leather"
          : layer2 !== "none" && !hasInterface
            ? "refusal"
            : "custom";

  const totalR = result
    ? R_IN +
      R_OUT +
      result.request.layers.reduce((sum, l) => sum + (l.record.properties.thermal_resistance?.value ?? 0), 0) +
      result.request.interface_resistance_m2_k_w.reduce((sum, r) => sum + r, 0)
    : undefined;

  const cliSnippet = `# replay this exact thermal stack natively with ftx
ftx materials thermal-steady stack-request.json --json`;

  return (
    <>
      <SiteHeader />
      <main id="main" className="th-page">
        <Crumbs trail={[["Exhibition", "/"], ["Material studies", "/#materials"], ["Thermal sandwich"]]} />
        <div className="wrap th-wrap">
          <div className="th-heading">
            <div>
              <p className="kicker">Source-bearing steady conduction · Live WASM</p>
              <h1>Thermal <em>sandwich.</em></h1>
            </div>
            <p>
              Stack measured neoprene and bovine leather sheets from the built-in catalog, declare an explicit contact
              resistance between layers, and inspect exact 1D Fourier surface temperatures and heat flux.
            </p>
          </div>

          <div className="th-desk">
            <section className="th-stage-card" aria-label="Explodable thermal layer stack">
              <div className="th-stage-head">
                <span>
                  {sourceTag === "starter"
                    ? "Verified native fixture · measured-neoprene-steady.json"
                    : sourceTag === "loading"
                      ? "Solving in local Rust WASM…"
                      : refusal
                        ? "Native Rust WASM · Refused"
                        : "Solved live in local Rust WASM"}
                  {" · "}
                  <strong>{activeLayerIds.length} {activeLayerIds.length === 1 ? "layer" : "layers"}</strong>
                </span>
                <label className="th-explode-bar">
                  Explode stack
                  <input
                    type="range"
                    min="0"
                    max="1"
                    step="0.05"
                    value={explode}
                    aria-label="Explode thermal layer stack"
                    onChange={e => setExplode(Number(e.target.value))}
                  />
                </label>
              </div>

              <div className="th-visual-split">
                <div className="th-iso-wrap">
                  <IsometricStack
                    layerIds={activeLayerIds}
                    result={result}
                    refusal={refusal}
                    insideC={insideC}
                    outsideC={outsideC}
                    hasInterface={hasInterface}
                    interfaceR={interfaceR}
                    explode={explode}
                  />
                </div>
                <TemperatureProfile result={result} insideC={insideC} outsideC={outsideC} />
              </div>

              {refusal ? (
                <div className="th-refusal-banner" role="alert">
                  <span className="mono">Structured solver refusal · zero invented numbers</span>
                  <h3>Layered solve blocked without explicit interface resistance.</h3>
                  <p>
                    Rust engine returned: <code>{refusal}</code>. Adding a second layer requires one explicit nonnegative{" "}
                    <code>interface_resistance_m2_k_w</code> entry per adjacent pair — FrankenTextiles refuses rather
                    than silently inventing zero contact resistance or a guessed air gap.
                  </p>
                </div>
              ) : (
                <dl className="th-kpis" aria-live="polite">
                  <div>
                    <dt>Heat flux</dt>
                    <dd data-metric="flux">
                      {result ? `${fmt(result.state.flux_w_m2, 2)} W/m²` : "—"}
                      <small>{result ? `${fmt(result.power_w, 2)} W over ${AREA_M2} m²` : ""}</small>
                    </dd>
                  </div>
                  <div>
                    <dt>Layer 1 faces</dt>
                    <dd data-metric="layer-0-faces">
                      {result ? `${fmt(result.state.layer_faces_c[0][0], 2)} → ${fmt(result.state.layer_faces_c[0][1], 2)} °C` : "—"}
                      <small>
                        {result
                          ? `ΔT ${fmt(result.state.layer_faces_c[0][0] - result.state.layer_faces_c[0][1], 2)} K`
                          : ""}
                      </small>
                    </dd>
                  </div>
                  <div>
                    <dt>{result && result.state.layer_faces_c.length === 2 ? "Layer 2 faces" : "Total R"}</dt>
                    <dd data-metric="layer-1-faces">
                      {result && result.state.layer_faces_c.length === 2
                        ? `${fmt(result.state.layer_faces_c[1][0], 2)} → ${fmt(result.state.layer_faces_c[1][1], 2)} °C`
                        : totalR !== undefined
                          ? `${totalR.toFixed(4)} m²·K/W`
                          : "—"}
                      <small>
                        {result && result.state.layer_faces_c.length === 2
                          ? `ΔT ${fmt(result.state.layer_faces_c[1][0] - result.state.layer_faces_c[1][1], 2)} K`
                          : "films + sheet"}
                      </small>
                    </dd>
                  </div>
                  <div>
                    <dt>{result && result.state.layer_faces_c.length === 2 ? "Interface jump" : "Flux residual"}</dt>
                    <dd data-metric="interface-drop">
                      {result && result.state.layer_faces_c.length === 2
                        ? `${fmt(result.state.layer_faces_c[0][1] - result.state.layer_faces_c[1][0], 2)} K`
                        : result
                          ? `${result.state.max_flux_residual_w_m2.toExponential(1)}`
                          : "—"}
                      <small>
                        {result && result.state.layer_faces_c.length === 2
                          ? `R_if = ${interfaceR.toFixed(3)} m²·K/W`
                          : "W/m² solver check"}
                      </small>
                    </dd>
                  </div>
                </dl>
              )}
            </section>

            <section className="th-console" aria-label="Thermal sandwich controls">
              <h2>Stack &amp; boundaries</h2>
              <div className="th-presets" role="group" aria-label="Measured presets">
                <button
                  type="button"
                  aria-pressed={preset === "fixture"}
                  onClick={() => {
                    setLayer1("zagreb/neoprene-2025/m3nob5");
                    setLayer2("none");
                    setHasInterface(true);
                    setInsideC(35);
                    setOutsideC(20);
                  }}
                >
                  Single neoprene (fixture)
                </button>
                <button
                  type="button"
                  aria-pressed={preset === "two-neoprene"}
                  onClick={() => {
                    setLayer1("zagreb/neoprene-2025/m3nob5");
                    setLayer2("zagreb/neoprene-2025/m1no5");
                    setHasInterface(true);
                    setInterfaceR(0.02);
                    setInsideC(35);
                    setOutsideC(20);
                  }}
                >
                  Two-sheet sandwich
                </button>
                <button
                  type="button"
                  aria-pressed={preset === "leather"}
                  onClick={() => {
                    setLayer1("zagreb/neoprene-2025/m3nob5");
                    setLayer2("zagreb/leather-2022/cr-d-h-back");
                    setHasInterface(true);
                    setInterfaceR(0.015);
                    setInsideC(35);
                    setOutsideC(20);
                  }}
                >
                  Neoprene + leather
                </button>
                <button
                  type="button"
                  data-refusal="true"
                  aria-pressed={preset === "refusal"}
                  onClick={() => {
                    setLayer1("zagreb/neoprene-2025/m3nob5");
                    setLayer2("zagreb/neoprene-2025/m1no5");
                    setHasInterface(false);
                    setInsideC(35);
                    setOutsideC(20);
                  }}
                >
                  Omit interface (refusal)
                </button>
              </div>

              <label className="th-field">
                Layer 1 · Inside measured sheet (no estimates)
                <select aria-label="Layer 1 measured sheet" value={layer1} onChange={e => setLayer1(e.target.value)}>
                  {sheets.map(s => (
                    <option key={s.id} value={s.id}>
                      {s.short} · Rct {s.rct.toFixed(4)}
                    </option>
                  ))}
                </select>
              </label>

              <label className="th-field">
                Layer 2 · Outside measured sheet
                <select aria-label="Layer 2 measured sheet" value={layer2} onChange={e => setLayer2(e.target.value)}>
                  <option value="none">None (single sheet)</option>
                  {sheets.map(s => (
                    <option key={s.id} value={s.id}>
                      {s.short} · Rct {s.rct.toFixed(4)}
                    </option>
                  ))}
                </select>
              </label>

              {layer2 !== "none" && (
                <div className="th-interface-box">
                  <label className="th-checkbox">
                    <input
                      type="checkbox"
                      checked={hasInterface}
                      onChange={e => setHasInterface(e.target.checked)}
                    />
                    Supply explicit interface resistance (required for 2 layers)
                  </label>
                  {hasInterface && (
                    <label className="th-slider">
                      Contact / gap resistance
                      <output>{interfaceR.toFixed(3)} m²·K/W</output>
                      <input
                        type="range"
                        min="0"
                        max="0.1"
                        step="0.005"
                        value={interfaceR}
                        aria-label="Interface resistance in square metre kelvin per watt"
                        onChange={e => setInterfaceR(Number(e.target.value))}
                      />
                    </label>
                  )}
                </div>
              )}

              <label className="th-slider">
                Inside reservoir temperature
                <output>{insideC.toFixed(1)} °C</output>
                <input
                  type="range"
                  min="0"
                  max="60"
                  step="1"
                  value={insideC}
                  aria-label="Inside reservoir temperature in degrees Celsius"
                  onChange={e => setInsideC(Number(e.target.value))}
                />
              </label>

              <label className="th-slider">
                Outside reservoir temperature
                <output>{outsideC.toFixed(1)} °C</output>
                <input
                  type="range"
                  min="-10"
                  max="50"
                  step="1"
                  value={outsideC}
                  aria-label="Outside reservoir temperature in degrees Celsius"
                  onChange={e => setOutsideC(Number(e.target.value))}
                />
              </label>

              <p className="th-status-line">
                Scenario constants: area {AREA_M2} m² · inside film {R_IN} m²·K/W · outside film {R_OUT} m²·K/W ·{" "}
                <code>allow_estimates: false</code>
              </p>

              {downloads && downloads.result === result && (
                <div className="th-downloads">
                  <a href={downloads.requestUrl} download="thermal-sandwich-request.json">
                    Download frozen request JSON ↓
                  </a>
                  <a href={downloads.reportUrl} download="thermal-sandwich-report.json">
                    Full report JSON ↓
                  </a>
                </div>
              )}
            </section>
          </div>

          <div className="th-details-grid">
            <div>
              <h2>Layer-by-layer breakdown &amp; source provenance</h2>
              {result ? (
                <div className="th-table-wrap">
                  <table className="th-table">
                    <thead>
                      <tr>
                        <th>Element</th>
                        <th>Source ID &amp; method</th>
                        <th>Thickness</th>
                        <th>R (m²·K/W)</th>
                        <th>Faces (°C)</th>
                        <th>ΔT (K)</th>
                      </tr>
                    </thead>
                    <tbody>
                      <tr>
                        <td><strong>Inside film</strong></td>
                        <td>Declared scenario boundary</td>
                        <td className="num">—</td>
                        <td className="num">{R_IN.toFixed(4)}</td>
                        <td className="num">{fmt(insideC, 2)} → {fmt(result.state.layer_faces_c[0][0], 2)}</td>
                        <td className="num">{fmt(insideC - result.state.layer_faces_c[0][0], 2)}</td>
                      </tr>
                      {result.request.layers.flatMap((layer, idx) => {
                        const faces = result.state.layer_faces_c[idx];
                        const rct = layer.record.properties.thermal_resistance?.value ?? 0;
                        const thick = layer.record.properties.thickness?.value;
                        const srcRef = layer.record.sources?.paper?.reference;
                        const rows = [
                          <tr key={`${layer.id}-${idx}`}>
                            <td><strong>Layer {idx + 1}</strong><br />{layer.record.name}</td>
                            <td>
                              <code>{layer.id}</code>
                              <br />
                              {layer.record.properties.thermal_resistance?.method}
                              {srcRef && (
                                <>
                                  {" · "}
                                  <a href={srcRef} target="_blank" rel="noreferrer">Source DOI ↗</a>
                                </>
                              )}
                            </td>
                            <td className="num">{thick !== undefined ? `${thick.toFixed(2)} mm` : "—"}</td>
                            <td className="num">{rct.toFixed(4)}</td>
                            <td className="num">{fmt(faces[0], 2)} → {fmt(faces[1], 2)}</td>
                            <td className="num">{fmt(faces[0] - faces[1], 2)}</td>
                          </tr>,
                        ];
                        if (idx === 0 && result.request.layers.length === 2) {
                          rows.push(
                            <tr key="contact-interface">
                              <td><strong>Contact interface</strong></td>
                              <td>Explicit scenario declaration (not inferred)</td>
                              <td className="num">—</td>
                              <td className="num">{result.request.interface_resistance_m2_k_w[0].toFixed(4)}</td>
                              <td className="num">
                                {fmt(result.state.layer_faces_c[0][1], 2)} → {fmt(result.state.layer_faces_c[1][0], 2)}
                              </td>
                              <td className="num">
                                {fmt(result.state.layer_faces_c[0][1] - result.state.layer_faces_c[1][0], 2)}
                              </td>
                            </tr>,
                          );
                        }
                        return rows;
                      })}
                      <tr>
                        <td><strong>Outside film</strong></td>
                        <td>Declared scenario boundary</td>
                        <td className="num">—</td>
                        <td className="num">{R_OUT.toFixed(4)}</td>
                        <td className="num">
                          {fmt(result.state.layer_faces_c[result.state.layer_faces_c.length - 1][1], 2)} → {fmt(outsideC, 2)}
                        </td>
                        <td className="num">
                          {fmt(result.state.layer_faces_c[result.state.layer_faces_c.length - 1][1] - outsideC, 2)}
                        </td>
                      </tr>
                    </tbody>
                  </table>
                </div>
              ) : (
                <p>Table unavailable while the stack request is refused. Supply an explicit interface resistance to solve.</p>
              )}
              <div style={{ marginTop: 18 }}>
                <CodeBlock title="Reproduce identically on the CLI" code={cliSnippet} />
              </div>
            </div>

            <aside className="th-scope" aria-label="Model scope and no-claim boundaries">
              <h2>Scope &amp; no-claim boundary</h2>
              <p>
                This exhibit runs <code>ftx-thermal-steady/1</code> directly through the compiled Rust WebAssembly solver
                on real catalog records with <code>allow_estimates: false</code>. The area ({AREA_M2} m²), boundary
                reservoirs and films are declared scenario inputs — not a diving exposure, measured garment, or comfort
                rating.
              </p>
              <div className="th-flags">
                <span>comfort_claim: false</span>
                <span>empirical_validation_claim: false</span>
                <span>storage_required: false</span>
                <span>uncertainty_propagated: false</span>
              </div>
              <p><strong>Explicitly not simulated by steady dry conduction:</strong></p>
              <ul className="th-tags">
                {(result?.not_simulated ?? [
                  "time-dependent heat storage",
                  "moisture transport",
                  "evaporation",
                  "human thermoregulation",
                  "wind-dependent films",
                  "compression-dependent loft",
                  "inferred air gaps",
                  "radiative exchange",
                  "thermal bridges",
                ]).map(item => (
                  <li key={item}>{item}</li>
                ))}
              </ul>
              {result && (
                <p className="th-sha">
                  Frozen <code>request_sha256</code>: {result.request_sha256}
                </p>
              )}
              <div className="th-links">
                <a href={workbench("materials")}>Open full Material Workbench ↗</a>
                <a href="/experiments/strip/">Cut on the bias →</a>
                <a href="/experiments/cylinder/">The torsion rig →</a>
              </div>
            </aside>
          </div>
        </div>
      </main>
      <SiteFooter />
    </>
  );
}
