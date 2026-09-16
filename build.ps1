# 构建 assets-cli：cargo build --release，并可选装到 cargo bin（已在 PATH，不改 PATH）
param([switch]$Install)
$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build 失败（退出码 $LASTEXITCODE）" }
    if ($Install) {
        cargo install --path cli --force
        if ($LASTEXITCODE -ne 0) { throw "cargo install 失败（退出码 $LASTEXITCODE）" }
        Write-Host "已安装到 cargo bin（已在 PATH，无需改 PATH）"
    }
} finally { Pop-Location }
