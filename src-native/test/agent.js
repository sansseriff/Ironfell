(async () => {
  const core = window.__TAURI__.core;
  if (!new URLSearchParams(location.search).has('demo')) return;
  const report = { checks: [], errors: [] };
  const pause = () => new Promise(resolve => setTimeout(resolve, 100));
  const waitUntil = async (condition, timeout = 15000) => {
    const deadline = Date.now() + timeout;
    while (!condition() && Date.now() < deadline) await pause();
    return condition();
  };
  const check = (name, pass) => { report.checks.push({ name, pass: !!pass }); if (!pass) throw new Error(name); };
  const readDocument = () => Promise.race([window.__iron.view({ fidelity: 'skeleton' }), new Promise((_, reject) => setTimeout(() => reject(new Error('native document read timed out')), 5000))]);
  void core.invoke('native_test', { action: 'report', report: { phase: 'agent-test-start', checks: [], errors: [] } });
  window.addEventListener('error', event => report.errors.push(event.message));
  window.addEventListener('unhandledrejection', event => report.errors.push(String(event.reason)));
  try {
    const previous = sessionStorage.getItem('iron.native.agent.reloaded');
    if (previous) {
      sessionStorage.removeItem('iron.native.agent.reloaded');
      report.checks.push(...JSON.parse(previous));
      const connection = await core.invoke('agent_connection');
      const response = await fetch(`${connection.endpoint}/models`, { method: 'POST', headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${connection.token}` }, body: JSON.stringify({ provider: 'claude' }), signal: AbortSignal.timeout(5000) });
      check('private connection is available after webview reload', connection.provider === 'claude' && response.ok && Array.isArray((await response.json()).models));
      check('native editor resumes after webview reload', await waitUntil(() => !!window.__iron && !document.querySelector('.loading')));
      check('native client pairs automatically after reload', await waitUntil(() => window.__iron.agentState().paired));
      check('native document reads work after reload', typeof await readDocument() === 'string');
      await core.invoke('native_test', { action: 'report', report });
      await core.invoke('native_test', { action: 'close' });
      return;
    }
    check('native workspace ready', await waitUntil(() => !!window.__iron && !document.querySelector('.loading')));
    const connection = await core.invoke('agent_connection');
    check('launcher supplies a private Claude connection', connection?.provider === 'claude' && connection.endpoint === 'http://127.0.0.1:4318' && connection.token.length >= 32);
    const tab = [...document.querySelectorAll('[role="tab"]')].find(element => element.textContent.trim() === 'Model');
    check('Model tab available', !!tab);
    tab.click();
    check('document reads work with Model tab active', typeof await readDocument() === 'string');
    check('native client pairs automatically', await waitUntil(() => window.__iron.agentState().paired));
    const state = window.__iron.agentState();
    check('Claude selected automatically', state.provider === 'claude' && state.hasToken && state.canAsk);
    const response = await fetch(`${connection.endpoint}/models`, { method: 'POST', headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${connection.token}` }, body: JSON.stringify({ provider: 'claude' }), signal: AbortSignal.timeout(5000) });
    check('native webview reaches authenticated model listing', response.ok && Array.isArray((await response.json()).models));
    check('no token in URL or browser storage', !location.href.includes(connection.token) && !JSON.stringify(localStorage).includes(connection.token) && !JSON.stringify(sessionStorage).includes(connection.token));
    sessionStorage.setItem('iron.native.agent.reloaded', JSON.stringify(report.checks));
    location.reload();
    return;
  } catch (error) { report.errors.push(String(error)); if (window.__iron?.agentState) report.connection = window.__iron.agentState(); }
  await core.invoke('native_test', { action: 'report', report });
  await core.invoke('native_test', { action: 'close' });
})();
