#!/usr/bin/env bun
import { createInterface } from 'node:readline';

const mode = process.env.IRON_TEST_RUNTIME_MODE;
const reply = (message: object) => process.stdout.write(`${JSON.stringify(message)}\n`);
if (process.argv.includes('app-server')) {
  createInterface({ input: process.stdin }).on('line', async line => {
    const message = JSON.parse(line);
    if (!message.id) return;
    if (mode === 'exit') process.exit(2);
    if (mode === 'malformed') { process.stdout.write('not json\n'); return; }
    if (message.method === 'initialize') reply({ id: message.id, result: {} });
    else if (message.method === 'thread/start') {
      if (message.params.sandbox !== 'read-only' || message.params.approvalPolicy !== 'never' || !message.params.ephemeral) process.exit(3);
      reply({ id: message.id, result: { thread: { id: 'fixture-thread' } } });
    } else if (message.method === 'turn/start') {
      reply({ id: message.id, result: { turn: { id: 'fixture-turn' } } });
      if (mode === 'approval') { reply({ id: 99, method: 'item/commandExecution/requestApproval', params: {} }); return; }
      if (mode === 'timeout') return;
      const input = message.params.input[0].text;
      const target = input.match(/TARGET (#\w+)/)?.[1] ?? '#1';
      const shape = input.match(/SHAPE (#\w+)/)?.[1] ?? '#fixture';
      let text = input.includes('edit-test-shape')
        ? `<edit><set id="${shape}" x="240" fill.color="#22aa55"/></edit>`
        : `<edit><create id="$shape" type="rect" parent="${target}" name="agent-test-shape" x="100" y="120" w="80" h="60" color="#ff2200"/></edit>`;
      if (input.includes('chat-test-prose')) text = '## A shared conversation\n\nA **rectangle** is a 2D shape.\n\n| Property | Meaning |\n| --- | --- |\n| width | horizontal size |\n\n```ts\nconst safe = true;\n```\n\n::view{show=main}\n\n<script>window.chatInjected = true</script>\n\n[unsafe](javascript:alert(1))';
      if (input.includes('chat-test-follow-up')) text = input.includes('A **rectangle** is a 2D shape.') ? 'The previous reply described a rectangle.' : 'ERROR: missing history';
      if (input.includes('chat-test-lesson')) text = '### Lesson companion\n\nWe can discuss the lesson without editing it.\n\n::view{show=main}';
      if (input.includes('chat-test-slow')) {
        await new Promise(resolve => setTimeout(resolve, 500));
        text = '### More detail\n\n' + Array.from({ length: 18 }, (_, index) => `Paragraph ${index + 1}: a longer explanation that lets the reader inspect earlier conversation without being pulled back to the end.`).join('\n\n');
      }
      const failed = mode === 'failed' || input.trimEnd().endsWith('chat-test-failure');
      if (failed) await new Promise(resolve => setTimeout(resolve, 250));
      reply({ method: 'item/completed', params: { item: { id: 'answer', type: 'agentMessage', text } } });
      reply({ method: 'turn/completed', params: { turn: { status: failed ? 'failed' : 'completed', error: { message: 'fixture turn failed' } } } });
    }
  });
} else {
  let input = '';
  for await (const chunk of process.stdin) input += chunk;
  if (!process.argv.includes('--tools') || process.argv[process.argv.indexOf('--tools') + 1] !== '') process.exit(3);
  if (!process.argv.includes('--bare') || !process.argv.includes('--strict-mcp-config')) process.exit(3);
  reply({ result: '<edit/>', is_error: false, usage: { input_tokens: input.length } });
}
