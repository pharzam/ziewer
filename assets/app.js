(function () {
  var dark = window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches;
  if (window.mermaid) {
    window.mermaid.initialize({ startOnLoad: true, theme: dark ? 'dark' : 'default' });
  }
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
  // Drag a local .md file onto the page to view it here. It is rendered by the server
  // (POST /render) and never saved.
  if (document.body.getAttribute('data-drop') === '1') {
    var overlay = document.createElement('div');
    overlay.className = 'drop-overlay';
    overlay.hidden = true;
    overlay.textContent = 'Drop a Markdown file to view it';
    document.body.appendChild(overlay);
    var depth = 0;
    var hasFiles = function (e) {
      return e.dataTransfer && Array.prototype.indexOf.call(e.dataTransfer.types, 'Files') !== -1;
    };
    document.addEventListener('dragenter', function (e) {
      if (!hasFiles(e)) return;
      depth++;
      overlay.hidden = false;
    });
    document.addEventListener('dragleave', function () {
      depth = Math.max(0, depth - 1);
      if (depth === 0) overlay.hidden = true;
    });
    document.addEventListener('dragover', function (e) {
      if (hasFiles(e)) e.preventDefault();
    });
    document.addEventListener('drop', function (e) {
      if (!hasFiles(e)) return;
      e.preventDefault();
      depth = 0;
      overlay.hidden = true;
      var file = e.dataTransfer.files[0];
      if (!file) return;
      if (!/\.(md|markdown|mdown|mkd|txt)$/i.test(file.name)) {
        showDropError('Only Markdown files (.md) can be dropped here: ' + file.name);
        return;
      }
      file.text().then(function (text) {
        return fetch('/render', {
          method: 'POST',
          headers: { 'Content-Type': 'text/markdown; charset=utf-8' },
          body: text
        });
      }).then(function (res) {
        if (res.status === 413) throw new Error('File is too large (max 5 MB).');
        if (!res.ok) throw new Error('Could not render the file (HTTP ' + res.status + ').');
        return res.text();
      }).then(function (html) {
        showDropped(file.name, html);
      }).catch(function (err) {
        showDropError(err.message);
      });
    });
  }

  function mainEl() {
    return document.querySelector('main.content');
  }

  function banner(text, cls) {
    var b = document.createElement('div');
    b.className = 'notice drop-banner ' + (cls || '');
    var span = document.createElement('span');
    span.textContent = text;
    b.appendChild(span);
    var back = document.createElement('button');
    back.type = 'button';
    back.textContent = 'Close';
    back.addEventListener('click', function () { location.reload(); });
    b.appendChild(back);
    return b;
  }

  function showDropped(name, html) {
    var main = mainEl();
    if (!main) return;
    main.innerHTML = '';
    main.appendChild(banner('Viewing dropped file “' + name + '” (not saved on the server)'));
    var article = document.createElement('article');
    article.className = 'markdown-body card';
    article.innerHTML = html;
    main.appendChild(article);
    document.title = name + ' · Ziewer';
    window.scrollTo(0, 0);
    if (window.mermaid) {
      window.mermaid.run({ nodes: article.querySelectorAll('.mermaid') });
    }
  }

  function showDropError(message) {
    var main = mainEl();
    if (!main) return;
    var old = main.querySelector('.drop-banner.error');
    if (old) old.remove();
    main.insertBefore(banner(message, 'error'), main.firstChild);
  }
})();
