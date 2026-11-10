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
  // On small screens start with the file tree collapsed.
  if (window.matchMedia && window.matchMedia('(max-width: 768px)').matches) {
    var side = document.querySelector('.sidebar > details');
    if (side) side.removeAttribute('open');
  }
})();
