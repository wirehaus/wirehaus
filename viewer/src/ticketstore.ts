const PREFIX = 'wirehaus.ticket.';
const TICKET_BYTES = 32;

export type TicketStorage = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>;

function defaultStorage(): TicketStorage | null {
  try {
    return typeof sessionStorage === 'undefined' ? null : sessionStorage;
  } catch {
    return null;
  }
}

function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

function fromHex(text: string): Uint8Array | null {
  if (text.length !== TICKET_BYTES * 2 || !/^[0-9a-f]+$/u.test(text)) {
    return null;
  }
  const out = new Uint8Array(TICKET_BYTES);
  for (let index = 0; index < TICKET_BYTES; index += 1) {
    out[index] = Number.parseInt(text.slice(index * 2, index * 2 + 2), 16);
  }
  return out;
}

export function loadTicket(origin: string, storage: TicketStorage | null = defaultStorage()): Uint8Array | null {
  try {
    const stored = storage?.getItem(PREFIX + origin);
    return typeof stored === 'string' ? fromHex(stored) : null;
  } catch {
    return null;
  }
}

export function saveTicket(
  origin: string,
  ticket: Uint8Array | null,
  storage: TicketStorage | null = defaultStorage(),
): void {
  try {
    if (ticket === null) {
      storage?.removeItem(PREFIX + origin);
    } else {
      storage?.setItem(PREFIX + origin, toHex(ticket));
    }
  } catch {
    return;
  }
}
