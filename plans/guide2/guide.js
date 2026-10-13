// Shared behaviour: syntax colour, the "on this page" list, and the field guide's filter.
(function () {
  function highlight() {
    if (!window.hljs) return;
    document.querySelectorAll("pre.src code, pre.plain code[class*='language-']").forEach(function (el) {
      try { window.hljs.highlightElement(el); } catch (e) { /* leave it plain */ }
    });
  }

  function toc() {
    var list = document.querySelector(".sidebar .toc");
    if (!list) return;
    document.querySelectorAll("main h2[id]").forEach(function (h) {
      var li = document.createElement("li");
      var a = document.createElement("a");
      a.href = "#" + h.id;
      a.textContent = h.textContent.replace(/^\s*\d+(\.\d+)?\s*/, "");
      li.appendChild(a);
      list.appendChild(li);
    });
    if (!list.children.length) list.previousElementSibling && (list.previousElementSibling.hidden = true);
  }

  function finder() {
    var input = document.getElementById("finder-q");
    if (!input) return;
    var rows = Array.prototype.slice.call(document.querySelectorAll("table.guide tbody tr"));
    var buttons = Array.prototype.slice.call(document.querySelectorAll(".finder button[data-sys]"));
    var count = document.getElementById("finder-count");
    var on = {};
    buttons.forEach(function (b) { on[b.dataset.sys] = true; });
    function apply() {
      var q = input.value.trim().toLowerCase();
      var shown = 0;
      rows.forEach(function (r) {
        var ok = on[r.dataset.sys] && (!q || r.textContent.toLowerCase().indexOf(q) !== -1);
        r.hidden = !ok;
        if (ok) shown++;
      });
      count.textContent = shown + " of " + rows.length + " entries";
    }
    buttons.forEach(function (b) {
      b.addEventListener("click", function () {
        var all = buttons.every(function (x) { return on[x.dataset.sys]; });
        if (all) {
          // First click isolates one system; later clicks toggle.
          buttons.forEach(function (x) { on[x.dataset.sys] = x === b; });
        } else {
          on[b.dataset.sys] = !on[b.dataset.sys];
          if (!buttons.some(function (x) { return on[x.dataset.sys]; })) buttons.forEach(function (x) { on[x.dataset.sys] = true; });
        }
        buttons.forEach(function (x) { x.setAttribute("aria-pressed", on[x.dataset.sys] ? "true" : "false"); });
        apply();
      });
    });
    input.addEventListener("input", apply);
    var hash = decodeURIComponent(location.hash.replace(/^#q=/, ""));
    if (location.hash.indexOf("#q=") === 0) input.value = hash;
    apply();
  }

  function start() { highlight(); toc(); finder(); }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start);
  else start();
})();
