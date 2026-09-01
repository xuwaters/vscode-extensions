// The `wgsl.*` and `glsl.*` settings, gathered into the one object the server
// deserializes.
//
// The shape here has to match `wgsl_lsp_core::settings::Settings` exactly. It
// is spelled out rather than read generically so a typo is a TypeScript error
// here instead of a silently-defaulted setting there.

import * as vscode from 'vscode';

/** Everything the server reads, for one language. */
export interface LanguageSettings {
  validate: { onSave: boolean; onType: boolean };
  // `defaultVersion` is declared under `glsl.` only; WGSL has one version, so
  // the server carries the field on the shared shape and never reads it there.
  defaultVersion: string;
  completion: { enabled: boolean };
  semanticTokens: boolean;
  inlayHints: { enabled: boolean; types: boolean; parameterNames: boolean };
  format: { enable: boolean; indentWidth: number };
  embedded: { enabled: boolean; diagnostics: boolean };
}

export interface Settings {
  wgsl: LanguageSettings;
  glsl: LanguageSettings;
}

/** The configuration sections the server cares about. */
export const SECTIONS = ['wgsl', 'glsl'] as const;

export function read(scope?: vscode.Uri): Settings {
  return { wgsl: readSection('wgsl', scope), glsl: readSection('glsl', scope) };
}

function readSection(section: string, scope?: vscode.Uri): LanguageSettings {
  const config = vscode.workspace.getConfiguration(section, scope);
  return {
    validate: {
      onSave: config.get<boolean>('validate.onSave', true),
      onType: config.get<boolean>('validate.onType', false),
    },
    defaultVersion: config.get<string>('defaultVersion', ''),
    completion: { enabled: config.get<boolean>('completion.enabled', true) },
    semanticTokens: config.get<boolean>('semanticTokens', true),
    inlayHints: {
      enabled: config.get<boolean>('inlayHints.enabled', false),
      types: config.get<boolean>('inlayHints.types', true),
      parameterNames: config.get<boolean>('inlayHints.parameterNames', true),
    },
    format: {
      enable: config.get<boolean>('format.enable', false),
      indentWidth: config.get<number>('format.indentWidth', 4),
    },
    embedded: {
      enabled: config.get<boolean>('embedded.enabled', true),
      diagnostics: config.get<boolean>('embedded.diagnostics', false),
    },
  };
}

/** Whether a configuration change touched anything the server reads. */
export function affectsServer(event: vscode.ConfigurationChangeEvent): boolean {
  return SECTIONS.some((section) => event.affectsConfiguration(section));
}

/**
 * Settings whose values are baked into the server's *capabilities* rather than
 * read per request. Changing one of these needs a restart, because a client
 * only reads capabilities once.
 */
export const RESTART_SETTINGS = [
  'wgsl.semanticTokens',
  'glsl.semanticTokens',
  'wgsl.inlayHints.enabled',
  'glsl.inlayHints.enabled',
  'wgsl.format.enable',
  'glsl.format.enable',
  'wgsl.completion.enabled',
  'glsl.completion.enabled',
] as const;

export function needsRestart(event: vscode.ConfigurationChangeEvent): boolean {
  return RESTART_SETTINGS.some((setting) => event.affectsConfiguration(setting));
}
