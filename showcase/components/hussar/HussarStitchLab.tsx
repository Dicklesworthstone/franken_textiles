"use client";

import dynamic from "next/dynamic";
import { useCallback, useEffect, useId, useMemo, useState } from "react";
import SiteHeader from "../site/SiteHeader";
import SiteFooter from "../site/SiteFooter";
import Crumbs from "../site/Crumbs";
import {
  RENDER_TOLERANCE_MM,
  decodeDst,
  extractPanelOutlineMm,
  registerDstOnDolmanPanel,
  unpackDolmanBinary,
} from "../../public/labs/dolman-stitch.mjs";
import type { CameraPreset, DolmanMeshData, RegimentLook, RegisteredStitchPoint } from "./DolmanStitchScene";
import "./hussar-stitch.css";

const DolmanStitchScene = dynamic(() => import("./DolmanStitchScene"), { ssr: false });

type SourceKey = "dolman_front.R" | "dolman_front.L" | "braid-sampler";

type SourceSpec = {
  key: SourceKey;
  label: string;
  panelName: string | null;
  dstHref: string;
  dstName: string;
  defaultCam: CameraPreset;
  note: string;
};

const SOURCES: SourceSpec[] = [
  {
    key: "dolman_front.R",
    label: "Right chest front (dolman_front.R)",
    panelName: "dolman_front.R",
    dstHref: "/exhibits/hussar/dolman-front-R.dst",
    dstName: "HUSSAR_BLUE_dolman_front_R_soutache.dst",
    defaultCam: "right-chest",
    note: "Registered directly to solved 3D dolman_front.R panel triangles via exact flat barycentric coordinates.",
  },
  {
    key: "dolman_front.L",
    label: "Left chest front (dolman_front.L)",
    panelName: "dolman_front.L",
    dstHref: "/exhibits/hussar/dolman-front-L.dst",
    dstName: "HUSSAR_BLUE_dolman_front_L_soutache.dst",
    defaultCam: "left-chest",
    note: "Mirrored left chest front registered directly to solved 3D dolman_front.L panel triangles.",
  },
  {
    key: "braid-sampler",
    label: "Unregistered flat sampler (100 × 36 mm)",
    panelName: null,
    dstHref: "/wardrobe/hussar/braid-sampler.dst",
    dstName: "hussar-braid-sampler.dst",
    defaultCam: "chest-front",
    note: "Standalone flat loop sample with no garment panel correspondence — all stitches remain on the flat board.",
  },
];

type RegistrationResult = {
  panelName: string | null;
  registered: boolean;
  toleranceMm: number;
  centreFlatMm: [number, number];
  flatBoundsMm: [number, number, number, number] | null;
  registeredStitches: number;
  unmatchedStitches: number;
  flatOnlyJumps: number;
  maxResidualMm: number | null;
  points: RegisteredStitchPoint[];
};

type LoadedDolman = {
  pack: {
    vertices: number;
    triangles: number[];
    panels: Record<string, [number, number]>;
    frames: number[];
    sourcePc2Sha256: string;
  };
  unpacked: {
    bodyUpright: Float32Array;
    bodyIndex: Uint32Array;
    flat: Float32Array;
    clothUpright: Float32Array;
    vertices: number;
    frameCount: number;
  };
  mesh: DolmanMeshData;
};

