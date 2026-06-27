// D96: single-step local agent panel — the first *executing* slice.
//
// Sends the user's instruction to the selected, running local MLX model
// (propose-diff prompt), then renders the real D85 event stream the
// backend returns: the model's reply, the read-only `patch.validate` it
// ran, and — if the diff is valid — the apply step held behind the
// approval gate.
//
// D100: the first *mutating* path, patch-only. When the step produced a
// valid diff the backend returns it as `applicableDiff`, and this panel
// shows an explicit Apply button that runs it through the EXISTING
// `patch.apply` (server-side re-validate → checkpoint → atomic write),
// then a Revert button (`patch.revert`). Both reuse the patch verbs — no
// new applier, no validator duplication — and the apply/revert outcome is
// appended to the same event log as `toolStarted`/`toolFinished`/
// `toolFailed` frames (constructed from the real `patch.apply` result), so
// the log shows the full lifecycle. Nothing is ever applied without the
// user's explicit click: the run itself only validates and pauses. There
// is still NO shell execution and no arbitrary tool invocation.
//
// Mirrors AgentDryRunPanel's shape (busy/error/mountedRef + AgentEventLog),
// but the events are real and it needs the selected model + its running
// server handle to send.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { runAgentSingleStep } from '../../lib/api/agent';
import type { AgentEvent, AgentEventEnvelope } from '../../lib/api/agentEvents';
import type { ChatAttachment } from '../../lib/api/chat';
import { isIpcError } from '../../lib/api/errors';
import { applyPatch, revertPatch } from '../../lib/api/patch';
import type { AgentMode } from '../../lib/api/session';
import { AttachBar, describeAttachCandidate, type ChipState } from '../chat/AttachBar';
import type { EditorLineRange } from '../editor/ReadOnlyEditor';
import type { SelectionState } from '../file-tree/FileBrowser';
import type { SelectedModel } from '../model-picker/useSelectedModel';
import type { MlxServersApi } from '../providers/useMlxServers';
import { AgentEventLog } from './AgentEventLog';

export type AgentSingleStepPanelProps = {
  selected: SelectedModel | null;
  mlxServers: MlxServersApi;
  /** The session's agentMode, mirrored from AgentSettingsPanel. A step is
   *  only allowed in `propose-diff` or higher — `chat` is talk-only. `null`
   *  while the mode is still loading (the backend stays authoritative). */
  agentMode?: AgentMode | null;
  /** D99: inspector selection state, the same the chat panel reads, so the
   *  single-step prompt can attach one read-only project file as context.
   *  `null` when no navigator is mounted (tests/scaffolds). */
  inspectorSelection?: SelectionState | null;
  /** D99: current 1-based text selection in the inspector editor, flipping
   *  the attach control to "Attach selection". `null` for a point cursor. */
  inspectorLineRange?: EditorLineRange | null;
};

