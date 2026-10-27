// Exact native topology and caller-authored correspondences; no observations enter this page.
(() => {
  const byId = id => document.getElementById(id);
  const link = byId("targetDownload"), status = byId("targetStatus");
  let download = null;
  function withdraw() {
    if (download !== null) URL.revokeObjectURL(download);
    download = null; link.hidden = true; link.removeAttribute("href");
  }
  try {
    const data = JSON.parse(byId("targetData").textContent);
    const original = data.annotations, observed = new Set(data.observed_names);
    const vertices = data.mesh.vertices_m, faces = data.mesh.triangles;
    const bones = new Map(data.bones.map(b => [b.name, b]));
    if (!vertices.length || vertices.length > 100000 || !faces.length || faces.length > 200000 ||
        vertices.some(v => v.length !== 3 || v.some(x => !Number.isFinite(x))) ||
        faces.some(f => f.length !== 3 || f.some(i => !Number.isSafeInteger(i) || i < 0 || i >= vertices.length))) {
      throw Error("Invalid native preview geometry");
    }
    let edited = structuredClone(original), selected = "", candidate = null, pending = false;
    const canvas = byId("targetCanvas"), context = canvas.getContext("2d");
    if (!context) throw Error("This browser cannot display the local model canvas");
    const width = canvas.width, height = canvas.height;
    const low = [Infinity, Infinity, Infinity], high = [-Infinity, -Infinity, -Infinity];
    vertices.forEach(v => v.forEach((x, i) => { low[i] = Math.min(low[i], x); high[i] = Math.max(high[i], x); }));
    const center = low.map((x, i) => x + 0.5 * (high[i] - x));
    const radius = vertices.reduce((largest, v) => Math.max(largest, Math.hypot(...v.map((x, i) => x - center[i]))), 0);
    if (!(radius > 0) || !Number.isFinite(radius)) throw Error("Native preview has no finite spatial extent");
    let projected = [], depthBuffer = null, renderQueued = false, rendered = false;
    const utf8 = new TextEncoder();
    const own = (object, name) => Object.hasOwn(object, name);
    function putTarget(name, value) {
      Object.defineProperty(edited.keypoints, name, {value: structuredClone(value), writable: true, enumerable: true, configurable: true});
    }
    function validName(name) {
      return name.trim().length > 0 && utf8.encode(name).length <= 256;
    }
    function checkTargets(targets, requireObserved = true) {
      const names = Object.keys(targets), locations = new Set();
      if (!names.length || names.length > 128) throw Error("Declare between one and 128 targets");
      for (const name of names) {
        if (!validName(name)) throw Error("Target names need one to 256 UTF-8 bytes");
        const target = targets[name];
        let identity;
        if (target.kind === "vertex") {
          if (!Number.isSafeInteger(target.index) || target.index < 0 || target.index >= vertices.length) throw Error("Vertex is outside the native mesh");
          identity = [[target.index, 1]];
        } else if (target.kind === "surface") {
          if (!Number.isSafeInteger(target.face) || target.face < 0 || target.face >= faces.length ||
              !Array.isArray(target.barycentric) || target.barycentric.length !== 3 ||
              target.barycentric.some(w => !Number.isFinite(w) || w < 0 || w > 1) ||
              Math.abs(target.barycentric.reduce((s, w) => s + w, 0) - 1) > 1e-12 ||
              new Set(faces[target.face]).size !== 3) throw Error("Invalid native triangle or barycentric weights");
          identity = faces[target.face].map((v, i) => [v, target.barycentric[i]]).filter(p => p[1] > 0).sort((a, b) => a[0] - b[0]);
        } else if (target.kind === "bone_head" || target.kind === "bone_tail") {
          if (!bones.has(target.bone)) throw Error("Unknown native rig bone");
          identity = [target.kind, target.bone];
        } else throw Error("Unknown correspondence type");
        const key = JSON.stringify(identity);
        if (locations.has(key)) throw Error("Two names refer to the same model location");
        locations.add(key);
      }
      if (requireObserved) for (const name of observed) {
        if (!own(targets, name)) throw Error("Define the required observed target: " + name);
      }
    }
    function updateDownload() {
      withdraw();
      if (pending || JSON.stringify(edited.keypoints) === JSON.stringify(original.keypoints)) return;
      try {
        checkTargets(edited.keypoints);
      } catch (error) {
        status.textContent = error.message + ". No download published.";
        return;
      }
      const output = structuredClone(edited);
      output.keypoints = Object.fromEntries(Object.entries(output.keypoints).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0));
      download = URL.createObjectURL(new Blob([JSON.stringify(output, null, 2) + "\n"], {type: "application/json"}));
      link.href = download; link.hidden = false;
    }
    function location(target) {
      if (!target) return null;
      if (target.kind === "vertex") return vertices[target.index];
      if (target.kind === "surface") return [0, 1, 2].map(axis => faces[target.face].reduce((sum, index, k) => sum + vertices[index][axis] * target.barycentric[k], 0));
      const bone = bones.get(target.bone);
      return bone ? bone[target.kind === "bone_head" ? "head_m" : "tail_m"] : null;
    }
    function project(point) {
      const yaw = Number(byId("yaw").value) * Math.PI / 180, pitch = Number(byId("pitch").value) * Math.PI / 180;
      const scale = Math.min(width, height) * 0.45 * Number(byId("zoom").value) / radius;
      if (!Number.isFinite(scale) || !(scale > 0)) throw Error("Preview scale is outside the browser's finite range");
      const [x, y, z] = point.map((v, i) => v - center[i]);
      const horizontal = Math.cos(yaw) * x + Math.sin(yaw) * y;
      const away = -Math.sin(yaw) * x + Math.cos(yaw) * y;
      const vertical = Math.cos(pitch) * z - Math.sin(pitch) * away;
      return [width / 2 + scale * horizontal, height / 2 - scale * vertical,
        Math.sin(pitch) * z + Math.cos(pitch) * away];
    }
    function weights(x, y, a, b, c) {
      const determinant = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
      if (Math.abs(determinant) < 1e-12) return null;
      const wa = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / determinant;
      const wb = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / determinant;
      const wc = 1 - wa - wb;
      return wa >= 0 && wb >= 0 && wc >= 0 ? [wa, wb, wc] : null;
    }
    function marker() {
      const point = location(candidate);
      if (!point || !rendered) return;
      const p = project(point), x = Math.floor(p[0]), y = Math.floor(p[1]);
      const onCanvas = x >= 0 && y >= 0 && x < width && y < height;
      const hidden = onCanvas && p[2] > depthBuffer[y * width + x] + radius * 0.005;
      context.strokeStyle = hidden ? "#a21caf" : "#b45309";
      context.lineWidth = 3; context.setLineDash(hidden ? [3, 3] : []);
      context.beginPath(); context.arc(p[0], p[1], 8, 0, 2 * Math.PI); context.stroke(); context.setLineDash([]);
      context.fillStyle = "#17212b"; context.font = "14px system-ui";
      context.fillText(hidden ? "Selected target behind surface" : "Selected target", p[0] + 12, p[1] - 8);
    }
    function render() {
      renderQueued = false; rendered = false;
      projected = vertices.map(project);
      const image = context.createImageData(width, height), pixels = image.data;
      depthBuffer = new Float64Array(width * height); depthBuffer.fill(Infinity);
      for (let i = 0; i < pixels.length; i += 4) { pixels[i] = 245; pixels[i + 1] = 247; pixels[i + 2] = 250; pixels[i + 3] = 255; }
      let work = 0;
      for (const face of faces) {
        const [a, b, c] = face.map(i => projected[i]);
        const xmin = Math.max(0, Math.floor(Math.min(a[0], b[0], c[0]))), xmax = Math.min(width - 1, Math.ceil(Math.max(a[0], b[0], c[0])));
        const ymin = Math.max(0, Math.floor(Math.min(a[1], b[1], c[1]))), ymax = Math.min(height - 1, Math.ceil(Math.max(a[1], b[1], c[1])));
        const determinant = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
        if (Math.abs(determinant) < 1e-12 || xmin > xmax || ymin > ymax) continue;
        work += (xmax - xmin + 1) * (ymax - ymin + 1);
        if (work > 100000000) throw Error("Preview drawing exceeds its work limit; reduce zoom or choose another view");
        const [v0, v1, v2] = face.map(i => vertices[i]);
        const u = v1.map((v, i) => v - v0[i]), v = v2.map((n, i) => n - v0[i]);
        const normal = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        const norm = Math.hypot(...normal), shade = norm ? 0.45 + 0.55 * Math.abs((normal[0] * 0.4 - normal[1] * 0.7 + normal[2] * 0.6) / (norm * Math.sqrt(1.01))) : 0.5;
        const color = [Math.round(100 + 70 * shade), Math.round(125 + 75 * shade), Math.round(145 + 80 * shade)];
        for (let y = ymin; y <= ymax; ++y) {
          for (let x = xmin; x <= xmax; ++x) {
            const w = weights(x + 0.5, y + 0.5, a, b, c);
            if (!w) continue;
            const depth = w[0] * a[2] + w[1] * b[2] + w[2] * c[2], index = y * width + x;
            if (depth >= depthBuffer[index]) continue;
            depthBuffer[index] = depth;
            pixels[index * 4] = color[0]; pixels[index * 4 + 1] = color[1]; pixels[index * 4 + 2] = color[2];
          }
        }
      }
      context.putImageData(image, 0, 0); rendered = true; marker();
      canvas.dataset.rendered = "true";
    }
    function queueRender() {
      rendered = false;
      canvas.dataset.rendered = "false";
      if (renderQueued) return;
      renderQueued = true;
      requestAnimationFrame(() => {
        try { render(); }
        catch (error) { rendered = false; context.clearRect(0, 0, width, height); status.textContent = error.message; }
      });
    }
    function definitions() {
      const target = candidate;
      byId("targetDefinition").textContent = target ? JSON.stringify(target, null, 2) + "\nPreview location (m): " + JSON.stringify(location(target)) : "Choose a location.";
      byId("targetBone").disabled = !byId("targetKind").value.startsWith("bone_");
      queueRender();
    }
    function fillOptions() {
      const list = byId("targetList");
      list.replaceChildren(new Option("New target", ""));
      [...new Set([...Object.keys(edited.keypoints), ...observed])].sort().forEach(name => {
        list.add(new Option(name + (observed.has(name) ? " (observed)" : "") + (!own(edited.keypoints, name) ? " — needs definition" : ""), name));
      });
      list.value = selected;
    }
    function select(name) {
      withdraw(); selected = name; pending = false;
      candidate = name && own(edited.keypoints, name) ? structuredClone(edited.keypoints[name]) : null;
      byId("targetName").value = name; byId("targetName").readOnly = name !== "";
      byId("targetKind").value = candidate?.kind || "surface";
      byId("targetBone").value = candidate?.bone || "";
      byId("removeTarget").disabled = !name || observed.has(name) || !own(edited.keypoints, name);
      definitions(); updateDownload();
    }
    function boneCandidate() {
      const kind = byId("targetKind").value, bone = byId("targetBone").value;
      candidate = kind.startsWith("bone_") && bones.has(bone) ? {kind, bone} : null;
      definitions();
    }
    function dirty() { pending = true; withdraw(); }
    byId("targetName").addEventListener("input", dirty);
    byId("targetList").addEventListener("change", () => select(byId("targetList").value));
    byId("targetKind").addEventListener("change", () => { dirty(); boneCandidate(); });
    byId("targetBone").addEventListener("change", () => { dirty(); boneCandidate(); });
    byId("applyTarget").addEventListener("click", () => {
      withdraw();
      try {
        const name = byId("targetName").value;
        if (!validName(name)) throw Error("Target names need one to 256 UTF-8 bytes");
        if (selected && name !== selected) throw Error("Select New target to create a new name");
        if (!selected && own(edited.keypoints, name)) throw Error("Select the existing name before changing its target");
        if (!candidate) throw Error("Choose a visible surface location or a named bone endpoint first");
        if (!own(edited.keypoints, name) && Object.keys(edited.keypoints).length >= 128) throw Error("At most 128 model targets are supported");
        const proposed = Object.fromEntries([...Object.entries(edited.keypoints).filter(([n]) => n !== name), [name, candidate]]);
        // Missing observed definitions are permitted during authoring, but not in a download.
        checkTargets(proposed, false);
        putTarget(name, candidate); selected = name; pending = false; fillOptions(); select(name);
        status.textContent = "Applied " + name + ". " + Object.keys(edited.keypoints).length + " target(s).";
        updateDownload();
      } catch (error) { pending = true; status.textContent = error.message + ". No target or download published."; }
    });
    byId("removeTarget").addEventListener("click", () => {
      withdraw();
      if (pending) { status.textContent = "Apply or cancel the pending target edit before removal."; return; }
      if (!selected || observed.has(selected)) { status.textContent = "An observed name must retain its target definition."; return; }
      delete edited.keypoints[selected]; selected = ""; fillOptions(); select("");
      status.textContent = "Removed the unobserved target."; updateDownload();
    });
    byId("cancelTarget").addEventListener("click", () => {
      select(selected); status.textContent = "Pending target edit cancelled; applied definitions are unchanged."; updateDownload();
    });
    byId("resetTargets").addEventListener("click", () => {
      withdraw(); edited = structuredClone(original); selected = ""; pending = false; fillOptions(); select("");
      status.textContent = "All target changes reset to the original declaration.";
    });
    canvas.addEventListener("click", event => {
      withdraw();
      const kind = byId("targetKind").value;
      if (kind !== "vertex" && kind !== "surface") { status.textContent = "Choose a named bone endpoint using the bone list."; return; }
      if (!rendered) { status.textContent = "Wait for the current model view to finish drawing."; return; }
      const rect = canvas.getBoundingClientRect();
      // The canvas keeps its native aspect ratio through object-fit: contain, even in a short viewport.
      const displayScale = Math.min(rect.width / width, rect.height / height);
      const x = (event.clientX - rect.left - (rect.width - width * displayScale) / 2) / displayScale;
      const y = (event.clientY - rect.top - (rect.height - height * displayScale) / 2) / displayScale;
      if (!(x >= 0 && y >= 0 && x < width && y < height)) { status.textContent = "Click inside the displayed model canvas."; return; }
      let hit = null, nearest = Infinity;
      faces.forEach((face, faceIndex) => {
        const triangle = face.map(i => projected[i]), w = weights(x, y, ...triangle);
        if (!w) return;
        const depth = w.reduce((sum, weight, i) => sum + weight * triangle[i][2], 0);
        if (depth < nearest) { nearest = depth; hit = {face: faceIndex, barycentric: w}; }
      });
      dirty();
      if (!hit) { candidate = null; definitions(); status.textContent = "No visible triangle at that location. No target applied."; return; }
      if (kind === "surface") candidate = {kind, ...hit};
      else {
        const index = [...faces[hit.face]].sort((a, b) => Math.hypot(projected[a][0] - x, projected[a][1] - y) - Math.hypot(projected[b][0] - x, projected[b][1] - y))[0];
        candidate = {kind, index};
      }
      definitions(); status.textContent = "Location selected on the nearest visible triangle. Apply the target to keep it.";
    });
    ["yaw", "pitch", "zoom"].forEach(id => byId(id).addEventListener("input", queueRender));
    const views = {front: [0, 0], back: [180, 0], left: [90, 0], right: [-90, 0], top: [0, -90]};
    document.querySelectorAll("[data-view]").forEach(button => button.addEventListener("click", () => {
      [byId("yaw").value, byId("pitch").value] = views[button.dataset.view]; queueRender();
    }));
    data.bones.forEach(bone => byId("targetBone").add(new Option(bone.name, bone.name)));
    byId("previewDetails").textContent = JSON.stringify(data.preview, null, 2);
    fillOptions(); select(""); byId("targetEditor").hidden = false;
    status.textContent = "Choose a required or existing name, or name a new target. Inspect the unfitted surface before selecting a location.";
    window.addEventListener("pagehide", withdraw);
  } catch (error) {
    withdraw(); status.textContent = error.message + ". No annotations were published.";
    byId("targetEditor").hidden = false;
    document.querySelectorAll("button,input,select").forEach(control => { control.disabled = true; });
  }
})();
