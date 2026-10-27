const sheets = new Map<HTMLStyleElement, CSSStyleSheet>();

function owner(node: Node): HTMLStyleElement | null {
  const element = node instanceof Element ? node : node.parentElement;
  const style = element?.closest('style');
  return style instanceof HTMLStyleElement ? style : null;
}

function sync(style: HTMLStyleElement): void {
  const css = style.textContent ?? '';
  if (!css.includes('xterm')) {
    return;
  }
  let sheet = sheets.get(style);
  if (sheet === undefined) {
    sheet = new CSSStyleSheet();
    sheets.set(style, sheet);
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
  }
  sheet.replaceSync(css);
}

function forget(style: HTMLStyleElement): void {
  const sheet = sheets.get(style);
  if (sheet !== undefined) {
    document.adoptedStyleSheets = document.adoptedStyleSheets.filter((other) => other !== sheet);
    sheets.delete(style);
  }
}

export function bridgeTerminalStyles(): void {
  if (typeof CSSStyleSheet === 'undefined' || !('replaceSync' in CSSStyleSheet.prototype)) {
    return;
  }
  new MutationObserver((records) => {
    for (const record of records) {
      for (const added of record.addedNodes) {
        if (added instanceof HTMLStyleElement) {
          sync(added);
        } else if (added instanceof Element) {
          added.querySelectorAll('style').forEach(sync);
        }
      }
      for (const removed of record.removedNodes) {
        if (removed instanceof HTMLStyleElement) {
          forget(removed);
        }
      }
      const style = owner(record.target);
      if (style !== null && (record.type === 'characterData' || record.type === 'childList')) {
        sync(style);
      }
    }
  }).observe(document, { childList: true, subtree: true, characterData: true });
}
