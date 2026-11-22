"use client";

import { useEffect, useId, useMemo, useState } from "react";
import SiteHeader from "./site/SiteHeader";
import SiteFooter from "./site/SiteFooter";
import Crumbs from "./site/Crumbs";
import { MIN_BOW_TAIL_MM, MIN_SAFE_OVERLAP_MM, TIE_PRESETS, evaluateWrapSetting } from "../public/labs/wrap-skirt.mjs";
import "./wrap-skirt.css";

type SliceData = {
  perimeter_mm: number;
  width_mm: number;
  depth_mm: number;
  hull: [number, number][];
};

type PoseRecord = {
  pose: "rest" | "seated";
  waist_z_mm: number;
  waist_tape_mm: number;
  hip_tape_mm: number;
  sections: { waist: SliceData; hip: SliceData; lap: SliceData };
};

type FormRecord = {
  id: string;
  label: string;
  stature_mm: number;
  chest_mm: number;
  waist_mm: number;
  hip_mm: number;
  flat_validation: {
    status: string;
    seams: { seam: string; a: string; b: string; len_a_mm: number; len_b_mm: number; residual_mm: number; status: string }[];
    points: { id: string; finished_mm: number; body_mm: number; ease_mm: number; status: string }[];
  };
  rest: PoseRecord;
  seated: PoseRecord;
};

type WrapExhibit = {
  schema: string;
  scope: string;
  flat_geometry: {
    back_waist_mm: number;
    front_each_waist_mm: number;
    total_waist_sweep_mm: number;
    hip_sweep_180mm_below_waist_mm: number;
    lap_sweep_310mm_below_waist_mm: number;
    total_hem_sweep_mm: number;
    skirt_length_mm: number;
    tie_each_length_mm: number;
    slot_station_from_left_end_mm: number;
  };
  forms: FormRecord[];
};

type EvalResult = ReturnType<typeof evaluateWrapSetting>;

