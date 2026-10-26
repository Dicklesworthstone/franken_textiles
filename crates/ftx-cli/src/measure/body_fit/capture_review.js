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
    let edited = structuredClone(original), selected = null, pending = false, draft = null;
    const sections = [...document.querySelectorAll("section")];
    const buttons = () => [...document.querySelectorAll(".edit-point")];
    if (original.fit.frames.length !== sections.length) throw Error("Frame identities disagree with the review");
    function bindButton(button) {
      button.hidden = false;
      button.addEventListener("click", () => select(Number(button.dataset.frame), Number(button.dataset.point)));
    }
    function selectionDisplay() {
      buttons().forEach(b => b.setAttribute("aria-pressed",
        String(selected !== null && Number(b.dataset.frame) === selected[0] && Number(b.dataset.point) === selected[1])));
      sections.forEach((s, i) => s.querySelector("svg").dataset.editing = String(selected !== null && selected[0] === i));
      byId("cancelPoint").disabled = draft === null;
    }
    function targetOptions() {
      const chooser = byId("newTarget"), old = chooser.value, frame = edited.fit.frames[Number(byId("newFrame").value)];
      chooser.replaceChildren(new Option("Choose target", ""));
      if (byId("newFrame").value !== "" && frame) {
        for (const name of Object.keys(original.model.keypoints).sort()) {
          if (!frame.keypoints.some(p => p.name === name)) chooser.add(new Option(name, name));
        }
      }
      chooser.value = [...chooser.options].some(o => o.value === old) ? old : "";
      const name = chooser.value;
      byId("targetDefinition").textContent = name && Object.hasOwn(original.model.keypoints, name)
        ? "Declared target: " + JSON.stringify(original.model.keypoints[name]) : "";
    }
    function appendDisplay(i, j, point) {
      const row = document.createElement("tr");
      for (let k = 0; k < 5; ++k) row.append(document.createElement("td"));
      const button = document.createElement("button");
      button.type = "button"; button.className = "edit-point"; button.textContent = "Select";
      button.dataset.frame = i; button.dataset.point = j; bindButton(button);
      row.children[0].append(button, document.createTextNode(" " + point.name));
      row.children[1].textContent = point.partition === "fit" ? "FIT" : "CHECK";
      row.children[2].className = "point-coordinates";
      row.children[3].textContent = point.sigma_px;
      row.children[4].textContent = JSON.stringify(original.model.keypoints[point.name]);
      sections[i].querySelector("tbody").append(row);
      const svg = name => document.createElementNS("http://www.w3.org/2000/svg", name);
      const mark = svg("g"), title = svg("title"), circle = svg("circle"), text = svg("text");
      mark.dataset.point = j; mark.dataset.partition = row.children[1].textContent;
      title.textContent = point.name + ": " + mark.dataset.partition + "; sigma " + point.sigma_px +
        " px; " + row.children[4].textContent;
      const color = point.partition === "fit" ? "#b45309" : "#0369a1";
      circle.setAttribute("r", 5); circle.setAttribute("fill", "none");
      circle.setAttribute("stroke", color); circle.setAttribute("stroke-width", 2);
      text.textContent = point.name + " (" + mark.dataset.partition + ")";
      text.setAttribute("fill", color); text.setAttribute("font-family", "sans-serif"); text.setAttribute("font-size", 12);
      mark.append(title, circle, text); sections[i].querySelector("svg").append(mark);
    }
    function render() {
      withdraw();
      let changed = 0;
      edited.fit.frames.forEach((frame, i) => {
        const body = sections[i].querySelector("tbody");
        while (body.children.length > frame.keypoints.length) body.lastElementChild.remove();
        [...sections[i].querySelectorAll("g[data-point]")].slice(frame.keypoints.length).forEach(n => n.remove());
        frame.keypoints.forEach((point, j) => {
          if (j >= body.children.length) appendDisplay(i, j, point);
          const row = body.children[j], [x, y] = point.pixel_px, before = original.fit.frames[i].keypoints[j]?.pixel_px;
          const different = before === undefined || x !== before[0] || y !== before[1];
          if (different) ++changed;
          row.dataset.changed = String(different);
          row.querySelector(".point-coordinates").textContent = x + ", " + y;
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
      selectionDisplay();
      return changed;
    }
    function select(i, j) {
      selected = [i, j]; pending = false; draft = null;
      const frame = edited.fit.frames[i], point = frame.keypoints[j];
      ["editX", "editY", "applyPoint"].forEach(id => byId(id).disabled = false);
      const size = frame.source.image_size_px;
      ["editX", "editY"].forEach((id, axis) => {
        byId(id).value = point.pixel_px[axis]; byId(id).min = 0; byId(id).max = size[axis] - 1;
      });
      const count = render();
      status.textContent = "Selected " + point.name + " (" + (point.partition === "fit" ? "FIT" : "CHECK") +
        ") in " + frame.source.metadata.frame.stream + " @ " + frame.source.metadata.frame.timestamp_us +
        " µs. " + count + " new or corrected point(s).";
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
      if (draft !== null) {
        const points = edited.fit.frames[i].keypoints;
        if (points.length >= 128 || points.some(p => p.name === draft.name)) {
          pending = true; status.textContent = "Duplicate target or 128-observation frame budget; no point published."; return;
        }
        points.push({name: draft.name, pixel_px: [x, y], sigma_px: draft.sigma_px, partition: draft.partition});
        selected = [i, points.length - 1]; draft = null;
        byId("newTarget").value = ""; byId("newPartition").value = ""; byId("newSigma").value = "";
        targetOptions();
      } else {
        // Existing observations permit coordinates only; their roles and uncertainties stay fixed.
        edited.fit.frames[i].keypoints[j].pixel_px = [x, y];
      }
      pending = false; byId("editX").value = x; byId("editY").value = y;
      status.textContent = render() + " new or corrected point(s). Download the current input and create a new review before fitting. Original preparation files stay unchanged.";
    }
    buttons().forEach(bindButton);
    original.fit.frames.forEach((frame, i) => {
      const id = frame.source.metadata.frame;
      byId("newFrame").add(new Option(id.stream + " @ " + id.timestamp_us + " µs", String(i)));
    });
    function stopPlacement() {
      draft = null; selected = null; pending = true; withdraw(); selectionDisplay();
      ["editX", "editY", "applyPoint"].forEach(id => byId(id).disabled = true);
      byId("editX").value = ""; byId("editY").value = "";
    }
    ["newFrame", "newTarget", "newPartition", "newSigma"].forEach(id => {
      byId(id).addEventListener(id === "newSigma" ? "input" : "change", () => {
        stopPlacement();
        if (id === "newFrame") byId("newTarget").value = "";
        targetOptions(); status.textContent = "Declare frame, target, role and sigma, then prepare a new point. No download published.";
      });
    });
    byId("preparePoint").addEventListener("click", () => {
      stopPlacement();
      const value = byId("newFrame").value, i = Number(value), name = byId("newTarget").value;
      const partition = byId("newPartition").value, rawSigma = byId("newSigma").value.trim(), sigma = Number(rawSigma);
      const frame = edited.fit.frames[i];
      if (value === "" || !Number.isInteger(i) || !frame || !Object.hasOwn(original.model.keypoints, name) ||
          !name.trim() || new TextEncoder().encode(name).length > 4096 ||
          !["fit", "held_out"].includes(partition) || !rawSigma || !Number.isFinite(sigma) || sigma < 1e-6 || sigma > 1e6) {
        status.textContent = "Choose a frame, declared target, FIT/CHECK and a finite sigma in 1e-6..1e6 px before placement."; return;
      }
      if (frame.keypoints.length >= 128 || frame.keypoints.some(p => p.name === name)) {
        status.textContent = "Duplicate target or 128-observation frame budget; no point published."; return;
      }
      draft = {name, partition, sigma_px: sigma}; selected = [i, -1]; selectionDisplay();
      ["editX", "editY", "applyPoint"].forEach(id => byId(id).disabled = false);
      status.textContent = "Placing " + name + " (" + (partition === "fit" ? "FIT" : "CHECK") +
        ", sigma " + sigma + " px). Click this frame or enter coordinates; no point has been added yet.";
    });
    byId("cancelPoint").addEventListener("click", () => {
      stopPlacement(); status.textContent = "Placement cancelled. No new point or download published. Select a point to continue.";
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
      edited = structuredClone(original); selected = null; pending = false; draft = null; render();
      byId("newFrame").value = ""; byId("newTarget").value = "";
      byId("newPartition").value = ""; byId("newSigma").value = ""; targetOptions();
      ["editX", "editY", "applyPoint"].forEach(id => byId(id).disabled = true);
      byId("editX").value = ""; byId("editY").value = "";
      status.textContent = "All corrections removed. Select a point; no corrected input is available.";
    });
    window.addEventListener("pagehide", withdraw);
    render(); byId("newPointFields").disabled = false; byId("captureEditor").hidden = false;
    status.textContent = "Select a point in its table, or declare a new observation before placing it.";
  } catch (error) {
    withdraw();
    document.querySelectorAll(".edit-point").forEach(b => b.disabled = true);
    byId("captureEditor").hidden = false;
    status.textContent = "Editor unavailable: " + error.message + ". Use the unchanged original input and review.";
  }
})();
