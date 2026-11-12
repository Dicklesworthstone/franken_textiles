// Actual workshop worker with explicit WASM doubles: routing, source retention and invalidation.
// These checks do not execute the Rust/WASM engine or establish body/garment accuracy.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

function worker({ missingBinding = false } = {}) {
  const sent = [], calls = [], rounds = [], models = [];
  let refuse = false;
  const bytes = text => new TextEncoder().encode(text);
  const pattern = '{"meta":{},"pieces":[{"placement":{"part":"torso"}}]}';
  const round = report => {
    const files = {
      'pattern.json': pattern, 'measurements.json': '{"records":[]}', 'fit_spec.json': '{}',
      ...(report === undefined ? {} : { 'body_fit.json': report, 'draft_body.json': '{"provenance":"MODEL_FIT"}' }),
    };
    const result = {
      page: () => JSON.stringify({ status: 'NOT_READY', zip: 'round.zip',
        files: report === undefined ? [] : [{ name: 'body_fit.json' }, { name: 'draft_body.json' }] }),
      file: name => files[name] === undefined ? null : bytes(files[name]),
      zip: () => new Uint8Array([1, 2]), freed: false,
      free() { assert.equal(this.freed, false); this.freed = true; },
    };
    rounds.push(result); return result;
  };
  const bindings = {
    WebBody: class {
      constructor(input) { this.input = [...input]; this.freed = false; models.push(this); }
      free() { assert.equal(this.freed, false); this.freed = true; }
    },
    run(...args) { calls.push({ method: 'run', args }); return round(); },
    run_on_body(...args) {
      calls.push({ method: 'run_on_body', args });
      if (refuse) throw Error('accepted shape refused');
      return round(args[4]);
    },
    alteration_targets: () => '{}',
    inspect_body(...args) { calls.push({ method: 'inspect_body', args }); return '{}'; },
  };
  if (missingBinding) delete bindings.run_on_body;
  const source = fs.readFileSync(path.join(__dirname, '..', 'web', 'worker.js'), 'utf8');
  const send = new Function('self', 'postMessage', 'TextDecoder', 'WebAssembly', 'bindings',
    'let onmessage;\n' + source + '\nftx = bindings;\nreturn data => onmessage({ data });')(
    {}, message => sent.push(message), TextDecoder, WebAssembly, bindings);
  return {
    async send(data) { const count = sent.length; await send(data); return sent.length > count ? sent.at(-1) : null; },
    calls, rounds, models, reject() { refuse = true; },
  };
}
const accepted = '\n { "status": "ACCEPTED", "synthetic": true }\n';
const request = '{"program":"fez","body":{"mode":"file"}}';
const draft = (extra = {}) => ({ type: 'run', request: 2, req: request, draftOnBody: true,
  body: 1, bodyFit: accepted, bodyFitConsent: true, ...extra });
async function loaded(options) {
  const w = worker(options);
  assert.equal((await w.send({ type: 'body', request: 1, bytes: new Uint8Array([4, 5]) })).type, 'bodyReady');
  return w;
}

test('ordinary drafting retains the ordinary binding without loading a body', async () => {
  const w = worker();
  assert.equal((await w.send({ type: 'run', request: 2, req: request })).type, 'result');
  assert.deepEqual(w.calls.map(call => call.method), ['run']);
  assert.equal(w.calls[0].args[0], request);
  assert.equal(w.rounds[0].freed, true);
});

test('selected-body drafting forwards exact sources and returns downloadable evidence', async () => {
  const w = await loaded();
  const result = await w.send(draft());
  assert.equal(result.type, 'result');
  assert.equal(result.res.status, 'NOT_READY');
  assert.equal(result.bodyCheckable, true);
  assert.equal(result.foldable, true);
  assert.deepEqual(w.calls.map(call => call.method), ['run_on_body']);
  assert.equal(w.calls[0].args[0], w.models[0]);
  assert.equal(w.calls[0].args[1], request);
  assert.equal(w.calls[0].args[4], accepted);
  assert.equal(new TextDecoder().decode(result.blobs['body_fit.json']), accepted);
  assert.equal(new TextDecoder().decode(result.blobs['draft_body.json']), '{"provenance":"MODEL_FIT"}');
  assert.equal(w.rounds[0].freed, true);
});

test('missing model, stale model, absent report and missing association never fall back', async () => {
  const absent = worker();
  assert.equal((await absent.send(draft())).type, 'error');
  assert.equal(absent.calls.length, 0);
  const w = await loaded();
  for (const extra of [{ body: 0 }, { bodyFit: null }, { bodyFit: '' }, { bodyFitConsent: false }]) {
    assert.equal((await w.send(draft(extra))).type, 'error');
  }
  assert.equal(w.calls.length, 0);
  const stale = await loaded({ missingBinding: true });
  assert.match((await stale.send(draft())).message, /Rebuild web\/pkg/);
  assert.equal(stale.calls.length, 0);
});

test('a refused replacement withdraws the earlier garment and body-check source', async () => {
  const w = await loaded();
  assert.equal((await w.send(draft())).type, 'result');
  w.reject();
  assert.match((await w.send(draft({ request: 3 }))).message, /accepted shape refused/);
  assert.equal((await w.send({ type: 'inspectBody', request: 4, round: 2, body: 1 })).type, 'error');
  assert.equal((await w.send({ type: 'fold', request: 5, round: 2, body: 1 })).type, 'error');
  assert.deepEqual(w.calls.map(call => call.method), ['run_on_body', 'run_on_body']);
});

test('clearing the selected model invalidates subsequent body-derived drafting', async () => {
  const w = await loaded();
  await w.send({ type: 'clearBody' });
  assert.equal(w.models[0].freed, true);
  assert.equal((await w.send(draft())).type, 'error');
  assert.equal(w.calls.length, 0);
});
