# assets-cli 便捷安装：把 assets.exe 与 assets-gui.exe 放进一个用户级目录、确保该目录在用户 PATH 上，
# 再跑一次 assets init，并给图形界面建桌面 / 开始菜单快捷方式。
#
# 改用户 PATH 有两条机器纪律：不用 setx（它截断到 1024 字符并把 REG_EXPAND_SZ 降级成 REG_SZ），
# 也不要用 [Environment]::GetEnvironmentVariable('Path','User') 读回来再写回去 —— 那个读法会把
# %JAVA_HOME%\bin 这类引用**展开**成字面路径，写回就永久丢掉了变量引用。
# 所以这里直接走注册表 API：读用 DoNotExpandEnvironmentNames，写回显式指定 ExpandString。
param(
    [string]$Target = 'D:\Dev\Global\bin',
    [string]$PathKey = 'Environment',
    [switch]$SkipBuild,
    [switch]$SkipInit,
    [switch]$SkipGui,
    [switch]$Uninstall
)

$ErrorActionPreference = 'Stop'
$repo = $PSScriptRoot
$exeName = 'assets.exe'
$guiName = 'assets-gui.exe'
$shortcutName = 'assets 资产台账.lnk'

$nativeSource = @(
    'using System;'
    'using System.Runtime.InteropServices;'
    'public static class AssetsCliNative {'
        '[DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)]'
        'public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, IntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out IntPtr lpdwResult);'
    '}'
) -join "`n"
Add-Type -TypeDefinition $nativeSource -Language CSharp

function Get-RawUserPath([string]$keyName) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($keyName, $false)
    if ($null -eq $key) { return '' }
    try {
        return [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    } finally { $key.Close() }
}

function Set-RawUserPath([string]$keyName, [string]$value) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($keyName, $true)
    if ($null -eq $key) { $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($keyName) }
    try {
        $key.SetValue('Path', $value, [Microsoft.Win32.RegistryValueKind]::ExpandString)
    } finally { $key.Close() }
    # 新起的 shell 自己会读注册表，但已开着的资源管理器不会：广播一次让环境块重新生成。
    $result = [IntPtr]::Zero
    [void][AssetsCliNative]::SendMessageTimeout([IntPtr]0xffff, 0x1A, [IntPtr]::Zero, 'Environment', 2, 5000, [ref]$result)
}

