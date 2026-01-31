import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";

function usage() {
  console.error("Usage: node scripts/logging/llm-wrapper.js --log-dir <dir> --label <label> -- <command> [args...]");
  process.exit(1);
}

function ensureDir(dirPath) {
  if (!fs.existsSync(dirPath)) {
    fs.mkdirSync(dirPath, { recursive: true });
  }
}

function nowStamp() {
  const d = new Date();
  const pad = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}`;
}

const args = process.argv.slice(2);
const sepIndex = args.indexOf("--");
if (sepIndex === -1) usage();

const options = args.slice(0, sepIndex);
const cmdArgs = args.slice(sepIndex + 1);
if (cmdArgs.length === 0) usage();

let logDir = path.join(process.cwd(), "logs", "llm");
let label = "llm";
for (let i = 0; i < options.length; i++) {
  if (options[i] === "--log-dir") {
    logDir = options[i + 1];
    i++;
  } else if (options[i] === "--label") {
    label = options[i + 1];
    i++;
  }
}

ensureDir(logDir);
const logPath = path.join(logDir, `${label}-${nowStamp()}-${process.pid}.log`);
const logStream = fs.createWriteStream(logPath, { flags: "a" });

function logLine(line) {
  logStream.write(line + "\n");
}

logLine(`# START ${new Date().toISOString()}`);
logLine(`# CMD ${cmdArgs.join(" ")}`);
logLine("# --- STDIN ---");

const child = spawn(cmdArgs[0], cmdArgs.slice(1), { stdio: ["pipe", "pipe", "pipe"] });

process.stdin.on("data", (chunk) => {
  logStream.write(chunk);
  child.stdin.write(chunk);
});

process.stdin.on("end", () => {
  child.stdin.end();
});

child.stdout.on("data", (chunk) => {
  process.stdout.write(chunk);
  logStream.write(chunk);
});

child.stderr.on("data", (chunk) => {
  process.stderr.write(chunk);
  logStream.write(chunk);
});

child.on("close", (code) => {
  logLine("\n# --- END ---");
  logLine(`# EXIT ${code}`);
  logStream.end();
  process.exit(code ?? 1);
});
