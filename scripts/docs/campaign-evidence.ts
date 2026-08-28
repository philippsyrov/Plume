/**
 * Decides whether a scenario's `automatedEvidence` entry really names a test that runs.
 *
 * The corpus checker (`campaign-fixtures.ts`) uses this to stop a scenario claiming
 * `implemented` on the strength of something that never executes an assertion. Text
 * matching is not enough for either language, so each side is parsed on its own terms:
 * TypeScript through the compiler's own syntax tree, Rust through its attribute grammar.
 */
import ts from 'typescript';

const JS_TEST_FUNCTIONS = new Set(['it', 'test']);

/** `it('name', fn)` and `test('name', fn)` only — see `declaresTypeScriptTest`. */
function isRunnableJsTest(node: ts.Node, testName: string): boolean {
  if (!ts.isCallExpression(node)) return false;
  if (!ts.isIdentifier(node.expression)) return false;
  if (!JS_TEST_FUNCTIONS.has(node.expression.text)) return false;

  // A single-argument call is a `todo` placeholder, not a test that runs.
  if (node.arguments.length < 2) return false;

  const [name, body] = node.arguments;
  if (name === undefined || body === undefined) return false;
  if (!ts.isStringLiteral(name) && !ts.isNoSubstitutionTemplateLiteral(name)) return false;
  if (name.text !== testName) return false;

  return ts.isArrowFunction(body) || ts.isFunctionExpression(body);
}

/**
 * Parses the file and looks for a real call node. A regex over raw text cannot tell a
 * test from the same characters inside a comment, inside a string literal, or in
 * `helper.test('name', fn)` — all three would certify a scenario that never ran.
 *
 * Deliberately strict: the callee must be the bare identifier `it` or `test`, so
 * `it.skip` (never runs) and `it.each` (the name is a template, not this string) are
 * both rejected. A scenario proved by a table-driven test names a plain test instead.
 */
export function declaresTypeScriptTest(source: string, testName: string, fileName: string): boolean {
  const parsed = ts.createSourceFile(fileName, source, ts.ScriptTarget.Latest, true);

  let found = false;
  const visit = (node: ts.Node): void => {
    if (found) return;
    if (isRunnableJsTest(node, testName)) {
      found = true;
      return;
    }
    ts.forEachChild(node, visit);
  };
  visit(parsed);

  return found;
}

/** Strips line and block comments so commented-out code cannot answer the search. */
function stripRustComments(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/[^\n]*/g, '');
}

/**
 * True only for an attribute whose path *is* a test runner: `#[test]`, `#[tokio::test]`,
 * and the like. Matching any attribute containing the word `test` accepts `#[cfg(test)]`,
 * which merely marks the item as compiled under the test profile and says nothing about
 * whether it is a test — a plain helper carries it too.
 */
function isRustTestAttribute(line: string): boolean {
  const match = /^#\[([^\]]*)\]$/.exec(line);
  if (match === null) return false;

  const path = (match[1] ?? '').split('(')[0]?.trim() ?? '';
  return path === 'test' || path.endsWith('::test');
}

/** Finds `fn <testName>` and requires a test attribute in the contiguous block above it. */
export function declaresRustTest(source: string, testName: string): boolean {
  const declaration = new RegExp(`^\\s*(?:pub\\s+)?(?:async\\s+)?fn\\s+${escapeRegExp(testName)}\\s*\\(`);
  const lines = stripRustComments(source).split('\n');

  for (let index = 0; index < lines.length; index += 1) {
    if (!declaration.test(lines[index] ?? '')) continue;
    for (let above = index - 1; above >= 0; above -= 1) {
      const line = (lines[above] ?? '').trim();
      if (line === '') continue;
      if (!line.startsWith('#[')) break;
      if (isRustTestAttribute(line)) return true;
    }
  }

  return false;
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/** Dispatches on the evidence file's language. */
export function declaresRunnableTest(source: string, testName: string, filePath: string): boolean {
  if (filePath.endsWith('.rs')) return declaresRustTest(source, testName);
  return declaresTypeScriptTest(source, testName, filePath);
}
