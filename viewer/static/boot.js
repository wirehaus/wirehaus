(function () {
  var KEY = 'wh.heal';
  var done = false;
  var spanish = /^es/i.test(navigator.language || '');

  function attempts() {
    try {
      return Number(sessionStorage.getItem(KEY) || '0');
    } catch (error) {
      return 0;
    }
  }

  function remember(value) {
    try {
      if (value === null) {
        sessionStorage.removeItem(KEY);
      } else {
        sessionStorage.setItem(KEY, String(value));
      }
    } catch (error) {
      return;
    }
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
    setTimeout(function () {
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
    remember(null);
  });

  setTimeout(function () {
    if (!done) {
      heal();
    }
  }, 15000);
})();
