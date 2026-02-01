# ShoruiChecker モジュール分離計画

## 概要

ShoruiCheckerのRustバックエンド（`src-tauri/`）を、再利用可能な独立クレートに分離するための設計ドキュメントです。

---

## 1. 分離の目的

### 1.1 再利用性（Reusability）
- **gemini-analyzer**: Gemini CLIラッパーを他のプロジェクトでも使用可能に
- **pdf-toolkit**: PDF操作（メタデータ埋め込み、テキスト抽出）を汎用ライブラリ化
- **folder-watcher**: ファイル監視機能を他のアプリケーションでも利用可能に

### 1.2 テスト容易性（Testability）
- 各クレートを独立してユニットテスト可能
- Tauri依存を排除し、純粋なRustコードとしてテスト
- モック実装による統合テストの簡易化

### 1.3 関心の分離（Separation of Concerns）
- **UI/Tauri層**: イベント処理、コマンド定義、通知
- **ドメイン層**: PDF解析ロジック、ガイドライン生成
- **インフラ層**: Gemini CLI実行、ファイル監視、設定管理

---

## 2. 現在のアーキテクチャ

```
┌─────────────────────────────────────────────────────────────────┐
│                    ShoruiChecker (Tauri App)                    │
├─────────────────────────────────────────────────────────────────┤
│  src-tauri/src/                                                 │
│  ├── lib.rs          # エントリポイント、Tauriセットアップ      │
│  ├── main.rs         # CLI引数処理、ヘッドレスモード            │
│  ├── analysis.rs     # PDF解析ロジック (Gemini連携)             │
│  ├── gemini.rs       # Gemini認証UI                             │
│  ├── gemini_cli.rs   # Gemini CLI実行 (PowerShell経由)          │
│  ├── pdf_embed.rs    # PDFメタデータ埋め込み (lopdf)            │
│  ├── watcher.rs      # PDFフォルダ監視 (notify)                 │
│  ├── code_review.rs  # コードレビュー機能 (git diff + Gemini)   │
│  ├── settings.rs     # 設定管理                                 │
│  ├── history.rs      # 解析履歴管理                             │
│  ├── guidelines.rs   # ガイドライン生成                         │
│  ├── events.rs       # Tauriイベント定義                        │
│  └── error.rs        # エラー型                                 │
└─────────────────────────────────────────────────────────────────┘
```

### 現在の問題点

1. **密結合**: Gemini CLI実行ロジックがアプリケーション固有のコードに埋め込まれている
2. **重複**: `pdf_embed.rs`のBase64処理やPDFメタデータ操作が汎用化されていない
3. **再利用不可**: ファイル監視ロジックがTauriイベントに依存している

---

## 3. 分離後のアーキテクチャ

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                         ShoruiChecker (Tauri App)                            │
│  src-tauri/src/                                                              │
│  ├── lib.rs              # Tauriセットアップ + クレート統合                   │
│  ├── main.rs             # CLI引数処理                                       │
│  ├── commands/           # Tauriコマンド (薄いラッパー)                       │
│  │   ├── analysis.rs     # analyze_pdfs コマンド                             │
│  │   ├── watcher.rs      # フォルダ監視コマンド                              │
│  │   └── settings.rs     # 設定コマンド                                      │
│  ├── events.rs           # Tauriイベント定義                                 │
│  └── app_state.rs        # アプリケーション状態                              │
└──────────────────────────────────────────────────────────────────────────────┘
           │                      │                        │
           ▼                      ▼                        ▼
┌──────────────────┐  ┌──────────────────────┐  ┌──────────────────────┐
│  gemini-analyzer │  │      pdf-toolkit     │  │    folder-watcher    │
│  (外部クレート)   │  │    (外部クレート)     │  │    (外部クレート)     │
├──────────────────┤  ├──────────────────────┤  ├──────────────────────┤
│ - analyze()      │  │ - メタデータ読み書き  │  │ - FolderWatcher      │
│ - prompt()       │  │ - テキスト抽出       │  │ - コールバック対応    │
│ - AnalysisBuilder│  │ - Base64エンコード   │  │ - 拡張子フィルタ      │
│ - AnalyzeOptions │  │ - カスタムフィールド  │  │ - 再帰監視           │
└──────────────────┘  └──────────────────────┘  └──────────────────────┘
        │                        │                        │
        ▼                        ▼                        ▼
    C:/Users/yuuji/          C:/Users/yuuji/          C:/Users/yuuji/
    gemini-analyzer/         pdf-toolkit/             folder-watcher/
```

---

## 4. 各クレートの責務

### 4.1 gemini-analyzer

**パス**: `C:/Users/yuuji/gemini-analyzer/`

**責務**:
- Gemini CLI（`gemini.cmd`）のラッパー
- 一時ディレクトリ管理
- プロンプト実行とレスポンス取得

**現在のAPI**:
```rust
// ファイル解析
pub fn analyze<P: AsRef<Path>>(
    prompt: &str,
    files: &[P],
    options: AnalyzeOptions,
) -> Result<String>

