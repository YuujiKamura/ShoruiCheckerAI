import { parseIndividualResults } from "./utils/analysis.js";
import { createPlainTextCopy } from "./utils/clipboard.js";
import { escapeHtml, markdownToHtml } from "./utils/text.js";
import { updateButtonsState } from "./utils/ui.js";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { open } = window.__TAURI__.dialog;
const { isPermissionGranted, requestPermission, sendNotification } = window.__TAURI__.notification;

// State
let pdfFiles = [];
let logUnlisten = null;

// ============================================
// 初期化 - 責務ごとに分割
// ============================================

window.addEventListener("DOMContentLoaded", async () => {
  await initSettings();
  await initEventListeners();
  await initTauriListeners();
  await initStartupFile();
});

async function initSettings() {
  try {
    const [watchFolder, currentModel, history, codeWatchFolder, codeReviewEnabled] = await Promise.all([
      invoke("get_watch_folder"),
      invoke("get_model"),
      invoke("get_all_history"),
      invoke("get_code_watch_folder"),
      invoke("is_code_review_enabled")
    ]);

    // PDF監視設定
    if (watchFolder) {
      document.getElementById("watch-folder").value = watchFolder;
      document.getElementById("watch-status").textContent = "監視中: " + watchFolder;
    }
    document.getElementById("model-select").value = currentModel;

    // コードレビュー設定
    if (codeWatchFolder) {
      document.getElementById("code-watch-folder").value = codeWatchFolder;
      if (codeReviewEnabled) {
        document.getElementById("code-watch-status").textContent = "コード監視中: " + codeWatchFolder;
      }
    }
    document.getElementById("code-review-enabled").checked = codeReviewEnabled;

    // 履歴読み込み
    if (history && history.length > 0) {
      loadHistoryToFileList(history);
    }
  } catch (e) {
    console.error("Settings load error:", e);
  }

  // 認証状態確認（遅延実行）
  setTimeout(checkAuthStatus, 2000);
}

function loadHistoryToFileList(history) {
  for (const entry of history) {
    if (pdfFiles.find(f => f.path === entry.file_path)) continue;
    pdfFiles.push({
      name: entry.file_name,
      path: entry.file_path,
      checked: false,
      result: entry.summary,
      resultError: false,
      analyzedAt: entry.analyzed_at,
      documentType: entry.document_type
    });
  }
  updateList();
}

function initEventListeners() {
  // モデル選択
  document.getElementById("model-select").addEventListener("change", async (e) => {
    await invoke("set_model", { model: e.target.value });
  });

  // PDF監視フォルダ選択
  document.getElementById("select-folder-btn").addEventListener("click", () => selectWatchFolder("pdf"));

  // コード監視フォルダ選択
  document.getElementById("select-code-folder-btn").addEventListener("click", () => selectWatchFolder("code"));

  // コードレビュー有効/無効
  document.getElementById("code-review-enabled").addEventListener("change", toggleCodeReview);

  // 認証ボタン
  document.getElementById("auth-btn").addEventListener("click", async () => {
    await invoke("open_gemini_auth");
    setTimeout(checkAuthStatus, 3000);
  });

  // 設定モーダル
  initSettingsModal();

  // ファイル操作ボタン
  initFileButtons();

  // トースト閉じるボタン
  document.getElementById("toast-close").addEventListener("click", () => hideNotificationToast("notification-toast"));
  document.getElementById("code-review-close").addEventListener("click", () => hideNotificationToast("code-review-toast"));

  // ドロップゾーン
  document.getElementById("drop-zone").addEventListener("click", openFileDialog);
}

async function selectWatchFolder(type) {
  const isCode = type === "code";
  const inputId = isCode ? "code-watch-folder" : "watch-folder";
  const statusId = isCode ? "code-watch-status" : "watch-status";
  const command = isCode ? "set_code_watch_folder" : "set_watch_folder";

  try {
    const folder = await open({ directory: true });
    if (!folder) return;

    document.getElementById(inputId).value = folder;
    await invoke(command, { folder });

    if (isCode) {
      const enabled = document.getElementById("code-review-enabled").checked;
      document.getElementById(statusId).textContent = enabled
        ? "コード監視中: " + folder
        : "フォルダ設定済み（監視停止中）";
    } else {
      document.getElementById(statusId).textContent = "監視開始: " + folder;
    }
    document.getElementById(statusId).classList.remove("error");
  } catch (e) {
    document.getElementById(statusId).textContent = "エラー: " + e;
    document.getElementById(statusId).classList.add("error");
  }
}

