import type { TrialConfig } from './protocol';

export interface Point {
  x: number;
  y: number;
}

export function pointAt(trial: TrialConfig, elapsedMs: number): Point {
  const t = Math.max(0, Math.min(trial.durationMs, elapsedMs));
  const progress = trial.durationMs > 0 ? t / trial.durationMs : 0;

  switch (trial.path) {
    case 'constant-horizontal': {
      const travel = trial.velocityPxPerMs * t;
      return { x: trial.startX + travel, y: trial.startY };
    }
    case 'sine-horizontal': {
      const travel = trial.velocityPxPerMs * t;
      const y = trial.startY + Math.sin(progress * Math.PI * 4) * trial.amplitudePx * 0.35;
      return { x: trial.startX + travel, y };
    }
    case 'step-horizontal': {
      const steps = 8;
      const stepIndex = Math.floor(progress * steps);
      const stepWidth = (trial.velocityPxPerMs * trial.durationMs) / steps;
      return { x: trial.startX + stepIndex * stepWidth, y: trial.startY };
    }
    case 'micro-jitter': {
      const travel = trial.velocityPxPerMs * t * 0.5;
      const jitter = Math.sin(t * 0.08) * 10 + Math.sin(t * 0.211) * 4;
      return { x: trial.startX + travel + jitter, y: trial.startY + Math.cos(t * 0.095) * 8 };
    }
  }
}

export function instantaneousSpeedPxPerMs(trial: TrialConfig, elapsedMs: number): number {
  const dt = 4;
  const a = pointAt(trial, Math.max(0, elapsedMs - dt));
  const b = pointAt(trial, Math.min(trial.durationMs, elapsedMs + dt));
  const distance = Math.hypot(b.x - a.x, b.y - a.y);
  return distance / (dt * 2);
}
