type InstallEvent = Event & { prompt: () => Promise<void>; userChoice: Promise<{ outcome?: string }> };

export type Platform = 'ios' | 'android' | 'desktop';

export type InstallOutcome = 'accepted' | 'dismissed' | 'unavailable';

export type PwaHooks = { label: string; open: () => void; ready: () => void };

export function isStandalone(): boolean {
  return (
    window.matchMedia('(display-mode: standalone)').matches ||
    (navigator as Navigator & { standalone?: boolean }).standalone === true
  );
}

export function platform(): Platform {
  const agent = navigator.userAgent;
  if (/iphone|ipad|ipod/iu.test(agent) || (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1)) {
    return 'ios';
  }
  return /android/iu.test(agent) ? 'android' : 'desktop';
}

let deferred: InstallEvent | undefined;

function dropManifestOnIos(): void {
  if (platform() === 'ios' && !isStandalone()) {
    document.querySelectorAll('link[rel="manifest"]').forEach((node) => node.remove());
  }
}

export function installReady(): boolean {
  return deferred !== undefined;
}

export async function promptInstall(): Promise<InstallOutcome> {
  const event = deferred;
  if (event === undefined) {
    return 'unavailable';
  }
  deferred = undefined;
  try {
    await event.prompt();
    const choice = await event.userChoice;
    return choice.outcome === 'accepted' ? 'accepted' : 'dismissed';
  } catch {
    return 'dismissed';
  }
}

export function initPwa(button: HTMLButtonElement, hooks: PwaHooks): void {
  dropManifestOnIos();
  if ('serviceWorker' in navigator && (location.protocol === 'https:' || location.hostname === 'localhost')) {
    navigator.serviceWorker.register('/sw.js', { scope: '/' }).catch(() => undefined);
  }
  if (isStandalone()) {
    navigator.storage?.persist?.().catch(() => undefined);
    return;
  }
  button.textContent = hooks.label;
  window.addEventListener('beforeinstallprompt', (event) => {
    event.preventDefault();
    deferred = event as InstallEvent;
    button.hidden = false;
    hooks.ready();
  });
  window.addEventListener('appinstalled', () => {
    deferred = undefined;
    button.hidden = true;
  });
  if (platform() === 'ios') {
    button.hidden = false;
  }
  button.addEventListener('click', hooks.open);
}

export function launchedAsApp(): boolean {
  return new URLSearchParams(location.search).get('source') === 'pwa' || isStandalone();
}
