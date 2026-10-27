(async () => {
  const core = window.__TAURI__.core;
  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const report = { checks: [], errors: [] };
  window.addEventListener('error', e => report.errors.push(e.message));
  window.addEventListener('unhandledrejection', e => report.errors.push(String(e.reason)));
  const check = (name, condition, detail) => { report.checks.push({name, pass:!!condition, detail}); if (!condition) throw new Error(name + ': ' + JSON.stringify(detail)); };
  const timed = p => Promise.race([p, sleep(15000).then(() => { throw new Error('request timed out'); })]);
  const key = (key, opts={}) => { window.dispatchEvent(new KeyboardEvent('keydown', {key, bubbles:true, ...opts})); window.dispatchEvent(new KeyboardEvent('keyup', {key, bubbles:true, ...opts})); };
  try {
    for (let i=0;i<300;i++) { if (window.__iron && !document.querySelector('.loading')) break; await sleep(100); }
    await sleep(4000);
    check('shared workspace ready', !!window.__iron && !document.querySelector('.loading'));
    check('native canvas has no browser GPU context', document.getElementById('bevy-canvas').getContext('2d') != null);
    const reloaded = sessionStorage.getItem('iron.native.protocol.reload');
    if (reloaded) {
      sessionStorage.removeItem('iron.native.protocol.reload');
      const saved = JSON.parse(reloaded);
      report.checks = saved.report.checks;
      report.errors.push(...saved.report.errors);
      check('webview reload retains authored document', await timed(__iron.save()) === saved.authored);
      check('webview reload reports existing undo history', __iron.protocolState().canUndo === true, __iron.protocolState());
      check('webview reload settles its initial reads', __iron.protocolState().pending === 0, __iron.protocolState());
      await timed(__iron.load(saved.baseline));
    } else {
    const baseline = await timed(__iron.save());
    check('real document loaded', baseline.length > 1000, baseline.length);
    for (const [name, args, expected] of [
      ['not_a_document_command', [], 'unknown document operation'],
      ['next_ids', [], 'expected 1 arguments'],
      ['next_ids', ['wrong type'], 'argument 0'],
    ]) {
      let error = '';
      try { await timed(__iron.call(name, args)); } catch (e) { error = String(e); }
      check('document dispatch rejects ' + name + ' ' + JSON.stringify(args), error.includes(expected), error);
    }
    check('invalid dispatch leaves authored state unchanged', await timed(__iron.save()) === baseline);
    const result = await timed(__iron.edit('<edit><create id="$native" type="rect" parent="#1" name="native test" x="50" y="50" w="80" h="60" color="#ff2200"/></edit>', 'native parity fixture'));
    check('shared edit parser and transaction', result.ok, result);
    const authored = await timed(__iron.save());
    check('authored state changed', authored !== baseline && authored.includes('native test'));
    const invalid = await timed(__iron.edit('<edit><set id="#zz" x="4"/></edit>'));
    check('rejected edit is atomic', !invalid.ok && await timed(__iron.save()) === authored, invalid);
    document.getElementById('bevy-canvas').focus();
    key('z', {metaKey:true}); await sleep(400);
    const undone = JSON.parse(await timed(__iron.save())).nodes[result.aliases.$native];
    check('keyboard undo', undone?.tombstoned != null, undone);
    key('z', {metaKey:true, shiftKey:true}); await sleep(400);
    const redone = JSON.parse(await timed(__iron.save())).nodes[result.aliases.$native];
    check('keyboard redo', redone && redone.tombstoned == null, redone);
    await timed(__iron.load(authored)); await sleep(300);
    check('save reload canonical parity', await timed(__iron.save()) === authored);
    __probe(); await sleep(200); const before = window.__lastProbeStats;
    const start = performance.now(); while(performance.now()-start < 1500) {}
    __probe(); await sleep(200); const after = window.__lastProbeStats;
    check('native engine survives JS stall', after.frames-before.frames > 30 && after.renderThreadEnabled, {beforeFrames:before.frames,afterFrames:after.frames});
    if (after.simulation) {
      const {spans,...summary}=after.simulation;
      check('runtime population and authored parameter', summary.entities===100000 && summary.documentSpeeds.includes(50), summary);
      check('parallel ECS execution', summary.ecsParallelIntervals>0, summary.ecsParallelIntervals);
      check('render thread overlaps simulation', summary.renderSimulationOverlapIntervals>0, summary.renderSimulationOverlapIntervals);
      await core.invoke('native_test', {action:'trace',report:after.simulation});
    } else {
      // Stats is a live composition since free-running.md landed. Disable its
      // continuous redraw before measuring an otherwise idle workspace.
      const stopped = await timed(__iron.runMode('stats', 'on_demand'));
      if (!stopped.ok) throw new Error('could not stop Stats: ' + JSON.stringify(stopped));
      await sleep(2000); __probe(); await sleep(150); const idle=window.__lastProbeStats;
      await sleep(1200); __probe(); await sleep(150);
      check('idle submits no GPU frames', window.__lastProbeStats.renderedFrames===idle.renderedFrames, {before:idle.renderedFrames,after:window.__lastProbeStats.renderedFrames,timings:await __iron.timings(false)});
    }
    await core.invoke('native_test', {action:'resize'}); await sleep(700);
    check('native resize updates DOM', innerWidth > 0 && innerHeight > 0, {innerWidth,innerHeight,dpr:devicePixelRatio});
    __probe(); await sleep(150);
    check('surface and DOM physical coordinates agree', JSON.stringify(__lastProbeStats.physicalSize)===JSON.stringify([Math.round(innerWidth*devicePixelRatio), Math.round(innerHeight*devicePixelRatio)]), {native:__lastProbeStats.physicalSize,dom:[innerWidth*devicePixelRatio,innerHeight*devicePixelRatio]});
    await core.invoke('native_test', {action:'minimize'}); await sleep(500);
    await core.invoke('native_test', {action:'restore'}); await sleep(700);
    check('restore remains queryable', (await timed(__iron.view({fidelity:'skeleton'}))).includes('<'));
    check('no page errors', report.errors.length===0, report.errors);
    await timed(__iron.load(baseline));
    await sleep(300);
    const reloadEdit = await timed(__iron.edit('<edit><create id="$reload" type="rect" parent="#1" name="reload history fixture" x="30" y="30" w="20" h="20"/></edit>'));
    if (!reloadEdit.ok) throw new Error(JSON.stringify(reloadEdit));
    await sleep(200);
    sessionStorage.setItem('iron.native.protocol.reload', JSON.stringify({report, baseline, authored: await timed(__iron.save())}));
    location.reload();
    return;
    }

  } catch(e) { report.failure = String(e); }
  await core.invoke('native_test', {action:'report',report});
  await core.invoke('native_test', {action:'close'});
})();
