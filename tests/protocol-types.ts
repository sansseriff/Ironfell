import type { DocumentCommand, EngineRequest } from '../src-ui/runtime/protocol.generated';
const valid: DocumentCommand = { op: 'table_edit', table: '#1', verb: 'merge', cells: '["#2"]' };
// @ts-expect-error Unknown operation must fail before reaching a host.
const unknown: DocumentCommand = { op: 'missing' };
// @ts-expect-error Named command arguments must have the engine's types.
const wrong: DocumentCommand = { op: 'next_ids', n: 'two' };
// @ts-expect-error A text event needs both selection offsets.
const incomplete: EngineRequest = { ty: 'textInput', text: 'hello' };
void [valid, unknown, wrong, incomplete];

import type { UiEvent, CommandReply } from '../src-ui/runtime/protocol';
// @ts-expect-error Text edit notifications require the selection and caret.
const badEvent: UiEvent = { ty: 'textEdit', active: true, node: '#1', text: 'hello' };
// @ts-expect-error A successful serialized reply must carry its result.
const missingResult: CommandReply = { ty: 'documentView', requestId: 1, ok: true };
// @ts-expect-error Applied/complete do not promise pixels have been presented.
const badAck: CommandReply = { ty: 'documentCall', requestId: 1, text: '{}', ack: 'rendered' };
void [badEvent, missingResult, badAck];
