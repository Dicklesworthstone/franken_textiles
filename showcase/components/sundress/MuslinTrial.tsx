"use client";

import { useState } from "react";
import TorsoSlice from "../construction/TorsoSlice";

type Sample = {
  name: string;
  pdf: string;
  section: { frontGapMm: number; backGapMm: number; heightMm: number; body:number[][]; cloth:number[][];
    rulerMm: { x:number; front:{bodyY:number;clothY:number}; back:{bodyY:number;clothY:number} } };
  report: {
    armDeg: number; sourcePc2Sha256: string; spacingMm: number;
    bodyFit: Record<string, { body_mm: number; residual_mm: number }>;
    fabric: { areal_kg_m2: number; bending_Nm: number };
  };
};
const variants = ["dress", "light"] as const;
const sides = ["front", "back"] as const;

function gapReadings(text: string) {
  const lines = text.split("\n").map(line => line.trim()).filter(Boolean);
  if (!lines.length) return null;
  let mean = 0, min = Infinity, max = 0;
  for (const [i, line] of lines.entries()) {
    const decimal = line.replace(",", ".");
    const mm = Number(decimal);
    if (!/^(?:\d+(?:\.\d*)?|\.\d+)$/.test(decimal) || !Number.isFinite(mm)) return null;
    mean += (mm - mean) / (i + 1);
    min = Math.min(min, mm);
    max = Math.max(max, mm);
  }
  return { lines, mean, min, max };
}

function signed(mm: number) {
  const rounded = Number(mm.toFixed(1));
  return `${rounded > 0 ? "+" : ""}${rounded.toFixed(1)}`;
}

