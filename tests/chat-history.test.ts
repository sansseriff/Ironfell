import { test } from 'bun:test';
import assert from 'node:assert/strict';
import { MAX_THREADS, mergeView, newThread, parseChats, savedView, threadTitle, type ChatThread } from '../src-ui/chat/history';
import { MemoryScopeStorage } from '../src-ui/runtime/scope_storage';
import type { ChatTurn } from '../src-ui/chat/conversation';

const turn = (id: string, instruction: string, status: ChatTurn['status'] = 'completed'): ChatTurn => ({ id, instruction, target: 'Document', status, startedAt: 1 });
const thread = (id: string, turns: ChatTurn[], updated = 1, draft = ''): ChatThread => ({ ...newThread(updated), id, turns, draft });

test('a chat is named by its first request, shortened', () => {
  assert.equal(threadTitle({ turns: [] }), 'New chat');
  assert.equal(threadTitle({ turns: [turn('a', '  Add a pump\nwith two ports')] }), 'Add a pump');
  const long = threadTitle({ turns: [turn('a', 'x'.repeat(100))] });
  assert.ok(long.length <= 58 && long.endsWith('…'));
});

test('saving keeps finished turns and their annotations, started chats, newest first, at most MAX_THREADS', () => {
  const running = thread('r', [turn('t1', 'done'), turn('t2', 'still going', 'running')], 5);
  running.annotations = { t1: { kept: true }, t2: { pending: true } };
  const saved = savedView([thread('empty', [], 9), running, thread('old', [turn('t3', 'older')], 2)], 'r');
  assert.deepEqual(saved.threads.map(t => t.id), ['r', 'old']);
  assert.deepEqual(saved.threads[0].turns.map(t => t.id), ['t1']);
  assert.deepEqual(Object.keys(saved.threads[0].annotations), ['t1']);
  assert.equal(saved.current, 'r');
  // An unsent draft keeps an otherwise empty chat; the current one is always kept.
  assert.deepEqual(savedView([thread('d', [], 3, 'half a thought'), thread('cur', [], 1)], 'cur').threads.map(t => t.id), ['d', 'cur']);
  const many = Array.from({ length: MAX_THREADS + 5 }, (_, i) => thread(`t${i}`, [turn(`x${i}`, 'q')], i));
  const capped = savedView(many, 't0');
  assert.equal(capped.threads.length, MAX_THREADS);
  assert.equal(capped.threads[0].id, `t${MAX_THREADS + 4}`);
  assert.equal(capped.current, capped.threads[0].id, 'a current chat dropped by the cap falls back to the newest');
});

test('chats.json round-trips, and anything unreadable is an empty history', () => {
  const views = { 'model:main': savedView([thread('a', [turn('t', 'hello')], 4)], 'a') };
  const parsed = parseChats(JSON.stringify({ version: 1, views }));
  assert.deepEqual(parsed.views['model:main'].threads.map(t => t.id), ['a']);
  assert.equal(parsed.views['model:main'].threads[0].turns[0].instruction, 'hello');
  for (const bad of [null, '', 'not json', '{"version":2,"views":{}}', '{"version":1,"views":{"x":{"threads":"no"}}}']) {
    assert.deepEqual(Object.keys(parseChats(bad).views).filter(k => parseChats(bad).views[k].threads.length), []);
  }
  // A turn saved while running (an older file) is dropped.
  const withRunning = parseChats(JSON.stringify({ version: 1, views: { v: { current: 'a', threads: [{ id: 'a', turns: [turn('x', 'q', 'running'), turn('y', 'r')] }] } } }));
  assert.deepEqual(withRunning.views.v.threads[0].turns.map(t => t.id), ['y']);
});

