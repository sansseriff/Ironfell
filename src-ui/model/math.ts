/// <reference types="vite/client" />

import { liteAdaptor } from '@mathjax/src/js/adaptors/liteAdaptor.js';
import { RegisterHTMLHandler } from '@mathjax/src/js/handlers/html.js';
import { TeX } from '@mathjax/src/js/input/tex.js';
import '@mathjax/src/js/input/tex/ams/AmsConfiguration.js';
import '@mathjax/src/js/input/tex/configmacros/ConfigMacrosConfiguration.js';
import '@mathjax/src/js/input/tex/html/HtmlConfiguration.js';
import { mathjax } from '@mathjax/src/js/mathjax.js';
import { SVG } from '@mathjax/src/js/output/svg.js';
import type { SvgFontData } from '@mathjax/src/js/output/svg/FontData.js';
import { MathJaxTexFont } from '@mathjax/mathjax-tex-font/js/svg.js';

const adaptor = liteAdaptor();
RegisterHTMLHandler(adaptor);

// MathJax requests the less common Fira glyph sets on demand. Resolve them
// through the bundled modules so equations work in workers and offline.
mathjax.asyncLoad = name => {
  const prefix = '@mathjax/mathjax-fira-font/js/svg/dynamic/';
  const modules = import.meta.glob('/node_modules/@mathjax/mathjax-fira-font/mjs/svg/dynamic/*.js');
  const load = name.startsWith(prefix) && modules[`/node_modules/@mathjax/mathjax-fira-font/mjs/svg/dynamic/${name.slice(prefix.length)}`];
  return load ? load() : Promise.reject(new Error(`Unsupported MathJax module: ${name}`));
};

function mathDocument(fontData: new () => SvgFontData) {
  return mathjax.document('', {
    InputJax: new TeX({
      packages: ['base', 'ams', 'configmacros', 'html'],
      macros: { ironpart: ['\\class{iron-part-#1}{#2}', 2] },
    }),
    OutputJax: new SVG({ fontCache: 'none', fontData }),
  });
}

const tex = mathDocument(MathJaxTexFont);
let fira: Promise<ReturnType<typeof mathDocument>> | undefined;

/** Typeset one TeX expression in the same DOM-free host on web and desktop. */
export async function mathSvg(source: string, font: string): Promise<string> {
  if (!source.trim()) throw new Error('LaTeX source is empty');
  if (source.length > 8192) throw new Error('LaTeX source exceeds 8192 characters');
  const equations = font === 'fira'
    ? await (fira ??= import('@mathjax/mathjax-fira-font/js/svg.js').then(({ MathJaxFiraFont }) => mathDocument(MathJaxFiraFont)))
    : tex;
  const rendered = await mathjax.handleRetriesFor(() => equations.convert(source, { display: true }));
  const svg = adaptor.tags(rendered, 'svg')[0];
  if (!svg) throw new Error('MathJax produced no SVG');
  for (const group of adaptor.tags(svg, 'g')) {
    const error = adaptor.getAttribute(group, 'data-mjx-error');
    if (error) throw new Error(error);
    const label: string | undefined = adaptor.getAttribute(group, 'class');
    const classes = label?.split(/\s+/) ?? [];
    const marks = classes.filter(name => name.startsWith('iron-part-'));
    if (marks.length > 1) throw new Error('Use one LaTeX part key per marked group');
    if (marks.length === 1) adaptor.setAttribute(group, 'id', marks[0]);
  }
  if (adaptor.tags(svg, 'text').length) {
    throw new Error('This equation contains characters without a MathJax outline');
  }
  const viewBox = adaptor.getAttribute(svg, 'viewBox')?.trim().split(/\s+/).map(Number);
  if (!viewBox || viewBox.length !== 4 || !viewBox.every(Number.isFinite) || viewBox[2] <= 0 || viewBox[3] <= 0) {
    throw new Error('The equation has no visible bounds');
  }
  // 1000 SVG units per em. Match the viewport to the viewBox size so
  // its top-left maps to the equation group's origin.
  adaptor.setAttribute(svg, 'width', String(viewBox[2]));
  adaptor.setAttribute(svg, 'height', String(viewBox[3]));
  return adaptor.outerHTML(svg);
}
