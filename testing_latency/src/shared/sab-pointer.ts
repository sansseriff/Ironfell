import type { Point } from './paths';

export interface SharedPointerView {
  buffer: SharedArrayBuffer;
  ints: Int32Array;
  floats: Float64Array;
}

export interface SharedPointerSnapshot {
  point: Point;
  buttons: number;
  eventTime: number;
  readTime: number;
  sequence: number;
}

const BYTE_LENGTH = 64;
const INT_OFFSET = 0;
const FLOAT_OFFSET = 16;

const SEQ = 0;
const BUTTONS = 1;
const X = 0;
const Y = 1;
const EVENT_TIME = 2;
const READ_TIME = 3;

export function createSharedPointerView(): SharedPointerView {
  return viewSharedPointer(new SharedArrayBuffer(BYTE_LENGTH));
}

export function viewSharedPointer(buffer: SharedArrayBuffer): SharedPointerView {
  return {
    buffer,
    ints: new Int32Array(buffer, INT_OFFSET, 4),
    floats: new Float64Array(buffer, FLOAT_OFFSET, 4),
  };
}

export function writeSharedPointer(view: SharedPointerView, point: Point, buttons: number, eventTime: number) {
  Atomics.add(view.ints, SEQ, 1);
  view.floats[X] = point.x;
  view.floats[Y] = point.y;
  view.floats[EVENT_TIME] = eventTime;
  view.floats[READ_TIME] = performance.now();
  Atomics.store(view.ints, BUTTONS, buttons);
  Atomics.add(view.ints, SEQ, 1);
}

export function readSharedPointer(view: SharedPointerView): SharedPointerSnapshot | null {
  for (let attempt = 0; attempt < 3; attempt++) {
    const sequenceBefore = Atomics.load(view.ints, SEQ);
    if (sequenceBefore % 2 !== 0) continue;

    const x = view.floats[X];
    const y = view.floats[Y];
    const eventTime = view.floats[EVENT_TIME];
    const readTime = view.floats[READ_TIME];
    const buttons = Atomics.load(view.ints, BUTTONS);
    const sequenceAfter = Atomics.load(view.ints, SEQ);

    if (sequenceBefore === sequenceAfter && sequenceAfter % 2 === 0) {
      return {
        point: { x, y },
        buttons,
        eventTime,
        readTime,
        sequence: sequenceAfter,
      };
    }
  }
  return null;
}
