// Source-bound polygons only. The native materializer owns geometry validation and rasterization.
(() => {
  const el = id => document.getElementById(id), link = el("mask-download"), status = el("status");
  let url = null;
  function withdraw() {
    if (url !== null) URL.revokeObjectURL(url);
    url = null; link.hidden = true; link.removeAttribute("href");
  }
  try {
    const original = JSON.parse(el("annotations").textContent), images = JSON.parse(el("frame-images").textContent);
    const targets = ["person_regions", "sheet_regions"], binding = ["index", "image_sha256", "decoded_gray_sha256", "image_size_px"];
    function keys(value, expected) {
      if (value === null || typeof value !== "object" || Array.isArray(value) ||
          Object.keys(value).length !== expected.length || !Object.keys(value).every(key => expected.includes(key))) throw Error("Unexpected draft fields");
    }
    function ring(points, size) {
      if (!Array.isArray(points) || points.length < 3 || points.length > 256) throw Error("Each ring needs 3–256 vertices");
      const seen = new Set();
      for (const p of points) {
        if (!Array.isArray(p) || p.length !== 2 || !p.every((n, axis) =>
          Number.isSafeInteger(n) && n >= 0 && n < size[axis])) throw Error("Vertices must be integer pixels inside the image");
        const id = p.join(",");
        if (seen.has(id)) throw Error("A ring cannot repeat a vertex; closure is implicit");
        seen.add(id);
      }
      if (points.reduce((sum, p, i) => { const q = points[(i + 1) % points.length]; return sum + p[0] * q[1] - q[0] * p[1]; }, 0) === 0)
        throw Error("A ring must enclose nonzero area");
    }
    function validate(value) {
      keys(value, ["schema", "source_manifest", "source_manifest_sha256", "frames"]);
      for (const key of ["schema", "source_manifest", "source_manifest_sha256"])
        if (value[key] !== original[key]) throw Error("Draft belongs to a different source manifest");
      if (value.schema !== "ftx-video-mask-polygons/1" || !Array.isArray(value.frames) ||
          value.frames.length !== original.frames.length || !value.frames.length) throw Error("Draft frame set disagrees with this review");
      value.frames.forEach((frame, i) => {
        keys(frame, [...binding, ...targets]);
        for (const key of binding) if (JSON.stringify(frame[key]) !== JSON.stringify(original.frames[i][key]))
          throw Error("Draft source binding changed at frame " + (i + 1));
        const size = frame.image_size_px;
        if (frame.index !== i || !Array.isArray(size) || size.length !== 2 ||
            !size.every(n => Number.isSafeInteger(n) && n >= 3) || size[0] * size[1] > 40000000)
          throw Error("Invalid frame identity or image dimensions");
        let count = 0;
        for (const target of targets) {
          const regions = frame[target];
          if (regions === null) continue;
          if (!Array.isArray(regions) || regions.length > 16) throw Error("At most 16 components per target");
          regions.forEach(region => {
            keys(region, ["outer", "holes"]);
            if (!Array.isArray(region.holes) || region.holes.length > 16) throw Error("At most 16 holes per component");
            for (const points of [region.outer, ...region.holes]) { ring(points, size); count += points.length; }
          });
        }
        if (count > 4096) throw Error("At most 4096 vertices across both targets in a frame");
      });
    }
    validate(original);
    if (!Array.isArray(images) || images.length !== original.frames.length || images.some((image, i) =>
      !["source-" + String(i).padStart(4, "0") + ".png", "source-" + String(i).padStart(4, "0") + ".jpg"].includes(image.file) ||
      typeof image.label !== "string")) throw Error("Review image identities disagree");
    let edited = structuredClone(original), at = 0, target = targets[0], selected = -1, draft = null;
    let shown = -1, imageReady = false, blocked = false, busy = false, closed = false, revision = 0;
    const svg = el("canvas"), source = el("frame-image");
    const regions = () => edited.frames[at][target] || [];
    const vertices = () => targets.reduce((n, key) => n + (edited.frames[at][key] || []).reduce((m, r) =>
      m + r.outer.length + r.holes.reduce((v, h) => v + h.length, 0), 0), 0);
    function touch() { ++revision; busy = false; blocked = false; withdraw(); }
    function path(points, close = true) { return points.length ? "M" + points.map(p => p.join(",")).join("L") + (close ? "Z" : "") : ""; }
    function shape(parent, name, attributes) {
      const node = document.createElementNS("http://www.w3.org/2000/svg", name);
      Object.entries(attributes).forEach(([k, v]) => node.setAttribute(k, String(v))); parent.append(node); return node;
    }
    function render() {
      withdraw();
      const frame = edited.frames[at], [w, h] = frame.image_size_px, list = regions();
      if (shown !== at) {
        shown = at; imageReady = false; svg.setAttribute("viewBox", "0 0 " + w + " " + h);
        source.setAttribute("width", w); source.setAttribute("height", h); source.setAttribute("href", images[at].file);
      }
      el("frame").value = String(at); el("target").value = target; el("label").textContent = images[at].label;
      if (selected >= list.length) selected = list.length - 1;
      el("region").replaceChildren(new Option("Choose component", "-1"));
      list.forEach((r, i) => el("region").add(new Option("Component " + (i + 1) + " (" + r.holes.length + " holes)", String(i))));
      el("region").value = String(selected);
      el("regions").replaceChildren(); el("draft").replaceChildren();
      const definitions = shape(el("regions"), "defs", {});
      targets.forEach((key, k) => (frame[key] || []).forEach((region, i) => {
        const id = "region-mask-" + k + "-" + i;
        const mask = shape(definitions, "mask", {id, x: 0, y: 0, width: w, height: h,
          maskUnits: "userSpaceOnUse", maskContentUnits: "userSpaceOnUse", style: "mask-type:luminance"});
        shape(mask, "path", {d: path(region.outer), fill: "white", "fill-rule": "evenodd"});
        // Independent black holes subtract their union from this component only. Global even-odd
        // filling would incorrectly restore overlapping holes and add holes outside the outer ring.
        region.holes.forEach(hole => shape(mask, "path", {d: path(hole), fill: "black", "fill-rule": "evenodd"}));
        shape(el("regions"), "rect", {x: 0, y: 0, width: w, height: h, mask: "url(#" + id + ")",
          fill: ["#d97706", "#0369a1"][k], "fill-opacity": .22});
        [region.outer, ...region.holes].forEach(points => shape(el("regions"), "path", {
          d: path(points), fill: "none", stroke: ["#b45309", "#075985"][k],
          "stroke-width": key === target && i === selected ? 3 : 1, "vector-effect": "non-scaling-stroke"
        }));
      }));
      if (draft !== null) {
        shape(el("draft"), "path", {d: path(draft.points, false), fill: "none", stroke: "#be123c", "stroke-width": 2, "vector-effect": "non-scaling-stroke"});
        draft.points.forEach(p => shape(el("draft"), "circle", {cx: p[0], cy: p[1], r: Math.max(w, h) / 250, fill: "#be123c"}));
      }
      svg.dataset.drawing = String(draft !== null);
      const active = draft !== null, locked = active || busy, current = list[selected];
      el("import").disabled = active;
      for (const id of ["frame", "target", "region"]) el(id).disabled = locked;
      el("previous").disabled = locked || at === 0; el("next").disabled = locked || at === edited.frames.length - 1;
      el("new-region").disabled = locked || !imageReady || list.length >= 16;
      el("new-hole").disabled = locked || !imageReady || !current || current.holes.length >= 16;
      el("remove-region").disabled = locked || !current; el("remove-hole").disabled = locked || !current || !current.holes.length;
      el("not-visible").disabled = locked || !imageReady; el("undo").disabled = !active || !draft.points.length;
      el("finish").disabled = !active || draft.points.length < 3; el("cancel").disabled = !active;
      const reviewed = edited.frames.reduce((n, f) => n + targets.filter(key => f[key] !== null).length, 0), total = edited.frames.length * 2;
      el("progress").textContent = reviewed + " / " + total + " targets reviewed. Current target: " +
        (frame[target] === null ? "unreviewed" : list.length ? list.length + " component(s)" : "explicitly not visible");
      if (!active && !busy && !blocked && !closed && imageReady) {
        url = URL.createObjectURL(new Blob([JSON.stringify(edited, null, 2) + "\n"], {type: "application/json"}));
        link.href = url; link.hidden = false;
        link.textContent = reviewed === total ? "Download polygons for native validation" : "Download incomplete draft";
      }
    }
    images.forEach((image, i) => el("frame").add(new Option((i + 1) + ": " + image.label, String(i))));
    function navigate(next) { touch(); at = next; selected = -1; render(); status.textContent = "Choose a target and draw its components, or mark it not visible."; }
    el("previous").addEventListener("click", () => navigate(at - 1));
    el("next").addEventListener("click", () => navigate(at + 1));
    el("frame").addEventListener("change", () => navigate(Number(el("frame").value)));
    el("target").addEventListener("change", () => { touch(); target = el("target").value; selected = -1; render(); });
    el("region").addEventListener("change", () => { touch(); selected = Number(el("region").value); render(); });
    function begin(kind) {
      touch(); draft = {kind, points: []}; render();
      status.textContent = "Click at least three boundary vertices for this " + kind + ". Finish or cancel before downloading.";
    }
    el("new-region").addEventListener("click", () => begin("component"));
    el("new-hole").addEventListener("click", () => begin("hole"));
    svg.addEventListener("click", event => {
      if (draft === null || !imageReady) return;
      withdraw();
      try {
        const matrix = svg.getScreenCTM(); if (matrix === null) throw Error("Image coordinates unavailable");
        const p = new DOMPoint(event.clientX, event.clientY).matrixTransform(matrix.inverse()), point = [Math.round(p.x), Math.round(p.y)];
        if (!point.every((n, axis) => Number.isSafeInteger(n) && n >= 0 && n < edited.frames[at].image_size_px[axis]))
          throw Error("Click inside the stored image pixels");
        if (draft.points.length >= 256 || vertices() + draft.points.length >= 4096) throw Error("Polygon or frame vertex budget reached");
        if (draft.points.some(q => q[0] === point[0] && q[1] === point[1])) throw Error("Vertex already present; use Finish to close the ring");
        draft.points.push(point); render(); status.textContent = draft.points.length + " vertices. Finish the polygon or keep adding vertices.";
      } catch (error) { status.textContent = error.message + ". No draft download published."; }
    });
    el("undo").addEventListener("click", () => { draft.points.pop(); render(); });
    el("finish").addEventListener("click", () => {
      try {
        ring(draft.points, edited.frames[at].image_size_px); touch();
        if (draft.kind === "component") {
          if (edited.frames[at][target] === null) edited.frames[at][target] = [];
          edited.frames[at][target].push({outer: draft.points, holes: []}); selected = regions().length - 1;
        } else regions()[selected].holes.push(draft.points);
        draft = null; render(); status.textContent = "Polygon saved in this draft. Native validation still checks ring topology and pixel bounds.";
      } catch (error) { withdraw(); status.textContent = error.message + ". Finish or cancel this polygon."; }
    });
    el("cancel").addEventListener("click", () => { touch(); draft = null; render(); status.textContent = "Unfinished polygon discarded; previously completed regions remain."; });
    el("remove-region").addEventListener("click", () => { touch(); regions().splice(selected, 1); if (!regions().length) edited.frames[at][target] = null; render(); });
    el("remove-hole").addEventListener("click", () => { touch(); regions()[selected].holes.pop(); render(); });
    el("not-visible").addEventListener("click", () => { touch(); edited.frames[at][target] = []; selected = -1; render(); status.textContent = "Explicit empty mask recorded. The selector will retain this frame and apply its ordinary sample-count gate."; });
    el("reset").addEventListener("click", () => {
      touch(); edited = structuredClone(original); draft = null; selected = -1; el("import").value = ""; render(); status.textContent = "Review reset to the original annotations.";
    });
    el("import").addEventListener("change", async () => {
      touch(); const ticket = revision, file = el("import").files[0]; busy = true; draft = null; render();
      try {
        if (!file || file.size > 16 * 1024 * 1024) throw Error("Choose a JSON draft of at most 16 MiB");
        const value = JSON.parse(await file.text()); if (ticket !== revision) return;
        validate(value); edited = structuredClone(value); selected = -1; busy = false; render();
        status.textContent = "Matching draft imported. Review its regions before native materialization.";
      } catch (error) {
        if (ticket !== revision) return;
        busy = false; blocked = true; render(); status.textContent = "Draft refused: " + error.message + ". Previous completed regions remain; no download published.";
      }
    });
    source.addEventListener("load", () => { imageReady = true; render(); });
    source.addEventListener("error", () => { imageReady = false; blocked = true; render(); status.textContent = "Original frame image could not load. No download published."; });
    window.addEventListener("pagehide", () => { ++revision; closed = true; withdraw(); });
    el("controls").disabled = false; render(); status.textContent = "Choose a target and start a component. Nothing is inferred or propagated between frames.";
  } catch (error) {
    withdraw(); el("controls").disabled = true; status.textContent = "Editor unavailable: " + error.message;
  }
})();
