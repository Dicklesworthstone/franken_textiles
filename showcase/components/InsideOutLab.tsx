"use client";

import dynamic from "next/dynamic";
import { useCallback, useEffect, useMemo, useState } from "react";
import SiteHeader from "./site/SiteHeader";
import SiteFooter from "./site/SiteFooter";
import Crumbs from "./site/Crumbs";
import { buildInsideOutState } from "../public/labs/waistcoat.mjs";
import type { CameraPreset } from "./InsideOutScene";
import "./inside-out.css";

const InsideOutScene = dynamic(() => import("./InsideOutScene"), {
  ssr: false,
  loading: () => <div className="io-loading">Loading 3D garment layers…</div>,
});

type SeamRecord = {
  index: number;
  id: string;
  a: string;
  b: string;
  instanceA: string;
  edgeA: string;
  layerA: string;
  instanceB: string;
  edgeB: string;
  layerB: string;
  reverse: boolean;
  lenAMm: number;
  lenBMm: number;
  residualMm: number;
  status: string;
};

type UnpairedRecord = {
  ref: string;
  instance: string;
  piece: string;
  edge: string;
  layer: string;
  kind: string;
  reason: string;
};

type WaistcoatManifest = {
  schema: string;
  provenance: {
    institution: string;
    accessionNumber: string;
    systemNumber: string;
    catalogUrl: string;
    date: string;
    materials: string;
  };
  validation: {
    status: string;
    seamsTotal: number;
    seamsPassing: number;
  };
  layers: Record<string, string>;
  seams: SeamRecord[];
  unpairedAndMissing: UnpairedRecord[];
};

type PatternDoc = {
  pieces: {
    id: string;
    name: string;
    cut: { qty: number; material?: string };
    vertices: Record<string, [number, number]>;
    edges: { id: string; from: string; to: string; curve?: { type: string; c: [number, number][] } }[];
    internal?: { kind: string; label: string; pts: [number, number][] }[];
    notches?: { id: string; edge: string; s: number; type: string }[];
  }[];
};

type InsideOutSeam = SeamRecord & {
  visibleA: boolean;
  visibleB: boolean;
  bothVisible: boolean;
  isSelected: boolean;
};

type InsideOutViewState = {
  layerVisible: Record<string, boolean>;
  explodeMm: number;
  radialOffsetMm: Record<string, number>;
  seams: InsideOutSeam[];
  unpairedAndMissing: UnpairedRecord[];
  selectedSeam: InsideOutSeam | null;
  selectedUnpaired: UnpairedRecord | null;
  missingLiningCount: number;
  unpairedEdgeCount: number;
  internalSlashCount: number;
};

const CAMERAS: CameraPreset[] = ["Front", "Three-quarter", "Back", "Inside-out", "Neck & collar"];

