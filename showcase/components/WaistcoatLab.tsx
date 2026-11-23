"use client";

import { useEffect, useId, useMemo, useState } from "react";
import SiteHeader from "./site/SiteHeader";
import SiteFooter from "./site/SiteFooter";
import Crumbs from "./site/Crumbs";
import {
  WELT_STAGES,
  buildPieceOutline,
  getSeamsForInstance,
  getUnpairedForInstance,
} from "../public/labs/waistcoat.mjs";
import "./waistcoat.css";

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
    objectType: string;
    date: string;
    origin: string;
    materials: string;
    publishedDimensionsMm: {
      chestCircumference: number;
      frontLength: number;
      backLength: number;
    };
    reconstructionScope: string;
  };
  patternSha256: string;
  svgSha256: string;
  svgDimensions: {
    widthAttr: string;
    heightAttr: string;
    viewBox: string;
  };
  validation: {
    ok: boolean;
    status: string;
    seamsTotal: number;
    seamsPassing: number;
    truingTotal: number;
    truingPassing: number;
    points: { id: string; label: string; finished_mm: number; body_mm: number; ease_mm: number; status: string }[];
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
    grain?: [number, number][];
    internal?: { kind: string; label: string; pts: [number, number][] }[];
    notches?: { id: string; edge: string; s: number; type: string }[];
    note?: string;
  }[];
  seams: { id: string; a: string; b: string; reverse?: boolean }[];
};

const LAYER_COLORS: Record<string, { fill: string; stroke: string; badge: string }> = {
  shell: { fill: "rgba(184, 71, 36, 0.14)", stroke: "#9e391b", badge: "Shell" },
  facing: { fill: "rgba(199, 139, 34, 0.18)", stroke: "#8f5e0f", badge: "Facing" },
  lining: { fill: "rgba(26, 118, 101, 0.14)", stroke: "#155a4e", badge: "Lining" },
  pocket: { fill: "rgba(38, 67, 82, 0.16)", stroke: "#1e3a48", badge: "Pocket" },
};

type InternalMark = { kind: string; label: string; pts: [number, number][] };
type NotchPoint = { id: string; edge: string; s: number; type: string; pt: [number, number] };
type PieceOutline = {
  id: string;
  instance: string;
  name: string;
  material: string;
  qty: number;
  note: string;
  widthMm: number;
  heightMm: number;
  bounds: { minX: number; maxX: number; minY: number; maxY: number };
  polygon: [number, number][];
  edges: { id: string; pts: [number, number][] }[];
  internals: InternalMark[];
  notches: NotchPoint[];
  grain: [number, number][];
};