async function toggleCodeReview(e) {
  const enabled = e.target.checked;
  const statusEl = document.getElementById("code-watch-status");

  try {
    await invoke("set_code_review_enabled", { enabled });
    const folder = document.getElementById("code-watch-folder").value;

    if (enabled && folder) {
      statusEl.textContent = "コード監視中: " + folder;
    } else if (enabled) {
      statusEl.textContent = "フォルダを選択してください";
    } else {
      statusEl.textContent = "コード監視停止";
    }
    statusEl.classList.remove("error");
  } catch (e) {
    statusEl.textContent = "エラー: " + e;
    statusEl.classList.add("error");
  }
}

function initSettingsModal() {
  const modal = document.getElementById("settings-modal");
  document.getElementById("settings-btn").addEventListener("click", () => modal.hidden = false);
  document.getElementById("close-settings").addEventListener("click", () => modal.hidden = true);
  modal.addEventListener("click", (e) => {
    if (e.target === modal) modal.hidden = true;
  });
}

function initFileButtons() {
  document.getElementById("analyze-btn").addEventListener("click", () => analyze("individual"));
  document.getElementById("compare-btn").addEventListener("click", () => analyze("compare"));
  document.getElementById("clear-btn").addEventListener("click", clearFiles);
  document.getElementById("select-all-btn").addEventListener("click", selectAll);
  document.getElementById("select-none-btn").addEventListener("click", selectNone);
  document.getElementById("guidelines-btn").addEventListener("click", generateGuidelines);
  document.getElementById("custom-instruction").addEventListener("input", updateButtons);
  document.getElementById("copy-instruction-btn").addEventListener("click", copyCustomInstruction);
}

async function initTauriListeners() {
  const dropZone = document.getElementById("drop-zone");

  // PDF検出
  await listen("pdf-detected", (event) => {
    const { path, name } = event.payload;
    if (!pdfFiles.find(f => f.path === path)) {
      pdfFiles.push({ name, path, checked: true });
      updateList();
      showNotificationToast("notification-toast", { icon: "📄", title: "PDF追加", body: name });
    }
  });

  // システム通知
  await listen("show-notification", async (event) => {
    const { title, body } = event.payload;
    let permissionGranted = await isPermissionGranted();
    if (!permissionGranted) {
      const permission = await requestPermission();
      permissionGranted = permission === 'granted';
    }
    if (permissionGranted) {
      sendNotification({ title, body });
    }
  });

  // コードレビュー完了
  await listen("code-review-complete", (event) => {
    const { name, review_result, has_issues } = event.payload;
    const shortResult = review_result.length > 100 ? review_result.substring(0, 100) + "..." : review_result;
    showNotificationToast("code-review-toast", {
      icon: has_issues ? "⚠️" : "✅",
      title: "コードレビュー",
      body: `<strong>${escapeHtml(name)}</strong><br>${escapeHtml(shortResult)}`,
      isHtml: true,
      hasIssues: has_issues,
      duration: has_issues ? 10000 : 5000
    });
  });

  // ドラッグ&ドロップ
  await listen("tauri://drag-drop", async (event) => {
    const paths = event.payload.paths || [];
    for (const path of paths) {
      if (path.toLowerCase().endsWith(".pdf")) {
        const name = path.split(/[\\/]/).pop();
        if (!pdfFiles.find(f => f.path === path)) {
          const file = { name, path, checked: true };
          await loadEmbeddedResult(file);
          pdfFiles.push(file);
        }
      }
    }
    updateList();
  });

  await listen("tauri://drag-enter", () => dropZone.classList.add("dragover"));
  await listen("tauri://drag-leave", () => dropZone.classList.remove("dragover"));
}

async function initStartupFile() {
  const startupFile = await invoke("get_startup_file");
  if (startupFile) {
    const name = startupFile.split(/[\\/]/).pop();
    pdfFiles.push({ name, path: startupFile, checked: true });
    updateList();
    setTimeout(() => analyze("individual"), 500);
  }
}

