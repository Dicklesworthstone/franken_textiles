/* Explicit numeric start authoring only. Native code owns frames, model evaluation and validation. */
(() => {
  'use strict';
  const data = JSON.parse(document.getElementById('startData').textContent);
  const byId = id => document.getElementById(id);
  const copy = value => JSON.parse(JSON.stringify(value));
  const original = copy(data.annotations.starts);
  let starts = copy(original), selected = starts.length ? 0 : null;
  let draft = selected === null ? null : copy(starts[selected]);
  let adding = false, pending = false, downloadUrl = null;
  let shapeInputs = [], poseInputs = [];
  const list = byId('startList'), status = byId('startStatus');
  const download = byId('startDownload');
  const changed = () => JSON.stringify(starts) !== JSON.stringify(original);

  function withdraw() {
    download.hidden = true;
    download.removeAttribute('href');
    if (downloadUrl !== null) URL.revokeObjectURL(downloadUrl);
    downloadUrl = null;
  }

  function updateDownload() {
    withdraw();
    if (pending || !changed() || starts.length < 2 || starts.length > 4) return;
    const annotations = { ...copy(data.annotations), starts: copy(starts) };
    downloadUrl = URL.createObjectURL(new Blob([JSON.stringify(annotations, null, 2) + '\n'],
      { type: 'application/json' }));
    download.href = downloadUrl;
    download.hidden = false;
  }

  function updateButtons() {
    list.disabled = pending || !starts.length;
    byId('newStart').disabled = pending || starts.length >= 4;
    byId('duplicateStart').disabled = pending || selected === null || starts.length >= 4;
    byId('removeStart').disabled = pending || selected === null;
    byId('midpointStart').disabled = draft === null;
    byId('applyStart').disabled = draft === null;
    byId('cancelStart').disabled = !pending;
    byId('startPreviewNotice').textContent = (pending || changed()
      ? 'Edited values have no updated preview. '
      : '') + 'All photographs, projections and meshes below show the original input. '
      + 'Materialize the download and regenerate a native review to inspect edited starts.';
  }

  function markPending() {
    pending = true;
    withdraw();
    updateButtons();
    status.textContent = 'Pending edit: apply every value or cancel. Downloads are withdrawn until the draft is resolved.';
  }

  function element(tag, text) {
    const node = document.createElement(tag);
    if (text !== undefined) node.textContent = text;
    return node;
  }

  function parameterInput(parameter, value, group, index, frameIndex) {
    const label = element('label');
    const input = element('input');
    input.type = 'number';
    input.step = 'any';
    input.min = String(parameter.lower);
    input.max = String(parameter.upper);
    input.value = value === '' ? '' : String(value);
    input.dataset.group = group;
    input.dataset.parameter = String(index);
    if (frameIndex !== undefined) input.dataset.frame = String(frameIndex);
    input.id = `start-${group}-${frameIndex === undefined ? '' : frameIndex + '-'}${index}`;
    label.htmlFor = input.id;
    label.append(element('span', `${parameter.name} (${parameter.unit}); ${parameter.lower} < value < ${parameter.upper} `), input);
    input.addEventListener('input', markPending);
    return { label, input, parameter };
  }

  function render() {
    list.replaceChildren();
    starts.forEach((_, index) => {
      const option = element('option', `Start ${index + 1}`);
      option.value = String(index);
      list.append(option);
    });
    if (adding || !starts.length) {
      const option = element('option', adding ? 'New start (pending)' : 'No starts yet');
      option.value = '';
      list.append(option);
    }
    list.value = adding || selected === null ? '' : String(selected);
    const parameters = byId('startParameters');
    parameters.replaceChildren();
    shapeInputs = [];
    poseInputs = [];
    if (draft !== null) {
      const shared = element('fieldset');
      shared.append(element('legend', 'Shared shape — same for every frame in this start'));
      data.shape.forEach((parameter, index) => {
        const control = parameterInput(parameter, draft.shape[index], 'shape', index);
        shared.append(control.label);
        shapeInputs.push(control);
      });
      parameters.append(shared);
      data.frames.forEach(frame => {
        const fieldset = element('fieldset');
        fieldset.append(element('legend', frame.label));
        const values = draft.poses.find(pose => pose.frame_index === frame.frame_index).values;
        const controls = data.pose.map((parameter, index) => {
          const control = parameterInput(parameter, values[index], 'pose', index, frame.frame_index);
          fieldset.append(control.label);
          return control;
        });
        poseInputs.push({ frame_index: frame.frame_index, label: frame.label, controls });
        parameters.append(fieldset);
      });
    } else {
      parameters.append(element('p', 'Add a blank start, then enter each named value.'));
    }
    updateButtons();
    updateDownload();
  }

  function valueOf(control, location) {
    const text = control.input.value.trim();
    const value = Number(text);
    const p = control.parameter;
    const normalized = (value - p.lower) / (p.upper - p.lower);
    if (!text || !Number.isFinite(value) || !(normalized > 0 && normalized < 1)) {
      throw new Error(`${location}: ${p.name} (${p.unit}) needs a finite value strictly between ${p.lower} and ${p.upper}.`);
    }
    return value;
  }

  function readDraft() {
    return {
      shape: shapeInputs.map(control => valueOf(control, 'Shared shape')),
      poses: poseInputs.map(frame => ({ frame_index: frame.frame_index,
        values: frame.controls.map(control => valueOf(control, frame.label)) }))
    };
  }

  function normalized(start) {
    const result = start.shape.map((value, i) =>
      (value - data.shape[i].lower) / (data.shape[i].upper - data.shape[i].lower));
    data.frames.forEach(frame => {
      const values = start.poses.find(pose => pose.frame_index === frame.frame_index).values;
      data.pose.forEach((p, i) => result.push((values[i] - p.lower) / (p.upper - p.lower)));
    });
    return result;
  }

  function validateDistinct(proposed) {
    const encoded = proposed.map(normalized);
    for (let i = 0; i < encoded.length; i++) {
      for (let j = 0; j < i; j++) {
        if (encoded[i].every((value, k) => Math.abs(value - encoded[j][k]) < 1e-6)) {
          throw new Error(`Starts ${j + 1} and ${i + 1} are duplicates within the native normalized tolerance. Change at least one parameter before applying.`);
        }
      }
    }
  }

  function begin(value) {
    if (pending || starts.length >= 4) return;
    draft = value;
    adding = true;
    pending = true;
    render();
    status.textContent = 'New start is pending. Enter every value, then apply; cancel discards this draft.';
  }

  byId('newStart').addEventListener('click', () => begin({
    shape: data.shape.map(() => ''),
    poses: data.frames.map(frame => ({ frame_index: frame.frame_index, values: data.pose.map(() => '') }))
  }));
  byId('duplicateStart').addEventListener('click', () => {
    if (selected !== null) begin(copy(starts[selected]));
  });
  byId('midpointStart').addEventListener('click', () => {
    if (draft === null) return;
    [...shapeInputs, ...poseInputs.flatMap(frame => frame.controls)].forEach(control => {
      const p = control.parameter;
      control.input.value = String(p.lower + 0.5 * (p.upper - p.lower));
    });
    markPending();
    status.textContent = 'Bounds midpoints entered as an explicit guess. Inspect every value and apply when ready; no body or pose was estimated.';
  });
  byId('applyStart').addEventListener('click', () => {
    if (draft === null) return;
    withdraw();
    try {
      const candidate = readDraft();
      const proposed = copy(starts);
      const index = adding ? proposed.length : selected;
      if (index === null || index >= 4) throw new Error('At most four starts are allowed.');
      proposed[index] = candidate;
      validateDistinct(proposed);
      starts = proposed;
      selected = index;
      adding = false;
      pending = false;
      draft = copy(starts[selected]);
      render();
      status.textContent = `${starts.length} applied start${starts.length === 1 ? '' : 's'}. `
        + (starts.length < 2 ? 'Add a distinct second start before downloading.'
          : changed() ? 'Download the edited annotations and materialize them with the native command.'
            : 'Values match the original annotations.');
    } catch (error) {
      pending = true;
      updateButtons();
      status.textContent = `Start not applied: ${error.message}`;
    }
  });
  list.addEventListener('change', () => {
    if (pending) {
      list.value = adding || selected === null ? '' : String(selected);
      status.textContent = 'Apply or cancel the pending edit before selecting another start.';
      return;
    }
    const index = Number(list.value);
    if (!Number.isInteger(index) || index < 0 || index >= starts.length) return;
    selected = index;
    draft = copy(starts[selected]);
    render();
    status.textContent = `Editing Start ${selected + 1}. Existing previews still show the original input.`;
  });
  byId('removeStart').addEventListener('click', () => {
    if (pending || selected === null) return;
    starts.splice(selected, 1);
    selected = starts.length ? Math.min(selected, starts.length - 1) : null;
    draft = selected === null ? null : copy(starts[selected]);
    render();
    status.textContent = `Start removed. ${starts.length} applied; a download needs 2–4 distinct starts.`;
  });
  byId('cancelStart').addEventListener('click', () => {
    adding = false;
    pending = false;
    draft = selected === null ? null : copy(starts[selected]);
    render();
    status.textContent = 'Pending edit canceled. Applied starts are unchanged.';
  });
  byId('resetStarts').addEventListener('click', () => {
    starts = copy(original);
    selected = starts.length ? 0 : null;
    draft = selected === null ? null : copy(starts[selected]);
    adding = false;
    pending = false;
    render();
    status.textContent = 'All start changes reset to the original input. No edited download remains.';
  });
  window.addEventListener('pagehide', withdraw);
  render();
  byId('startEditor').hidden = false;
  status.textContent = `${starts.length} original start${starts.length === 1 ? '' : 's'}. Add or edit explicit values; a download needs 2–4 distinct starts.`;
})();