function PieceOutlineSvg({
  outline,
  layer,
}: {
  outline: PieceOutline;
  layer: string;
}) {
  const uid = useId();
  const colors = LAYER_COLORS[layer] ?? LAYER_COLORS.shell;
  const { minX, maxX, minY, maxY } = outline.bounds;
  const w = Math.max(80, maxX - minX);
  const h = Math.max(80, maxY - minY);
  const padX = Math.max(40, w * 0.22);
  const padY = Math.max(46, h * 0.18);
  // Pattern coordinates are y-up; flip y for SVG rendering (-y).
  const vbMinX = minX - padX;
  const vbMinY = -maxY - padY;
  const vbW = w + padX * 2;
  const vbH = h + padY * 2;
  const strokeW = Math.max(1.2, Math.max(vbW, vbH) * 0.0042);
  const fontSize = Math.max(8.5, Math.min(20, Math.max(vbW, vbH) * 0.025));

  const polyPath =
    outline.polygon.map(([x, y], i) => `${i === 0 ? "M" : "L"}${x.toFixed(1)},${(-y).toFixed(1)}`).join(" ") + " Z";

  return (
    <svg
      className="wc-svg"
      role="img"
      aria-label={`Cut-piece outline for ${outline.instance} (${outline.widthMm} by ${outline.heightMm} mm) with sewing landmarks and notches`}
      viewBox={`${vbMinX.toFixed(1)} ${vbMinY.toFixed(1)} ${vbW.toFixed(1)} ${vbH.toFixed(1)}`}
    >
      <defs>
        <pattern id={`${uid}g`} width="50" height="50" patternUnits="userSpaceOnUse">
          <path d="M 50 0 H 0 V 50" fill="none" stroke="#d5cfc0" strokeWidth={Math.max(0.4, strokeW * 0.35)} opacity="0.7" />
        </pattern>
      </defs>
      <rect x={vbMinX - 400} y={vbMinY - 400} width={vbW + 800} height={vbH + 800} fill="#f5f1e4" />
      <rect x={vbMinX - 400} y={vbMinY - 400} width={vbW + 800} height={vbH + 800} fill={`url(#${uid}g)`} />

      {/* Main cut piece perimeter */}
      <path d={polyPath} fill={colors.fill} stroke={colors.stroke} strokeWidth={strokeW * 1.4} strokeLinejoin="round" />

      {/* Grainline */}
      {outline.grain.length === 2 && (
        <g stroke="#566668" strokeWidth={strokeW * 0.85} strokeDasharray={`${strokeW * 4} ${strokeW * 2.5}`}>
          <line
            x1={outline.grain[0][0]}
            y1={-outline.grain[0][1]}
            x2={outline.grain[1][0]}
            y2={-outline.grain[1][1]}
          />
        </g>
      )}

      {/* Internal sewing landmarks (pocket welt slash, box, corner clips, buttonholes, back lacing tapes, fold line) */}
      {outline.internals.map((item, idx) => {
        const isSlash = /slash/i.test(item.label);
        const isClip = /clip/i.test(item.label);
        const isFold = item.kind === "fold";
        const isStitchBox = /stitch line/i.test(item.label);
        const isButton = /button/i.test(item.label);
        const stroke = isSlash
          ? "#b82d15"
          : isClip
          ? "#b82d15"
          : isStitchBox
          ? "#1a7665"
          : isFold
          ? "#8f5e0f"
          : isButton
          ? "#3b4c52"
          : "#4d6770";
        const d = item.pts.map(([x, y], i) => `${i === 0 ? "M" : "L"}${x.toFixed(1)},${(-y).toFixed(1)}`).join(" ");
        return (
          <path
            key={idx}
            d={d}
            fill="none"
            stroke={stroke}
            strokeWidth={isSlash ? strokeW * 1.65 : strokeW * 1.0}
            strokeDasharray={isFold || isStitchBox ? `${strokeW * 3.5} ${strokeW * 2}` : undefined}
          />
        );
      })}

      {/* Seam notches */}
      {outline.notches.map(n => (
        <g key={n.id}>
          <circle cx={n.pt[0]} cy={-n.pt[1]} r={strokeW * 2.4} fill="#b84724" stroke="#fffef8" strokeWidth={strokeW * 0.8} />
        </g>
      ))}

      {/* Piece title and exact dimensions */}
      <text
        x={(minX + maxX) / 2}
        y={-maxY - padY * 0.42}
        textAnchor="middle"
        fill="#1a2c32"
        fontSize={fontSize.toFixed(1)}
        fontWeight="700"
        fontFamily="var(--mono)"
      >
        {outline.instance.toUpperCase()} · {outline.widthMm} × {outline.heightMm} MM
      </text>
      <text
        x={(minX + maxX) / 2}
        y={-minY + padY * 0.56}
        textAnchor="middle"
        fill="#47585b"
        fontSize={(fontSize * 0.86).toFixed(1)}
        fontFamily="var(--mono)"
      >
        {outline.material} · cut ×{outline.qty}
      </text>
    </svg>
  );
}

