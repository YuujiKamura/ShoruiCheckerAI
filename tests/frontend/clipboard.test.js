import test from "node:test";
import assert from "node:assert/strict";

import { createPlainTextCopy, replaceBrWithNewline } from "../../src/utils/clipboard.js";

test("replaceBrWithNewline handles multiple br tags", () => {
  const input = "Line1<br>Line2<BR/>Line3<br />Line4";
  const output = replaceBrWithNewline(input);
  assert.equal(output, "Line1\nLine2\nLine3\nLine4");
});

test("createPlainTextCopy strips tags and trims", () => {
  const input = "<h2>Title</h2><br>Body <strong>bold</strong>";
  const output = createPlainTextCopy(input);
  assert.equal(output, "Title\nBody bold");
});
