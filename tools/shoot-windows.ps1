# 在真实的 Windows 桌面上打开悬浮球，点开小球，连续截屏。
# 界面自动测试（tools/test_ui.py）是在浏览器里跑的，看不到透明窗口、毛玻璃和窗口缩放时的跳动，这个脚本补上这一块。
# 只截图、不做判断：截图由人来看。云端每次构建都跑，截图在构建产物「真机截图-windows」里。
# 用法：pwsh -File tools\shoot-windows.ps1 -Exe <floatball.exe> -Out <输出目录> [-Wallpaper <图片>]
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [Parameter(Mandatory = $true)][string]$Out,
  [string]$Wallpaper
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type -Namespace FloatBall -Name Desk -MemberDefinition @'
[StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowW(string cls, string title);
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern bool SystemParametersInfoW(uint action, uint param, string value, uint flags);
'@
[void][FloatBall.Desk]::SetProcessDPIAware()
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
Write-Host "屏幕 $($screen.Width)x$($screen.Height)"

function Shot([string]$name) {
  $bmp = New-Object System.Drawing.Bitmap $screen.Width, $screen.Height
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($screen.Left, $screen.Top, 0, 0, $bmp.Size)
  $g.Dispose()
  $bmp.Save((Join-Path $Out "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
}
function WinRect {
  $h = [FloatBall.Desk]::FindWindowW($null, '悬浮球')
  if ($h -eq [IntPtr]::Zero) { return $null }
  $r = New-Object FloatBall.Desk+RECT
  [void][FloatBall.Desk]::GetWindowRect($h, [ref]$r)
  return $r
}
function Say([string]$label) {
  $r = WinRect
  if ($r) { Write-Host "$label：窗口 x=$($r.Left) y=$($r.Top) 宽=$($r.Right - $r.Left) 高=$($r.Bottom - $r.Top)" } else { Write-Host "$label：没找到窗口" }
}
function Click([int]$x, [int]$y) {
  [void][FloatBall.Desk]::SetCursorPos($x, $y)
  Start-Sleep -Milliseconds 120
  [FloatBall.Desk]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)   # 左键按下
  Start-Sleep -Milliseconds 40
  [FloatBall.Desk]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)   # 左键松开
}
# 连续截屏：看展开和收起的过程里有没有跳动
function Burst([string]$prefix, [int]$count) {
  $sw = [Diagnostics.Stopwatch]::StartNew()
  for ($i = 0; $i -lt $count; $i++) { Shot ('{0}_{1:d2}_{2:d4}ms' -f $prefix, $i, $sw.ElapsedMilliseconds) }
}

if ($Wallpaper) {
  # 换一张有内容的壁纸，才看得出玻璃透不透、糊不糊
  $w = (Resolve-Path $Wallpaper).Path
  Write-Host "壁纸：$w 设置结果 $([FloatBall.Desk]::SystemParametersInfoW(0x0014, 0, $w, 3))"
  Start-Sleep -Seconds 2
}

$p = Start-Process -FilePath $Exe -PassThru
try {
  $r = $null
  for ($i = 0; $i -lt 40 -and -not $r; $i++) { Start-Sleep -Milliseconds 500; $r = WinRect }
  if (-not $r) { Shot '00_没有窗口'; throw '等了 20 秒，悬浮球的窗口没有出现' }
  Start-Sleep -Seconds 4
  Say '启动后'
  Shot '01_启动后'

  $r = WinRect
  $cx = [int](($r.Left + $r.Right) / 2); $cy = [int](($r.Top + $r.Bottom) / 2)
  [void][FloatBall.Desk]::SetCursorPos($cx - 200, $cy - 200)
  Start-Sleep -Milliseconds 300
  [void][FloatBall.Desk]::SetCursorPos($cx, $cy)
  Start-Sleep -Milliseconds 700
  Shot '02_鼠标移到小球上'

  Click $cx $cy
  Burst '03_展开' 28
  Start-Sleep -Milliseconds 800
  Say '展开后'
  Shot '04_展开后'

  [System.Windows.Forms.SendKeys]::SendWait('hello')
  Start-Sleep -Milliseconds 600
  Shot '05_输入文字'

  [System.Windows.Forms.SendKeys]::SendWait('{ESC}')
  Burst '06_收起' 16
  Start-Sleep -Milliseconds 800
  Say '收起后'
  Shot '07_收起后'
} finally {
  if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
  Get-Process -Name floatball -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
}
Write-Host "截图在 $Out，共 $((Get-ChildItem $Out -Filter *.png).Count) 张"
