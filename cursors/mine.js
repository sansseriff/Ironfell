// Mines Figma's custom cursors. Paste into the DevTools console of a Figma
// file open in Chrome (figma.com), then use Figma as usual: every new cursor
// that appears is captured and logged with a number. When done, run
// `saveCursors()` to download each one as cursor-<n>.txt (the CSS `cursor`
// value: image-set(url("data:image/png;base64,…") 8dppx) <hotspot x> <y>, auto),
// the same format as the other files in this folder.
(() => {
  const seen = new Map(); // css value -> n
  const check = (el) => {
    for (let e = el; e && e !== document; e = e.parentNode) {
      const c = e.nodeType === 1 && getComputedStyle(e).cursor;
      if (c && c.includes('url(')) {
        if (!seen.has(c)) {
          seen.set(c, seen.size + 1);
          const n = seen.size;
          const src = c.match(/url\("?([^")]+)"?\)/)?.[1];
          const hot = c.match(/\)\s*([\d.]+ [\d.]+)/)?.[1];
          console.log(`%c cursor ${n} (hotspot ${hot})`, `padding:24px 32px;background:url(${src}) left/32px no-repeat`);
        }
        return;
      }
    }
  };
  let last = null;
  addEventListener('pointermove', (ev) => { last = ev.target; check(last); }, true);
  // Holding a modifier changes the cursor without the pointer moving.
  const recheck = () => setTimeout(() => last && check(last), 30);
  addEventListener('keydown', recheck, true);
  addEventListener('keyup', recheck, true);
  setInterval(() => last && check(last), 100);
  window.saveCursors = () => {
    for (const [css, n] of seen) {
      const a = document.createElement('a');
      a.href = URL.createObjectURL(new Blob([css], { type: 'text/plain' }));
      a.download = `cursor-${n}.txt`;
      a.click();
    }
    return `${seen.size} cursors`;
  };
  // Or one at a time to the clipboard: copyCursor(3), then paste into a file.
  window.copyCursor = (n) => {
    const css = [...seen].find(([, m]) => m === n)?.[0];
    if (!css) return `no cursor ${n}`;
    copy(css);
    return `cursor ${n} copied (${css.length} characters)`;
  };
  console.log('Capturing cursors. Run saveCursors() when done, or copyCursor(n).');
})();
