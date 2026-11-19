//! 交给终端（B4）：只读 PATH 找本机装了的编程 CLI 和终端；把交接说明写成文件，
//! 在选好的终端里、项目文件夹下打开那个 CLI，只传一句「读这个文件按里面做」。
//! 命令行只有这一句话：整段对话会超过 Windows 命令行长度，引号也容易出错。

use serde::Serialize;
use std::path::{Path, PathBuf};

/// 一个编程 CLI。`arg`：怎么在打开时就带上第一句话（{p} = 那句话）；None = 只能打开后粘贴
pub struct Tool {
    pub id: &'static str,
    pub name: &'static str,
    /// 依次找这些命令名（Cursor 改过名）
    pub bins: &'static [&'static str],
    pub arg: Option<&'static [&'static str]>,
}

// 带参数的写法都按各家官方文档核实过（2026-11）；没核实的不猜，走粘贴，免得参数错了工具直接报错退出
pub const TOOLS: &[Tool] = &[
    Tool { id: "claude", name: "Claude Code", bins: &["claude"], arg: Some(&["{p}"]) },
    Tool { id: "codex", name: "Codex", bins: &["codex"], arg: Some(&["{p}"]) },
    Tool { id: "gemini", name: "Gemini CLI", bins: &["gemini"], arg: Some(&["-i", "{p}"]) },
    Tool { id: "qwen", name: "Qwen Code", bins: &["qwen"], arg: Some(&["-i", "{p}"]) },
    Tool { id: "cursor", name: "Cursor CLI", bins: &["agent", "cursor-agent"], arg: Some(&["{p}"]) },
    Tool { id: "goose", name: "Goose", bins: &["goose"], arg: Some(&["run", "-s", "-i", "{f}"]) },
    Tool { id: "copilot", name: "GitHub Copilot CLI", bins: &["copilot"], arg: None },
    Tool { id: "kimi", name: "Kimi Code CLI", bins: &["kimi"], arg: None },
    Tool { id: "aider", name: "Aider", bins: &["aider"], arg: None },
    Tool { id: "opencode", name: "OpenCode", bins: &["opencode"], arg: None },
    Tool { id: "amp", name: "Amp", bins: &["amp"], arg: None },
];

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Found {
    pub id: String,
    pub name: String,
    /// 实际找到的命令名
    pub bin: String,
    /// 打开时就能带上交接说明；false = 打开后要粘贴一次
    pub direct: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Term {
    pub id: String,
    pub name: String,
}

#[derive(Serialize)]
pub struct Scan {
    pub tools: Vec<Found>,
    pub terms: Vec<Term>,
}

/// 在 PATH 里找一个命令（只看文件在不在，不运行它）
pub fn which(bin: &str, path: &str, exts: &[&str]) -> Option<PathBuf> {
    std::env::split_paths(path).filter(|d| !d.as_os_str().is_empty()).find_map(|d| {
        exts.iter().map(|e| d.join(format!("{bin}{e}"))).find(|p| p.is_file())
    })
}

fn exts() -> Vec<String> {
    if cfg!(windows) {
        // PATHEXT 里常见的那几种；npm 装的是 .cmd，有的是 .ps1
        let mut v: Vec<String> = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into()).split(';').map(|e| e.to_ascii_lowercase()).filter(|e| !e.is_empty()).collect();
        v.push(".ps1".into());
        v
    } else {
        vec![String::new()]
    }
}

pub fn scan_tools(path: &str, exts: &[&str]) -> Vec<Found> {
    TOOLS.iter().filter_map(|t| {
        let bin = t.bins.iter().find(|b| which(b, path, exts).is_some())?;
        Some(Found { id: t.id.into(), name: t.name.into(), bin: (*bin).into(), direct: t.arg.is_some() })
    }).collect()
}

pub fn scan() -> Scan {
    let path = std::env::var("PATH").unwrap_or_default();
    let ex = exts();
    let ex: Vec<&str> = ex.iter().map(String::as_str).collect();
    let mut tools = scan_tools(&path, &ex);
    // 能直接带上说明的排前面（列表本来就按这个顺序，这里保底）
    tools.sort_by_key(|f| !f.direct);
    Scan { tools, terms: terms(&path, &ex) }
}

