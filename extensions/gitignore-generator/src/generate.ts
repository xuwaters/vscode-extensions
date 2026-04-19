import type { Template } from './templates.js';

const SECTION_MARKER = '### Gitignore Generator: ';

export function buildGitignore(templates: Template[]): string {
  if (templates.length === 0) return '';
  const sections = templates.map(t => `${SECTION_MARKER}${t.label}\n${t.content.trimEnd()}\n`);
  return sections.join('\n');
}

export function appendGitignore(existing: string, templates: Template[]): string {
  const addition = buildGitignore(templates);
  if (!addition) return existing;
  const trimmed = existing.replace(/\s+$/, '');
  if (trimmed.length === 0) return addition;
  return `${trimmed}\n\n${addition}`;
}
