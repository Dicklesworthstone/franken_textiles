// Stored pixels and caller-authored correspondences only. No fitting or anatomical inference.
(() => {
  const byId = id => document.getElementById(id);
  const link = byId("correctedInput"), status = byId("editStatus");
  let download = null;
  function withdraw() {
    if (download !== null) URL.revokeObjectURL(download);
    download = null; link.hidden = true; link.removeAttribute("href");
  }
  try {
    const original = JSON.parse(byId("captureInput").textContent);
    // A u64 decoded timestamp may exceed JavaScript's exact integer range. Refuse editing the
    // document instead of quietly changing its source identity during JSON parse/stringify.
    function requireSafeNumbers(value) {
      if (typeof value === "number" && (!Number.isFinite(value) ||
          (Number.isInteger(value) && !Number.isSafeInteger(value)))) {
        throw Error("Input exceeds safe browser numeric precision; edit the original JSON with an integer-preserving tool");
      }
      if (value !== null && typeof value === "object") Object.values(value).forEach(requireSafeNumbers);
    }
    requireSafeNumbers(original);
    let edited = structuredClone(original), selected = null, pending = false;
    const sections = [...document.querySelectorAll("section")];
    const buttons = [...document.querySelectorAll(".edit-point")];
    if (original.fit.frames.length !== sections.length) throw Error("Frame identities disagree with the review");
    function render() {
      withdraw();
      let changed = 0;
      edited.fit.frames.forEach((frame, i) => {
        const rows = sections[i].querySelectorAll("tbody tr");
        frame.keypoints.forEach((point, j) => {
          const [x, y] = point.pixel_px, before = original.fit.frames[i].keypoints[j].pixel_px;
          const different = x !== before[0] || y !== before[1];
          if (different) ++changed;
          rows[j].dataset.changed = String(different);
          rows[j].querySelector(".point-coordinates").textContent = x + ", " + y;
          const mark = sections[i].querySelector('g[data-point="' + j + '"]');
          mark.querySelector("circle").setAttribute("cx", x);
          mark.querySelector("circle").setAttribute("cy", y);
          mark.querySelector("text").setAttribute("x", x + 7);
          mark.querySelector("text").setAttribute("y", y - 7);
        });
      });
      if (changed && !pending) {
        download = URL.createObjectURL(new Blob([JSON.stringify(edited, null, 2) + "\n"], {type: "application/json"}));
        link.href = download; link.hidden = false;
      }
      return changed;
    }
    function select(i, j) {
      selected = [i, j]; pending = false;
      const frame = edited.fit.frames[i], point = frame.keypoints[j];
      buttons.forEach(b => b.setAttribute("aria-pressed", String(Number(b.dataset.frame) === i && Number(b.dataset.point) === j)));
      sections.forEach((s, index) => s.querySelector("svg").dataset.editing = String(index === i));
      ["editX", "editY", "applyPoint"].forEach(id => byId(id).disabled = false);
      const size = frame.source.image_size_px;
      ["editX", "editY"].forEach((id, axis) => {
        byId(id).value = point.pixel_px[axis]; byId(id).min = 0; byId(id).max = size[axis] - 1;
      });
      const count = render();
      status.textContent = "Selected " + point.name + " (" + (point.partition === "fit" ? "FIT" : "CHECK") +
        ") in " + frame.source.metadata.frame.stream + " @ " + frame.source.metadata.frame.timestamp_us +
        " µs. " + count + " corrected point(s).";
    }
    function apply(x, y) {
      withdraw();
      if (selected === null) return;
      const [i, j] = selected, size = edited.fit.frames[i].source.image_size_px;
      if (![x, y].every(Number.isFinite) || x < 0 || y < 0 || x > size[0] - 1 || y > size[1] - 1) {
        pending = true;
        status.textContent = "Coordinates must be finite and inside 0.." + (size[0] - 1) + ", 0.." +
          (size[1] - 1) + ". No correction or download published.";
        return;
      }
      // Only pixel coordinates change. Source identities, targets, sigmas and predeclared
      // FIT/CHECK assignments are never inferred, reassigned or removed.
      edited.fit.frames[i].keypoints[j].pixel_px = [x, y];
      pending = false; byId("editX").value = x; byId("editY").value = y;
      status.textContent = render() + " corrected point(s). Download the current input and create a new review before fitting. Original preparation files stay unchanged.";
    }
    buttons.forEach(button => {
      button.hidden = false;
      button.addEventListener("click", () => select(Number(button.dataset.frame), Number(button.dataset.point)));
    });
    sections.forEach((section, i) => {
      const svg = section.querySelector("svg");
      svg.addEventListener("click", event => {
        if (selected === null || selected[0] !== i) {
          status.textContent = "Select a point belonging to this image before correcting it.";
          return;
        }
        const matrix = svg.getScreenCTM();
        if (matrix === null) { withdraw(); status.textContent = "Image coordinates unavailable; no download published."; return; }
        const pixel = new DOMPoint(event.clientX, event.clientY).matrixTransform(matrix.inverse());
        apply(pixel.x, pixel.y);
      });
    });
    ["editX", "editY"].forEach(id => byId(id).addEventListener("input", () => {
      pending = true; withdraw(); status.textContent = "Coordinate edit pending. Apply valid coordinates or select a point again; no download is available.";
    }));
    byId("coordinateForm").addEventListener("submit", event => {
      event.preventDefault();
      const values = ["editX", "editY"].map(id => byId(id).value.trim());
      apply(...values.map(value => value === "" ? NaN : Number(value)));
    });
    byId("resetPoints").addEventListener("click", () => {
      edited = structuredClone(original); selected = null; pending = false; render();
      buttons.forEach(b => b.setAttribute("aria-pressed", "false"));
      sections.forEach(s => s.querySelector("svg").dataset.editing = "false");
      ["editX", "editY", "applyPoint"].forEach(id => byId(id).disabled = true);
      byId("editX").value = ""; byId("editY").value = "";
      status.textContent = "All corrections removed. Select a point; no corrected input is available.";
    });
    window.addEventListener("pagehide", withdraw);
    render(); byId("captureEditor").hidden = false;
    status.textContent = "Select a point in its table. Only its pixel coordinates can change.";
  } catch (error) {
    withdraw();
    document.querySelectorAll(".edit-point").forEach(b => b.disabled = true);
    byId("captureEditor").hidden = false;
    status.textContent = "Editor unavailable: " + error.message + ". Use the unchanged original input and review.";
  }
})();
