/**
 * The hidden textarea behind canvas text editing (plans/specs/text.md §7),
 * Figma's technique: the engine draws the text, caret and selection; this
 * textarea receives the keystrokes, so typing, IME composition, dead keys,
 * the clipboard, spellcheck and word-wise motion are the browser's.
 *
 * The engine sends the draft and its selection (`textEdit`); the textarea
 * takes them and sits under the caret, so an IME's candidate window opens
 * by the text. Every change goes back whole (`textInput`): text and
 * selection in UTF-16 offsets, the textarea's own. Up and Down move by the
 * canvas layout's lines, which only the engine knows (`textMotion`); Esc
 * ends the edit.
 */

import { pixelRatio } from './settings.svelte';

export type TextEditState = { active: boolean; node: string; text: string; anchor: number; focus: number; caret: [number, number, number] };

export class TextInputMirror {
  private el: HTMLTextAreaElement;
  private composing = false;
  private active = false;

  constructor(
    private canvas: HTMLCanvasElement,
    private post: (msg: any) => void,
  ) {
    const el = document.createElement('textarea');
    el.setAttribute('aria-label', 'Text');
    el.autocapitalize = 'off';
    el.spellcheck = false;
    el.wrap = 'off';
    // Present for the browser (focus, IME, accessibility), invisible to people.
    Object.assign(el.style, {
      position: 'fixed', left: '0px', top: '0px', width: '1px', height: '1em',
      padding: '0', border: '0', margin: '0', outline: 'none', resize: 'none',
      opacity: '0', color: 'transparent', background: 'transparent', caretColor: 'transparent',
      overflow: 'hidden', whiteSpace: 'pre', pointerEvents: 'none', zIndex: '-1',
    });
    document.body.appendChild(el);
    this.el = el;
    el.addEventListener('compositionstart', () => (this.composing = true));
    el.addEventListener('compositionend', () => {
      this.composing = false;
      this.send();
    });
    el.addEventListener('input', () => this.send());
    // Selection moves (arrows, Home/End within a line, Cmd+A) with no input.
    document.addEventListener('selectionchange', () => {
      if (this.active && document.activeElement === el && !this.composing) this.send();
    });
    el.addEventListener('keydown', (e) => this.onKey(e));
    // Clicks on the canvas would move focus to it; the engine handles them
    // and focus returns here.
    el.addEventListener('blur', () => {
      if (this.active) requestAnimationFrame(() => this.active && !this.otherInputFocused() && el.focus({ preventScroll: true }));
    });
  }

  /** The engine's latest state. */
  update(s: TextEditState) {
    if (!s.active) {
      if (this.active) {
        this.active = false;
        this.el.blur();
        this.el.value = '';
      }
      return;
    }
    this.active = true;
    const rect = this.canvas.getBoundingClientRect();
    const dpr = pixelRatio();
    Object.assign(this.el.style, {
      left: `${rect.left + s.caret[0] / dpr}px`,
      top: `${rect.top + s.caret[1] / dpr}px`,
      height: `${Math.max(8, s.caret[2] / dpr)}px`,
      fontSize: `${Math.max(8, s.caret[2] / dpr) * 0.8}px`,
    });
    // Mid-composition the textarea is the truth; the echo would cut it short.
    if (!this.composing) {
      if (this.el.value !== s.text) this.el.value = s.text;
      const [start, end, dir] = s.anchor <= s.focus ? [s.anchor, s.focus, 'forward'] : [s.focus, s.anchor, 'backward'];
      if (this.el.selectionStart !== start || this.el.selectionEnd !== end) this.el.setSelectionRange(start, end, dir as 'forward' | 'backward');
    }
    if (document.activeElement !== this.el && !this.otherInputFocused()) this.el.focus({ preventScroll: true });
  }

  private send() {
    if (!this.active || this.composing) return;
    const el = this.el;
    const backward = el.selectionDirection === 'backward';
    const anchor = backward ? el.selectionEnd : el.selectionStart;
    const focus = backward ? el.selectionStart : el.selectionEnd;
    this.post({ ty: 'textInput', text: el.value, anchor, focus });
  }

  private onKey(e: KeyboardEvent) {
    if (this.composing) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      this.post({ ty: 'toolKey', key: 'escape', m: 0 });
      return;
    }
    // Styling the selection: Figma's shortcuts.
    const mod = e.metaKey || e.ctrlKey;
    const toggle = mod && !e.altKey && (
      e.key.toLowerCase() === 'b' && !e.shiftKey ? 'bold'
      : e.key.toLowerCase() === 'i' && !e.shiftKey ? 'italic'
      : e.key.toLowerCase() === 'u' && !e.shiftKey ? 'underline'
      : e.key.toLowerCase() === 'x' && e.shiftKey ? 'strike'
      : null);
    if (toggle) {
      e.preventDefault();
      e.stopPropagation();
      this.post({ ty: 'textToggle', what: toggle });
      return;
    }
    // Lines are the canvas layout's: wrapped lines exist only there.
    if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
      e.preventDefault();
      this.post({ ty: 'textMotion', motion: e.key === 'ArrowUp' ? 'up' : 'down', extend: e.shiftKey });
      return;
    }
    // Undo inside the edit is the textarea's; the canvas's would undo
    // around the open edit. Stop the page's shortcut handler seeing it.
    e.stopPropagation();
  }

  /** Another field (the inspector) has focus: leave it there. */
  private otherInputFocused(): boolean {
    const a = document.activeElement as HTMLElement | null;
    return !!a && a !== this.el && (a.tagName === 'INPUT' || a.tagName === 'TEXTAREA' || a.tagName === 'SELECT' || a.isContentEditable);
  }
}
