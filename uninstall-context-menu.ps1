# PDF右クリックメニュー「AI解析」アンインストーラー

$regPath = "HKCU:\Software\Classes\SystemFileAssociations\.pdf\shell\AIAnalyze"

if (Test-Path $regPath) {
    Remove-Item -Path $regPath -Recurse -Force
    Write-Host "右クリックメニューを削除しました" -ForegroundColor Green
} else {
    Write-Host "メニューは既に削除されています" -ForegroundColor Yellow
}

Read-Host "Enterで終了"
