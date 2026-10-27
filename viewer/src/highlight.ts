export type Language =
  | 'plain'
  | 'html'
  | 'css'
  | 'js'
  | 'json'
  | 'python'
  | 'shell'
  | 'rust'
  | 'go'
  | 'c'
  | 'java'
  | 'php'
  | 'sql'
  | 'yaml'
  | 'toml'
  | 'markdown';

export type Token = { from: number; to: number; cls: string };

export const LANGUAGES: readonly Language[] = [
  'plain', 'html', 'css', 'js', 'json', 'python', 'shell', 'rust', 'go', 'c', 'java', 'php', 'sql', 'yaml', 'toml', 'markdown',
];

const LABELS: Record<Language, string> = {
  plain: 'Texto',
  html: 'HTML',
  css: 'CSS',
  js: 'JavaScript / TypeScript',
  json: 'JSON',
  python: 'Python',
  shell: 'Shell',
  rust: 'Rust',
  go: 'Go',
  c: 'C / C++ / C#',
  java: 'Java / Kotlin',
  php: 'PHP',
  sql: 'SQL',
  yaml: 'YAML',
  toml: 'TOML / INI',
  markdown: 'Markdown',
};

export const languageLabel = (language: Language): string => LABELS[language];

export const MAX_HIGHLIGHT_CHARS = 200_000;
const TIME_BUDGET_MS = 120;
let deadline = Number.POSITIVE_INFINITY;
const expired = (): boolean => performance.now() > deadline;

const BY_EXTENSION: Record<string, Language> = {
  html: 'html', htm: 'html', xml: 'html', svg: 'html', vue: 'html', svelte: 'html', xhtml: 'html',
  css: 'css', scss: 'css', less: 'css',
  js: 'js', mjs: 'js', cjs: 'js', ts: 'js', tsx: 'js', jsx: 'js',
  json: 'json', jsonc: 'json', webmanifest: 'json',
  py: 'python', pyw: 'python',
  sh: 'shell', bash: 'shell', zsh: 'shell', fish: 'shell', env: 'shell', service: 'toml', desktop: 'toml',
  rs: 'rust',
  go: 'go',
  c: 'c', h: 'c', cpp: 'c', hpp: 'c', cc: 'c', cs: 'c',
  java: 'java', kt: 'java', swift: 'java',
  php: 'php',
  sql: 'sql',
  yml: 'yaml', yaml: 'yaml',
  toml: 'toml', ini: 'toml', cfg: 'toml', conf: 'toml',
  md: 'markdown', markdown: 'markdown',
};

const BY_NAME: Record<string, Language> = {
  makefile: 'shell', dockerfile: 'shell', '.bashrc': 'shell', '.zshrc': 'shell', '.profile': 'shell', '.bash_profile': 'shell',
  '.gitignore': 'shell', '.env': 'shell',
};

