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
public delegate bool EnumProc(IntPtr hwnd, IntPtr lParam);
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
// 这个进程里面积最大的可见窗口（按标题找不可靠：标题是中文，而且以后可能改）
public static IntPtr MainWindowOf(uint pid) {
  IntPtr best = IntPtr.Zero; long bestArea = 0;
  EnumWindows(delegate (IntPtr h, IntPtr l) {
    uint p; GetWindowThreadProcessId(h, out p);
    if (p == pid && IsWindowVisible(h)) {
      RECT r; GetWindowRect(h, out r);
      long a = (long)(r.Right - r.Left) * (r.Bottom - r.Top);
      if (a > bestArea) { bestArea = a; best = h; }
    }
    return true;
  }, IntPtr.Zero);
  return best;
}
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
[DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
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
  $h = [FloatBall.Desk]::MainWindowOf([uint32]$script:appPid)
  if ($h -eq [IntPtr]::Zero) { return $null }
  $r = New-Object FloatBall.Desk+RECT
  [void][FloatBall.Desk]::GetWindowRect($h, [ref]$r)
  return $r
}
function Say([string]$label) {
  $r = WinRect
  if ($r) { Write-Host "$label：窗口 x=$($r.Left) y=$($r.Top) 宽=$($r.Right - $r.Left) 高=$($r.Bottom - $r.Top)" } else { Write-Host "$label：没找到窗口" }
}
function DoubleClick([int]$x, [int]$y) {
  [void][FloatBall.Desk]::SetCursorPos($x, $y)
  Start-Sleep -Milliseconds 300
  for ($k = 0; $k -lt 2; $k++) {
    [FloatBall.Desk]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 30
    [FloatBall.Desk]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 60
  }
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

# 划词默认关：截图要看划词工具栏，先在配置里打开（只改这台云端构建机上的配置）
$cfgDir = Join-Path $env:APPDATA 'com.floatball.desktop'
New-Item -ItemType Directory -Force $cfgDir | Out-Null
$cfgFile = Join-Path $cfgDir 'config.json'
$cfg = if (Test-Path $cfgFile) { Get-Content $cfgFile -Raw | ConvertFrom-Json } else { [pscustomobject]@{} }
$cfg | Add-Member -Force -NotePropertyName sel -NotePropertyValue ([pscustomobject]@{ on = $true; mode = 'instant' })
$cfg | ConvertTo-Json -Depth 20 | Set-Content -Encoding utf8 $cfgFile
Write-Host "配置：$cfgFile 打开划词"

# 把桌面上别的窗口都最小化，免得挡住小球后面的壁纸
(New-Object -ComObject Shell.Application).MinimizeAll()
Start-Sleep -Seconds 1

# 程序自己的输出（含界面里的报错 [前端]）一起放进截图产物，方便查问题
$p = Start-Process -FilePath $Exe -PassThru -RedirectStandardError (Join-Path $Out 'floatball-stderr.txt') -RedirectStandardOutput (Join-Path $Out 'floatball-stdout.txt')
$script:appPid = $p.Id
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

  # 设置是单独的窗口：再启动一次并带上 --settings，已经在运行的那个会把设置窗口打开
  Start-Process -FilePath $Exe -ArgumentList '--settings'
  Start-Sleep -Seconds 8
  Say '打开设置后（面积最大的窗口）'
  Shot '08_设置窗口'

  # 设置的 7 页：左边一列按钮，从上往下每 36 像素一个（100% 缩放）
  $sr = WinRect
  if ($sr) {
    $tabs = '模型', 'Agents', '权限', '外观', '快捷键', '划词', '语音'
    for ($i = 0; $i -lt $tabs.Count; $i++) {
      Click ($sr.Left + 48) ($sr.Top + 73 + 36 * $i)
      Start-Sleep -Milliseconds 900
      Shot ('12_设置_{0}_{1}' -f ($i + 1), $tabs[$i])
    }
  }
  if ($sr) { Click ($sr.Right - 41) ($sr.Top + 31); Start-Sleep -Milliseconds 800 }   # 点右上角关掉设置

  # 语音：按住右 Alt 说话（构建机没有麦克风，看的是没麦克风/授权时怎么提示）
  [void][FloatBall.Desk]::SetCursorPos(200, 200)
  [FloatBall.Desk]::keybd_event(0xA5, 0, 0x0001, [UIntPtr]::Zero)          # 右 Alt 按下
  Start-Sleep -Milliseconds 1500
  Shot '09_按住右Alt'
  [FloatBall.Desk]::keybd_event(0xA5, 0, 0x0003, [UIntPtr]::Zero)          # 右 Alt 松开
  Start-Sleep -Milliseconds 1500
  Shot '10_松开右Alt'
  Start-Sleep -Seconds 6   # 等提示消失

  # 划词：在记事本里双击选中一个词，看工具栏出现在哪
  $txt = Join-Path $env:RUNNER_TEMP '划词测试.txt'
  Set-Content -Encoding utf8 $txt "FloatBall selection test`r`n悬浮球划词测试：双击选中一个词"
  $np = Start-Process notepad.exe -ArgumentList "`"$txt`"" -PassThru
  Start-Sleep -Seconds 3
  $nh = (Get-Process -Id $np.Id).MainWindowHandle
  if ($nh -eq [IntPtr]::Zero) { $nh = [FloatBall.Desk]::MainWindowOf([uint32]$np.Id) }
  if ($nh -ne [IntPtr]::Zero) {
    [void][FloatBall.Desk]::SetForegroundWindow($nh)
    $nr = New-Object FloatBall.Desk+RECT
    [void][FloatBall.Desk]::GetWindowRect($nh, [ref]$nr)
    Write-Host "记事本：x=$($nr.Left) y=$($nr.Top) 宽=$($nr.Right - $nr.Left) 高=$($nr.Bottom - $nr.Top)"
    # 第一行的「FloatBall」：标题栏和菜单下面第一行（100% 缩放，上一次截图量出来在窗口顶下 60 像素）
    DoubleClick ($nr.Left + 40) ($nr.Top + 62)
    Start-Sleep -Milliseconds 1200
    Shot '11_划词工具栏'
  } else { Write-Host '记事本窗口没找到' }
  Stop-Process -Id $np.Id -Force -ErrorAction SilentlyContinue
  Start-Sleep -Seconds 1

  # 划词之后小球还能不能正常点开（看主线程有没有卡住）
  $r = WinRect
  if ($r) { Click ([int](($r.Left + $r.Right) / 2)) ([int](($r.Top + $r.Bottom) / 2)); Start-Sleep -Milliseconds 1500; Say '划词后点小球'; Shot '13_划词后点小球' }
} finally {
  if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
  Get-Process -Name floatball -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
}
Write-Host "截图在 $Out，共 $((Get-ChildItem $Out -Filter *.png).Count) 张"
