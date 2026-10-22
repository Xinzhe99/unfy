/**
 * Version agreement.
 *
 * Run with `bun test`.
 *
 * Three places carry the version and Tauri offers no way to derive one from another:
 * `Cargo.toml` (the workspace version, which `package_info()` reports and the window
 * displays), `tauri.conf.json` (which the bundler stamps into the installer), and the
 * updater manifest published next to a release.
 *
 * If the first two disagree, the window says 0.1.0 while the installer is called 0.2.0,
 * and a user reporting a bug quotes a number that matches nothing. This test is cheap and
 * the failure it prevents is expensive to diagnose after the fact.
 */
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

// `src/lib` -> `apps/desktop` -> `apps` -> repository root.
const ROOT = `${import.meta.dir}/../../../..`;

function versionFromCargo(): string {
  const text = readFileSync(`${ROOT}/Cargo.toml`, "utf8");
  // The workspace version lives under `[workspace.package]`.
  const section = text.split("[workspace.package]")[1] ?? "";
  const match = section.match(/^version\s*=\s*"([^"]+)"/m);
  if (!match) throw new Error("no workspace version in Cargo.toml");
  return match[1]!;
}

function versionFromTauri(): string {
  const config = JSON.parse(
    readFileSync(`${ROOT}/apps/desktop/src-tauri/tauri.conf.json`, "utf8"),
  ) as { version?: string };
  if (!config.version) throw new Error("no version in tauri.conf.json");
  return config.version;
}

describe("version", () => {
  test("the window and the installer report the same number", () => {
    expect(versionFromTauri()).toBe(versionFromCargo());
  });

  test("the version is a plain semver triple", () => {
    // The updater compares versions, and a suffix it does not understand makes every
    // update look newer or older than it is.
    expect(versionFromCargo()).toMatch(/^\d+\.\d+\.\d+$/);
  });
});