function Test-DirOnPath([string]$keyName, [string]$dir) {
    foreach ($entry in ((Get-RawUserPath $keyName) -split ';')) {
        if ([string]::IsNullOrWhiteSpace($entry)) { continue }
        if ([Environment]::ExpandEnvironmentVariables($entry).TrimEnd('\') -ieq $dir) { return $true }
    }
    return $false
}

# 快捷方式落两处：桌面（用户直接看到）与开始菜单。都取 shell 的文件夹路径，
# 这样桌面被重定向到 OneDrive 之类的场景也能落在真实位置。
function Get-ShortcutPaths {
    $paths = @()
    $desktop = [Environment]::GetFolderPath('Desktop')
    if ($desktop) { $paths += (Join-Path $desktop $shortcutName) }
    $startMenu = [Environment]::GetFolderPath('StartMenu')
    if ($startMenu) { $paths += (Join-Path (Join-Path $startMenu 'Programs') $shortcutName) }
    return $paths
}

function New-GuiShortcut([string]$path, [string]$exe) {
    $shell = New-Object -ComObject WScript.Shell
    try {
        $link = $shell.CreateShortcut($path)
        $link.TargetPath = $exe
        $link.WorkingDirectory = (Split-Path -Parent $exe)
        # 图标取自 exe 内嵌的 PE 资源，不额外依赖 .ico 文件
        $link.IconLocation = "$exe,0"
        $link.Description = 'assets 资产台账'
        $link.Save()
    } finally {
        [void][Runtime.InteropServices.Marshal]::ReleaseComObject($shell)
    }
}

$target = [Environment]::ExpandEnvironmentVariables($Target).TrimEnd('\')
if (-not [IO.Path]::IsPathRooted($target)) { throw "目标目录必须是绝对路径：$Target" }
$installed = Join-Path $target $exeName
$guiInstalled = Join-Path $target $guiName

if ($Uninstall) {
    if (Test-Path -LiteralPath $installed) {
        Remove-Item -LiteralPath $installed -Force
        Write-Host "已移除 $installed"
    } else {
        Write-Host "没找到 $installed，无需移除"
    }
    if (Test-Path -LiteralPath $guiInstalled) {
        Remove-Item -LiteralPath $guiInstalled -Force
        Write-Host "已移除 $guiInstalled"
    } else {
        Write-Host "没找到 $guiInstalled，无需移除"
    }
    foreach ($shortcut in (Get-ShortcutPaths)) {
        if (Test-Path -LiteralPath $shortcut) {
            Remove-Item -LiteralPath $shortcut -Force
            Write-Host "已移除快捷方式 $shortcut"
        }
    }
    if (Test-DirOnPath $PathKey $target) {
        Write-Host "用户 PATH 里的 $target 保持不动：它可能还装着别的东西，删不删由你决定。要删就在 PowerShell 里跑："
        Write-Host "  `$k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('$PathKey', `$true)"
        Write-Host "  `$p = (`$k.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) -split ';') | Where-Object { `$_.TrimEnd('\') -ine '$target' }"
        Write-Host "  `$k.SetValue('Path', (`$p -join ';'), [Microsoft.Win32.RegistryValueKind]::ExpandString); `$k.Close()"
    }
    Write-Host '台账、快照与 shell 钩子都没动：要拆钩子就删 profile 里的 assets-cli 标记块与 ~\.assets-cli\。'
    exit 0
}

if (-not $SkipBuild) {
    Push-Location $repo
    try {
        cargo build --release
        if ($LASTEXITCODE -ne 0) { throw "cargo build 失败（退出码 $LASTEXITCODE）" }
    } finally { Pop-Location }
}

$built = Join-Path $repo 'target\release\assets.exe'
if (-not (Test-Path -LiteralPath $built)) { throw "没找到构建产物：$built（别加 -SkipBuild）" }
New-Item -ItemType Directory -Force -Path $target | Out-Null
Copy-Item -LiteralPath $built -Destination $installed -Force
Write-Host "已放入 $installed"

if (Test-DirOnPath $PathKey $target) {
    Write-Host "用户 PATH 里已有 $target，没动它"
} else {
    $raw = Get-RawUserPath $PathKey
    $joined = if ([string]::IsNullOrWhiteSpace($raw)) { $target } else { $raw.TrimEnd(';') + ';' + $target }
    Set-RawUserPath $PathKey $joined
    Write-Host "已把 $target 追加到用户 PATH（保持 REG_EXPAND_SZ；新开的 shell 生效）"
}

if (-not $SkipInit) {
    & $installed init
    if ($LASTEXITCODE -ne 0) { throw "assets init 失败（退出码 $LASTEXITCODE）" }
}

# GUI 与 CLI 同属一个 cargo workspace，一次 cargo build --release 会一起产出。
if (-not $SkipGui) {
    $guiBuilt = Join-Path $repo 'target\release\assets-gui.exe'
    if (-not (Test-Path -LiteralPath $guiBuilt)) { throw "没找到 GUI 构建产物：$guiBuilt（别加 -SkipBuild，或者用 -SkipGui 跳过 GUI）" }
    Copy-Item -LiteralPath $guiBuilt -Destination $guiInstalled -Force
    Write-Host "已放入 $guiInstalled"

    foreach ($shortcut in (Get-ShortcutPaths)) {
        $dir = Split-Path -Parent $shortcut
        if (-not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
        New-GuiShortcut $shortcut $guiInstalled
        Write-Host "已创建快捷方式 $shortcut"
    }
}

if ($SkipGui) {
    Write-Host '完成：新开一个 shell 后直接敲 assets 即可。'
} else {
    Write-Host '完成：新开一个 shell 后直接敲 assets 即可；图形界面在桌面与开始菜单的「assets 资产台账」。'
}
