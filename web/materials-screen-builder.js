// Author catalog discovery requests; Rust owns observations, units, capability admission and ranking.
export const SCREEN_FORMS = Object.freeze([
  "sheet", "fiber", "yarn", "woven", "knit", "nonwoven", "hide", "fill", "laminate",
]);
export const SCREEN_CAPABILITIES = Object.freeze(["mechanics", "steady_conduction", "transient_thermal"]);
const MAX_REQUEST_BYTES = 16 * 1024 * 1024;
const own = (value, key) => Object.hasOwn(value, key);

function object(value, allowed, required, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw Error(`${label} must be an object.`);
  if (Object.keys(value).some(key => !allowed.includes(key)) || required.some(key => !own(value, key))) {
    throw Error(`${label} has missing or unsupported fields.`);
  }
}
function bytes(value) {
  let count = 0;
  for (let i = 0; i < value.length; i++) {
    const c = value.charCodeAt(i);
    if (c < 0x80) count++;
    else if (c < 0x800) count += 2;
    else if (c >= 0xd800 && c <= 0xdbff && value.charCodeAt(i + 1) >= 0xdc00 && value.charCodeAt(i + 1) <= 0xdfff) {
      count += 4; i++;
    } else count += 3;
    if (count > MAX_REQUEST_BYTES) throw Error("Guided screening request exceeds 16 MiB.");
  }
  return count;
}
function text(value, label, empty = false) {
  if (typeof value !== "string" || (!empty && !value.trim())) {
    throw Error(`${label} must be ${empty ? "text" : "nonempty text"}.`);
  }
  bytes(value);
  return value;
}
function choices(value, allowed, label) {
  if (!Array.isArray(value) || value.length > allowed.length) throw Error(`${label} must be a bounded list of supported values.`);
  const seen = new Set();
  for (const item of value) {
    if (!allowed.includes(item) || seen.has(item)) throw Error(`${label} must contain unique supported values.`);
    seen.add(item);
  }
  return value;
}
function consent(state, key) {
  if (!own(state, key)) return false;
  if (typeof state[key] !== "boolean") throw Error(`Screening ${key} consent must be a boolean.`);
  return state[key];
}
function conditions(raw) {
  text(raw, "Attribute conditions original JSON");
  let value;
  try { value = JSON.parse(raw); }
  catch { throw Error("Attribute conditions must retain valid original JSON text."); }
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw Error("Attribute conditions must be an original JSON object of exact string qualifiers.");
  }
  for (const [key, qualifier] of Object.entries(value)) {
    text(key, "Condition name"); text(qualifier, "Condition value");
  }
  return raw;
}

/**
 * Author an ftx-material-screen/1 discovery request over the built-in catalog.
 * `attributes` contains unique {key, unit, method?, conditions?, minimum?, maximum?, direction?}
 * rows. Conditions retain original JSON text with exact string values; all other strings are
 * authored identities, without trimming, unit conversion or inferred method equivalence.
 * Each row supplies an inclusive bound, an objective, or both. Empty rows are not silently dropped.
 * An empty attributes list supports plain discovery or independently requested compiler readiness.
 * Catalog estimate and mixed-protocol consent default to false; explicit values must be booleans.
 * This function does not resolve sources, create material values or infer a garment performance score.
 */
export function buildMaterialScreenRequest(state) {
  object(state, ["query", "forms", "basis", "allowCatalogEstimates", "allowMixedProtocols", "requiredCapabilities", "attributes"],
    ["basis", "attributes"], "Guided screening");
  if (!["nominal", "reported_bounds"].includes(state.basis)) throw Error("Choose nominal or reported-bound screening explicitly.");
  const query = own(state, "query") ? text(state.query, "Catalog query", true) : "";
  const forms = choices(own(state, "forms") ? state.forms : [], SCREEN_FORMS, "Material forms");
  const capabilities = choices(own(state, "requiredCapabilities") ? state.requiredCapabilities : [], SCREEN_CAPABILITIES, "Required capabilities");
  const estimates = consent(state, "allowCatalogEstimates"), mixed = consent(state, "allowMixedProtocols");
  if (!Array.isArray(state.attributes) || state.attributes.length > 80) {
    throw Error("Screening allows at most 64 constraints and 16 objectives across 80 attribute rows.");
  }
  const seen = new Set(), constraints = [], objectives = [];
  for (const row of state.attributes) {
    object(row, ["key", "unit", "method", "conditions", "minimum", "maximum", "direction"], ["key", "unit"], "Screening attribute");
    text(row.key, "Property key"); text(row.unit, "Property unit");
    if (seen.has(row.key)) throw Error("Use one row per property key; combine its limits and objective in that row.");
    seen.add(row.key);
    const bounds = ["minimum", "maximum"].filter(key => own(row, key));
    if (bounds.some(key => typeof row[key] !== "number" || !Number.isFinite(row[key])) ||
      (bounds.length === 2 && row.minimum > row.maximum)) {
      throw Error("Attribute bounds must be finite numbers in an ordered inclusive interval.");
    }
    const objective = own(row, "direction");
    if (objective && !["minimize", "maximize"].includes(row.direction)) throw Error("Choose minimize or maximize for an attribute objective.");
    if (!bounds.length && !objective) throw Error("Each attribute row needs a bound or an objective.");
    if (bounds.length && constraints.length === 64) throw Error("Screening allows at most 64 constraints.");
    if (objective && objectives.length === 16) throw Error("Screening allows at most 16 objectives.");
    const attribute = ['{"key":', JSON.stringify(row.key), ',"unit":', JSON.stringify(row.unit),
      ...(own(row, "method") ? [',"method":', JSON.stringify(text(row.method, "Attribute method"))] : []),
      ...(own(row, "conditions") ? [',"conditions":', conditions(row.conditions)] : []), '}'].join("");
    if (bounds.length) constraints.push(['{"attribute":', attribute,
      ...bounds.flatMap(key => [`,"${key}":`, JSON.stringify(row[key])]), '}']);
    if (objective) objectives.push(['{"attribute":', attribute, ',"direction":', JSON.stringify(row.direction), '}']);
  }
  const parts = []; let size = 0;
  const append = value => {
    size += bytes(value);
    if (size > MAX_REQUEST_BYTES) throw Error("Guided screening request exceeds 16 MiB.");
    parts.push(value);
  };
  append('{"schema":"ftx-material-screen/1","basis":'); append(JSON.stringify(state.basis));
  append(',"query":'); append(JSON.stringify(query));
  append(',"forms":'); append(JSON.stringify(forms));
  append(`,"allow_catalog_estimates":${estimates},"allow_mixed_protocols":${mixed},"required_capabilities":`);
  append(JSON.stringify(capabilities));
  for (const [key, rows] of [["constraints", constraints], ["objectives", objectives]]) {
    append(`,"${key}":[`);
    rows.forEach((row, index) => { if (index) append(","); for (const part of row) append(part); });
    append("]");
  }
  append("}");
  return parts.join("");
}
