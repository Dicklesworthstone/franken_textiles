//! Offline authoring of source-bound sharpness regions; rasterization stays native.

use ftx_core::Refusal;
use serde_json::Value;

use crate::bad_input;

pub(super) const SCRIPT: &str = include_str!("editor.js");

pub(super) fn html(annotations: &Value, images: &[Value]) -> Result<Vec<u8>, Refusal> {
    let frames = annotations["frames"]
        .as_array()
        .ok_or_else(|| bad_input("mask review requires frame annotations"))?;
    if frames.is_empty() || frames.len() != images.len() {
        return Err(bad_input(
            "mask review images and frame annotations disagree",
        ));
    }
    for (i, image) in images.iter().enumerate() {
        let file = image["file"].as_str().unwrap_or_default();
        if file
            .strip_prefix(&format!("source-{i:04}."))
            .is_none_or(|extension| !matches!(extension, "png" | "jpg"))
            || image["label"].as_str().is_none()
        {
            return Err(bad_input(
                "mask review requires local generated image names and labels",
            ));
        }
    }
    // '<' escaping makes even an authored '</script>' inert. Values never enter HTML attributes.
    let annotations = serde_json::to_string(annotations)
        .map_err(|e| bad_input(format!("mask review annotations: {e}")))?
        .replace('<', "\\u003c");
    let images = serde_json::to_string(images)
        .map_err(|e| bad_input(format!("mask review images: {e}")))?
        .replace('<', "\\u003c");
    Ok([HEAD, &annotations, MIDDLE, &images, TAIL]
        .concat()
        .into_bytes())
}

const HEAD: &str = r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src 'self'; script-src 'self'; style-src 'unsafe-inline'">
<title>Review video sharpness regions</title>
<style>
body{font:16px system-ui,sans-serif;max-width:1100px;margin:2rem auto;padding:0 1rem;color:#17212b;background:#fafafa}
h1{font-size:1.7rem}fieldset{border:1px solid #bbc7cf;border-radius:.4rem;padding:1rem}button,select,input{font:inherit;margin:.25rem;padding:.4rem}button:disabled{opacity:.5}
.row{display:flex;gap:.4rem;flex-wrap:wrap;align-items:center;margin:.5rem 0}#frame{max-width:35rem}#status{min-height:3rem}#label{overflow-wrap:anywhere}
svg{display:block;width:100%;max-height:75vh;border:1px solid #899ba8;background:#ddd;touch-action:manipulation}svg[data-drawing="true"]{cursor:crosshair}a{color:#075985}#progress{font-weight:600}
</style></head><body>
<h1>Review video sharpness regions</h1>
<p>Mark the person and reference sheet on the original, already-oriented pixels. These manual regions select pixels for sharpness scoring; they do not establish anatomy, stillness, silhouette accuracy or body fit. All images and edits stay local.</p>
<fieldset id="controls" disabled><legend>Mask editor</legend>
<div class="row"><button id="previous" type="button">Previous frame</button><label>Frame <select id="frame"></select></label><button id="next" type="button">Next frame</button></div>
<div class="row"><label>Target <select id="target"><option value="person_regions">Person (orange)</option><option value="sheet_regions">Sheet (blue)</option></select></label><label>Component <select id="region"></select></label></div>
<div class="row"><button id="new-region" type="button">New component</button><button id="new-hole" type="button">New hole in component</button><button id="undo" type="button">Undo vertex</button><button id="finish" type="button">Finish polygon</button><button id="cancel" type="button">Cancel polygon</button></div>
<div class="row"><button id="remove-region" type="button">Remove component</button><button id="remove-hole" type="button">Remove last hole</button><button id="not-visible" type="button">Target not visible</button><button id="reset" type="button">Reset all review</button></div>
<div class="row"><label>Resume a source-matching draft <input id="import" type="file" accept="application/json,.json"></label><a id="mask-download" download="mask-polygons.json" hidden>Download draft</a></div>
</fieldset>
<p id="progress"></p><p id="status" role="status" aria-live="polite">Loading the local editor…</p><p id="label"></p>
<svg id="canvas" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1" aria-label="Original frame with editable sharpness regions"><image id="frame-image" x="0" y="0"/><g id="regions"></g><g id="draft"></g></svg>
<p>Start a component, click its boundary, then finish it. A hole subtracts pixels only from its own component; overlapping components are united. Clicks snap to the nearest integer pixel. Use “Target not visible” for an explicitly empty mask. Unreviewed targets remain null in drafts.</p>
<p>Save a draft to resume later. Finish or cancel the active polygon before downloading. Native <code>ftx measure video-masks mask-polygons.json --out NEW_DIR</code> validates every polygon and source before creating binary masks; it refuses unfinished targets. Run <code>video-select</code> on the resulting <code>frames.json</code>.</p>
<noscript><p>This local editor requires JavaScript. Original images remain available as the source image files beside this page.</p></noscript>
<script id="annotations" type="application/json">"##;

const MIDDLE: &str = r#"</script><script id="frame-images" type="application/json">"#;
const TAIL: &str = r#"</script><script src="editor.js" defer></script></body></html>
"#;
