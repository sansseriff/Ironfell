/**
 * The narrative editor's ProseMirror schema (plans/specs/narrative-panel.md
 * §3, §7.1). Each top-level node is one `block` node in the store, its id in
 * the `id` attribute; inline marks are the block's `richtext` marks.
 *
 *   paragraph, heading(level), bullet, ordered, quote, code_block, view(show, focus)
 *   inline: text, value (an atom: a live value, its text derived)
 *   marks: mention(target), strong, em, code, link(href)
 */
import { Schema, type NodeSpec, type MarkSpec } from 'prosemirror-model';

const id = { id: { default: null as string | null } };

const block = (tag: string, extra: Partial<NodeSpec> = {}): NodeSpec => ({
  group: 'block',
  content: 'inline*',
  attrs: { ...id },
  parseDOM: [{ tag }],
  toDOM: () => [tag, 0],
  ...extra,
});

const nodes: Record<string, NodeSpec> = {
  doc: { content: 'block+' },
  paragraph: block('p'),
  heading: block('h1', {
    attrs: { ...id, level: { default: 1 } },
    parseDOM: [1, 2, 3, 4, 5, 6].map((level) => ({ tag: `h${level}`, attrs: { level } })),
    toDOM: (n) => [`h${n.attrs.level}`, 0],
  }),
  bullet: block('li', { toDOM: () => ['p', { class: 'nb-bullet' }, 0], parseDOM: [{ tag: 'ul > li' }] }),
  ordered: block('li', { toDOM: () => ['p', { class: 'nb-ordered' }, 0], parseDOM: [{ tag: 'ol > li' }] }),
  quote: block('blockquote', { toDOM: () => ['blockquote', ['p', 0]] }),
  // `code_block`: the store's `code` kind (a mark is called `code` too).
  code_block: block('pre', { content: 'text*', marks: '', code: true, toDOM: () => ['pre', ['code', 0]] }),
  // A view block: its caption is editable inline text; the header (drawn by
  // the node view) names the composition it shows.
  view: block('div.nb-view', { attrs: { ...id, show: { default: '' }, focus: { default: '' } } }),
  text: { group: 'inline' },
  value: {
    group: 'inline',
    inline: true,
    atom: true,
    attrs: { target: { default: '' }, unit: { default: null as string | null }, digits: { default: null as number | null } },
    toDOM: (n) => ['span', { class: 'nb-value', 'data-target': n.attrs.target }, '…'],
  },
};

const marks: Record<string, MarkSpec> = {
  mention: {
    attrs: { target: { default: '' } },
    inclusive: false,
    toDOM: (m) => ['span', { class: 'nb-mention', 'data-target': m.attrs.target }, 0],
  },
  strong: { toDOM: () => ['strong', 0], parseDOM: [{ tag: 'strong' }, { tag: 'b' }] },
  em: { toDOM: () => ['em', 0], parseDOM: [{ tag: 'em' }, { tag: 'i' }] },
  code: { toDOM: () => ['code', 0], parseDOM: [{ tag: 'code' }], excludes: '_' },
  link: {
    attrs: { href: { default: '' } },
    inclusive: false,
    toDOM: (m) => ['a', { href: m.attrs.href, target: '_blank', rel: 'noopener' }, 0],
  },
};

export const schema = new Schema({ nodes, marks });

/** The store's block kinds; each is a top-level node name here, except
 *  `code`, which is `code_block`. */
export const KINDS = ['paragraph', 'heading', 'bullet', 'ordered', 'quote', 'code', 'view'] as const;
export const nodeName = (kind: string) => (kind === 'code' ? 'code_block' : kind);
export const kindOf = (node: string) => (node === 'code_block' ? 'code' : node);
