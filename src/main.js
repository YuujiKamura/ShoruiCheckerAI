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

  // Load settings in parallel (non-blocking)
  Promise.all([
    invoke("get_watch_folder"),
    invoke("get_model")
  ]).then(([watchFolder, currentModel]) => {
    if (watchFolder) {
      document.getElementById("watch-folder").value = watchFolder;
      document.getElementById("watch-status").textContent = "監視中: " + watchFolder;
    }
    document.getElementById("model-select").value = currentModel;
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
      pdfFiles.push({ name, path });
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
  await listen("tauri://drag-drop", (event) => {
    const paths = event.payload.paths || [];
    for (const path of paths) {
      if (path.toLowerCase().endsWith(".pdf")) {
        const name = path.split(/[\\/]/).pop();
        if (!pdfFiles.find(f => f.path === path)) {
          pdfFiles.push({ name, path });
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
            pdfFiles.push({ name, path });
          }
        }
        updateList();
      }
    } catch (e) {
      console.error("File open error:", e);
    }
  });

  // Button events
  analyzeBtn.addEventListener("click", analyze);
  clearBtn.addEventListener("click", clearFiles);
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
  const analyzeBtn = document.getElementById("analyze-btn");

  count.textContent = `(${pdfFiles.length})`;
  analyzeBtn.disabled = pdfFiles.length === 0;

  list.innerHTML = pdfFiles.map((f, i) => `
    <li>
      <div class="file-info">
        <div class="filename">${escapeHtml(f.name)}</div>
        <div class="path">${escapeHtml(f.path)}</div>
      </div>
      <button class="remove" onclick="removeFile(${i})">✕</button>
    </li>
  `).join("");
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

async function analyze() {
  if (pdfFiles.length === 0) return;

  const terminalSection = document.getElementById("terminal-section");
  const resultSection = document.getElementById("result-section");
  const resultContent = document.getElementById("result-content");
  const analyzeBtn = document.getElementById("analyze-btn");

  // Show terminal, hide result
  terminalSection.hidden = false;
  resultSection.hidden = true;
  clearTerminal();
  analyzeBtn.disabled = true;

  // Listen for log events
  if (logUnlisten) {
    logUnlisten();
  }
  logUnlisten = await listen("log", (event) => {
    const { message, level } = event.payload;
    appendLog(message, level);
  });

  try {
    const paths = pdfFiles.map(f => f.path);
    const result = await invoke("analyze_pdfs", { paths });

    resultContent.innerHTML = markdownToHtml(result);
    resultSection.hidden = false;
  } catch (e) {
    appendLog(`エラー: ${e.toString()}`, "error");
    resultContent.innerHTML = `<p style="color: #ff4757;">エラー: ${escapeHtml(e.toString())}</p>`;
    resultSection.hidden = false;
  } finally {
    analyzeBtn.disabled = pdfFiles.length === 0;
    if (logUnlisten) {
      logUnlisten();
      logUnlisten = null;
    }
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
