import fs from "node:fs";
import path from "node:path";

function readIssue(filePath) {
  return fs.readFileSync(filePath, "utf8");
}

function parseSections(markdown) {
  const lines = markdown.split(/\r?\n/);
  const sections = {};
  let current = null;
  for (const line of lines) {
    const heading = line.match(/^###\s+(.+)$/);
    if (heading) {
      current = heading[1].trim();
      sections[current] = [];
      continue;
    }
    if (current) {
      sections[current].push(line);
    }
  }
  return sections;
}

function extractTestItems(sectionLines) {
  const items = [];
  for (const line of sectionLines) {
    const item = line.match(/^\s*-\s+(.+)$/);
    if (!item) continue;
    items.push(item[1].trim());
  }
  return items;
}

function classifyTestTarget(entry) {
  const [filePart, description] = entry.split(":").map((part) => part.trim());
  if (!filePart) return null;
  return { filePath: filePart, description: description || "TODO" };
}

function ensureDir(dirPath) {
  if (!fs.existsSync(dirPath)) {
    fs.mkdirSync(dirPath, { recursive: true });
  }
}

function appendIfNotExists(filePath, content) {
  if (fs.existsSync(filePath)) {
    return { created: false, path: filePath };
  }
  fs.writeFileSync(filePath, content, "utf8");
  return { created: true, path: filePath };
}

function generateFrontendTest(filePath, description) {
  const name = path.basename(filePath, path.extname(filePath));
  const content = `import test from "node:test";\nimport assert from "node:assert/strict";\n\n// TODO: ${description}\n\ntest("${name} - TODO", () => {\n  assert.ok(true);\n});\n`;
  return appendIfNotExists(filePath, content);
}

function generateRustTest(filePath, description) {
  const moduleName = path.basename(filePath, path.extname(filePath));
  const content = `// TODO: ${description}\n\n#[test]\nfn ${moduleName}_todo() {\n    assert!(true);\n}\n`;
  return appendIfNotExists(filePath, content);
}

function run(issuePath) {
  if (!issuePath) {
    console.error("Usage: node scripts/tdd/from-issue.js path/to/issue.md");
    process.exit(1);
  }

  const absoluteIssuePath = path.resolve(issuePath);
  const markdown = readIssue(absoluteIssuePath);
  const sections = parseSections(markdown);
  const testSection = sections["先に書くテスト"] || sections["Tests"] || [];
  const items = extractTestItems(testSection);

  if (items.length === 0) {
    console.error("No test items found in Issue body (section: 先に書くテスト)");
    process.exit(1);
  }

  const results = [];
  for (const item of items) {
    const parsed = classifyTestTarget(item);
    if (!parsed) continue;
    const filePath = parsed.filePath;
    const description = parsed.description;
    const dir = path.dirname(filePath);
    ensureDir(dir);

    if (filePath.startsWith("tests/frontend/")) {
      results.push(generateFrontendTest(filePath, description));
    } else if (filePath.startsWith("src-tauri/tests/")) {
      results.push(generateRustTest(filePath, description));
    } else {
      // Default to frontend tests if located under tests/
      results.push(generateFrontendTest(filePath, description));
    }
  }

  for (const result of results) {
    const status = result.created ? "created" : "skipped";
    console.log(`${status}: ${result.path}`);
  }
}

run(process.argv[2]);