fn terms(path: &str, ex: &[&str]) -> Vec<Term> {
    let t = |id: &str, name: &str| Term { id: id.into(), name: name.into() };
    let mut v = vec![];
    if cfg!(windows) {
        if which("wt", path, ex).is_some() { v.push(t("wt", "Windows Terminal")); }
        if which("pwsh", path, ex).is_some() { v.push(t("pwsh", "PowerShell 7")); }
        v.push(t("powershell", "Windows PowerShell"));
    } else if cfg!(target_os = "macos") {
        v.push(t("terminal", "终端"));
        if Path::new("/Applications/iTerm.app").is_dir() { v.push(t("iterm", "iTerm")); }
    }
    v
}

/// 交给终端时那一句话
pub fn prompt_for(file: &Path) -> String {
    format!("读 {} ，按里面的交接说明继续做。", file.display())
}

/// 写交接说明：开关开着写到 项目/.floatball/handoff.md（旁边放一个只有「*」的 .gitignore，Git 看不到这个文件夹），
/// 关着写到 data/handoff/（下次交接或退出时删）。返回写好的文件
pub fn write(md: &str, dir: &Path, in_project: bool, data_dir: &Path) -> Result<PathBuf, String> {
    let f = if in_project {
        let d = dir.join(".floatball");
        std::fs::create_dir_all(&d).map_err(|e| format!("写不进项目文件夹：{e}"))?;
        let gi = d.join(".gitignore");
        if !gi.exists() {
            std::fs::write(&gi, "# 悬浮球的交接说明，不提交\n*\n").map_err(|e| e.to_string())?;
        }
        d.join("handoff.md")
    } else {
        let d = data_dir.join("handoff");
        clean(data_dir);
        std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        d.join(format!("handoff-{}.md", std::process::id()))
    };
    std::fs::write(&f, md).map_err(|e| format!("交接说明写不进去：{e}"))?;
    Ok(f)
}

/// 删掉 data/handoff 里的旧说明（项目里的那份不动）
pub fn clean(data_dir: &Path) {
    let _ = std::fs::remove_dir_all(data_dir.join("handoff"));
}

/// 这个工具在打开时要带的参数（{p} 换成那句话，{f} 换成文件）
pub fn args_for(id: &str, prompt: &str, file: &Path) -> Vec<String> {
    let Some(t) = TOOLS.iter().find(|t| t.id == id) else { return vec![] };
    t.arg.unwrap_or(&[]).iter().map(|a| a.replace("{p}", prompt).replace("{f}", &file.display().to_string())).collect()
}

/// 自定义命令：{file} 换成交接说明文件的完整路径；没写 {file} 就只运行命令
pub fn custom_line(cmd: &str, file: &Path) -> String {
    cmd.trim().replace("{file}", &quote_ps(&file.display().to_string()))
}

