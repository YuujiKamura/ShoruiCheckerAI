export function escapeHtml(text) {
  const value = text ?? "";
  return String(value)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

export function markdownToHtml(md) {
  if (!md) return "";

  return md
    .replace(/^### (.+)$/gm, "<h3>$1</h3>")
    .replace(/^## (.+)$/gm, "<h2>$1</h2>")
    .replace(/^# (.+)$/gm, "<h1>$1</h1>")
    .replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
    .replace(/\|(.+)\|/g, (match) => {
      const cells = match.split("|").filter((c) => c.trim());
      if (cells.every((c) => /^[-:]+$/.test(c.trim()))) return "";
      const tag = cells.some((c) => c.includes("---")) ? "th" : "td";
      return (
        "<tr>" +
        cells.map((c) => `<${tag}>${c.trim()}</${tag}>`).join("") +
        "</tr>"
      );
    })
    .replace(/(<tr>.*<\/tr>\n?)+/g, "<table>$&</table>")
    .replace(/^- (.+)$/gm, "<li>$1</li>")
    .replace(/(<li>.*<\/li>\n?)+/g, "<ul>$&</ul>")
    .replace(/\n\n/g, "</p><p>")
    .replace(/\n/g, "<br>");
}
