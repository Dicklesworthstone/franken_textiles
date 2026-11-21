"use client";

import { useEffect, useRef, useState } from "react";
import { readPieces, type Piece } from "./DraftLab";
import "./repair-lab.css";

type StageId = "original" | "broken" | "repaired";

interface SeamCheck {
  seam: string;
  a: string;
  b: string;
  len_a_mm: number;
  len_b_mm: number;
  ratio: number;
  target_ratio: string;
  residual_mm: number;
  status: string;
  message?: string;
}

interface FileBytes {
  bytes: Uint8Array<ArrayBuffer>;
  name: string;
}

interface RawStage {
  status: string;
  passedSeams: number;
  totalSeams: number;
  checks: string;
  sideR: SeamCheck;
  sideL: SeamCheck;
  previewSvg: string;
  files: Partial<Record<"pdf" | "svg", FileBytes>>;
}

interface WorkerResult {
  mismatchMm: number;
  breakWhy: string;
  repairWhy: string;
  stages: Record<StageId, RawStage>;
}

interface ParsedStage extends Omit<RawStage, "files" | "previewSvg"> {
  front: Piece;
  back: Piece;
  frontSidePath: string;
  backSidePath: string;
  files: { type: "pdf" | "svg"; url: string; name: string }[];
}

interface ParsedBundle {
  mismatchMm: number;
  breakWhy: string;
  repairWhy: string;
  stages: Record<StageId, ParsedStage>;
}

const OFFSETS = [12, 20, 30, -15] as const;

// Extract the two straight side-seam segments directly from the native sewing path (`M x0,y0 L x1,y1 L x2,y2 ...`).
// In the native Zouave bolero master SVG:
// - `back` sew path starts at cb_hem -> side_hem (pt 1) -> ua (pt 2): segment pt1->pt2 is `back/side.R`.
// - `front` sew path closes from ua (last pt) back to side_hem (pt 0): segment lastPt->pt0 is `front.R/side`.
function extractPoints(pathD: string): [number, number][] {
  const pts: [number, number][] = [];
  for (const m of pathD.matchAll(/(?:M|L)\s*(-?[\d.]+),\s*(-?[\d.]+)/g)) {
    pts.push([Number(m[1]), Number(m[2])]);
  }
  return pts;
}

function sideEdgeSegment(piece: Piece, kind: "front" | "back"): string {
  const pts = extractPoints(piece.sew.join(" "));
  if (pts.length < 3) return "";
  if (kind === "back") {
    const [x1, y1] = pts[1];
    const [x2, y2] = pts[2];
    return `M${x1},${y1} L${x2},${y2}`;
  }
  const [x1, y1] = pts[pts.length - 1];
  const [x2, y2] = pts[0];
  return `M${x1},${y1} L${x2},${y2}`;
}

function parseStage(raw: RawStage): ParsedStage {
  const pieces = readPieces(raw.previewSvg);
  const front = pieces.find(p => p.id === "front");
  const back = pieces.find(p => p.id === "back");
  if (!front || !back) throw new Error("Expected front and back pieces in the bolero preview.");
  const files = (Object.entries(raw.files) as ["pdf" | "svg", FileBytes][]).map(([type, f]) => {
    const blob = new Blob([f.bytes], { type: type === "svg" ? "image/svg+xml" : "application/pdf" });
    return { type, name: f.name, url: URL.createObjectURL(blob) };
  });
  return {
    status: raw.status,
    passedSeams: raw.passedSeams,
    totalSeams: raw.totalSeams,
    checks: raw.checks,
    sideR: raw.sideR,
    sideL: raw.sideL,
    front,
    back,
    frontSidePath: sideEdgeSegment(front, "front"),
    backSidePath: sideEdgeSegment(back, "back"),
    files,
  };
}

