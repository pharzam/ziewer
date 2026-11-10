(function () {
  var dark = window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches;
  if (window.mermaid) {
    window.mermaid.initialize({ startOnLoad: true, theme: dark ? 'dark' : 'default' });
  }
  // Size embedded HTML pages to their content (same origin).
  document.querySelectorAll('iframe.html-frame').forEach(function (frame) {
    function fit() {
      try {
        var doc = frame.contentDocument;
        if (doc && doc.documentElement) frame.style.height = (doc.documentElement.scrollHeight + 4) + 'px';
      } catch (e) { /* cross-origin: keep default height */ }
    }
    frame.addEventListener('load', function () {
      fit();
      try {
        if (window.ResizeObserver && frame.contentDocument.body) {
          new ResizeObserver(fit).observe(frame.contentDocument.body);
        }
      } catch (e) {}
    });
  });
  // File drawer: toggled from the top bar; closes on backdrop click or Escape.
  var toggle = document.getElementById('files-toggle');
  var drawer = document.getElementById('files-drawer');
  var backdrop = document.getElementById('drawer-backdrop');
  if (toggle && drawer && backdrop) {
    var setOpen = function (open) {
      drawer.hidden = !open;
      backdrop.hidden = !open;
      toggle.setAttribute('aria-expanded', open ? 'true' : 'false');
      if (open) {
        var active = drawer.querySelector('a.active');
        if (active) active.scrollIntoView({ block: 'center' });
      }
    };
    toggle.addEventListener('click', function () { setOpen(drawer.hidden); });
    backdrop.addEventListener('click', function () { setOpen(false); });
    document.addEventListener('keydown', function (e) { if (e.key === 'Escape') setOpen(false); });
  }
})();
