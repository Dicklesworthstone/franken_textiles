//! Local setup choices over a native catalogue; no browser camera/model reconstruction.

use ftx_core::Refusal;
use serde_json::{Value, json};

use super::review::Bundle;
use crate::bad_input;

pub(super) fn attach(bundle: &mut Bundle, data: &Value) -> Result<Value, Refusal> {
    let script = bundle.push(
        "setup-editor.js".into(),
        include_bytes!("setup_edit.js").to_vec(),
    )?;
    let viewer = bundle.push("index.html".into(), page(data)?.into_bytes())?;
    Ok(json!({"viewer":viewer,"script":script}))
}

fn page(data: &Value) -> Result<String, Refusal> {
    let payload = serde_json::to_string(data)
        .map_err(|e| bad_input(e.to_string()))?
        .replace('<', "\\u003c");
    Ok(format!(
        r##"<!doctype html><html lang="en"><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src 'self'; script-src 'self'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'">
<title>Set up a body capture</title>
<style>
body{{font:16px system-ui;max-width:1150px;margin:2rem auto;padding:0 1rem;color:#172033;background:#fff}}p{{line-height:1.5}}h2{{margin-top:2rem}}button,input,select,textarea{{font:inherit;padding:.35rem}}button{{margin:.2rem}}label{{display:block;margin:.6rem 0}}input[type=number]{{max-width:11rem}}textarea{{display:block;box-sizing:border-box;width:100%;min-height:5rem}}fieldset{{border:1px solid #aab4c2;margin:1rem 0;padding:1rem}}legend{{font-weight:600}}small{{display:block;color:#475569}}.frame-grid{{display:grid;grid-template-columns:repeat(auto-fit,minmax(230px,1fr));gap:1rem}}.frame-card{{border:1px solid #aab4c2;padding:.7rem;overflow-wrap:anywhere}}.frame-card img{{display:block;width:100%;height:auto;max-height:330px;object-fit:contain;background:#eef2f7}}.frame-card label{{font-weight:600}}.frame-card.ineligible{{background:#f2f4f7}}.fields{{display:flex;flex-wrap:wrap;gap:.6rem 1.2rem}}.fields label{{margin:.4rem 0}}.actions{{position:sticky;bottom:0;background:#fff;border-top:2px solid #64748b;padding:.7rem 0;margin-top:2rem}}#setupStatus{{min-height:3rem}}a{{color:#075985}}[hidden]{{display:none!important}}pre{{white-space:pre-wrap;overflow-wrap:anywhere}}
</style>
<h1>Set up a body capture</h1>
<p>Choose exact decoded frames and the model controls you intend to fit. This page prepares a declaration for the existing capture and body-fit workflow. Cameras, anatomical correspondences, observations and initialization vectors are reviewed in the following steps; no body has been estimated here.</p>
<p>All images are original native copies from this setup. Labels retain exact decoded timestamps. The page runs locally and sends no images or choices anywhere.</p>
<div id="setupEditor" hidden>
<h2>1. Choose 2–8 compatible frames</h2>
<p>Use one acquisition cohort with the supplied checked calibration. A compatible image can still be blurred, occluded or unsuitable; inspect the source photographs. Selection establishes neither stillness nor independent views.</p>
<p id="setupFrameCount" role="status"></p><div class="frame-grid" id="setupFrames"></div>
<h2>2. Declare shared model inputs</h2>
<div class="fields"><label>Age in years, 0–110 <input id="setupAge" type="number" min="0" max="110" step="any"></label>
<label>Model gender parameter, 0–1 <input id="setupGender" type="number" min="0" max="1" step="any"></label></div>
<p>The model's gender parameter runs from 0 (male) to 1 (female). Age and gender are fixed shared inputs for this capture.</p>
<label>Metric scale source <textarea id="setupScale" maxlength="4096"></textarea></label>
<label>Observation and capture evidence <textarea id="setupEvidence" maxlength="4096"></textarea></label>
<h2>3. Choose shape controls</h2>
<p>Add each control you want to declare explicitly. Choose a fixed value or an exclusive fitting range. At least one and at most twelve shape controls must be free. New rows leave the choice and numbers blank.</p>
<label>Model shape control <select id="setupShapeName"></select></label><button id="addSetupShape" type="button">Add shape control</button><div id="setupShapes"></div>
<h2>4. Declare root placement and joint motion</h2>
<p>All six root components are shown explicitly. The original setup seeds them as fixed zero; choose each component you intend to fit and enter its bounds. Translations use metres in the calibrated world frame; root rotations are rotation-vector components in radians. The combined fixed rotation vector must have length at most π. At least one and at most eighteen root/joint controls must be free.</p>
<div id="setupRoots"></div>
<p>Joint axes are explicit unit vectors in the selected bone's local frame. Enter all three components; the editor does not invent or normalize an axis. Unselected joints remain in the model's rest pose.</p>
<label>Articulated model bone <select id="setupJointName"></select></label><button id="addSetupJoint" type="button">Add joint control</button><div id="setupJoints"></div>
<details><summary>Model defaults for controls you have not selected</summary><div id="setupDefaults"></div></details>
<h2>5. Optional independent tape observations</h2>
<p>Declare the named tape functional, measured value and uncertainty in metres, evidence and its predeclared FIT or CHECK role. CHECK observations are held out of fitting. Leave this section empty if no independent tape observations are available.</p>
<label>Tape functional <select id="setupMeasurementName"></select></label><button id="addSetupMeasurement" type="button">Add tape observation</button><div id="setupMeasurements"></div>
<div class="actions"><button id="applySetup" type="button">Check choices and prepare download</button> <button id="cancelSetup" type="button">Cancel pending edits</button> <button id="resetSetup" type="button">Reset to original setup</button>
<p id="setupStatus" role="status"></p><a id="setupDownload" download="capture-choices.json" hidden>Download capture-choices.json</a></div>
<p>Apply the downloaded choices with <code>ftx measure capture-init capture-choices.json --model MODEL --out NEW_DIR</code>. The native command checks source bindings, camera compatibility, parameter ranges and axes again and writes an ordinary capture input. Then author model targets, review image observations and author fitting starts with the existing commands. No fit acceptance or READY measurement follows from setup.</p>
</div>
<noscript><p>The supplied local script is required to choose frames and controls. Original setup and source files remain available in this directory.</p></noscript>
<script type="application/json" id="setupData">{payload}</script><script src="setup-editor.js" defer></script></html>
"##
    ))
}
