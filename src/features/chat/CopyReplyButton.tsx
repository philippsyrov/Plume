// D14: per-reply Copy button on completed assistant turns. Only
// uses `navigator.clipboard.writeText` — no new dependencies, no
// IPC. The two-second "Copied!" state gives the user a quick
// confirmation without a toast/modal. Streaming and cancelled
// turns deliberately don't get a button — copying a partial
// reply mid-stream would be a footgun (the user could miss
// content that arrives moments later).
//
// D22 extraction: lifted out of `ChatPanel.tsx`. Pure leaf
// component — no chat-feature deps.

import { useCallback, useState } from 'react';

export function CopyReplyButton({ text }: { text: string }) {
  const [state, setState] = useState<'idle' | 'copied' | 'failed'>('idle');
  const onCopy = useCallback(async () => {
    if (!text) return;
    try {
      // `navigator.clipboard` is available in Tauri's webview;
      // gate on its existence anyway so a future headless test
      // harness doesn't crash here.
      if (typeof navigator !== 'undefined' && navigator.clipboard) {
        await navigator.clipboard.writeText(text);
        setState('copied');
      } else {
        setState('failed');
      }
    } catch {
      setState('failed');
    }
    // Auto-revert after a beat so subsequent copies don't appear
    // stuck on the previous status. 2 s is the same window the
    // attachment chip uses for its transient labels.
    window.setTimeout(() => setState('idle'), 2000);
  }, [text]);
  const label =
    state === 'idle'
      ? 'Copy'
      : state === 'copied'
        ? 'Copied!'
        : 'Copy failed';
  return (
    <button
      type="button"
      className="plume-chat-copy-button"
      onClick={onCopy}
      aria-label="Copy assistant reply text to clipboard"
      title="Copy the reply text to your clipboard."
    >
      {label}
    </button>
  );
}
