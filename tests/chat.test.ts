import { test } from 'node:test';
import assert from 'node:assert/strict';
import { contentParts, failureSummary, safeChatUrl, chatHistory, shouldSubmit, narrativeMode, nearBottom, type ChatTurn } from '../src-ui/chat/conversation';
import { boundedHistory } from '../src-ui/model/conversation';

test('content parts preserve prose and ordered composition visuals', () => {
  const parts = contentParts('## Explanation\n\nThe **beam** is here.\n\n::view{show="optics" focus=#4}\n\nMore prose.\n\n::view{show=main}\n');
  assert.deepEqual(parts.map(part => part.kind), ['markdown', 'view', 'markdown', 'view']);
  assert.deepEqual(parts[1], { kind: 'view', show: 'optics', focus: '#4' });
  assert.deepEqual(parts[3], { kind: 'view', show: 'main', focus: '' });
});

test('visual-looking code and malformed directives are never executed as cards', () => {
  for (const text of ['```text\n::view{show=secret}\n```', '::view{show=main onclick=evil}', '::view{show="unterminated}', '<script>window.evil=true</script>']) {
    const parts = contentParts(text);
    assert.equal(parts.length, 1);
    assert.equal(parts[0].kind, 'markdown');
  }
});

test('links/images reject executable, local, data, relative and credentialed addresses', () => {
  for (const address of ['javascript:alert(1)', 'data:image/svg+xml,<svg/>', 'file:///etc/passwd', '/relative', 'https://user:password@example.com', 'javascript&#58;alert(1)']) assert.equal(safeChatUrl(address), null);
  assert.equal(safeChatUrl('https://example.com/a'), 'https://example.com/a');
  assert.equal(safeChatUrl('mailto:reader@example.com'), 'mailto:reader@example.com');
  assert.equal(safeChatUrl('http://example.com/image.png', true), null);
  assert.equal(safeChatUrl('https://example.com/image.png', true), 'https://example.com/image.png');
});

test('conversation history excludes failures, preserves edit context and is bounded', () => {
  const good: ChatTurn = { id: 'one', instruction: 'make a rectangle', target: 'document', status: 'completed', startedAt: 0, result: { ok: true, note: 'Created it.', edit: '<edit/>', attempts: 1, outcome: null, transport: 'fixture' } };
  assert.deepEqual(chatHistory([good, { ...good, id: 'failed', status: 'failed' }, { ...good, id: 'pending', status: 'running' }]), [{ role: 'user', content: 'make a rectangle' }, { role: 'assistant', content: 'Created it.\n\n<edit/>' }]);
  const history = boundedHistory(Array.from({ length: 30 }, () => ({ role: 'user' as const, content: 'a'.repeat(20000) })));
  assert.ok(history.length <= 12);
  assert.ok(history.every(message => message.content.length <= 8000));
  assert.ok(history.reduce((sum, message) => sum + message.content.length, 0) <= 48000);
});

test('composer submit is modifier-based, IME safe, non-repeating, with newline modifiers preserved', () => {
  const key = { key: 'Enter', isComposing: false, repeat: false, shiftKey: false, altKey: false, metaKey: true, ctrlKey: false };
  assert.equal(shouldSubmit(key), true);
  assert.equal(shouldSubmit({ ...key, metaKey: false, ctrlKey: true }), true);
  for (const event of [{ isComposing: true }, { repeat: true }, { shiftKey: true }, { altKey: true }, { metaKey: false }, { key: 'a' }]) assert.equal(shouldSubmit({ ...key, ...event }), false);
});

test('layout modes validate stored values and scroll follow uses the reader position', () => {
  assert.equal(narrativeMode('exploration'), 'exploration');
  assert.equal(narrativeMode('hybrid'), 'hybrid');
  assert.equal(narrativeMode('corrupt-local-storage'), 'authoring');
  assert.equal(nearBottom(100, 500, 400), true);
  assert.equal(nearBottom(0, 1000, 400), false);
});

test('a rejected edit reads as a sentence, not markup', () => {
  const failure = '<edit-failed applied="none" version="14">\n  <error kind="parse">instance.def: not a valid id: &quot;#$def&quot; at line 1</error>\n</edit-failed>';
  assert.equal(failureSummary(failure, 2), 'The edit was rejected after 2 attempts, so nothing changed: instance.def: not a valid id: "#$def" at line 1');
  assert.equal(failureSummary('plain words'), 'plain words');
});

test('a streaming reply shows prose, holds back half-written markup and lists closed verbs', async () => {
  const { streamView } = await import('../src-ui/chat/conversation');
  assert.deepEqual(streamView('Making a pump.\n<ed'), { prose: 'Making a pump.\n', edit: null, verbs: [] });
  assert.equal(streamView('See this:\n\n::vie').prose, 'See this:\n\n');
  const forming = streamView('Making it.\n<edit><create id="$a" type="rect" name="pump body" x="1"/><set id="#4" x="2" fill.color="#f00"/><import-svg to="#1"><svg><g><path d="M0 0"/></g></svg></import-svg><create id="$b" type="te');
  assert.equal(forming.prose, 'Making it.\n');
  assert.deepEqual(forming.verbs, ['Create rect "pump body"', 'Set #4 · x, fill.color', 'Import SVG']);
  assert.match(forming.edit!, /^<edit>/);
  const closed = streamView('<edit><delete id="#3"/></edit>\nDone.');
  assert.deepEqual([closed.prose, closed.verbs], ['\nDone.', ['Delete #3']]);
  assert.equal(streamView('Like this:\n```xml\n<edit><delete id="#3"/></edit>\n```\n').edit, null);
  assert.deepEqual(streamView('<look expand="#u"/>'), { prose: '', edit: null, verbs: [] });
});