test('the saved history replaces an untouched panel, but a chat already under way stays current', () => {
  const saved = { current: 's2', threads: [thread('s1', [turn('a', 'one')], 2), thread('s2', [turn('b', 'two')], 3)] };
  const fresh = thread('fresh', [], 9);
  assert.deepEqual(mergeView([fresh], 'fresh', saved), saved);
  const typing = thread('typing', [], 9, 'a draft');
  const merged = mergeView([typing], 'typing', saved);
  assert.equal(merged.current, 'typing');
  assert.deepEqual(merged.threads.map(t => t.id), ['typing', 's1', 's2']);
  assert.deepEqual(mergeView([fresh], 'fresh', undefined), { current: 'fresh', threads: [fresh] });
});

test('scope storage keeps chats.json per scope, counted in its size', async () => {
  const storage = new MemoryScopeStorage();
  await storage.create({ id: 'S', name: 'S', created: 0, modified: 0 }, '{"version":1}', 1);
  assert.equal(await storage.readChats('S'), null);
  const before = await storage.size('S');
  await storage.writeChats('S', '{"version":1,"views":{}}');
  assert.equal(await storage.readChats('S'), '{"version":1,"views":{}}');
  assert.ok(await storage.size('S') > before);
  await storage.writeChats('missing', 'x');
  assert.equal(await storage.readChats('missing'), null);
});

test('saved chat images and image-only drafts round trip, invalid attachments are dropped', () => {
  const image = { name: 'picture.png', mediaType: 'image/png' as const, data: 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg==' };
  const chat = thread('images', [{ ...turn('image', 'Inspect this'), images: [image] }]);
  chat.draftImages = [image];
  const saved = savedView([chat], chat.id);
  const loaded = parseChats(JSON.stringify({ version: 1, views: { model: saved } })).views.model.threads[0];
  assert.deepEqual(loaded.turns[0].images, [image]);
  assert.deepEqual(loaded.draftImages, [image]);
  const draft = thread('draft', []); draft.draftImages = [image];
  assert.equal(mergeView([draft], draft.id, saved).current, draft.id);
  const malformed = JSON.stringify({ version: 1, views: { model: saved } }).replaceAll('image/png', 'image/svg+xml');
  assert.deepEqual(parseChats(malformed).views.model.threads[0].turns[0].images, []);
  // A declared type the bytes do not match is dropped too.
  const mislabelled = JSON.stringify({ version: 1, views: { model: saved } }).replaceAll('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg==', 'AAAA');
  assert.deepEqual(parseChats(mislabelled).views.model.threads[0].turns[0].images, []);
});

test('saved history keeps full images only within its budget; older ones keep their thumbnail', () => {
  const thumb = '/9j/4AAQSkZJRgABAQAAAQABAAD/';
  const big = (name: string) => ({ name, mediaType: 'image/png' as const, data: 'iVBORw0KGgoAAAAN' + 'A'.repeat(2_800_000 - 16), thumb });
  // Newest first (2.8 M characters each): b2 and b1 fit in the 8 M budget, a1 and older do not.
  const older = thread('older', [{ ...turn('o', 'old'), images: [big('o1'), { ...big('o2'), thumb: undefined }] }]);
  older.updated = 1;
  const newer = thread('newer', [{ ...turn('a', 'first'), images: [big('a1')] }, { ...turn('b', 'second'), images: [big('b1'), big('b2')] }]);
  newer.updated = 2;
  const saved = savedView([older, newer], newer.id);
  const [n, o] = saved.threads;
  assert.deepEqual(n.turns[1].images!.map(i => [i.name, !!i.preview]), [['b1', false], ['b2', false]]);
  assert.deepEqual(n.turns[0].images!.map(i => [i.name, !!i.preview]), [['a1', true]]);
  assert.equal(n.turns[0].images![0].data, thumb);
  // Over budget with no thumbnail: dropped, not kept whole.
  assert.deepEqual(o.turns[0].images!.map(i => i.name), ['o1']);
  assert.ok(JSON.stringify(saved).length < 8_000_000 + 100_000);
  // Previews survive a reload as previews.
  const loaded = parseChats(JSON.stringify({ version: 1, views: { model: saved } })).views.model.threads[0];
  assert.equal(loaded.turns[0].images![0].preview, true);
});
