# Agent image input and composition capture

**Status:** Built 2026-11-09 on `t3/agent-image-vision`, started from `8cba756` and rebased onto committed `a3880ee`.

The Model and Lesson chats accept pasted, dropped, or selected PNG, JPEG, WebP,
and GIF files. The composer decodes and normalizes them to PNG, with a JPEG fallback for detailed photos (first frame for
animation), caps the long edge at 1568 px, and accepts up to four attachments.
Source files are limited to 20 MB; normalized base64 is limited to 2.8 million
characters per image. Invalid formats, including SVG, produce a visible error.
Thumbnails, removal, retry restoration and saved chat history carry the images.
Recent model history has a separate 4-million-character image budget.

Images travel through the shared chat/model options in browser worker, browser
main-thread and native adapters. The Claude API receives image content blocks;
Claude Code receives stream-JSON image blocks; Codex receives temporary local
image files through app-server `localImage` inputs. Temporary runtime directories
are removed after completion or cancellation. The bridge validates attachment
shape, format, count and encoded size and bounds the full request to 32 MB.

`<look picture="composition name or #id"/>` asks the engine for a PNG. An edit
followed by this markup is applied first; the agent then receives the pixels and
current document data and can refine the edit. Turns allow at most three pictures
and three edit/repair attempts. Each edit remains an ordinary document transaction.
Stopping does not roll back earlier successful edits; the transcript says so.

Capture is a temporary offscreen 2D view using the ordinary renderer and Bevy GPU
texture readback. It does not use the SVG exporter or the page canvas. It frames
content with 16 document-pixel padding, excludes editor overlays and highlights,
and flattens premultiplied pixels onto white. The view is removed on completion,
cancellation, document switch or expiry. Polling has a 20-second timeout. The
first slice supports 2D compositions, including those not open in a panel; 3D
requests return an explicit unsupported error.

## Verification

- Unit tests: attachment validation, real bytes delivered to both runtime subprocess
  fixtures, edit/capture/refine ordering, capture failures and bounded requests.
- `agent-vision.ts`: unopened composition GPU pixel assertions, unchanged document
  serialization, attachment through the bridge, transcript thumbnail; run in both
  browser runtimes/backends and at DPR 1 and 2.
- Full run-iron smoke and type checks before committing.

## Follow-ups

3D camera capture and configurable capture framing/background are separate work.

## Verification results (2026-11-09)

Both wasm backends build. Type checks pass with zero errors (10 pre-existing
Svelte warnings); 106 unit tests pass. GPU captures and chat image delivery pass
on WebGPU and WebGL2, in worker and main-thread browser runtimes, including DPR
2. The fast smoke, regression and gestures suites pass with no page errors.
Runtime subprocess tests use fixtures and verify actual image bytes; live model
inference and a native package rebuild were not run.