// ============================================
// 通知トースト - 統合された汎用関数
// ============================================

function showNotificationToast(toastId, options) {
  const toast = document.getElementById(toastId);
  const body = toast.querySelector(".toast-body");
  const icon = toast.querySelector(".toast-icon");

  if (icon && options.icon) icon.textContent = options.icon;
  if (body) {
    if (options.isHtml) {
      body.innerHTML = options.body;
    } else {
      body.textContent = options.body;
    }
  }

  if (options.hasIssues !== undefined) {
    toast.classList.toggle("has-issues", options.hasIssues);
  }

  toast.hidden = false;

  const duration = options.duration || 5000;
  setTimeout(() => hideNotificationToast(toastId), duration);
}

function hideNotificationToast(toastId) {
  document.getElementById(toastId).hidden = true;
}

// ============================================
// 認証
// ============================================

async function checkAuthStatus() {
  const statusEl = document.getElementById("auth-status");
  statusEl.textContent = "確認中...";
  statusEl.className = "auth-status";

  try {
    const isAuth = await invoke("check_gemini_auth");
    statusEl.textContent = isAuth ? "✓ 認証済み" : "✗ 未認証";
    statusEl.className = "auth-status " + (isAuth ? "ok" : "ng");
  } catch (e) {
    statusEl.textContent = "✗ エラー";
    statusEl.className = "auth-status ng";
  }
}

// ============================================
// ファイルリスト操作
// ============================================

function updateList() {
  const list = document.getElementById("pdf-list");
  const count = document.getElementById("file-count");

  count.textContent = `(${pdfFiles.length})`;

  list.innerHTML = pdfFiles.map((f, i) => {
    const hasResult = f.result !== undefined;
    const statusIcon = hasResult ? (f.resultError ? '⚠' : '✓') : '';
    const statusClass = hasResult ? (f.resultError ? 'has-error' : 'has-result') : '';
    const dateInfo = f.analyzedAt ? `<span class="analyzed-date">${f.analyzedAt}</span>` : '';
    const typeInfo = f.documentType ? `<span class="doc-type">[${f.documentType}]</span>` : '';
    const embeddedIcon = f.embedded ? '<span class="embedded-icon" title="PDF内に結果埋め込み済み">📎</span>' : '';

    return `
    <li class="${statusClass}">
      <input type="checkbox" class="file-check" data-index="${i}" ${f.checked ? 'checked' : ''} onchange="toggleFile(${i})">
      <div class="file-info" onclick="showResult(${i})" style="cursor: ${hasResult ? 'pointer' : 'default'}">
        <div class="filename">
          ${statusIcon ? `<span class="status-icon">${statusIcon}</span>` : ''}
          ${embeddedIcon}
          ${escapeHtml(f.name)}
          ${typeInfo}
        </div>
        <div class="path">${escapeHtml(f.path)} ${dateInfo}</div>
      </div>
      <button class="remove" onclick="removeFile(${i})">✕</button>
    </li>
  `}).join("");

  updateButtons();
}

function showResult(index) {
  const file = pdfFiles[index];
  if (file.result === undefined) return;

  const resultSection = document.getElementById("result-section");
  const resultContent = document.getElementById("result-content");

  resultContent.innerHTML = `<h3>📄 ${escapeHtml(file.name)}</h3><hr>` +
    (file.resultError
      ? `<p style="color: #ff4757;">⚠ ${escapeHtml(file.result)}</p>`
      : markdownToHtml(file.result));
  resultSection.hidden = false;
}

function toggleFile(index) {
  pdfFiles[index].checked = !pdfFiles[index].checked;
  updateButtons();
}

