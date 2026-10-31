import { installReady, isStandalone, platform, promptInstall } from './pwa.js';
import type { Messages } from './strings.js';

const NEVER_KEY = 'wh.installHint';
const SNOOZE_KEY = 'wh.installSnooze';
const SNOOZE_MS = 3 * 24 * 60 * 60 * 1000;
const DELAY_MS = 2500;

export type InstallSheet = {
  offer: () => void;
  open: () => void;
  openForPairing: (inBrowser: () => void) => void;
  close: () => void;
  refresh: () => void;
};

function byId<T extends HTMLElement>(id: string): T {
  const found = document.getElementById(id);
  if (found === null) {
    throw new Error(`missing element #${id}`);
  }
  return found as T;
}

function stored(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function store(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    return;
  }
}

function storageWorks(): boolean {
  try {
    localStorage.setItem('wh.probe', '1');
    localStorage.removeItem('wh.probe');
    return true;
  } catch {
    return false;
  }
}

function allowed(): boolean {
  if (!storageWorks() || stored(NEVER_KEY) === '1') {
    return false;
  }
  const until = Number(stored(SNOOZE_KEY) ?? '0');
  return !(Number.isFinite(until) && until > Date.now());
}

export function initInstallSheet(messages: Messages, touch: boolean): InstallSheet {
  const dialog = byId<HTMLDialogElement>('install-dialog');
  const steps = byId<HTMLOListElement>('install-steps');
  const go = byId<HTMLButtonElement>('install-go');
  const session = byId('install-session');
  byId('install-title').textContent = messages.installTitle;
  byId('install-lead').textContent = messages.installLead;
  go.textContent = messages.installGo;
  byId('install-later').textContent = messages.installLater;
  byId('install-never').textContent = messages.installNever;
  const browser = byId<HTMLButtonElement>('install-browser');
  const later = byId('install-later');
  const never = byId('install-never');
  browser.textContent = messages.installPairBrowser;
  let offered = false;
  let pairing: (() => void) | undefined;
  let quiet = false;

  function render(): void {
    const ios = platform() === 'ios';
    const native = installReady();
    go.hidden = !native;
    steps.hidden = native;
    const waiting = pairing !== undefined;
    byId('install-title').textContent = waiting ? messages.installPairTitle : messages.installTitle;
    byId('install-lead').textContent = waiting ? messages.installPairLead : messages.installLead;
    browser.hidden = !waiting;
    later.hidden = waiting;
    never.hidden = waiting;
    session.hidden = !ios || waiting;
    session.textContent = ios && !waiting ? messages.installSession : '';
    steps.replaceChildren();
    if (native) {
      return;
    }
    const icons = ios ? ['i-share', 'i-add-home', 'i-app'] : ['i-dots', 'i-add-home', 'i-app'];
    (ios ? messages.installStepsIos : messages.installStepsOther).forEach((text, index) => {
      const item = document.createElement('li');
      const badge = document.createElement('span');
      badge.className = 'step-icon';
      badge.setAttribute('aria-hidden', 'true');
      const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
      const use = document.createElementNS('http://www.w3.org/2000/svg', 'use');
      use.setAttribute('href', `#${icons[index] ?? 'i-app'}`);
      svg.append(use);
      badge.append(svg);
      const label = document.createElement('span');
      label.textContent = text;
      item.append(badge, label);
      steps.append(item);
    });
  }

  function open(): void {
    if (dialog.open) {
      return;
    }
    render();
    dialog.showModal();
    byId('install-title').focus();
  }

  function openForPairing(inBrowser: () => void): void {
    pairing = inBrowser;
    open();
  }

  function close(): void {
    if (dialog.open) {
      dialog.close();
    }
  }

  go.addEventListener('click', () => {
    void promptInstall().then((outcome) => {
      if (outcome !== 'unavailable') {
        close();
      }
      if (outcome === 'dismissed') {
        store(SNOOZE_KEY, String(Date.now() + SNOOZE_MS));
      }
    });
  });
  browser.addEventListener('click', () => {
    const proceed = pairing;
    pairing = undefined;
    quiet = true;
    close();
    quiet = false;
    proceed?.();
  });
  later.addEventListener('click', () => {
    store(SNOOZE_KEY, String(Date.now() + SNOOZE_MS));
    close();
  });
  never.addEventListener('click', () => {
    store(NEVER_KEY, '1');
    close();
  });
  dialog.addEventListener('cancel', (event) => {
    if (pairing !== undefined) {
      event.preventDefault();
    }
  });
  dialog.addEventListener('close', () => {
    if (pairing !== undefined) {
      window.setTimeout(() => dialog.open || pairing === undefined || dialog.showModal(), 0);
      return;
    }
    if (!quiet && stored(NEVER_KEY) !== '1') {
      store(SNOOZE_KEY, String(Date.now() + SNOOZE_MS));
    }
  });
  dialog.addEventListener('click', (event) => {
    if (event.target === dialog && pairing === undefined) {
      close();
    }
  });

  function offer(): void {
    if (offered || isStandalone() || !allowed() || !touch) {
      return;
    }
    offered = true;
    window.setTimeout(() => {
      if (!isStandalone() && allowed() && !document.querySelector('dialog[open]')) {
        open();
      }
    }, DELAY_MS);
  }

  function refresh(): void {
    if (dialog.open) {
      render();
    }
  }

  return { offer, open, openForPairing, close, refresh };
}
