"use client";

import { useEffect, useId, useMemo, useState } from "react";
import SiteHeader from "./site/SiteHeader";
import SiteFooter from "./site/SiteFooter";
import Crumbs from "./site/Crumbs";
import { parseTiledPdf } from "../public/labs/pdf-tiles.mjs";
import "./tile-floor.css";

type TileInfo = {
  name: string;
  col: number;
  row: number;
  blank: boolean;
  page: number | null;
  joins: string[];
};

type ParsedPdf = {
  totalPages: number;
  printedTileCount: number;
  blankTileCount: number;
  pageWidthMm: number;
  pageHeightMm: number;
  insetLeftMm: number;
  insetTopMm: number;
  insetBottomMm: number;
  frameWidthMm: number;
  frameHeightMm: number;
  overlapMm: number;
  scaleBarMm: number;
  cols: number;
  rows: number;
  assembledWidthMm: number;
  assembledHeightMm: number;
  tiles: TileInfo[];
};

type MasterData = {
  widthMm: number;
  heightMm: number;
  cutPaths: string[];
  sewPaths: string[];
  markPaths: string[];
};

type GarmentId = "zouave_bolero" | "zouave_trousers" | "fez";

const GARMENTS: { id: GarmentId; label: string; pdf: string; svg: string; summary: string }[] = [
  {
    id: "zouave_bolero",
    label: "Zouave bolero (92 cm)",
    pdf: "/patterns/chart-zouave_bolero.pdf",
    svg: "/labs/tiles/chart-zouave_bolero.svg",
    summary: "Five-panel cropped bolero drafted on the 92 cm chart wearer",
  },
  {
    id: "zouave_trousers",
    label: "Gathered trousers (92 cm)",
    pdf: "/patterns/chart-zouave_trousers.pdf",
    svg: "/labs/tiles/chart-zouave_trousers.svg",
    summary: "Seven-panel pleated trousers with waistband and ankle cuffs",
  },
  {
    id: "fez",
    label: "Fez cap (92 cm)",
    pdf: "/patterns/chart-fez.pdf",
    svg: "/labs/tiles/chart-fez.svg",
    summary: "Two-piece crown and flat circular tip",
  },
];

function parseMasterSvg(svgText: string): MasterData {
  const doc = new DOMParser().parseFromString(svgText, "image/svg+xml");
  const root = doc.querySelector("svg");
  const vb = (root?.getAttribute("viewBox") || "0 0 400 600").trim().split(/\s+/).map(Number);
  const widthMm = vb[2] || 400;
  const heightMm = vb[3] || 600;
  const cutPaths = Array.from(doc.querySelectorAll("g.cut path"), p => p.getAttribute("d") || "").filter(Boolean);
  const sewPaths = Array.from(doc.querySelectorAll("g.sew path"), p => p.getAttribute("d") || "").filter(Boolean);
  const markPaths = Array.from(doc.querySelectorAll("g.grain path, g.fold path, g.notches path"), p => p.getAttribute("d") || "").filter(Boolean);
  return { widthMm, heightMm, cutPaths, sewPaths, markPaths };
}

