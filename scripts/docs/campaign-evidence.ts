/**
 * Decides whether a scenario's `automatedEvidence` entry names a test that actually runs.
 *
 * The corpus checker (`campaign-fixtures.ts`) uses this to stop a scenario claiming
 * `implemented` on the strength of something that never executes an assertion. "Declared"
 * is not the same as "runs": a test can sit inside a skipped suite, inside a function
 * nobody calls, behind `#[ignore]`, or inside a comment. Each language is therefore read
 * on its own terms — TypeScript through the compiler's syntax tree, Rust through a small
 * lexer plus its attribute grammar — and every check fails closed.
 */
import ts from 'typescript';

const JS_TEST_FUNCTIONS = new Set(['it', 'test']);
const JS_SUITE_FUNCTION = 'describe';
const JS_RUNNER_NAMES = new Set([...JS_TEST_FUNCTIONS, JS_SUITE_FUNCTION]);
const JS_RUNNER_MODULE = 'vitest';

/**
 * Which runner names this file may legitimately call.
 *
 * The name alone proves nothing: `const test = () => {}` above a `test('name', fn)` call
 * makes the "test" a no-op that reports nothing. The suite runs with `globals: false`, so
 * a real runner is always an import from `vitest` — that is the positive fact to check.
 * Any other binding of the same name, including an alias onto it, disqualifies the file.
 */
type RunnerBindings = { imported: Set<string>; shadowed: Set<string> };

function collectRunnerBindings(file: ts.SourceFile): RunnerBindings {
  const imported = new Set<string>();
  const shadowed = new Set<string>();

  const noteBinding = (name: ts.BindingName | undefined): void => {
    if (name === undefined || !ts.isIdentifier(name)) return;
    if (JS_RUNNER_NAMES.has(name.text)) shadowed.add(name.text);
  };

  const visit = (node: ts.Node): void => {
    if (ts.isImportDeclaration(node) && ts.isStringLiteral(node.moduleSpecifier)) {
      const clause = node.importClause?.namedBindings;
      if (clause !== undefined && ts.isNamedImports(clause)) {
        const fromRunner = node.moduleSpecifier.text === JS_RUNNER_MODULE;
        for (const element of clause.elements) {
          const local = element.name.text;
          if (!JS_RUNNER_NAMES.has(local)) continue;
          // `import { foo as it }` binds `it` to something else entirely.
          const renamed = element.propertyName !== undefined && element.propertyName.text !== local;
          if (fromRunner && !renamed) imported.add(local);
          else shadowed.add(local);
        }
      }
    } else if (
      ts.isVariableDeclaration(node) ||
      ts.isParameter(node) ||
      ts.isBindingElement(node)
    ) {
      noteBinding(node.name);
    } else if (
      (ts.isFunctionDeclaration(node) || ts.isClassDeclaration(node)) &&
      node.name !== undefined
    ) {
      noteBinding(node.name);
    }

    ts.forEachChild(node, visit);
  };
  visit(file);

  return { imported, shadowed };
}

function isRealRunner(bindings: RunnerBindings, name: string): boolean {
  return bindings.imported.has(name) && !bindings.shadowed.has(name);
}

/**
 * True when `call` is reached during an ordinary test-file load: a statement at module
 * top level, or one nested only inside bare `describe(...)` suite bodies.
 *
 * Walking every call node instead would accept a test inside `describe.skip(...)`, inside
 * an uncalled helper, or behind an `if` — none of which run. Requiring the bare identifier
 * also rejects `describe.only`, which changes what the rest of the suite does and has no
 * business in committed evidence.
 */
function runsOnLoad(call: ts.CallExpression, bindings: RunnerBindings): boolean {
  const statement = call.parent;
  if (statement === undefined || !ts.isExpressionStatement(statement)) return false;

  const container = statement.parent;
  if (ts.isSourceFile(container)) return true;
  if (!ts.isBlock(container)) return false;

  const body = container.parent;
  if (!ts.isArrowFunction(body) && !ts.isFunctionExpression(body)) return false;

  const suite = body.parent;
  if (!ts.isCallExpression(suite)) return false;
  if (!ts.isIdentifier(suite.expression) || suite.expression.text !== JS_SUITE_FUNCTION) {
    return false;
  }
  if (!isRealRunner(bindings, JS_SUITE_FUNCTION)) return false;
  if (suite.arguments[1] !== body) return false;

  return runsOnLoad(suite, bindings);
}

