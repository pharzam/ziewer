//! Markdown → HTML (GitHub-flavoured) with Mermaid support.

use comrak::nodes::{NodeHtmlBlock, NodeValue};
use comrak::plugins::syntect::SyntectAdapter;
use comrak::{Arena, Options, format_html_with_plugins, options::Plugins, parse_document};
use std::sync::OnceLock;

const MERMAID_TAGS: &str =
    "<script src=\"/_assets/vendor/mermaid.min.js\"></script><script src=\"/_assets/app.js\"></script>";

fn options() -> Options<'static> {
    let mut o = Options::default();
    o.extension.table = true;
    o.extension.tasklist = true;
    o.extension.strikethrough = true;
    o.extension.autolink = true;
    o.extension.footnotes = true;
    o.extension.header_id_prefix = Some(String::new());
    o.render.r#unsafe = true;
    o
}

fn highlighter() -> &'static SyntectAdapter {
    static H: OnceLock<SyntectAdapter> = OnceLock::new();
    H.get_or_init(|| SyntectAdapter::new(Some("InspiredGitHub")))
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Render Markdown to HTML. ```` ```mermaid ```` blocks become `<pre class="mermaid">`.
pub fn markdown_to_html(src: &str) -> String {
    let arena = Arena::new();
    let opts = options();
    let root = parse_document(&arena, src, &opts);
    for node in root.descendants() {
        let replacement = match &node.data().value {
            NodeValue::CodeBlock(cb) if cb.info.split_whitespace().next() == Some("mermaid") => {
                Some(format!("<pre class=\"mermaid\">{}</pre>\n", escape_html(&cb.literal)))
            }
            _ => None,
        };
        if let Some(literal) = replacement {
            node.data_mut().value = NodeValue::HtmlBlock(NodeHtmlBlock { block_type: 6, literal });
        }
    }
    let mut plugins = Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(highlighter());
    let mut out = String::new();
    format_html_with_plugins(root, &opts, &mut out, &plugins).expect("writing to String");
    add_dir_auto(&out)
}

/// Block tags that get `dir="auto"` so right-to-left paragraphs (Persian, Arabic, Hebrew)
/// take their direction from their own text, as GitHub does. `pre`/`code` stay LTR.
const DIR_AUTO_TAGS: &[&str] =
    &["p", "h1", "h2", "h3", "h4", "h5", "h6", "li", "ul", "ol", "blockquote", "table", "th", "td", "dl", "dt", "dd"];

/// Containers whose children carry their own `dir`: `dir="auto"` would ignore that text and
/// fall back to LTR, so their direction is computed from the first strong letter instead.
const DIR_CONTAINERS: &[&str] = &["ul", "ol", "blockquote", "table", "dl"];

fn is_rtl_char(c: char) -> bool {
    matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF)
}

/// Direction of the first strongly-directional letter in `html` text (markup and entities skipped).
fn first_strong_is_rtl(html: &str) -> bool {
    let mut chars = html.chars();
    while let Some(c) = chars.next() {
        match c {
            '<' => while chars.next().is_some_and(|c| c != '>') {},
            '&' => while chars.next().is_some_and(|c| c != ';') {},
            c if is_rtl_char(c) => return true,
            c if c.is_alphabetic() => return false,
            _ => {}
        }
    }
    false
}

/// Add `dir="auto"` to opening block tags that don't already carry a `dir` attribute.
pub fn add_dir_auto(html: &str) -> String {
    let mut out = String::with_capacity(html.len() + html.len() / 8);
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let name_len = rest[1..].find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(rest.len() - 1);
        let name = &rest[1..1 + name_len];
        let after = &rest[1 + name_len..];
        let tag_end = after.find('>').unwrap_or(after.len());
        let is_block = DIR_AUTO_TAGS.contains(&name)
            && after.starts_with(|c: char| c == '>' || c.is_ascii_whitespace())
            && !after[..tag_end].contains("dir=");
        out.push('<');
        out.push_str(name);
        if is_block {
            let rtl = DIR_CONTAINERS.contains(&name) && first_strong_is_rtl(&after[tag_end..]);
            out.push_str(if rtl { " dir=\"rtl\"" } else { " dir=\"auto\"" });
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Insert the Mermaid + app scripts before the last `</body>` (case-insensitive), else append.
pub fn inject_mermaid(html: &str) -> String {
    match html.to_ascii_lowercase().rfind("</body>") {
        Some(i) => format!("{}{}{}", &html[..i], MERMAID_TAGS, &html[i..]),
        None => format!("{html}{MERMAID_TAGS}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn gfm_table() { assert!(markdown_to_html("|a|b|\n|-|-|\n|1|2|\n").contains("<table")); }
    #[test] fn tasklist() {
        let h = markdown_to_html("- [x] done\n");
        assert!(h.contains("type=\"checkbox\"") && h.contains("checked"), "{h}");
    }
    #[test] fn heading_anchor() { let h = markdown_to_html("# Hello World\n"); assert!(h.contains("id=\"hello-world\""), "{h}"); }
    #[test] fn mermaid_block() {
        let h = markdown_to_html("```mermaid\ngraph TD; A-->B\n```\n");
        assert!(h.contains("<pre class=\"mermaid\">graph TD; A--&gt;B"), "{h}");
        assert!(!h.contains("<code"), "{h}");
    }
    #[test] fn rust_highlighted() { let h = markdown_to_html("```rust\nfn main(){}\n```\n"); assert!(h.contains("style="), "{h}"); }
    #[test] fn rtl_blocks_get_dir_auto() {
        let h = markdown_to_html("# عنوان\n\nسلام دنیا\n\n- یک\n- دو\n\n> نقل\n\n|الف|ب|\n|-|-|\n|۱|۲|\n");
        for tag in ["<h1 dir=\"auto\"", "<p dir=\"auto\"", "<ul dir=\"rtl\"", "<li dir=\"auto\"",
                    "<blockquote dir=\"rtl\"", "<table dir=\"rtl\"", "<th dir=\"auto\"", "<td dir=\"auto\""] {
            assert!(h.contains(tag), "missing {tag} in {h}");
        }
    }
    #[test] fn containers_take_direction_from_first_strong_letter() {
        // "#" and entities are not strong; the first real letter decides.
        let h = markdown_to_html("|#|درخواست|\n|-|-|\n|1|x|\n\n|#|Request|\n|-|-|\n|1|x|\n\n> &amp; English\n");
        assert!(h.contains("<table dir=\"rtl\""), "{h}");
        assert!(h.contains("<table dir=\"auto\""), "{h}");
        assert!(h.contains("<blockquote dir=\"auto\""), "{h}");
    }
    #[test] fn dir_auto_leaves_code_and_lookalike_tags_alone() {
        let h = markdown_to_html("```\ncode\n```\n\n<pre>x</pre>\n");
        assert!(!h.contains("dir="), "{h}");
        assert_eq!(add_dir_auto("<thead><link><path><p dir=\"rtl\">"), "<thead><link><path><p dir=\"rtl\">");
        assert_eq!(add_dir_auto("<li>\n<p class=\"x\">"), "<li dir=\"auto\">\n<p dir=\"auto\" class=\"x\">");
    }
    #[test] fn inject_before_body() {
        let h = inject_mermaid("<html><body>x</BODY></html>");
        let s = h.find("mermaid.min.js").unwrap();
        assert!(s < h.find("</BODY>").unwrap());
    }
    #[test] fn inject_without_body() {
        assert!(inject_mermaid("<p>x</p>").ends_with("<script src=\"/_assets/app.js\"></script>"));
    }
}
