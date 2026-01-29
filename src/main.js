const { invoke } = window.__TAURI__.core;

// State
let isWatching = false;
let pollInterval = null;

// Initialize
window.addEventListener("DOMContentLoaded", async () => {
  // Event listeners
  document.querySelector("#start-btn").addEventListener("click", startWatching);
  document.querySelector("#stop-btn").addEventListener("click", stopWatching);
  document.querySelector("#clear-pending-btn").addEventListener("click", clearPending);

  document.querySelector("#edit-policy-btn").addEventListener("click", openPolicyDialog);
  document.querySelector("#save-policy-btn").addEventListener("click", savePolicy);
  document.querySelector("#close-policy-btn").addEventListener("click", () =>
    document.querySelector("#policy-dialog").close());

  document.querySelector("#edit-guidelines-btn").addEventListener("click", openGuidelinesDialog);
  document.querySelector("#save-guidelines-btn").addEventListener("click", saveGuidelines);
  document.querySelector("#close-guidelines-btn").addEventListener("click", () =>
    document.querySelector("#guidelines-dialog").close());

  // Initial load
  await checkCliStatus();
  await loadPendingFiles();
  await loadHistory();
});

// ========== CLI Status ==========

async function checkCliStatus() {
  try {
    const hasCli = await invoke("check_claude_cli");
    document.querySelector("#cli-status").textContent =
      hasCli ? "🟢 Claude CLI" : "🔴 Claude CLI未検出";
  } catch (e) {
    document.querySelector("#cli-status").textContent = "⚠️";
  }
}

// ========== Folder Watching ==========

async function startWatching() {
  const folderPath = document.querySelector("#folder-path").value.trim();
  if (!folderPath) {
    alert("監視フォルダを指定してください");
    return;
  }

  try {
    await invoke("start_watching", { folderPath });
    isWatching = true;
    updateWatchUI();

    // Start polling for new files
    pollInterval = setInterval(pollNewFiles, 2000);
  } catch (e) {
    alert("エラー: " + e);
  }
}

async function stopWatching() {
  try {
    await invoke("stop_watching");
    isWatching = false;
    updateWatchUI();

    if (pollInterval) {
      clearInterval(pollInterval);
      pollInterval = null;
    }
  } catch (e) {
    alert("エラー: " + e);
  }
}

function updateWatchUI() {
  document.querySelector("#start-btn").disabled = isWatching;
  document.querySelector("#stop-btn").disabled = !isWatching;
  document.querySelector("#folder-path").disabled = isWatching;
  document.querySelector("#watch-status").textContent =
    isWatching ? "👁️ 監視中" : "⏸️ 待機中";
}

// ========== Pending Files ==========

async function pollNewFiles() {
  try {
    const newFiles = await invoke("poll_new_files");
    if (newFiles.length > 0) {
      await loadPendingFiles();
      // Notify user
      newFiles.forEach(f => {
        showNotification(`新しいPDF: ${f.name}`);
      });
    }
  } catch (e) {
    console.error("Poll error:", e);
  }
}

async function loadPendingFiles() {
  try {
    const files = await invoke("get_pending_files");
    const list = document.querySelector("#pending-list");
    const count = document.querySelector("#pending-count");

    count.textContent = `(${files.length})`;

    if (files.length === 0) {
      list.innerHTML = '<p class="placeholder">新しいPDFがここに表示されます</p>';
      return;
    }

    list.innerHTML = files.map(f => `
      <div class="pending-item" data-path="${escapeAttr(f.path)}">
        <div class="file-info">
          <div class="file-name">${escapeHtml(f.name)}</div>
          <div class="file-meta">${formatSize(f.size_bytes)} - ${f.detected_at}</div>
        </div>
        <div class="file-actions">
          <button class="analyze-btn" onclick="analyzeFile('${escapeAttr(f.path)}')">解析</button>
          <button class="skip-btn" onclick="skipFile('${escapeAttr(f.path)}')">スキップ</button>
        </div>
      </div>
    `).join('');
  } catch (e) {
    console.error("Load pending error:", e);
  }
}

async function analyzeFile(path) {
  const btn = document.querySelector(`.pending-item[data-path="${path}"] .analyze-btn`);
  if (btn) {
    btn.disabled = true;
    btn.textContent = "解析中...";
  }

  try {
    const result = await invoke("analyze_file", { filePath: path });
    await loadPendingFiles();
    await loadHistory();
    showNotification(`解析完了: ${result.file_name}`);
  } catch (e) {
    alert("解析エラー: " + e);
    if (btn) {
      btn.disabled = false;
      btn.textContent = "解析";
    }
  }
}