/** `it('name', fn)` / `test('name', fn)` — a bare identifier, a literal name, a body. */
function isRunnableJsTest(
  node: ts.Node,
  testName: string,
  bindings: RunnerBindings,
): node is ts.CallExpression {
  if (!ts.isCallExpression(node)) return false;
  if (!ts.isIdentifier(node.expression)) return false;
  if (!JS_TEST_FUNCTIONS.has(node.expression.text)) return false;
  if (!isRealRunner(bindings, node.expression.text)) return false;

  // A single-argument call is a `todo` placeholder, not a test that runs.
  if (node.arguments.length < 2) return false;

  const [name, body] = node.arguments;
  if (name === undefined || body === undefined) return false;
  if (!ts.isStringLiteral(name) && !ts.isNoSubstitutionTemplateLiteral(name)) return false;
  if (name.text !== testName) return false;

  return ts.isArrowFunction(body) || ts.isFunctionExpression(body);
}

/**
 * Parses the file and looks for a real, reachable call node. A regex over raw text cannot
 * tell a test from the same characters inside a comment, inside a string literal, or in
 * `helper.test('name', fn)`.
 *
 * Deliberately strict: the callee must be the bare identifier `it` or `test`, so `it.skip`
 * (never runs) and `it.each` (the name is a template, not this string) are both rejected.
 * A scenario proved by a table-driven test names a plain test instead.
 */
export function declaresTypeScriptTest(source: string, testName: string, fileName: string): boolean {
  const parsed = ts.createSourceFile(fileName, source, ts.ScriptTarget.Latest, true);
  const bindings = collectRunnerBindings(parsed);

  let found = false;
  const visit = (node: ts.Node): void => {
    if (found) return;
    if (isRunnableJsTest(node, testName, bindings) && runsOnLoad(node, bindings)) {
      found = true;
      return;
    }
    ts.forEachChild(node, visit);
  };
  visit(parsed);

  return found;
}

/**
 * Blanks out comments, preserving line structure so the attribute scan below still lines up.
 *
 * Rust block comments nest, so a non-greedy block-comment regex ends an outer comment at
 * the first inner close marker and re-exposes the rest — enough to uncover a commented-out
 * test attribute. String literals are skipped so an open marker inside one cannot start a
 * comment either. Char literals are left alone because `'` is also a lifetime sigil;
 * the failure that causes is over-stripping, which hides a test rather than inventing one.
 */
function stripRustComments(source: string): string {
  let out = '';
  let index = 0;
  let depth = 0;

  while (index < source.length) {
    const rest = source.slice(index, index + 2);

    if (depth > 0) {
      if (rest === '/*') {
        depth += 1;
        index += 2;
      } else if (rest === '*/') {
        depth -= 1;
        index += 2;
      } else {
        out += source[index] === '\n' ? '\n' : ' ';
        index += 1;
      }
      continue;
    }

    if (rest === '/*') {
      depth = 1;
      index += 2;
      continue;
    }

    if (rest === '//') {
      while (index < source.length && source[index] !== '\n') index += 1;
      continue;
    }

    const raw = readRawStringOpener(source, index);
    if (raw !== null) {
      const end = source.indexOf(raw.terminator, index + raw.opener.length);
      const stop = end === -1 ? source.length : end + raw.terminator.length;
      out += source.slice(index, stop);
      index = stop;
      continue;
    }

    if (source[index] === '"') {
      const stop = endOfStringLiteral(source, index);
      out += source.slice(index, stop);
      index = stop;
      continue;
    }

    out += source[index];
    index += 1;
  }

  return out;
}

