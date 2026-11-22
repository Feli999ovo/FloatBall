use super::super::{folder, host, html_text, parse_ddg};

#[test]
fn html_and_search_parsing() {
    assert_eq!(html_text("<html><head><title>T</title><style>a{}</style></head><body><p>你好&amp;世界</p><script>x()</script></body></html>"), "T 你好&世界");
    let h = r#"<a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2F%E4%BD%A0&amp;rut=1">Example <b>站</b></a><a class="result__snippet" href="x">摘要 <b>一</b></a>"#;
    assert_eq!(
        parse_ddg(h),
        vec![(
            "Example 站".into(),
            "https://example.com/你".into(),
            "摘要 一".into()
        )]
    );
    assert_eq!(folder("/a/b/c.txt"), "b");
    assert_eq!(host("https://x.com/a?b"), "x.com");
}
