/* Explicit numeric start authoring only. Native code owns frames, model evaluation and validation. */
(() => {
  'use strict';
  const data = JSON.parse(document.getElementById('startData').textContent);
  const byId = id => document.getElementById(id);
  const copy = value => structuredClone(value);
  const original = copy(data.annotations.starts);
  const originalFixed = data.frames.map(frame => frame.fixed_pose ?? null);
  let fixed = copy(originalFixed), fixedPending = false, fixedInputs = [];
  let starts = copy(original), selected = starts.length ? 0 : null;
  let draft = selected === null ? null : copy(starts[selected]);
  let adding = false, pending = false, downloadUrl = null, revision = 0, needsReview = false;
  let importPending = false;
  let shapeInputs = [], poseInputs = [];
  const list = byId('startList'), status = byId('startStatus');
  const download = byId('startDownload');
  const draftDownload = byId('startDraftDownload');
  const preparedDownload = byId('startPreparedDownload'), preparedNotice = byId('startPreparedNotice');
  let preparedSource = null, preparedDownloadUrl = null;
  const preparedBoundary = 'Prepared input keeps the reviewed cameras, observations, masks and model settings. Source files are not reread; use native review if they changed.';
  function sameFixed(left, right) {
    if (!left || !right) return left === right;
    return left.evidence === right.evidence && data.pose.every(({name}) =>
      left.values[name] === right.values[name]);
  }
  const fixedChanged = () => fixed.some((entry, i) => !sameFixed(entry, originalFixed[i]));
  const changed = () => fixedChanged() || JSON.stringify(starts) !== JSON.stringify(original);

  function parsePrepared(text) {
    const value = JSON.parse(text, (_key, item, context) => {
      if (typeof item === 'number' && Number.isInteger(item) && !Number.isSafeInteger(item)
        && typeof JSON.rawJSON === 'function' && context?.source && /^-?\d+$/.test(context.source)) {
        return BigInt(context.source);
      }
      return item;
    });
    function exactNumbers(item) {
      if (typeof item === 'number' && (!Number.isFinite(item)
        || Number.isInteger(item) && !Number.isSafeInteger(item))) {
        throw new Error('This browser cannot preserve the prepared input numeric identities exactly.');
      }
      if (item !== null && typeof item === 'object') Object.values(item).forEach(exactNumbers);
    }
    exactNumbers(value);
    return value;
  }

  function checkedPrepared(value) {
    if (value.schema !== 'ftx-anny-body-fit-input/1'
      || value.model?.model_sha256 !== data.annotations.model_sha256
      || value.fit?.schema !== 'ftx-body-fit-input/1'
      || !Array.isArray(value.fit.frames) || value.fit.frames.length !== data.frames.length) {
      throw new Error('Prepared input does not match this initialization review.');
    }
    const seen = new Set();
    data.frames.forEach((frame, index) => {
      const identity = value.fit.frames[index].frame, expected = frame.identity;
      if (frame.frame_index !== index || !expected || typeof expected.stream !== 'string'
        || typeof expected.timestamp_us !== 'string' || !/^(0|[1-9]\d*)$/.test(expected.timestamp_us)
        || identity?.stream !== expected.stream
        || !['number', 'bigint'].includes(typeof identity.timestamp_us)
        || String(identity.timestamp_us) !== expected.timestamp_us) {
        throw new Error('Prepared frame identities disagree with the native frame ordinals.');
      }
      const key = JSON.stringify([expected.stream, expected.timestamp_us]);
      if (seen.has(key)) throw new Error('Prepared frame identities are duplicated.');
      seen.add(key);
      const fixed = frame.fixed_pose ?? null;
      const preparedFixed = value.fit.frames[index].fixed_pose ?? null;
      if ((fixed === null) !== (preparedFixed === null)) {
        throw new Error('Prepared input changes the reviewed fixed pose controls.');
      }
      if (fixed !== null) {
        fields(preparedFixed, ['values', 'evidence']);
        fields(preparedFixed.values, Object.keys(fixed.values));
        if (preparedFixed.evidence !== fixed.evidence
          || Object.keys(fixed.values).some(name => preparedFixed.values[name] !== fixed.values[name])) {
          throw new Error('Prepared input changes the reviewed fixed pose controls or evidence.');
        }
      }
    });
    return value;
  }

  try {
    preparedSource = checkedPrepared(parsePrepared(byId('startPreparedInput').textContent));
    preparedNotice.textContent = preparedBoundary;
  } catch (error) {
    preparedNotice.textContent = 'Prepared browser input unavailable: ' + error.message
      + ' Initialization annotations and native materialization remain available.';
  }

  function preparedText(annotations) {
    if (annotations.starts.length < 2 || annotations.starts.length > 4) {
      throw new Error('Prepared input needs 2–4 distinct complete starts.');
    }
    const complete = checkedAnnotations(annotations), value = copy(preparedSource);
    if (annotations.fixed_poses) {
      complete.fixed.forEach((pose, i) => {
        if (pose) value.fit.frames[i].fixed_pose = pose;
        else delete value.fit.frames[i].fixed_pose;
      });
    }
    // Frame ordinals identify original_fit, never the separately sorted preview/report frames.
    value.fit.starts = complete.starts.map(start => ({
      shape: copy(start.shape),
      poses: data.frames.map(frame => ({
        frame: value.fit.frames[frame.frame_index].frame,
        values: copy(start.poses.find(pose => pose.frame_index === frame.frame_index).values)
      }))
    }));
    return JSON.stringify(value, (_key, item) =>
      typeof item === 'bigint' ? JSON.rawJSON(item.toString())
        : Object.is(item, -0) ? JSON.rawJSON('-0.0') : item, 2) + '\n';
  }

  function withdrawPrepared() {
    preparedDownload.hidden = true;
    preparedDownload.removeAttribute('href');
    if (preparedDownloadUrl !== null) URL.revokeObjectURL(preparedDownloadUrl);
    preparedDownloadUrl = null;
  }

  function withdraw() {
    download.hidden = true;
    download.removeAttribute('href');
    draftDownload.hidden = true;
    draftDownload.removeAttribute('href');
    if (downloadUrl !== null) URL.revokeObjectURL(downloadUrl);
    downloadUrl = null;
    withdrawPrepared();
  }

  function updateDownload() {
    withdraw();
    if (pending || fixedPending || importPending || (!starts.length && !fixedChanged())) return;
    const annotations = { ...copy(data.annotations), starts: copy(starts) };
    if (fixedChanged()) {
      annotations.fixed_poses = fixed.map((fixed_pose, frame_index) => ({frame_index, fixed_pose}));
    }
    downloadUrl = URL.createObjectURL(new Blob([JSON.stringify(annotations, null, 2) + '\n'],
      { type: 'application/json' }));
    draftDownload.href = downloadUrl;
    draftDownload.hidden = false;
    if (!needsReview && changed() && starts.length >= 2 && starts.length <= 4) {
      download.href = downloadUrl;
      download.hidden = false;
      if (preparedSource !== null) {
        try {
          preparedDownloadUrl = URL.createObjectURL(new Blob([preparedText(annotations)],
            { type: 'application/json' }));
          preparedDownload.href = preparedDownloadUrl;
          preparedDownload.hidden = false;
          preparedNotice.textContent = preparedBoundary;
        } catch (error) {
          withdrawPrepared();
          preparedNotice.textContent = 'Prepared browser input unavailable: ' + error.message
            + ' Initialization annotations and native materialization remain available.';
        }
      }
    }
  }

  function updateButtons() {
    const editing = pending || fixedPending;
    list.disabled = editing || !starts.length;
    byId('newStart').disabled = editing || starts.length >= 4;
    byId('duplicateStart').disabled = editing || selected === null || starts.length >= 4;
    byId('removeStart').disabled = editing || selected === null;
    byId('midpointStart').disabled = fixedPending || draft === null;
    byId('applyStart').disabled = fixedPending || draft === null;
    byId('cancelStart').disabled = !editing && !importPending;
    byId('startFixedEditor').disabled = pending;
    byId('applyFixedStarts').disabled = !fixedPending;
    for (const fieldset of byId('startParameters').children) fieldset.disabled = fixedPending;
    byId('startPreviewNotice').textContent = (editing || changed()
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

  function editConstraints() {
    ++revision;
    fixedPending = true;
    withdraw();
    updateButtons();
    status.textContent = 'Review fixed pose values and evidence. Applying constraints copies prescribed values into every start. Releasing a control keeps its current starting values.';
  }

  function renderConstraints() {
    const container = byId('startFixedFields');
    container.replaceChildren();
    byId('startFixedEditor').hidden = !data.pose.length;
    fixedInputs = data.frames.map((frame, i) => {
      const group = element('fieldset');
      group.append(element('legend', frame.label));
      const inputs = data.pose.map(parameter => {
        const label = element('label'), prescribe = element('input'), value = element('input');
        prescribe.type = 'checkbox';
        prescribe.checked = Object.hasOwn(fixed[i]?.values ?? {}, parameter.name);
        label.append(prescribe, element('span', ' Fix ' + parameter.name));
        const valueLabel = element('label');
        value.type = 'number';
        value.step = 'any';
        value.min = String(parameter.lower);
        value.max = String(parameter.upper);
        value.value = prescribe.checked ? String(fixed[i].values[parameter.name]) : '';
        value.disabled = !prescribe.checked;
        valueLabel.append(element('span', `${parameter.name} (${parameter.unit}), ${parameter.lower} < value < ${parameter.upper} `), value);
        prescribe.addEventListener('change', () => {
          value.disabled = !prescribe.checked;
          editConstraints();
        });
        value.addEventListener('input', editConstraints);
        group.append(label, valueLabel);
        return {parameter, prescribe, value};
      });
      const evidence = element('textarea'), label = element('label');
      evidence.value = fixed[i]?.evidence ?? '';
      label.append(element('span', 'Independent evidence for these fixed controls '), evidence);
      evidence.addEventListener('input', editConstraints);
      group.append(label);
      container.append(group);
      return {inputs, evidence};
    });
  }

  function parameterInput(parameter, value, group, index, frameIndex, fixedPose) {
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
    const fixed = Object.hasOwn(fixedPose?.values ?? {}, parameter.name);
    input.readOnly = fixed;
    input.disabled = fixed;
    label.append(element('span', fixed
      ? `${parameter.name} (${parameter.unit}); fixed for this frame. ${fixedPose.evidence} `
      : `${parameter.name} (${parameter.unit}); ${parameter.lower} < value < ${parameter.upper} `), input);
    input.addEventListener('input', () => { if (!fixed) markPending(); });
    return { label, input, parameter, fixed, fixedValue: fixed ? fixedPose.values[parameter.name] : undefined };
  }

  function render() {
    renderConstraints();
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
          const control = parameterInput(parameter, values[index], 'pose', index, frame.frame_index, fixed[frame.frame_index]);
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
    return checkedValue(text ? Number(text) : NaN, control.parameter, location,
      control.fixed ? control.fixedValue : undefined);
  }

  function checkedValue(value, p, location, fixedValue) {
    if (fixedValue !== undefined) {
      if (typeof value !== 'number' || !Number.isFinite(value) || value !== fixedValue
        || value <= p.lower || value >= p.upper) {
        throw new Error(`${location}: ${p.name} must retain its declared fixed value inside the bounds.`);
      }
      return value;
    }
    const normalized = (value - p.lower) / (p.upper - p.lower);
    if (typeof value !== 'number' || !Number.isFinite(value) || !(normalized > 0 && normalized < 1)) {
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

  function fields(value, expected) {
    if (!value || typeof value !== 'object' || Array.isArray(value)
      || Object.keys(value).length !== expected.length || expected.some(key => !Object.hasOwn(value, key))) {
      throw new Error('Missing or unsupported initialization fields.');
    }
  }

  function checkedConstraints(entries) {
    if (!Array.isArray(entries) || entries.length !== data.frames.length) {
      throw new Error('Supply constraints or an explicit release for every source frame.');
    }
    const result = new Array(entries.length);
    let free = data.shape.length;
    for (const entry of entries) {
      fields(entry, ['frame_index', 'fixed_pose']);
      const {frame_index: i, fixed_pose: pose} = entry;
      if (!Number.isInteger(i) || i < 0 || i >= entries.length || Object.hasOwn(result, i)) {
        throw new Error('Unknown or duplicate fixed pose frame ordinal.');
      }
      result[i] = pose;
      free += data.pose.length;
      if (pose === null) continue;
      fields(pose, ['values', 'evidence']);
      if (typeof pose.evidence !== 'string' || !pose.evidence.trim() || new TextEncoder().encode(pose.evidence).length > 4096) {
        throw new Error(`Frame ${i + 1} requires explicit fixed pose evidence of at most 4096 UTF-8 bytes.`);
      }
      if (!pose.values || typeof pose.values !== 'object' || Array.isArray(pose.values) || !Object.keys(pose.values).length) {
        throw new Error(`Frame ${i + 1} requires a nonempty set of named fixed controls.`);
      }
      for (const [name, value] of Object.entries(pose.values)) {
        const p = data.pose.find(parameter => parameter.name === name);
        if (!p) throw new Error('Unknown fixed pose control: ' + name);
        checkedValue(value, p, 'Fixed pose for Frame ' + (i + 1), value);
        free--;
      }
    }
    if (free > 128) throw new Error('Releasing these constraints exceeds the native 128 free-parameter limit.');
    return result;
  }

  function checkedAnnotations(annotations) {
    const edited = annotations != null && Object.hasOwn(annotations, 'fixed_poses');
    fields(annotations, edited ? [...Object.keys(data.annotations), 'fixed_poses'] : Object.keys(data.annotations));
    for (const key of ['schema', 'source_input', 'source_input_sha256', 'model_sha256', 'capture_sources_sha256']) {
      if (annotations[key] !== data.annotations[key]) throw new Error('The draft belongs to a different source, model or capture.');
    }
    const masks = data.annotations.silhouettes;
    if (masks === null) {
      if (annotations.silhouettes !== null) throw new Error('The draft has different silhouette bindings.');
    } else {
      fields(annotations.silhouettes, ['path', 'sha256']);
      if (annotations.silhouettes.path !== masks.path || annotations.silhouettes.sha256 !== masks.sha256) {
        throw new Error('The draft has different silhouette bindings.');
      }
    }
    const constraints = edited ? checkedConstraints(annotations.fixed_poses) : copy(originalFixed);
    checkStarts(annotations.starts, constraints);
    return {starts: annotations.starts, fixed: constraints};
  }

  function checkStarts(proposed, constraints) {
    if (!Array.isArray(proposed) || proposed.length > 4) throw new Error('A draft supports at most four applied starts.');
    for (const start of proposed) {
      fields(start, ['shape', 'poses']);
      if (!Array.isArray(start.shape) || start.shape.length !== data.shape.length) throw new Error('Every shared shape value is required.');
      start.shape.forEach((value, index) => checkedValue(value, data.shape[index], 'Shared shape'));
      if (!Array.isArray(start.poses) || start.poses.length !== data.frames.length) throw new Error('Every original frame pose is required.');
      const seen = new Set();
      for (const pose of start.poses) {
        fields(pose, ['frame_index', 'values']);
        if (!data.frames.some(frame => frame.frame_index === pose.frame_index) || seen.has(pose.frame_index)) throw new Error('Unknown or duplicate frame ordinal.');
        seen.add(pose.frame_index);
        if (!Array.isArray(pose.values) || pose.values.length !== data.pose.length) throw new Error('Every named pose value is required.');
        const fixed = constraints[pose.frame_index]?.values ?? {};
        pose.values.forEach((value, index) => {
          checkedValue(value, data.pose[index], 'Frame pose',
            Object.hasOwn(fixed, data.pose[index].name) ? fixed[data.pose[index].name] : undefined);
        });
      }
    }
    validateDistinct(proposed);
  }

  byId('applyFixedStarts').addEventListener('click', () => {
    if (!fixedPending || pending) return;
    try {
      const next = checkedConstraints(fixedInputs.map(({inputs, evidence}, frame_index) => {
        const values = Object.fromEntries(inputs.filter(({prescribe}) => prescribe.checked).map(({parameter, value}) =>
          [parameter.name, value.value.trim() === '' ? NaN : Number(value.value)]));
        return {frame_index, fixed_pose: Object.keys(values).length ? {values, evidence: evidence.value} : null};
      }));
      const proposed = copy(starts);
      for (const start of proposed) {
        for (const pose of start.poses) {
          const values = next[pose.frame_index]?.values ?? {};
          data.pose.forEach(({name}, i) => {
            if (Object.hasOwn(values, name)) pose.values[i] = values[name];
          });
        }
      }
      checkStarts(proposed, next);
      fixed = next;
      starts = proposed;
      draft = selected === null ? null : copy(starts[selected]);
      fixedPending = false;
      importPending = false;
      needsReview = false;
      render();
      status.textContent = `Applied frame constraints and reconciled ${starts.length} starts. Inspect updated poses in a new native review.`;
    } catch (error) {
      status.textContent = 'Constraints not applied: ' + error.message;
    }
  });

  const savedFile = byId('startDraftFile');
  for (const event of ['focus', 'click', 'cancel']) savedFile.addEventListener(event, () => {
    ++revision;
    importPending = true;
    withdraw();
    updateButtons();
    status.textContent = 'Choose saved starts, or Apply this start or Cancel pending edit to restore downloads for retained values.';
  });
  savedFile.addEventListener('change', async () => {
    const ticket = ++revision, file = byId('startDraftFile').files[0];
    byId('startDraftFile').value = '';
    // A replacement file is unresolved evidence, even if the read fails or the chooser is canceled.
    importPending = true;
    withdraw();
    updateButtons();
    if (!file) { updateButtons(); status.textContent = 'No draft selected; current starts retained.'; return; }
    status.textContent = 'Reading saved starts. Current values remain unchanged until the draft is checked.';
    try {
      if (file.size > 256 * 1024) throw new Error('The draft exceeds the 256 KiB limit.');
      const bytes = await file.arrayBuffer();
      if (ticket !== revision) return;
      const proposed = checkedAnnotations(JSON.parse(new TextDecoder('utf-8', {fatal: true, ignoreBOM: true}).decode(bytes)));
      starts = proposed.starts;
      fixed = proposed.fixed;
      selected = starts.length ? 0 : null;
      draft = selected === null ? null : copy(starts[selected]);
      adding = false; pending = false; fixedPending = false; needsReview = true; importPending = false;
      render();
      status.textContent = `${starts.length} applied starts restored. Review every start, then Apply this start to prepare completed annotations. Original previews remain unchanged.`;
    } catch (error) {
      if (ticket === revision) status.textContent = `Draft not loaded: ${error.message}`;
    } finally {
      if (ticket === revision) updateButtons();
    }
  });

  for (const event of ['input', 'change', 'click']) {
    byId('startEditor').addEventListener(event, () => {
      ++revision;
      if (status.textContent.startsWith('Reading saved starts.')) status.textContent = 'Draft read canceled by a later editor action; current starts retained.';
      updateButtons();
    });
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
    poses: data.frames.map(frame => ({ frame_index: frame.frame_index,
      values: data.pose.map(parameter => Object.hasOwn(fixed[frame.frame_index]?.values ?? {}, parameter.name)
        ? fixed[frame.frame_index].values[parameter.name] : '') }))
  }));
  byId('duplicateStart').addEventListener('click', () => {
    if (selected !== null) begin(copy(starts[selected]));
  });
  byId('midpointStart').addEventListener('click', () => {
    if (draft === null) return;
    [...shapeInputs, ...poseInputs.flatMap(frame => frame.controls)].forEach(control => {
      const p = control.parameter;
      control.input.value = String(control.fixed ? control.fixedValue : p.lower + 0.5 * (p.upper - p.lower));
    });
    markPending();
    status.textContent = 'Bounds midpoints entered as an explicit guess for free controls; declared fixed poses are retained. Inspect every value and apply when ready; no body or pose was estimated.';
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
      needsReview = false;
      importPending = false;
      draft = copy(starts[selected]);
      render();
      status.textContent = `${starts.length} applied start${starts.length === 1 ? '' : 's'}. `
        + (starts.length < 2 ? 'Save the applied start draft, or add a distinct second start for completed annotations.'
          : changed() ? 'Download prepared input for browser fitting, or materialize annotations natively for updated previews and source checks.'
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
    fixedPending = false;
    importPending = false;
    draft = selected === null ? null : copy(starts[selected]);
    render();
    status.textContent = 'Pending edit canceled. Applied starts are unchanged.';
  });
  byId('resetStarts').addEventListener('click', () => {
    starts = copy(original);
    fixed = copy(originalFixed);
    fixedPending = false;
    selected = starts.length ? 0 : null;
    draft = selected === null ? null : copy(starts[selected]);
    adding = false;
    pending = false;
    needsReview = false;
    importPending = false;
    render();
    status.textContent = 'All start changes reset to the original input. No completed edited download remains.';
  });
  window.addEventListener('pagehide', () => {
    ++revision; importPending = true; withdraw();
    updateButtons();
    status.textContent = 'Saved-start reads canceled when leaving this page. Applied starts retained; Apply this start or Cancel pending edit to restore downloads after returning.';
  });
  render();
  byId('startEditor').hidden = false;
  status.textContent = `${starts.length} original start${starts.length === 1 ? '' : 's'}. Add or edit explicit values; a download needs 2–4 distinct starts.`;
})();
