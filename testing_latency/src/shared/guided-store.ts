import type { TestAppId, TrialPathId, TrialResult } from './protocol';

const DB_NAME = 'iron-latency-guided';
const DB_VERSION = 1;
const STORE_NAME = 'sessions';

export interface GuidedStep {
  app: TestAppId;
  path: TrialPathId;
}

export interface GuidedSession {
  id: string;
  createdAtIso: string;
  updatedAtIso: string;
  platformLabel: string;
  browserLabel: string;
  userAgent: string;
  dpr: number;
  steps: GuidedStep[];
  results: Array<TrialResult | null>;
  skipped: boolean[];
}

export async function createGuidedSession(input: {
  platformLabel: string;
  browserLabel: string;
  steps: GuidedStep[];
}): Promise<GuidedSession> {
  const now = new Date().toISOString();
  const session: GuidedSession = {
    id: crypto.randomUUID(),
    createdAtIso: now,
    updatedAtIso: now,
    platformLabel: input.platformLabel,
    browserLabel: input.browserLabel,
    userAgent: navigator.userAgent,
    dpr: window.devicePixelRatio || 1,
    steps: input.steps,
    results: input.steps.map(() => null),
    skipped: input.steps.map(() => false),
  };
  await putGuidedSession(session);
  return session;
}

export async function getGuidedSession(id: string): Promise<GuidedSession | null> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, 'readonly');
    const request = tx.objectStore(STORE_NAME).get(id);
    request.onerror = () => reject(request.error);
    request.onsuccess = () => resolve((request.result as GuidedSession | undefined) || null);
  });
}

export async function putGuidedSession(session: GuidedSession): Promise<void> {
  const db = await openDb();
  session.updatedAtIso = new Date().toISOString();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, 'readwrite');
    tx.onerror = () => reject(tx.error);
    tx.oncomplete = () => resolve();
    tx.objectStore(STORE_NAME).put(session);
  });
}

export async function saveGuidedResult(id: string, stepIndex: number, result: TrialResult): Promise<GuidedSession> {
  const session = await requireGuidedSession(id);
  assertStepIndex(session, stepIndex);
  session.results[stepIndex] = result;
  session.skipped[stepIndex] = false;
  await putGuidedSession(session);
  return session;
}

export async function skipGuidedStep(id: string, stepIndex: number): Promise<GuidedSession> {
  const session = await requireGuidedSession(id);
  assertStepIndex(session, stepIndex);
  session.results[stepIndex] = null;
  session.skipped[stepIndex] = true;
  await putGuidedSession(session);
  return session;
}

export async function requireGuidedSession(id: string): Promise<GuidedSession> {
  const session = await getGuidedSession(id);
  if (!session) throw new Error(`Guided session not found: ${id}`);
  return session;
}

export function nextIncompleteStep(session: GuidedSession, afterIndex = -1): number {
  for (let index = afterIndex + 1; index < session.steps.length; index += 1) {
    if (!session.results[index] && !session.skipped[index]) return index;
  }
  return -1;
}

function assertStepIndex(session: GuidedSession, stepIndex: number) {
  if (!Number.isInteger(stepIndex) || stepIndex < 0 || stepIndex >= session.steps.length) {
    throw new Error(`Invalid guided step index ${stepIndex}`);
  }
}

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION);
    request.onerror = () => reject(request.error);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(STORE_NAME)) {
        db.createObjectStore(STORE_NAME, { keyPath: 'id' });
      }
    };
    request.onsuccess = () => resolve(request.result);
  });
}
