# wotb-agent 打包分发脚本（Windows）
#
# 用法（在仓库根目录）：
#   powershell -ExecutionPolicy Bypass -File scripts\package.ps1 -All         # 一键重新打包全部三种产物（改完代码用这个）
#   powershell -ExecutionPolicy Bypass -File scripts\package.ps1              # 仅轻量便携 zip（模型联网懒下载）
#   powershell -ExecutionPolicy Bypass -File scripts\package.ps1 -Full        # 仅全量目录：附带 glb_cache 完全离线（~1.9GB）
#   powershell -ExecutionPolicy Bypass -File scripts\package.ps1 -Full -Zip   # 全量目录 + 顺手压一份轻量 zip
#   powershell -ExecutionPolicy Bypass -File scripts\package.ps1 -Bundled     # 仅单文件版 wotb-agent-standalone-win64.exe
#   -SkipBuild   跳过 cargo build，直接用现有 exe（只改了数据/文档时用；改了 Rust 代码不要加）
#
# 产物（默认在 dist/）：
#   wotb-agent-portable-win64.zip          轻量便携包
#   wotb-agent-portable-win64\             全量便携目录（-Full/-All）
#   wotb-agent-standalone-win64.exe        单文件版（-Bundled/-All，首次运行自释放）
#
# 注意：绝不会打包真实 config.toml（含密钥），只带 config.toml.example 模板。

param(
    [switch]$Full,
    [switch]$Zip,
    [switch]$Bundled,
    [switch]$All,
    [switch]$SkipBuild,
    [string]$OutDir = "dist"
)

# -All = 一键产出全部三种产物（轻量 zip + 全量目录 + 单文件 exe）
if ($All) { $Full = $true; $Bundled = $true }

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$exe = "target/release/wotb-agent.exe"

# ---- 1. 构建 ----
if (-not $SkipBuild) {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build --release 失败" }
}
if (-not (Test-Path $exe)) { throw "未找到 $exe，请先执行 cargo build --release" }

# 纯 -Bundled 模式只产出单文件 exe，不动已打包好的便携目录/全量目录
$doPortable = (-not $Bundled) -or $Full

# ---- 2. 组装便携目录 ----
$stage = Join-Path $OutDir "wotb-agent-portable-win64"
if ($doPortable) {
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Path $stage -Force | Out-Null
}

# robocopy 返回码 0-7 均为成功
function Copy-Tree($src, $dst, $excludeDirs = @()) {
    if (-not (Test-Path $src)) { throw "缺少目录: $src" }
    $args = @($src, $dst, "/E", "/NFL", "/NDL", "/NJH", "/NJS", "/NP")
    foreach ($d in $excludeDirs) { $args += @("/XD", $d) }
    & robocopy @args | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "robocopy 复制 $src 失败（exit=$LASTEXITCODE）" }
}