function WrapCrossSectionSvg({
  evalA,
  evalB,
  sliceLevel,
}: {
  evalA: EvalResult;
  evalB: EvalResult;
  sliceLevel: "waist" | "hip" | "lap";
}) {
  const uid = useId();
  const hull: [number, number][] = sliceLevel === "waist" ? evalA.hullWaist : sliceLevel === "hip" ? evalA.hullHip : evalA.hullLap;
  const overlapA = sliceLevel === "waist" ? evalA.waistOverlapMm : sliceLevel === "hip" ? evalA.hipOverlapMm : evalA.lapOverlapMm;
  const overlapB = sliceLevel === "waist" ? evalB.waistOverlapMm : sliceLevel === "hip" ? evalB.hipOverlapMm : evalB.lapOverlapMm;
  const perim = sliceLevel === "waist" ? evalA.bodyWaistMm : sliceLevel === "hip" ? evalA.hipSlicePerimMm : evalA.lapSlicePerimMm;

  const { vb, bodyPath, cx, cy, rx, ry, spanX } = useMemo(() => {
    if (!hull.length) {
      return { vb: "-260 -260 520 520", bodyPath: "", cx: 0, cy: 0, rx: 140, ry: 110, spanX: 520 };
    }
    // Anny faces -y. The front is at the bottom (+y) in the diagram.
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    const pts = hull.map(([x, y]) => [x, -y] as [number, number]);
    for (const [x, y] of pts) {
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
    }
    const cx0 = (minX + maxX) / 2;
    const cy0 = (minY + maxY) / 2;
    const halfWidth = (maxX - minX) / 2;
    const halfDepth = (maxY - minY) / 2;
    // An ellipse through the axis extrema can still cut across the hull at the sides.
    const hullScale = pts.reduce(
      (scale, [x, y]) => Math.max(scale, Math.hypot((x - cx0) / halfWidth, (y - cy0) / halfDepth)),
      1,
    );
    const rx0 = halfWidth * hullScale;
    const ry0 = halfDepth * hullScale;
    const pad = 74;
    const spanX0 = Math.max((rx0 + pad) * 2, 460);
    const spanY0 = Math.max((ry0 + pad) * 2, 420);
    return {
      vb: `${(cx0 - spanX0 / 2).toFixed(1)} ${(cy0 - spanY0 / 2).toFixed(1)} ${spanX0.toFixed(1)} ${spanY0.toFixed(1)}`,
      bodyPath: pts.map(([x, y], i) => `${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`).join(" ") + " Z",
      cx: cx0,
      cy: cy0,
      rx: rx0,
      ry: ry0,
      spanX: Math.max(spanX0, spanY0),
    };
  }, [hull]);

  // Draw schematic fixed wrap arc around the real Anny cross-section hull:
  // Angular span of front overlap = (overlapMm / perim) * 2*PI centered at front (+y = +PI/2 in SVG).
  const arcPath = (rScaleX: number, rScaleY: number, startAngle: number, endAngle: number) => {
    const steps = 48;
    const parts: string[] = [];
    for (let i = 0; i <= steps; i++) {
      const a = startAngle + ((endAngle - startAngle) * i) / steps;
      const x = cx + rScaleX * Math.cos(a);
      const y = cy + rScaleY * Math.sin(a);
      parts.push(`${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`);
    }
    return parts.join(" ");
  };

  const frontAngle = Math.PI / 2; // bottom of diagram = wearer's front (-y in Anny -> +y in SVG)
  const halfAngleA = Math.min(Math.PI * 0.45, Math.max(-Math.PI * 0.35, (overlapA / Math.max(400, perim)) * Math.PI));
  const halfAngleB = Math.min(Math.PI * 0.45, Math.max(-Math.PI * 0.35, (overlapB / Math.max(400, perim)) * Math.PI));

  const rxUnder = rx + 14;
  const ryUnder = ry + 14;
  const rxOver = rx + 23;
  const ryOver = ry + 23;
  const rxB = rx + 33;
  const ryB = ry + 33;

  return (
    <svg
      className="ws-svg"
      role="img"
      aria-label={`Native Anny ${evalA.posture} ${sliceLevel} slice with schematic arcs for both wrap settings`}
      viewBox={vb}
    >
      <defs>
        <pattern id={`${uid}g`} width="40" height="40" patternUnits="userSpaceOnUse">
          <path d="M 40 0 H 0 V 40" fill="none" stroke="#d5cfc0" strokeWidth="0.7" opacity="0.65" />
        </pattern>
      </defs>
      <rect x="-1500" y="-1500" width="3000" height="3000" fill="#f5f1e4" />
      <rect x="-1500" y="-1500" width="3000" height="3000" fill={`url(#${uid}g)`} />

      {/* Real native Anny horizontal slice hull */}
      {bodyPath && <path className="ws-body-slice" d={bodyPath} fill="#c4a78b" fillOpacity="0.58" stroke="#7d5e42" strokeWidth="2.2" />}

      {/* Back & single-layer side wrap (Setting A) */}
      <path
        className="ws-wrap-main"
        d={arcPath(rxUnder, ryUnder, frontAngle + Math.abs(halfAngleA), frontAngle + Math.PI * 2 - Math.abs(halfAngleA))}
        fill="none"
        stroke="#274754"
        strokeWidth="3.2"
      />

      {/* Double-layer front overlap zone (Setting A) or red gap warning if negative */}
      {overlapA >= 0 ? (
        <>
          <path
            className="ws-overlap-under"
            d={arcPath(rxUnder, ryUnder, frontAngle - halfAngleA, frontAngle + halfAngleA)}
            fill="none"
            stroke="#1a7665"
            strokeWidth="4.2"
            strokeLinecap="round"
          />
          <path
            className="ws-overlap-over"
            d={arcPath(rxOver, ryOver, frontAngle - halfAngleA, frontAngle + halfAngleA)}
            fill="none"
            stroke="#b84724"
            strokeWidth="4.2"
            strokeLinecap="round"
          />
        </>
      ) : (
        <path
          className="ws-overlap-gap"
          d={arcPath(rxOver, ryOver, frontAngle + halfAngleA, frontAngle - halfAngleA)}
          fill="none"
          stroke="#c92e1a"
          strokeWidth="4.4"
          strokeDasharray="8 6"
        />
      )}

      {/* Setting B comparison arc (dashed amber reference) */}
      {overlapB >= 0 && (
        <path
          className="ws-overlap-b"
          d={arcPath(rxB, ryB, frontAngle - halfAngleB, frontAngle + halfAngleB)}
          fill="none"
          stroke="#c78b22"
          strokeWidth="2.6"
          strokeDasharray="7 4"
        />
      )}

      {/* Orientation labels */}
      <text x={cx} y={cy - ry - 36} textAnchor="middle" fill="#465558" fontSize={Math.max(11, spanX * 0.023).toFixed(1)} fontWeight="700" fontFamily="var(--mono)">
        BACK · ANNY 0.6.1 ({evalA.posture.toUpperCase()} POSE)
      </text>
      <text x={cx} y={cy + ry + 54} textAnchor="middle" fill={overlapA < 0 ? "#a92914" : "#1c2e34"} fontSize={Math.max(11.5, spanX * 0.024).toFixed(1)} fontWeight="700" fontFamily="var(--mono)">
        {overlapA >= 0
          ? `FRONT OVERLAP · A: ${overlapA.toFixed(0)} MM · B: ${overlapB.toFixed(0)} MM`
          : `FRONT PARTS BY ${Math.abs(overlapA).toFixed(0)} MM · INSUFFICIENT FLAT SWEEP`}
      </text>
    </svg>
  );
}

