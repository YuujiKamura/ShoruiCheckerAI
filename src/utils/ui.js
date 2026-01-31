export function updateButtonsState({
  hasFiles,
  hasChecked,
  hasResultsSelected,
  busy,
  hasCustomInstruction,
}) {
  return {
    analyzeDisabled: busy || !hasChecked,
    compareDisabled: busy || !hasChecked || hasResultsSelected,
    clearDisabled: busy || !hasFiles,
    selectAllDisabled: !hasFiles,
    selectNoneDisabled: !hasFiles,
    guidelinesDisabled: busy || !hasResultsSelected,
    customInstructionDisabled: busy || !hasChecked,
    copyInstructionDisabled: busy || !hasResultsSelected || !hasCustomInstruction,
  };
}
