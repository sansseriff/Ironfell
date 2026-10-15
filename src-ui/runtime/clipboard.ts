/**
 * The shell's half of copy and paste (plans/specs/clipboard-and-assets.md).
 *
 * The document lives in the worker, which cannot reach `navigator.clipboard`.
 * So Cmd+C starts the clipboard write here, inside the key event's user
 * activation, with a `ClipboardItem` whose content is a promise; the worker
 * answers with the copied fragment's text and the promise resolves. A paste
 * is read from the DOM `paste` event, which hands over the clipboard without
 * a permission prompt.
 *
 * If the system clipboard refuses the write (no permission, an unfocused
 * document), the last copied text is kept here and the next paste uses it,
 * so copy and paste still work inside the tab.
 */

let pendingCopy: ((text: string) => void) | null = null;
let lastText = '';
let lastWriteFailed = false;
let selected = 0;

/** How many nodes the canvas has selected, from the worker's selection messages. */
export function setSelectionCount(n: number) {
    selected = n;
}

/**
 * Whether Cmd+C belongs to the canvas: something is selected there, and the
 * user is not copying text from the page or a text field.
 */
export function canvasOwnsCopy(target: EventTarget | null): boolean {
    const el = target as HTMLElement | null;
    const editing = !!el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.isContentEditable);
    const textSelected = (window.getSelection()?.toString() ?? '') !== '';
    return selected > 0 && !editing && !textSelected;
}

/** Ask the worker for the selection's fragment and put it on the clipboard. */
export function beginCopy(ask: () => void) {
    pendingCopy?.('');
    const text = new Promise<string>((resolve) => {
        pendingCopy = resolve;
        setTimeout(() => resolve(''), 2000);
    });
    ask();
    const blob = text.then((t) => {
        if (!t) throw new Error('nothing was copied');
        lastText = t;
        return new Blob([t], { type: 'text/plain' });
    });
    try {
        navigator.clipboard
            .write([new ClipboardItem({ 'text/plain': blob })])
            .then(() => { lastWriteFailed = false; }, () => { lastWriteFailed = true; });
    } catch {
        // No ClipboardItem: fall back to writeText once the text is here.
        lastWriteFailed = true;
        text.then((t) => t && navigator.clipboard?.writeText(t).then(() => { lastWriteFailed = false; }, () => { }));
    }
}

/** The worker's answer to `beginCopy`. */
export function copied(text: string) {
    if (text) lastText = text;
    pendingCopy?.(text);
    pendingCopy = null;
}

/** The last fragment copied in this tab, for driving and tests. */
export function lastCopied(): string {
    return lastText;
}

function isFragment(text: string): boolean {
    return text.trimStart().startsWith('{') && text.includes('"iron_fragment":');
}

/**
 * The fragment a paste event carries, or the tab's own last copy when the
 * system clipboard refused it. `null` when the paste is not for the canvas.
 */
export function pastedFragment(e: ClipboardEvent): string | null {
    const el = e.target as HTMLElement | null;
    if (el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.isContentEditable)) return null;
    const text = e.clipboardData?.getData('text/plain') ?? '';
    if (isFragment(text)) return text;
    if (lastWriteFailed && lastText) return lastText;
    return null;
}