// テキストのみのプロンプト
pub fn prompt(prompt: &str, options: AnalyzeOptions) -> Result<String>

// ビルダーパターン
AnalysisBuilder::new("prompt")
    .file("document.pdf")
    .model("gemini-2.5-pro")
    .json()
    .run()
```

**ShoruiCheckerでの使用例**:
```rust
use gemini_analyzer::{analyze, AnalyzeOptions};

// PDF解析
let result = analyze(
    &prompt,
    &[pdf_path],
    AnalyzeOptions::with_model("gemini-2.5-pro"),
)?;
```

---

### 4.2 pdf-toolkit

**パス**: `C:/Users/yuuji/pdf-toolkit/`

**責務**:
- PDFメタデータの読み書き
- テキスト抽出
- カスタムフィールド（解析結果埋め込み用）

**現在のAPI**:
```rust
// メタデータ操作
pub fn get_metadata(path: &Path) -> Result<PdfMetadata>
pub fn set_metadata(path: &Path, metadata: &PdfMetadata) -> Result<()>
pub fn get_custom_field(path: &Path, key: &str) -> Result<Option<String>>
pub fn set_custom_field(path: &Path, key: &str, value: &str) -> Result<()>

// テキスト抽出
pub fn extract_text(path: &Path) -> Result<String>
pub fn get_page_count(path: &Path) -> Result<u32>
```

**ShoruiCheckerでの使用例**:
```rust
use pdf_toolkit::metadata::{set_custom_field, get_custom_field};

// 解析結果をPDFに埋め込み
set_custom_field(
    Path::new(&pdf_path),
    "ShoruiCheckerResult",
    &base64::encode(&result),
)?;

// 埋め込まれた結果を読み取り
if let Some(encoded) = get_custom_field(Path::new(&pdf_path), "ShoruiCheckerResult")? {
    let result = base64::decode(&encoded)?;
}
```

---

### 4.3 folder-watcher

**パス**: `C:/Users/yuuji/folder-watcher/`

**責務**:
- フォルダ変更の監視
- 拡張子フィルタリング
- コールバックベースのイベント処理

**現在のAPI**:
```rust
// ビルダーパターン
let watcher = FolderWatcher::new(Path::new("/path/to/watch"))?
    .with_filter(&["pdf", "txt"])
    .on_create(|path| { /* 処理 */ })
    .on_modify(|path| { /* 処理 */ })
    .on_delete(|path| { /* 処理 */ });

watcher.start()?;
// ...
watcher.stop()?;
```

**ShoruiCheckerでの使用例**:
```rust
use folder_watcher::FolderWatcher;

let app_handle = app.handle().clone();
let watcher = FolderWatcher::new(Path::new(&folder))?
    .with_filter(&["pdf"])
    .on_create(move |path| {
        // Tauriイベントを発火
        let _ = app_handle.emit("pdf-detected", PdfDetectedEvent {
            path: path.to_string_lossy().to_string(),
            name: path.file_name().unwrap().to_string_lossy().to_string(),
        });
    });

watcher.start()?;
```

---

## 5. Cargo.toml 更新案

### 5.1 ShoruiChecker の Cargo.toml

```toml
[package]
name = "shoruichecker"
version = "0.2.0"
description = "PDF整合性チェッカー"
authors = ["you"]
edition = "2021"

[lib]
name = "shoruichecker_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
# Tauri関連
tauri = { version = "2", features = ["tray-icon"] }
tauri-plugin-opener = "2"
tauri-plugin-dialog = "2"
tauri-plugin-notification = "2"

# 分離したクレート（ローカルパス）
gemini-analyzer = { path = "../gemini-analyzer" }
pdf-toolkit = { path = "../pdf-toolkit" }
folder-watcher = { path = "../folder-watcher" }

# 共通依存
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["sync"] }
dirs = "5"
chrono = "0.4"
base64 = "0.22.1"

