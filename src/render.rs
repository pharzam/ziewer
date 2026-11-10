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

    #[test] fn gfm_table() { assert!(markdown_to_html("|a|b|\n|-|-|\n|1|2|\n").contains("<table>")); }
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
    #[test] fn inject_before_body() {
        let h = inject_mermaid("<html><body>x</BODY></html>");
        let s = h.find("mermaid.min.js").unwrap();
        assert!(s < h.find("</BODY>").unwrap());
    }
    #[test] fn inject_without_body() {
        assert!(inject_mermaid("<p>x</p>").ends_with("<script src=\"/_assets/app.js\"></script>"));
    }
}
