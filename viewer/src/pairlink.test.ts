import assert from 'node:assert/strict';
import test from 'node:test';

import { pairCodeFromHash, withoutPairCode } from './pairlink.js';

test('the pairing code is read from the fragment and normalised', () => {
  assert.equal(
    pairCodeFromHash('#pair=yxq7-h74e-izau-ajop-ojfr-cf2o-fhkb-ivpa'),
    'YXQ7-H74E-IZAU-AJOP-OJFR-CF2O-FHKB-IVPA',
  );
});

test('anything that does not look like a code is ignored', () => {
  assert.equal(pairCodeFromHash(''), undefined);
  assert.equal(pairCodeFromHash('#pair='), undefined);
  assert.equal(pairCodeFromHash('#pair=short'), undefined);
  assert.equal(pairCodeFromHash('#pair=<script>alert(1)</script>0000'), undefined);
  assert.equal(pairCodeFromHash('#pair=AAAA-BBBB-CCCC-DDDD&pair=AAAA-BBBB-CCCC-EEEE'), undefined);
});

test('only the code is removed from the fragment', () => {
  assert.equal(withoutPairCode('#pair=AAAA-BBBB-CCCC-DDDD'), '');
  assert.equal(withoutPairCode('#agent=host&pair=AAAA-BBBB-CCCC-DDDD'), '#agent=host');
});
