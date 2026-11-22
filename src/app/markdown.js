/* ───────── Markdown（够用的子集，先转义再排版） ───────── */
export function esc(s) {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}

export function inline(s) {
  return s
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<b>$1</b>")
    .replace(/(^|[^*])\*([^*\s][^*]*)\*/g, "$1<i>$2</i>")
    .replace(/\[([^\]]+)\]\((https?:[^)\s]+)\)/g, '<u title="$2">$1</u>');
}

export function md(src) {
  const out = [];
  const parts = src.split(/```/);
  parts.forEach((part, i) => {
    if (i % 2 === 1) {
      const code = part.replace(/^[\w+-]*\n/, "");
      out.push("<pre><code>" + esc(code.replace(/\n$/, "")) + "</code></pre>");
      return;
    }
    let list = null;
    const close = () => {
      if (list) {
        out.push("</" + list + ">");
        list = null;
      }
    };
    let para = [];
    const flush = () => {
      if (para.length) {
        out.push("<p>" + para.map((l) => inline(esc(l))).join("<br>") + "</p>");
        para = [];
      }
    };
    for (const line of part.split("\n")) {
      let m;
      if (!line.trim()) {
        flush();
        close();
        continue;
      }
      if ((m = line.match(/^(#{1,3})\s+(.*)/))) {
        flush();
        close();
        out.push(`<h${m[1].length}>` + inline(esc(m[2])) + `</h${m[1].length}>`);
        continue;
      }
      if ((m = line.match(/^\s*[-*•]\s+(.*)/)) || (m = line.match(/^\s*\d+[.)]\s+(.*)/))) {
        flush();
        const kind = /^\s*\d/.test(line) ? "ol" : "ul";
        if (list !== kind) {
          close();
          out.push("<" + kind + ">");
          list = kind;
        }
        out.push("<li>" + inline(esc(m[1])) + "</li>");
        continue;
      }
      close();
      para.push(line);
    }
    flush();
    close();
  });
  return out.join("");
}
