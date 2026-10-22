import assert from 'node:assert/strict';
import { test } from 'node:test';

import { timestamp } from './download.js';

test('formats the file name stamp with zero padding in local time', () => {
  assert.equal(timestamp(new Date(2026, 0, 5, 3, 4, 9)), '20260105-030409');
  assert.equal(timestamp(new Date(2026, 11, 31, 23, 59, 59)), '20261231-235959');
});