function FloorCanvas({
  pdf,
  master,
  assembled,
  showCrosses,
  showMaster,
  showBlanks,
  selectedTile,
  onSelectTile,
}: {
  pdf: ParsedPdf;
  master: MasterData;
  assembled: number; // 0 = spread loose sheets, 1 = taped flush
  showCrosses: boolean;
  showMaster: boolean;
  showBlanks: boolean;
  selectedTile: string;
  onSelectTile: (name: string) => void;
}) {
  const uid = useId();
  const spread = (1 - assembled) * 34; // mm gap between sheets when loose
  const fw = pdf.frameWidthMm;
  const fh = pdf.frameHeightMm;
  const ov = pdf.overlapMm;
  const inset = pdf.insetLeftMm;
  const padX = 28;
  const padTop = 26;
  const padBot = 58; // room for 1:1 100 mm calibration scale bars

  const totalW = pdf.cols * fw + Math.max(0, pdf.cols - 1) * spread;
  const totalH = pdf.rows * fh + Math.max(0, pdf.rows - 1) * spread;
  const vbH = totalH + padTop + padBot;
  const vbW = Math.max(totalW + padX * 2, vbH * 1.06);
  const originX = (vbW - totalW) / 2;

  const tileByCoord = useMemo(() => {
    const map = new Map<string, TileInfo>();
    for (const t of pdf.tiles) map.set(`${t.col},${t.row}`, t);
    return map;
  }, [pdf.tiles]);

  const diamond = (cx: number, cy: number, r: number) =>
    `M${cx},${cy - r} L${cx + r},${cy} L${cx},${cy + r} L${cx - r},${cy} Z`;

  return (
    <svg
      className="tf-svg"
      role="img"
      aria-label={`Virtual assembly floor showing ${pdf.printedTileCount} A4 pattern tiles (${pdf.cols} columns by ${pdf.rows} rows, ${pdf.overlapMm} mm overlap) with ${pdf.scaleBarMm} mm scale bar`}
      viewBox={`0 0 ${vbW.toFixed(1)} ${vbH.toFixed(1)}`}
    >
      <defs>
        <pattern id={`${uid}floor`} width="25" height="25" patternUnits="userSpaceOnUse">
          <path d="M 25 0 H 0 V 25" fill="none" stroke="#c4ba9e" strokeWidth="0.6" opacity="0.55" />
        </pattern>
      </defs>

      {/* Cutting-room wooden / kraft mat floor */}
      <rect width={vbW} height={vbH} fill="#e6decb" />
      <rect width={vbW} height={vbH} fill={`url(#${uid}floor)`} />

      {/* Layer 1: Physical A4 sheets & windowed master artwork (non-interactive so hit bboxes equal exact 190x277 frames) */}
      <g className="tf-sheets-art" style={{ pointerEvents: "none" }}>
        {pdf.tiles.map(tile => {
          if (tile.blank) return null;
          const tx = originX + tile.col * (fw + spread);
          const ty = padTop + tile.row * (fh + spread);
          const isSel = selectedTile === tile.name;
          const showTrimPaper = 1 - assembled;
          const bleed = ov * (1 - assembled * 0.5);
          const hasRight = Boolean(tileByCoord.get(`${tile.col + 1},${tile.row}`) && !tileByCoord.get(`${tile.col + 1},${tile.row}`)?.blank);
          const hasBelow = Boolean(tileByCoord.get(`${tile.col},${tile.row + 1}`) && !tileByCoord.get(`${tile.col},${tile.row + 1}`)?.blank);

          return (
            <g key={`art-${tile.name}`}>
              {/* Outer physical A4 margin when sheets are spread before trimming */}
              {showTrimPaper > 0.04 && (
                <rect
                  x={tx - inset * showTrimPaper}
                  y={ty - inset * showTrimPaper}
                  width={fw + 2 * inset * showTrimPaper}
                  height={fh + 2 * inset * showTrimPaper}
                  fill="#f4f0e4"
                  stroke="#b7ae9a"
                  strokeWidth="0.9"
                  rx="1.5"
                />
              )}

              {/* Printable 190 x 277 mm sheet surface */}
              <rect
                x={tx}
                y={ty}
                width={fw}
                height={fh}
                fill={isSel ? "#fffef6" : "#fcfaf2"}
              />

              {/* 5 mm overlap / bleed strip extending past right and bottom trim lines */}
              {showCrosses && hasRight && (
                <rect x={tx + fw} y={ty} width={ov} height={fh} fill="#d8864a" fillOpacity="0.22" />
              )}
              {showCrosses && hasBelow && (
                <rect x={tx} y={ty + fh} width={fw} height={ov} fill="#d8864a" fillOpacity="0.22" />
              )}

              {/* Actual master pattern lines windowed to this tile (+ 5 mm bleed) */}
              <svg
                x={tx - bleed}
                y={ty - bleed}
                width={fw + 2 * bleed}
                height={fh + 2 * bleed}
                viewBox={`${(tile.col * fw - bleed).toFixed(2)} ${(tile.row * fh - bleed).toFixed(2)} ${(fw + 2 * bleed).toFixed(2)} ${(fh + 2 * bleed).toFixed(2)}`}
                overflow="hidden"
              >
                {master.cutPaths.map((d, i) => (
                  <path key={`c-${i}`} d={d} fill="#25424c" fillOpacity="0.08" stroke="#1b2e35" strokeWidth="1.5" />
                ))}
                {master.sewPaths.map((d, i) => (
                  <path key={`s-${i}`} d={d} fill="none" stroke="#8f4a2e" strokeWidth="1" strokeDasharray="4 2.5" />
                ))}
                {master.markPaths.map((d, i) => (
                  <path key={`m-${i}`} d={d} fill="none" stroke="#475c63" strokeWidth="0.9" />
                ))}
              </svg>
            </g>
          );
        })}
      </g>

      {/* Layer 2: Interactive tile frames, badges, crosses, and omitted blank slots */}
      {pdf.tiles.map(tile => {
        if (tile.blank && !showBlanks) return null;
        const tx = originX + tile.col * (fw + spread);
        const ty = padTop + tile.row * (fh + spread);
        const isSel = selectedTile === tile.name;
        const onTileKey = (e: React.KeyboardEvent<SVGGElement>) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onSelectTile(tile.name);
          }
        };

        if (tile.blank) {
          return (
            <g
              key={tile.name}
              className="tf-tile-blank"
              data-tile={tile.name}
              role="button"
              tabIndex={0}
              aria-label={`Blank omitted tile ${tile.name}`}
              aria-pressed={isSel}
              onClick={() => onSelectTile(tile.name)}
              onKeyDown={onTileKey}
            >
              <rect
                x={tx}
                y={ty}
                width={fw}
                height={fh}
                fill="#d8d1be"
                fillOpacity="0.45"
                stroke={isSel ? "#a64123" : "#8e8673"}
                strokeWidth={isSel ? 2 : 1.2}
                strokeDasharray="6 5"
              />
              <text
                x={tx + fw / 2}
                y={ty + fh / 2 - 4}
                textAnchor="middle"
                fill="#696254"
                fontSize="14"
                fontWeight="600"
                fontFamily="var(--mono)"
                style={{ pointerEvents: "none" }}
              >
                {tile.name} · OMITTED
              </text>
              <text
                x={tx + fw / 2}
                y={ty + fh / 2 + 13}
                textAnchor="middle"
                fill="#7b7362"
                fontSize="10"
                fontFamily="var(--sans)"
                style={{ pointerEvents: "none" }}
              >
                Blank tile (not printed)
              </text>
            </g>
          );
        }

        const hasRight = Boolean(tileByCoord.get(`${tile.col + 1},${tile.row}`) && !tileByCoord.get(`${tile.col + 1},${tile.row}`)?.blank);
        const hasBelow = Boolean(tileByCoord.get(`${tile.col},${tile.row + 1}`) && !tileByCoord.get(`${tile.col},${tile.row + 1}`)?.blank);

        return (
          <g
            key={tile.name}
            className="tf-tile"
            data-tile={tile.name}
            data-page={tile.page ?? undefined}
            role="button"
            tabIndex={0}
            aria-label={`Tile ${tile.name}, PDF page ${tile.page}`}
            aria-pressed={isSel}
            onClick={() => onSelectTile(tile.name)}
            onKeyDown={onTileKey}
          >
            {/* Printable 190 x 277 mm tile frame (exact click & focus hit target) */}
            <rect
              x={tx}
              y={ty}
              width={fw}
              height={fh}
              fill="transparent"
              stroke={isSel ? "#a64123" : "#445458"}
              strokeWidth={isSel ? 2.4 : 1.1}
            />

            {/* Tile code + PDF page badge in corner */}
            <g style={{ pointerEvents: "none" }}>
              <rect x={tx + 6} y={ty + 6} width="54" height="18" rx="2.5" fill={isSel ? "#a64123" : "#1f333a"} />
              <text
                x={tx + 33}
                y={ty + 18.5}
                textAnchor="middle"
                fill="#fffdf6"
                fontSize="10.5"
                fontWeight="700"
                fontFamily="var(--mono)"
              >
                {tile.name} · p{tile.page}
              </text>
            </g>

            {/* Alignment crosses & join diamonds */}
            {showCrosses && (
              <g className="tf-crosses" data-crosses="true" style={{ pointerEvents: "none" }}>
                {/* Corner crosshairs at all 4 frame corners */}
                {[
                  [tx, ty],
                  [tx + fw, ty],
                  [tx, ty + fh],
                  [tx + fw, ty + fh],
                ].map(([cx, cy], ci) => (
                  <g key={ci} stroke="#b63f22" strokeWidth="1.2">
                    <line x1={cx - 6} y1={cy} x2={cx + 6} y2={cy} />
                    <line x1={cx} y1={cy - 6} x2={cx} y2={cy + 6} />
                  </g>
                ))}
                {/* Join diamond on right edge (A1|B1) */}
                {hasRight && (
                  <g>
                    <path d={diamond(tx + fw, ty + fh / 2, 7.5)} fill="#fff6eb" stroke="#b63f22" strokeWidth="1.4" />
                    <text
                      x={tx + fw - 12}
                      y={ty + fh / 2 + 3.5}
                      textAnchor="end"
                      fill="#912d16"
                      fontSize="9.5"
                      fontWeight="700"
                      fontFamily="var(--mono)"
                    >
                      {tile.name}|{tileByCoord.get(`${tile.col + 1},${tile.row}`)?.name}
                    </text>
                  </g>
                )}
                {/* Join diamond on bottom edge (A1|A2) */}
                {hasBelow && (
                  <g>
                    <path d={diamond(tx + fw / 2, ty + fh, 7.5)} fill="#fff6eb" stroke="#b63f22" strokeWidth="1.4" />
                    <text
                      x={tx + fw / 2}
                      y={ty + fh - 11}
                      textAnchor="middle"
                      fill="#912d16"
                      fontSize="9.5"
                      fontWeight="700"
                      fontFamily="var(--mono)"
                    >
                      {tile.name}|{tileByCoord.get(`${tile.col},${tile.row + 1}`)?.name}
                    </text>
                  </g>
                )}
              </g>
            )}
          </g>
        );
      })}

      {/* Optional uncut full master boundary overlay (snapped to the assembled origin) */}
      {showMaster && (
        <g
          className="tf-master-overlay"
          data-master-overlay="true"
          transform={`translate(${originX} ${padTop})`}
          opacity={assembled >= 0.98 ? 1 : 0.72}
          style={{ pointerEvents: "none" }}
        >
          <rect
            x="0"
            y="0"
            width={master.widthMm}
            height={master.heightMm}
            fill="none"
            stroke="#1a7664"
            strokeWidth="2.4"
            strokeDasharray="10 5"
          />
          <rect x="6" y={master.heightMm - 22} width={assembled >= 0.98 ? "236" : "276"} height="17" rx="2.5" fill="#1a7664" />
          <text
            x="13"
            y={master.heightMm - 10}
            fill="#fffef7"
            fontSize="9.5"
            fontWeight="700"
            fontFamily="var(--mono)"
          >
            {assembled >= 0.98
              ? `MASTER ${master.widthMm.toFixed(1)} × ${master.heightMm.toFixed(1)} mm · 1:1 MATCH`
              : `TARGET MASTER ${master.widthMm.toFixed(1)} × ${master.heightMm.toFixed(1)} mm (TAPE TO MATCH)`}
          </text>
        </g>
      )}

      {/* Full-scale 100 mm calibration scale bars (PDF test block vs Master 100 mm) */}
      <g className="tf-scale-bars" data-scale-mm={pdf.scaleBarMm} transform={`translate(${originX} ${padTop + totalH + 18})`}>
        <rect x="0" y="0" width={pdf.scaleBarMm} height="10" fill="#1f333a" />
        <rect x="0" y="13" width="100" height="10" fill="#1a7664" />
        <text x={pdf.scaleBarMm + 10} y="8.5" fill="#1f333a" fontSize="10.5" fontWeight="700" fontFamily="var(--mono)">
          PDF COVER TEST SQUARE · {pdf.scaleBarMm.toFixed(0)} mm (10 cm)
        </text>
        <text x="110" y="21.5" fill="#1a7664" fontSize="10.5" fontWeight="700" fontFamily="var(--mono)">
          MASTER SVG SCALE · 100 mm (1:1 MATCH)
        </text>
      </g>
    </svg>
  );
}