export default function InsideOutLab() {
  const [pattern, setPattern] = useState<PatternDoc | null>(null);
  const [manifest, setManifest] = useState<WaistcoatManifest | null>(null);
  const [error, setError] = useState("");
  const [webglFailed, setWebglFailed] = useState(false);

  const [showShell, setShowShell] = useState(true);
  const [showFacing, setShowFacing] = useState(true);
  const [showLining, setShowLining] = useState(true);
  const [showPockets, setShowPockets] = useState(true);
  const [explodeMm, setExplodeMm] = useState(26);
  const [selectedSeamIndex, setSelectedSeamIndex] = useState<number | null>(13); // facing_to_lining_R (index 13)
  const [selectedUnpairedRef, setSelectedUnpairedRef] = useState<string | null>(null);
  const [preset, setPreset] = useState<{ name: CameraPreset; nonce: number }>({ name: "Three-quarter", nonce: 0 });
  const [tab, setTab] = useState<"seams" | "audit">("seams");

  const onWebglFailure = useCallback(() => setWebglFailed(true), []);

  useEffect(() => {
    const controller = new AbortController();
    Promise.all([
      fetch("/exhibits/waistcoat/waistcoat-pattern.json", { signal: controller.signal }).then(r => {
        if (!r.ok) throw new Error(`Could not load waistcoat pattern (${r.status}).`);
        return r.json() as Promise<PatternDoc>;
      }),
      fetch("/exhibits/waistcoat/waistcoat-manifest.json", { signal: controller.signal }).then(r => {
        if (!r.ok) throw new Error(`Could not load waistcoat manifest (${r.status}).`);
        return r.json() as Promise<WaistcoatManifest>;
      }),
    ])
      .then(([pat, man]) => {
        if (!controller.signal.aborted) {
          setPattern(pat);
          setManifest(man);
        }
      })
      .catch(err => {
        if (!controller.signal.aborted) setError(err instanceof Error ? err.message : String(err));
      });
    return () => controller.abort();
  }, []);

  const viewState = useMemo<InsideOutViewState | null>(() => {
    if (!manifest) return null;
    const opts = {
      showShell,
      showFacing,
      showLining,
      showPockets,
      explodeMm,
      selectedSeamIndex,
      selectedUnpairedRef,
    };
    return (buildInsideOutState as (m: WaistcoatManifest, o: typeof opts) => InsideOutViewState)(manifest, opts);
  }, [manifest, showShell, showFacing, showLining, showPockets, explodeMm, selectedSeamIndex, selectedUnpairedRef]);

  const visibleLayersStr = viewState
    ? Object.entries(viewState.layerVisible)
        .filter(([, v]) => v)
        .map(([k]) => k)
        .join(",")
    : "";

  return (
    <>
      <SiteHeader />
      <main id="main" className="io-page">
        <Crumbs
          trail={[
            ["Exhibition", "/"],
            ["Wardrobe", "/sections/wardrobe/"],
            ["1780s Waistcoat", "/wardrobe/waistcoat/"],
            ["Turn inside out"],
          ]}
        />
        <div className="wrap io-wrap">
          <div className="io-heading">
            <div>
              <p className="kicker">Layered construction · V&amp;A 256-1880 (1780s)</p>
              <h1>Turn a lined garment <em>inside out.</em></h1>
            </div>
            <p>
              Expose the ivory silk shell, plain silk front facing, unbleached linen lining, and enclosed pocket bags of
              the documented V&amp;A <code>256-1880</code> waistcoat. Rotate every layer together, toggle the shell off
              to reveal the lining, separate layers radially with invariant native seam indices, and audit every missing
              collar lining or unpaired back-vent edge honestly.
            </p>
          </div>

          {error && <p className="io-error" role="alert">{error}</p>}

          {pattern && manifest && viewState && (
            <>
              {/* Layer toggles + Turn-layers-apart slider + Camera presets */}
              <section className="io-controls" aria-label="Layer visibility, separation, and camera controls">
                <div className="io-ctrl-top">
                  <div className="io-toggles" role="group" aria-label="Toggle garment layers">
                    <button
                      type="button"
                      data-toggle-layer="shell"
                      aria-pressed={showShell}
                      onClick={() => setShowShell(v => !v)}
                    >
                      <i className="dot-shell" /> Shell ({showShell ? "Visible" : "Hidden"})
                    </button>
                    <button
                      type="button"
                      data-toggle-layer="facing"
                      aria-pressed={showFacing}
                      onClick={() => setShowFacing(v => !v)}
                    >
                      <i className="dot-facing" /> Facing ({showFacing ? "Visible" : "Hidden"})
                    </button>
                    <button
                      type="button"
                      data-toggle-layer="lining"
                      aria-pressed={showLining}
                      onClick={() => setShowLining(v => !v)}
                    >
                      <i className="dot-lining" /> Linen lining ({showLining ? "Visible" : "Hidden"})
                    </button>
                    <button
                      type="button"
                      data-toggle-layer="pocket"
                      aria-pressed={showPockets}
                      onClick={() => setShowPockets(v => !v)}
                    >
                      <i className="dot-pocket" /> Pocket welt &amp; bags ({showPockets ? "Visible" : "Hidden"})
                    </button>
                  </div>

                  <div className="io-cam-pills" role="group" aria-label="3D camera viewpoints">
                    {CAMERAS.map(name => (
                      <button
                        key={name}
                        type="button"
                        aria-pressed={preset.name === name}
                        onClick={() => setPreset(prev => ({ name, nonce: prev.nonce + 1 }))}
                      >
                        {name}
                      </button>
                    ))}
                  </div>
                </div>

                <div className="io-sep-row">
                  <label>
                    <span>
                      Turn layers apart (radial separation):{" "}
                      <strong data-metric="explode-mm">{viewState.explodeMm} mm</strong>
                    </span>
                    <input
                      type="range"
                      min="0"
                      max="80"
                      step="2"
                      value={explodeMm}
                      aria-label="Turn garment layers apart in millimetres"
                      onChange={e => setExplodeMm(Number(e.target.value))}
                    />
                  </label>
                  <div className="io-quick-actions">
                    <button
                      type="button"
                      onClick={() => {
                        setShowShell(false);
                        setShowFacing(true);
                        setShowLining(true);
                        setShowPockets(true);
                        setExplodeMm(16);
                      }}
                    >
                      Reveal lining only
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        setShowShell(true);
                        setShowFacing(true);
                        setShowLining(true);
                        setShowPockets(true);
                        setExplodeMm(56);
                      }}
                    >
                      Explode all 4 layers
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        setShowShell(true);
                        setShowFacing(true);
                        setShowLining(true);
                        setShowPockets(true);
                        setExplodeMm(0);
                      }}
                    >
                      Assembled (0 mm)
                    </button>
                  </div>
                </div>
              </section>

              {/* Main 3D Stage + Construction Edge & Missing-Lining Inspector */}
              <div className="io-main">
                <section
                  className="io-stage-card"
                  data-visible-layers={visibleLayersStr}
                  data-explode-mm={viewState.explodeMm}
                  data-selected-seam-index={viewState.selectedSeam ? String(viewState.selectedSeam.index) : ""}
                  data-selected-unpaired={viewState.selectedUnpaired ? viewState.selectedUnpaired.ref : ""}
                  aria-label="3D layered waistcoat viewport"
                >
                  <div className="io-stage-head">
                    <span>
                      3D LAYERED GARMENT · DRAG TO TURN · SCROLL TO ZOOM · RIGHT-DRAG TO MOVE
                    </span>
                    <span>
                      SHELL {viewState.radialOffsetMm.shell.toFixed(0)} · FACING{" "}
                      {viewState.radialOffsetMm.facing.toFixed(0)} · LINING{" "}
                      {viewState.radialOffsetMm.lining.toFixed(0)} MM
                    </span>
                  </div>

                  {webglFailed ? (
                    <div className="io-fallback">
                      <p>WebGL 3D context unavailable — layer radii and exact seam indices remain active in the inspector.</p>
                    </div>
                  ) : (
                    <InsideOutScene
                      pattern={pattern}
                      layers={manifest.layers}
                      layerVisible={viewState.layerVisible}
                      radialOffsetMm={viewState.radialOffsetMm}
                      selectedSeam={viewState.selectedSeam}
                      selectedUnpaired={viewState.selectedUnpaired}
                      preset={preset}
                      onFailure={onWebglFailure}
                    />
                  )}

                  {/* Active Selection Callout Bar */}
                  <div className="io-selection-bar" data-metric="selection-readout">
                    {viewState.selectedSeam ? (
                      <>
                        <span className="io-badge-seam">SEAM #{viewState.selectedSeam.index}</span>
                        <strong>
                          <code>{viewState.selectedSeam.id}</code>
                        </strong>
                        <span>
                          <code className="io-side-a">{viewState.selectedSeam.a}</code> ({viewState.selectedSeam.layerA}) ↔{" "}
                          <code className="io-side-b">{viewState.selectedSeam.b}</code> ({viewState.selectedSeam.layerB}) ·{" "}
                          {viewState.selectedSeam.lenAMm.toFixed(1)} mm · residual{" "}
                          {viewState.selectedSeam.residualMm.toFixed(1)} mm
                        </span>
                      </>
                    ) : viewState.selectedUnpaired ? (
                      <>
                        <span className="io-badge-warn">{viewState.selectedUnpaired.kind}</span>
                        <strong>
                          <code>{viewState.selectedUnpaired.ref}</code> ({viewState.selectedUnpaired.layer})
                        </strong>
                        <span>{viewState.selectedUnpaired.reason}</span>
                      </>
                    ) : (
                      <span>Select any paired construction edge or unpaired/missing-lining audit item on the right.</span>
                    )}
                  </div>
                </section>

                {/* Right Inspector: All 36 native seams (0..35) + Honest Missing-Lining / Unpaired Audit */}
                <aside className="io-inspector" aria-label="Construction seams and missing-lining audit">
                  <div className="io-tabs" role="tablist">
                    <button
                      type="button"
                      role="tab"
                      aria-selected={tab === "seams"}
                      data-tab="seams"
                      onClick={() => setTab("seams")}
                    >
                      Paired seams ({viewState.seams.length})
                    </button>
                    <button
                      type="button"
                      role="tab"
                      aria-selected={tab === "audit"}
                      data-tab="audit"
                      onClick={() => setTab("audit")}
                    >
                      Missing lining &amp; unpaired ({viewState.unpairedAndMissing.length})
                    </button>
                  </div>

                  {tab === "seams" ? (
                    <div className="io-list-wrap">
                      <p className="io-inspector-note">
                        Exact 0-based native seam indices from <code>waistcoat-pattern.json</code>. Separating layers or
                        toggling shell visibility never alters seam numbering.
                      </p>
                      <ul className="io-seam-list">
                        {viewState.seams.map(s => (
                          <li key={s.id}>
                            <button
                              type="button"
                              data-seam-index={s.index}
                              data-seam-id={s.id}
                              aria-pressed={s.isSelected}
                              onClick={() => {
                                setSelectedUnpairedRef(null);
                                setSelectedSeamIndex(s.index);
                              }}
                            >
                              <span className="io-idx">#{s.index}</span>
                              <span className="io-sbody">
                                <strong>{s.id}</strong>
                                <small>
                                  {s.a} ({s.layerA}) ↔ {s.b} ({s.layerB}) · {s.lenAMm.toFixed(1)} mm
                                </small>
                              </span>
                            </button>
                          </li>
                        ))}
                      </ul>
                    </div>
                  ) : (
                    <div className="io-list-wrap" data-audit-panel="true">
                      <div className="io-audit-summary">
                        <span>
                          <strong>{viewState.missingLiningCount}</strong> missing collar-lining edges
                        </span>
                        <span>
                          <strong>{viewState.unpairedEdgeCount}</strong> open back-vent edges
                        </span>
                        <span>
                          <strong>{viewState.internalSlashCount}</strong> internal pocket-slash joins
                        </span>
                      </div>
                      <p className="io-inspector-note">
                        FrankenTextiles never invents missing linings or fake duplicate seams. Every unpaired edge on
                        V&amp;A 256-1880 is surfaced directly from the native edge audit:
                      </p>
                      <ul className="io-audit-list">
                        {viewState.unpairedAndMissing.map(u => (
                          <li key={u.ref}>
                            <button
                              type="button"
                              data-unpaired-ref={u.ref}
                              data-unpaired-kind={u.kind}
                              aria-pressed={selectedUnpairedRef === u.ref}
                              onClick={() => {
                                setSelectedSeamIndex(null);
                                setSelectedUnpairedRef(u.ref);
                              }}
                            >
                              <span className="io-kind-pill" data-kind={u.kind}>
                                {u.kind}
                              </span>
                              <strong>
                                {u.ref} <small>({u.layer})</small>
                              </strong>
                              <p>{u.reason}</p>
                            </button>
                          </li>
                        ))}
                      </ul>
                    </div>
                  )}

                  <div className="io-footer-links">
                    <a href="/wardrobe/waistcoat/">← Back to flat cut pieces &amp; welt pocket steps</a>
                    <a href={manifest.provenance.catalogUrl} target="_blank" rel="noreferrer">
                      V&amp;A 256-1880 record ↗
                    </a>
                  </div>
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
