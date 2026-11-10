# Agent image input and composition capture

**Status:** Built 2026-11-09 on `t3/agent-image-vision`, started from `8cba756` and rebased onto committed `a3880ee`.

The Model and Lesson chats accept pasted, dropped, or selected PNG, JPEG, WebP,
and GIF files. The composer decodes and normalizes them to PNG, with a JPEG fallback for detailed photos (first frame for
animation), caps the long edge at 1568 px, and accepts up to four attachments.
Source files are limited to 20 MB; normalized base64 is limited to 2.8 million
characters per image. Invalid formats, including SVG, produce a visible error.
Thumbnails, removal, retry restoration and saved chat history carry the images.
Recent model history has a separate 4-million-character image budget, applied per
image, newest first; a message whose images were left out says so to the model.

Saved history (`chats.json`, rewritten whole on each save) keeps full image bytes
for the newest 8 million characters of images in a panel (`SAVED_IMAGE_BUDGET`).
Older images are saved as the ~192 px JPEG thumbnail made at attach time, marked
`preview`: shown in the transcript, never sent to a model or restored to the
composer. Validation checks each image's leading bytes against its declared type.

Images travel through the shared chat/model options in browser worker, browser
main-thread and native adapters. The Claude API receives image content blocks;
Claude Code receives stream-JSON image blocks; Codex receives temporary local
image files through app-server `localImage` inputs. Temporary runtime directories
are removed after completion or cancellation. The bridge validates attachment
shape, format, count and encoded size and bounds a completion request to 32 MB;
its other routes (`/log`, `/models`) keep the 1 MB limit.

The agent log never holds image bytes. A call record's message keeps
`{ name, mediaType, bytes, sha256, file }` for each image, and the image is
written once to `images/<sha256>.<ext>` in the log folder, so an agent reviewing
a log reads plain text and can open the picture it names.

`<look picture="composition name or #id"/>` asks the engine for a PNG. An edit
followed by this markup is applied first; the agent then receives the pixels and
the captured composition's current view (scoped to its id, not the whole document)
and can refine the edit. Turns allow at most three pictures (`MAX_PICTURES`) and
three edit/repair attempts; a picture request past the limit is answered once with
"no more pictures" rather than ending the turn. Each edit remains an ordinary
document transaction. Stopping does not roll back earlier successful edits; the
transcript says so, the prompt is not put back (sending it again would repeat
them), and the turn stays in the next request's history.

Capture is a temporary offscreen 2D view using the ordinary renderer and Bevy GPU
texture readback. It does not use the SVG exporter or the page canvas. It frames
content with 16 document-pixel padding, excludes editor overlays and highlights,
and flattens premultiplied pixels onto white. The framing is recomputed each frame
and the pixels are read once (`ReadbackOnce`) after the bounds have held still for
three frames. The view is removed on completion, cancellation, document switch,
deletion of the composition, or expiry. Polling has a 20-second timeout. A finished
result nobody polls is dropped after 30 s, and only pending captures count against
the limit of four, so abandoned tickets never block capture. A hidden tab may not
render frames; the capture then times out and the model is told it failed. The
first slice supports 2D compositions, including those not open in a panel; 3D
requests return an explicit unsupported error.

## Verification

- Unit tests: attachment validation, real bytes delivered to both runtime subprocess
  fixtures, edit/capture/refine ordering, capture failures and bounded requests.
- `agent-vision.ts`: unopened composition GPU pixel assertions, unchanged document
  serialization, attachment through the bridge, transcript thumbnail; run in both
  browser runtimes/backends and at DPR 1 and 2.
- Full run-iron smoke and type checks before committing.
- Review fixes (2026-11-09): agent-log image references, saved-history image
  budget and previews, per-image model history, applied-edit restore/history,
  scoped refinement view, picture limit, `/log` body limit, image signatures;
  Rust `encode()` padding/flattening and abandoned-ticket expiry unit tests.

## Follow-ups

3D camera capture and configurable capture framing/background are separate work.

## Verification results (2026-11-09)

Both wasm backends build. Type checks pass with zero errors (10 pre-existing
Svelte warnings); 106 unit tests pass. GPU captures and chat image delivery pass
on WebGPU and WebGL2, in worker and main-thread browser runtimes, including DPR
2. The fast smoke, regression and gestures suites pass with no page errors.
Runtime subprocess tests use fixtures and verify actual image bytes; live model
inference and a native package rebuild were not run.
