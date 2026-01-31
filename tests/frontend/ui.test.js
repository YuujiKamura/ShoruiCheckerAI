import test from "node:test";
import assert from "node:assert/strict";

import { updateButtonsState } from "../../src/utils/ui.js";

test("updateButtonsState disables actions when busy", () => {
  const state = updateButtonsState({
    hasFiles: true,
    hasChecked: true,
    hasResultsSelected: true,
    busy: true,
    hasCustomInstruction: true,
  });

  assert.equal(state.analyzeDisabled, true);
  assert.equal(state.compareDisabled, true);
  assert.equal(state.clearDisabled, true);
  assert.equal(state.guidelinesDisabled, true);
});

test("updateButtonsState enables actions based on inputs", () => {
  const state = updateButtonsState({
    hasFiles: true,
    hasChecked: true,
    hasResultsSelected: true,
    busy: false,
    hasCustomInstruction: false,
  });

  assert.equal(state.analyzeDisabled, false);
  assert.equal(state.compareDisabled, true);
  assert.equal(state.guidelinesDisabled, false);
  assert.equal(state.copyInstructionDisabled, true);
});
