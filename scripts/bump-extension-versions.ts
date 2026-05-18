#!/usr/bin/env -S deno --allow-read --allow-write

import { readdirSync, readFileSync, writeFileSync, statSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

type BumpKind = "patch" | "minor" | "major";

interface Options {
  bump: BumpKind;
  filter: string[];
  dryRun: boolean;
  setVersion?: string;
}

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const EXTENSIONS_DIR = join(REPO_ROOT, "extensions");

function parseCliArgs(argv: string[]): Options {
  const { values } = parseArgs({
    args: argv,
    options: {
      patch: { type: "boolean" },
      minor: { type: "boolean" },
      major: { type: "boolean" },
      "dry-run": { type: "boolean", short: "n" },
      filter: { type: "string", short: "f", multiple: true },
      set: { type: "string" },
      help: { type: "boolean", short: "h" },
    },
    strict: true,
    allowPositionals: false,
  });

  if (values.help) {
    printHelp();
    process.exit(0);
  }

  const bumpFlags = (["major", "minor", "patch"] as const).filter((k) => values[k]);
  if (bumpFlags.length > 1) {
    throw new Error(`Specify at most one of --patch, --minor, --major`);
  }
  const bump: BumpKind = bumpFlags[0] ?? "patch";

  if (values.set !== undefined && !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(values.set)) {
    throw new Error(`--set value '${values.set}' is not a valid semver`);
  }

  const filter = (values.filter ?? []).flatMap((v) =>
    v.split(",").map((s) => s.trim()).filter(Boolean),
  );

  return {
    bump,
    filter,
    dryRun: values["dry-run"] ?? false,
    setVersion: values.set,
  };
}

function printHelp(): void {
  const msg = [
    "Bump version field in extensions/*/package.json.",
    "",
    "Usage:",
    "  bump-extension-versions [--patch|--minor|--major] [--filter <name,...>] [--dry-run]",
    "  bump-extension-versions --set <x.y.z> [--filter <name,...>] [--dry-run]",
    "",
    "Options:",
    "  --patch            Increment patch (default).",
    "  --minor            Increment minor, reset patch.",
    "  --major            Increment major, reset minor and patch.",
    "  --set <x.y.z>      Set every targeted package to this exact version.",
    "  --filter, -f       Comma-separated list of extension directory names to bump.",
    "                     May be repeated. If omitted, every extension is bumped.",
    "  --dry-run, -n      Print planned changes without writing.",
    "  --help, -h         Show this help.",
  ].join("\n");
  console.log(msg);
}

function bumpSemver(current: string, kind: BumpKind): string {
  const m = /^(\d+)\.(\d+)\.(\d+)(?:-[0-9A-Za-z.-]+)?$/.exec(current);
  if (!m) throw new Error(`Cannot bump non-semver version '${current}'`);
  let [major, minor, patch] = [Number(m[1]), Number(m[2]), Number(m[3])];
  switch (kind) {
    case "major":
      major += 1;
      minor = 0;
      patch = 0;
      break;
    case "minor":
      minor += 1;
      patch = 0;
      break;
    case "patch":
      patch += 1;
      break;
  }
  return `${major}.${minor}.${patch}`;
}

function listExtensionDirs(): string[] {
  return readdirSync(EXTENSIONS_DIR)
    .filter((name) => {
      const full = join(EXTENSIONS_DIR, name);
      try {
        return statSync(full).isDirectory() && statSync(join(full, "package.json")).isFile();
      } catch {
        return false;
      }
    })
    .sort();
}

function detectIndent(text: string): string {
  const m = /\n([ \t]+)"/.exec(text);
  return m ? m[1] : "  ";
}

function main(): void {
  const opts = parseCliArgs(process.argv.slice(2));
  const all = listExtensionDirs();
  const targets = opts.filter.length === 0 ? all : all.filter((d) => opts.filter.includes(d));

  if (opts.filter.length > 0) {
    const unknown = opts.filter.filter((f) => !all.includes(f));
    if (unknown.length > 0) {
      throw new Error(
        `Unknown extension(s): ${unknown.join(", ")}. Available: ${all.join(", ")}`,
      );
    }
  }

  if (targets.length === 0) {
    console.log("No extensions matched.");
    return;
  }

  const action = opts.setVersion ? `set -> ${opts.setVersion}` : `bump ${opts.bump}`;
  console.log(`${opts.dryRun ? "[dry-run] " : ""}${action} (${targets.length} package${targets.length === 1 ? "" : "s"}):`);

  const nameWidth = targets.reduce((w, n) => Math.max(w, n.length), 0);

  for (const dir of targets) {
    const pkgPath = join(EXTENSIONS_DIR, dir, "package.json");
    const raw = readFileSync(pkgPath, "utf8");
    const pkg = JSON.parse(raw) as { version?: string };
    const current = pkg.version;
    if (typeof current !== "string") {
      throw new Error(`${pkgPath} has no string 'version' field`);
    }
    const next = opts.setVersion ?? bumpSemver(current, opts.bump);
    console.log(`  ${dir.padEnd(nameWidth)}  ${current} -> ${next}${current === next ? "  (unchanged)" : ""}`);
    if (opts.dryRun || current === next) continue;

    pkg.version = next;
    const indent = detectIndent(raw);
    const trailingNewline = raw.endsWith("\n") ? "\n" : "";
    writeFileSync(pkgPath, JSON.stringify(pkg, null, indent) + trailingNewline);
  }
}

try {
  main();
} catch (err) {
  console.error(err instanceof Error ? err.message : err);
  process.exit(1);
}
