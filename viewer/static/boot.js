(function () {
  var KEY = 'wh.heal';
  var done = false;
  var spanish = /^es/i.test(navigator.language || '');

  var reloadTimer = 0;

  function attempts() {
    try {
      var stored = sessionStorage.getItem(KEY);
      if (stored !== null) {
        return Number(stored) || 0;
      }
    } catch (error) {
      void error;
    }
    var match = /wh\.heal=(\d+)/.exec(window.name || '');
    return match ? Number(match[1]) : 0;
  }

  function remember(value) {
    var cleaned = (window.name || '').replace(/wh\.heal=\d+;?/, '');
    try {
      if (value === null) {
        sessionStorage.removeItem(KEY);
      } else {
        sessionStorage.setItem(KEY, String(value));
      }
    } catch (error) {
      void error;
    }
    window.name = value === null || value === 0 ? cleaned : cleaned + 'wh.heal=' + value + ';';
  }

  function note(text, retry) {
    var box = document.getElementById('boot-note');
    if (!box) {
      return;
    }
    box.hidden = false;
    var label = box.querySelector('span');
    if (label) {
      label.textContent = text;
    }
    var button = box.querySelector('button');
    if (button) {
      button.hidden = !retry;
      button.textContent = spanish ? 'Reintentar' : 'Retry';
      button.onclick = function () {
        remember(0);
        location.reload();
      };
    }
  }

  function heal() {
    if (done) {
      return;
    }
    done = true;
    var tries = attempts();
    if (tries >= 4) {
      note(
        spanish
          ? 'No se ha podido cargar la versión nueva. Pulsa Reintentar o recarga con Ctrl+Mayús+R.'
          : 'The new version could not be loaded. Press Retry or reload with Ctrl+Shift+R.',
        true,
      );
      return;
    }
    remember(tries + 1);
    note(spanish ? 'Actualizando Wirehaus…' : 'Updating Wirehaus…', false);
    reloadTimer = setTimeout(function () {
      location.reload();
    }, 2500 + tries * 3000);
  }

  window.addEventListener(
    'error',
    function (event) {
      var target = event.target;
      if (target && target.tagName === 'SCRIPT') {
        heal();
      }
    },
    true,
  );

  window.addEventListener('wirehaus-ready', function () {
    done = true;
    clearTimeout(reloadTimer);
    remember(null);
    var box = document.getElementById('boot-note');
    if (box) {
      box.hidden = true;
    }
  });

  setTimeout(function () {
    if (!done) {
      heal();
    }
  }, 15000);
})();