/// PowerShell 单引号字符串（里面的 ' 写两遍）
pub fn quote_ps(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// POSIX shell 单引号字符串
pub fn quote_sh(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// AppleScript 字符串
fn quote_as(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// PowerShell -EncodedCommand 要的：UTF-16LE 再 Base64（不为这几行加依赖）
pub fn encode_ps(s: &str) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let b: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut o = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for (i, sh) in [18, 12, 6, 0].into_iter().enumerate() {
            o.push(if i <= c.len() { T[(n >> sh & 63) as usize] as char } else { '=' });
        }
    }
    o
}

/// 在终端里、dir 下运行 line（一整行 shell 命令）
pub fn open(term: &str, dir: &Path, line: &str) -> Result<(), String> {
    if !dir.is_dir() { return Err("项目文件夹不存在".into()); }
    #[cfg(windows)]
    {
        use std::process::Command;
        // 在 PowerShell 里先进文件夹再运行；-NoExit 让工具退出后窗口还在。
        // 用 -EncodedCommand：wt 把参数里的 ; 当成自己的分隔符，cmd start 会重新拆引号，Base64 里两样都没有
        let ps = format!("Set-Location -LiteralPath {}; {}", quote_ps(&dir.display().to_string()), line);
        let enc = encode_ps(&ps);
        let shell = if term == "pwsh" { "pwsh" } else if term == "wt" && which("pwsh", &std::env::var("PATH").unwrap_or_default(), &[".exe"]).is_some() { "pwsh" } else { "powershell" };
        let r = if term == "wt" {
            Command::new("wt").args(["-d"]).arg(dir).args(["--", shell, "-NoExit", "-NoLogo", "-EncodedCommand", &enc]).spawn()
        } else {
            // 用 cmd 的 start 开一个新的控制台窗口
            Command::new("cmd").args(["/C", "start", "", shell, "-NoExit", "-NoLogo", "-EncodedCommand", &enc]).current_dir(dir).spawn()
        };
        r.map(|_| ()).map_err(|e| format!("终端没打开：{e}"))
    }
    #[cfg(target_os = "macos")]
    {
        let sh = format!("cd {} && {}", quote_sh(&dir.display().to_string()), line);
        let script = if term == "iterm" {
            format!("tell application \"iTerm\"\nactivate\ncreate window with default profile command \"/bin/zsh -l\"\ntell current session of current window to write text {}\nend tell", quote_as(&sh))
        } else {
            format!("tell application \"Terminal\"\nactivate\ndo script {}\nend tell", quote_as(&sh))
        };
        std::process::Command::new("osascript").args(["-e", &script]).spawn().map(|_| ()).map_err(|e| format!("终端没打开：{e}"))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (term, line);
        Err("这个系统上还不支持".into())
    }
}

/// 选项目文件夹：用系统自带的对话框（Windows 用 PowerShell 的 FolderBrowserDialog，mac 用 osascript），不为它多加依赖。
/// 取消返回 None
pub fn pick_folder(start: &str) -> Result<Option<String>, String> {
    #[cfg(windows)]
    let out = {
        use std::os::windows::process::CommandExt;
        // 起始文件夹走环境变量，不拼进脚本
        let ps = "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; $d.Description = '选择项目文件夹'; $d.UseDescriptionForTitle = $true; $d.ShowNewFolderButton = $true; if ($env:FB_START -and (Test-Path -LiteralPath $env:FB_START)) { $d.SelectedPath = $env:FB_START }; $f = New-Object System.Windows.Forms.Form; $f.TopMost = $true; if ($d.ShowDialog($f) -eq 'OK') { [Console]::OutputEncoding = [Text.Encoding]::UTF8; [Console]::Write($d.SelectedPath) }";
        std::process::Command::new("powershell").args(["-NoProfile", "-STA", "-Command", ps]).env("FB_START", start).creation_flags(0x0800_0000).output()
    };
    #[cfg(target_os = "macos")]
    let out = {
        let loc = if Path::new(start).is_dir() { format!(" default location (POSIX file {})", quote_as(start)) } else { String::new() };
        std::process::Command::new("osascript").args(["-e", &format!("POSIX path of (choose folder with prompt \"选择项目文件夹\"{loc})")]).output()
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let out: std::io::Result<std::process::Output> = { let _ = start; Err(std::io::Error::other("这个系统上还不支持")) };
    let o = out.map_err(|e| format!("打不开选文件夹的窗口：{e}"))?;
    let s = String::from_utf8_lossy(&o.stdout).trim().trim_end_matches('/').to_string();
    Ok((!s.is_empty()).then_some(s))
}

/// 拼出终端里要运行的那一行
pub fn line_for(bin: &str, args: &[String]) -> String {
    let q = if cfg!(windows) { quote_ps } else { quote_sh };
    // PowerShell 里带引号的命令名要用 & 调用
    let head = if cfg!(windows) { format!("& {}", q(bin)) } else { q(bin) };
    std::iter::once(head).chain(args.iter().map(|a| q(a))).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fb-handoff-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn finds_only_installed_tools_in_path() {
        let a = tmp("a");
        let b = tmp("b");
        std::fs::write(a.join("claude.cmd"), "").unwrap();
        std::fs::write(b.join("cursor-agent.exe"), "").unwrap();
        std::fs::write(b.join("kimi"), "").unwrap();
        std::fs::create_dir_all(b.join("codex.exe")).unwrap(); // 同名文件夹不算
        let path = std::env::join_paths([&a, &b]).unwrap().into_string().unwrap();
        let got = scan_tools(&path, &[".exe", ".cmd"]);
        let ids: Vec<(&str, &str, bool)> = got.iter().map(|f| (f.id.as_str(), f.bin.as_str(), f.direct)).collect();
        assert_eq!(ids, vec![("claude", "claude", true), ("cursor", "cursor-agent", true)]);
        let got = scan_tools(&path, &["", ".exe", ".cmd"]);
        assert!(got.iter().any(|f| f.id == "kimi" && !f.direct), "没有扩展名的（mac 上）也找得到，Kimi 要粘贴");
    }

    #[test]
    fn verified_tools_pass_the_prompt_others_do_not() {
        let f = Path::new("/p/.floatball/handoff.md");
        let p = prompt_for(f);
        assert_eq!(args_for("claude", &p, f), vec![p.clone()]);
        assert_eq!(args_for("gemini", &p, f), vec!["-i".to_string(), p.clone()]);
        assert_eq!(args_for("goose", &p, f), vec!["run", "-s", "-i", "/p/.floatball/handoff.md"]);
        for id in ["copilot", "kimi", "aider", "opencode", "amp"] {
            assert!(args_for(id, &p, f).is_empty(), "{id} 没核实过，不带参数");
        }
        assert_eq!(TOOLS.iter().filter(|t| t.arg.is_some()).count(), 6);
        assert_eq!(TOOLS.len(), 11);
    }

    #[test]
    fn quoting_survives_spaces_and_quotes() {
        // -EncodedCommand：UTF-16LE + Base64（参考值用 Python base64 算的）
        assert_eq!(encode_ps("a"), "YQA=");
        assert_eq!(encode_ps("ab"), "YQBiAA==");
        assert_eq!(encode_ps("abc"), "YQBiAGMA");
        assert_eq!(encode_ps("读 x；y"), "+4sgAHgAG/95AA==");
        assert_eq!(quote_ps("C:\\Users\\J's dir"), "'C:\\Users\\J''s dir'");
        assert_eq!(quote_sh("/Users/j/it's"), "'/Users/j/it'\\''s'");
        assert_eq!(quote_as("a \"b\" \\c"), "\"a \\\"b\\\" \\\\c\"");
        assert_eq!(custom_line("  my-agent --read {file} ", Path::new("C:\\a b\\h.md")), "my-agent --read 'C:\\a b\\h.md'");
    }

    #[test]
    fn project_copy_is_hidden_from_git_and_data_copy_is_cleaned() {
        let proj = tmp("proj");
        let data = tmp("data");
        let f = write("# 交接", &proj, true, &data).unwrap();
        assert_eq!(f, proj.join(".floatball").join("handoff.md"));
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "# 交接");
        let gi = std::fs::read_to_string(proj.join(".floatball").join(".gitignore")).unwrap();
        assert!(gi.lines().any(|l| l == "*"), "整个 .floatball 文件夹 Git 都看不到");
        // 再写一次：覆盖 handoff.md，.gitignore 不重写
        std::fs::write(proj.join(".floatball").join(".gitignore"), "*\n# mine\n").unwrap();
        write("# 新的", &proj, true, &data).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "# 新的");
        assert!(std::fs::read_to_string(proj.join(".floatball").join(".gitignore")).unwrap().contains("# mine"));
        // 不存进项目：写到 data/handoff，下次交接先清掉旧的
        let d1 = write("一", &proj, false, &data).unwrap();
        assert!(d1.starts_with(data.join("handoff")));
        std::fs::write(data.join("handoff").join("old.md"), "旧").unwrap();
        write("二", &proj, false, &data).unwrap();
        assert!(!data.join("handoff").join("old.md").exists());
        clean(&data);
        assert!(!data.join("handoff").exists());
    }
}
