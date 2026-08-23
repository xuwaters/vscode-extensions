/**
 * wx-fast-element-tsplugin — the TypeScript server plugin.
 *
 * tsserver requires this module from `node_modules/wx-fast-element-tsplugin/`
 * inside the extension's install directory and calls `create` once per
 * project. The WASM engine is instantiated per project so two projects never
 * see each other's registries; the compiled module is shared by Node's
 * require cache.
 *
 * Supported TypeScript: >= 5.5, < 8. Outside that range the plugin logs why
 * and returns the language service untouched (closes RFC 011 open question 3).
 */

import type * as tslib from 'typescript';

import { SafeEngine } from './engine.js';
import { Logger } from './logger.js';
import type { PluginSettings } from './config.js';
import { decorateLanguageService, FastService } from './service.js';

const MIN_TS = [5, 5] as const;
const MAX_TS_MAJOR_EXCLUSIVE = 8;

interface ProjectState {
  service: FastService;
  logger: Logger;
}

const projects = new Set<ProjectState>();

function versionSupported(versionMajorMinor: string): boolean {
  const [major, minor] = versionMajorMinor.split('.').map((n) => Number.parseInt(n, 10));
  if (Number.isNaN(major) || Number.isNaN(minor)) return false;
  if (major < MIN_TS[0] || (major === MIN_TS[0] && minor < MIN_TS[1])) return false;
  return major < MAX_TS_MAJOR_EXCLUSIVE;
}

const factory: tslib.server.PluginModuleFactory = (mod) => {
  const ts = mod.typescript;

  return {
    create(info: tslib.server.PluginCreateInfo): tslib.LanguageService {
      const logger = new Logger((message) =>
        info.project.projectService.logger.info(message),
      );
      let settings = info.config as PluginSettings | undefined;
      logger.level = settings?.logging ?? 'off';

      if (!versionSupported(ts.versionMajorMinor)) {
        logger.error(
          `TypeScript ${ts.version} is outside the supported range (>=${MIN_TS.join('.')} <${MAX_TS_MAJOR_EXCLUSIVE}); FAST analysis is off, TypeScript is unmodified.`,
        );
        return info.languageService;
      }

      const engine = new SafeEngine(logger);
      const service = new FastService({
        ts,
        languageService: info.languageService,
        languageServiceHost: info.languageServiceHost,
        engine,
        logger,
        getSettings: () => settings,
        projectRoot: info.project.getCurrentDirectory(),
      });
      const state: ProjectState = { service, logger };
      projects.add(state);

      // Let the service observe settings updates delivered after create.
      const applySettings = (next: PluginSettings | undefined): void => {
        settings = next;
        service.onSettingsChanged();
      };
      settingsAppliers.set(state, applySettings);

      // Custom protocol handlers: the extension's workspace-analysis command
      // and status item reach the plugin through `typescript.tsserverRequest`.
      const session = info.session;
      if (session && !sessionsWithHandlers.has(session)) {
        sessionsWithHandlers.add(session);
        try {
          session.addProtocolHandler('_fast-element-ultra:analyze', () => ({
            response: collectWorkspaceDiagnostics(),
            responseRequired: true,
          }));
          session.addProtocolHandler('_fast-element-ultra:status', () => ({
            response: {
              engineState: firstEngineState(),
              tsVersion: ts.version,
            },
            responseRequired: true,
          }));
        } catch {
          // An already-registered handler (server restart edge) is fine.
        }
      }

      logger.debug(`plugin created for ${info.project.getProjectName()}`);
      return decorateLanguageService(service, info.languageService, logger);
    },

    onConfigurationChanged(config: PluginSettings): void {
      for (const state of projects) {
        settingsAppliers.get(state)?.(config);
      }
    },
  };
};

const settingsAppliers = new WeakMap<ProjectState, (s: PluginSettings | undefined) => void>();
const sessionsWithHandlers = new WeakSet<object>();

function collectWorkspaceDiagnostics(): unknown[] {
  const out: unknown[] = [];
  for (const state of projects) {
    try {
      out.push(...state.service.workspaceDiagnostics());
    } catch (error) {
      state.logger.error(
        `workspace analysis failed: ${error instanceof Error ? error.message : error}`,
      );
    }
  }
  return out;
}

function firstEngineState(): string {
  for (const state of projects) {
    return state.service.engineState();
  }
  return 'unavailable';
}

export default factory;