function updateButtons() {
  const checkedCount = pdfFiles.filter(f => f.checked).length;
  const checkedWithResults = pdfFiles.filter(f => f.checked && f.result && !f.resultError).length;
  const busy = pdfFiles.some(f => f.analyzing);
  const customInstruction = document.getElementById("custom-instruction").value.trim();

  const state = updateButtonsState({
    hasFiles: pdfFiles.length > 0,
    hasChecked: checkedCount > 0,
    hasResultsSelected: checkedWithResults > 0,
    busy,
    hasCustomInstruction: customInstruction.length > 0,
  });

  document.getElementById("analyze-btn").disabled = state.analyzeDisabled;
  document.getElementById("compare-btn").disabled = state.compareDisabled;
  document.getElementById("clear-btn").disabled = state.clearDisabled;
  document.getElementById("select-all-btn").disabled = state.selectAllDisabled;
  document.getElementById("select-none-btn").disabled = state.selectNoneDisabled;
  document.getElementById("guidelines-btn").disabled = state.guidelinesDisabled;
  document.getElementById("custom-instruction").disabled = state.customInstructionDisabled;
  document.getElementById("copy-instruction-btn").disabled = state.copyInstructionDisabled;
}

function getCheckedFiles() {
  return pdfFiles.filter(f => f.checked);
}

function selectAll() {
  pdfFiles.forEach(f => f.checked = true);
  updateList();
}

function selectNone() {
  pdfFiles.forEach(f => f.checked = false);
  updateList();
}

function removeFile(index) {
  pdfFiles.splice(index, 1);
  updateList();
}

function clearFiles() {
  pdfFiles = [];
  updateList();
  document.getElementById("result-section").hidden = true;
  document.getElementById("terminal-section").hidden = true;
}

async function copyCustomInstruction() {
  const resultContent = document.getElementById("result-content");
  const customInput = document.getElementById("custom-instruction");
  const existing = customInput.value.trim();
  if (!resultContent || !resultContent.innerHTML) return;

  const plain = createPlainTextCopy(resultContent.innerHTML);
  const nextValue = existing ? `${existing}\n${plain}` : plain;
  try {
    await navigator.clipboard.writeText(plain);
  } catch (e) {
    console.warn("Clipboard copy failed:", e);
  }
  customInput.value = nextValue;
  updateButtons();
}

async function openFileDialog() {
  try {
    const selected = await open({
      multiple: true,
      filters: [{ name: "PDF", extensions: ["pdf"] }]
    });
    if (selected) {
      const paths = Array.isArray(selected) ? selected : [selected];
      for (const path of paths) {
        const name = path.split(/[\\/]/).pop();
        if (!pdfFiles.find(f => f.path === path)) {
          const file = { name, path, checked: true };
          await loadEmbeddedResult(file);
          pdfFiles.push(file);
        }
      }
      updateList();
    }
  } catch (e) {
    console.error("File open error:", e);
  }
}

async function loadEmbeddedResult(file) {
  try {
    const embedded = await invoke("read_pdf_result", { path: file.path });
    if (embedded) {
      const [result, date] = embedded;
      file.result = result;
      file.resultError = false;
      file.analyzedAt = date;
      file.embedded = true;
    }
  } catch (e) {
    // Ignore - PDF doesn't have embedded result
  }
}

// ============================================
// ターミナル
// ============================================

function appendLog(message, level) {
  const terminal = document.getElementById("terminal-output");

  if (level === "wave" || level === "success" || level === "error") {
    const existingStatus = terminal.querySelector(".status-line");
    if (existingStatus) existingStatus.remove();
  }

  const line = document.createElement("div");

  if (level === "wave") {
    line.className = "status-line";
    line.innerHTML = `<span class="status-text">${escapeHtml(message)}</span><span class="dots"></span>`;
  } else {
    line.className = `log-line log-${level}`;
    line.textContent = message;
  }

  terminal.appendChild(line);
  terminal.scrollTop = terminal.scrollHeight;
}

function clearTerminal() {
  document.getElementById("terminal-output").innerHTML = "";
}

// ============================================
// 解析処理
// ============================================

