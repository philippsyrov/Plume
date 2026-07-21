import { useEffect, useRef } from 'react';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import type { UnlistenFn } from '@tauri-apps/api/event';

export function useProjectFolderDrop({
  busy,
  onCandidate,
}: {
  busy: boolean;
  onCandidate: (path: string) => void | Promise<void>;
}) {
  const busyRef = useRef(busy);
  const onCandidateRef = useRef(onCandidate);

  useEffect(() => {
    busyRef.current = busy;
  }, [busy]);
  useEffect(() => {
    onCandidateRef.current = onCandidate;
  }, [onCandidate]);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (busyRef.current || event.payload.type !== 'drop') return;
        const first = event.payload.paths[0];
        if (!first) return;
        void onCandidateRef.current(first);
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch((error) => {
        console.error(
          'Project folder drop listener registration failed:',
          error instanceof Error ? error.message : String(error),
        );
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}
