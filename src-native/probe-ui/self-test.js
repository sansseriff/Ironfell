// Loaded by the native host only when IRON_NATIVE_SELF_TEST is set.
(async () => {
  const call = window.__TAURI__.core.invoke;
  const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
  const checks = [];
  const diagnostics = {};
  function assert(condition, name) { if (!condition) throw Error(name); checks.push(name); }
  const control = action => call('probe_test', { action, report: null });
  try {
    await wait(6000);
    const initial = await call('probe_stats');
    assert(initial.frames > 5, 'native frames advance');
    assert(initial.render_thread_enabled, 'pipelined render thread installed');
    assert(initial.ecs_threads.length >= 2, 'ECS work uses multiple threads');
    const before = await call('probe_stats');
    const until = performance.now() + 1500;
    while (performance.now() < until) { /* Deliberately stall webview JS. */ }
    const after = await call('probe_stats');
    diagnostics.beforeStall = before;
    diagnostics.afterStall = after;
    diagnostics.framesDuring1500msStall = after.frames - before.frames;
    assert(after.frames - before.frames > 30, 'native animation survives a stalled webview');
    document.querySelector('#focus').focus();
    document.querySelector('#focus').value = 'HTML focus works';
    assert(document.activeElement.id === 'focus', 'HTML input receives focus');
    await call('toggle_animation');
    await wait(500);
    const paused = await call('probe_stats');
    await wait(500);
    assert((await call('probe_stats')).frames === paused.frames, 'paused native driver stops idle frames');
    await call('toggle_background');
    await wait(300);
    assert((await call('probe_stats')).frames > paused.frames, 'command wakes idle driver');
    await control('resize');
    await wait(400);
    assert((await call('probe_stats')).physical_size.join() === '1800,1200', 'physical resize reaches Bevy');
    await call('toggle_animation');
    await wait(500);
    const resumed = await call('probe_stats');
    assert(resumed.running && resumed.frames > paused.frames, 'animation resumes');
    await control('minimize');
    await wait(400);
    const minimized = await call('probe_stats');
    await wait(400);
    assert((await call('probe_stats')).frames === minimized.frames, 'minimized driver stops');
    await control('restore');
    await wait(600);
    assert((await call('probe_stats')).frames > minimized.frames, 'restored driver resumes');
    await call('probe_test', {action:'report', report:{ok:true, checks, diagnostics, stats:await call('probe_stats')}});
  } catch(error) {
    await call('probe_test', {action:'report', report:{ok:false, checks, diagnostics, error:String(error)}});
  }
})();
