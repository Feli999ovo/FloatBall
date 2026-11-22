use super::*;

pub struct RealWorld {
    pub remember: Box<dyn Fn(Allow) + Send + Sync>,
    pub memory: Option<Box<dyn MemoryStore>>,
}

/// 网页 → 纯文字：去掉 script/style 和标签，合并空白，最多 8000 字
pub fn html_text(html: &str) -> String {
    let mut s = html.to_string();
    for tag in ["script", "style", "noscript", "svg"] {
        while let Some(i) = s.to_ascii_lowercase().find(&format!("<{tag}")) {
            let lower = s.to_ascii_lowercase();
            let end = lower[i..]
                .find(&format!("</{tag}>"))
                .map(|j| i + j + tag.len() + 3)
                .unwrap_or(s.len());
            s.replace_range(i..end, " ");
        }
    }
    let mut out = String::new();
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let text = out.split_whitespace().collect::<Vec<_>>().join(" ");
    short(&text, 8000)
}

/// DuckDuckGo 的 HTML 版结果页 → 「标题 / 链接 / 摘要」
pub fn parse_ddg(html: &str) -> Vec<(String, String, String)> {
    let mut out = vec![];
    for part in html.split("class=\"result__a\"").skip(1) {
        let href = part
            .split("href=\"")
            .nth(1)
            .and_then(|x| x.split('"').next())
            .unwrap_or("");
        let title = part
            .split_once('>')
            .map(|(_, x)| x.split("</a").next().unwrap_or(x))
            .unwrap_or("");
        let snip = part
            .split("result__snippet")
            .nth(1)
            .and_then(|x| {
                x.split('>')
                    .skip(1)
                    .collect::<Vec<_>>()
                    .join(">")
                    .split("</a")
                    .next()
                    .map(str::to_string)
            })
            .unwrap_or_default();
        let mut url = href.replace("&amp;", "&");
        if let Some(u) = url.split("uddg=").nth(1) {
            let u = u.split('&').next().unwrap_or(u);
            url = pct_decode(u);
        }
        if url.starts_with("//") {
            url = format!("https:{url}");
        }
        if !url.is_empty() {
            out.push((html_text(title), url, html_text(&snip)));
        }
        if out.len() >= 8 {
            break;
        }
    }
    out
}

pub(super) fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

impl World for RealWorld {
    fn open(&self, url: &str) -> Result<(), String> {
        #[cfg(target_os = "windows")]
        let r = std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn();
        #[cfg(target_os = "macos")]
        let r = std::process::Command::new("open").arg(url).spawn();
        #[cfg(all(unix, not(target_os = "macos")))]
        let r = std::process::Command::new("xdg-open").arg(url).spawn();
        r.map(|_| ()).map_err(|e| format!("打不开：{e}"))
    }

    fn search<'a>(
        &'a self,
        q: &'a str,
    ) -> futures_util::future::BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let url = "https://html.duckduckgo.com/html/";
            let r = llm::client(url)
                .post(url)
                .form(&[("q", q)])
                .header("User-Agent", "Mozilla/5.0 FloatBall")
                .timeout(Duration::from_secs(20))
                .send()
                .await
                .map_err(|e| format!("搜索连不上：{e}"))?;
            let html = r.text().await.map_err(|e| e.to_string())?;
            let res = parse_ddg(&html);
            if res.is_empty() {
                return Ok("没有搜到结果".into());
            }
            Ok(res
                .iter()
                .enumerate()
                .map(|(i, (t, u, s))| format!("{}. {t}\n{u}\n{s}", i + 1))
                .collect::<Vec<_>>()
                .join("\n\n"))
        })
    }

    fn fetch<'a>(
        &'a self,
        url: &'a str,
    ) -> futures_util::future::BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let r = llm::client(url)
                .get(url)
                .header("User-Agent", "Mozilla/5.0 FloatBall")
                .timeout(Duration::from_secs(20))
                .send()
                .await
                .map_err(|e| format!("打不开：{e}"))?;
            if !r.status().is_success() {
                return Err(format!("网页返回 {}", r.status().as_u16()));
            }
            Ok(html_text(&r.text().await.map_err(|e| e.to_string())?))
        })
    }

    fn remember(&self, a: Allow) {
        (self.remember)(a)
    }
    fn memory_store(&self) -> Option<&dyn MemoryStore> {
        self.memory.as_deref()
    }
}
