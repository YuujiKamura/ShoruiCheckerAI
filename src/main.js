const { invoke } = window.__TAURI__.core;

// DOM elements
let folderPathEl;
let startBtnEl;
let stopBtnEl;
let checkBtnEl;
let fileInputEl;
let watchStatusEl;
let lastCheckEl;
let resultsListEl;
let cliStatusEl;
let guidelinesDialog;
let guidelinesTextEl;

let isWatching = false;

// Initialize
window.addEventListener("DOMContentLoaded", async () => {
  // Get DOM elements
  folderPathEl = document.querySelector("#folder-path");
  startBtnEl = document.querySelector("#start-btn");
  stopBtnEl = document.querySelector("#stop-btn");
  checkBtnEl = document.querySelector("#check-btn");
  fileInputEl = document.querySelector("#file-input");
  watchStatusEl = document.querySelector("#watch-status");
  lastCheckEl = document.querySelector("#last-check");
  resultsListEl = document.querySelector("#results-list");
  cliStatusEl = document.querySelector("#cli-status");
  guidelinesDialog = document.querySelector("#guidelines-dialog");
  guidelinesTextEl = document.querySelector("#guidelines-text");

  // Event listeners
  startBtnEl.addEventListener("click", startWatching);
  stopBtnEl.addEventListener("click", stopWatching);
  checkBtnEl.addEventListener("click", checkManually);
  document.querySelector("#edit-guidelines-btn").addEventListener("click", openGuidelinesDialog);
  document.querySelector("#save-guidelines-btn").addEventListener("click", saveGuidelines);
  document.querySelector("#close-dialog-btn").addEventListener("click", () => guidelinesDialog.close());

  // Check Claude CLI status
  await checkCliStatus();

  // Load history
  await loadHistory();
});

async function checkCliStatus() {
  try {
    const hasCliude = await invoke("check_claude_cli");
    cliStatusEl.textContent = hasCliude ? "🟢 Claude CLI 利用可能" : "🔴 Claude CLI 未検出";
  } catch (e) {
    cliStatusEl.textContent = "⚠️ 状態不明";
  }
}

async function openGuidelinesDialog() {
  try {
    const guidelines = await invoke("get_guidelines");
    guidelinesTextEl.value = guidelines || "";
  } catch (e) {
    guidelinesTextEl.value = "";
  }
  guidelinesDialog.showModal();
}

async function saveGuidelines() {
  const content = guidelinesTextEl.value;
  try {
    const path = await invoke("save_guidelines", { content });
    alert("ガイドラインを保存しました: " + path);
    guidelinesDialog.close();
  } catch (e) {
    alert("エラー: " + e);
  }
}

async function startWatching() {
  const folderPath = folderPathEl.value.trim();
  if (!folderPath) {
    alert("監視フォルダを指定してください");
    return;
  }

  try {
    const result = await invoke("start_watching", { folderPath });
    console.log(result);
    isWatching = true;
    updateWatchUI();
    watchStatusEl.textContent = "👁️ 監視中: " + folderPath;
  } catch (e) {
    alert("監視開始エラー: " + e);
  }
}

async function stopWatching() {
  try {
    await invoke("stop_watching");
    isWatching = false;
    updateWatchUI();
    watchStatusEl.textContent = "⏸️ 待機中";
  } catch (e) {
    alert("監視停止エラー: " + e);
  }
}

function updateWatchUI() {
  startBtnEl.disabled = isWatching;
  stopBtnEl.disabled = !isWatching;
  folderPathEl.disabled = isWatching;
}

async function checkManually() {
  const files = fileInputEl.files;
  if (!files || files.length === 0) {
    alert("PDFファイルを選択してください");
    return;
  }

  checkBtnEl.disabled = true;
  checkBtnEl.textContent = "チェック中...";

  try {
    for (const file of files) {
      const result = await invoke("check_pdf_manually", {
        filePath: file.name
      });

      addResultToList(result);
      lastCheckEl.textContent = "最終チェック: " + result.checked_at;
    }
  } catch (e) {
    alert("チェックエラー: " + e);
  } finally {
    checkBtnEl.disabled = false;
    checkBtnEl.textContent = "チェック実行";
  }
}

async function loadHistory() {
  try {
    const results = await invoke("get_check_history", { limit: 20 });
    resultsListEl.innerHTML = "";

    if (results.length === 0) {
      resultsListEl.innerHTML = '<p class="placeholder">チェック結果がここに表示されます</p>';
      return;
    }

    for (const result of results) {
      addResultToList(result);
    }
  } catch (e) {
    console.error("履歴読み込みエラー:", e);
  }
}

function addResultToList(result) {
  const placeholder = resultsListEl.querySelector(".placeholder");
  if (placeholder) {
    placeholder.remove();
  }

  const item = document.createElement("div");
  item.className = `result-item ${result.status}`;
  item.innerHTML = `
    <div class="file-name">${escapeHtml(result.file_name)}</div>
    <div class="message">${escapeHtml(result.message)}</div>
    <div class="time">${escapeHtml(result.checked_at)}</div>
  `;

  if (result.details) {
    item.style.cursor = "pointer";
    item.addEventListener("click", () => {
      alert(result.details);
    });
  }

  resultsListEl.insertBefore(item, resultsListEl.firstChild);
}

function escapeHtml(text) {
  const div = document.createElement("div");
  div.textContent = text;
  return div.innerHTML;
}
