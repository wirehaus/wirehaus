import assert from 'node:assert/strict';
import test from 'node:test';

import { detectLanguage, lineStarts, piecesForLine, tokenize } from './highlight.js';

const classes = (text: string, language: Parameters<typeof tokenize>[1]): [string, string][] =>
  tokenize(text, language).map((token) => [text.slice(token.from, token.to), token.cls]);

test('detects the language from the name, the shebang and the content', () => {
  assert.equal(detectLanguage('index.html', ''), 'html');
  assert.equal(detectLanguage('app.tsx', ''), 'js');
  assert.equal(detectLanguage('main.py', ''), 'python');
  assert.equal(detectLanguage('Makefile', ''), 'shell');
  assert.equal(detectLanguage('run', '#!/usr/bin/env python3\n'), 'python');
  assert.equal(detectLanguage('run', '#!/bin/bash\n'), 'shell');
  assert.equal(detectLanguage('data', '{"a": 1}'), 'json');
  assert.equal(detectLanguage('page', '<!doctype html>'), 'html');
  assert.equal(detectLanguage('notes.txt', 'hola'), 'plain');
});

test('html colours tags, attributes, strings, comments and embedded script and style', () => {
  const text = '<!-- c --><a href="x" id=y>hi &amp;</a><script>let n = 1;</script><style>p { color: red; }</style>';
  const found = classes(text, 'html');
  assert.deepEqual(found.find(([t]) => t === '<!-- c -->'), ['<!-- c -->', 'com']);
  assert.ok(found.some(([t, c]) => t === 'a' && c === 'tag'));
  assert.ok(found.some(([t, c]) => t === 'href' && c === 'attr'));
  assert.ok(found.some(([t, c]) => t === '"x"' && c === 'str'));
  assert.ok(found.some(([t, c]) => t === '&amp;' && c === 'num'));
  assert.ok(found.some(([t, c]) => t === 'let' && c === 'kw'));
  assert.ok(found.some(([t, c]) => t === 'color' && c === 'attr'));
});

test('javascript colours keywords, strings, numbers, comments, functions and regular expressions', () => {
  const text = 'const x = foo(1, "a"); // hi\nlet r = /ab+c/g; class Foo {}';
  const found = classes(text, 'js');
  for (const expected of [['const', 'kw'], ['foo', 'fn'], ['1', 'num'], ['"a"', 'str'], ['// hi', 'com'], ['/ab+c/g', 'str'], ['Foo', 'type']] as const) {
    assert.ok(found.some(([t, c]) => t === expected[0] && c === expected[1]), expected[0]);
  }
  assert.ok(!classes('a / b / c', 'js').some(([, c]) => c === 'str'));
});

test('python handles triple quoted strings, decorators, definitions and comments', () => {
  const text = '@app.route\ndef hello(n):\n    """doc\n    more"""\n    return n  # done';
  const found = classes(text, 'python');
  assert.ok(found.some(([t, c]) => t === '@app.route' && c === 'meta'));
  assert.ok(found.some(([t, c]) => t === 'hello' && c === 'fn'));
  assert.ok(found.some(([t, c]) => t === '"""doc\n    more"""' && c === 'str'));
  assert.ok(found.some(([t, c]) => t === '# done' && c === 'com'));
});

test('json separates keys from string values', () => {
  const found = classes('{"name": "x", "n": 3, "ok": true}', 'json');
  assert.deepEqual(found.filter(([, c]) => c === 'attr').map(([t]) => t), ['"name"', '"n"', '"ok"']);
  assert.ok(found.some(([t, c]) => t === '"x"' && c === 'str'));
  assert.ok(found.some(([t, c]) => t === 'true' && c === 'lit'));
});

test('shell, yaml, toml, css, sql and markdown get their own colours', () => {
  assert.ok(classes('echo "$HOME" # hi', 'shell').some(([t, c]) => t === '# hi' && c === 'com'));
  assert.ok(classes('name: web\nport: 80\n', 'yaml').some(([t, c]) => t === 'name' && c === 'attr'));
  assert.ok(classes('[server]\nport = 80\n', 'toml').some(([t, c]) => t === '[server]' && c === 'type'));
  assert.ok(classes('a { color: #fff; margin: 4px }', 'css').some(([t, c]) => t === '#fff' && c === 'num'));
  assert.ok(classes('SELECT id FROM t -- x', 'sql').some(([t, c]) => t === 'SELECT' && c === 'kw'));
  assert.ok(classes('# Title\n`code`\n', 'markdown').some(([t, c]) => t === '# Title' && c === 'kw'));
});

test('tokens never overlap and stay inside the text on awkward input', () => {
  const samples = ['', '"unterminated', '/* open', '<a href="', '<script>', '`tpl ${', "'''x", '\u0000\ud800', 'a'.repeat(5000)];
  for (const language of ['html', 'css', 'js', 'json', 'python', 'shell', 'rust', 'go', 'c', 'java', 'php', 'sql', 'yaml', 'toml', 'markdown'] as const) {
    for (const text of samples) {
      let end = 0;
      for (const token of tokenize(text, language)) {
        assert.ok(token.from >= end && token.to > token.from && token.to <= text.length, `${language} ${JSON.stringify(text.slice(0, 20))}`);
        end = token.to;
      }
    }
  }
});

test('pieces for a line split multi-line tokens and cover the whole line', () => {
  const text = 'a /* x\ny */ b\nlast';
  const starts = lineStarts(text);
  const tokens = tokenize(text, 'js');
  const rebuilt = [0, 1, 2].map((line) => piecesForLine(text, starts, tokens, line).map((piece) => piece.text).join(''));
  assert.deepEqual(rebuilt, ['a /* x', 'y */ b', 'last']);
  assert.ok(piecesForLine(text, starts, tokens, 1).some((piece) => piece.text === 'y */' && piece.cls === 'com'));
});

test('hostile input cannot freeze the highlighter', () => {
  const cases: [Parameters<typeof tokenize>[1], string][] = [
    ['js', '=/['.repeat(60_000)],
    ['shell', '${'.repeat(90_000)],
    ['rust', 'r#"'.repeat(60_000)],
    ['markdown', '['.repeat(190_000)],
    ['markdown', '[a]('.repeat(40_000)],
    ['c', '1a'.repeat(90_000)],
    ['html', '<script></script>'.repeat(10_000)],
  ];
  for (const [language, text] of cases) {
    const started = performance.now();
    tokenize(text, language);
    assert.ok(performance.now() - started < 1500, `${language} took ${Math.round(performance.now() - started)} ms`);
  }
});
