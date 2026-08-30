// D63B: quiet status strip for the session layer — the streaming
// switch block / load failure (`notice`, polite) and transcript-save
// failures (`saveError`, assertive). Rendered by the shell above the
// chat surface so blocked actions and storage problems are visible
// instead of silent (spec acceptance: storage failures are visible
// and recoverable without crashing Plume).
//
// Phase 1B: a full store is a state, not an incident. The retry copy below is
// right for a transient failure and wrong for a permanent cap — retrying
// forever would never succeed — so the caller passes `storageFull`, resolved
// from the typed `StorageFull` refusal rather than by reading the error text.
// Control flow never parses an error message; see `src/lib/api/errors.ts`.
//
// The recovery names export before deletion. Deletion is the only way to
// reclaim space and it is irreversible, so pointing at it without mentioning
// that a copy can be kept first points the user at data loss.

export function SessionNotices({
  notice,
  saveError,
  storageFull = false,
  storageWarning = null,
}: {
  notice: string | null;
  saveError: string | null;
  storageFull?: boolean;
  storageWarning?: string | null;
}) {
  if (notice === null && saveError === null && storageWarning === null) return null;
  return (
    <div className="plume-session-notices">
      {notice !== null ? (
        <p className="plume-session-notice" role="status">
          {notice}
        </p>
      ) : null}
      {storageWarning !== null && saveError === null ? (
        <p className="plume-session-notice" role="status">
          {storageWarning}
        </p>
      ) : null}
      {saveError !== null ? (
        <p className="plume-session-notice plume-session-notice-error" role="alert">
          {storageFull ? (
            <>
              Chat history could not be saved: {saveError} Your visible transcript is
              unaffected and nothing has been deleted, but new messages will not be
              saved until you make room. Export a conversation you want to keep,
              then delete it.
            </>
          ) : (
            <>
              Chat history could not be saved: {saveError}. Your visible transcript is
              unaffected; the next completed turn retries automatically.
            </>
          )}
        </p>
      ) : null}
    </div>
  );
}
