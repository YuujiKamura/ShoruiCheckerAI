export function replaceBrWithNewline(text) {
  return String(text || "").replace(/<br\s*\/?>/gi, "\n");
}

export function createPlainTextCopy(content) {
  return replaceBrWithNewline(content)
    .replace(/<[^>]+>/g, "")
    .trim();
}
