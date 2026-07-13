import { invokeIpc } from './ipc';

export type SkillMetadata = {
  slug: string;
  name: string;
  description: string;
};

export type InvalidSkill = {
  slug: string;
  reason: string;
};

export type SkillIndex = {
  skills: SkillMetadata[];
  invalid: InvalidSkill[];
};

export type SkillDocument = SkillMetadata & {
  body: string;
  content: string;
};

export type SkillDraft = {
  slug: string;
  name: string;
  description: string;
  body: string;
};

export type SkillPreview = {
  slug: string;
  content: string;
  exists: boolean;
};

export type SkillApplyResponse =
  | { ok: true; skill: SkillMetadata }
  | { ok: false; reason: 'alreadyExists' | 'capacityReached'; message: string };

export type SkillPromotionPreview = {
  draft: SkillDraft;
  source: {
    sessionId: string;
    title: string;
    entryIndexes: number[];
  };
  redactionCount: number;
};

export type SkillPromotionPayload = {
  sessionId: string;
  entryIndexes: number[];
  snapshotToken: string;
};

export type SkillPromotionContext = {
  sessionId: string;
  title: string;
  snapshotToken: string;
  entries: Array<{
    index: number;
    role: 'user' | 'assistant';
    content: string;
  }>;
  excludedCount: number;
};

export function listSkills(): Promise<SkillIndex> {
  return invokeIpc<Record<string, never>, SkillIndex>('skills_list', {});
}

export function loadSkill(slug: string): Promise<SkillDocument> {
  return invokeIpc<{ slug: string }, SkillDocument>('skills_load', { slug });
}

export function previewSkill(draft: SkillDraft): Promise<SkillPreview> {
  return invokeIpc<SkillDraft, SkillPreview>('skills_preview', draft);
}

export function applySkill(draft: SkillDraft): Promise<SkillApplyResponse> {
  return invokeIpc<SkillDraft, SkillApplyResponse>('skills_apply', draft);
}

export function previewSkillPromotion(
  payload: SkillPromotionPayload,
): Promise<SkillPromotionPreview> {
  return invokeIpc<SkillPromotionPayload, SkillPromotionPreview>(
    'skills_promote_preview',
    payload,
  );
}

export function loadSkillPromotionContext(sessionId: string): Promise<SkillPromotionContext> {
  return invokeIpc<{ sessionId: string }, SkillPromotionContext>('skills_promotion_context', {
    sessionId,
  });
}
