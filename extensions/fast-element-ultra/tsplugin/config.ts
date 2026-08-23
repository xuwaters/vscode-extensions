/**
 * Plugin settings: the `fastElementUltra.*` object the extension forwards via
 * `configurePlugin`, normalised, with custom-data files resolved to parsed
 * JSON here — the engine does no I/O.
 */

import * as fs from 'fs';
import * as path from 'path';

import type { EngineConfig, RuleSetting } from './protocol.js';
import type { LogLevel } from './logger.js';

export interface PluginSettings {
  disable?: boolean;
  strict?: boolean;
  logging?: LogLevel;
  dontShowSuggestions?: boolean;
  htmlTemplateTags?: string[];
  cssTemplateTags?: string[];
  maxProjectImportDepth?: number;
  maxNodeModuleImportDepth?: number;
  globalTags?: string[];
  globalAttributes?: string[];
  globalEvents?: string[];
  customHtmlData?: unknown;
  /** From `html.experimental.customData`, forwarded by the extension. */
  htmlCustomData?: unknown;
  rules?: Record<string, RuleSetting>;
}

export interface ResolvedConfig {
  disable: boolean;
  logging: LogLevel;
  htmlTemplateTags: string[];
  cssTemplateTags: string[];
  engine: EngineConfig;
}

export function resolveConfig(
  settings: PluginSettings | undefined,
  projectRoot: string,
): ResolvedConfig {
  const s = settings ?? {};
  return {
    disable: s.disable === true,
    logging: s.logging ?? 'off',
    htmlTemplateTags: s.htmlTemplateTags?.length ? s.htmlTemplateTags : ['html'],
    cssTemplateTags: s.cssTemplateTags?.length ? s.cssTemplateTags : ['css'],
    engine: {
      strict: s.strict === true,
      rules: s.rules ?? {},
      globalTags: s.globalTags ?? [],
      globalAttributes: s.globalAttributes ?? [],
      globalEvents: s.globalEvents ?? [],
      dontShowSuggestions: s.dontShowSuggestions === true,
      customHtmlData: [
        ...loadCustomData(s.customHtmlData, projectRoot),
        ...loadCustomData(s.htmlCustomData, projectRoot),
      ],
      maxProjectImportDepth: s.maxProjectImportDepth ?? -1,
      maxNodeModuleImportDepth: s.maxNodeModuleImportDepth ?? 1,
    },
  };
}

/** A path, an inline object, or an array of either. Bad entries are skipped. */
function loadCustomData(value: unknown, projectRoot: string): unknown[] {
  if (value == null) return [];
  if (Array.isArray(value)) {
    return value.flatMap((entry) => loadCustomData(entry, projectRoot));
  }
  if (typeof value === 'object') return [value];
  if (typeof value === 'string') {
    try {
      const resolved = path.isAbsolute(value) ? value : path.join(projectRoot, value);
      return [JSON.parse(fs.readFileSync(resolved, 'utf8'))];
    } catch {
      return [];
    }
  }
  return [];
}
