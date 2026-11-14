#!/usr/bin/env bun
import { readFile } from 'node:fs/promises';
import { createInterface } from 'node:readline';

const mode = process.env.IRON_TEST_RUNTIME_MODE;
const reply = (message: object) => process.stdout.write(`${JSON.stringify(message)}\n`);
if (process.argv.includes('app-server')) {
  let turns = 0;
  let threads = 0;
  let conversation = '';
  createInterface({ input: process.stdin }).on('line', async line => {
    const message = JSON.parse(line);
    if (!message.id) return;
    if (mode === 'exit') process.exit(2);
    if (mode === 'malformed') { process.stdout.write('not json\n'); return; }
    if (message.method === 'initialize') reply({ id: message.id, result: {} });
    else if (message.method === 'model/list') reply({ id: message.id, result: { data: [
      { id: 'fixture-default', model: 'fixture-default', displayName: 'Fixture Default', isDefault: true, hidden: false, supportedReasoningEfforts: [{ reasoningEffort: 'low', description: '' }, { reasoningEffort: 'high', description: '' }, { reasoningEffort: 'ultra', description: '' }], serviceTiers: [{ id: 'priority', name: 'Fast', description: '1.5x speed' }] },
      { id: 'fixture-other', model: 'fixture-other', displayName: 'Fixture Other', isDefault: false, hidden: false },
      { id: 'fixture-hidden', model: 'fixture-hidden', displayName: 'Hidden', isDefault: false, hidden: true },
    ] } });
    else if (message.method === 'thread/start') {
      if (message.params.sandbox !== 'read-only' || message.params.approvalPolicy !== 'never' || !message.params.ephemeral) process.exit(3);
      threads++;
      reply({ id: message.id, result: { thread: { id: 'fixture-thread' } } });
    } else if (message.method === 'turn/start') {
      reply({ id: message.id, result: { turn: { id: 'fixture-turn' } } });
      if (mode === 'approval') { reply({ id: 99, method: 'item/commandExecution/requestApproval', params: {} }); return; }
      if (mode === 'timeout') return;
      // What this turn sent, and the thread's whole conversation, which a
      // real model remembers: the replies below read the conversation.
      const sent = message.params.input.filter((i: any) => i.type === 'text').map((i: any) => i.text).join('\n\n');
      conversation += `${conversation ? '\n\n' : ''}${sent}`;
      const input = conversation;
      const target = input.match(/TARGET (#\w+)/)?.[1] ?? '#1';
      const shape = input.match(/SHAPE (#\w+)/)?.[1] ?? '#fixture';
      let text = input.includes('edit-test-shape')
        ? `<edit><set id="${shape}" x="240" fill.color="#22aa55"/></edit>`
        : `<edit><rect in="${target}" id="$shape" name="agent-test-shape" x="100" y="120" w="80" h="60" color="#ff2200"/></edit>`;
      if (input.includes('chat-test-prose')) text = '## A shared conversation\n\nA **rectangle** is a 2D shape.\n\n| Property | Meaning |\n| --- | --- |\n| width | horizontal size |\n\n```ts\nconst safe = true;\n```\n\n::view{show=main}\n\n<script>window.chatInjected = true</script>\n\n[unsafe](javascript:alert(1))';
      if (input.includes('image-wire-test')) {
        const image = message.params.input.find((i: any) => i.type === 'localImage');
        const bytes = image ? await readFile(image.path) : Buffer.alloc(0);
        text = bytes.subarray(0, 8).toString('hex') === '89504e470d0a1a0a' ? 'image received' : 'missing image';
      }
      turns++;
      if (input.includes('session-test')) text = `thread ${threads} turn ${turns} input ${JSON.stringify(sent)}`;
      if (input.includes('effort-test')) text = `effort=${message.params.effort ?? 'none'} tier=${message.params.serviceTier ?? 'none'}`;
      if (input.includes('chat-test-follow-up')) text = input.includes('A **rectangle** is a 2D shape.') ? 'The previous reply described a rectangle.' : 'ERROR: missing history';
      if (input.includes('chat-test-lesson')) text = '### Lesson companion\n\nWe can discuss the lesson without editing it.\n\n::view{show=main}';
      if (input.includes('chat-test-slow')) {
        await new Promise(resolve => setTimeout(resolve, 500));
        text = '### More detail\n\n' + Array.from({ length: 18 }, (_, index) => `Paragraph ${index + 1}: a longer explanation that lets the reader inspect earlier conversation without being pulled back to the end.`).join('\n\n');
      }
      // Components pictured in two compositions (.claude/skills/run-iron/agent-components.ts).
      if (input.includes('component-test-place')) {
        const [a, b] = (input.match(/SHAPES (#\w+) (#\w+)/) ?? []).slice(1);
        const [bench, overview] = (input.match(/IN (#\w+) (#\w+)/) ?? []).slice(1);
        text = `Making a pump and drawing it in both.\n<edit><make-component of="${a} ${b}" name="pump" as="$pump"/><place def="$pump" in="${bench}" x="300" y="80" name="P1" as="$p1"/><place entity="$p1" in="${overview}" x="40" y="40"/><place def="std:laser" in="${bench}" x="40" y="200" power="5 mW" name="L1"/></edit>`;
      }
      if (input.includes('component-test-look')) {
        const use = input.match(/USE (#\w+)/)?.[1];
        const entity = input.match(/ENTITY (#\w+)/)?.[1];
        const look = input.split('\nLOOK\n')[1];
        text = !look ? `<look expand="${use}"/>`
          : /derived/.test(look) && look.includes('name="dot"') ? `<edit><set id="${entity}" name="P1 seen"/></edit>` : 'ERROR: the look showed no derived picture';
      }
      // A slow, streamed turn for the chat's live progress (chat-ui.ts): reasoning,
      // prose, then an edit verb by verb; `chat-test-stop` goes quiet after the
      // reasoning so the test can press Stop. Only this request, not the history.
      const request = input.split('\nREQUEST\n').pop() ?? '';
      if (request.includes('chat-test-stream') || request.includes('chat-test-stop')) {
        const pause = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));
        const main = input.match(/<composition id="(#[^"]+)" name="main"/)?.[1] ?? '#1';
        for (const piece of ['Sizing ', 'the pump ', 'and its ', 'two ports.']) {
          reply({ method: 'item/reasoning/summaryTextDelta', params: { itemId: 'reasoning', summaryIndex: 0, delta: piece } });
          await pause(150);
        }
        if (request.includes('chat-test-stop')) await pause(4000);
        const streamed = `Here is a **pump** with two ports.\n<edit><rect in="${main}" id="$a" name="pump body" x="40" y="40" w="80" h="50" color="#3366cc"/><rect in="${main}" id="$b" name="inlet" x="20" y="55" w="20" h="20" color="#22aa55"/><rect in="${main}" id="$c" name="outlet" x="120" y="55" w="20" h="20" color="#cc3322"/></edit>`;
        for (const piece of streamed.match(/[\s\S]{1,24}/g) ?? []) {
          reply({ method: 'item/agentMessage/delta', params: { itemId: 'answer', delta: piece } });
          await pause(70);
        }
        reply({ method: 'item/completed', params: { item: { id: 'answer', type: 'agentMessage', text: streamed } } });
        reply({ method: 'thread/tokenUsage/updated', params: { tokenUsage: { last: { outputTokens: 42, reasoningOutputTokens: 30 } } } });
        reply({ method: 'turn/completed', params: { turn: { status: 'completed' } } });
        return;
      }
      const failed = mode === 'failed' || input.trimEnd().endsWith('chat-test-failure');
      if (failed) await new Promise(resolve => setTimeout(resolve, 250));
      // Stream as the app-server does: a reasoning summary, then the reply in pieces.
      reply({ method: 'item/reasoning/summaryTextDelta', params: { itemId: 'reasoning', summaryIndex: 0, delta: 'Reading the request.' } });
      for (const piece of text.match(/[\s\S]{1,40}/g) ?? []) reply({ method: 'item/agentMessage/delta', params: { itemId: 'answer', delta: piece } });
      reply({ method: 'item/completed', params: { item: { id: 'answer', type: 'agentMessage', text } } });
      reply({ method: 'thread/tokenUsage/updated', params: { tokenUsage: { last: { outputTokens: 42, reasoningOutputTokens: 30 } } } });
      reply({ method: 'turn/completed', params: { turn: { status: failed ? 'failed' : 'completed', error: { message: 'fixture turn failed' } } } });
    }
  });
} else {
  if (!process.argv.includes('--tools') || process.argv[process.argv.indexOf('--tools') + 1] !== '') process.exit(3);
  if (!process.argv.includes('--strict-mcp-config')) process.exit(3);
  // A subscription runs in safe mode with no API key and nothing from a
  // surrounding Claude Code session; an API key runs bare.
  const bare = process.argv.includes('--bare'), safe = process.argv.includes('--safe-mode');
  if (bare === safe) process.exit(3);
  if (bare && !process.env.ANTHROPIC_API_KEY) process.exit(3);
  if (safe && (process.env.ANTHROPIC_API_KEY || process.env.CLAUDECODE || Object.keys(process.env).some(k => k.startsWith('CLAUDE_CODE_')))) process.exit(4);
  if (process.argv[process.argv.indexOf('--output-format') + 1] !== 'stream-json' || !process.argv.includes('--verbose')) process.exit(5);
  if (process.argv[process.argv.indexOf('--input-format') + 1] !== 'stream-json') process.exit(6);
  // One stream-json user message a line, each answered with a result, as
  // Claude Code does; the conversation so far is what it has been sent.
  let seen = '';
  let turns = 0;
  for await (const line of createInterface({ input: process.stdin })) {
    if (!line.trim()) continue;
    const envelope = JSON.parse(line);
    const content: any[] = envelope.message.content;
    const input = content.filter(i => i.type === 'text').map(i => i.text).join('\n\n');
    seen += `\n\n${input}`;
    turns++;
    if (input.includes('image-wire-test')) {
      const image = content.find(i => i.type === 'image');
      const bytes = Buffer.from(image?.source?.data ?? '', 'base64');
      reply({ type: 'result', result: bytes.subarray(0, 8).toString('hex') === '89504e470d0a1a0a' ? 'image received' : 'missing image' });
      continue;
    }
    reply({ type: 'system', subtype: 'init' });
    if (input.includes('effort-test')) {
      const effort = process.argv.includes('--effort') ? process.argv[process.argv.indexOf('--effort') + 1] : 'none';
      reply({ type: 'result', subtype: 'success', result: `effort=${effort}`, is_error: false, usage: { output_tokens: 1 } });
      continue;
    }
    // A session test: which message of the session this is, and whether the
    // first is still in its memory.
    if (input.includes('session-test')) {
      reply({ type: 'result', subtype: 'success', result: `turn ${turns} remembers ${seen.includes('session-test-first')}`, is_error: false, usage: { input_tokens: 2, cache_read_input_tokens: turns > 1 ? 1000 : 0, output_tokens: 3 } });
      continue;
    }
    reply({ type: 'stream_event', event: { type: 'content_block_start', index: 0, content_block: { type: 'thinking', thinking: '' } } });
    reply({ type: 'stream_event', event: { type: 'content_block_delta', index: 0, delta: { type: 'thinking_delta', thinking: 'Nothing to change.' } } });
    reply({ type: 'system', subtype: 'thinking_tokens', estimated_tokens: 50, estimated_tokens_delta: 50 });
    reply({ type: 'system', subtype: 'thinking_tokens', estimated_tokens: 120, estimated_tokens_delta: 70 });
    for (const piece of ['<edit', '/>']) reply({ type: 'stream_event', event: { type: 'content_block_delta', index: 1, delta: { type: 'text_delta', text: piece } } });
    reply({ type: 'result', subtype: 'success', result: '<edit/>', is_error: false, usage: { input_tokens: input.length, output_tokens: 130, output_tokens_details: { thinking_tokens: 125 } } });
  }
}
