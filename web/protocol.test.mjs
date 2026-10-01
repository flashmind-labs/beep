import test from 'node:test';
import assert from 'node:assert/strict';
import { readEvents, acceptEvent, uuid } from './protocol.mjs';

test('UTF-8 and NDJSON survive every byte boundary including a final line without newline', async () => {
  const events = [{ run_id: 'abc', seq: 1, type: 'text_delta', delta: 'Hello 👋 café 中文' }, { run_id: 'abc', seq: 2, type: 'run_finished', status: 'completed' }];
  const bytes = new TextEncoder().encode(events.map(e => JSON.stringify(e)).join('\n'));
  const stream = new ReadableStream({ start(controller) { for (const byte of bytes) controller.enqueue(new Uint8Array([byte])); controller.close(); } });
  const received = []; await readEvents(stream, event => received.push(event)); assert.deepEqual(received, events);
});
test('replayed events and events for other runs cannot duplicate text', () => {
  const pending = { runId: 'abc', after: 2 };
  assert.equal(acceptEvent(pending, { run_id: 'abc', seq: 2 }), false);
  assert.equal(acceptEvent(pending, { run_id: 'other', seq: 3 }), false);
  assert.equal(acceptEvent(pending, { run_id: 'abc', seq: 3 }), true);
  assert.equal(pending.after, 3);
  assert.equal(acceptEvent(pending, { run_id: 'abc', seq: '4' }), false);
});
test('generated request identifiers are UUID v4 values even without crypto.randomUUID', () => {
  const a = uuid(); assert.match(a, /^[\da-f]{8}-[\da-f]{4}-4[\da-f]{3}-[89ab][\da-f]{3}-[\da-f]{12}$/); assert.notEqual(a, uuid());
});