function WeltTransformationSvg({ stageIndex }: { stageIndex: number }) {
  // Visual engineering cross-diagram showing how the flat 136 mm slash in wc_front
  // + 136x48 mm silk welt + 136x140 mm linen bags transform into the visible pocket detail.
  return (
    <svg
      className="wc-welt-svg"
      role="img"
      aria-label={`Welt pocket construction stage ${stageIndex + 1}: ${WELT_STAGES[stageIndex].title}`}
      viewBox="0 0 560 250"
    >
      <rect x="0" y="0" width="560" height="250" fill="#f5f1e4" />

      {/* Left panel: Front silk exterior around y=176 mm */}
      <rect x="24" y="28" width="250" height="196" rx="3" fill="#efe4cf" stroke="#8e6c45" strokeWidth="1.8" />
      <text x="149" y="46" textAnchor="middle" fill="#465558" fontSize="10" fontWeight="700" fontFamily="var(--mono)">
        EXTERIOR SILK FRONT (WC_FRONT)
      </text>

      {/* Stage 1: stitch box & folded welt stitched to lower line */}
      {stageIndex === 0 && (
        <g>
          <rect x="64" y="96" width="170" height="32" fill="rgba(26, 118, 101, 0.1)" stroke="#1a7665" strokeWidth="1.6" strokeDasharray="5 3" />
          <line x1="64" y1="112" x2="234" y2="112" stroke="#b82d15" strokeWidth="1.8" strokeDasharray="6 4" />
          <rect x="64" y="128" width="170" height="32" fill="rgba(184, 71, 36, 0.22)" stroke="#9e391b" strokeWidth="1.8" />
          <text x="149" y="148" textAnchor="middle" fill="#6f250e" fontSize="9.5" fontWeight="700" fontFamily="var(--mono)">
            FOLDED WELT (136 × 24 MM)
          </text>
          <text x="149" y="88" textAnchor="middle" fill="#155a4e" fontSize="9" fontFamily="var(--mono)">
            136 mm slash line + 24 mm stitch box
          </text>
        </g>
      )}

      {/* Stage 2: slashed 136 mm & triangular corner clips */}
      {stageIndex === 1 && (
        <g>
          <rect x="64" y="96" width="170" height="32" fill="#e5d6ba" stroke="#1a7665" strokeWidth="1.6" strokeDasharray="5 3" />
          <line x1="80" y1="112" x2="218" y2="112" stroke="#c72c14" strokeWidth="3.2" />
          <polyline points="64,96 80,112 64,128" fill="rgba(199,44,20,0.22)" stroke="#c72c14" strokeWidth="2" />
          <polyline points="234,96 218,112 234,128" fill="rgba(199,44,20,0.22)" stroke="#c72c14" strokeWidth="2" />
          <text x="149" y="86" textAnchor="middle" fill="#a8240f" fontSize="9.5" fontWeight="700" fontFamily="var(--mono)">
            CUT SLASH + 12 MM DIAGONAL CORNER CLIPS
          </text>
          <text x="149" y="150" textAnchor="middle" fill="#465558" fontSize="9" fontFamily="var(--mono)">
            Stop clip 1 mm short of box corners
          </text>
        </g>
      )}

      {/* Stage 3: welt turned upward, bags pushed inside */}
      {stageIndex === 2 && (
        <g>
          <rect x="64" y="96" width="170" height="32" fill="#d9c6a3" stroke="#7d5c37" strokeWidth="2" />
          <path d="M64,128 L64,96 L234,96 L234,128" fill="none" stroke="#9e391b" strokeWidth="2.4" />
          <line x1="64" y1="128" x2="234" y2="128" stroke="#1a7665" strokeWidth="1.8" strokeDasharray="4 3" />
          <text x="149" y="116" textAnchor="middle" fill="#4d2c12" fontSize="9.5" fontWeight="700" fontFamily="var(--mono)">
            WELT PRESSED UP INTO OPENING
          </text>
          <text x="149" y="152" textAnchor="middle" fill="#155a4e" fontSize="9" fontFamily="var(--mono)">
            Linen bags turned inside through slash
          </text>
        </g>
      )}

      {/* Stage 4: finished visible welt pocket detail */}
      {stageIndex === 3 && (
        <g>
          <rect x="64" y="96" width="170" height="32" rx="1.5" fill="#e5d5b4" stroke="#6d4f2e" strokeWidth="2.2" />
          <line x1="64" y1="96" x2="234" y2="96" stroke="#2d241b" strokeWidth="2.4" />
          <line x1="67" y1="124" x2="231" y2="124" stroke="#9e391b" strokeWidth="1.2" strokeDasharray="3 2" />
          <line x1="68" y1="98" x2="68" y2="126" stroke="#9e391b" strokeWidth="1.6" />
          <line x1="230" y1="98" x2="230" y2="126" stroke="#9e391b" strokeWidth="1.6" />
          <text x="149" y="115" textAnchor="middle" fill="#322619" fontSize="9.5" fontWeight="700" fontFamily="var(--mono)">
            VISIBLE 136 × 24 MM SILK WELT
          </text>
          <text x="149" y="152" textAnchor="middle" fill="#465558" fontSize="9" fontFamily="var(--mono)">
            Ends bar-tacked; linen bag enclosed inside
          </text>
        </g>
      )}

      {/* Right panel: Vertical cross-section through the pocket layers */}
      <rect x="292" y="28" width="244" height="196" rx="3" fill="#fffef8" stroke="#c6bfae" strokeWidth="1.4" />
      <text x="414" y="46" textAnchor="middle" fill="#465558" fontSize="10" fontWeight="700" fontFamily="var(--mono)">
        CROSS-SECTION (EXTERIOR ← → INSIDE)
      </text>

      {/* Shell vertical line */}
      <line x1="350" y1="64" x2="350" y2="104" stroke="#9e391b" strokeWidth="3.2" />
      <line x1="350" y1={stageIndex >= 1 ? "128" : "104"} x2="350" y2="206" stroke="#9e391b" strokeWidth="3.2" />
      <text x="322" y="84" textAnchor="middle" fill="#9e391b" fontSize="8.5" fontWeight="700" fontFamily="var(--mono)">
        SHELL
      </text>

      {/* Welt fold loop */}
      {stageIndex < 2 ? (
        <path d="M 350,128 L 336,128 L 336,154 L 350,154" fill="rgba(184,71,36,0.22)" stroke="#9e391b" strokeWidth="2.2" />
      ) : (
        <path d="M 350,128 L 341,128 L 341,104 L 356,104 L 356,128" fill="rgba(184,71,36,0.24)" stroke="#9e391b" strokeWidth="2.2" />
      )}

      {/* Outer & inner linen pocket bags (hanging inside in stages 3 & 4) */}
      {stageIndex >= 2 && (
        <g>
          <path
            d={stageIndex === 3 ? "M 356,128 L 376,128 L 376,196 L 414,196 L 414,104 L 350,104" : "M 356,128 L 378,128 L 378,194 M 414,194 L 414,104 L 350,104"}
            fill={stageIndex === 3 ? "rgba(38,67,82,0.12)" : "none"}
            stroke="#1e3a48"
            strokeWidth="2.2"
          />
          <text x="395" y="164" textAnchor="middle" fill="#1e3a48" fontSize="8.5" fontWeight="700" fontFamily="var(--mono)">
            BAG
          </text>
        </g>
      )}

      {/* Front linen lining behind pocket bag (stage 4) */}
      {stageIndex === 3 && (
        <g>
          <line x1="456" y1="64" x2="456" y2="206" stroke="#155a4e" strokeWidth="3.2" strokeDasharray="6 3" />
          <text x="494" y="138" textAnchor="middle" fill="#155a4e" fontSize="8.5" fontWeight="700" fontFamily="var(--mono)">
            LINING
          </text>
        </g>
      )}
    </svg>
  );
}

