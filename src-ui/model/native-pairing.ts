import { localEndpoint } from './runtime-options';

export interface NativeAgentConnection { endpoint: string; token: string; provider: 'codex' | 'claude' }

export async function discoverNativeAgent(invoke: (command: string) => Promise<unknown>): Promise<NativeAgentConnection | null> {
  try {
    const value = await invoke('agent_connection') as NativeAgentConnection | null;
    if (!value || value.endpoint !== 'http://127.0.0.1:4318' || localEndpoint(value.endpoint).origin !== value.endpoint ||
      typeof value.token !== 'string' || value.token.length < 32 || value.token.length > 1024 || !['codex', 'claude'].includes(value.provider)) return null;
    return value;
  } catch { return null; }
}
