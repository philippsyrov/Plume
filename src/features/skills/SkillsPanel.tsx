import { useCallback, useEffect, useRef, useState } from 'react';

import {
  applySkill,
  listSkills,
  loadSkill,
  previewSkill,
  type SkillDraft,
} from '../../lib/api/skills';
import { ChatSkillDraft } from './ChatSkillDraft';

const EMPTY_DRAFT: SkillDraft = { slug: '', name: '', description: '', body: '' };

type ListResult = Awaited<ReturnType<typeof listSkills>>;
type LoadedSkill = Awaited<ReturnType<typeof loadSkill>>;
type Preview = Awaited<ReturnType<typeof previewSkill>>;

export function SkillsPanel() {
  const [list, setList] = useState<ListResult | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const [selected, setSelected] = useState<LoadedSkill | null>(null);
  const [selectionBusy, setSelectionBusy] = useState(false);
  const [selectionError, setSelectionError] = useState<string | null>(null);
  const selectionRequest = useRef(0);
  const [draft, setDraft] = useState<SkillDraft>(EMPTY_DRAFT);
  const [preview, setPreview] = useState<Preview | null>(null);
  const [previewBusy, setPreviewBusy] = useState(false);
  const previewRequest = useRef(0);
  const [applyBusy, setApplyBusy] = useState(false);
  const [promotionBusy, setPromotionBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [existingContent, setExistingContent] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [promotionSummary, setPromotionSummary] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setListError(null);
    try {
      setList(await listSkills());
    } catch (error) {
      setListError(errorMessage(error));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const updateDraft = (field: keyof SkillDraft, value: string) => {
    setDraft((current) => ({ ...current, [field]: value }));
    setPreview(null);
    setActionError(null);
    setExistingContent(null);
    setNotice(null);
    setPromotionSummary(null);
  };

  const selectSkill = async (slug: string) => {
    const request = ++selectionRequest.current;
    setSelectionBusy(true);
    setSelectionError(null);
    setSelected(null);
    try {
      const skill = await loadSkill(slug);
      if (selectionRequest.current === request) setSelected(skill);
    } catch (error) {
      if (selectionRequest.current === request) setSelectionError(errorMessage(error));
    } finally {
      if (selectionRequest.current === request) setSelectionBusy(false);
    }
  };

  const requestPreview = async () => {
    if (previewBusy || promotionBusy || !draftComplete(draft)) return;
    const request = ++previewRequest.current;
    setPreviewBusy(true);
    setActionError(null);
    setExistingContent(null);
    setNotice(null);
    try {
      const response = await previewSkill(draft);
      if (previewRequest.current === request) setPreview(response);
    } catch (error) {
      if (previewRequest.current === request) setActionError(errorMessage(error));
    } finally {
      if (previewRequest.current === request) setPreviewBusy(false);
    }
  };

  const confirmApply = async () => {
    if (!preview || preview.exists || applyBusy || promotionBusy) return;
    setApplyBusy(true);
    setActionError(null);
    setExistingContent(null);
    setNotice(null);
    try {
      const response = await applySkill(draft);
      if (response.ok) {
        setDraft(EMPTY_DRAFT);
        setPreview(null);
        setNotice('Skill saved.');
        await refresh();
      } else {
        setActionError(response.message);
        if (response.reason === 'alreadyExists') {
          try {
            const existing = await loadSkill(draft.slug);
            setExistingContent(existing.content);
          } catch (error) {
            setActionError(`${response.message} Could not load the existing file: ${errorMessage(error)}`);
          }
        }
      }
    } catch (error) {
      setActionError(errorMessage(error));
    } finally {
      setApplyBusy(false);
    }
  };

  return (
    <section className="ink-panel plume-skills-card" aria-labelledby="plume-skills-title">
      <header className="plume-skills-header">
        <div>
          <h4 id="plume-skills-title">Project skills</h4>
          <p>Project-local procedures. They grant no permissions and run nothing.</p>
        </div>
      </header>

      <div className="plume-skills-layout">
        <div className="plume-skills-library" aria-label="Project skill library">
          <h5>Library</h5>
          {!list && !listError ? <p role="status">Loading skills…</p> : null}
          {listError ? <p role="alert">{listError}</p> : null}
          {list?.skills.length === 0 && list.invalid.length === 0 ? (
            <p className="plume-skills-muted">No project skills yet.</p>
          ) : null}
          {list?.skills.map((skill) => (
            <button
              key={skill.slug}
              type="button"
              className="plume-skills-row"
              onClick={() => void selectSkill(skill.slug)}
            >
              <strong>{skill.name}</strong>
              <span>{skill.description}</span>
              <code>{skill.slug}</code>
            </button>
          ))}
          {list?.invalid.map((entry) => (
            <div className="plume-skills-invalid" key={entry.slug} role="note">
              <strong>{entry.slug}</strong>
              <span>{entry.reason}</span>
            </div>
          ))}
          {selectionBusy ? <p role="status">Loading skill…</p> : null}
          {selectionError ? <p role="alert">{selectionError}</p> : null}
          {selected ? (
            <section className="plume-skills-content" aria-label={`${selected.name} file`}>
              <h5>Exact SKILL.md</h5>
              <pre>{selected.content}</pre>
            </section>
          ) : null}
        </div>

        <form
          className="plume-skills-form"
          onSubmit={(event) => {
            event.preventDefault();
            void requestPreview();
          }}
        >
          <h5>Create a skill</h5>
          <ChatSkillDraft
            disabled={previewBusy || applyBusy}
            onBusyChange={setPromotionBusy}
            onPromotionStart={() => {
              previewRequest.current += 1;
              setPreviewBusy(false);
              setPreview(null);
              setActionError(null);
              setExistingContent(null);
            }}
            onDraft={(promotion) => {
              setDraft(promotion.draft);
              setPreview(null);
              setActionError(null);
              setExistingContent(null);
              setNotice('Draft filled — review it, then preview the exact SKILL.md.');
              const selected = promotion.source.entryIndexes.length;
              const redactions = promotion.redactionCount;
              setPromotionSummary(
                `${promotion.source.title} · ${selected} selected ${selected === 1 ? 'entry' : 'entries'}`
                + (redactions > 0 ? ` · ${redactions} secret-like ${redactions === 1 ? 'value was' : 'values were'} redacted` : ''),
              );
            }}
          />
          <label>
            Skill slug
            <input
              value={draft.slug}
              disabled={previewBusy || applyBusy || promotionBusy}
              onChange={(event) => updateDraft('slug', event.target.value)}
              autoCapitalize="off"
              autoCorrect="off"
              spellCheck={false}
            />
          </label>
          <label>
            Skill name
            <input
              value={draft.name}
              disabled={previewBusy || applyBusy || promotionBusy}
              onChange={(event) => updateDraft('name', event.target.value)}
            />
          </label>
          <label>
            Skill description
            <textarea
              rows={2}
              value={draft.description}
              disabled={previewBusy || applyBusy || promotionBusy}
              onChange={(event) => updateDraft('description', event.target.value)}
            />
          </label>
          <label>
            Skill instructions
            <textarea
              rows={7}
              value={draft.body}
              disabled={previewBusy || applyBusy || promotionBusy}
              onChange={(event) => updateDraft('body', event.target.value)}
            />
          </label>
          <button
            type="submit"
            className="ink-button"
            disabled={!draftComplete(draft) || previewBusy || applyBusy || promotionBusy}
          >
            {previewBusy ? 'Preparing preview…' : 'Preview skill'}
          </button>
          {notice ? <p className="plume-skills-notice" role="status">{notice}</p> : null}
          {promotionSummary ? <p className="plume-skills-muted">{promotionSummary}</p> : null}
          {actionError ? <p role="alert">{actionError}</p> : null}
          {preview ? (
            <section className="plume-skills-preview" role="region" aria-label="Skill preview">
              <div className="plume-skills-preview-heading">
                <h5>Exact file preview</h5>
                {preview.exists ? <span className="ink-badge">already exists</span> : null}
              </div>
              <pre>{preview.content}</pre>
              <button
                type="button"
                className="ink-button"
                onClick={() => void confirmApply()}
                disabled={preview.exists || applyBusy || promotionBusy}
              >
                {applyBusy ? 'Applying…' : 'Apply skill'}
              </button>
            </section>
          ) : null}
          {existingContent ? (
            <section className="plume-skills-content" aria-label="Existing skill file">
              <h5>Exact existing SKILL.md</h5>
              <pre>{existingContent}</pre>
            </section>
          ) : null}
        </form>
      </div>
    </section>
  );
}

function draftComplete(draft: SkillDraft): boolean {
  return Boolean(draft.slug.trim() && draft.name.trim() && draft.description.trim() && draft.body.trim());
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
