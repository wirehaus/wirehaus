import assert from 'node:assert/strict';
import test from 'node:test';

import { type TicketStorage, loadTicket, saveTicket } from './ticketstore.js';

function memory(): TicketStorage & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return {
    data,
    getItem: (key) => data.get(key) ?? null,
    setItem: (key, value) => void data.set(key, value),
    removeItem: (key) => void data.delete(key),
  };
}

test('a ticket survives a reload and is forgotten when cleared', () => {
  const storage = memory();
  const ticket = Uint8Array.from({ length: 32 }, (_, index) => index * 7);
  saveTicket('agent.example', ticket, storage);
  assert.deepEqual(loadTicket('agent.example', storage), ticket);
  assert.equal(loadTicket('other.example', storage), null);
  saveTicket('agent.example', null, storage);
  assert.equal(loadTicket('agent.example', storage), null);
});

test('damaged stored values and missing storage are ignored', () => {
  const storage = memory();
  for (const bad of ['', 'zz', 'ab'.repeat(31), 'AB'.repeat(32), `${'ab'.repeat(32)}00`]) {
    storage.setItem('wirehaus.ticket.agent.example', bad);
    assert.equal(loadTicket('agent.example', storage), null, bad);
  }
  assert.equal(loadTicket('agent.example', null), null);
  saveTicket('agent.example', new Uint8Array(32), null);
  const broken: TicketStorage = {
    getItem: () => {
      throw new Error('denied');
    },
    setItem: () => {
      throw new Error('denied');
    },
    removeItem: () => {
      throw new Error('denied');
    },
  };
  assert.equal(loadTicket('agent.example', broken), null);
  saveTicket('agent.example', new Uint8Array(32), broken);
});
