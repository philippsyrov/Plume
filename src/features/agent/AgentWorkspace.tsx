// Central agent workspace.
//
// D1.5 carved out the shape of the workspace shell — left navigation,
// central agent surface, right inspector — without committing to a
// chat backend. D6 added a "Selected model" banner. D7 added the
// first interactive surface in this zone: a read-only chat panel
// that round-trips a prompt + assistant reply against the selected
// local model. D7.1 turned that into a streaming surface with a
// Stop button. D8 layered an explicit "Attach current file" control
// onto the chat panel, fed by the file-inspector selection state
// hoisted in `App.tsx`.
//
// D87 cleanup: the center zone used to carry a four-card grid naming
// every safety mode plus a footnote, which crowded the chat panel
// below it. Those cards were descriptive only — the *real* controls
// already live elsewhere: the per-send response mode (chat /
// propose-diff) is the toggle in the chat header (`ModeToggle`), and
// the agent-autonomy mode + gates are the compact Agent settings card
// in the left column (D84). So the cards are gone; the center is now
// just a short orientation line, the selected-model banner, and the
// chat panel itself. Less to read, nothing to overlap.
//
// D98 cleanup: the workspace section dropped its own `ink-panel` border.
// The selected-model banner and the chat panel are already bordered cards,
// so the outer border made them cards-inside-a-card. The center is now the
// primary working surface — the orientation line as plain text, then the
// two cards directly on the paper. The section keeps its own `overflow-y`
// so the surface still scrolls when the chat transcript grows.

import { ChatPanel } from '../chat/ChatPanel';
import type { EditorLineRange } from '../editor/ReadOnlyEditor';
import type { SelectionState } from '../file-tree/FileBrowser';
import { SelectedModelBanner } from '../model-picker/SelectedModelBanner';
import type { SelectedModel } from '../model-picker/useSelectedModel';
import type { MlxServersApi } from '../providers/useMlxServers';

export type AgentWorkspaceProps = {
  selected: SelectedModel | null;
  onClearSelection: () => void;
  /**
   * Selection state from the file inspector. ChatPanel uses it to
   * decide whether the "Attach current file" control is eligible.
   * `null` is allowed for tests/scaffolds that mount this surface
   * without a navigator.
   */
  inspectorSelection: SelectionState | null;
  /**
   * D10: current non-empty text selection inside the inspector's
   * editor, expressed as 1-based line numbers. ChatPanel uses
   * this to flip its attach control between "Attach selection"
   * and "Attach current file". `null` when nothing is selected.
   */
  inspectorLineRange: EditorLineRange | null;
  /**
   * D11: `true` when the trusted project has a root `AGENTS.md`.
   * ChatPanel renders a small "Project instructions" indicator
   * when this is set. Driven by `ProjectMeta.hasAgentsMd` —
   * the backend re-reads on every send, so this is a
   * forward-looking promise rather than a per-message confirmation.
   */
  projectHasInstructions: boolean;
  /**
   * D46: lifecycle bus for Plume-managed MLX servers. ChatPanel
   * reads `handleOf(selected.modelId)` when the current selection
   * is an `mlx-lm` provider so it can thread `handleId` through to
   * `chat.send` (the D45 wire field that drives MLX dispatch).
   */
  mlxServers: MlxServersApi;
};

export function AgentWorkspace({
  selected,
  onClearSelection,
  inspectorSelection,
  inspectorLineRange,
  projectHasInstructions,
  mlxServers,
}: AgentWorkspaceProps) {
  // Pre-selection the center stays calm — a serif headline plus the
  // (empty) banner pointing at the provider panel. Once a model is
  // picked the orientation line shifts to a single short sentence that
  // points at the two places the modes actually live: the response-mode
  // toggle in the chat header, and the Agent settings card on the left.
  // The chat panel always renders (smoke step 10 asserts the
  // visible-but-disabled placeholder), so the workspace always shows the
  // user where chat lives.
  const hasSelection = selected !== null;
  return (
    <section
      className="plume-agent-workspace"
      aria-label="Agent workspace"
      aria-describedby="plume-agent-workspace-status"
    >
      <header className="plume-agent-header">
        <h2>Agent workspace</h2>
        {hasSelection ? (
          <p id="plume-agent-workspace-status" className="plume-agent-subtitle">
            Pick the response mode in the chat header; set the agent mode and
            its file / command gates in the Agent card on the left. Send a
            prompt below — optionally attach one project file as read-only
            context.
          </p>
        ) : (
          <p
            id="plume-agent-workspace-status"
            className="plume-agent-subtitle plume-agent-subtitle-calm"
          >
            Pick a model on the left to start chatting.
          </p>
        )}
      </header>

      <SelectedModelBanner
        selected={selected}
        onClear={onClearSelection}
        mlxServers={mlxServers}
      />

      <ChatPanel
        selected={selected}
        inspectorSelection={inspectorSelection}
        inspectorLineRange={inspectorLineRange}
        projectHasInstructions={projectHasInstructions}
        mlxServers={mlxServers}
      />
    </section>
  );
}