export function detectLanguage(name: string, text: string): Language {
  const lower = name.toLowerCase();
  const byName = BY_NAME[lower];
  if (byName !== undefined) {
    return byName;
  }
  const dot = lower.lastIndexOf('.');
  const byExtension = dot > 0 ? BY_EXTENSION[lower.slice(dot + 1)] : undefined;
  if (byExtension !== undefined) {
    return byExtension;
  }
  const first = text.slice(0, 200);
  if (first.startsWith('#!')) {
    if (/python/u.test(first)) return 'python';
    if (/node|deno|bun/u.test(first)) return 'js';
    return 'shell';
  }
  const trimmed = first.trimStart();
  if (/^<(!doctype|html|\?xml|svg)/iu.test(trimmed)) return 'html';
  if (/^[{[]/u.test(trimmed) && /"\s*:/u.test(first)) return 'json';
  if (trimmed.startsWith('<?php')) return 'php';
  return 'plain';
}

type Out = Token[];
type Matcher = (text: string, pos: number) => { length: number; cls: string } | null;

const words = (list: string): Set<string> => new Set(list.split(/\s+/u).filter((word) => word !== ''));

const JS_KEYWORDS = words(`break case catch class const continue debugger default delete do else enum export extends finally for
  function if import in instanceof let new of return static super switch this throw try typeof var void while with yield async await
  from as get set type interface implements namespace declare abstract readonly public private protected keyof satisfies`);
const JS_LITERALS = words('true false null undefined NaN Infinity');
const PY_KEYWORDS = words(`and as assert async await break class continue def del elif else except finally for from global if import
  in is lambda nonlocal not or pass raise return try while with yield match case`);
const PY_LITERALS = words('True False None');
const PY_BUILTINS = words(`print len range int str float list dict set tuple bool type isinstance open super self cls enumerate zip map
  filter sorted sum min max abs any all input id iter next repr round reversed`);
const SH_KEYWORDS = words('if then else elif fi for while until do done case esac in function select time return exit break continue local export readonly unset');
const SH_BUILTINS = words('echo cd printf read set shift test source alias eval exec trap wait kill sudo grep sed awk cat ls rm cp mv mkdir chmod chown curl tar git systemctl');
const RUST_KEYWORDS = words(`as async await break const continue crate dyn else enum extern fn for if impl in let loop match mod move mut pub
  ref return self Self static struct super trait type unsafe use where while`);
const GO_KEYWORDS = words(`break case chan const continue default defer else fallthrough for func go goto if import interface map package
  range return select struct switch type var`);
const GO_TYPES = words('string int int8 int16 int32 int64 uint uint8 uint16 uint32 uint64 float32 float64 bool byte rune error any');
const C_KEYWORDS = words(`auto break case const continue default do else enum extern for goto if inline register return sizeof static struct
  switch typedef union volatile while class namespace template typename public private protected virtual override new delete this using
  try catch throw operator explicit constexpr noexcept nullptr var async await foreach in is as out ref params readonly sealed string`);
const C_TYPES = words('void int char short long float double unsigned signed bool size_t uint8_t uint16_t uint32_t uint64_t int8_t int16_t int32_t int64_t');
const JAVA_KEYWORDS = words(`abstract assert break case catch class const continue default do else enum extends final finally for goto if
  implements import instanceof interface native new package private protected public return static strictfp super switch synchronized this
  throw throws transient try volatile while fun val var when object companion data sealed override lateinit suspend inline let guard func
  init deinit extension protocol struct`);
const JAVA_TYPES = words('int long short byte char float double boolean void String Integer Long Boolean Object List Map Set Int Unit Any');
const PHP_KEYWORDS = words(`abstract and array as break callable case catch class clone const continue declare default do echo else elseif
  empty enddeclare endfor endforeach endif endswitch endwhile extends final finally fn for foreach function global if implements include
  include_once instanceof insteadof interface isset list match namespace new or print private protected public readonly require
  require_once return static switch throw trait try unset use var while xor yield`);
const SQL_KEYWORDS = words(`select from where insert into values update set delete create table alter drop index view join inner outer left
  right full cross on group by order having limit offset union all distinct as and or not null is in like between exists case when then else
  end primary key foreign references default unique check constraint begin commit rollback transaction with returning asc desc count sum avg
  min max if cascade truncate database schema grant revoke`);

const identifier = /[A-Za-z_$][\w$]*/y;

function nextCharIs(text: string, pos: number, char: string): boolean {
  let at = pos;
  while (at < text.length && (text[at] === ' ' || text[at] === '\t')) {
    at += 1;
  }
  return text[at] === char;
}

function previousWord(text: string, pos: number): string {
  let end = pos;
  while (end > 0 && /\s/u.test(text[end - 1] ?? '')) {
    end -= 1;
  }
  let start = end;
  while (start > 0 && /[\w$]/u.test(text[start - 1] ?? '')) {
    start -= 1;
  }
  return text.slice(start, end);
}

function previousSignificant(text: string, pos: number): string {
  let at = pos - 1;
  while (at >= 0 && /\s/u.test(text[at] ?? '')) {
    at -= 1;
  }
  return at >= 0 ? (text[at] ?? '') : '';
}

function sticky(re: RegExp): RegExp {
  return new RegExp(re.source, re.flags.includes('y') ? re.flags : `${re.flags}y`);
}

function rule(re: RegExp, cls: string): Matcher {
  const matcher = sticky(re);
  return (text, pos) => {
    matcher.lastIndex = pos;
    const match = matcher.exec(text);
    return match !== null && match[0].length > 0 ? { length: match[0].length, cls } : null;
  };
}

type WordConfig = {
  keywords: Set<string>;
  literals?: Set<string>;
  types?: Set<string>;
  builtins?: Set<string>;
  fnAfter?: Record<string, string>;
  macros?: boolean;
  capitalTypes?: boolean;
};

function wordMatcher(config: WordConfig, caseInsensitive = false): Matcher {
  return (text, pos) => {
    identifier.lastIndex = pos;
    const match = identifier.exec(text);
    if (match === null) {
      return null;
    }
    const word = match[0];
    const key = caseInsensitive ? word.toLowerCase() : word;
    const length = word.length;
    if (config.keywords.has(key)) return { length, cls: 'kw' };
    if (config.literals?.has(key) === true) return { length, cls: 'lit' };
    if (config.types?.has(key) === true) return { length, cls: 'type' };
    if (config.builtins?.has(key) === true) return { length, cls: 'fn' };
    const before = previousWord(text, pos);
    const after = config.fnAfter?.[before];
    if (after !== undefined) return { length, cls: after };
    if (config.macros === true && text[pos + length] === '!') return { length, cls: 'fn' };
    if (nextCharIs(text, pos + length, '(')) return { length, cls: 'fn' };
    if (config.capitalTypes === true && /^[A-Z]/u.test(word) && /[a-z]/u.test(word)) return { length, cls: 'type' };
    return { length, cls: '' };
  };
}

const NUMBER = rule(/(?:0[xX][\da-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)[a-zA-Z]{0,3}\b/y, 'num');

function scan(text: string, matchers: Matcher[], out: Out, from = 0, to = text.length): void {
  let pos = from;
  const limit = to;
  let steps = 0;
  while (pos < limit) {
    steps += 1;
    if ((steps & 63) === 0 && expired()) {
      return;
    }
    let hit: { length: number; cls: string } | null = null;
    for (const matcher of matchers) {
      hit = matcher(text, pos);
      if (hit !== null) {
        break;
      }
    }
    if (hit === null) {
      pos += 1;
      continue;
    }
    const end = Math.min(pos + hit.length, limit);
    if (hit.cls !== '') {
      out.push({ from: pos, to: end, cls: hit.cls });
    }
    pos = end;
  }
}

const cLikeComments = [rule(/\/\/[^\n]*/y, 'com'), rule(/\/\*[\s\S]*?(?:\*\/|$)/y, 'com')];
const dq = rule(/"(?:\\[\s\S]|[^"\\\n])*"?/y, 'str');
const sq = rule(/'(?:\\[\s\S]|[^'\\\n])*'?/y, 'str');
const backtick = rule(/`(?:\\[\s\S]|[^`\\])*`?/y, 'str');

function regexLiteral(text: string, pos: number): { length: number; cls: string } | null {
  if (text[pos] !== '/') {
    return null;
  }
  const before = previousSignificant(text, pos);
  if (before !== '' && !'(,=:[!&|?{};+-*%<>~^'.includes(before) && !/\b(?:return|typeof|case)$/u.test(text.slice(Math.max(0, pos - 8), pos).trimEnd())) {
    return null;
  }
  const matcher = /\/(?![*/])(?:\\.|\[(?:\\.|[^\]\n])*\]|[^/\n\\[])+\/[dgimsuyv]*/y;
  matcher.lastIndex = pos;
  const match = matcher.exec(text);
  return match === null ? null : { length: match[0].length, cls: 'str' };
}

function lexJs(text: string, out: Out, from = 0, to = text.length): void {
  scan(
    text,
    [
      ...cLikeComments,
      dq,
      sq,
      backtick,
      regexLiteral,
      NUMBER,
      wordMatcher({ keywords: JS_KEYWORDS, literals: JS_LITERALS, capitalTypes: true, fnAfter: { class: 'type', function: 'fn', extends: 'type', new: 'type' } }),
    ],
    out,
    from,
    to,
  );
}

function lexJson(text: string, out: Out): void {
  const key = (source: string, pos: number): { length: number; cls: string } | null => {
    const matcher = /"(?:\\.|[^"\\\n])*"/y;
    matcher.lastIndex = pos;
    const match = matcher.exec(source);
    if (match === null) {
      return null;
    }
    return { length: match[0].length, cls: nextCharIs(source, pos + match[0].length, ':') ? 'attr' : 'str' };
  };
  scan(text, [rule(/\/\/[^\n]*/y, 'com'), rule(/\/\*[\s\S]*?(?:\*\/|$)/y, 'com'), key, rule(/-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/y, 'num'), rule(/\b(?:true|false|null)\b/y, 'lit')], out);
}

function lexPython(text: string, out: Out): void {
  scan(
    text,
    [
      rule(/#[^\n]*/y, 'com'),
      rule(/(?:[rRbBuUfF]{1,2})?(?:'''[\s\S]*?(?:'''|$)|"""[\s\S]*?(?:"""|$))/y, 'str'),
      rule(/(?:[rRbBuUfF]{1,2})?"(?:\\[\s\S]|[^"\\\n])*"?/y, 'str'),
      rule(/(?:[rRbBuUfF]{1,2})?'(?:\\[\s\S]|[^'\\\n])*'?/y, 'str'),
      rule(/@[A-Za-z_][\w.]*/y, 'meta'),
      NUMBER,
      wordMatcher({ keywords: PY_KEYWORDS, literals: PY_LITERALS, builtins: PY_BUILTINS, capitalTypes: true, fnAfter: { def: 'fn', class: 'type' } }),
    ],
    out,
  );
}

function lexShell(text: string, out: Out): void {
  scan(
    text,
    [
      (source, pos) => {
        if (source[pos] !== '#') return null;
        const before = source[pos - 1];
        if (pos > 0 && before !== undefined && !/\s|;/u.test(before)) return null;
        const end = source.indexOf('\n', pos);
        return { length: (end < 0 ? source.length : end) - pos, cls: 'com' };
      },
      rule(/\$(?:\{[^}\n]*\}|\(\(?|[A-Za-z_]\w*|[0-9@#?$!*-])/y, 'attr'),
      rule(/"(?:\\[\s\S]|[^"\\])*"?/y, 'str'),
      rule(/'[^']*'?/y, 'str'),
      rule(/(?<=\s|^)--?[A-Za-z][\w-]*/y, 'meta'),
      NUMBER,
      wordMatcher({ keywords: SH_KEYWORDS, builtins: SH_BUILTINS }),
    ],
    out,
  );
}

function lexRust(text: string, out: Out): void {
  scan(
    text,
    [
      ...cLikeComments,
      rule(/#!?\[[^\]\n]*\]/y, 'meta'),
      rule(/b?r(#*)"[\s\S]*?"\1/y, 'str'),
      rule(/b?"(?:\\[\s\S]|[^"\\])*"?/y, 'str'),
      rule(/b?'(?:\\.|[^'\\])'/y, 'str'),
      rule(/'[A-Za-z_]\w*/y, 'attr'),
      NUMBER,
      wordMatcher({ keywords: RUST_KEYWORDS, literals: words('true false'), macros: true, capitalTypes: true, types: words('u8 u16 u32 u64 u128 usize i8 i16 i32 i64 i128 isize f32 f64 bool char str String Vec Option Result Box'), fnAfter: { fn: 'fn', struct: 'type', enum: 'type', trait: 'type', impl: 'type' } }),
    ],
    out,
  );
}

function lexGo(text: string, out: Out): void {
  scan(text, [...cLikeComments, dq, rule(/`[^`]*`?/y, 'str'), sq, NUMBER, wordMatcher({ keywords: GO_KEYWORDS, literals: words('true false nil iota'), types: GO_TYPES, capitalTypes: true, fnAfter: { func: 'fn', type: 'type' } })], out);
}

