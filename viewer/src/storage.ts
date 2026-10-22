const DATABASE = 'wirehaus';
const STORE = 'identity';
const KEY = 'current';

export type Identity = {
  deviceKey: Uint8Array;
  agents: Record<string, Uint8Array>;
};

function open(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DATABASE, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(STORE);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error ?? new Error('cannot open the key store'));
  });
}

async function transaction<T>(
  mode: IDBTransactionMode,
  action: (store: IDBObjectStore) => IDBRequest<T>,
): Promise<T> {
  const database = await open();
  try {
    return await new Promise<T>((resolve, reject) => {
      const request = action(database.transaction(STORE, mode).objectStore(STORE));
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error ?? new Error('key store failure'));
    });
  } finally {
    database.close();
  }
}

export async function loadIdentity(): Promise<Identity | undefined> {
  const stored: unknown = await transaction('readonly', (store) => store.get(KEY));
  if (typeof stored !== 'object' || stored === null) {
    return undefined;
  }
  const candidate = stored as Partial<Identity>;
  if (!(candidate.deviceKey instanceof Uint8Array) || candidate.deviceKey.length !== 64) {
    return undefined;
  }
  return { deviceKey: candidate.deviceKey, agents: candidate.agents ?? {} };
}

export async function saveIdentity(identity: Identity): Promise<void> {
  await transaction('readwrite', (store) => store.put(identity, KEY));
}

export async function forgetIdentity(): Promise<void> {
  await transaction('readwrite', (store) => store.delete(KEY));
}
