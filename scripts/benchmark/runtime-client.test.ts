// Regression for the transient verify-battery failure (observed twice
// on 2026-07-12): RuntimeSession's stdin writes (cancel at
// lineHandler, generate at invoke start) had no 'error' listener, so
// a write racing the child's death emitted an UNHANDLED 'write EPIPE'
// — an uncaught exception that failed the whole vitest run while all
// tests passed. The scripted crash suites make that race a normal,
// expected occurrence, so the client must tolerate stdin dying at any
// moment.

import { describe, expect, it } from 'vitest';

import { RuntimeSession } from './runtime-client.ts';

type WithChild = { child: { stdin: NodeJS.EventEmitter } };

describe('RuntimeSession stdin error handling', () => {
  it('has an error listener on stdin, so a mid-write EPIPE cannot become an uncaught exception', () => {
    const session = new RuntimeSession([process.execPath, '-e', 'setTimeout(() => {}, 5000)']);
    try {
      const stdin = (session as unknown as WithChild).child.stdin;
      const epipe = Object.assign(new Error('write EPIPE'), {
        code: 'EPIPE',
        errno: -32,
        syscall: 'write',
      });
      // EventEmitter semantics make this deterministic: emitting
      // 'error' with NO listener throws synchronously — exactly the
      // uncaught-exception path that killed the verify battery.
      expect(() => stdin.emit('error', epipe)).not.toThrow();
    } finally {
      session.close();
    }
  });

  it('settles the invocation as crashed when the child dies instead of acknowledging a cancel', async () => {
    // The real race shape, end to end: the child emits one token and
    // exits nonzero without ever reading stdin, so the harness's
    // cancel write targets a dying pipe. The invocation must settle
    // as the crash measurement — never throw, never hang.
    const session = new RuntimeSession([
      process.execPath,
      '-e',
      'process.stdout.write(JSON.stringify({ type: "token", text: "x" }) + "\\n", () => process.exit(3))',
    ]);
    try {
      const result = await session.invoke({ prompt: 'p', timeoutMs: 5_000, cancelAfterTokens: 1 });
      expect(result.terminal).toBe('crashed');
      expect(result.reply).toBe('x');
    } finally {
      session.close();
    }
  });
});