test('progress folds phases, reasoning and text into labelled steps', async () => {
  const { applyAgentEvents, finishProgress, stepLabel } = await import('../src-ui/chat/conversation');
  const progress = { steps: [], text: '' } as any;
  applyAgentEvents(progress, [{ kind: 'phase', phase: 'reading', attempt: 1 }, { kind: 'phase', phase: 'thinking', attempt: 1 }, { kind: 'reasoning', delta: 'Plan: ' }, { kind: 'reasoning', delta: 'a pump.' }], 10);
  assert.equal(stepLabel(progress.steps[1], false), 'Thinking');
  applyAgentEvents(progress, [{ kind: 'text', delta: 'Sure.\n<edit><create type="rect"/>' }], 20);
  assert.equal(stepLabel(progress.steps[1], false), 'Writing the edit');
  assert.equal(progress.steps[1].reasoning, 'Plan: a pump.');
  applyAgentEvents(progress, [{ kind: 'phase', phase: 'applying', attempt: 1 }, { kind: 'phase', phase: 'thinking', attempt: 2 }], 30);
  assert.equal(progress.text, '');
  assert.equal(stepLabel(progress.steps[3], false), 'Reconsidering the rejected edit');
  finishProgress(progress, 40);
  assert.deepEqual(progress.steps.map((s: any) => [s.startedAt, s.endedAt]), [[10, 10], [10, 30], [30, 30], [30, 40]]);
  assert.equal(stepLabel(progress.steps[1], true), 'Wrote the edit');
});

test('progress batches merge deltas and send a phase change at once', async () => {
  const { eventBatcher } = await import('../src-ui/model/progress');
  const sent: unknown[][] = [];
  const batch = eventBatcher(events => sent.push(events), 20);
  batch.push({ kind: 'text', delta: 'a' });
  batch.push({ kind: 'text', delta: 'b' });
  batch.push({ kind: 'reasoning', delta: 'r' });
  assert.equal(sent.length, 0);
  batch.push({ kind: 'phase', phase: 'applying', attempt: 1 });
  assert.deepEqual(sent, [[{ kind: 'text', delta: 'ab' }, { kind: 'reasoning', delta: 'r' }, { kind: 'phase', phase: 'applying', attempt: 1 }]]);
  batch.push({ kind: 'text', delta: 'c' });
  await new Promise(resolve => setTimeout(resolve, 40));
  assert.deepEqual(sent[1], [{ kind: 'text', delta: 'c' }]);
});

test('token counts: estimates while running, exact once each call reports, with a rate', async () => {
  const { applyAgentEvents, turnTokens, stepTokens, formatTokens } = await import('../src-ui/chat/conversation');
  const { mergeEvents } = await import('../src-ui/model/progress');
  assert.deepEqual(mergeEvents([{ kind: 'thinking-tokens', estimated: 50 }, { kind: 'thinking-tokens', estimated: 150 }]), [{ kind: 'thinking-tokens', estimated: 150 }]);
  const progress = { steps: [], text: '' } as any;
  applyAgentEvents(progress, [{ kind: 'phase', phase: 'thinking', attempt: 1 }], 0);
  applyAgentEvents(progress, [{ kind: 'thinking-tokens', estimated: 300 }], 1000);
  applyAgentEvents(progress, [{ kind: 'text', delta: 'x'.repeat(300) }], 2000);
  // 300 estimated thinking + 300 chars / 3 = 400, over 2 s since the first token.
  assert.deepEqual(turnTokens(progress, 3000, true), { tokens: 400, thinking: 300, exact: false, perSecond: 200 });
  applyAgentEvents(progress, [{ kind: 'usage', output: 420, thinking: 310 }, { kind: 'phase', phase: 'applying', attempt: 1 }], 4000);
  // 300 chars were 110 reply tokens: the next estimate uses about 2.9 characters a token.
  const next = applyAgentEvents({ steps: [], text: '' }, [{ kind: 'phase', phase: 'thinking', attempt: 1 }, { kind: 'text', delta: 'y'.repeat(290) }], 0);
  assert.equal(stepTokens(next.steps[0]).tokens, Math.ceil(290 / (0.5 * 3 + 0.5 * (300 / 110))));
  assert.deepEqual(stepTokens(progress.steps[0]), { tokens: 420, thinking: 310, exact: true });
  // Done: per second of model time (4 s).
  assert.deepEqual(turnTokens(progress, 5000, false), { tokens: 420, thinking: 310, exact: true, perSecond: 105 });
  // Codex shares nothing while it reasons: no count, no rate yet.
  const quiet = applyAgentEvents({ steps: [], text: '' }, [{ kind: 'phase', phase: 'thinking', attempt: 1 }], 0);
  assert.deepEqual(turnTokens(quiet, 5000, true), { tokens: 0, thinking: 0, exact: false, perSecond: null });
  assert.deepEqual([formatTokens(1234), formatTokens(12345)], ['1,234', '12.3k']);
});
