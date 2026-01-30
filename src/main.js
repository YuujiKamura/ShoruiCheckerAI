const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { open } = window.__TAURI__.dialog;
const { isPermissionGranted, requestPermission, sendNotification } = window.__TAURI__.notification;

// State
let pdfFiles = [];
let logUnlisten = null;

// Initialize
window.addEventListener("DOMContentLoaded", async () => {
  const dropZone = document.getElementById("drop-zone");
  const analyzeBtn = document.getElementById("analyze-btn");
  const clearBtn = document.getElementById("clear-btn");
  const settingsBtn = document.getElementById("settings-btn");
  const closeSettings = document.getElementById("close-settings");
  const selectFolderBtn = document.getElementById("select-folder-btn");
  const settingsModal = document.getElementById("settings-modal");

  // Load settings and history in parallel (non-blocking)
  Promise.all([
    invoke("get_watch_folder"),
    invoke("get_model"),
    invoke("get_all_history")
  ]).then(([watchFolder, currentModel, history]) => {
    if (watchFolder) {
      document.getElementById("watch-folder").value = watchFolder;
      document.getElementById("watch-status").textContent = "監視中: " + watchFolder;
    }
    document.getElementById("model-select").value = currentModel;

    // Load history into file list
    if (history && history.length > 0) {
      for (const entry of history) {
        // Skip if already in list
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
  }).catch(console.error);

  // Model selection
  document.getElementById("model-select").addEventListener("change", async (e) => {
    await invoke("set_model", { model: e.target.value });
  });

  // Check Gemini auth status (delayed to avoid blocking startup)
  setTimeout(() => checkAuthStatus(), 2000);

  // Auth button
  document.getElementById("auth-btn").addEventListener("click", async () => {
    await invoke("open_gemini_auth");
    // Recheck after a delay
    setTimeout(checkAuthStatus, 3000);
  });

  // Settings modal
  settingsBtn.addEventListener("click", () => {
    settingsModal.hidden = false;
  });

  closeSettings.addEventListener("click", () => {
    settingsModal.hidden = true;
  });

  settingsModal.addEventListener("click", (e) => {
    if (e.target === settingsModal) {
      settingsModal.hidden = true;
    }
  });

  // Select folder
  selectFolderBtn.addEventListener("click", async () => {
    try {
      const folder = await open({ directory: true });
      if (folder) {
        document.getElementById("watch-folder").value = folder;
        await invoke("set_watch_folder", { folder });
        document.getElementById("watch-status").textContent = "監視開始: " + folder;
        document.getElementById("watch-status").classList.remove("error");
      }
    } catch (e) {
      document.getElementById("watch-status").textContent = "エラー: " + e;
      document.getElementById("watch-status").classList.add("error");
    }
  });

  // Listen for PDF detection - auto add to list
  await listen("pdf-detected", (event) => {
    const { path, name } = event.payload;
    // Add to file list automatically
    if (!pdfFiles.find(f => f.path === path)) {
      pdfFiles.push({ name, path, checked: true });
      updateList();
      // Show toast notification (info only)
      showToast(name);
    }
  });

  // Listen for notification request
  await listen("show-notification", async (event) => {
    const { title, body, path } = event.payload;

    // Check permission and send system notification
    let permissionGranted = await isPermissionGranted();
    if (!permissionGranted) {
      const permission = await requestPermission();
      permissionGranted = permission === 'granted';
    }

    if (permissionGranted) {
      sendNotification({ title, body });
    }
  });

  // Toast close button
  document.getElementById("toast-close").addEventListener("click", () => {
    hideToast();
  });

  // Tauri file drop event
  await listen("tauri://drag-drop", async (event) => {
    const paths = event.payload.paths || [];
    for (const path of paths) {
      if (path.toLowerCase().endsWith(".pdf")) {
        const name = path.split(/[\\/]/).pop();
        if (!pdfFiles.find(f => f.path === path)) {
          const file = { name, path, checked: true };
          // Check for embedded result in PDF
          await loadEmbeddedResult(file);
          pdfFiles.push(file);
        }
      }
    }
    updateList();
  });

  await listen("tauri://drag-enter", () => {
    dropZone.classList.add("dragover");
  });

  await listen("tauri://drag-leave", () => {
    dropZone.classList.remove("dragover");
  });

  // Click to open file dialog
  dropZone.addEventListener("click", async () => {
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
            // Check for embedded result in PDF
            await loadEmbeddedResult(file);
            pdfFiles.push(file);
          }
        }
        updateList();
      }
    } catch (e) {
      console.error("File open error:", e);
    }
  });

  // Button events
  const compareBtn = document.getElementById("compare-btn");
  const selectAllBtn = document.getElementById("select-all-btn");
  const selectNoneBtn = document.getElementById("select-none-btn");

  analyzeBtn.addEventListener("click", () => analyze("individual"));
  compareBtn.addEventListener("click", () => analyze("compare"));
  clearBtn.addEventListener("click", clearFiles);
  selectAllBtn.addEventListener("click", selectAll);
  selectNoneBtn.addEventListener("click", selectNone);
});

