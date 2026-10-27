type InstallEvent = Event & { prompt: () => Promise<void>; userChoice: Promise<unknown> };

export type PwaTexts = { install: string; ios: string };

function standalone(): boolean {
  return (
    window.matchMedia('(display-mode: standalone)').matches ||
    (navigator as Navigator & { standalone?: boolean }).standalone === true
  );
}

function onIos(): boolean {
  return (
    /iphone|ipad|ipod/iu.test(navigator.userAgent) ||
    (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1)
  );
}

export function initPwa(button: HTMLButtonElement, texts: PwaTexts, notify: (text: string) => void): void {
  if ('serviceWorker' in navigator && (location.protocol === 'https:' || location.hostname === 'localhost')) {
    navigator.serviceWorker.register('/sw.js', { scope: '/' }).catch(() => undefined);
  }
  if (standalone()) {
    return;
  }
  button.textContent = texts.install;
  let deferred: InstallEvent | undefined;
  window.addEventListener('beforeinstallprompt', (event) => {
    event.preventDefault();
    deferred = event as InstallEvent;
    button.hidden = false;
  });
  window.addEventListener('appinstalled', () => {
    deferred = undefined;
    button.hidden = true;
  });
  if (onIos()) {
    button.hidden = false;
  }
  button.addEventListener('click', () => {
    if (deferred === undefined) {
      notify(texts.ios);
      return;
    }
    const event = deferred;
    deferred = undefined;
    button.hidden = true;
    void event.prompt().then(() => event.userChoice).catch(() => undefined);
  });
}

export function launchedAsApp(): boolean {
  return new URLSearchParams(location.search).get('source') === 'pwa' || standalone();
}
