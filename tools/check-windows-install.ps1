# 在 Windows 上真装一遍安装包，检查四件事：
#   1. 数据都写在「安装目录\data」，不往用户目录（AppData）写
#   2. 重装不会删掉用户数据
#   3. 普通卸载删掉程序、快捷方式和开机启动项，保留数据和 API Key
#   4. 带 /CLEAN 卸载（等于勾选「删除应用程序数据」）后什么都不留
# 用法：pwsh -File tools\check-windows-install.ps1 -Installer <安装包> [-InstallDir <目录>]
# 它会删数据，所以在已经装了悬浮球的电脑上会直接拒绝运行。
param(
  [Parameter(Mandatory = $true)][string]$Installer,
  [string]$InstallDir = (Join-Path $env:TEMP '悬浮球-安装测试')
)
$ErrorActionPreference = 'Stop'
$name = '悬浮球'
$id = 'com.floatball.desktop'
$credTarget = "api_key.$id"
$roaming = Join-Path $env:APPDATA $id
$local = Join-Path $env:LOCALAPPDATA $id
$uninstKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$name"
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$approvedKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'
$exe = Join-Path $InstallDir 'floatball.exe'
$uninstaller = Join-Path $InstallDir 'uninstall.exe'
$data = Join-Path $InstallDir 'data'
$marker = Join-Path $data '测试记号.txt'
$script:failed = 0

function Check([bool]$ok, [string]$msg) {
  if ($ok) { Write-Host "PASS $msg" } else { Write-Host "FAIL $msg"; $script:failed++ }
}
function Wait-For([scriptblock]$cond, [int]$seconds) {
  for ($i = 0; $i -lt $seconds * 2; $i++) { if (& $cond) { return $true }; Start-Sleep -Milliseconds 500 }
  return [bool](& $cond)
}
#region 凭据
# 直接调用 Windows 的凭据接口，和程序里 keyring 库的做法一样（1 = 通用凭据，3 = 保存方式）。
# 不用 cmdkey：它的输出格式随系统语言变化，失败时也不说原因
Add-Type -Namespace FloatBall -Name Cred -MemberDefinition @'
[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
public struct CREDENTIAL {
  public uint Flags; public uint Type; public string TargetName; public string Comment;
  public long LastWritten; public uint CredentialBlobSize; public IntPtr CredentialBlob;
  public uint Persist; public uint AttributeCount; public IntPtr Attributes;
  public string TargetAlias; public string UserName;
}
[DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)] public static extern bool CredWriteW(ref CREDENTIAL cred, uint flags);
[DllImport("advapi32.dll", CharSet = CharSet.Unicode)] public static extern bool CredReadW(string target, uint type, uint flags, out IntPtr cred);
[DllImport("advapi32.dll", CharSet = CharSet.Unicode)] public static extern bool CredDeleteW(string target, uint type, uint flags);
[DllImport("advapi32.dll")] public static extern void CredFree(IntPtr cred);
'@
function Test-Cred {
  $p = [IntPtr]::Zero
  if (-not [FloatBall.Cred]::CredReadW($credTarget, 1, 0, [ref]$p)) { return $false }
  [FloatBall.Cred]::CredFree($p)
  return $true
}
function Set-TestCred {
  $bytes = [Text.Encoding]::Unicode.GetBytes('not-a-real-key')
  $c = New-Object 'FloatBall.Cred+CREDENTIAL'
  $c.Type = 1; $c.Persist = 3; $c.TargetName = $credTarget; $c.UserName = 'api_key'
  $c.CredentialBlobSize = $bytes.Length
  $c.CredentialBlob = [Runtime.InteropServices.Marshal]::AllocHGlobal($bytes.Length)
  [Runtime.InteropServices.Marshal]::Copy($bytes, 0, $c.CredentialBlob, $bytes.Length)
  try {
    $ok = [FloatBall.Cred]::CredWriteW([ref]$c, 0)
    $err = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
  }
  finally { [Runtime.InteropServices.Marshal]::FreeHGlobal($c.CredentialBlob) }
  if (-not $ok) { Write-Host "   写入测试凭据失败，Windows 错误码 $err" }
}
function Remove-TestCred { [void][FloatBall.Cred]::CredDeleteW($credTarget, 1, 0) }
#endregion
function Test-RegValue([string]$key, [string]$value) { return $null -ne (Get-ItemProperty -Path $key -Name $value -ErrorAction SilentlyContinue) }
function Find-Shortcuts {
  $dirs = @([Environment]::GetFolderPath('Programs'), [Environment]::GetFolderPath('Desktop'))
  return @(Get-ChildItem -Path $dirs -Recurse -Filter "$name.lnk" -ErrorAction SilentlyContinue)
}
# 检查没过时把现场打印出来，方便看是哪里没清掉
function Show-Leftovers([string]$title) {
  Write-Host "   [$title]"
  foreach ($s in Find-Shortcuts) {
    $target = (New-Object -ComObject WScript.Shell).CreateShortcut($s.FullName).TargetPath
    Write-Host "   快捷方式 $($s.FullName) -> $target"
  }
  if (Test-Path $InstallDir) {
    Get-ChildItem -LiteralPath $InstallDir -Force -Recurse -Depth 2 -ErrorAction SilentlyContinue | Select-Object -First 25 | ForEach-Object { Write-Host "   留下 $($_.FullName)" }
  }
  Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($InstallDir, [StringComparison]::OrdinalIgnoreCase) } | ForEach-Object { Write-Host "   还在运行 $($_.Path)" }
  Get-CimInstance Win32_Process -Filter "Name = 'msedgewebview2.exe'" -ErrorAction SilentlyContinue | Where-Object { $_.CommandLine -like "*$InstallDir*" } | Select-Object -First 3 | ForEach-Object { Write-Host "   还在运行 WebView2 进程 $($_.ProcessId)，数据目录在安装目录里" }
}
function Stop-App { Get-Process -Name 'floatball' -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Seconds 2 }
# /D= 必须是最后一个参数，而且不能加引号
function Install([string[]]$extra) { return (Start-Process -FilePath $Installer -ArgumentList (@('/S') + $extra + "/D=$InstallDir") -Wait -PassThru).ExitCode }
function Uninstall([string[]]$extra) {
  $code = (Start-Process -FilePath $uninstaller -ArgumentList (@('/S') + $extra) -Wait -PassThru).ExitCode
  # 卸载程序会把自己复制到临时目录再运行，所以要等到程序文件真的没了
  [void](Wait-For { -not (Test-Path $exe) } 60)
  return $code
}