function PairedSeamBoard({
  stage,
  ghostBack,
  stageId,
}: {
  stage: ParsedStage;
  ghostBack?: Piece;
  stageId: StageId;
}) {
  const { back, front, sideR } = stage;
  const fail = sideR.status !== "PASS";
  const gap = 64;
  const maxH = Math.max(back.height, front.height, ghostBack?.height ?? 0);
  const totalW = back.width + gap + front.width;
  const pad = 26;

  // Align both pieces along their underarm top so lengthening/shortening at `side_hem` is immediately visible at the hem.
  const backTx = -back.left;
  const backTy = -back.top;
  const frontTx = back.width + gap - front.left;
  const frontTy = -front.top;
  const ghostTx = ghostBack ? -ghostBack.left : 0;
  const ghostTy = ghostBack ? -ghostBack.top : 0;

  return (
    <svg
      className="rl-svg"
      viewBox={`${-pad} ${-pad} ${totalW + 2 * pad} ${maxH + 2 * pad + 18}`}
      role="img"
      aria-label={`Paired back and front bolero side seams (${sideR.a} ${sideR.len_a_mm.toFixed(1)} mm vs ${sideR.b} ${sideR.len_b_mm.toFixed(1)} mm)`}
    >
      {/* Back piece (left) */}
      <g transform={`translate(${backTx} ${backTy})`}>
        {back.cut.map((d, i) => <path key={`bc-${i}`} className="rl-sa-fill" d={d} />)}
        {back.sew.map((d, i) => <path key={`bs-${i}`} className="rl-sew-fill" d={d} />)}
        {back.marks.map((d, i) => <path key={`bm-${i}`} className="rl-mark" d={d} />)}
        {stage.backSidePath && (
          <path className="rl-seam-active" data-state={fail ? "fail" : "pass"} d={stage.backSidePath} />
        )}
      </g>

      {/* Ghost of broken back sewing line when viewing repaired */}
      {stageId === "repaired" && ghostBack && (
        <g transform={`translate(${ghostTx} ${ghostTy})`}>
          {ghostBack.sew.map((d, i) => <path key={`gs-${i}`} className="rl-ghost" d={d} />)}
        </g>
      )}

      {/* Front piece (right) */}
      <g transform={`translate(${frontTx} ${frontTy})`}>
        {front.cut.map((d, i) => <path key={`fc-${i}`} className="rl-sa-fill" d={d} />)}
        {front.sew.map((d, i) => <path key={`fs-${i}`} className="rl-sew-fill" d={d} />)}
        {front.marks.map((d, i) => <path key={`fm-${i}`} className="rl-mark" d={d} />)}
        {stage.frontSidePath && (
          <path className="rl-seam-active" data-state={fail ? "fail" : "pass"} d={stage.frontSidePath} />
        )}
      </g>

      {/* Piece & seam callout labels */}
      <text x={back.width / 2} y={maxH + 14} textAnchor="middle" fill="#1f2c33" fontSize="9.5" fontWeight="600" fontFamily="var(--mono)">
        BACK · back/side.R ({sideR.len_b_mm.toFixed(1)} mm)
      </text>
      <text x={back.width + gap + front.width / 2} y={maxH + 14} textAnchor="middle" fill="#1f2c33" fontSize="9.5" fontWeight="600" fontFamily="var(--mono)">
        FRONT · front.R/side ({sideR.len_a_mm.toFixed(1)} mm)
      </text>

      {/* Seam-match bridge arrow between the two side edges */}
      <line
        x1={back.width - 2}
        y1={maxH * 0.62}
        x2={back.width + gap + 2}
        y2={maxH * 0.62}
        stroke={fail ? "#c0381e" : "#2f6344"}
        strokeWidth="1.8"
        strokeDasharray="3 2"
      />
      <rect
        x={back.width + gap / 2 - 34}
        y={maxH * 0.62 - 22}
        width="68"
        height="18"
        rx="3"
        fill={fail ? "#c0381e" : "#2f6344"}
      />
      <text
        x={back.width + gap / 2}
        y={maxH * 0.62 - 9.5}
        textAnchor="middle"
        fill="#fff8ee"
        fontSize="9.5"
        fontWeight="600"
        fontFamily="var(--mono)"
      >
        {fail ? `Δ ${Math.abs(sideR.residual_mm).toFixed(1)} mm` : "0.0 mm ✓"}
      </text>
    </svg>
  );
}

