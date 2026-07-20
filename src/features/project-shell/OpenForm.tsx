// The open-project form from the pre-project shell. Extracted from
// App.tsx verbatim when D132 pushed it over the decomposition amber
// cap (docs/DECOMPOSITION.md § Cadence rule) — a pure move, not a
// rewrite.

import { useEffect, useRef, useState } from 'react';
import { chooseProjectFolder } from '../../lib/api/project';
import { useProjectFolderDrop } from './useProjectFolderDrop';

type OpenFormProps = {
  path: string;
  busy: boolean;
  onOpen: (path: string) => void;
  onChange: (path: string) => void;
  /** D49: take the user to no-project chat without opening any
   *  folder. The button sits below the Open form so the project
   *  flow stays the primary affordance. */
  onChatOnly: () => void;
};

export function OpenForm({ path, busy, onOpen, onChange, onChatOnly }: OpenFormProps) {
  const trimmed = path.trim();
  const canOpen = trimmed.length > 0 && !busy;
  const [manualOpen, setManualOpen] = useState(false);
  const [choosing, setChoosing] = useState(false);
  const [pickerError, setPickerError] = useState<string | null>(null);
  const mountedRef = useRef(true);
  const choosingRef = useRef(false);

  useEffect(() => () => {
    mountedRef.current = false;
  }, []);

  const openCandidate = (candidate: string) => {
    if (busy || choosingRef.current) return;
    onChange(candidate);
    onOpen(candidate);
  };

  const chooseFolder = async () => {
    if (busy || choosingRef.current) return;
    choosingRef.current = true;
    setChoosing(true);
    setPickerError(null);
    try {
      const candidate = await chooseProjectFolder();
      if (!mountedRef.current) return;
      choosingRef.current = false;
      setChoosing(false);
      if (candidate !== null) openCandidate(candidate);
    } catch {
      if (!mountedRef.current) return;
      choosingRef.current = false;
      setChoosing(false);
      setPickerError('Couldn’t open the folder chooser. Enter a path instead.');
    }
  };

  useProjectFolderDrop({ busy: busy || choosing, onCandidate: openCandidate });

  return (
    <section className="plume-empty ink-panel">
      <p>Open a project folder to use its files and project tools.</p>
      <button
        type="button"
        className="ink-button plume-open-project-choose"
        disabled={busy || choosing}
        onClick={() => void chooseFolder()}
      >
        {choosing ? 'Choosing…' : 'Choose folder…'}
      </button>
      <div className="plume-open-project-drop" aria-label="Folder drop area">
        <strong>Drop a folder from Finder</strong>
        <span>Plume will ask you to trust it before using project context.</span>
      </div>
      <div className="plume-open-project-manual">
        <button
          type="button"
          className="plume-open-project-manual-toggle"
          aria-expanded={manualOpen}
          onClick={() => setManualOpen((open) => !open)}
        >
          Enter path instead
        </button>
        {manualOpen ? (
          <form
            className="plume-open-form"
            onSubmit={(event) => {
              event.preventDefault();
              if (canOpen) onOpen(trimmed);
            }}
          >
            <label className="plume-open-form-label">
              Project folder
              <input
                type="text"
                className="plume-open-form-input"
                value={path}
                placeholder="Paste a folder path"
                spellCheck={false}
                autoCapitalize="off"
                autoCorrect="off"
                onChange={(event) => onChange(event.target.value)}
                disabled={busy}
              />
            </label>
            <button type="submit" className="ink-button" disabled={!canOpen}>
              {busy ? 'Opening…' : 'Open'}
            </button>
          </form>
        ) : null}
      </div>
      {pickerError ? <p className="plume-open-project-error" role="alert">{pickerError}</p> : null}
      {/* D49: secondary affordance — chat with a local model without
          opening a project. File tree / inspector / patch
          stay disabled in that mode; this is for the "I just want
          to talk to my local model" path. */}
      <div className="plume-open-form-secondary">
        <button
          type="button"
          className="ink-button plume-open-form-chat-only"
          onClick={onChatOnly}
          disabled={busy}
          aria-label="Chat with a local model without opening a project"
        >
          Chat without a project
        </button>
        <p className="plume-open-form-hint">
          Talk to a local model right away. No project files, editing,
          or agent tools. You can still attach items from About you.
        </p>
      </div>
    </section>
  );
}
