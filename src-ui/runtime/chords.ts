/**
 * Chords (iron_document `commands.rs`): a key event as the canonical chord a
 * `keys.commands` keymap names (`mod+alt+g`), and a chord as a menu shows it.
 */

const KEYS: Record<string, string> = {
  Equal: '=', Minus: '-', BracketLeft: '[', BracketRight: ']', Semicolon: ';', Quote: "'",
  Comma: ',', Period: '.', Slash: '/', Backslash: '\\', Backquote: '`', NumpadSubtract: '-',
};

/** A key event as a canonical chord (`mod+alt+g`, `shift+1`, `]`): by key
 *  code, since Alt and Shift change the character typed. `mod` is Cmd or
 *  Ctrl. `null` for a key no chord ends in (a modifier, Escape, an arrow). */
export function chordOf(e: Pick<KeyboardEvent, 'code' | 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey'>): string | null {
  let key: string | undefined;
  let shift = e.shiftKey;
  if (/^Key[A-Z]$/.test(e.code)) key = e.code.slice(3).toLowerCase();
  else if (/^(Digit|Numpad)[0-9]$/.test(e.code)) key = e.code.slice(-1);
  else if (e.code === 'NumpadAdd') { key = '='; shift = true; }
  else key = KEYS[e.code];
  if (!key) return null;
  return [e.metaKey || e.ctrlKey ? 'mod' : '', e.altKey ? 'alt' : '', shift ? 'shift' : '', key].filter(Boolean).join('+');
}

const MAC = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);

/** A chord as a menu shows it: `⌥⌘G` on macOS, `Ctrl+Alt+G` elsewhere. */
export function chordLabel(chord: string, mac = MAC): string {
  if (!chord) return '';
  const parts = chord.split('+');
  const key = parts[parts.length - 1];
  const mods = new Set(parts.slice(0, -1));
  const k = key.length === 1 ? key.toUpperCase() : key;
  if (mac) return `${mods.has('alt') ? '⌥' : ''}${mods.has('shift') ? '⇧' : ''}${mods.has('mod') ? '⌘' : ''}${k}`;
  return [mods.has('mod') ? 'Ctrl' : '', mods.has('alt') ? 'Alt' : '', mods.has('shift') ? 'Shift' : '', k].filter(Boolean).join('+');
}
