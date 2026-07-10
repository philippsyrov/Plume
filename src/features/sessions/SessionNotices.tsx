// D63B: quiet status strip for the session layer — the streaming
// switch block / load failure (`notice`, polite) and transcript-save
// failures (`saveError`, assertive). Rendered by the shell above the
// chat surface so blocked actions and storage problems are visible
// instead of silent (spec acceptance: storage failures are visible
// and recoverable without crashing Plume).

export function SessionNotices({
  notice,
  saveError,
}: {
  notice: string | null;
  saveError: string | null;
}) {
  if (notice === null && saveError === null) return null;
  return (
    <div className="plume-session-notices">
      {notice !== null ? (
        <p className="plume-session-notice" role="status">
          {notice}
        </p>
      ) : null}
      {saveError !== null ? (
        <p className="plume-session-notice plume-session-notice-error" role="alert">
          Chat history could not be saved: {saveError}. Your visible transcript is
          unaffected; the next completed turn retries automatically.
        </p>
      ) : null}
    </div>
  );
}