/** Recognises `r"`, `r#"`, `r##"` … and returns the matching terminator. */
function readRawStringOpener(
  source: string,
  index: number,
): { opener: string; terminator: string } | null {
  if (source[index] !== 'r') return null;

  const previous = source[index - 1];
  if (previous !== undefined && /[\w]/.test(previous)) return null;

  let cursor = index + 1;
  let hashes = 0;
  while (source[cursor] === '#') {
    hashes += 1;
    cursor += 1;
  }
  if (source[cursor] !== '"') return null;

  return { opener: `r${'#'.repeat(hashes)}"`, terminator: `"${'#'.repeat(hashes)}` };
}

function endOfStringLiteral(source: string, start: number): number {
  let index = start + 1;
  while (index < source.length) {
    if (source[index] === '\\') {
      index += 2;
      continue;
    }
    if (source[index] === '"') return index + 1;
    index += 1;
  }
  return source.length;
}

/** The attribute's path, without any arguments: `#[tokio::test(flavor = "x")]` → `tokio::test`. */
function attributePath(line: string): string | null {
  const match = /^#\[([^\]]*)\]$/.exec(line);
  if (match === null) return null;
  return (match[1] ?? '').split('(')[0]?.trim() ?? '';
}

/**
 * True for an attribute whose path *is* a test runner: `#[test]`, `#[tokio::test]`.
 * Matching any attribute containing the word `test` accepts `#[cfg(test)]`, which merely
 * marks the item as compiled under the test profile — every helper in the module carries it.
 */
function isTestAttribute(path: string): boolean {
  return path === 'test' || path.endsWith('::test');
}

/**
 * Attributes that mean "cargo may not run this". `ignore` in any form is skipped by
 * default; `cfg` and `cfg_attr` make the outcome depend on features and platform, and
 * `#[cfg_attr(test, ignore)]` is exactly the ignore-in-disguise this needs to catch.
 * Deciding them properly would mean resolving the whole feature graph, so the rule fails
 * closed: a conditional test is never evidence.
 */
const RUST_DISQUALIFYING_PATHS = new Set(['ignore', 'cfg', 'cfg_attr']);

function disqualifies(path: string, line: string): boolean {
  if (RUST_DISQUALIFYING_PATHS.has(path)) return true;
  return /\bignore\b/.test(line);
}

/**
 * Finds `fn <testName>` and requires a genuine test attribute in the contiguous block above
 * it, with nothing in that block that could stop cargo running it.
 */
export function declaresRustTest(source: string, testName: string): boolean {
  const declaration = new RegExp(`^\\s*(?:pub\\s+)?(?:async\\s+)?fn\\s+${escapeRegExp(testName)}\\s*\\(`);
  const lines = stripRustComments(source).split('\n');

  for (let index = 0; index < lines.length; index += 1) {
    if (!declaration.test(lines[index] ?? '')) continue;

    let isTest = false;
    let isIgnored = false;
    for (let above = index - 1; above >= 0; above -= 1) {
      const line = (lines[above] ?? '').trim();
      if (line === '') continue;

      const path = attributePath(line);
      if (path === null) {
        // A line that opens an attribute but does not close it is a multi-line
        // attribute this simple scan cannot read — and `#[cfg_attr(\n ...,\n
        // ignore)]` is exactly the shape that would hide an ignore. Stopping
        // here would accept the test on the strength of a `#[test]` seen
        // earlier, so an unreadable attribute disqualifies instead.
        // `#[cfg_attr(\n  ...,\n  ignore\n)]` reaches this scan as fragments,
        // and its closing `)]` is what sits directly above the `#[test]`.
        // Treating either end of a wrapped attribute as "not an attribute"
        // would skip the block and let the `#[test]` carry the day, so an
        // unreadable attribute disqualifies the function instead.
        if (line.startsWith('#[') || line.startsWith('#!') || line.endsWith(']')) return false;
        break;
      }
      if (isTestAttribute(path)) isTest = true;
      if (disqualifies(path, line)) isIgnored = true;
    }

    if (isTest && !isIgnored) return true;
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
