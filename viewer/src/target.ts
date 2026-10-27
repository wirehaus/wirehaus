export type Target = {
  authority: string;
  origin: string;
  socketUrl: string;
  foreign: boolean;
};

export type TargetError = 'missing' | 'invalid';

export type PageLocation = {
  protocol: string;
  host: string;
  pathname: string;
  hash: string;
};

const MAX_HOST_LENGTH = 253;
const MAX_LABEL_LENGTH = 63;
const LABEL = /^[a-z0-9]([a-z0-9-]*[a-z0-9])?$/;
const PORT = /^[0-9]{1,5}$/;
const RESERVED_NAMES = new Set(['app', 'www', 'api', 'admin', 'mail']);
const SCHEMES = ['https://', 'wss://'];

function validHost(host: string): boolean {
  if (host.length === 0 || host.length > MAX_HOST_LENGTH) {
    return false;
  }
  return host.split('.').every((label) => label.length <= MAX_LABEL_LENGTH && LABEL.test(label));
}

function validPort(port: string): boolean {
  if (!PORT.test(port)) {
    return false;
  }
  const value = Number(port);
  return value >= 1 && value <= 65535;
}

function isLoopback(host: string): boolean {
  return host === 'localhost' || /^127(\.[0-9]{1,3}){3}$/.test(host);
}

export function parseAgentAddress(input: string): string | undefined {
  let rest = input.trim().toLowerCase();
  for (const scheme of SCHEMES) {
    if (rest.startsWith(scheme)) {
      rest = rest.slice(scheme.length);
      break;
    }
  }
  if (rest.endsWith('/')) {
    rest = rest.slice(0, -1);
  }
  const parts = rest.split(':');
  if (parts.length > 2) {
    return undefined;
  }
  const [host = '', port] = parts;
  if (!validHost(host) || (port !== undefined && !validPort(port))) {
    return undefined;
  }
  return rest;
}

function hostOf(authority: string): string {
  return authority.split(':')[0] ?? authority;
}

function foreignTarget(authority: string): Target {
  const secure = !isLoopback(hostOf(authority));
  const web = secure ? 'https' : 'http';
  const socket = secure ? 'wss' : 'ws';
  return {
    authority,
    origin: `${web}://${authority}`,
    socketUrl: `${socket}://${authority}/ws`,
    foreign: true,
  };
}

function ownTarget(page: PageLocation): Target {
  const secure = page.protocol === 'https:';
  return {
    authority: page.host,
    origin: `${secure ? 'https' : 'http'}://${page.host}`,
    socketUrl: `${secure ? 'wss' : 'ws'}://${page.host}/ws`,
    foreign: false,
  };
}

function agentFromPath(page: PageLocation): string | undefined {
  const match = /^\/([A-Za-z0-9-]+)\/?$/.exec(page.pathname);
  const name = match?.[1]?.toLowerCase();
  const dot = page.host.indexOf('.');
  if (name === undefined || dot === -1 || !LABEL.test(name) || name.length > MAX_LABEL_LENGTH || RESERVED_NAMES.has(name)) {
    return undefined;
  }
  return parseAgentAddress(`${name}${page.host.slice(dot)}`);
}

export function resolveTarget(page: PageLocation, configurable: boolean): Target | TargetError {
  if (!configurable) {
    return ownTarget(page);
  }
  const requested = new URLSearchParams(page.hash.replace(/^#/, '')).getAll('agent');
  if (requested.length === 0) {
    const named = agentFromPath(page);
    if (named !== undefined) {
      return foreignTarget(named);
    }
    return page.pathname.replace(/\/+$/, '') === '' ? 'missing' : 'invalid';
  }
  const authority = requested.length === 1 ? parseAgentAddress(requested[0] ?? '') : undefined;
  return authority === undefined ? 'invalid' : foreignTarget(authority);
}

const NAME = /^(?!\d+$)[a-z0-9]+(-[a-z0-9]+)*$/;

export function validAccessName(name: string): boolean {
  return name.length >= 3 && name.length <= 32 && NAME.test(name) && !RESERVED_NAMES.has(name);
}

export function cleanName(input: string): string {
  return input
    .trim()
    .toLowerCase()
    .replace(/^https?:\/\//, '')
    .replace(/^app\.wirehaus\.dev\//, '')
    .replace(/\.wirehaus\.dev.*$/, '')
    .replace(/\/+$/, '');
}

export function nameFromPath(pathname: string): string | undefined {
  const first = pathname.split('/').filter((part) => part !== '')[0];
  return first === undefined ? undefined : decodeURIComponent(first).toLowerCase();
}

export function namedByPath(page: PageLocation): string | undefined {
  const hasHash = new URLSearchParams(page.hash.replace(/^#/, '')).getAll('agent').length > 0;
  if (hasHash) {
    return undefined;
  }
  const match = /^\/([A-Za-z0-9-]+)\/?$/.exec(page.pathname);
  return match?.[1]?.toLowerCase();
}