if ($doPortable) {
Copy-Item $exe (Join-Path $stage "wotb-agent.exe")
Copy-Item config.toml.example $stage/
Copy-Item README.md $stage/
Copy-Tree "web/vendor"    (Join-Path $stage "web/vendor")
Copy-Tree "data"          (Join-Path $stage "data")          @("sessions")   # sessions 为用户运行时会话，不分发
Copy-Tree "replay_samples" (Join-Path $stage "replay_samples")

# 启动器：cmd 用纯 ASCII，避免编码问题
@"
@echo off
cd /d "%~dp0"
wotb-agent.exe web
pause
"@ | Set-Content -Path (Join-Path $stage "start-web.bat") -Encoding Ascii

# 使用说明：UTF-8 BOM，老版记事本也能正常显示中文
$readme = @"
WoTB Blitz Tactics Agent - 便携版使用说明
==========================================

一、启动
  双击 start-web.bat（或命令行运行: wotb-agent.exe web）
  启动后会自动打开浏览器访问 http://127.0.0.1:18999

二、首次配置
  1. 把 config.toml.example 复制一份，重命名为 config.toml
     （首次运行会自动生成，可直接改）
  2. WG API 的 application_id 已自带可用的公开 key，无需修改
  3. LLM 的 api_key 需要填入你自己的 key（也可启动后在
     网页 Settings - Model Config 里在线填写并保存）

三、3D 模型说明
  轻量包：首次查看某坦克的 3D 装甲/真实车模时会自动联网下载到
  glb_cache/ 目录，之后离线可用。
  全量包：已内置全部模型，无需联网。

四、常用命令（命令行运行 wotb-agent.exe <命令>）
  web            Web 图形界面（推荐）
  scan <目录>    批量扫描回放目录生成报告
  single <文件>  解析单个回放
  playback <文件> 全场实时回放
  update-data    游戏版本更新后一键刷新数据
  fetch-models   全量预下载坦克模型（轻量包转离线）
  --help         查看全部命令

五、数据与目录
  全部数据/缓存均在解压目录内（data/ glb_cache/ tank_images/），
  整个文件夹可随意移动；删除 data/ 可重置为内置初始数据。
"@
[System.IO.File]::WriteAllText((Join-Path $stage "使用说明.txt"), $readme, (New-Object System.Text.UTF8Encoding($true)))

# ---- 3. 压缩轻量 zip（必须在附加大体积缓存之前压，保证 zip 始终轻量） ----
$zipPath = Join-Path $OutDir "wotb-agent-portable-win64.zip"
$needZip = (-not $Full) -or $Zip -or $All
if ($needZip) {
    if (Test-Path $zipPath) { Remove-Item -Force $zipPath }
    Write-Host "压缩 $stage -> $zipPath ..."
    Compress-Archive -Path "$stage/*" -DestinationPath $zipPath -CompressionLevel Optimal
}

# ---- 4. 全量附加（完全离线；robocopy 增量复制，缓存没变时很快） ----
if ($Full) {
    if (Test-Path "glb_cache") {
        Write-Host "复制 glb_cache/（约 1.8GB，增量复制，请稍候）..."
        Copy-Tree "glb_cache" (Join-Path $stage "glb_cache")
    } else {
        Write-Warning "未找到 glb_cache/，跳过。可先运行: wotb-agent.exe fetch-models"
    }
    if (Test-Path "tank_images") {
        Copy-Tree "tank_images" (Join-Path $stage "tank_images")
    }
}
} # end $doPortable

# ---- 5. 单文件版（bundle feature，首次运行自释放） ----
if ($Bundled) {
    Write-Host "构建单文件版（--features bundle）..."
    cargo build --release --features bundle
    if ($LASTEXITCODE -ne 0) { throw "cargo build --features bundle 失败" }
    Copy-Item $exe (Join-Path $OutDir "wotb-agent-standalone-win64.exe") -Force
    Write-Host "注意：此步之后 target/release/wotb-agent.exe 为 bundle 版，"
    Write-Host "      日常开发请重新 cargo build --release 还原。"
}

# ---- 6. 汇总 ----
function Size-Of($path) {
    if (Test-Path $path -PathType Container) {
        "{0:N1} MB" -f ((Get-ChildItem $path -Recurse -File | Measure-Object Length -Sum).Sum / 1MB)
    } else {
        "{0:N1} MB" -f ((Get-Item $path).Length / 1MB)
    }
}
Write-Host ""
Write-Host "===== 打包完成 ====="
if ($doPortable) { Write-Host ("  便携目录     {0}  ({1})" -f $stage, (Size-Of $stage)) }
if ($needZip)   { Write-Host ("  轻量 zip     {0}  ({1})" -f $zipPath, (Size-Of $zipPath)) }
if ($Full)      { Write-Host "  全量包       $stage（含 glb_cache 完全离线）" }
if ($Bundled)   { Write-Host ("  单文件版     {0}  ({1})" -f (Join-Path $OutDir "wotb-agent-standalone-win64.exe"), (Size-Of (Join-Path $OutDir "wotb-agent-standalone-win64.exe"))) }
