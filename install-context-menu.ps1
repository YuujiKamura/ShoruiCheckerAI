# PDF Context Menu Installer - Claude Code版（複数ファイル対応）
# 右クリック → Claude Codeで整合性チェック

$ErrorActionPreference = "Stop"

$claudePath = "$env:APPDATA\npm\claude.cmd"
if (-not (Test-Path $claudePath)) {
    Write-Host "Error: claude CLI not found" -ForegroundColor Red
    exit 1
}

Write-Host "Installing context menu..." -ForegroundColor Cyan

# ラッパースクリプト作成（複数ファイル対応）
$wrapperDir = "$env:LOCALAPPDATA\ShoruiChecker"
$wrapperPath = "$wrapperDir\pdf-analyze.cmd"

if (-not (Test-Path $wrapperDir)) {
    New-Item -Path $wrapperDir -ItemType Directory -Force | Out-Null
}

# ラッパースクリプト: 全ての引数をClaudeに渡す
@"
@echo off
chcp 65001 > nul
cd /d "%~dp1"
set FILES=
:loop
if "%~1"=="" goto run
set FILES=%FILES% "%~1"
shift
goto loop
:run
echo === PDF整合性チェック ===
echo.
claude -p "以下のPDFを読み取って整合性をチェックしてください:%FILES%" --output-format text
echo.
pause
"@ | Out-File -FilePath $wrapperPath -Encoding ascii

Write-Host "Wrapper: $wrapperPath" -ForegroundColor Gray

# レジストリ設定
$regPath = "HKCU:\Software\Classes\SystemFileAssociations\.pdf\shell\AIAnalyze"
$commandPath = "$regPath\command"

try {
    if (-not (Test-Path $regPath)) {
        New-Item -Path $regPath -Force | Out-Null
    }
    Set-ItemProperty -Path $regPath -Name "(Default)" -Value "AI Analyze (Claude)"
    Set-ItemProperty -Path $regPath -Name "Icon" -Value "shell32.dll,23"
    # 複数選択対応
    Set-ItemProperty -Path $regPath -Name "MultiSelectModel" -Value "Player"

    if (-not (Test-Path $commandPath)) {
        New-Item -Path $commandPath -Force | Out-Null
    }
    Set-ItemProperty -Path $commandPath -Name "(Default)" -Value "`"$wrapperPath`" %*"

    Write-Host "Done!" -ForegroundColor Green
    Write-Host "Right-click PDF(s) and select 'AI Analyze (Claude)'" -ForegroundColor Cyan
    Write-Host "Multiple file selection supported." -ForegroundColor Cyan
} catch {
    Write-Host "Error: $_" -ForegroundColor Red
    exit 1
}