async function skipFile(path) {
  try {
    await invoke("remove_pending_file", { path });
    await loadPendingFiles();
  } catch (e) {
    console.error("Skip error:", e);
  }
}

async function clearPending() {
  if (!confirm("検出ファイルをすべてクリアしますか？")) return;

  try {
    await invoke("clear_pending_files");
    await loadPendingFiles();
  } catch (e) {
    alert("エラー: " + e);
  }
}

// ========== History ==========

async function loadHistory() {
  try {
    const results = await invoke("get_check_history", { limit: 20 });
    const list = document.querySelector("#results-list");

    if (results.length === 0) {
      list.innerHTML = '<p class="placeholder">チェック結果がここに表示されます</p>';
      return;
    }

    list.innerHTML = results.map(r => `
      <div class="result-item ${r.status}" onclick="showDetails('${escapeAttr(r.details || '')}')">
        <div class="file-name">${escapeHtml(r.file_name)}</div>
        <div class="message">${escapeHtml(r.message)}</div>
        <div class="time">${r.checked_at}</div>
      </div>
    `).join('');
  } catch (e) {
    console.error("Load history error:", e);
  }
}

function showDetails(details) {
  if (details) {
    alert(details);
  }
}

// ========== Policy ==========

async function openPolicyDialog() {
  try {
    const policy = await invoke("get_policy");
    document.querySelector("#policy-auto").checked = policy.auto_analyze;
    document.querySelector("#policy-include").value = policy.include_patterns.join(", ");
    document.querySelector("#policy-exclude").value = policy.exclude_patterns.join(", ");
    document.querySelector("#policy-min-size").value = Math.floor(policy.min_size_bytes / 1024);
    document.querySelector("#policy-max-size").value = Math.floor(policy.max_size_bytes / 1024);
  } catch (e) {
    console.error("Load policy error:", e);
  }
  document.querySelector("#policy-dialog").showModal();
}

async function savePolicy() {
  const policy = {
    auto_analyze: document.querySelector("#policy-auto").checked,
    include_patterns: parsePatterns(document.querySelector("#policy-include").value),
    exclude_patterns: parsePatterns(document.querySelector("#policy-exclude").value),
    min_size_bytes: (parseInt(document.querySelector("#policy-min-size").value) || 1) * 1024,
    max_size_bytes: (parseInt(document.querySelector("#policy-max-size").value) || 50000) * 1024,
  };

  try {
    await invoke("save_policy", { policy });
    document.querySelector("#policy-dialog").close();
    showNotification("ポリシーを保存しました");
  } catch (e) {
    alert("エラー: " + e);
  }
}

function parsePatterns(str) {
  return str.split(",").map(s => s.trim()).filter(s => s.length > 0);
}

// ========== Guidelines ==========

async function openGuidelinesDialog() {
  try {
    const guidelines = await invoke("get_guidelines");
    document.querySelector("#guidelines-text").value = guidelines || "";
  } catch (e) {
    document.querySelector("#guidelines-text").value = "";
  }
  document.querySelector("#guidelines-dialog").showModal();
}

async function saveGuidelines() {
  const content = document.querySelector("#guidelines-text").value;
  try {
    await invoke("save_guidelines", { content });
    document.querySelector("#guidelines-dialog").close();
    showNotification("ガイドラインを保存しました");
  } catch (e) {
    alert("エラー: " + e);
  }
}

// ========== Utilities ==========

function escapeHtml(text) {
  const div = document.createElement("div");
  div.textContent = text || "";
  return div.innerHTML;
}

function escapeAttr(text) {
  return (text || "").replace(/'/g, "\\'").replace(/"/g, '\\"');
}

function formatSize(bytes) {
  if (bytes < 1024) return bytes + " B";
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + " KB";
  return (bytes / (1024 * 1024)).toFixed(1) + " MB";
}

function showNotification(message) {
  // Simple notification - could be enhanced with Tauri notifications
  console.log("Notification:", message);
}

// Make functions available globally for onclick handlers
window.analyzeFile = analyzeFile;
window.skipFile = skipFile;
window.showDetails = showDetails;