export default function TileFloorLab() {
  const [garmentId, setGarmentId] = useState<GarmentId>("zouave_bolero");
  const garment = GARMENTS.find(g => g.id === garmentId) ?? GARMENTS[0];
  const [pdf, setPdf] = useState<ParsedPdf | null>(null);
  const [master, setMaster] = useState<MasterData | null>(null);
  const [error, setError] = useState("");
  const [assembled, setAssembled] = useState(0.55);
  const [showCrosses, setShowCrosses] = useState(true);
  const [showMaster, setShowMaster] = useState(true);
  const [showBlanks, setShowBlanks] = useState(true);
  const [selectedTile, setSelectedTile] = useState("A1");

  useEffect(() => {
    const controller = new AbortController();
    setError("");
    setPdf(null);
    setMaster(null);

    Promise.all([
      fetch(garment.pdf, { signal: controller.signal }).then(async r => {
        if (!r.ok) throw new Error(`Could not fetch exported PDF (${r.status}).`);
        return new Uint8Array(await r.arrayBuffer());
      }),
      fetch(garment.svg, { signal: controller.signal }).then(async r => {
        if (!r.ok) throw new Error(`Could not fetch master SVG (${r.status}).`);
        return r.text();
      }),
    ])
      .then(async ([pdfBytes, svgText]) => {
        const parsed: ParsedPdf = await parseTiledPdf(pdfBytes);
        if (controller.signal.aborted) return;
        setPdf(parsed);
        setMaster(parseMasterSvg(svgText));
        const firstPrinted = parsed.tiles.find((t: TileInfo) => !t.blank)?.name ?? "A1";
        setSelectedTile(firstPrinted);
      })
      .catch(err => {
        if (!controller.signal.aborted) {
          setError(err instanceof Error ? err.message : String(err));
        }
      });

    return () => controller.abort();
  }, [garment]);

  const visibleTiles = useMemo(
    () => (pdf ? pdf.tiles.filter(t => showBlanks || !t.blank) : []),
    [pdf, showBlanks],
  );
  const activeTile = visibleTiles.find(t => t.name === selectedTile) ?? visibleTiles[0];

  return (
    <>
      <SiteHeader />
      <main id="main" className="tf-page">
        <Crumbs trail={[["Exhibition", "/"], ["Studios", "/sections/studio/"], ["A4 tile assembly"]]} />
        <div className="wrap tf-wrap">
          <div className="tf-heading">
            <div>
              <p className="kicker">Home printing · Real exported A4 PDFs</p>
              <h1>Assemble an A4 <em>pattern.</em></h1>
            </div>
            <p>
              Every number on this floor — page count, grid rows and columns, 10 mm insets, 5 mm bleed overlap,
              omitted blank sheets, and join diamond codes — is decoded live from the actual downloadable A4 PDF.
            </p>
          </div>

          <div className="tf-desk">
            <section className="tf-stage-card" aria-label="Virtual A4 pattern assembly floor">
              <div className="tf-stage-head">
                <span>
                  {pdf
                    ? `${pdf.totalPages} pages · ${pdf.printedTileCount} printed tiles (${pdf.cols}×${pdf.rows} A4 grid)`
                    : "Reading exported A4 PDF streams…"}
                </span>
                <label className="tf-assemble-bar">
                  {assembled >= 0.98 ? "Taped flush" : assembled <= 0.02 ? "Loose sheets" : "Tape sheets"}
                  <input
                    type="range"
                    min="0"
                    max="1"
                    step="0.05"
                    value={assembled}
                    aria-label="Slide from loose A4 sheets to taped flush assembly"
                    onChange={e => setAssembled(Number(e.target.value))}
                  />
                </label>
              </div>

              <div className="tf-floor-wrap">
                {pdf && master ? (
                  <FloorCanvas
                    pdf={pdf}
                    master={master}
                    assembled={assembled}
                    showCrosses={showCrosses}
                    showMaster={showMaster}
                    showBlanks={showBlanks}
                    selectedTile={selectedTile}
                    onSelectTile={setSelectedTile}
                  />
                ) : (
                  <p className="tf-status" role="status">
                    {error || "Decoding exported A4 PDF pages and master SVG…"}
                  </p>
                )}
              </div>

              <div className="tf-legend">
                <span><i className="sw-cut" /> Cut line (+SA)</span>
                <span><i className="sw-sew" /> Sewing line</span>
                <span><i className="sw-ov" /> 5 mm overlap bleed</span>
                <span><i className="sw-master" /> Full master boundary</span>
              </div>

              <dl className="tf-kpis" aria-live="polite">
                <div>
                  <dt>PDF pages</dt>
                  <dd>{pdf ? `${pdf.totalPages} pages` : "—"}</dd>
                  <small>{pdf ? `Cover + ${pdf.printedTileCount} tiles (${pdf.blankTileCount} blank omitted)` : "From PDF"}</small>
                </div>
                <div>
                  <dt>Sheet &amp; frame</dt>
                  <dd>{pdf ? `${pdf.frameWidthMm} × ${pdf.frameHeightMm} mm` : "—"}</dd>
                  <small>{pdf ? `A4 ${pdf.pageWidthMm}×${pdf.pageHeightMm} mm · ${pdf.insetLeftMm} mm insets` : "MediaBox"}</small>
                </div>
                <div>
                  <dt>Bleed overlap</dt>
                  <dd>{pdf ? `${pdf.overlapMm} mm` : "—"}</dd>
                  <small>Past right &amp; bottom trim lines</small>
                </div>
                <div>
                  <dt>Scale verification</dt>
                  <dd>{pdf ? `${pdf.scaleBarMm} mm = 100 mm` : "—"}</dd>
                  <small>{master ? `Master ${master.widthMm.toFixed(1)} × ${master.heightMm.toFixed(1)} mm` : "1:1 scale"}</small>
                </div>
              </dl>
            </section>

            <aside className="tf-console" aria-label="Assembly controls and tile inspector">
              <h2>Print &amp; tape controls</h2>

              <label className="tf-field">
                Exported A4 pattern PDF
                <select
                  aria-label="Choose exported A4 pattern PDF"
                  value={garmentId}
                  onChange={e => setGarmentId(e.target.value as GarmentId)}
                >
                  {GARMENTS.map(g => (
                    <option key={g.id} value={g.id}>
                      {g.label}
                    </option>
                  ))}
                </select>
              </label>
              <p className="tf-summary">{garment.summary}.</p>

              <div className="tf-toggles">
                <label className="tf-check">
                  <input
                    type="checkbox"
                    checked={showCrosses}
                    onChange={e => setShowCrosses(e.target.checked)}
                  />
                  Show alignment crosses, join diamonds &amp; 5 mm bleed
                </label>
                <label className="tf-check">
                  <input
                    type="checkbox"
                    checked={showMaster}
                    onChange={e => setShowMaster(e.target.checked)}
                  />
                  Compare against full uncut master boundary
                </label>
                <label className="tf-check">
                  <input
                    type="checkbox"
                    checked={showBlanks}
                    onChange={e => {
                      const next = e.target.checked;
                      setShowBlanks(next);
                      if (!next && pdf) {
                        const sel = pdf.tiles.find(t => t.name === selectedTile);
                        if (sel?.blank) {
                          setSelectedTile(pdf.tiles.find(t => !t.blank)?.name ?? "A1");
                        }
                      }
                    }}
                  />
                  Show omitted blank tiles
                </label>
              </div>

              <div className="tf-quick-snap" role="group" aria-label="Quick floor arrangement">
                <button type="button" aria-pressed={assembled <= 0.05} onClick={() => setAssembled(0)}>
                  Spread sheets
                </button>
                <button type="button" aria-pressed={assembled >= 0.95} onClick={() => setAssembled(1)}>
                  Tape flush
                </button>
              </div>

              {activeTile && (
                <div className="tf-tile-card" aria-live="polite">
                  <div className="tf-tile-card-head">
                    <strong>Tile {activeTile.name}</strong>
                    <span>{activeTile.blank ? "Omitted (blank)" : `PDF Page ${activeTile.page}`}</span>
                  </div>
                  {activeTile.blank ? (
                    <p>No pattern cut line touches this frame, so FrankenTextiles omits it from the PDF to save paper.</p>
                  ) : (
                    <>
                      <p>
                        Trim the right and bottom 10 mm margins along the solid frame line, align the half-diamonds, and
                        tape flush. Pattern lines extend {pdf?.overlapMm ?? 5} mm past the frame for alignment checking.
                      </p>
                      <div className="tf-joins">
                        <span>Join codes on this sheet:</span>
                        <div>
                          {activeTile.joins.length ? (
                            activeTile.joins.map(code => <code key={code}>{code}</code>)
                          ) : (
                            <code>Outer corner</code>
                          )}
                        </div>
                      </div>
                    </>
                  )}
                </div>
              )}

              <div className="tf-downloads">
                <a className="tf-dl-primary" href={garment.pdf} download={`frankentextiles-${garment.id}-A4.pdf`}>
                  Download this exact A4 PDF ↓
                </a>
                <a className="tf-dl-secondary" href={garment.svg} download={`frankentextiles-${garment.id}-master-mm.svg`}>
                  Download uncut master SVG ↓
                </a>
              </div>

              <p className="tf-note">
                Print Page 1 first at <strong>100% / Actual Size</strong> (never Fit to Page) and measure the{" "}
                <code>100 mm</code> / <code>4 in</code> calibration square with a ruler before printing the remaining
                tile pages.
              </p>
            </aside>
          </div>
        </div>
      </main>
      <SiteFooter />
    </>
  );
}
