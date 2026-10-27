const CODE = /^[A-Za-z2-7-]{16,64}$/u;

export function pairCodeFromHash(hash: string): string | undefined {
  const values = new URLSearchParams(hash.replace(/^#/u, '')).getAll('pair');
  const value = values.length === 1 ? values[0] : undefined;
  return value !== undefined && CODE.test(value) ? value.toUpperCase() : undefined;
}

export function withoutPairCode(hash: string): string {
  const params = new URLSearchParams(hash.replace(/^#/u, ''));
  params.delete('pair');
  const rest = params.toString();
  return rest === '' ? '' : `#${rest}`;
}