if ((Test-Path $uninstKey) -or (Test-Path $roaming) -or (Test-Path $local) -or (Test-Path $InstallDir) -or (Test-Cred)) {
  throw '这台电脑上已经有悬浮球的安装或数据，测试脚本不在这里运行（它会删除数据）'
}
$Installer = (Resolve-Path $Installer).Path

try {
  Write-Host "== 安装到 $InstallDir"
  Write-Host "   （临时目录的原始写法：$env:TEMP）"
  Check ((Install @()) -eq 0) '安装程序正常结束'
  Check (Test-Path $exe) '程序装进了选定的目录'
  Check (Test-Path $uninstaller) '安装目录里有卸载程序'

  Write-Host '== 第一次运行'
  $app = Start-Process -FilePath $exe -PassThru
  Check (Wait-For { Test-Path (Join-Path $data 'history.db') } 60) '历史记录建在 安装目录\data'
  Check (Wait-For { Test-Path (Join-Path $data 'EBWebView') } 60) '界面缓存建在 安装目录\data'
  Start-Sleep -Seconds 3
  Check (-not $app.HasExited) '程序在运行，没有闪退'
  Check (-not (Test-Path $roaming)) '没有往 AppData\Roaming 写东西'
  Check (-not (Test-Path $local)) '没有往 AppData\Local 写东西'
  Stop-App

  # 放一个记号文件、一条假的 API Key 和一份开机启动记录，看重装和卸载分别怎么处理它们
  Set-Content -Path $marker -Value 'keep me'
  Set-TestCred
  Check (Test-Cred) '放入一条测试用的 API Key 凭据'
  # 只在注册表项不存在时才新建：对已有的项用 New-Item -Force 会清空里面原有的内容
  foreach ($k in @($runKey, $approvedKey)) { if (-not (Test-Path $k)) { New-Item -Path $k -Force | Out-Null } }
  New-ItemProperty -Path $runKey -Name $name -Value "`"$exe`"" -Force | Out-Null
  New-ItemProperty -Path $approvedKey -Name $name -PropertyType Binary -Value ([byte[]](2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)) -Force | Out-Null

  Write-Host '== 重装（升级时安装器的做法）'
  Check ((Install @('/UPDATE')) -eq 0) '重装正常结束'
  Stop-App
  Check (Test-Path $marker) '重装后用户数据还在'
  Check (Test-Path (Join-Path $data 'history.db')) '重装后历史记录还在'
  Check (Test-Cred) '重装后 API Key 还在'
  Check (Test-RegValue $runKey $name) '重装后开机启动项还在'

  Write-Host '== 普通卸载（不勾选「删除应用程序数据」）'
  Check ((Uninstall @()) -eq 0) '卸载程序正常结束'
  Check (-not (Test-Path $exe)) '程序文件删掉了'
  Check (Wait-For { -not (Test-Path $uninstaller) } 30) '卸载程序自己也删掉了'
  Check (Test-Path $marker) '不勾选时用户数据保留'
  Check (Test-Cred) '不勾选时 API Key 保留'
  Check (-not (Test-RegValue $runKey $name)) '开机启动项删掉了'
  Check (-not (Test-RegValue $approvedKey $name)) '开机启动的启用状态记录删掉了'
  Check (-not (Test-Path $uninstKey)) '「应用」列表里的记录删掉了'
  Check ((Find-Shortcuts).Count -eq 0) '开始菜单和桌面的快捷方式删掉了'
  if ((Find-Shortcuts).Count) { Show-Leftovers '普通卸载之后' }

  Write-Host '== 再装一次，然后带 /CLEAN 卸载（等于勾选「删除应用程序数据」）'
  Check ((Install @()) -eq 0) '再次安装正常结束'
  Check (Test-Path $marker) '装回同一个目录后，之前的数据还在'
  # 小球平时一直开着，所以这次不先关程序，让卸载程序自己去关
  $app = Start-Process -FilePath $exe -PassThru
  Start-Sleep -Seconds 8
  Check (-not $app.HasExited) '卸载前程序在运行'
  Check ((Uninstall @('/CLEAN')) -eq 0) '程序开着时卸载，卸载程序正常结束'
  Check ($null -eq (Get-Process -Name 'floatball' -ErrorAction SilentlyContinue)) '卸载程序把正在运行的小球关掉了'
  Check (Wait-For { -not (Test-Path $InstallDir) } 60) '安装目录整个没有了（包括 data）'
  Check (-not (Test-Cred)) '凭据管理器里的 API Key 没有了'
  Check (-not (Test-Path $uninstKey)) '「应用」列表里的记录没有了'
  Check (-not (Test-Path 'HKCU:\Software\floatball')) '注册表里的安装位置记录没有了'
  Check (-not (Test-Path $roaming)) 'AppData\Roaming 里没有残留'
  Check (-not (Test-Path $local)) 'AppData\Local 里没有残留'
  Check ((Find-Shortcuts).Count -eq 0) '开始菜单和桌面的快捷方式没有了'
  if ((Find-Shortcuts).Count -or (Test-Path $InstallDir)) { Show-Leftovers '带 /CLEAN 卸载之后' }
}
finally {
  # 测试中途失败也不留下假的凭据和开机启动记录
  Stop-App
  Remove-TestCred
  Remove-ItemProperty -Path $runKey -Name $name -ErrorAction SilentlyContinue
  Remove-ItemProperty -Path $approvedKey -Name $name -ErrorAction SilentlyContinue
}
if ($script:failed) { Write-Host "`n$($script:failed) 项失败"; exit 1 }
Write-Host "`n全部通过"