function FlatPatternPreviewSvg({
  slotAMm,
  slotBMm,
}: {
  slotAMm: number;
  slotBMm: number;
}) {
  // Exact 1:1 mm drafted layout of the 4 fixed pieces from wrap-skirt-pattern.json
  return (
    <svg
      className="ws-flat-svg"
      role="img"
      aria-label="Actual flat wrap skirt outline showing back skirt, wrap front panels, 1020 mm waistband with requested tie slot stations, and 780 mm waist ties"
      viewBox="-320 -95 1680 930"
    >
      <rect x="-320" y="-95" width="1680" height="930" fill="#f5f1e4" />

      {/* Back skirt (360 waist, 580 hem, 600 long, trued perpendicular at waist & hem) */}
      <g transform="translate(0, 620)">
        <path
          d="M -290,0 L 290,0 C 290,-360 180,-530 180,-600 L -180,-600 C -180,-530 -290,-360 -290,0 Z"
          fill="#264352"
          fillOpacity="0.14"
          stroke="#1c3542"
          strokeWidth="3.2"
        />
        <text x="0" y="-305" textAnchor="middle" fill="#1b2d34" fontSize="22" fontWeight="700" fontFamily="var(--mono)">
          BACK SKIRT
        </text>
        <text x="0" y="-275" textAnchor="middle" fill="#465659" fontSize="18" fontFamily="var(--mono)">
          waist 360 · hem 580 mm
        </text>
      </g>

      {/* Wrap front panel (cut 2: 330 waist, 480 hem, 601.3 wrap edge) */}
      <g transform="translate(450, 620)">
        <path
          d="M -110,0 L 370,0 L 330,-600 L 0,-600 C 0,-530 -110,-360 -110,0 Z"
          fill="#b84724"
          fillOpacity="0.14"
          stroke="#9e391b"
          strokeWidth="3.2"
        />
        <line x1="180" y1="-600" x2="180" y2="-540" stroke="#186d5c" strokeWidth="3.2" />
        <line x1="350" y1="0" x2="310" y2="-600" stroke="#9e391b" strokeWidth="2" strokeDasharray="10 7" />
        <text x="145" y="-305" textAnchor="middle" fill="#1b2d34" fontSize="22" fontWeight="700" fontFamily="var(--mono)">
          WRAP FRONT (×2)
        </text>
        <text x="145" y="-275" textAnchor="middle" fill="#465659" fontSize="18" fontFamily="var(--mono)">
          waist 330 · wrap edge 601.3 mm
        </text>
      </g>

      {/* Waistband (1020 x 70 mm) with exact divisions + Setting A & B pass-through slot markers */}
      <g transform="translate(-260, 706)">
        <rect x="0" y="0" width="1020" height="70" fill="#1c685a" fillOpacity="0.14" stroke="#155449" strokeWidth="3" />
        <line x1="330" y1="0" x2="330" y2="70" stroke="#155449" strokeWidth="2" strokeDasharray="6 5" />
        <line x1="690" y1="0" x2="690" y2="70" stroke="#155449" strokeWidth="2" strokeDasharray="6 5" />
        <line x1="0" y1="35" x2="1020" y2="35" stroke="#536866" strokeWidth="1.6" strokeDasharray="10 6" />
        {/* Setting A slot marker */}
        <line x1={slotAMm} y1="-16" x2={slotAMm} y2="86" stroke="#b84724" strokeWidth="4.5" />
        <text x={slotAMm} y="-22" textAnchor="middle" fill="#9c3214" fontSize="18" fontWeight="700" fontFamily="var(--mono)">
          SLOT A ({slotAMm.toFixed(0)})
        </text>
        {/* Setting B slot marker */}
        {Math.abs(slotBMm - slotAMm) >= 5 && (
          <>
            <line x1={slotBMm} y1="-16" x2={slotBMm} y2="86" stroke="#c78b22" strokeWidth="4.5" strokeDasharray="8 4" />
            <text x={slotBMm} y="106" textAnchor="middle" fill="#8b5a0d" fontSize="18" fontWeight="700" fontFamily="var(--mono)">
              SLOT B ({slotBMm.toFixed(0)})
            </text>
          </>
        )}
        <text
          x="330"
          y="58"
          textAnchor="middle"
          fill="#17332e"
          stroke="#f5f1e4"
          strokeWidth="6"
          paintOrder="stroke"
          fontSize="18"
          fontWeight="700"
          fontFamily="var(--mono)"
        >
          WAISTBAND · 330 (L) + 360 (BACK) + 330 (R) = 1020 MM
        </text>
      </g>

      {/* Waist tie strap (780 x 70 mm folded to 35 mm, cut 2) */}
      <g transform="translate(880, 120)">
        <rect x="0" y="0" width="420" height="70" fill="#c78b22" fillOpacity="0.2" stroke="#8f5f10" strokeWidth="3" />
        <line x1="0" y1="35" x2="420" y2="35" stroke="#8f5f10" strokeWidth="1.6" strokeDasharray="10 6" />
        <text x="210" y="-16" textAnchor="middle" fill="#1b2d34" fontSize="20" fontWeight="700" fontFamily="var(--mono)">
          WAIST TIE ×2 (780 × 70 MM FOLD)
        </text>
        <text x="210" y="102" textAnchor="middle" fill="#465659" fontSize="17" fontFamily="var(--mono)">
          Joins waistband ends (70 mm seam)
        </text>
      </g>
    </svg>
  );
}

