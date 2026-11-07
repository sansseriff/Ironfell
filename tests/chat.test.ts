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