async function analyze(mode = "individual") {
  const checkedFiles = getCheckedFiles();
  if (checkedFiles.length === 0) return;

  const terminalSection = document.getElementById("terminal-section");
  const resultSection = document.getElementById("result-section");
  const resultContent = document.getElementById("result-content");

  // UI準備
  terminalSection.hidden = false;
  resultSection.hidden = true;
  clearTerminal();
  document.getElementById("analyze-btn").disabled = true;
  document.getElementById("compare-btn").disabled = true;

  // ログリスナー
  if (logUnlisten) logUnlisten();
  logUnlisten = await listen("log", (event) => {
    appendLog(event.payload.message, event.payload.level);
  });

  const progressUnlisten = await listen("analysis-progress", (event) => {
    const file = pdfFiles.find(f => f.name === event.payload.file_name);
    if (file) {
      file.analyzing = !event.payload.completed;
      updateList();
    }
  });

  try {
    const paths = checkedFiles.map(f => f.path);
    const customInstruction = document.getElementById("custom-instruction").value.trim();
    const result = await invoke("analyze_pdfs", { paths, mode, customInstruction });

    const now = new Date().toLocaleString('ja-JP');
    if (mode === "compare") {
      applyCompareResult(checkedFiles, result, now);
    } else {
      applyIndividualResults(checkedFiles, result, now);
    }
    resultContent.innerHTML = markdownToHtml(result);
    resultSection.hidden = false;
    updateList();
  } catch (e) {
    appendLog(`エラー: ${e.toString()}`, "error");
    applyErrorResult(checkedFiles, e.toString());
    resultContent.innerHTML = `<p style="color: #ff4757;">エラー: ${escapeHtml(e.toString())}</p>`;
    resultSection.hidden = false;
    updateList();
  } finally {
    updateButtons();
    progressUnlisten();
    if (logUnlisten) {
      logUnlisten();
      logUnlisten = null;
    }
  }
}

function applyCompareResult(checkedFiles, result, timestamp) {
  checkedFiles.forEach(f => {
    const file = pdfFiles.find(pf => pf.path === f.path);
    if (file) {
      file.result = result;
      file.resultError = false;
      file.compareMode = true;
      file.analyzedAt = timestamp;
      file.documentType = "照合解析";
    }
  });
}

function applyIndividualResults(checkedFiles, result, timestamp) {
  const fileResults = parseIndividualResults(result);
  checkedFiles.forEach(f => {
    const file = pdfFiles.find(pf => pf.path === f.path);
    if (file) {
      const fileResult = fileResults[file.name];
      if (fileResult) {
        file.result = fileResult;
        file.resultError = false;
        file.analyzedAt = timestamp;
        file.embedded = true;
      }
    }
  });
}

function applyErrorResult(checkedFiles, error) {
  checkedFiles.forEach(f => {
    const file = pdfFiles.find(pf => pf.path === f.path);
    if (file) {
      file.result = error;
      file.resultError = true;
    }
  });
}

// ============================================
// ガイドライン生成
// ============================================

async function generateGuidelines() {
  const filesWithResults = pdfFiles.filter(f => f.checked && f.result && !f.resultError);
  if (filesWithResults.length === 0) {
    alert("解析結果のあるファイルを選択してください");
    return;
  }

  const paths = filesWithResults.map(f => f.path);
  const folder = paths[0].replace(/[\\/][^\\/]+$/, "");

  const terminalSection = document.getElementById("terminal-section");
  const resultSection = document.getElementById("result-section");
  const resultContent = document.getElementById("result-content");

  terminalSection.hidden = false;
  resultSection.hidden = true;
  clearTerminal();
  document.getElementById("guidelines-btn").disabled = true;

  if (logUnlisten) logUnlisten();
  logUnlisten = await listen("log", (event) => {
    appendLog(event.payload.message, event.payload.level);
  });

  try {
    appendLog(`対象: ${filesWithResults.length} ファイル`, "info");
    appendLog("PDFから埋め込みデータを収集中...", "wave");

    const customInstruction = document.getElementById("custom-instruction").value.trim();
    const result = await invoke("generate_guidelines", { paths, folder, customInstruction });

    resultContent.innerHTML = `<h2>📋 ガイドライン</h2><hr>` + markdownToHtml(result);
    resultSection.hidden = false;
    appendLog("ガイドライン生成完了", "success");
  } catch (e) {
    appendLog(`エラー: ${e.toString()}`, "error");
    resultContent.innerHTML = `<p style="color: #ff4757;">エラー: ${escapeHtml(e.toString())}</p>`;
    resultSection.hidden = false;
  } finally {
    updateButtons();
    if (logUnlisten) {
      logUnlisten();
      logUnlisten = null;
    }
  }
}

// ============================================
// ユーティリティ
// ============================================

// ============================================
// グローバル公開（onclick用）
// ============================================

window.removeFile = removeFile;
window.toggleFile = toggleFile;
window.showResult = showResult;
