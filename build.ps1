# 构建 assets-cli：cargo build --release；-Install 交给 install.ps1
# （放进用户级目录 -> 确保目录在用户 PATH 上 -> 跑一次 assets init）
param([switch]$Install)
$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build 失败（退出码 $LASTEXITCODE）" }
    if ($Install) {
        & (Join-Path $PSScriptRoot 'install.ps1') -SkipBuild
        if ($LASTEXITCODE -ne 0) { throw "install.ps1 失败（退出码 $LASTEXITCODE）" }
    }
} finally { Pop-Location }
