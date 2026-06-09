#!/usr/bin/env node

import fs from "node:fs/promises";
import path from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";

const root = process.cwd();
const version = process.argv[2];
const dryRun = process.argv.includes("--dry-run");
const noGit = process.argv.includes("--no-git");
const run = promisify(execFile);

if (!version) {
  console.error("Usage: npm run version:set -- <version> [--dry-run]");
  process.exit(1);
}

if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)) {
  console.error(`Invalid semver version: ${version}`);
  process.exit(1);
}

const files = [
  {
    file: "package.json",
    update: (text) => replaceFirst(text, /"version":\s*"[^"]+"/, `"version": "${version}"`),
  },
  {
    file: "package-lock.json",
    update: (text) => {
      const first = replaceFirst(text, /"version":\s*"[^"]+"/, `"version": "${version}"`);
      const second = replaceFirst(
        first.next,
        /("":\s*\{\s*"name":\s*"clankerclocker",\s*"version":\s*")[^"]+(")/s,
        `$1${version}$2`
      );
      return {
        next: second.next,
        matched: first.matched || second.matched,
        changed: first.changed || second.changed,
      };
    },
  },
  {
    file: path.join("src-tauri", "Cargo.toml"),
    update: (text) => replaceFirst(text, /^version = ".*"$/m, `version = "${version}"`),
  },
  {
    file: path.join("src-tauri", "tauri.conf.json"),
    update: (text) => replaceFirst(text, /"version":\s*"[^"]+"/, `"version": "${version}"`),
  },
  {
    file: path.join("src-tauri", "Cargo.lock"),
    update: (text) =>
      replaceFirst(
        text,
        /(\[\[package\]\]\s+name = "clankerclocker"\s+version = ")([^"]+)(")/m,
        `$1${version}$3`
      ),
  },
];

const changes = [];

for (const entry of files) {
  const fullPath = path.join(root, entry.file);
  const current = await fs.readFile(fullPath, "utf8");
  const { next, matched, changed } = entry.update(current);
  if (!matched) {
    console.warn(`Skipped ${entry.file}: no matching version field found`);
    continue;
  }
  if (!changed) {
    console.log(`Already on ${version}: ${entry.file}`);
    continue;
  }
  changes.push(entry.file);
  if (!dryRun) {
    await fs.writeFile(fullPath, next);
  }
}

if (dryRun) {
  if (changes.length === 0) {
    console.log(`No files need changes for version ${version}.`);
  } else {
    console.log(`Would update version to ${version} in:`);
    for (const file of changes) {
      console.log(`- ${file}`);
    }
  }
} else {
  if (changes.length === 0) {
    console.log(`No files needed changes for version ${version}.`);
  } else {
    console.log(`Updated version to ${version} in:`);
    for (const file of changes) {
      console.log(`- ${file}`);
    }
  }
}

if (!dryRun && !noGit && changes.length > 0) {
  await run("git", ["add", ...changes], { cwd: root });
  const commitMessage = `Bump version to ${version}`;
  await run("git", ["commit", "-m", commitMessage], { cwd: root });
  await run("git", ["tag", `v${version}`], { cwd: root });
  console.log(`Created commit and tag v${version}.`);
}

function replaceFirst(text, pattern, replacement) {
  if (!pattern.test(text)) {
    return { next: text, matched: false, changed: false };
  }
  const next = text.replace(pattern, replacement);
  return { next, matched: true, changed: next !== text };
}
