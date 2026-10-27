import { linkAddress, parseLinkText } from './linkaddr.js';
import { ComputerLink, LinkApprover, type LinkEnd, drawQr, probeLink, startScanner } from './linkflow.js';
import type { LinkReply } from './linkprotocol.js';
import type { Messages } from './strings.js';

function byId<T extends HTMLElement>(id: string): T {
  const found = document.getElementById(id);
  if (found === null) {
    throw new Error(`missing element ${id}`);
  }
  return found as T;
}

export type LinkUiDeps = {
  messages: Messages;
  base: () => string | undefined;
  name: () => string | undefined;
  sendLink: (payload: Uint8Array) => void;
  pair: (code: string) => void;
  rerender: () => void;
  closeMenu: () => void;
};

export type LinkUi = {
  probe: () => Promise<void>;
  kind: (kind: string) => void;
  active: () => boolean;
  receive: (reply: LinkReply) => void;
};

export function initLinkUi(deps: LinkUiDeps): LinkUi {
  const m = deps.messages;
  const labels: [string, string][] = [
    ['link-or', m.linkOr],
    ['link-start', m.linkStart],
    ['linkpc-title', m.linkPcTitle],
    ['linkpc-intro', m.linkPcIntro],
    ['linkpc-manual-label', m.linkPcManual],
    ['linkpc-verify-label', m.linkPcVerify],
    ['linkpc-retry', m.linkPcRetry],
    ['linkpc-cancel', m.linkPcCancel],
    ['link-open', m.linkOpen],
    ['link-title', m.linkOpen],
    ['link-intro', m.linkIntro],
    ['link-scan', m.linkScan],
    ['link-manual-label', m.linkManualLabel],
    ['link-find', m.linkFind],
    ['link-verify-text', m.linkVerifyText],
    ['link-code-label', m.linkCodeLabel],
    ['link-code-hint', m.linkCodeHint],
    ['link-approve', m.linkApprove],
    ['link-done', m.linkDone],
    ['link-close', m.linkClose],
  ];
  for (const [id, text] of labels) {
    byId(id).textContent = text;
  }

  const offer = byId('link-offer');
  const openButton = byId<HTMLButtonElement>('link-open');
  const pcQr = byId<HTMLCanvasElement>('linkpc-qr');
  const pcStatus = byId('linkpc-status');
  const pcError = byId('linkpc-error');
  const pcRetry = byId<HTMLButtonElement>('linkpc-retry');
  const dialog = byId<HTMLDialogElement>('link-dialog');
  const stepFind = byId('link-step-find');
  const stepConfirm = byId('link-step-confirm');
  const stepDone = byId('link-step-done');
  const scanBox = byId('link-scanbox');
  const video = byId<HTMLVideoElement>('link-video');
  const manual = byId<HTMLInputElement>('link-manual');
  const codeInput = byId<HTMLInputElement>('link-code');
  const dialogError = byId('link-error');
  const approveButton = byId<HTMLButtonElement>('link-approve');

  let supported: boolean | undefined;
  let lastKind = 'loading';
  let computer: ComputerLink | undefined;
  let active = false;
  let countdown: number | undefined;
  let scanner: { stop: () => void } | undefined;
  let pending: Uint8Array | undefined;
  const approver = new LinkApprover((payload) => deps.sendLink(payload));

  function refresh(): void {
    offer.hidden = !(supported === true && lastKind === 'needsPairing');
    openButton.hidden = !(supported === true && lastKind === 'connected');
  }

  function stopCountdown(): void {
    window.clearInterval(countdown);
    countdown = undefined;
  }

  function endMessage(reason: LinkEnd): string {
    switch (reason) {
      case 'busy':
        return m.linkPcBusy;
      case 'slow':
        return m.linkPcSlow;
      case 'expired':
        return m.linkPcExpired;
      default:
        return m.linkPcFailed;
    }
  }

  function startPc(): void {
    const base = deps.base();
    if (base === undefined) {
      return;
    }
    computer?.cancel();
    stopCountdown();
    active = true;
    deps.rerender();
    pcError.textContent = '';
    pcStatus.textContent = m.linkPcWaiting;
    pcRetry.hidden = true;
    byId('linkpc-manual').textContent = '';
    byId('linkpc-verify').textContent = '';
    pcQr.hidden = true;
    computer = new ComputerLink(
      base,
      {
        started: (info) => {
          const shown = info.link !== undefined && drawQr(pcQr, info.link);
          pcQr.hidden = !shown;
          byId('linkpc-manual').textContent = info.manual;
          byId('linkpc-verify').textContent = info.verify;
          const end = Date.now() + info.ttl * 1000;
          const tick = (): void => {
            const left = Math.max(0, Math.ceil((end - Date.now()) / 1000));
            pcStatus.textContent = `${m.linkPcWaiting} ${m.linkPcCountdown(left)}`;
          };
          tick();
          countdown = window.setInterval(tick, 1000);
        },
        code: (pairingCode) => {
          stopCountdown();
          pcStatus.textContent = m.linkPcApproved;
          active = false;
          deps.rerender();
          deps.pair(pairingCode);
        },
        ended: (reason) => {
          stopCountdown();
          if (reason === 'cancelled') {
            return;
          }
          pcStatus.textContent = '';
          pcError.textContent = endMessage(reason);
          pcRetry.hidden = false;
        },
      },
      (id) => {
        const name = deps.name();
        return name === undefined ? undefined : linkAddress(location.origin, name, id);
      },
    );
    void computer.start();
  }

  byId('link-start').addEventListener('click', startPc);
  pcRetry.addEventListener('click', startPc);
  byId('linkpc-cancel').addEventListener('click', () => {
    computer?.cancel();
    stopCountdown();
    active = false;
    deps.rerender();
  });

  function stopScan(): void {
    scanner?.stop();
    scanner = undefined;
    scanBox.hidden = true;
  }

  function resetDialog(): void {
    stopScan();
    pending = undefined;
    stepFind.hidden = false;
    stepConfirm.hidden = true;
    stepDone.hidden = true;
    dialogError.textContent = '';
    manual.value = '';
    codeInput.value = '';
    approveButton.disabled = false;
  }

  function failureText(reason: string): string {
    switch (reason) {
      case 'wrongCode':
        return m.linkWrongCode;
      case 'tooManyAttempts':
        return m.linkTooMany;
      case 'gone':
        return m.linkGone;
      default:
        return m.linkUnavailable;
    }
  }

  async function find(text: string): Promise<void> {
    dialogError.textContent = '';
    const name = deps.name() ?? '';
    const parsed = parseLinkText(text, location.origin, name);
    if (parsed.error === 'other') {
      let other = '';
      try {
        other = new URL(text.trim()).pathname.split('/').filter((part) => part !== '')[0] ?? '';
      } catch {
        other = '';
      }
      dialogError.textContent = m.linkOther(other);
      return;
    }
    const key = parsed.id ?? parsed.manual;
    if (key === undefined) {
      dialogError.textContent = m.linkInvalid;
      return;
    }
    try {
      const reply = await approver.lookup(key);
      if (reply.kind === 'info') {
        stopScan();
        pending = reply.id;
        byId('link-verification').textContent = reply.verification;
        stepFind.hidden = true;
        stepConfirm.hidden = false;
        codeInput.focus();
      } else if (reply.kind === 'notFound') {
        dialogError.textContent = m.linkNotFound;
      } else if (reply.kind === 'failed') {
        dialogError.textContent = failureText(reply.reason);
      }
    } catch {
      dialogError.textContent = m.linkUnavailable;
    }
  }

  byId('link-find').addEventListener('click', () => void find(manual.value));
  manual.addEventListener('keydown', (event) => {
    if (event.key === 'Enter') {
      event.preventDefault();
      void find(manual.value);
    }
  });
  byId('link-scan').addEventListener('click', () => {
    dialogError.textContent = '';
    stopScan();
    scanBox.hidden = false;
    void startScanner(video, (text) => {
      stopScan();
      void find(text);
    }).then((started) => {
      if (typeof started === 'string') {
        scanBox.hidden = true;
        dialogError.textContent = m.linkScanDenied;
        return;
      }
      if (scanBox.hidden) {
        started.stop();
        return;
      }
      scanner = started;
      dialogError.textContent = '';
    });
  });

  codeInput.addEventListener('input', () => {
    codeInput.value = codeInput.value.replace(/\D/gu, '').slice(0, 6);
    if (codeInput.value.length === 6) {
      void approve();
    }
  });
  approveButton.addEventListener('click', () => void approve());

  async function approve(): Promise<void> {
    if (pending === undefined || !/^[0-9]{6}$/u.test(codeInput.value) || approveButton.disabled) {
      return;
    }
    approveButton.disabled = true;
    dialogError.textContent = '';
    try {
      const reply = await approver.approve(pending, codeInput.value);
      if (reply.kind === 'approved') {
        stepConfirm.hidden = true;
        stepDone.hidden = false;
      } else if (reply.kind === 'failed') {
        dialogError.textContent = failureText(reply.reason);
        codeInput.value = '';
        if (reply.reason === 'gone') {
          pending = undefined;
          stepConfirm.hidden = true;
          stepFind.hidden = false;
        }
      }
    } catch {
      dialogError.textContent = m.linkUnavailable;
    }
    approveButton.disabled = false;
  }

  openButton.addEventListener('click', () => {
    deps.closeMenu();
    resetDialog();
    dialog.showModal();
  });
  byId('link-close').addEventListener('click', () => dialog.close());
  dialog.addEventListener('close', resetDialog);

  return {
    probe: async () => {
      const base = deps.base();
      supported = base === undefined ? false : await probeLink(base);
      refresh();
    },
    kind: (kind) => {
      lastKind = kind;
      if (kind !== 'connected' && dialog.open) {
        dialog.close();
      }
      refresh();
    },
    active: () => active,
    receive: (reply) => approver.receive(reply),
  };
}