# 削除される依存（クレートに移動）
# notify = { version = "6", features = ["serde"] }  # folder-watcher
# lopdf = "0.34"                                     # pdf-toolkit
```

### 5.2 将来の公開（オプション）

ローカルパスからcrates.io公開版への移行:
```toml
[dependencies]
gemini-analyzer = "0.1"
pdf-toolkit = "0.1"
folder-watcher = "0.1"
```

---

## 6. マイグレーション計画

### Phase 1: 準備（現在）
- [x] 既存クレートの確認
  - `C:/Users/yuuji/gemini-analyzer/` - 存在
  - `C:/Users/yuuji/pdf-toolkit/` - 存在
  - `C:/Users/yuuji/folder-watcher/` - 存在
- [x] 各クレートのAPI確認
- [x] 設計ドキュメント作成（本ドキュメント）

### Phase 2: pdf-toolkit 統合
**優先度: 高**（最も単純な変更）

1. `pdf_embed.rs` の機能を `pdf-toolkit` に移行
   - `ShoruiCheckerResult` カスタムフィールド操作
   - Base64エンコード/デコード

2. ShoruiCheckerを更新
   ```rust
   // Before (pdf_embed.rs)
   embed_result_in_pdf(&path, &result)?;

   // After (pdf-toolkit使用)
   use pdf_toolkit::metadata::set_custom_field;
   set_custom_field(Path::new(&path), "ShoruiCheckerResult", &encoded_result)?;
   ```

3. テスト実行・動作確認

### Phase 3: folder-watcher 統合
**優先度: 中**

1. `watcher.rs` を `folder-watcher` に置き換え
   - コールバック内でTauriイベントを発火する形に変更

2. `code_review.rs` のファイル監視部分も統合

3. グローバル状態管理の見直し
   - `WATCHER_HANDLE` をアプリケーション状態に移動

### Phase 4: gemini-analyzer 統合
**優先度: 中**

1. `gemini_cli.rs` の機能を `gemini-analyzer` で置き換え
   - `run_gemini_with_prompt` -> `gemini_analyzer::analyze`
   - `create_temp_dir` / `cleanup_temp_dir` -> ライブラリ内部で管理

2. `analysis.rs` の簡素化
   - プロンプト構築のみに集中
   - Gemini実行はライブラリに委譲

### Phase 5: ドメインロジック分離（オプション）
**優先度: 低**（将来の拡張）

1. `shoruichecker-core` クレート作成
   - ガイドライン生成ロジック
   - 履歴管理
   - 書類タイプ判定

2. Tauriアプリケーションは薄いUIラッパーに

---

## 7. ディレクトリ構造（最終形）

```
C:/Users/yuuji/
├── ShoruiChecker/
│   ├── docs/
│   │   └── MODULARIZATION.md    # 本ドキュメント
│   ├── src/                      # フロントエンド (TypeScript/React)
│   ├── src-tauri/
│   │   ├── Cargo.toml           # 外部クレートへの依存
│   │   ├── src/
│   │   │   ├── lib.rs           # Tauriセットアップ
│   │   │   ├── main.rs          # エントリポイント
│   │   │   ├── commands/        # Tauriコマンド
│   │   │   │   ├── mod.rs
│   │   │   │   ├── analysis.rs
│   │   │   │   ├── watcher.rs
│   │   │   │   └── settings.rs
│   │   │   ├── domain/          # ドメインロジック
│   │   │   │   ├── mod.rs
│   │   │   │   ├── guidelines.rs
│   │   │   │   └── history.rs
│   │   │   ├── events.rs
│   │   │   └── app_state.rs
│   │   └── ...
│   └── ...
│
├── gemini-analyzer/              # 独立クレート
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── executor.rs
│       ├── temp.rs
│       └── error.rs
│
├── pdf-toolkit/                  # 独立クレート
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs               # (要作成)
│       ├── metadata.rs
│       ├── text.rs
│       └── error.rs
│
└── folder-watcher/               # 独立クレート
    ├── Cargo.toml
    └── src/
        ├── lib.rs
        ├── watcher.rs
        └── error.rs
```

---

## 8. 移行時の注意点

### 8.1 エラーハンドリング
各クレートは独自のエラー型を持つため、ShoruiChecker側で統一的にハンドリング:
```rust
use gemini_analyzer::Error as GeminiError;
use pdf_toolkit::PdfError;
use folder_watcher::WatcherError;

enum AppError {
    Gemini(GeminiError),
    Pdf(PdfError),
    Watcher(WatcherError),
    // ...
}
```

### 8.2 設定管理
`gemini-analyzer` の `gemini_path` オプションを活用:
```rust
let gemini_path = std::env::var("GEMINI_CMD_PATH").ok();
let options = AnalyzeOptions::default()
    .with_model(&settings.model)
    .with_gemini_path(gemini_path.unwrap_or_else(|| "gemini.cmd".into()));
```

### 8.3 Tauri依存の分離
- クレート側: Tauri依存なし（純粋なRust）
- アプリケーション側: Tauriイベント発火はコールバック内で

---

## 9. テスト戦略

### 9.1 クレートレベル
```rust
// gemini-analyzer/tests/integration.rs
#[test]
fn test_analyze_pdf() {
    // Gemini CLIがインストールされている環境でのみ実行
}

// pdf-toolkit/tests/metadata.rs
#[test]
fn test_custom_field_roundtrip() {
    // テスト用PDFを使った往復テスト
}
```

### 9.2 アプリケーションレベル
```rust
// src-tauri/tests/integration.rs
#[test]
fn test_full_workflow() {
    // PDF検出 -> 解析 -> 結果埋め込み の一連のフロー
}
```

---

## 10. 参考資料

- [gemini-analyzer README](../../../gemini-analyzer/README.md) *(要確認)*
- [pdf-toolkit ソース](../../../pdf-toolkit/src/)
- [folder-watcher ソース](../../../folder-watcher/src/)
- [Tauri v2 ドキュメント](https://tauri.app/v2/)
- [Rust Workspaces](https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html)
