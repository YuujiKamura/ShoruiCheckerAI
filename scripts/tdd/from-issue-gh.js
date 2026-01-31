import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

function getIssueBody(issueNumber) {
  const command = `gh issue view ${issueNumber} --json body -q .body`;
  return execSync(command, { encoding: "utf8" }).trim();
}

function run(issueNumber) {
  if (!issueNumber) {
    console.error("Usage: node scripts/tdd/from-issue-gh.js ISSUE_NUMBER");
    process.exit(1);
  }

  const body = getIssueBody(issueNumber);
  if (!body) {
    console.error("Issue body is empty or not accessible");
    process.exit(1);
  }

  const tmpPath = path.join(process.cwd(), ".tmp-issue.md");
  fs.writeFileSync(tmpPath, body, "utf8");

  try {
    execSync(`node scripts/tdd/from-issue.js ${tmpPath}`, { stdio: "inherit" });
  } finally {
    fs.unlinkSync(tmpPath);
  }
}

run(process.argv[2]);