async function checkAuthStatus() {
  const statusEl = document.getElementById("auth-status");
  statusEl.textContent = "確認中...";
  statusEl.className = "auth-status";

  try {
    const isAuth = await invoke("check_gemini_auth");
    if (isAuth) {
      statusEl.textContent = "✓ 認証済み";
      statusEl.className = "auth-status ok";
    } else {
      statusEl.textContent = "✗ 未認証";
      statusEl.className = "auth-status ng";
    }
  } catch (e) {
    statusEl.textContent = "✗ エラー";
    statusEl.className = "auth-status ng";
  }
}

function showToast(name) {
  document.getElementById("toast-body").textContent = name;
  document.getElementById("notification-toast").hidden = false;

  // Auto-hide after 5 seconds
  setTimeout(() => {
    hideToast();
  }, 5000);
}

function hideToast() {
  document.getElementById("notification-toast").hidden = true;
}

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
  const analyzeBtn = document.getElementById("analyze-btn");
  const compareBtn = document.getElementById("compare-btn");
  const checkedCount = pdfFiles.filter(f => f.checked).length;

  analyzeBtn.disabled = checkedCount === 0;
  compareBtn.disabled = checkedCount < 2;
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

function appendLog(message, level) {
  const terminal = document.getElementById("terminal-output");

  // Remove existing status line when new wave comes or when done
  if (level === "wave" || level === "success" || level === "error") {
    const existingStatus = terminal.querySelector(".status-line");
    if (existingStatus) {
      existingStatus.remove();
    }
  }

  const line = document.createElement("div");

  if (level === "wave") {
    // Create prominent status line with animated dots
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
  const terminal = document.getElementById("terminal-output");
  terminal.innerHTML = "";
}

async function analyze(mode = "individual") {
  const checkedFiles = getCheckedFiles();
  if (checkedFiles.length === 0) return;

  const terminalSection = document.getElementById("terminal-section");
  const resultSection = document.getElementById("result-section");
  const resultContent = document.getElementById("result-content");
  const analyzeBtn = document.getElementById("analyze-btn");
  const compareBtn = document.getElementById("compare-btn");

  // Show terminal, hide result
  terminalSection.hidden = false;
  resultSection.hidden = true;
  clearTerminal();
  analyzeBtn.disabled = true;
  compareBtn.disabled = true;

  // Listen for log events
  if (logUnlisten) {
    logUnlisten();
  }
  logUnlisten = await listen("log", (event) => {
    const { message, level } = event.payload;
    appendLog(message, level);
  });

  // Listen for per-file progress
  const progressUnlisten = await listen("analysis-progress", (event) => {
    const { file_name, completed, success } = event.payload;
    // Find the file and update its status
    const file = pdfFiles.find(f => f.name === file_name);
    if (file) {
      file.analyzing = !completed;
      updateList();
    }
  });

  try {
    const paths = checkedFiles.map(f => f.path);
    const customInstruction = document.getElementById("custom-instruction").value.trim();
    const result = await invoke("analyze_pdfs", { paths, mode, customInstruction });

    const now = new Date().toLocaleString('ja-JP');
    if (mode === "compare") {
      // 照合モード: 全ファイルに同じ結果を紐付け
      checkedFiles.forEach(f => {
        const file = pdfFiles.find(pf => pf.path === f.path);
        if (file) {
          file.result = result;
          file.resultError = false;
          file.compareMode = true;
          file.analyzedAt = now;
          file.documentType = "照合解析";
        }
      });
      resultContent.innerHTML = markdownToHtml(result);
    } else {
      // 個別モード: ファイルごとに結果をパース
      const fileResults = parseIndividualResults(result);
      checkedFiles.forEach(f => {
        const file = pdfFiles.find(pf => pf.path === f.path);
        if (file) {
          const fileResult = fileResults[file.name];
          if (fileResult) {
            file.result = fileResult;
            file.resultError = false;
            file.analyzedAt = now;
            file.embedded = true; // Result is embedded in PDF by backend
          }
        }
      });
      resultContent.innerHTML = markdownToHtml(result);
    }

    resultSection.hidden = false;
    updateList();
  } catch (e) {
    appendLog(`エラー: ${e.toString()}`, "error");
    // エラー時も結果を記録
    checkedFiles.forEach(f => {
      const file = pdfFiles.find(pf => pf.path === f.path);
      if (file) {
        file.result = e.toString();
        file.resultError = true;
      }
    });
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

// 個別解析結果をファイルごとにパース
function parseIndividualResults(result) {
  const fileResults = {};
  const sections = result.split(/\n## 📄 /);

  for (const section of sections) {
    if (!section.trim()) continue;
    const lines = section.split('\n');
    const fileName = lines[0].trim();
    const content = lines.slice(1).join('\n').replace(/^---\n/, '').trim();
    if (fileName) {
      fileResults[fileName] = content;
    }
  }

  return fileResults;
}

// Load embedded analysis result from PDF metadata
async function loadEmbeddedResult(file) {
  try {
    const embedded = await invoke("read_pdf_result", { path: file.path });
    if (embedded) {
      const [result, date] = embedded;
      file.result = result;
      file.resultError = false;
      file.analyzedAt = date;
      file.embedded = true; // Mark as loaded from PDF
    }
  } catch (e) {
    // Ignore - PDF doesn't have embedded result
  }
}

function escapeHtml(text) {
  const div = document.createElement("div");
  div.textContent = text || "";
  return div.innerHTML;
}

function markdownToHtml(md) {
  if (!md) return "";

  return md
    // Headers
    .replace(/^### (.+)$/gm, '<h3>$1</h3>')
    .replace(/^## (.+)$/gm, '<h2>$1</h2>')
    .replace(/^# (.+)$/gm, '<h1>$1</h1>')
    // Bold
    .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
    // Tables
    .replace(/\|(.+)\|/g, (match) => {
      const cells = match.split('|').filter(c => c.trim());
      if (cells.every(c => /^[-:]+$/.test(c.trim()))) {
        return ''; // Skip separator row
      }
      const isHeader = cells.some(c => c.includes('---'));
      const tag = isHeader ? 'th' : 'td';
      return '<tr>' + cells.map(c => `<${tag}>${c.trim()}</${tag}>`).join('') + '</tr>';
    })
    .replace(/(<tr>.*<\/tr>\n?)+/g, '<table>$&</table>')
    // Lists
    .replace(/^- (.+)$/gm, '<li>$1</li>')
    .replace(/(<li>.*<\/li>\n?)+/g, '<ul>$&</ul>')
    // Line breaks
    .replace(/\n\n/g, '</p><p>')
    .replace(/\n/g, '<br>');
}

// Global functions for onclick
window.removeFile = removeFile;
window.toggleFile = toggleFile;
window.showResult = showResult;