function FlatEmbroiderySvg({
  reg,
  panelLoops,
  cursor,
  showJumps,
}: {
  reg: RegistrationResult;
  panelLoops: [number, number][][];
  cursor: number;
  showJumps: boolean;
}) {
  const uid = useId();
  const { vb, panelPaths, placePath, couchPath, jumpPath, needlePt, hoopPt } = useMemo(() => {
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    const expand = (x: number, y: number) => {
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
    };
    for (const loop of panelLoops) {
      for (const [x, y] of loop) expand(x, -y);
    }
    for (const p of reg.points) expand(p.flatMm[0], -p.flatMm[1]);
    if (!Number.isFinite(minX)) {
      minX = -50; maxX = 50; minY = -50; maxY = 50;
    }
    const padX = 24;
    const padY = 24;
    const rawW = maxX - minX + padX * 2;
    const rawH = maxY - minY + padY * 2;
    const vbW = Math.max(rawW, rawH * 0.92);
    const extraX = (vbW - rawW) / 2;
    const vx = minX - padX - extraX;
    const vy = minY - padY;

    const pPaths = panelLoops.map(
      loop => loop.map(([x, y], i) => `${i === 0 ? "M" : "L"}${x.toFixed(1)},${(-y).toFixed(1)}`).join(" ") + " Z",
    );

    const limit = Math.min(cursor, reg.points.length - 1);
    const placeSegs: string[] = [];
    const couchSegs: string[] = [];
    const jumpSegs: string[] = [];
    for (let i = 1; i <= limit; i++) {
      const a = reg.points[i - 1];
      const b = reg.points[i];
      const seg = `M${a.flatMm[0].toFixed(1)},${(-a.flatMm[1]).toFixed(1)}L${b.flatMm[0].toFixed(1)},${(-b.flatMm[1]).toFixed(1)}`;
      if (b.kind !== "stitch" || a.kind !== "stitch") {
        jumpSegs.push(seg);
      } else if (b.pass === "couching") {
        couchSegs.push(seg);
      } else {
        placeSegs.push(seg);
      }
    }

    const cur = reg.points[limit];
    return {
      vb: `${vx.toFixed(1)} ${vy.toFixed(1)} ${vbW.toFixed(1)} ${rawH.toFixed(1)}`,
      panelPaths: pPaths,
      placePath: placeSegs.join(" "),
      couchPath: couchSegs.join(" "),
      jumpPath: jumpSegs.join(" "),
      needlePt: cur ? [cur.flatMm[0], -cur.flatMm[1]] : null,
      hoopPt: [reg.centreFlatMm[0], -reg.centreFlatMm[1]],
    };
  }, [reg, panelLoops, cursor]);

  const strokeScale = reg.registered ? 1 : 0.32;

  return (
    <svg
      className="hs-flat-svg"
      role="img"
      aria-label={
        reg.registered
          ? `Flat ${reg.panelName} embroidery hoop showing DST placement, couching stitches, and flat-only jump travel`
          : "Unregistered 100 by 36 mm flat braid sampler with no 3D panel correspondence"
      }
      viewBox={vb}
    >
      <defs>
        <pattern id={`${uid}grid`} width={reg.registered ? 25 : 10} height={reg.registered ? 25 : 10} patternUnits="userSpaceOnUse">
          <path
            d={reg.registered ? "M 25 0 H 0 V 25" : "M 10 0 H 0 V 10"}
            fill="none"
            stroke="#d4cdb7"
            strokeWidth={0.6 * strokeScale}
            opacity="0.65"
          />
        </pattern>
      </defs>

      <rect x="-2000" y="-2000" width="4000" height="4000" fill="#f3eee0" />
      <rect x="-2000" y="-2000" width="4000" height="4000" fill={`url(#${uid}grid)`} />

      {/* Registered panel outline or unregistered sampler board */}
      {panelPaths.length > 0 ? (
        panelPaths.map((d, i) => (
          <path
            key={i}
            d={d}
            fill="#1f3147"
            fillOpacity="0.12"
            stroke="#22344a"
            strokeWidth={1.6 * strokeScale}
          />
        ))
      ) : (
        <rect
          x="-50"
          y="-18"
          width="100"
          height="36"
          fill="#22344a"
          fillOpacity="0.1"
          stroke="#7b6f5b"
          strokeWidth={0.8}
          strokeDasharray="3 2"
        />
      )}

      {/* Hoop centre crosshair */}
      <g stroke="#7b8688" strokeWidth={0.8 * strokeScale} opacity="0.75">
        <line x1={hoopPt[0] - 8 * strokeScale} y1={hoopPt[1]} x2={hoopPt[0] + 8 * strokeScale} y2={hoopPt[1]} />
        <line x1={hoopPt[0]} y1={hoopPt[1] - 8 * strokeScale} x2={hoopPt[0]} y2={hoopPt[1] + 8 * strokeScale} />
      </g>

      {/* Flat-only jump travel (left strictly on flat sample, never drawn on 3D chest) */}
      {showJumps && jumpPath && (
        <path
          className="hs-flat-jumps"
          d={jumpPath}
          fill="none"
          stroke="#8a5a44"
          strokeWidth={0.9 * strokeScale}
          strokeDasharray={`${3.2 * strokeScale} ${2.4 * strokeScale}`}
          opacity="0.6"
        />
      )}

      {/* Placement run (Colour 1) */}
      {placePath && (
        <path
          className="hs-flat-place"
          d={placePath}
          fill="none"
          stroke="#197562"
          strokeWidth={2.0 * strokeScale}
          strokeLinecap="round"
        />
      )}

      {/* Couching / tack run (Colour 2) */}
      {couchPath && (
        <path
          className="hs-flat-couch"
          d={couchPath}
          fill="none"
          stroke="#b87a1a"
          strokeWidth={2.3 * strokeScale}
          strokeLinecap="round"
        />
      )}

      {/* Live needle crosshair */}
      {needlePt && (
        <g className="hs-flat-needle">
          <circle
            cx={needlePt[0]}
            cy={needlePt[1]}
            r={5.2 * strokeScale}
            fill="#d94e22"
            stroke="#fffdf7"
            strokeWidth={1.6 * strokeScale}
          />
        </g>
      )}
    </svg>
  );
}

