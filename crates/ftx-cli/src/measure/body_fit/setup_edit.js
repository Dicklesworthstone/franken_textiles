/* Explicit capture setup choices only. Native commands own source, camera and model validation. */
(() => {
  'use strict';
  const data = JSON.parse(document.getElementById('setupData').textContent);
  const byId = id => document.getElementById(id);
  const copy = value => JSON.parse(JSON.stringify(value));
  const seed = copy(data.annotations), catalogue = data.catalogue;
  const download = byId('setupDownload'), status = byId('setupStatus');
  let shapeRows = [], rootRows = [], jointRows = [], measurementRows = [], frameInputs = [];
  let applied = null, pending = false, downloadUrl = null;

  function element(tag, text) {
    const node = document.createElement(tag);
    if (text !== undefined) node.textContent = text;
    return node;
  }
  function option(value, text) {
    const node = element('option', text);
    node.value = value;
    return node;
  }
  function field(parent, text, input) {
    const label = element('label', text + ' ');
    label.append(input);
    parent.append(label);
    return input;
  }
  function number(parent, text, value, fieldName, bounds) {
    const input = element('input');
    input.type = 'number';
    input.step = 'any';
    input.dataset.field = fieldName;
    input.value = value === undefined || value === null ? '' : String(value);
    if (bounds) { input.min = String(bounds.lower); input.max = String(bounds.upper); }
    return field(parent, text, input);
  }
  function withdraw() {
    download.hidden = true;
    download.removeAttribute('href');
    if (downloadUrl !== null) URL.revokeObjectURL(downloadUrl);
    downloadUrl = null;
  }
  function publish() {
    withdraw();
    if (pending || applied === null) return;
    downloadUrl = URL.createObjectURL(new Blob([JSON.stringify(applied, null, 2) + '\n'],
      { type: 'application/json' }));
    download.href = downloadUrl;
    download.hidden = false;
  }
  function frameSummary() {
    const selected = frameInputs.filter(item => item.input.checked);
    const cohorts = new Set(selected.map(item => item.frame.cohort));
    byId('setupFrameCount').textContent = `${selected.length} frame${selected.length === 1 ? '' : 's'} selected; `
      + (cohorts.size > 1 ? 'several acquisition cohorts selected — choose one cohort.' : 'choose 2–8 from one compatible cohort.');
  }
  function markPending() {
    pending = true;
    withdraw();
    byId('cancelSetup').disabled = false;
    frameSummary();
    status.textContent = 'Pending choices. The previous download has been withdrawn; check the complete form or cancel these edits.';
  }
  function selectItems(id, items, used, prompt) {
    const select = byId(id);
    const old = select.value;
    select.replaceChildren(option('', prompt));
    items.filter(item => !used.includes(item)).forEach(item => select.append(option(item, item)));
    if ([...select.options].some(item => item.value === old)) select.value = old;
  }
  function updateCatalogues() {
    selectItems('setupShapeName', catalogue.shapes.map(item => item.name), shapeRows.map(row => row.name), 'Choose a model shape control');
    selectItems('setupJointName', catalogue.joints.map(item => item.name), jointRows.map(row => row.name), 'Choose an articulated bone');
    selectItems('setupMeasurementName', catalogue.measurements, measurementRows.map(row => row.name), 'Choose a tape functional');
    byId('addSetupShape').disabled = shapeRows.length >= catalogue.shapes.length;
    byId('addSetupJoint').disabled = jointRows.length >= 32 || jointRows.length >= catalogue.joints.length;
    byId('addSetupMeasurement').disabled = measurementRows.length >= 32 || measurementRows.length >= catalogue.measurements.length;
    const defaults = byId('setupDefaults');
    defaults.replaceChildren();
    const remaining = catalogue.shapes.filter(item => !shapeRows.some(row => row.name === item.name));
    defaults.append(element('p', 'Unselected shape controls keep their declared model defaults. These are model assumptions, not measured values.'));
    defaults.append(element('pre', remaining.map(item => `${item.name}: ${item.default_value} ${item.unit}`).join('\n') || 'Every available shape control is explicitly declared.'));
    const joints = catalogue.joints.filter(item => !jointRows.some(row => row.name === item.name));
    defaults.append(element('p', `${joints.length} unselected articulated bones retain zero additional rotation in the model rest pose.`));
    defaults.append(element('pre', joints.map(item => item.name).join(', ') || 'Every available articulated bone is explicitly declared.'));
  }
  function scalarRow(group, parameter, choice, removable) {
    const node = element('fieldset');
    node.dataset.group = group;
    node.dataset.name = parameter.name;
    if (group === 'joint') node.dataset.bone = parameter.name;
    node.append(element('legend', `${parameter.name} (${parameter.unit})`));
    node.append(element('small', `Model domain: ${parameter.lower} to ${parameter.upper}. Original model default: ${parameter.default_value}.`));
    const mode = element('select');
    mode.dataset.field = 'mode';
    mode.append(option('', 'Choose fixed or free'), option('fixed', 'Fixed value'), option('fit', 'Free within bounds'));
    mode.value = choice ? choice.mode : '';
    field(node, 'Treatment', mode);
    const fixed = element('div'), fit = element('div');
    fit.className = 'fields';
    const value = number(fixed, `Fixed value (${parameter.unit})`, choice && choice.value, 'value', parameter);
    const lower = number(fit, `Exclusive lower bound (${parameter.unit})`, choice && choice.lower, 'lower', parameter);
    const upper = number(fit, `Exclusive upper bound (${parameter.unit})`, choice && choice.upper, 'upper', parameter);
    node.append(fixed, fit);
    const visibility = () => { fixed.hidden = mode.value !== 'fixed'; fit.hidden = mode.value !== 'fit'; };
    mode.addEventListener('change', visibility);
    visibility();
    const row = { name: parameter.name, parameter, node, mode, value, lower, upper };
    if (removable) {
      const remove = element('button', 'Remove this control');
      remove.type = 'button';
      remove.dataset.action = 'remove';
      remove.addEventListener('click', () => {
        if (group === 'shape') shapeRows = shapeRows.filter(item => item !== row);
        else jointRows = jointRows.filter(item => item !== row);
        node.remove();
        updateCatalogues();
        markPending();
      });
      node.append(remove);
    }
    return row;
  }
  function addShape(name, choice) {
    const parameter = catalogue.shapes.find(item => item.name === name);
    if (!parameter || shapeRows.some(row => row.name === name)) return;
    const row = scalarRow('shape', parameter, choice, true);
    shapeRows.push(row);
    byId('setupShapes').append(row.node);
  }
  function addJoint(name, values) {
    const bone = catalogue.joints.find(item => item.name === name);
    if (!bone || jointRows.some(row => row.name === name) || jointRows.length >= 32) return;
    const parameter = { name, unit: 'rad', lower: -Math.PI, upper: Math.PI, default_value: 0 };
    const row = scalarRow('joint', parameter, values && values.choice, true);
    const axes = element('div');
    axes.className = 'fields';
    row.axes = ['x', 'y', 'z'].map((axis, i) =>
      number(axes, `Local unit axis ${axis}`, values && values.axis[i], `axis-${axis}`, { lower: -1, upper: 1 }));
    row.node.append(element('small', `Rig index ${bone.index}; parent ${bone.parent_name}. All three axis components are required.`), axes);
    jointRows.push(row);
    byId('setupJoints').append(row.node);
  }
  function addMeasurement(name, values) {
    if (!catalogue.measurements.includes(name) || measurementRows.some(row => row.name === name) || measurementRows.length >= 32) return;
    const node = element('fieldset');
    node.dataset.group = 'measurement';
    node.dataset.name = name;
    node.append(element('legend', name));
    const fields = element('div');
    fields.className = 'fields';
    const value = number(fields, 'Measured value (m)', values && values.value_m, 'value_m');
    const sigma = number(fields, 'Uncertainty sigma (m)', values && values.sigma_m, 'sigma_m', { lower: 1e-6, upper: 1 });
    const partition = element('select');
    partition.dataset.field = 'partition';
    partition.append(option('', 'Choose a predeclared role'), option('fit', 'FIT — used in fitting'), option('held_out', 'CHECK — held out'));
    partition.value = values ? values.partition : '';
    field(fields, 'Role', partition);
    node.append(fields);
    const evidence = element('textarea');
    evidence.dataset.field = 'evidence';
    evidence.maxLength = 4096;
    evidence.value = values ? values.evidence : '';
    field(node, 'Measurement definition and source evidence', evidence);
    const row = { name, node, value, sigma, partition, evidence };
    const remove = element('button', 'Remove this tape observation');
    remove.type = 'button';
    remove.dataset.action = 'remove';
    remove.addEventListener('click', () => {
      measurementRows = measurementRows.filter(item => item !== row);
      node.remove();
      updateCatalogues();
      markPending();
    });
    node.append(remove);
    measurementRows.push(row);
    byId('setupMeasurements').append(node);
  }
  function render(annotations) {
    withdraw();
    byId('setupAge').value = annotations.age_years === null ? '' : String(annotations.age_years);
    byId('setupGender').value = annotations.gender === null ? '' : String(annotations.gender);
    byId('setupScale').value = annotations.metric_scale_source;
    byId('setupEvidence').value = annotations.observation_evidence;
    frameInputs = [];
    const frames = byId('setupFrames');
    frames.replaceChildren();
    data.frames.forEach(frame => {
      const card = element('article');
      card.className = `frame-card${frame.eligible ? '' : ' ineligible'}`;
      const image = element('img');
      image.src = frame.image.file;
      image.alt = frame.label;
      image.loading = 'lazy';
      image.width = frame.image_size_px[0];
      image.height = frame.image_size_px[1];
      card.append(image);
      const input = element('input');
      input.type = 'checkbox';
      input.dataset.frameIndex = String(frame.frame_index);
      input.checked = annotations.frame_indices.includes(frame.frame_index);
      input.disabled = !frame.eligible;
      field(card, frame.label, input);
      card.append(element('small', `Cohort: ${frame.cohort}. ${frame.image_size_px[0]} × ${frame.image_size_px[1]} px.`));
      if (!frame.eligible) card.append(element('p', 'This frame is incompatible with the supplied calibration. Prepare a separate setup with its matching calibration.'));
      frames.append(card);
      frameInputs.push({ frame, input });
    });
    shapeRows = []; rootRows = []; jointRows = []; measurementRows = [];
    ['setupShapes', 'setupRoots', 'setupJoints', 'setupMeasurements'].forEach(id => byId(id).replaceChildren());
    annotations.shapes.forEach(item => addShape(item.name, item.choice));
    catalogue.roots.forEach(parameter => {
      const value = annotations.roots.find(item => item.name === parameter.name);
      const row = scalarRow('root', parameter, value && value.choice, false);
      rootRows.push(row);
      byId('setupRoots').append(row.node);
    });
    annotations.joints.forEach(item => addJoint(item.bone, item));
    annotations.measurements.forEach(item => addMeasurement(item.name, item));
    updateCatalogues();
    frameSummary();
    byId('cancelSetup').disabled = !pending;
  }
  function finite(input, label, bounds) {
    const text = input.value.trim(), value = Number(text);
    if (!text || !Number.isFinite(value)) throw new Error(`${label} needs an explicit finite number.`);
    if (bounds && (value < bounds.lower || value > bounds.upper)) throw new Error(`${label} must be between ${bounds.lower} and ${bounds.upper}.`);
    return value;
  }
  function textValue(input, label) {
    const value = input.value;
    if (!value.trim() || new TextEncoder().encode(value).length > 4096) throw new Error(`${label} needs nonempty evidence of at most 4096 UTF-8 bytes.`);
    return value;
  }
  function readChoice(row) {
    if (row.mode.value === 'fixed') return { mode: 'fixed', value: finite(row.value, row.name, row.parameter) };
    if (row.mode.value !== 'fit') throw new Error(`${row.name}: explicitly choose fixed or free.`);
    const lower = finite(row.lower, `${row.name} lower bound`, row.parameter);
    const upper = finite(row.upper, `${row.name} upper bound`, row.parameter);
    if (!(lower < upper) || !Number.isFinite(upper - lower)) throw new Error(`${row.name}: lower bound must be smaller than upper bound.`);
    return { mode: 'fit', lower, upper };
  }
  function readChoices() {
    const selected = frameInputs.filter(item => item.input.checked);
    if (selected.length < 2 || selected.length > 8) throw new Error('Choose 2–8 source frames.');
    if (selected.some(item => !item.frame.eligible)) throw new Error('Every selected frame must match the supplied calibration.');
    if (new Set(selected.map(item => item.frame.cohort)).size !== 1) throw new Error('Choose frames from one acquisition cohort.');
    const shapes = shapeRows.map(row => ({ name: row.name, choice: readChoice(row) }));
    const roots = rootRows.map(row => ({ name: row.name, choice: readChoice(row) }));
    const fixedRotation = ['root_rx_rad', 'root_ry_rad', 'root_rz_rad'].map(name => {
      const choice = roots.find(item => item.name === name).choice;
      return choice.mode === 'fixed' ? choice.value : 0;
    });
    if (Math.sqrt(fixedRotation.reduce((sum, value) => sum + value * value, 0)) > Math.PI) throw new Error('The fixed root rotation-vector components together must have length at most pi radians.');
    const joints = jointRows.map(row => {
      const axis = row.axes.map((input, i) => finite(input, `${row.name} axis ${'xyz'[i]}`, { lower: -1, upper: 1 }));
      if (Math.abs(Math.hypot(...axis) - 1) > 1e-10) throw new Error(`${row.name}: enter a unit axis; no automatic normalization is performed.`);
      return { bone: row.name, axis, choice: readChoice(row) };
    });
    const shapeCount = shapes.filter(item => item.choice.mode === 'fit').length;
    const poseCount = [...roots, ...joints].filter(item => item.choice.mode === 'fit').length;
    if (shapeCount < 1 || shapeCount > 12) throw new Error('Declare 1–12 free shape controls.');
    if (poseCount < 1 || poseCount > 18) throw new Error('Declare 1–18 free root/joint controls.');
    if (shapeCount + poseCount * selected.length > 128) throw new Error('Shared shape plus independent poses exceed the 128-parameter native budget.');
    const measurements = measurementRows.map(row => {
      const value = finite(row.value, `${row.name} measured value`);
      if (!(value > 0)) throw new Error(`${row.name}: measured value must be positive metres.`);
      const sigma = finite(row.sigma, `${row.name} uncertainty`, { lower: 1e-6, upper: 1 });
      if (!['fit', 'held_out'].includes(row.partition.value)) throw new Error(`${row.name}: choose a predeclared FIT or CHECK role.`);
      return { name: row.name, value_m: value, sigma_m: sigma,
        evidence: textValue(row.evidence, `${row.name} evidence`), partition: row.partition.value };
    });
    return { ...copy(seed), frame_indices: selected.map(item => item.frame.frame_index),
      age_years: finite(byId('setupAge'), 'Age in years', { lower: 0, upper: 110 }),
      gender: finite(byId('setupGender'), 'Model gender parameter', { lower: 0, upper: 1 }),
      shapes, roots, joints, measurements,
      metric_scale_source: textValue(byId('setupScale'), 'Metric scale source'),
      observation_evidence: textValue(byId('setupEvidence'), 'Observation and capture evidence') };
  }
  byId('setupEditor').addEventListener('input', markPending);
  byId('setupEditor').addEventListener('change', markPending);
  byId('addSetupShape').addEventListener('click', () => {
    const name = byId('setupShapeName').value;
    if (!name) { status.textContent = 'Choose a shape control to add.'; return; }
    addShape(name);
    updateCatalogues();
    markPending();
  });
  byId('addSetupJoint').addEventListener('click', () => {
    const name = byId('setupJointName').value;
    if (!name) { status.textContent = 'Choose an articulated bone to add.'; return; }
    addJoint(name);
    updateCatalogues();
    markPending();
  });
  byId('addSetupMeasurement').addEventListener('click', () => {
    const name = byId('setupMeasurementName').value;
    if (!name) { status.textContent = 'Choose a tape functional to add.'; return; }
    addMeasurement(name);
    updateCatalogues();
    markPending();
  });
  byId('applySetup').addEventListener('click', () => {
    withdraw();
    try {
      const choices = readChoices();
      applied = choices;
      pending = false;
      byId('cancelSetup').disabled = true;
      publish();
      status.textContent = `${choices.frame_indices.length} frames and explicit model choices prepared. Download the choices and apply them with the native capture-init command.`;
    } catch (error) {
      pending = true;
      byId('cancelSetup').disabled = false;
      status.textContent = `Choices are incomplete or invalid: ${error.message}`;
    }
  });
  byId('cancelSetup').addEventListener('click', () => {
    pending = false;
    render(applied === null ? seed : applied);
    publish();
    status.textContent = applied === null ? 'Pending edits canceled; original setup restored.' : 'Pending edits canceled; the last applied choices are restored.';
  });
  byId('resetSetup').addEventListener('click', () => {
    applied = null;
    pending = false;
    render(seed);
    status.textContent = 'Reset to the original setup. No edited download remains.';
  });
  window.addEventListener('pagehide', withdraw);
  render(seed);
  byId('setupEditor').hidden = false;
  status.textContent = 'Choose frames, age, gender, evidence and explicit fitting controls. Original root choices and model defaults are shown; nothing is inferred from the photographs.';
})();