function lexC(text: string, out: Out): void {
  scan(text, [...cLikeComments, rule(/^[ \t]*#[ \t]*\w+[^\n]*/ym, 'meta'), dq, sq, NUMBER, wordMatcher({ keywords: C_KEYWORDS, literals: words('true false NULL null'), types: C_TYPES, capitalTypes: true, fnAfter: { class: 'type', struct: 'type', enum: 'type' } })], out);
}

function lexJava(text: string, out: Out): void {
  scan(text, [...cLikeComments, rule(/@[A-Za-z_]\w*/y, 'meta'), rule(/"""[\s\S]*?(?:"""|$)/y, 'str'), dq, sq, NUMBER, wordMatcher({ keywords: JAVA_KEYWORDS, literals: words('true false null nil'), types: JAVA_TYPES, capitalTypes: true, fnAfter: { class: 'type', interface: 'type', enum: 'type', fun: 'fn', func: 'fn' } })], out);
}

function lexPhp(text: string, out: Out): void {
  scan(text, [...cLikeComments, rule(/#[^\n]*/y, 'com'), rule(/<\?php|\?>/y, 'meta'), rule(/\$[A-Za-z_]\w*/y, 'attr'), dq, sq, NUMBER, wordMatcher({ keywords: PHP_KEYWORDS, literals: words('true false null TRUE FALSE NULL'), capitalTypes: true, fnAfter: { function: 'fn', class: 'type' } }, false)], out);
}

function lexSql(text: string, out: Out): void {
  scan(text, [rule(/--[^\n]*/y, 'com'), rule(/\/\*[\s\S]*?(?:\*\/|$)/y, 'com'), sq, dq, rule(/`[^`\n]*`?/y, 'attr'), NUMBER, wordMatcher({ keywords: SQL_KEYWORDS, literals: words('true false null') }, true)], out);
}

function lexYaml(text: string, out: Out): void {
  scan(
    text,
    [
      rule(/#[^\n]*/y, 'com'),
      rule(/"(?:\\.|[^"\\\n])*"?/y, 'str'),
      rule(/'(?:''|[^'\n])*'?/y, 'str'),
      rule(/(?<=^|\n)[ \t]*(?:-[ \t]+)?[\w./-]+(?=[ \t]*:(?:\s|$))/y, 'attr'),
      rule(/[&*][\w-]+/y, 'meta'),
      rule(/\b(?:true|false|yes|no|null|on|off)\b/y, 'lit'),
      rule(/-?\b\d+(?:\.\d+)?\b/y, 'num'),
    ],
    out,
  );
}

function lexToml(text: string, out: Out): void {
  scan(
    text,
    [
      rule(/[#;][^\n]*/y, 'com'),
      rule(/^[ \t]*\[\[?[^\]\n]+\]\]?/ym, 'type'),
      rule(/"""[\s\S]*?(?:"""|$)/y, 'str'),
      rule(/"(?:\\.|[^"\\\n])*"?/y, 'str'),
      rule(/'[^'\n]*'?/y, 'str'),
      rule(/^[ \t]*[\w.-]+(?=[ \t]*[=:])/ym, 'attr'),
      rule(/\b(?:true|false)\b/y, 'lit'),
      NUMBER,
    ],
    out,
  );
}

function lexMarkdown(text: string, out: Out): void {
  scan(
    text,
    [
      rule(/^(?:```|~~~)[\s\S]*?(?:^(?:```|~~~)[^\n]*|$)/ym, 'str'),
      rule(/^#{1,6}[ \t].*$/ym, 'kw'),
      rule(/^>[^\n]*/ym, 'com'),
      rule(/^[ \t]*(?:[-*+]|\d+\.)(?=[ \t])/ym, 'num'),
      rule(/`[^`\n]+`/y, 'str'),
      rule(/\*\*[^*\n]+\*\*|__[^_\n]+__/y, 'attr'),
      rule(/!?\[[^\]\n]*\]\([^)\n]*\)/y, 'fn'),
      rule(/<!--[\s\S]*?(?:-->|$)/y, 'com'),
    ],
    out,
  );
}

function lexCss(text: string, out: Out, from = 0, to = text.length): void {
  let depth = 0;
  const brace: Matcher = (source, pos) => {
    const char = source[pos];
    if (char === '{') depth += 1;
    else if (char === '}') depth = Math.max(0, depth - 1);
    return null;
  };
  const inBlock = (cls: string, re: RegExp): Matcher => {
    const matcher = rule(re, cls);
    return (source, pos) => (depth > 0 ? matcher(source, pos) : null);
  };
  const outside = (cls: string, re: RegExp): Matcher => {
    const matcher = rule(re, cls);
    return (source, pos) => (depth === 0 ? matcher(source, pos) : null);
  };
  scan(
    text,
    [
      brace,
      rule(/\/\*[\s\S]*?(?:\*\/|$)/y, 'com'),
      dq,
      sq,
      rule(/@[\w-]+/y, 'kw'),
      rule(/#[\da-fA-F]{3,8}\b/y, 'num'),
      rule(/-?\d*\.?\d+(?:%|[a-zA-Z]{1,4})?/y, 'num'),
      rule(/!important\b/y, 'kw'),
      inBlock('attr', /--?[A-Za-z][\w-]*(?=\s*:)/y),
      inBlock('attr', /[A-Za-z][\w-]*(?=\s*:)/y),
      inBlock('fn', /[A-Za-z][\w-]*(?=\()/y),
      outside('type', /\.[\w-]+/y),
      outside('fn', /#[\w-]+/y),
      outside('attr', /::?[\w-]+/y),
      outside('tag', /[A-Za-z][\w-]*/y),
    ],
    out,
    from,
    to,
  );
}

function lexHtml(text: string, out: Out): void {
  const length = text.length;
  let pos = 0;
  let lowered: string | null = null;
  let guard = 0;
  const push = (from: number, to: number, cls: string): void => {
    if (to > from) out.push({ from, to, cls });
  };
  const name = /[A-Za-z][\w:.-]*/y;
  const attr = /[^\s=>/"'<]+/y;
  while (pos < length) {
    guard += 1;
    if ((guard & 63) === 0 && expired()) {
      return;
    }
    const open = text.indexOf('<', pos);
    const amp = text.indexOf('&', pos);
    const next = [open, amp].filter((index) => index >= 0).sort((a, b) => a - b)[0];
    if (next === undefined) {
      return;
    }
    pos = next;
    if (text.startsWith('<!--', pos)) {
      const end = text.indexOf('-->', pos + 4);
      const stop = end < 0 ? length : end + 3;
      push(pos, stop, 'com');
      pos = stop;
      continue;
    }
    if (text[pos] === '&') {
      const match = /&(?:#\d+|#x[\da-fA-F]+|[A-Za-z]\w*);/y;
      match.lastIndex = pos;
      const found = match.exec(text);
      if (found === null) {
        pos += 1;
      } else {
        push(pos, pos + found[0].length, 'num');
        pos += found[0].length;
      }
      continue;
    }
    if (text.startsWith('<!', pos) || text.startsWith('<?', pos)) {
      const end = text.indexOf('>', pos);
      const stop = end < 0 ? length : end + 1;
      push(pos, stop, 'meta');
      pos = stop;
      continue;
    }
    name.lastIndex = pos + (text[pos + 1] === '/' ? 2 : 1);
    const tag = name.exec(text);
    if (tag === null) {
      pos += 1;
      continue;
    }
    const closing = text[pos + 1] === '/';
    push(pos, pos + (closing ? 2 : 1), 'punct');
    push(tag.index, tag.index + tag[0].length, 'tag');
    pos = tag.index + tag[0].length;
    while (pos < length && text[pos] !== '>') {
      const char = text[pos] ?? '';
      if (/\s/u.test(char) || char === '/') {
        pos += 1;
      } else if (char === '"' || char === "'") {
        const end = text.indexOf(char, pos + 1);
        const stop = end < 0 ? length : end + 1;
        push(pos, stop, 'str');
        pos = stop;
      } else if (char === '=') {
        push(pos, pos + 1, 'punct');
        pos += 1;
      } else {
        attr.lastIndex = pos;
        const found = attr.exec(text);
        if (found === null) {
          pos += 1;
        } else {
          const afterEquals = text[pos - 1] === '=';
          push(pos, pos + found[0].length, afterEquals ? 'str' : 'attr');
          pos += found[0].length;
        }
      }
    }
    if (pos < length) {
      push(pos, pos + 1, 'punct');
      pos += 1;
    }
    const lower = tag[0].toLowerCase();
    if (!closing && (lower === 'script' || lower === 'style')) {
      lowered ??= text.toLowerCase();
      const end = lowered.indexOf(`</${lower}`, pos);
      const stop = end < 0 ? length : end;
      if (lower === 'script') lexJs(text, out, pos, stop);
      else lexCss(text, out, pos, stop);
      pos = stop;
    }
  }
}

export function tokenize(text: string, language: Language): Token[] {
  if (language === 'plain' || text.length > MAX_HIGHLIGHT_CHARS) {
    return [];
  }
  const out: Out = [];
  deadline = performance.now() + TIME_BUDGET_MS;
  switch (language) {
    case 'html': lexHtml(text, out); break;
    case 'css': lexCss(text, out); break;
    case 'js': lexJs(text, out); break;
    case 'json': lexJson(text, out); break;
    case 'python': lexPython(text, out); break;
    case 'shell': lexShell(text, out); break;
    case 'rust': lexRust(text, out); break;
    case 'go': lexGo(text, out); break;
    case 'c': lexC(text, out); break;
    case 'java': lexJava(text, out); break;
    case 'php': lexPhp(text, out); break;
    case 'sql': lexSql(text, out); break;
    case 'yaml': lexYaml(text, out); break;
    case 'toml': lexToml(text, out); break;
    case 'markdown': lexMarkdown(text, out); break;
  }
  deadline = Number.POSITIVE_INFINITY;
  return out;
}

export function lineStarts(text: string): number[] {
  const starts = [0];
  for (let index = text.indexOf('\n'); index >= 0; index = text.indexOf('\n', index + 1)) {
    starts.push(index + 1);
  }
  return starts;
}

export type Piece = { text: string; cls: string };

export function piecesForLine(text: string, starts: readonly number[], tokens: readonly Token[], line: number): Piece[] {
  const from = starts[line] ?? text.length;
  const nextStart = starts[line + 1];
  const to = nextStart === undefined ? text.length : nextStart - 1;
  if (to <= from) {
    return [];
  }
  let low = 0;
  let high = tokens.length;
  while (low < high) {
    const mid = (low + high) >> 1;
    if ((tokens[mid]?.to ?? 0) <= from) low = mid + 1;
    else high = mid;
  }
  const pieces: Piece[] = [];
  let cursor = from;
  for (let index = low; index < tokens.length; index += 1) {
    const token = tokens[index];
    if (token === undefined || token.from >= to) {
      break;
    }
    const start = Math.max(token.from, from);
    const end = Math.min(token.to, to);
    if (start > cursor) pieces.push({ text: text.slice(cursor, start), cls: '' });
    if (end > start && start >= cursor) {
      pieces.push({ text: text.slice(start, end), cls: token.cls });
      cursor = end;
    }
  }
  if (cursor < to) pieces.push({ text: text.slice(cursor, to), cls: '' });
  return pieces;
}