export default function HussarStitchLab() {
  const [sourceKey, setSourceKey] = useState<SourceKey>("dolman_front.R");
  const source = SOURCES.find(s => s.key === sourceKey) ?? SOURCES[0];
  const [regiment, setRegiment] = useState<RegimentLook>("Blue");
  const [cameraPreset, setCameraPreset] = useState<CameraPreset>("right-chest");
  const [resetCount, setResetCount] = useState(0);
  const [showJumps, setShowJumps] = useState(true);
  const [dolman, setDolman] = useState<LoadedDolman | null>(null);
  const [dst, setDst] = useState<{ label: string; records: RegisteredStitchPoint[]; stopIndex: number } | null>(null);
  const [reg, setReg] = useState<RegistrationResult | null>(null);
  const [cursor, setCursor] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [error, setError] = useState("");
  const [webglReady, setWebglReady] = useState(false);
  const [webglFailed, setWebglFailed] = useState(false);

  // Load solved dolman mesh once
  useEffect(() => {
    const controller = new AbortController();
    Promise.all([
      fetch("/exhibits/hussar/dolman.json", { signal: controller.signal }).then(r => {
        if (!r.ok) throw new Error(`Could not load dolman layout (${r.status}).`);
        return r.json();
      }),
      fetch("/exhibits/hussar/dolman.gz", { signal: controller.signal }).then(async r => {
        if (!r.ok || !r.body) throw new Error(`Could not load dolman geometry (${r.status}).`);
        return new Response(r.body.pipeThrough(new DecompressionStream("gzip"))).arrayBuffer();
      }),
    ])
      .then(([pack, bin]) => {
        if (controller.signal.aborted) return;
        const unpacked = unpackDolmanBinary(bin, pack);
        setDolman({
          pack,
          unpacked,
          mesh: {
            bodyUpright: unpacked.bodyUpright,
            bodyIndex: unpacked.bodyIndex,
            clothUpright: unpacked.clothUpright,
            triangles: pack.triangles,
            panels: pack.panels,
          },
        });
      })
      .catch(err => {
        if (!controller.signal.aborted) setError(err instanceof Error ? err.message : String(err));
      });
    return () => controller.abort();
  }, []);

  // Load and register selected Tajima DST file whenever source or dolman changes
  useEffect(() => {
    if (!dolman) return;
    const controller = new AbortController();
    setPlaying(false);
    setError("");
    fetch(source.dstHref, { signal: controller.signal })
      .then(async r => {
        if (!r.ok) throw new Error(`Could not fetch DST file (${r.status}).`);
        return new Uint8Array(await r.arrayBuffer());
      })
      .then(bytes => {
        if (controller.signal.aborted) return;
        const decoded = decodeDst(bytes);
        const registered = registerDstOnDolmanPanel(
          decoded,
          dolman.pack,
          dolman.unpacked,
          source.panelName,
          RENDER_TOLERANCE_MM,
        ) as RegistrationResult;
        setDst(decoded as unknown as { label: string; records: RegisteredStitchPoint[]; stopIndex: number });
        setReg(registered);
        // Start right at the braid stop so placement outline is immediately visible on chest and visitor can orbit or play couching!
        setCursor(decoded.stopIndex > 0 ? decoded.stopIndex : Math.max(0, decoded.records.length - 1));
        setCameraPreset(source.defaultCam);
      })
      .catch(err => {
        if (!controller.signal.aborted) setError(err instanceof Error ? err.message : String(err));
      });
    return () => controller.abort();
  }, [dolman, source]);

  // Smooth stitch playback that automatically pauses at the exact braid stop without side effects inside state updaters.
  useEffect(() => {
    if (!playing || !dst || !reg) return;
    const total = reg.points.length;
    if (cursor >= total - 1) {
      setPlaying(false);
      return;
    }
    const stopIdx = dst.stopIndex;
    const stride = total > 500 ? 12 : 2;
    const next = Math.min(total - 1, cursor + stride);
    const id = window.setTimeout(() => {
      if (stopIdx > 0 && cursor < stopIdx && next >= stopIdx) {
        setCursor(stopIdx);
        setPlaying(false);
      } else if (next >= total - 1) {
        setCursor(total - 1);
        setPlaying(false);
      } else {
        setCursor(next);
      }
    }, 35);
    return () => window.clearTimeout(id);
  }, [playing, cursor, dst, reg]);

  const panelLoops = useMemo(() => {
    if (!dolman || !source.panelName) return [];
    return extractPanelOutlineMm(dolman.pack, dolman.unpacked.flat, source.panelName) as [number, number][][];
  }, [dolman, source.panelName]);

  const onReady = useCallback(() => setWebglReady(true), []);
  const onError = useCallback(() => setWebglFailed(true), []);

  const total = reg?.points.length ?? 0;
  const active = reg && total > 0 ? reg.points[Math.min(cursor, total - 1)] : null;
  const atStop = Boolean(dst && dst.stopIndex >= 0 && cursor === dst.stopIndex);
  const phaseLabel = !active
    ? "Loading DST…"
    : atStop
      ? "Stop · lay the soutache braid"
      : active.pass === "placement"
        ? "Colour 1 · Placement run"
        : cursor >= total - 1
          ? reg?.registered
            ? "Complete · 12 chest cords tacked"
            : "Complete · flat sampler loop finished"
          : "Colour 2 · Couching tack run";

  const pdfHref = regiment === "Blue" ? "/patterns/hussar-blue.pdf" : "/patterns/hussar-yellow.pdf";

  return (
    <>
      <SiteHeader />
      <main id="main" className="hs-page">
        <Crumbs trail={[["Exhibition", "/"], ["Hussar’s cabinet", "/wardrobe/hussar/"], ["Chest embroidery trace"]]} />
        <div className="wrap hs-wrap">
          <div className="hs-heading">
            <div>
              <p className="kicker">Tajima DST · Panel-to-surface registration</p>
              <h1>Trace every stitch onto the <em>dolman.</em></h1>
            </div>
            <p>
              Follow the real exported Tajima <code>.dst</code> placement and soutache couching runs from the flat
              embroidery hoop straight onto the solved 3D 1761 Swedish hussar dolman chest. Pause at the exact machine
              stop to orbit the chest, inspect barycentric mesh coordinates, and verify that jump moves and unregistered
              samplers stay strictly on the flat board.
            </p>
          </div>

          {/* Controls & source selector */}
          <div className="hs-bar" role="region" aria-label="Embroidery source and stitch transport">
            <div className="hs-source-group" role="group" aria-label="Choose embroidery DST source">
              {SOURCES.map(s => (
                <button
                  key={s.key}
                  type="button"
                  data-source={s.key}
                  aria-pressed={sourceKey === s.key}
                  onClick={() => setSourceKey(s.key)}
                >
                  {s.label}
                </button>
              ))}
            </div>
            <div className="hs-transport">
              <button
                type="button"
                className="hs-play"
                disabled={!reg}
                aria-pressed={playing}
                onClick={() => {
                  if (!reg) return;
                  if (!playing && cursor >= total - 1) setCursor(0);
                  setPlaying(p => !p);
                }}
              >
                {playing ? "Pause" : atStop ? "Resume couching" : cursor >= total - 1 ? "Replay DST" : "Play stitches"}
              </button>
              <button
                type="button"
                disabled={!dst || dst.stopIndex < 0}
                onClick={() => {
                  if (!dst || dst.stopIndex < 0) return;
                  setPlaying(false);
                  setCursor(dst.stopIndex);
                }}
              >
                Pause at braid stop
              </button>
              <span className="hs-step-pair">
                <button
                  type="button"
                  disabled={!reg || cursor <= 0}
                  aria-label="Previous DST record"
                  onClick={() => { setPlaying(false); setCursor(c => Math.max(0, c - 1)); }}
                >
                  ←
                </button>
                <button
                  type="button"
                  disabled={!reg || cursor >= total - 1}
                  aria-label="Next DST record"
                  onClick={() => { setPlaying(false); setCursor(c => Math.min(total - 1, c + 1)); }}
                >
                  →
                </button>
              </span>
              <button
                type="button"
                disabled={!reg}
                onClick={() => { setPlaying(false); setCursor(Math.max(0, total - 1)); }}
              >
                All stitches
              </button>
            </div>
          </div>

          {/* Timeline scrubber */}
          <div className="hs-scrub-card">
            <div className="hs-scrub-meta" aria-live="polite">
              <span className="hs-phase-badge" data-stop={atStop ? "true" : "false"}>
                {phaseLabel}
              </span>
              <strong>
                Record {total ? cursor + 1 : 0} / {total}
              </strong>
              {dst && dst.stopIndex >= 0 && (
                <span>Braid stop at record #{dst.stopIndex + 1}</span>
              )}
            </div>
            <input
              type="range"
              min="0"
              max={Math.max(0, total - 1)}
              step="1"
              value={cursor}
              disabled={!reg}
              aria-label="DST embroidery stitch record cursor"
              onChange={e => { setPlaying(false); setCursor(Number(e.target.value)); }}
            />
          </div>

          {/* Dual synchronized stage: Flat Hoop/Panel + Live 3D Dolman Chest */}
          <div className="hs-dual">
            <section className="hs-card" aria-label="Flat embroidery hoop and panel view">
              <div className="hs-card-head">
                <span>
                  {reg?.registered ? `FLAT PANEL · ${reg.panelName?.toUpperCase()}` : "FLAT SAMPLE ONLY · UNREGISTERED"}
                </span>
                <label className="hs-jump-toggle">
                  <input type="checkbox" checked={showJumps} onChange={e => setShowJumps(e.target.checked)} />
                  Show flat-only jumps ({reg?.flatOnlyJumps ?? 0})
                </label>
              </div>
              <div className="hs-flat-wrap">
                {reg ? (
                  <FlatEmbroiderySvg reg={reg} panelLoops={panelLoops} cursor={cursor} showJumps={showJumps} />
                ) : (
                  <p className="hs-loading" role="status">{error || "Decoding Tajima DST & registering panel coordinates…"}</p>
                )}
              </div>
              <div className="hs-legend">
                <span><i className="sw-place" /> Placement (Colour 1)</span>
                <span><i className="sw-couch" /> Couching (Colour 2)</span>
                <span><i className="sw-jump" /> Flat-only jumps</span>
                <span><i className="sw-needle" /> Active needle</span>
              </div>
            </section>

            <section className="hs-card" aria-label="Settled 3D dolman chest embroidery view">
              <div className="hs-card-head">
                <span>3D DOLMAN CHEST · {regiment.toUpperCase()} REGIMENT</span>
                <div className="hs-3d-controls">
                  {(["Blue", "Yellow"] as const).map(r => (
                    <button key={r} type="button" aria-pressed={regiment === r} onClick={() => setRegiment(r)}>
                      {r}
                    </button>
                  ))}
                  <button type="button" onClick={() => setResetCount(k => k + 1)}>
                    Reset
                  </button>
                </div>
              </div>
              <div className="hs-3d-wrap">
                {webglFailed ? (
                  <p className="hs-3d-status" role="status">
                    WebGL is unavailable on this device. Flat hoop geometry, registered triangle coordinates, and DST
                    downloads remain available below.
                  </p>
                ) : dolman && reg ? (
                  <>
                    <DolmanStitchScene
                      mesh={dolman.mesh}
                      points={reg.points}
                      cursor={cursor}
                      activePanel={source.panelName}
                      regiment={regiment}
                      cameraPreset={cameraPreset}
                      resetCount={resetCount}
                      onReady={onReady}
                      onError={onError}
                    />
                    {!webglReady && <span className="hs-3d-status" role="status">Loading 3D dolman…</span>}
                    {!reg.registered && (
                      <div className="hs-unregistered-banner" role="status">
                        No panel-to-surface correspondence — standalone sampler stays on the flat board (0 3D stitches).
                      </div>
                    )}
                  </>
                ) : (
                  <span className="hs-3d-status" role="status">{error || "Loading solved 3D dolman mesh…"}</span>
                )}
              </div>
              <div className="hs-cam-bar" role="group" aria-label="3D chest camera presets">
                {(
                  [
                    ["right-chest", "Right chest"],
                    ["left-chest", "Left chest"],
                    ["chest-front", "Chest front"],
                    ["full", "Full jacket"],
                  ] as const
                ).map(([id, label]) => (
                  <button
                    key={id}
                    type="button"
                    aria-pressed={cameraPreset === id}
                    onClick={() => { setCameraPreset(id); setResetCount(k => k + 1); }}
                  >
                    {label}
                  </button>
                ))}
                <span className="hs-orbit-tip">Drag to turn · scroll to zoom · right-drag to move</span>
              </div>
            </section>
          </div>

          {/* Correspondence inspector & real files */}
          <div className="hs-lower">
            <section className="hs-panel-card" aria-label="Needle registration and correspondence telemetry">
              <h2>Panel-to-surface correspondence</h2>
              <p className="hs-note-lead">{source.note}</p>
              <dl className="hs-kpis" aria-live="polite">
                <div>
                  <dt>Registered 3D stitches</dt>
                  <dd data-metric="registered">{reg ? `${reg.registeredStitches}` : "—"}</dd>
                  <small>
                    {reg
                      ? reg.registered
                        ? `Within ≤ ${reg.toleranceMm} mm render tolerance`
                        : `${reg.unmatchedStitches} flat-only stitches`
                      : "Pending"}
                  </small>
                </div>
                <div>
                  <dt>Flat-only segments</dt>
                  <dd data-metric="flat-only">
                    {reg ? `${reg.flatOnlyJumps + reg.unmatchedStitches}` : "—"}
                  </dd>
                  <small>Jumps, stop &amp; unmatched never drawn on 3D</small>
                </div>
                <div>
                  <dt>Max flat residual</dt>
                  <dd data-metric="residual">
                    {reg?.maxResidualMm !== null && reg?.maxResidualMm !== undefined
                      ? `${reg.maxResidualMm.toFixed(3)} mm`
                      : "Unregistered"}
                  </dd>
                  <small>Stated render tolerance: {RENDER_TOLERANCE_MM.toFixed(2)} mm</small>
                </div>
                <div>
                  <dt>Active record</dt>
                  <dd data-metric="active-status">
                    {active ? (active.onSurface ? `Tri #${active.tri}` : active.reason) : "—"}
                  </dd>
                  <small>{active ? `${active.kind} · ${active.pass}` : "Pending"}</small>
                </div>
              </dl>

              {active && (
                <div className="hs-coord-grid">
                  <div>
                    <span>Flat hoop (DST 0.1 mm)</span>
                    <code>({active.xMm.toFixed(1)}, {active.yMm.toFixed(1)}) mm</code>
                  </div>
                  <div>
                    <span>Flat panel board</span>
                    <code>({active.flatMm[0].toFixed(2)}, {active.flatMm[1].toFixed(2)}) mm</code>
                  </div>
                  <div>
                    <span>Barycentric (w₀, u, v)</span>
                    <code>
                      {active.bary
                        ? `(${active.bary[0].toFixed(3)}, ${active.bary[1].toFixed(3)}, ${active.bary[2].toFixed(3)})`
                        : "Flat sample only"}
                    </code>
                  </div>
                  <div>
                    <span>Settled 3D chest (X, Y, Z)</span>
                    <code>
                      {active.pos3d
                        ? `(${(active.pos3d[0] * 1000).toFixed(1)}, ${(active.pos3d[1] * 1000).toFixed(1)}, ${(active.pos3d[2] * 1000).toFixed(1)}) mm`
                        : "Not mapped to 3D surface"}
                    </code>
                  </div>
                </div>
              )}
            </section>

            <aside className="hs-panel-card" aria-label="Downloadable Tajima DST and pattern files">
              <h2>Original DST &amp; pattern files</h2>
              <p className="hs-note-lead">
                Every stitch on the chest comes directly from the downloadable Tajima <code>.dst</code> exported by
                FrankenTextiles from the 1761 dolman pattern.
              </p>
              <div className="hs-downloads">
                <a className="hs-dl-primary" href={source.dstHref} download={source.dstName}>
                  Download this Tajima DST ({source.key}) ↓
                </a>
                <a className="hs-dl-secondary" href={pdfHref} download={`frankentextiles-hussar-${regiment.toLowerCase()}.pdf`}>
                  Download {regiment} regiment pattern PDF ↓
                </a>
                <a className="hs-dl-secondary" href="/exhibits/hussar/dolman-embroidery-manifest.json" download>
                  Dolman embroidery manifest (JSON · 2 fronts) ↓
                </a>
                <a className="hs-dl-secondary" href="/exhibits/hussar/dolman-pattern.json" download>
                  Dolman pattern JSON (12 soutache lines) ↓
                </a>
              </div>
              <p className="hs-back-link">
                <a href="/wardrobe/hussar/">← Return to The hussar’s cabinet</a>
              </p>
            </aside>
          </div>
        </div>
      </main>
      <SiteFooter />
    </>
  );
}