export function AgentSingleStepPanel({
  selected,
  mlxServers,
  agentMode = null,
  inspectorSelection = null,
  inspectorLineRange = null,
}: AgentSingleStepPanelProps) {
  const [prompt, setPrompt] = useState('');
  const [events, setEvents] = useState<AgentEventEnvelope[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // D99: the pending one-shot file attachment, mirroring the chat panel's
  // chip. Cleared after a successful run so a follow-up step doesn't
  // silently re-fold the same file.
  const [chip, setChip] = useState<ChipState | null>(null);
  // D100: the validated diff the user may apply (from `resp.applicableDiff`),
  // and the apply/revert lifecycle. `null` diff ⇒ no Apply offered.
  const [applicableDiff, setApplicableDiff] = useState<string | null>(null);
  const [applyState, setApplyState] = useState<'idle' | 'applying' | 'applied' | 'failed'>('idle');
  const [checkpoint, setCheckpoint] = useState<string | null>(null);
  const [revertState, setRevertState] = useState<'idle' | 'reverting' | 'reverted' | 'failed'>(
    'idle',
  );

  // Skip post-await state writes if the panel unmounted mid-request.
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  // Resolve the running MLX server for the selected model, if any. Only
  // an MLX model with a live server can run a step.
  const isMlx = selected?.providerId === 'mlx-lm';
  const handle = useMemo(
    () => (selected && isMlx ? mlxServers.handleOf(selected.modelId) : null),
    [selected, isMlx, mlxServers],
  );

  // D99: clear the chip when the inspector goes empty (a project-root
  // change) — the chip's relPath was rooted to the previous project. Same
  // guard the chat panel uses.
  useEffect(() => {
    if (chip !== null && inspectorSelection?.kind === 'empty') {
      setChip(null);
    }
  }, [chip, inspectorSelection]);

  const attachCandidate = useMemo(
    () => describeAttachCandidate(inspectorSelection, inspectorLineRange, chip),
    [inspectorSelection, inspectorLineRange, chip],
  );

  const onAttach = useCallback(() => {
    if (attachCandidate.kind !== 'eligible') return;
    setChip({
      relPath: attachCandidate.relPath,
      bytes: attachCandidate.bytes,
      lineRange: attachCandidate.lineRange,
    });
  }, [attachCandidate]);

  const onClearChip = useCallback(() => setChip(null), []);

  // The agentMode axis: `chat` is talk-only. We only block when we know the
  // mode is chat; while it's still loading (`null`) we defer to the backend,
  // which rejects a chat-mode step authoritatively.
  const modeBlocked = agentMode === 'chat';
  const trimmed = prompt.trim();
  const canRun = !busy && !modeBlocked && isMlx && handle != null && trimmed.length > 0;

  // One honest line for why Run is unavailable, in priority order — the
  // mode gate is the most fundamental, so it comes first.
  const blockedReason = modeBlocked
    ? 'Switch Agent mode to Propose diff or higher to run a step.'
    : !isMlx
      ? 'Select a local (MLX) model to run a step.'
      : handle == null
        ? 'Start the selected model to run a step.'
        : trimmed.length === 0
          ? 'Type an instruction to run a step.'
          : null;

  const onRun = useCallback(async () => {
    if (!selected || handle == null) return;
    // Build the wire attachment from the chip — line range is all-or-nothing
    // (both startLine + endLine or neither; the backend rejects half a range).
    const attachment: ChatAttachment | undefined = chip
      ? {
          kind: 'projectFile',
          relPath: chip.relPath,
          ...(chip.lineRange
            ? { startLine: chip.lineRange.startLine, endLine: chip.lineRange.endLine }
            : {}),
        }
      : undefined;
    setBusy(true);
    setError(null);
    // D100: a new run supersedes any prior diff — drop the mutation controls
    // up front (before the await), so a stale Apply/Revert can't fire against
    // the previous run's diff while this one is in flight, and can't linger if
    // this run fails. A successful run repopulates `applicableDiff` below.
    setApplicableDiff(null);
    setApplyState('idle');
    setCheckpoint(null);
    setRevertState('idle');
    try {
      const resp = await runAgentSingleStep({
        prompt: trimmed,
        providerId: 'mlx-lm',
        modelId: selected.modelId,
        handleId: handle.id,
        ...(attachment ? { attachment } : {}),
      });
      if (mountedRef.current) {
        setEvents(resp.events);
        // One-shot: the file was folded into this step's prompt. Clear it so
        // the next step doesn't silently re-attach the same context (the
        // backend accepted the run — nothing to restore).
        setChip(null);
        // `applicableDiff` is set only for a validated diff, so an invalid
        // diff / blocked tool / no-diff reply offers no Apply. The lifecycle
        // (applyState/checkpoint/revertState) was already reset at run start.
        setApplicableDiff(resp.applicableDiff ?? null);
      }
    } catch (err) {
      if (mountedRef.current) {
        setError(isIpcError(err) ? `IPC error: ${err.kind}` : 'Single step failed.');
      }
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  }, [selected, handle, trimmed, chip]);

  // D100: append synthesized lifecycle frames to the event log. The frames
  // are constructed from the REAL `patch.apply` / `patch.revert` result (the
  // apply already happened on disk), continuing the backend stream's `seq`.
  const appendEvents = useCallback((next: AgentEvent[]) => {
    setEvents((prev) => {
      let seq = prev.length === 0 ? 0 : prev[prev.length - 1].seq + 1;
      const tsMs = Date.now();
      return [...prev, ...next.map((event) => ({ ...event, seq: seq++, tsMs }))];
    });
  }, []);

  // D100: explicit apply. Never auto-fires — only this click runs a write,
  // and it goes through the existing `patch.apply` (re-validate → checkpoint
  // → atomic write → rollback-on-failure). The `apply-1` callId continues the
  // backend's `toolProposed`/`approvalRequired` lifecycle for that write.
  const onApply = useCallback(async () => {
    // `busy` guards the in-flight window: a new run is superseding this diff,
    // so the write must not fire even if a click sneaks in before re-render.
    if (!applicableDiff || busy || applyState === 'applying' || applyState === 'applied') return;
    setApplyState('applying');
    try {
      const resp = await applyPatch({ diff: applicableDiff });
      if (!mountedRef.current) return;
      if (resp.applied) {
        setCheckpoint(resp.checkpoint);
        setApplyState('applied');
        const fileWord = resp.touched.length === 1 ? 'file' : 'files';
        appendEvents([
          { kind: 'toolStarted', callId: 'apply-1', tool: 'write' },
          {
            kind: 'toolFinished',
            callId: 'apply-1',
            tool: 'write',
            summary: `applied — ${resp.touched.length} ${fileWord} · checkpoint ${resp.checkpoint.slice(0, 8)}`,
          },
          { kind: 'done', summary: 'patch applied — Revert undoes it' },
        ]);
      } else {
        setApplyState('failed');
        const detail = resp.details[0] ? `: ${resp.details[0].message}` : '';
        appendEvents([
          { kind: 'toolStarted', callId: 'apply-1', tool: 'write' },
          {
            kind: 'toolFailed',
            callId: 'apply-1',
            tool: 'write',
            error: `apply failed (${resp.reason})${detail}`,
          },
          { kind: 'done', summary: 'apply failed — nothing changed on disk' },
        ]);
      }
    } catch (err) {
      if (!mountedRef.current) return;
      setApplyState('failed');
      appendEvents([
        { kind: 'toolStarted', callId: 'apply-1', tool: 'write' },
        {
          kind: 'toolFailed',
          callId: 'apply-1',
          tool: 'write',
          error: isIpcError(err) ? `apply unavailable: ${err.kind}` : 'apply failed',
        },
        { kind: 'done', summary: null },
      ]);
    }
  }, [applicableDiff, busy, applyState, appendEvents]);

  // D100: explicit revert of the applied patch via the existing
  // `patch.revert` (drift-detect → restore pre-apply files all-or-nothing).
  const onRevert = useCallback(async () => {
    if (!checkpoint || busy || revertState === 'reverting' || revertState === 'reverted') return;
    setRevertState('reverting');
    try {
      const resp = await revertPatch({ checkpoint });
      if (!mountedRef.current) return;
      if (resp.reverted) {
        setRevertState('reverted');
        const fileWord = resp.restored.length === 1 ? 'file' : 'files';
        appendEvents([
          { kind: 'toolProposed', callId: 'revert-1', tool: 'write', summary: 'revert the applied patch' },
          { kind: 'toolStarted', callId: 'revert-1', tool: 'write' },
          {
            kind: 'toolFinished',
            callId: 'revert-1',
            tool: 'write',
            summary: `reverted — ${resp.restored.length} ${fileWord} restored`,
          },
          { kind: 'done', summary: 'patch reverted' },
        ]);
      } else {
        setRevertState('failed');
        const detail = resp.details[0] ? `: ${resp.details[0].message}` : '';
        appendEvents([
          { kind: 'toolProposed', callId: 'revert-1', tool: 'write', summary: 'revert the applied patch' },
          {
            kind: 'toolFailed',
            callId: 'revert-1',
            tool: 'write',
            error: `revert failed (${resp.reason})${detail}`,
          },
          { kind: 'done', summary: 'revert failed' },
        ]);
      }
    } catch (err) {
      if (!mountedRef.current) return;
      setRevertState('failed');
      appendEvents([
        {
          kind: 'toolFailed',
          callId: 'revert-1',
          tool: 'write',
          error: isIpcError(err) ? `revert unavailable: ${err.kind}` : 'revert failed',
        },
        { kind: 'done', summary: null },
      ]);
    }
  }, [checkpoint, busy, revertState, appendEvents]);

  return (
    <section className="plume-agent-singlestep ink-panel" aria-label="Single-step agent">
      <div className="plume-agent-singlestep-head">
        <h3>Run one step</h3>
        <button
          type="button"
          className="ink-button"
          onClick={() => void onRun()}
          disabled={!canRun}
        >
          {busy ? 'Running…' : 'Run step'}
        </button>
      </div>
      <p className="plume-agent-singlestep-hint">
        Sends your instruction to the selected local model and asks for a diff. Plume validates it
        and shows the real event stream — applying stays behind approval, so nothing is written.
        Optionally attach one project file from the inspector as read-only context.
      </p>
      <textarea
        className="plume-agent-singlestep-prompt"
        aria-label="Step instruction"
        placeholder="e.g. make greet() return an f-string"
        value={prompt}
        onChange={(e) => setPrompt(e.target.value)}
        rows={3}
        disabled={busy}
      />
      <AttachBar
        chip={chip}
        candidate={attachCandidate}
        onAttach={onAttach}
        onClear={onClearChip}
        disabled={busy}
      />
      {blockedReason ? <p className="plume-agent-singlestep-note">{blockedReason}</p> : null}
      {error ? (
        <p className="plume-agent-singlestep-error" role="alert">
          {error}
        </p>
      ) : null}
      <AgentEventLog events={events} />
      {applicableDiff ? (
        <div className="plume-agent-singlestep-apply" role="group" aria-label="Apply the proposed diff">
          {applyState !== 'applied' ? (
            <button
              type="button"
              className="ink-button"
              onClick={() => void onApply()}
              disabled={busy || applyState === 'applying'}
            >
              {applyState === 'applying' ? 'Applying…' : 'Apply diff'}
            </button>
          ) : (
            <button
              type="button"
              className="ink-button"
              onClick={() => void onRevert()}
              disabled={busy || revertState === 'reverting' || revertState === 'reverted'}
            >
              {revertState === 'reverting'
                ? 'Reverting…'
                : revertState === 'reverted'
                  ? 'Reverted'
                  : 'Revert'}
            </button>
          )}
          <span className="plume-agent-singlestep-apply-note" role="status">
            {applyState === 'applied'
              ? revertState === 'reverted'
                ? 'Restored to the pre-apply state.'
                : 'Applied behind a checkpoint — Revert undoes it.'
              : applyState === 'failed'
                ? 'Apply failed — see the log. You can try again.'
                : 'Writes the validated diff via patch.apply (checkpoint + atomic write).'}
          </span>
        </div>
      ) : null}
    </section>
  );
}
