import fs from "node:fs";
import path from "node:path";

function ensureDir(dirPath) {
  if (!fs.existsSync(dirPath)) {
    fs.mkdirSync(dirPath, { recursive: true });
  }
}

function nowStamp() {
  const d = new Date();
  const pad = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

const args = process.argv.slice(2);
const logDir = path.join(process.cwd(), "logs", "chat");
ensureDir(logDir);
const logPath = path.join(logDir, `${new Date().toISOString().slice(0, 10)}.log`);

let content = "";
if (args.length > 0) {
  content = args.join(" ");
} else {
  content = fs.readFileSync(0, "utf8");
}

if (!content.trim()) {
  console.error("No content provided");
  process.exit(1);
}

const entry = [
  "---",
  `time: ${nowStamp()}`,
  content.trim(),
  ""
].join("\n");

fs.appendFileSync(logPath, entry, "utf8");
console.log(`appended: ${logPath}`);
