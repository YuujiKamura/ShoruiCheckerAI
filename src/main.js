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
let apiKeyEl;
let apiStatusEl;

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
  apiKeyEl = document.querySelector("#api-key");
  apiStatusEl = document.querySelector("#api-status");

  // Event listeners
  startBtnEl.addEventListener("click", startWatching);
  stopBtnEl.addEventListener("click", stopWatching);
  checkBtnEl.addEventListener("click", checkManually);
  document.querySelector("#save-key-btn").addEventListener("click", saveApiKey);

  // Check API key status
  await checkApiStatus();

  // Load history
  await loadHistory();
});

async function checkApiStatus() {
  try {
    const hasKey = await invoke("get_api_key_status");
    apiStatusEl.textContent = hasKey ? "🟢 API Key設定済み" : "🔴 API Key未設定";
  } catch (e) {
    apiStatusEl.textContent = "⚠️ 状態不明";
  }
}

async function saveApiKey() {
  const key = apiKeyEl.value.trim();
  if (!key) {
    alert("API Keyを入力してください");
    return;
  }

  try {
    await invoke("set_api_key", { key });
    apiKeyEl.value = "";
    await checkApiStatus();
    alert("API Keyを保存しました");
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
      // Note: In Tauri, we need to get the actual file path
      // For now, we'll use a workaround with the file name
      const result = await invoke("check_pdf_manually", {
        filePath: file.name // This will need proper file path handling
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
  // Remove placeholder if exists
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

  // Add click handler to show details
  if (result.details) {
    item.style.cursor = "pointer";
    item.addEventListener("click", () => {
      alert(result.details);
    });
  }

  // Insert at the top
  resultsListEl.insertBefore(item, resultsListEl.firstChild);
}

function escapeHtml(text) {
  const div = document.createElement("div");
  div.textContent = text;
  return div.innerHTML;
}
