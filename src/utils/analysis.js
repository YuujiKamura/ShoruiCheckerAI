export function parseIndividualResults(result) {
  const fileResults = {};
  const sections = result.split(/(?:^|\n)## 📄 /);

  for (const section of sections) {
    if (!section.trim()) continue;
    const lines = section.split("\n");
    const fileName = lines[0].trim();
    const content = lines
      .slice(1)
      .join("\n")
      .replace(/^---\n/, "")
      .trim();
    if (fileName) {
      fileResults[fileName] = content;
    }
  }

  return fileResults;
}