export default function WrapSkirtLab() {
  const [exhibit, setExhibit] = useState<WrapExhibit | null>(null);
  const [error, setError] = useState("");
  const [formId, setFormId] = useState("reference");
  const [posture, setPosture] = useState<"rest" | "seated">("rest");
  const [sliceLevel, setSliceLevel] = useState<"waist" | "hip" | "lap">("hip");

  // Setting A vs Setting B adjustable tie comparison on the exact same fixed skirt!
  const [easeA, setEaseA] = useState(20);
  const [slotShiftA, setSlotShiftA] = useState(0);
  const [easeB, setEaseB] = useState(65);
  const [slotShiftB, setSlotShiftB] = useState(45);

  useEffect(() => {
    const controller = new AbortController();
    fetch("/exhibits/wrap-skirt/forms.json", { signal: controller.signal })
      .then(r => {
        if (!r.ok) throw new Error(`Could not load wrap-skirt data (${r.status}).`);
        return r.json() as Promise<WrapExhibit>;
      })
      .then(data => {
        if (!controller.signal.aborted) setExhibit(data);
      })
      .catch(err => {
        if (!controller.signal.aborted) setError(err instanceof Error ? err.message : String(err));
      });
    return () => controller.abort();
  }, []);

  const evalA = useMemo(() => {
    if (!exhibit) return null;
    return evaluateWrapSetting(exhibit, { formId, posture, easeMm: easeA, slotShiftMm: slotShiftA });
  }, [exhibit, formId, posture, easeA, slotShiftA]);

  const evalB = useMemo(() => {
    if (!exhibit) return null;
    return evaluateWrapSetting(exhibit, { formId, posture, easeMm: easeB, slotShiftMm: slotShiftB });
  }, [exhibit, formId, posture, easeB, slotShiftB]);

  const activeForm = exhibit?.forms.find(f => f.id === formId) ?? null;

  return (
    <>
      <SiteHeader />
      <main id="main" className="ws-page">
        <Crumbs trail={[["Exhibition", "/"], ["Studies", "/sections/studies/"], ["Adjustable-overlap wrap skirt"]]} />
        <div className="wrap ws-wrap">
          <div className="ws-heading">
            <div>
              <p className="kicker">Fixed pattern · Neutral vs seated Anny slices</p>
              <h1>A wrap skirt with <em>adjustable overlap.</em></h1>
            </div>
            <p>
              Hold one checked wrap-skirt pattern fixed (<code>1,020 mm</code> waistband, <code>1,540 mm</code> hem,
              two <code>780 mm</code> ties), change the synthetic adult form or sit the body down, and compare two tie
              settings directly against flat pattern lengths and native Anny 0.6.1 mesh slices.
            </p>
          </div>

          {/* Controls: Form, Posture, Slice height, Tie Settings A & B */}
          <section className="ws-bar" aria-label="Wearer form, posture, and tie settings">
            <div className="ws-bar-top">
              <div className="ws-group" role="group" aria-label="Choose synthetic adult form">
                {exhibit?.forms.map(f => (
                  <button
                    key={f.id}
                    type="button"
                    data-form={f.id}
                    aria-pressed={formId === f.id}
                    onClick={() => setFormId(f.id)}
                  >
                    {f.label}
                  </button>
                ))}
              </div>
              <div className="ws-group" role="group" aria-label="Choose neutral or seated posture">
                <button type="button" data-posture="rest" aria-pressed={posture === "rest"} onClick={() => setPosture("rest")}>
                  Neutral standing (rest)
                </button>
                <button type="button" data-posture="seated" aria-pressed={posture === "seated"} onClick={() => setPosture("seated")}>
                  Seated synthetic pose
                </button>
              </div>
            </div>

            <div className="ws-tie-grid">
              <div className="ws-tie-box">
                <div className="ws-tie-head">
                  <strong>Tie setting A (primary)</strong>
                  <span>Ease +{easeA} mm · Slot {(690 + slotShiftA).toFixed(0)} mm</span>
                </div>
                <label>
                  <span>Waist tie ease</span>
                  <input type="range" min="0" max="120" step="5" value={easeA} aria-label="Setting A waist tie ease in mm" onChange={e => setEaseA(Number(e.target.value))} />
                </label>
                <label>
                  <span>Right pass-through slot shift</span>
                  <input type="range" min="-80" max="140" step="5" value={slotShiftA} aria-label="Setting A tie slot shift in mm" onChange={e => setSlotShiftA(Number(e.target.value))} />
                </label>
              </div>
              <div className="ws-tie-box ws-tie-b">
                <div className="ws-tie-head">
                  <strong>Tie setting B (comparison)</strong>
                  <span>Ease +{easeB} mm · Slot {(690 + slotShiftB).toFixed(0)} mm</span>
                </div>
                <label>
                  <span>Waist tie ease</span>
                  <input type="range" min="0" max="120" step="5" value={easeB} aria-label="Setting B waist tie ease in mm" onChange={e => setEaseB(Number(e.target.value))} />
                </label>
                <label>
                  <span>Right pass-through slot shift</span>
                  <input type="range" min="-80" max="140" step="5" value={slotShiftB} aria-label="Setting B tie slot shift in mm" onChange={e => setSlotShiftB(Number(e.target.value))} />
                </label>
              </div>
            </div>

            <div className="ws-preset-row">
              <span>Quick tie presets for A:</span>
              {TIE_PRESETS.map(p => (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => { setEaseA(p.easeMm); setSlotShiftA(p.slotShiftMm); }}
                >
                  {p.label}
                </button>
              ))}
            </div>
          </section>

          {error && <p className="ws-error" role="alert">{error}</p>}

          {evalA && evalB && activeForm && (
            <>
              {/* Main dual stage: Horizontal Overlap Cross-Section + Actual Flat Outline */}
              <div className="ws-dual">
                <section className="ws-card" aria-label="Horizontal cross-section overlap comparison">
                  <div className="ws-card-head">
                    <span>CROSS-SECTION · {posture.toUpperCase()} ANNY SLICE</span>
                    <div className="ws-slice-tabs" role="group" aria-label="Choose horizontal slice height">
                      {(
                        [
                          ["waist", "Waist (0 mm)"],
                          ["hip", "Hip / seated thighs (-180 mm)"],
                          ["lap", "Lower thigh / shins (-310 mm)"],
                        ] as const
                      ).map(([lvl, lbl]) => (
                        <button key={lvl} type="button" aria-pressed={sliceLevel === lvl} onClick={() => setSliceLevel(lvl)}>
                          {lbl}
                        </button>
                      ))}
                    </div>
                  </div>
                  <div className="ws-svg-wrap">
                    <WrapCrossSectionSvg evalA={evalA} evalB={evalB} sliceLevel={sliceLevel} />
                  </div>
                  <div className="ws-legend">
                    <span><i className="sw-under" /> Schematic underwrap (front.L)</span>
                    <span><i className="sw-over" /> Overwrap (front.R · Setting A)</span>
                    <span><i className="sw-b" /> Setting B comparison arc</span>
                    <span><i className="sw-gap" /> Negative overlap / parting gap</span>
                  </div>
                </section>

                <section className="ws-card" aria-label="Actual fixed wrap skirt flat pattern">
                  <div className="ws-card-head">
                    <span>FIXED FLAT PATTERN · 7/7 SEAMS · 4/4 TRUING</span>
                    <span>1,020 MM WAIST · 780 MM TIES</span>
                  </div>
                  <div className="ws-svg-wrap">
                    <FlatPatternPreviewSvg slotAMm={evalA.requestedSlotMm} slotBMm={evalB.requestedSlotMm} />
                  </div>
                  <div className="ws-legend">
                    <span>Waistband safe slot zone: 350–960 mm</span>
                    <span>Safe overlap target: ≥ {MIN_SAFE_OVERLAP_MM} mm</span>
                    <span>Bow tail target: ≥ {MIN_BOW_TAIL_MM} mm</span>
                  </div>
                </section>
              </div>

              {/* Setting A vs Setting B Comparison Table + Explicit Limits */}
              <div className="ws-lower">
                <section className="ws-panel-card" aria-label="Tie setting A versus setting B comparison">
                  <h2>Setting A vs Setting B on {activeForm.label}</h2>
                  <p className="ws-note">
                    Posture: <strong>{posture === "seated" ? "Seated synthetic Anny mesh" : "Neutral standing (rest) Anny mesh"}</strong>.
                    Every length comes directly from the fixed <code>WRAP_SKIRT</code> pattern and native Anny slices.
                  </p>
                  <div className="ws-table-scroll">
                    <table>
                      <thead>
                        <tr>
                          <th>Metric</th>
                          <th>Setting A</th>
                          <th>Setting B</th>
                          <th>Δ (B − A)</th>
                        </tr>
                      </thead>
                      <tbody>
                        <tr>
                          <td>Front waist overlap</td>
                          <td data-metric="waist-overlap-a"><strong>{evalA.waistOverlapMm.toFixed(1)} mm</strong></td>
                          <td data-metric="waist-overlap-b">{evalB.waistOverlapMm.toFixed(1)} mm</td>
                          <td>{(evalB.waistOverlapMm - evalA.waistOverlapMm).toFixed(1)} mm</td>
                        </tr>
                        <tr>
                          <td>Hip overlap ({evalA.hipSlicePerimMm} mm slice)</td>
                          <td data-metric="hip-overlap-a"><strong>{evalA.hipOverlapMm.toFixed(1)} mm</strong></td>
                          <td>{evalB.hipOverlapMm.toFixed(1)} mm</td>
                          <td>{(evalB.hipOverlapMm - evalA.hipOverlapMm).toFixed(1)} mm</td>
                        </tr>
                        <tr>
                          <td>Lap overlap ({evalA.lapSlicePerimMm} mm slice)</td>
                          <td data-metric="lap-overlap-a"><strong>{evalA.lapOverlapMm.toFixed(1)} mm</strong></td>
                          <td>{evalB.lapOverlapMm.toFixed(1)} mm</td>
                          <td>{(evalB.lapOverlapMm - evalA.lapOverlapMm).toFixed(1)} mm</td>
                        </tr>
                        <tr>
                          <td>Remaining bow tail (min of L/R)</td>
                          <td data-metric="tie-tail-a"><strong>{evalA.minTieTailMm.toFixed(1)} mm</strong></td>
                          <td>{evalB.minTieTailMm.toFixed(1)} mm</td>
                          <td>{(evalB.minTieTailMm - evalA.minTieTailMm).toFixed(1)} mm</td>
                        </tr>
                        <tr>
                          <td>Tie pass-through slot station</td>
                          <td>{evalA.requestedSlotMm.toFixed(0)} mm ({evalA.slotInBounds ? "OK" : "OUT OF BOUNDS"})</td>
                          <td>{evalB.requestedSlotMm.toFixed(0)} mm ({evalB.slotInBounds ? "OK" : "OUT OF BOUNDS"})</td>
                          <td>{(evalB.requestedSlotMm - evalA.requestedSlotMm).toFixed(0)} mm</td>
                        </tr>
                        <tr>
                          <td>Coverage status</td>
                          <td>
                            <span className="ws-pill" data-status={evalA.status} data-metric="status-a">{evalA.status}</span>
                          </td>
                          <td>
                            <span className="ws-pill" data-status={evalB.status} data-metric="status-b">{evalB.status}</span>
                          </td>
                          <td>—</td>
                        </tr>
                      </tbody>
                    </table>
                  </div>

                  {/* Explicit Limits */}
                  <div className="ws-limits" data-limits-count={evalA.limits.length}>
                    <h3>Explicit limits on Setting A ({evalA.limits.length === 0 ? "within flat bounds" : `${evalA.limits.length} limit(s)`})</h3>
                    {evalA.limits.length === 0 ? (
                      <p>Setting A stays within flat overlap, waistband slot, and tie-tail margins in the {posture} pose.</p>
                    ) : (
                      <ul>
                        {evalA.limits.map((msg, i) => (
                          <li key={i}>{msg}</li>
                        ))}
                      </ul>
                    )}
                  </div>
                </section>

                <aside className="ws-panel-card" aria-label="Printable wrap skirt pattern downloads and scope">
                  <h2>Print the actual outline</h2>
                  <p className="ws-note">
                    All 7 flat seams (<code>side_R/L</code>, <code>band_back/front_R/L</code> with piecewise centre-front
                    notches, and <code>tie_R/L</code>) pass native <code>ftx validate</code> with <code>0.0 mm</code>{" "}
                    residual.
                  </p>
                  <div className="ws-downloads">
                    <a className="ws-dl-pri" href="/exhibits/wrap-skirt/wrap-skirt-A4.pdf" download="frankentextiles-wrap-skirt-A4.pdf">
                      Download full-size A4 pattern PDF ↓
                    </a>
                    <a className="ws-dl-sec" href="/exhibits/wrap-skirt/wrap-skirt-master.svg" download="frankentextiles-wrap-skirt-master.svg">
                      Download master pattern SVG (1:1 mm) ↓
                    </a>
                    <a className="ws-dl-sec" href="/exhibits/wrap-skirt/wrap-skirt-pattern.json" download>
                      Wrap skirt pattern JSON (fabric-pattern/1) ↓
                    </a>
                    <a className="ws-dl-sec" href="/exhibits/wrap-skirt/forms.json" download>
                      Neutral &amp; seated Anny slices JSON ↓
                    </a>
                  </div>
                  <p className="ws-scope">{exhibit?.scope}</p>
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