export default function MuslinTrial({ cuts }: { cuts: Record<typeof variants[number], Sample> }) {
  const [readings, setReadings] = useState<Record<string, string>>({});
  const [notes, setNotes] = useState("");
  const [position, setPosition] = useState("dress-front");
  const { section, report } = cuts.dress;
  const rows = sides.map(side => {
    const key = side === "front" ? "frontGapMm" : "backGapMm";
    const pair = variants.map(id => {
      const field = `${id}-${side}`, raw = readings[field] ?? "", sample = gapReadings(raw);
      const ruler = cuts[id].section.rulerMm;
      return { field, raw, sample, side, ruler: {x:ruler.x,...ruler[side]}, invalid: !!raw.trim() && !sample, predicted: cuts[id].section[key], name: cuts[id].name };
    });
    const [a, b] = pair;
    return { side, pair, predictedChange: b.predicted - a.predicted,
      observedChange: a.sample && b.sample ? b.sample.mean - a.sample.mean : null };
  });
  const cells = rows.flatMap(row=>row.pair);
  const active = cells.find(cell=>cell.field===position)!;
  const description = `${active.name}, centre ${active.side}. The marked gap is ${active.predicted} mm.`;
  const steps = [
    "Print each pattern at 100%. Check the 100 mm and 4 inch squares across and down. Cut both skirts on the marked grain from the same conditioned cloth. Use one bodice and the same reversible side closure. Mark centre front and back before sewing.",
    "Baste each skirt to the same bodice in turn. Match centres and side seams, distribute the fullness evenly, and keep seam allowance, hem finish and pressing the same. Close the side opening on the stitching line. Check the finished waist seam measures 780 mm around before dressing the form.",
    `Fix the bare form in place with its arms at ${report.armDeg}°. At ${section.heightMm} mm above the soles mark the centre-front and centre-back plane. From a fixed horizontal reference, record the bare surface position at each mark. Dress the form and let the cloth settle without arranging its folds.`,
    "At each mark measure the horizontal distance from bare form to the nearest cloth surface on that same centreline. Record contact as 0 mm. Where folds overlap, use the innermost crossing. Leave a reading blank if you cannot locate it without moving the cloth. Repeat after re-dressing and note the spread.",
  ];
  const model = `Model bust ${report.bodyFit.chest.body_mm} mm (${signed(report.bodyFit.chest.residual_mm)} mm from target), waist ${report.bodyFit.waist.body_mm} mm (${signed(report.bodyFit.waist.residual_mm)} mm). Flat pattern bust 940 mm and waist 780 mm. Both digital cuts settled with a 1 mm cloth-contact setting on a ${report.spacingMm} mm mesh. The cloth uses synthetic ${report.fabric.areal_kg_m2 * 1000} g/m² and ${report.fabric.bending_Nm} N·m bending stiffness. The gathered run also uses a pleat approximation and a different initial back-panel radius.`;
  function saveSheet() {
    const lines = ["FrankenTextiles · Garden sundress muslin trial", "All distances in millimetres. Each observation is the mean of the recorded dressings for that cut and centreline. Changes are eased minus gathered. Residuals are observed minus predicted.", "", model, "", ...steps, ""];
    for (const id of variants) lines.push(`${cuts[id].name} · ${new URL(cuts[id].pdf, location.origin).href}`, `Native trajectory SHA-256 ${cuts[id].report.sourcePc2Sha256}`);
    for (const row of rows) {
      lines.push("", `Centre ${row.side}`);
      for (const { name, predicted, sample } of row.pair) {
        const mean = sample ? Number(sample.mean.toFixed(1)) : "____";
        lines.push(`${name}: predicted ${predicted} mm, observed ${mean} mm, residual ${sample ? signed(sample.mean - predicted) : "____"} mm`);
        lines.push(sample ? `  Readings (mm): ${sample.lines.join(" / ")}. Count ${sample.lines.length}. Range ${sample.min}–${sample.max} mm.`
          : "  Readings across dressings (mm): ____ / ____ / ____");
      }
      lines.push(`Eased minus gathered: predicted ${signed(row.predictedChange)} mm, observed ${row.observedChange === null ? "____" : signed(row.observedChange)} mm, residual ${row.observedChange === null ? "____" : signed(row.observedChange - row.predictedChange)} mm`);
    }
    lines.push("", "Form differences, cloth and finish, measuring instrument and uncertainty, unresolved folds or unexpected results:", notes || "________________________________________");
    const url = URL.createObjectURL(new Blob([lines.join("\n") + "\n"], { type: "text/plain;charset=utf-8" }));
    const link = document.createElement("a"); link.href = url; link.download = "sundress-muslin-trial.txt"; link.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
  return <section className="sd-toile" id="muslin" aria-labelledby="muslin-title">
    <div><p className="kicker">At the sewing table</p><h2 id="muslin-title">Try both skirts on one bodice.</h2>
      <p>Changing from gathered to eased gives a predicted waist-gap change of {signed(rows[0].predictedChange)} mm at centre front and {signed(rows[1].predictedChange)} mm at centre back. A negative change brings the skirt closer to the form.</p>
      <figure className="sd-toile-section"><label>Measure on the section
        <select value={position} onChange={e=>setPosition(e.target.value)}>{cells.map(cell=><option key={cell.field} value={cell.field}>{cell.name} · centre {cell.side}</option>)}</select></label>
        <TorsoSlice body={section.body} cloth={section.cloth} otherCut={cuts.light.section.cloth} compare ruler={{...active.ruler,description}}/>
        <figcaption><span>Form</span><span>Gathered</span><span>Eased</span></figcaption>
        <p><strong>{active.predicted} mm</strong> from the form to the first fold at centre {active.side}, {section.heightMm} mm above the soles.</p></figure>
      <nav aria-label="Muslin trial patterns">{variants.map(id => <a key={id} href={cuts[id].pdf} download>{cuts[id].name} · A4 PDF ↓</a>)}</nav>
      <details><summary>Set up the physical comparison</summary><ol>{steps.map(step => <li key={step}>{step}</li>)}</ol><p>{model}</p></details>
    </div>
    <div className="sd-toile-sheet"><h3>Waist gap worksheet</h3><p id="waist-gap-help">Enter one reading per line, in millimetres. Dress the form again for each repeat. The comparison uses the mean for each cut. Values stay in this tab until you save a copy.</p>
      {rows.map(row => <fieldset key={row.side}><legend>Centre {row.side}</legend><div className="sd-toile-pair">{row.pair.map(cell => <label key={cell.field}>{cell.name}
        <textarea aria-label={`${cell.name} centre ${row.side} in millimetres`} aria-describedby={`waist-gap-help ${cell.field}-summary`} rows={2} autoComplete="off" spellCheck={false} value={cell.raw} placeholder="mm" aria-invalid={cell.invalid}
          onFocus={()=>setPosition(cell.field)} onChange={e => { const value=e.target.value; setReadings(current=>({...current,[cell.field]:value})); }}/>
        <small id={`${cell.field}-summary`}>{cell.invalid ? "Enter one distance per line. Use zero for contact." : `Predicted ${cell.predicted} mm`}
          {cell.sample && cell.sample.lines.length > 1 && ` · ${cell.sample.lines.length} readings · ${cell.sample.min}–${cell.sample.max} mm · mean ${cell.sample.mean.toFixed(1)} mm`}</small></label>)}</div>
        <output aria-live="polite">Eased minus gathered · predicted {signed(row.predictedChange)} mm{row.observedChange === null ? "" : ` · observed ${signed(row.observedChange)} mm · residual ${signed(row.observedChange - row.predictedChange)} mm`}</output>
      </fieldset>)}
      <label>Form, cloth, measuring uncertainty and discrepancies<textarea value={notes} rows={2} autoComplete="off" spellCheck={false} onChange={e => setNotes(e.target.value)}/></label>
      <button type="button" onClick={saveSheet} disabled={rows.some(row => row.pair.some(cell => cell.invalid))}>Save worksheet (.txt) ↓</button>
    </div>
  </section>;
}
