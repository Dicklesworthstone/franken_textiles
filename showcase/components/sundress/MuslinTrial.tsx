"use client";

import { useState } from "react";

type Sample = {
  name: string;
  pdf: string;
  section: { frontGapMm: number; backGapMm: number; heightMm: number };
  report: {
    armDeg: number; sourcePc2Sha256: string; spacingMm: number;
    bodyFit: Record<string, { body_mm: number; residual_mm: number }>;
    fabric: { areal_kg_m2: number; bending_Nm: number };
  };
};
const variants = ["dress", "light"] as const;
const sides = ["front", "back"] as const;

function parseGap(text: string) {
  const decimal = text.trim().replace(",", ".");
  if (!/^(?:\d+(?:\.\d*)?|\.\d+)$/.test(decimal)) return null;
  const mm = Number(decimal);
  return Number.isFinite(mm) ? mm : null;
}

function signed(mm: number) {
  const rounded = Number(mm.toFixed(1));
  return `${rounded > 0 ? "+" : ""}${rounded.toFixed(1)}`;
}

export default function MuslinTrial({ cuts }: { cuts: Record<typeof variants[number], Sample> }) {
  const [readings, setReadings] = useState<Record<string, string>>({});
  const [notes, setNotes] = useState("");
  const { section, report } = cuts.dress;
  const rows = sides.map(side => {
    const key = side === "front" ? "frontGapMm" : "backGapMm";
    const pair = variants.map(id => {
      const field = `${id}-${side}`, raw = readings[field] ?? "", observed = parseGap(raw);
      return { field, raw, observed, invalid: raw.trim() !== "" && observed === null, predicted: cuts[id].section[key], name: cuts[id].name };
    });
    const [a, b] = pair;
    return { side, pair, predictedChange: b.predicted - a.predicted,
      observedChange: a.observed === null || b.observed === null ? null : b.observed - a.observed };
  });
  const steps = [
    "Print each pattern at 100%. Check the 100 mm and 4 inch squares across and down. Cut both skirts on the marked grain from the same conditioned cloth. Use one bodice and the same reversible side closure. Mark centre front and back before sewing.",
    "Baste each skirt to the same bodice in turn. Match centres and side seams, distribute the fullness evenly, and keep seam allowance, hem finish and pressing the same. Close the side opening on the stitching line. Check the finished waist seam measures 780 mm around before dressing the form.",
    `Fix the bare form in place with its arms at ${report.armDeg}°. At ${section.heightMm} mm above the soles mark the centre-front and centre-back plane. From a fixed horizontal reference, record the bare surface position at each mark. Dress the form and let the cloth settle without arranging its folds.`,
    "At each mark measure the horizontal distance from bare form to the nearest cloth surface on that same centreline. Record contact as 0 mm. Where folds overlap, use the innermost crossing. Leave a reading blank if you cannot locate it without moving the cloth. Repeat after re-dressing and note the spread.",
  ];
  const model = `Model bust ${report.bodyFit.chest.body_mm} mm (${signed(report.bodyFit.chest.residual_mm)} mm from target), waist ${report.bodyFit.waist.body_mm} mm (${signed(report.bodyFit.waist.residual_mm)} mm). Flat pattern bust 940 mm and waist 780 mm. Both digital cuts settled with a 1 mm cloth-contact setting on a ${report.spacingMm} mm mesh. The cloth uses synthetic ${report.fabric.areal_kg_m2 * 1000} g/m² and ${report.fabric.bending_Nm} N·m bending stiffness. The gathered run also uses a pleat approximation and a different initial back-panel radius.`;
  function saveSheet() {
    const lines = ["FrankenTextiles · Garden sundress muslin trial", "All distances in millimetres. Changes are eased minus gathered. Residuals are observed minus predicted.", "", model, "", ...steps, ""];
    for (const id of variants) lines.push(`${cuts[id].name} · ${new URL(cuts[id].pdf, location.origin).href}`, `Native trajectory SHA-256 ${cuts[id].report.sourcePc2Sha256}`);
    for (const row of rows) {
      lines.push("", `Centre ${row.side}`);
      for (const cell of row.pair) lines.push(`${cell.name}: predicted ${cell.predicted} mm, observed ${cell.observed ?? "____"} mm, residual ${cell.observed === null ? "____" : signed(cell.observed - cell.predicted)} mm`);
      lines.push(`Eased minus gathered: predicted ${signed(row.predictedChange)} mm, observed ${row.observedChange === null ? "____" : signed(row.observedChange)} mm, residual ${row.observedChange === null ? "____" : signed(row.observedChange - row.predictedChange)} mm`);
    }
    lines.push("", "Form differences, cloth and finish, repeat spread, unresolved folds or unexpected results:", notes || "________________________________________");
    const url = URL.createObjectURL(new Blob([lines.join("\n") + "\n"], { type: "text/plain;charset=utf-8" }));
    const link = document.createElement("a"); link.href = url; link.download = "sundress-muslin-trial.txt"; link.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
  return <section className="sd-toile" id="muslin" aria-labelledby="muslin-title">
    <div><p className="kicker">At the sewing table</p><h2 id="muslin-title">Try both skirts on one bodice.</h2>
      <p>Changing from gathered to eased gives a predicted waist-gap change of {signed(rows[0].predictedChange)} mm at centre front and {signed(rows[1].predictedChange)} mm at centre back. A negative change brings the skirt closer to the form.</p>
      <nav aria-label="Muslin trial patterns">{variants.map(id => <a key={id} href={cuts[id].pdf} download>{cuts[id].name} · A4 PDF ↓</a>)}</nav>
      <details><summary>Set up the physical comparison</summary><ol>{steps.map(step => <li key={step}>{step}</li>)}</ol><p>{model}</p></details>
    </div>
    <div className="sd-toile-sheet"><h3>Waist gap worksheet</h3><p>Enter readings from the sewn samples in millimetres. Values stay in this tab. Save a blank copy to take to the cutting table or a completed copy for your records.</p>
      {rows.map(row => <fieldset key={row.side}><legend>Centre {row.side}</legend><div className="sd-toile-pair">{row.pair.map(cell => <label key={cell.field}>{cell.name}
        <input aria-label={`${cell.name} centre ${row.side} in millimetres`} inputMode="decimal" maxLength={16} autoComplete="off" value={cell.raw} placeholder="mm" aria-invalid={cell.invalid}
          onChange={e => setReadings({ ...readings, [cell.field]: e.target.value })}/>
        <small>{cell.invalid ? "Enter zero or a positive distance." : `Predicted ${cell.predicted} mm`}</small></label>)}</div>
        <output aria-live="polite">Eased minus gathered · predicted {signed(row.predictedChange)} mm{row.observedChange === null ? "" : ` · observed ${signed(row.observedChange)} mm · residual ${signed(row.observedChange - row.predictedChange)} mm`}</output>
      </fieldset>)}
      <label>Form, cloth, repeat readings and discrepancies<textarea value={notes} rows={2} onChange={e => setNotes(e.target.value)}/></label>
      <button type="button" onClick={saveSheet} disabled={rows.some(row => row.pair.some(cell => cell.invalid))}>Save worksheet (.txt) ↓</button>
    </div>
  </section>;
}
