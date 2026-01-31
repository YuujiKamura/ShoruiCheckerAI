import test from "node:test";
import assert from "node:assert/strict";

import { parseIndividualResults } from "../../src/utils/analysis.js";

test("parseIndividualResults splits sections by filename", () => {
  const input = [
    "## 📄 first.pdf",
    "---",
    "Result A",
    "",
    "## 📄 second.pdf",
    "---",
    "Result B",
  ].join("\n");

  const result = parseIndividualResults(input);
  assert.equal(result["first.pdf"], "Result A");
  assert.equal(result["second.pdf"], "Result B");
});

test("parseIndividualResults ignores empty sections", () => {
  const result = parseIndividualResults("\n\n");
  assert.deepEqual(result, {});
});
