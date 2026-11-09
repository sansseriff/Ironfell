# Prompt topics: the agent's prompt in parts

**Built 2026-11-09 (step 7 of the current order).** The agent's system prompt
(`src-ui/model/manifest.ts`, generated from the registry schema so the vocabulary told
and the vocabulary enforced cannot drift, doc 04 §9.7) had grown to one 26k-character
block sent with every request, schematics and tables and state machines included when
the request was "make it red".

## 1. Shape

- **The core** (about 10k characters): what every edit needs. The view format, node
  types and facets, the edit verbs, values, colours and tokens, expressions, models and
  compositions, and how to fix a rejected edit with `<revise>`.
- **Topics**, each with a one-line summary: `components` (definitions, instances,
  `make-component`, `place`, the standard components), `data` (series and data
  functions, curves from numbers, `std:plot`), `machines`, `ports`, `schematic`,
  `contexts`, `constraints`, `layout`, `tables`, `time`, `narratives`.
- **Which topics are sent:** those the document makes relevant (its view holds what the
  topic is about: a schematic sheet, a `<table>`, a `<machine>`) or the request names
  (its words, and the last few messages of the conversation); `topicsFor`.
- **The rest are listed** under MORE TOPICS with their summaries; the model may reply
  `<topic name="tables layout"/>` before it edits, and the loop answers with those
  topics' text and asks again (not a repair attempt; two such asks per request), as it
  answers `<look>`.
- With every topic included the prompt is exactly the old one (no line lost).

## 2. Tests

`tests/compose-edit.test.ts`: the core keeps the verbs and indexes every topic it leaves
out; an included topic shows and leaves the index; topics chosen by document and request;
`<topic>` parsed; a topic request answered with its text and not counted as an attempt.

## 3. Not yet

- Tuning the triggers against real agent runs (the agent log records each turn's
  `topic` asks: a topic often asked for should join the core or get a trigger).
- The schema's field lists (NODE TYPES, FACETS AND FIELDS) are most of the core; they
  could be cut to what the view shows, with the rest by topic.
