export type WakeDeps = {
  fetch: typeof fetch;
  sleep: (ms: number) => Promise<void>;
  now: () => number;
};

export const WAKE_DEADLINE_MS = 90_000;
export const REACH_DEADLINE_MS = 90_000;
const REACH_STREAK = 2;
const WAKE_POLL_MS = 2_000;
const REACH_POLL_MS = 1_500;
const REQUEST_TIMEOUT_MS = 8_000;
const HOST = /^([a-z0-9]([a-z0-9-]*[a-z0-9])?)\.[a-z0-9.-]+$/;

const realDeps: WakeDeps = {
  fetch: (input, init) => fetch(input, init),
  sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
  now: () => Date.now(),
};

export function agentName(authority: string): string | undefined {
  return HOST.exec(authority)?.[1];
}

async function askState(api: string, name: string, deps: WakeDeps): Promise<string | undefined> {
  try {
    const answer = await deps.fetch(`${api}/v1/wake`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name }),
      credentials: 'omit',
      cache: 'no-store',
      signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
    });
    if (!answer.ok) {
      return undefined;
    }
    const body = (await answer.json()) as { state?: unknown };
    return typeof body.state === 'string' ? body.state : undefined;
  } catch {
    return undefined;
  }
}

async function reachable(authority: string, deps: WakeDeps): Promise<boolean> {
  try {
    const answer = await deps.fetch(`https://${authority}/link/ping`, {
      mode: 'cors',
      credentials: 'omit',
      cache: 'no-store',
      signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
    });
    return answer.ok;
  } catch {
    return false;
  }
}

export async function ensureAwake(
  api: string,
  authority: string,
  waking: () => void,
  deps: WakeDeps = realDeps,
): Promise<boolean> {
  const name = agentName(authority);
  if (api === '' || name === undefined) {
    return false;
  }
  let state = await askState(api, name, deps);
  if (state !== 'waking') {
    return false;
  }
  waking();
  const wakeBy = deps.now() + WAKE_DEADLINE_MS;
  while (state === 'waking' && deps.now() < wakeBy) {
    await deps.sleep(WAKE_POLL_MS);
    state = await askState(api, name, deps);
  }
  if (state !== 'awake') {
    return true;
  }
  const reachBy = deps.now() + REACH_DEADLINE_MS;
  let streak = 0;
  while (deps.now() < reachBy) {
    streak = (await reachable(authority, deps)) ? streak + 1 : 0;
    if (streak >= REACH_STREAK) {
      return true;
    }
    await deps.sleep(REACH_POLL_MS);
  }
  return true;
}