export default function RepairLab() {
  const [stageId, setStageId] = useState<StageId>("broken");
  const [mismatchMm, setMismatchMm] = useState<number>(20);
  const [bundle, setBundle] = useState<ParsedBundle>();
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  const worker = useRef<Worker | null>(null);
  const serial = useRef(0);

  useEffect(() => () => { worker.current?.terminate(); worker.current = null; }, []);

  useEffect(() => {
    const id = ++serial.current;
    setError("");
    try {
      worker.current ??= new Worker("/labs/repair-worker.mjs", { type: "module" });
      worker.current.onmessage = ({ data }: MessageEvent<{ id: number; result?: WorkerResult; error?: string }>) => {
        if (data.id !== serial.current) return;
        if (data.error) {
          setError(data.error);
          return;
        }
        if (data.result) {
          try {
            const parsed: ParsedBundle = {
              mismatchMm: data.result.mismatchMm,
              breakWhy: data.result.breakWhy,
              repairWhy: data.result.repairWhy,
              stages: {
                original: parseStage(data.result.stages.original),
                broken: parseStage(data.result.stages.broken),
                repaired: parseStage(data.result.stages.repaired),
              },
            };
            setBundle(parsed);
            setError("");
          } catch (err) {
            setError(err instanceof Error ? err.message : String(err));
          }
        }
      };
      worker.current.onerror = () => {
        worker.current?.terminate();
        worker.current = null;
        if (id === serial.current) setError("The local seam-repair WASM engine could not load.");
      };
      worker.current.postMessage({ id, mismatchMm });
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [mismatchMm, retry]);

  useEffect(() => {
    return () => {
      if (!bundle) return;
      for (const stage of Object.values(bundle.stages)) {
        for (const f of stage.files) URL.revokeObjectURL(f.url);
      }
    };
  }, [bundle]);

  const ready = bundle?.mismatchMm === mismatchMm && !error;
  const active = ready ? bundle.stages[stageId] : undefined;

  const cliCmd =
    stageId === "original"
      ? "ftx validate bolero/pattern.json --measurements m.json --fit bolero/fit_spec.json"
      : stageId === "broken"
        ? `ftx alter move bolero/pattern.json --piece back --vertex side_hem --dx 0 --dy ${-mismatchMm} --out broken.json && ftx validate broken.json --measurements m.json --fit bolero/fit_spec.json`
        : "ftx alter true-seam broken.json --seam side_R --end start --out repaired.json && ftx validate repaired.json --measurements m.json --fit bolero/fit_spec.json";

  return (
    <div className="repair-lab">
      <div className="rl-controls">
        <h3>Find &amp; repair a mismatched seam</h3>
        <p>
          Walk the actual sewing line (separate from the 10 mm seam allowance), catch a pre-cut length error on{" "}
          <code>side_R</code> / <code>side_L</code>, and true the seam back to zero residual in local Rust WebAssembly.
        </p>

        <fieldset className="rl-offsets">
          <legend>Synthetic mismatch on back:side_hem</legend>
          <div className="rl-offset-row" role="group" aria-label="Synthetic seam mismatch amount">
            {OFFSETS.map(mm => (
              <button
                key={mm}
                type="button"
                aria-pressed={mismatchMm === mm}
                onClick={() => setMismatchMm(mm)}
              >
                {mm > 0 ? `+${mm}` : mm} mm
              </button>
            ))}
          </div>
        </fieldset>

        <div className="rl-steps" role="group" aria-label="Seam repair stages">
          {([
            ["original", "01", "Original drafted cut", "Baseline 92 cm Zouave bolero"],
            ["broken", "02", "Break the side seam", `Move back:side_hem (${mismatchMm > 0 ? `+${mismatchMm}` : mismatchMm} mm along side)`],
            ["repaired", "03", "Apply true-seam repair", "Native ftx alter true-seam --seam side_R --end start"],
          ] as const).map(([id, num, label, sub]) => {
            const st = ready ? bundle.stages[id] : undefined;
            const fail = st ? st.status !== "CONVERGED" : id === "broken";
            return (
              <button
                key={id}
                type="button"
                aria-pressed={stageId === id}
                onClick={() => setStageId(id)}
              >
                <span className="mono">{num}</span>
                <strong>{label}</strong>
                <span className="rl-badge" data-state={fail ? "fail" : "pass"}>
                  {st ? st.status : "…"}
                </span>
                <small>{sub}</small>
              </button>
            );
          })}
        </div>

        {ready && active && (
          <p className="rl-why" aria-live="polite">
            <strong>
              {stageId === "original"
                ? "Flat seam walk"
                : stageId === "broken"
                  ? "Reversible alteration & validator flag"
                  : "Native true-seam history"}
            </strong>
            {stageId === "original"
              ? `All ${active.passedSeams} of ${active.totalSeams} seams pass on the flat pattern. Side seams front.R/side and back/side.R both measure ${active.sideR.len_a_mm.toFixed(1)} mm along the sewing line.`
              : stageId === "broken"
                ? `${bundle.breakWhy} Validator: ${active.sideR.message ?? "seam length outside ratio"}.`
                : `${bundle.repairWhy} All ${active.passedSeams}/${active.totalSeams} seams pass again (${active.checks.replace(/^PASS: /, "")}).`}
          </p>
        )}

        {ready && active && (
          <div className="rl-downloads">
            {active.files.map(f => (
              <a
                key={f.type}
                href={f.url}
                download={`${stageId}-${f.name}`}
              >
                {f.type === "pdf" ? `${stageId === "repaired" ? "Repaired" : stageId === "broken" ? "Broken" : "Original"} A4 PDF` : "Master SVG"} ↓
              </a>
            ))}
          </div>
        )}
      </div>

      <div className="rl-board" aria-busy={!ready && !error}>
        <div className="rl-board-head">
          <span>
            Seam <strong>side_R</strong> (front.R/side ↔ back/side.R)
          </span>
          <span>10 mm seam allowance vs walked sewing line</span>
        </div>

        <div className="rl-stage">
          {active ? (
            <PairedSeamBoard
              stage={active}
              ghostBack={bundle?.stages.broken.back}
              stageId={stageId}
            />
          ) : null}
          {!ready && (
            <div className="rl-overlay" role="status">
              <div>
                <p>{error || "Running flat validation and true-seam in local Rust WASM…"}</p>
                {error && (
                  <button type="button" onClick={() => setRetry(n => n + 1)}>
                    Retry
                  </button>
                )}
              </div>
            </div>
          )}
        </div>

        <div className="rl-legend">
          <div className="rl-legend-items">
            <span><i className="rl-swatch-sa" /> Cut line (+10 mm seam allowance)</span>
            <span><i className="rl-swatch-sew" /> Walked sewing line</span>
            <span>
              <i className="rl-swatch-edge" data-state={active?.sideR.status !== "PASS" ? "fail" : "pass"} />
              Active paired seam ({active?.sideR.status ?? "…"})
            </span>
          </div>
          <span>Units: mm · 1:1 scale</span>
        </div>

        <dl className="rl-metrics" aria-live="polite">
          <div>
            <dt>Flat status</dt>
            <dd data-metric="flat-status" data-state={active?.status === "CONVERGED" ? "pass" : "fail"}>
              {active ? active.status : "—"}
              <small>{active ? `${active.passedSeams}/${active.totalSeams} seams pass` : ""}</small>
            </dd>
          </div>
          <div>
            <dt>front.R/side (sew)</dt>
            <dd data-metric="len-a">
              {active ? `${active.sideR.len_a_mm.toFixed(1)} mm` : "—"}
              <small>side a · reference</small>
            </dd>
          </div>
          <div>
            <dt>back/side.R (sew)</dt>
            <dd data-metric="len-b">
              {active ? `${active.sideR.len_b_mm.toFixed(1)} mm` : "—"}
              <small>side b · altered</small>
            </dd>
          </div>
          <div>
            <dt>Residual</dt>
            <dd data-metric="residual" data-state={active && Math.abs(active.sideR.residual_mm) > 0.01 ? "fail" : "pass"}>
              {active ? `${active.sideR.residual_mm >= 0 ? "+" : ""}${active.sideR.residual_mm.toFixed(1)} mm` : "—"}
              <small>{active ? `ratio ${active.sideR.ratio.toFixed(3)}` : ""}</small>
            </dd>
          </div>
        </dl>

        <div className="rl-cli" title="Equivalent native CLI command">
          <span>$</span> {cliCmd}
        </div>
      </div>
    </div>
  );
}
