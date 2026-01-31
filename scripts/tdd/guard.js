import { execSync, spawnSync } from "node:child_process";
import path from "node:path";

function getRepoRoot() {
  return execSync("git rev-parse --show-toplevel", { encoding: "utf8" }).trim();
}

function getStagedFiles() {
  const output = execSync("git diff --cached --name-only", { encoding: "utf8" });
  return output.split("\n").map((line) => line.trim()).filter(Boolean);
}

function getStagedDiff() {
  return execSync("git diff --cached -U0", { encoding: "utf8" });
}

function hasFrontendChanges(files) {
  return files.some((file) => file.startsWith("src/"));
}

function hasRustChanges(files) {
  return files.some((file) => file.startsWith("src-tauri/src/") || file === "src-tauri/Cargo.toml" || file === "src-tauri/Cargo.lock");
}

function hasFrontendTestChanges(files) {
  return files.some((file) => file.startsWith("tests/frontend/"));
}

function hasRustTestChanges(files, diff) {
  if (files.some((file) => file.startsWith("src-tauri/tests/"))) return true;
  return diff.includes("#[test]") || diff.includes("#[cfg(test)]") || diff.includes("mod tests");
}

function runCommand(command, args, options = {}) {
  const result = spawnSync(command, args, { stdio: "inherit", ...options });
  if (result.status !== 0) {
    process.exit(result.status || 1);
  }
}

const repoRoot = getRepoRoot();
const stagedFiles = getStagedFiles();
if (stagedFiles.length === 0) {
  process.exit(0);
}

const stagedDiff = getStagedDiff();
const frontendChanged = hasFrontendChanges(stagedFiles);
const rustChanged = hasRustChanges(stagedFiles);

if (frontendChanged && !hasFrontendTestChanges(stagedFiles)) {
  console.error("TDD guard: frontend changes require tests in tests/frontend/*");
  process.exit(1);
}

if (rustChanged && !hasRustTestChanges(stagedFiles, stagedDiff)) {
  console.error("TDD guard: Rust changes require tests (# [test] additions or src-tauri/tests)");
  process.exit(1);
}

if (frontendChanged) {
  runCommand("npm", ["run", "test:frontend"], { cwd: repoRoot });
}

if (rustChanged) {
  runCommand("cargo", ["test"], { cwd: path.join(repoRoot, "src-tauri") });
}
