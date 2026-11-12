import { describe, expect, test } from 'bun:test';
import { chordLabel, chordOf } from '../src-ui/runtime/chords';

const key = (code: string, mods: Partial<Record<'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey', boolean>> = {}) =>
  ({ code, metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...mods });

describe('chords (commands.rs keys.commands)', () => {
  test('a key event is a canonical chord, by key code', () => {
    // Alt changes the character typed (⌥G is ©), so the code decides.
    expect(chordOf(key('KeyG', { metaKey: true, altKey: true }))).toBe('mod+alt+g');
    expect(chordOf(key('KeyG', { ctrlKey: true, altKey: true }))).toBe('mod+alt+g');
    expect(chordOf(key('Digit1', { shiftKey: true }))).toBe('shift+1');
    expect(chordOf(key('BracketRight'))).toBe(']');
    expect(chordOf(key('Minus', { metaKey: true }))).toBe('mod+-');
    expect(chordOf(key('NumpadAdd'))).toBe('shift+=');
    expect(chordOf(key('Escape'))).toBeNull();
    expect(chordOf(key('ShiftLeft', { shiftKey: true }))).toBeNull();
  });
  test('a chord reads as the platform writes it', () => {
    expect(chordLabel('mod+alt+g', true)).toBe('⌥⌘G');
    expect(chordLabel('alt+shift+c', true)).toBe('⌥⇧C');
    expect(chordLabel('mod+alt+g', false)).toBe('Ctrl+Alt+G');
    expect(chordLabel(']', true)).toBe(']');
    expect(chordLabel('', true)).toBe('');
  });
});