export default function WaistcoatLab() {
  const [pattern, setPattern] = useState<PatternDoc | null>(null);
  const [manifest, setManifest] = useState<WaistcoatManifest | null>(null);
  const [error, setError] = useState("");
  const [selectedPieceId, setSelectedPieceId] = useState("wc_front");
  const [selectedSide, setSelectedSide] = useState<"R" | "L">("R");
  const [weltStage, setWeltStage] = useState(0);

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

  const outline = useMemo<PieceOutline | null>(() => {
    if (!pattern) return null;
    return buildPieceOutline(pattern, selectedPieceId, selectedSide) as PieceOutline;
  }, [pattern, selectedPieceId, selectedSide]);

  const linkedSeams = useMemo<SeamRecord[]>(() => {
    if (!manifest || !outline) return [];
    return getSeamsForInstance(manifest, outline.instance) as SeamRecord[];
  }, [manifest, outline]);

  const linkedUnpaired = useMemo<UnpairedRecord[]>(() => {
    if (!manifest || !outline) return [];
    return getUnpairedForInstance(manifest, outline.instance) as UnpairedRecord[];
  }, [manifest, outline]);

  const activeLayer = manifest?.layers[selectedPieceId] ?? "shell";

  return (
    <>
      <SiteHeader />
      <main id="main" className="wc-page">
        <Crumbs trail={[["Exhibition", "/"], ["Wardrobe", "/sections/wardrobe/"], ["1780s Waistcoat (V&A 256-1880)"]]} />
        <div className="wrap wc-wrap">
          <div className="wc-heading">
            <div>
              <p className="kicker">Historical wardrobe · Victoria and Albert Museum 256-1880</p>
              <h1>Cut and sew an <em>eighteenth-century waistcoat.</em></h1>
            </div>
            <p>
              Reconstructed from V&amp;A accession <code>256-1880</code> (<code>O112019</code>, 1780s: chest{" "}
              <code>107.5 cm</code>, front <code>69 cm</code>, back <code>67 cm</code>). Inspect separate silk fronts,
              plain silk facings, linen linings, and slit welt pockets, trace how the flat slash becomes a visible
              detail, and open the full-size master SVG at published dimensions.
            </p>
          </div>

          {/* Curatorial Provenance Banner */}
          {manifest && (
            <section className="wc-provenance" aria-label="Museum collection provenance">
              <div className="wc-prov-main">
                <strong>
                  {manifest.provenance.institution} · Accession <code>{manifest.provenance.accessionNumber}</code> (
                  {manifest.provenance.date})
                </strong>
                <p>{manifest.provenance.materials}</p>
                <p className="wc-scope-small">{manifest.provenance.reconstructionScope}</p>
              </div>
              <div className="wc-prov-meta">
                <div>
                  <span>Published dimensions</span>
                  <code>
                    Chest {manifest.provenance.publishedDimensionsMm.chestCircumference} mm · Front{" "}
                    {manifest.provenance.publishedDimensionsMm.frontLength} mm · Back{" "}
                    {manifest.provenance.publishedDimensionsMm.backLength} mm
                  </code>
                </div>
                <div>
                  <span>Native validation</span>
                  <code>
                    {manifest.validation.status} · {manifest.validation.seamsPassing}/{manifest.validation.seamsTotal}{" "}
                    seams · {manifest.validation.truingPassing}/{manifest.validation.truingTotal} truing
                  </code>
                </div>
                <div className="wc-prov-links">
                  <a href={manifest.provenance.catalogUrl} target="_blank" rel="noreferrer">
                    V&amp;A catalogue record ({manifest.provenance.systemNumber}) ↗
                  </a>
                  <a className="wc-inside-link" href="/wardrobe/waistcoat/inside-out/">
                    Turn this waistcoat inside out (3D layers) →
                  </a>
                </div>
              </div>
            </section>
          )}

          {error && <p className="wc-error" role="alert">{error}</p>}

          {pattern && manifest && outline && (
            <>
              {/* Piece selector bar grouped by layer */}
              <section className="wc-bar" aria-label="Select cut piece template and garment side">
                <div className="wc-bar-row">
                  <div className="wc-piece-pills" role="group" aria-label="Select waistcoat piece">
                    {pattern.pieces.map(p => {
                      const layer = manifest.layers[p.id] ?? "shell";
                      return (
                        <button
                          key={p.id}
                          type="button"
                          data-piece={p.id}
                          data-layer={layer}
                          aria-pressed={selectedPieceId === p.id}
                          onClick={() => setSelectedPieceId(p.id)}
                        >
                          <small>{LAYER_COLORS[layer]?.badge ?? layer}</small>
                          <span>{p.name}</span>
                        </button>
                      );
                    })}
                  </div>
                  {outline.qty === 2 && (
                    <div className="wc-side-group" role="group" aria-label="Choose right or left cut mirror">
                      {(["R", "L"] as const).map(side => (
                        <button
                          key={side}
                          type="button"
                          data-side={side}
                          aria-pressed={selectedSide === side}
                          onClick={() => setSelectedSide(side)}
                        >
                          {side === "R" ? "Right (.R)" : "Left (.L)"}
                        </button>
                      ))}
                    </div>
                  )}
                </div>
              </section>

              {/* Main dual stage: Cut-piece outline + Landmarks & Seams */}
              <div className="wc-dual">
                <section className="wc-card" aria-label="Selected cut-piece outline">
                  <div className="wc-card-head">
                    <span data-metric="selected-instance">
                      CUT PIECE · {outline.instance} ({LAYER_COLORS[activeLayer]?.badge.toUpperCase()})
                    </span>
                    <span data-metric="selected-dims">
                      {outline.widthMm} × {outline.heightMm} MM
                    </span>
                  </div>
                  <div className="wc-svg-wrap">
                    <PieceOutlineSvg outline={outline} layer={activeLayer} />
                  </div>
                  <div className="wc-legend">
                    <span><i className="sw-slash" /> Pocket welt slash &amp; corner clips</span>
                    <span><i className="sw-box" /> Welt stitch box</span>
                    <span><i className="sw-notch" /> Seam balance notch</span>
                    <span><i className="sw-grain" /> Grainline</span>
                  </div>
                </section>

                <section className="wc-card wc-card-light" aria-label="Sewing landmarks and native seams">
                  <h2 data-metric="selected-title">{outline.name}</h2>
                  <p className="wc-note">{outline.note}</p>

                  {/* Sewing landmarks */}
                  <div className="wc-subblock">
                    <h3>
                      Sewing landmarks &amp; internal marks ({outline.internals.length} marks · {outline.notches.length}{" "}
                      notches)
                    </h3>
                    {outline.internals.length === 0 && outline.notches.length === 0 ? (
                      <p className="wc-muted">Perimeter seams only (no internal slash or buttonhole marks on this template).</p>
                    ) : (
                      <ul className="wc-landmarks" data-landmarks-count={outline.internals.length + outline.notches.length}>
                        {outline.notches.map(n => (
                          <li key={n.id}>
                            <code>{n.id}</code> — {n.type} notch on edge <code>{n.edge}</code> (s = {n.s})
                          </li>
                        ))}
                        {outline.internals.slice(0, 8).map((m, i) => (
                          <li key={i}>
                            <strong>{m.kind}:</strong> {m.label}
                          </li>
                        ))}
                        {outline.internals.length > 8 && (
                          <li className="wc-muted">
                            + {outline.internals.length - 8} more centre-front button/buttonhole stations (12 total, matching V&amp;A 256-1880).
                          </li>
                        )}
                      </ul>
                    )}
                  </div>

                  {/* Linked native seams */}
                  <div className="wc-subblock">
                    <h3>Paired native seams ({linkedSeams.length})</h3>
                    <div className="wc-table-scroll">
                      <table>
                        <thead>
                          <tr>
                            <th>#</th>
                            <th>Seam ID</th>
                            <th>Joins</th>
                            <th>Length</th>
                            <th>Residual</th>
                          </tr>
                        </thead>
                        <tbody>
                          {linkedSeams.map(s => (
                            <tr key={s.id}>
                              <td><code>{s.index}</code></td>
                              <td><code>{s.id}</code></td>
                              <td>
                                <code>{s.a}</code> ↔ <code>{s.b}</code>
                              </td>
                              <td>{s.lenAMm.toFixed(1)} mm</td>
                              <td className="wc-pass">{s.residualMm.toFixed(1)} mm ✓</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  </div>

                  {linkedUnpaired.length > 0 && (
                    <div className="wc-subblock wc-unpaired-box">
                      <h3>Unpaired / internal-slash edges on {outline.instance} ({linkedUnpaired.length})</h3>
                      <ul>
                        {linkedUnpaired.map(u => (
                          <li key={u.ref}>
                            <code>{u.ref}</code> ({u.kind}): {u.reason}
                          </li>
                        ))}
                      </ul>
                    </div>
                  )}
                </section>
              </div>

              {/* Lower section: Flat welt slash -> visible pocket detail + Native SVG/PDF exports */}
              <div className="wc-lower">
                <section className="wc-panel-card" aria-label="Flat welt opening to visible pocket detail">
                  <div className="wc-welt-head">
                    <div>
                      <p className="kicker">Construction detail</p>
                      <h2>How the flat welt slash becomes a visible pocket</h2>
                    </div>
                    <div className="wc-stage-pills" role="group" aria-label="Choose welt pocket stage">
                      {WELT_STAGES.map((st, idx) => (
                        <button
                          key={st.id}
                          type="button"
                          data-welt-stage={st.id}
                          aria-pressed={weltStage === idx}
                          onClick={() => setWeltStage(idx)}
                        >
                          {st.step}. {st.id}
                        </button>
                      ))}
                    </div>
                  </div>
                  <WeltTransformationSvg stageIndex={weltStage} />
                  <p className="wc-welt-caption" data-metric="welt-stage-title">
                    <strong>
                      Stage {WELT_STAGES[weltStage].step}: {WELT_STAGES[weltStage].title}.
                    </strong>{" "}
                    {WELT_STAGES[weltStage].summary}
                  </p>
                  <div className="wc-welt-shortcuts">
                    <span>Jump straight to cut pieces:</span>
                    <button type="button" onClick={() => setSelectedPieceId("wc_front")}>
                      Inspect front shell (wc_front)
                    </button>
                    <button type="button" onClick={() => setSelectedPieceId("wc_welt")}>
                      Inspect pocket welt (wc_welt)
                    </button>
                    <button type="button" onClick={() => setSelectedPieceId("wc_pocket_outer")}>
                      Inspect lower pocket bag (wc_pocket_outer)
                    </button>
                    <button type="button" onClick={() => setSelectedPieceId("wc_pocket_inner")}>
                      Inspect upper pocket bag (wc_pocket_inner)
                    </button>
                  </div>
                </section>

                <aside className="wc-panel-card" aria-label="Published SVG dimensions and verified downloads">
                  <h2>Verified pattern files</h2>
                  <p className="wc-note">
                    Exported by native <code>ftx export</code> and verified by <code>ftx check</code>. Master SVG opens
                    at exact published physical dimensions:{" "}
                    <code data-metric="svg-dims">
                      {manifest.svgDimensions.widthAttr} × {manifest.svgDimensions.heightAttr}
                    </code>{" "}
                    (<code>viewBox=&quot;{manifest.svgDimensions.viewBox}&quot;</code>).
                  </p>
                  <div className="wc-downloads">
                    <a
                      className="wc-dl-pri"
                      href="/exhibits/waistcoat/waistcoat-master.svg"
                      download="frankentextiles-waistcoat-1780s-master.svg"
                    >
                      Download 1:1 mm master SVG ({manifest.svgDimensions.widthAttr}) ↓
                    </a>
                    <a
                      className="wc-dl-sec"
                      href="/exhibits/waistcoat/waistcoat-A4.pdf"
                      download="frankentextiles-waistcoat-1780s-A4.pdf"
                    >
                      Download tiled A4 PDF ↓
                    </a>
                    <a className="wc-dl-sec" href="/exhibits/waistcoat/waistcoat-pattern.json" download>
                      Waistcoat pattern JSON (fabric-pattern/1) ↓
                    </a>
                    <a className="wc-dl-sec" href="/exhibits/waistcoat/waistcoat-manifest.json" download>
                      Provenance &amp; seam audit JSON ↓
                    </a>
                  </div>
                  <p className="wc-sha">
                    Pattern SHA-256: <code>{manifest.patternSha256.slice(0, 16)}…</code> · SVG SHA-256:{" "}
                    <code>{manifest.svgSha256.slice(0, 16)}…</code>
                  </p>
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
