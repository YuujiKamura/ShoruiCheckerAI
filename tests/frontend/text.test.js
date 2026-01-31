import test from "node:test";
import assert from "node:assert/strict";

import { escapeHtml, markdownToHtml } from "../../src/utils/text.js";

test("escapeHtml escapes critical characters", () => {
  const input = "<div>Me & You</div>";
  const output = escapeHtml(input);
  assert.equal(output, "&lt;div&gt;Me &amp; You&lt;/div&gt;");
});

test("escapeHtml handles nullish values", () => {
  assert.equal(escapeHtml(null), "");
  assert.equal(escapeHtml(undefined), "");
});

test("markdownToHtml converts headings and lists", () => {
  const md = "# Title\n\n- One\n- Two";
  const html = markdownToHtml(md);
  assert.ok(html.includes("<h1>Title</h1>"));
  assert.ok(html.includes("<ul>"));
  assert.ok(html.includes("<li>One</li>"));
});
