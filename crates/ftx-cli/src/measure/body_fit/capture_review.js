// Stored pixels and caller-authored observations only. No fitting or anatomical inference.
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
    let silhouettePending = false, silhouetteDrawing = null;
    const sections = [...document.querySelectorAll("section")];
    const silhouetteEditable = sections.map(section => section.dataset.silhouetteEditable === "true");
    const buttons = () => [...document.querySelectorAll(".edit-point")];
    if (original.fit.frames.length !== sections.length) throw Error("Frame identities disagree with the review");
    function bindButton(button) {
      button.hidden = false;
      button.addEventListener("click", () => select(Number(button.dataset.frame), Number(button.dataset.point)));
    }
    function selectionDisplay() {
      buttons().forEach(b => b.setAttribute("aria-pressed",
        String(selected !== null && Number(b.dataset.frame) === selected[0] && Number(b.dataset.point) === selected[1])));
      sections.forEach((s, i) => s.querySelector("svg").dataset.editing = String(
        selected !== null && selected[0] === i || silhouetteDrawing !== null && silhouetteDrawing.frame === i));
      byId("cancelPoint").disabled = draft === null;
      byId("removePoint").disabled = selected === null || draft !== null;
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
          const row = body.children[j], [x, y] = point.pixel_px;
          const before = original.fit.frames[i].keypoints.find(other => other.name === point.name);
          const different = before === undefined ||
            point.partition !== before.partition || point.sigma_px !== before.sigma_px ||
            x !== before.pixel_px[0] || y !== before.pixel_px[1];
          if (different) ++changed;
          row.dataset.changed = String(different);
          row.querySelector(".point-coordinates").textContent = x + ", " + y;
          const mark = sections[i].querySelector('g[data-point="' + j + '"]');
          mark.querySelector("circle").setAttribute("cx", x);
          mark.querySelector("circle").setAttribute("cy", y);
          mark.querySelector("text").setAttribute("x", x + 7);
          mark.querySelector("text").setAttribute("y", y - 7);
        });
        changed += original.fit.frames[i].keypoints.filter(before =>
          !frame.keypoints.some(point => point.name === before.name)).length;
        if (JSON.stringify(frame.silhouette) !== JSON.stringify(original.fit.frames[i].silhouette)) ++changed;
        if (silhouetteEditable[i]) renderSilhouette(i);
      });
      if (changed && !pending && !silhouettePending) {
        download = URL.createObjectURL(new Blob([JSON.stringify(edited, null, 2) + "\n"], {type: "application/json"}));
        link.href = download; link.hidden = false;
      }
      selectionDisplay();
      return changed;
    }
    function clearPointDisplay(i) {
      sections[i].querySelector("tbody").replaceChildren();
      sections[i].querySelectorAll("g[data-point]").forEach(node => node.remove());
    }
    function select(i, j) {
      clearSilhouetteDrawing();
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
        " µs. " + count + " changed point(s) or silhouette frame(s).";
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
      const count = render();
      status.textContent = silhouettePending ? "Point updated. Apply or cancel the pending silhouette edit before downloading." :
        count + " changed point(s) or silhouette frame(s). Download the current input and create a new review before fitting. Original preparation files stay unchanged.";
    }
    buttons().forEach(bindButton);
    original.fit.frames.forEach((frame, i) => {
      const id = frame.source.metadata.frame;
      byId("newFrame").add(new Option(id.stream + " @ " + id.timestamp_us + " µs", String(i)));
      byId("silhouetteFrame").add(new Option(id.stream + " @ " + id.timestamp_us + " µs", String(i)));
    });
    function stopPlacement() {
      clearSilhouetteDrawing();
      draft = null; selected = null; pending = true; withdraw(); selectionDisplay();
      ["editX", "editY", "applyPoint"].forEach(id => byId(id).disabled = true);
      byId("editX").value = ""; byId("editY").value = "";
    }
    function clearSilhouetteDrawing() {
      silhouetteDrawing = null;
      sections.forEach(section => section.querySelectorAll("g[data-silhouette-draft]").forEach(node => node.remove()));
      selectionDisplay();
    }
    function sliceOverlay(i, line, isDraft = false) {
      const svg = sections[i].querySelector("svg"), group = document.createElementNS(svg.namespaceURI, "g");
      group.dataset[isDraft ? "silhouetteDraft" : "silhouette"] = isDraft ? "true" : line.partition;
      group.setAttribute("pointer-events", "none");
      const horizontal = line.axis === "horizontal", size = edited.fit.frames[i].source.image_size_px;
      const color = isDraft ? "#067568" : line.partition === "fit" ? "#b45309" : "#0369a1";
      for (const [j, [lo, hi]] of [[0, size[horizontal ? 0 : 1] - 1], ...line.intervals_px].entries()) {
        const segment = document.createElementNS(svg.namespaceURI, "line");
        const coordinates = horizontal ? [lo, line.at_px, hi, line.at_px] : [line.at_px, lo, line.at_px, hi];
        ["x1", "y1", "x2", "y2"].forEach((key, k) => segment.setAttribute(key, coordinates[k]));
        segment.setAttribute("stroke", color); segment.setAttribute("stroke-width", j === 0 ? 1 : 6);
        segment.setAttribute("stroke-opacity", j === 0 ? 1 : .6);
        if (j === 0) segment.setAttribute("stroke-dasharray", "5 5");
        group.append(segment);
      }
      svg.append(group);
    }
    function renderSilhouette(i) {
      const section = sections[i], current = edited.fit.frames[i].silhouette;
      section.querySelectorAll("g[data-silhouette]").forEach(node => node.remove());
      const review = section.querySelector(".silhouette-review");
      review.replaceChildren();
      const description = document.createElement("p");
      description.textContent = current ? "Authored silhouette: " + current.evidence : "No authored silhouette samples.";
      review.append(description);
      if (!current) return;
      const list = document.createElement("ul");
      current.lines.forEach(line => {
        sliceOverlay(i, line);
        const item = document.createElement("li");
        item.textContent = (line.partition === "fit" ? "FIT" : "CHECK") + " · " + line.axis + " @ " +
          line.at_px + " px · sigma " + line.sigma_px + " px · foreground " +
          (line.intervals_px.length ? line.intervals_px.map(pair => pair.join(" to ")).join("; ") : "empty (background)");
        list.append(item);
      });
      review.append(list);
    }
    function silhouetteFrame() {
      const raw = byId("silhouetteFrame").value, i = Number(raw);
      if (!raw || !Number.isInteger(i) || !silhouetteEditable[i]) {
        throw Error("Choose an editable frame with a checked rectified camera. Imported mask constraints are read-only.");
      }
      return i;
    }
    function loadSilhouetteForm() {
      const raw = byId("silhouetteFrame").value, i = Number(raw), frame = raw ? edited.fit.frames[i] : null;
      const chooser = byId("silhouetteLine"), old = chooser.value, lines = frame?.silhouette?.lines || [];
      chooser.replaceChildren(new Option("New slice", ""));
      lines.forEach((line, j) => chooser.add(new Option(
        (line.partition === "fit" ? "FIT" : "CHECK") + " · " + line.axis + " @ " + line.at_px, String(j))));
      chooser.value = [...chooser.options].some(option => option.value === old) ? old : "";
      const line = chooser.value === "" ? null : lines[Number(chooser.value)];
      const allowed = !!frame && silhouetteEditable[i];
      byId("silhouetteInputs").disabled = !allowed;
      byId("silhouetteAvailability").textContent = frame ? sections[i].dataset.silhouetteReason : "Choose a frame to author or review sampled constraints.";
      for (const [id, value] of [["silhouetteAxis", line?.axis], ["silhouetteAt", line?.at_px],
        ["silhouettePartition", line?.partition], ["silhouetteSigma", line?.sigma_px]]) {
        byId(id).value = value ?? ""; byId(id).disabled = line !== null;
      }
      byId("silhouetteEvidence").value = frame?.silhouette?.evidence || "";
      byId("silhouetteIntervals").value = line?.intervals_px.map(pair => pair.join(", ")).join("\n") || "";
      byId("silhouetteBackground").checked = line !== null && line.intervals_px.length === 0;
      byId("silhouetteIntervals").disabled = byId("silhouetteBackground").checked;
      byId("removeSilhouette").disabled = !allowed || line === null;
    }
    function readNumber(id) {
      const raw = byId(id).value.trim();
      return raw ? Number(raw) : NaN;
    }
    function intervalsFromForm(allowEmpty = false) {
      if (byId("silhouetteBackground").checked) return [];
      const raw = byId("silhouetteIntervals").value.trim();
      if (!raw) {
        if (allowEmpty) return [];
        throw Error("Enter foreground intervals or explicitly select an empty/background slice.");
      }
      return raw.split(/\r?\n/).map(row => {
        const values = row.trim().split(/[\s,]+/);
        if (values.length !== 2 || values.some(value => !value)) throw Error("Each interval needs one start, end pair per line.");
        return values.map(Number);
      });
    }
    function validateIntervals(intervals, size) {
      if (intervals.length > 8 || intervals.some(([lo, hi], j) => !Number.isFinite(lo) || !Number.isFinite(hi) ||
          lo < 0 || hi > size - 1 || lo >= hi || j > 0 && intervals[j - 1][1] >= lo)) {
        throw Error("Use at most eight increasing, disjoint intervals inside the image; touching or overlapping intervals refuse.");
      }
    }
    function silhouetteDeclaration() {
      const i = silhouetteFrame(), frame = edited.fit.frames[i], size = frame.source.image_size_px;
      const raw = byId("silhouetteLine").value, index = raw === "" ? -1 : Number(raw);
      const old = index === -1 ? null : frame.silhouette?.lines[index];
      if (index !== -1 && (!Number.isInteger(index) || !old)) throw Error("Choose an existing slice or New slice.");
      const line = {axis: byId("silhouetteAxis").value, at_px: readNumber("silhouetteAt"),
        intervals_px: [], sigma_px: readNumber("silhouetteSigma"), partition: byId("silhouettePartition").value};
      const evidence = byId("silhouetteEvidence").value;
      if (!["horizontal", "vertical"].includes(line.axis) || !["fit", "held_out"].includes(line.partition) ||
          !Number.isFinite(line.at_px) || line.at_px < 0 || line.at_px > size[line.axis === "horizontal" ? 1 : 0] - 1 ||
          !Number.isFinite(line.sigma_px) || line.sigma_px < 1e-6 || line.sigma_px > 1e6 ||
          !evidence.trim() || new TextEncoder().encode(evidence).length > 4096) {
        throw Error("Declare axis, in-image slice coordinate, FIT/CHECK, sigma in 1e-6..1e6 px and clothing/occlusion evidence (1..4096 bytes).");
      }
      if (old && ["axis", "at_px", "sigma_px", "partition"].some(key => old[key] !== line[key])) {
        throw Error("Existing slice location, role and sigma stay fixed; only its foreground intervals and evidence may be corrected.");
      }
      const lines = frame.silhouette?.lines || [];
      if (index === -1 && lines.length >= 64 || lines.some((other, j) => j !== index && other.axis === line.axis && other.at_px === line.at_px)) {
        throw Error("A frame supports 64 unique slices; the same slice cannot supply both FIT and CHECK evidence.");
      }
      return {i, index, line, evidence};
    }
    function validateSilhouette(value, size) {
      if (!value.lines.length || !value.lines.some(line => line.intervals_px.length)) {
        throw Error("A silhouette needs foreground on at least one slice. Add a foreground slice before empty/background slices.");
      }
      value.lines.forEach(line => validateIntervals(line.intervals_px, size[line.axis === "horizontal" ? 0 : 1]));
    }
    function silhouetteFailure(error) {
      silhouettePending = true; withdraw();
      status.textContent = error.message + " No silhouette change or download published.";
    }
    function chooseSilhouette() {
      stopPlacement(); pending = false; silhouettePending = true; clearSilhouetteDrawing(); loadSilhouetteForm();
      status.textContent = "Declare a new slice, or correct an existing slice's intervals. Apply or cancel the pending silhouette edit before downloading.";
    }
    byId("silhouetteFrame").addEventListener("change", () => { byId("silhouetteLine").value = ""; chooseSilhouette(); });
    byId("silhouetteLine").addEventListener("change", chooseSilhouette);
    ["silhouetteAxis", "silhouetteAt", "silhouettePartition", "silhouetteSigma", "silhouetteEvidence", "silhouetteIntervals", "silhouetteBackground"].forEach(id => {
      byId(id).addEventListener(["silhouetteAxis", "silhouettePartition", "silhouetteBackground"].includes(id) ? "change" : "input", () => {
        stopPlacement(); pending = false; silhouettePending = true; clearSilhouetteDrawing();
        byId("silhouetteIntervals").disabled = byId("silhouetteBackground").checked;
        status.textContent = "Silhouette edit pending. Apply valid intervals or cancel; no download is available.";
      });
    });
    byId("prepareSilhouette").addEventListener("click", () => {
      stopPlacement(); pending = false; silhouettePending = true; clearSilhouetteDrawing();
      try {
        const {i, line} = silhouetteDeclaration();
        if (byId("silhouetteBackground").checked) throw Error("Uncheck empty/background before picking foreground endpoints.");
        line.intervals_px = intervalsFromForm(true);
        validateIntervals(line.intervals_px, edited.fit.frames[i].source.image_size_px[line.axis === "horizontal" ? 0 : 1]);
        silhouetteDrawing = {frame: i, line, start: null}; sliceOverlay(i, line, true); selectionDisplay();
        status.textContent = "Click interval start then end in increasing pixel order on this image. Clicks are projected onto the declared slice; Apply saves completed intervals.";
      } catch (error) { silhouetteFailure(error); }
    });
    function pickSilhouette(i, pixel) {
      try {
        const drawing = silhouetteDrawing, size = edited.fit.frames[i].source.image_size_px;
        if (drawing.frame !== i) throw Error("Pick endpoints on the selected silhouette frame.");
        if (![pixel.x, pixel.y].every(Number.isFinite) || pixel.x < 0 || pixel.y < 0 || pixel.x > size[0] - 1 || pixel.y > size[1] - 1) {
          throw Error("Silhouette endpoints must lie inside the original image; no clamping is performed.");
        }
        const coordinate = drawing.line.axis === "horizontal" ? pixel.x : pixel.y;
        if (drawing.start === null) {
          drawing.start = coordinate; status.textContent = "Start recorded. Click the interval end at a larger coordinate; no download is available.";
        } else {
          const intervals = [...drawing.line.intervals_px, [drawing.start, coordinate]];
          validateIntervals(intervals, size[drawing.line.axis === "horizontal" ? 0 : 1]);
          drawing.line.intervals_px = intervals; drawing.start = null;
          byId("silhouetteIntervals").value = intervals.map(pair => pair.join(", ")).join("\n");
          sections[i].querySelectorAll("g[data-silhouette-draft]").forEach(node => node.remove());
          sliceOverlay(i, drawing.line, true);
          status.textContent = intervals.length + " interval(s) ready. Pick another pair or Apply slice; no annotation is saved yet.";
        }
      } catch (error) { silhouetteFailure(error); }
    }
    byId("applySilhouette").addEventListener("click", () => {
      withdraw();
      try {
        if (pending || draft !== null) throw Error("Apply or cancel the pending point edit or placement before changing silhouette samples.");
        const {i, index, line, evidence} = silhouetteDeclaration();
        if (silhouetteDrawing !== null && silhouetteDrawing.start !== null) throw Error("Finish the pending endpoint pair or cancel before applying.");
        line.intervals_px = intervalsFromForm();
        const frame = edited.fit.frames[i], value = {evidence, lines: structuredClone(frame.silhouette?.lines || [])};
        if (index === -1) value.lines.push(line); else value.lines[index] = line;
        validateSilhouette(value, frame.source.image_size_px);
        frame.silhouette = value; clearSilhouetteDrawing(); silhouettePending = false;
        byId("silhouetteLine").value = ""; loadSilhouetteForm();
        status.textContent = render() + " changed point(s) or silhouette frame(s). Download the current input and create a new native review before fitting.";
      } catch (error) { silhouetteFailure(error); }
    });
    byId("removeSilhouette").addEventListener("click", () => {
      withdraw();
      try {
        if (pending || draft !== null) throw Error("Apply or cancel the pending point edit or placement before changing silhouette samples.");
        const i = silhouetteFrame(), raw = byId("silhouetteLine").value, frame = edited.fit.frames[i];
        const index = Number(raw);
        if (raw === "" || !Number.isInteger(index) || !frame.silhouette?.lines[index]) throw Error("Choose an existing slice to remove.");
        const value = structuredClone(frame.silhouette); value.lines.splice(index, 1);
        if (value.lines.length) { validateSilhouette(value, frame.source.image_size_px); frame.silhouette = value; }
        else delete frame.silhouette;
        clearSilhouetteDrawing(); silhouettePending = false;
        byId("silhouetteLine").value = ""; loadSilhouetteForm();
        status.textContent = "Slice removed. " + render() + " changed point(s) or silhouette frame(s). Native review and fit gates still apply.";
      } catch (error) { silhouetteFailure(error); }
    });
    byId("cancelSilhouette").addEventListener("click", () => {
      clearSilhouetteDrawing(); silhouettePending = false; withdraw(); loadSilhouetteForm();
      status.textContent = "Pending silhouette edit cancelled; committed annotations remain in memory. No download is published. Select a point or apply a slice to continue.";
    });
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
      stopPlacement(); pending = false;
      status.textContent = "Placement cancelled. No new point or download published. Select a point or apply a slice to continue.";
    });
    byId("removePoint").addEventListener("click", () => {
      withdraw();
      if (selected === null || draft !== null) return;
      if (pending || silhouettePending) {
        status.textContent = "Apply or cancel the pending point or silhouette edit before removing an observation. No change or download published.";
        return;
      }
      const [i, j] = selected, [point] = edited.fit.frames[i].keypoints.splice(j, 1);
      // Row buttons and marker indices must follow the remaining observations, including
      // their names, roles, uncertainties and targets. No other observation is reassigned.
      clearPointDisplay(i); stopPlacement(); pending = false; targetOptions();
      const count = render();
      status.textContent = "Removed " + point.name + ". " + count +
        " changed point(s) or silhouette frame(s). Create a new native review before fitting; missing evidence can refuse. Do not remove CHECKs to tune a fit result.";
    });
    sections.forEach((section, i) => {
      const svg = section.querySelector("svg");
      svg.addEventListener("click", event => {
        if (silhouetteDrawing === null && (selected === null || selected[0] !== i)) {
          status.textContent = "Select a point belonging to this image before correcting it.";
          return;
        }
        const matrix = svg.getScreenCTM();
        if (matrix === null) { withdraw(); status.textContent = "Image coordinates unavailable; no download published."; return; }
        const pixel = new DOMPoint(event.clientX, event.clientY).matrixTransform(matrix.inverse());
        if (silhouetteDrawing !== null) pickSilhouette(i, pixel); else apply(pixel.x, pixel.y);
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
      edited = structuredClone(original); selected = null; pending = false; draft = null;
      sections.forEach((_, i) => clearPointDisplay(i));
      silhouettePending = false; clearSilhouetteDrawing(); render();
      byId("newFrame").value = ""; byId("newTarget").value = "";
      byId("newPartition").value = ""; byId("newSigma").value = ""; targetOptions();
      byId("silhouetteFrame").value = ""; byId("silhouetteLine").value = ""; loadSilhouetteForm();
      ["editX", "editY", "applyPoint"].forEach(id => byId(id).disabled = true);
      byId("editX").value = ""; byId("editY").value = "";
      status.textContent = "All point and silhouette corrections removed. Original observations restored; no corrected input is available.";
    });
    window.addEventListener("pagehide", withdraw);
    render(); loadSilhouetteForm(); byId("newPointFields").disabled = false;
    byId("silhouetteFields").disabled = false; byId("captureEditor").hidden = false;
    status.textContent = "Select a point, declare a new observation, or author sampled silhouettes on a checked rectified frame.";
  } catch (error) {
    withdraw();
    document.querySelectorAll(".edit-point").forEach(b => b.disabled = true);
    byId("captureEditor").hidden = false;
    status.textContent = "Editor unavailable: " + error.message + ". Use the unchanged original input and review.";
  }
})();
