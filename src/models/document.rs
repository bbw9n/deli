use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentFormat {
    Markdown,
    Mintlify,
    PlainText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentResource {
    pub id: String,
    pub title: String,
    pub path: String,
    pub format: DocumentFormat,
    pub raw: String,
    pub nodes: Vec<DocumentNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentNode {
    Heading { level: u8, text: String },
    Paragraph(String),
    CodeBlock(String),
    ListItem(String),
    Quote(String),
}

impl DocumentResource {
    pub fn from_source(
        id: impl Into<String>,
        path: impl Into<String>,
        format: DocumentFormat,
        raw: impl Into<String>,
    ) -> Self {
        let path = path.into();
        let raw = raw.into();
        let normalized = normalize_document(&raw, &format);
        let title = normalized
            .iter()
            .find_map(|node| match node {
                DocumentNode::Heading { text, .. } => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_else(|| path.clone());

        Self {
            id: id.into(),
            title,
            path,
            format,
            raw,
            nodes: normalized,
        }
    }
}

pub fn normalize_document(source: &str, format: &DocumentFormat) -> Vec<DocumentNode> {
    let normalized = match format {
        DocumentFormat::Mintlify => strip_mintlify_constructs(source),
        DocumentFormat::Markdown => source.to_string(),
        DocumentFormat::PlainText => {
            return vec![DocumentNode::Paragraph(source.trim().to_string())];
        }
    };

    parse_markdown(&normalized)
}

fn strip_mintlify_constructs(source: &str) -> String {
    let mut lines = Vec::new();
    let mut in_frontmatter = false;
    let mut frontmatter_consumed = false;

    for line in source.lines() {
        if !frontmatter_consumed && line.trim() == "---" {
            in_frontmatter = !in_frontmatter;
            if !in_frontmatter {
                frontmatter_consumed = true;
            }
            continue;
        }

        if in_frontmatter {
            continue;
        }

        let trimmed = line.trim();
        if trimmed.starts_with('<') && trimmed.ends_with('>') && !trimmed.starts_with("</") {
            continue;
        }
        if trimmed.starts_with("</") {
            continue;
        }
        lines.push(line);
    }

    lines.join("\n")
}

fn parse_markdown(source: &str) -> Vec<DocumentNode> {
    let mut nodes = Vec::new();
    let mut current_text = String::new();
    let mut current_heading = None;
    let mut in_code_block = false;
    let mut current_code = String::new();
    let mut in_item = false;
    let mut in_quote = false;

    for event in Parser::new(source) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => current_heading = Some(level),
            Event::End(TagEnd::Heading(..)) => {
                let text = current_text.trim().to_string();
                nodes.push(DocumentNode::Heading {
                    level: heading_level(current_heading.take().unwrap_or(HeadingLevel::H1)),
                    text,
                });
                current_text.clear();
            }
            Event::Start(Tag::CodeBlock(..)) => {
                in_code_block = true;
                current_code.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                nodes.push(DocumentNode::CodeBlock(current_code.trim_end().to_string()));
                current_code.clear();
                in_code_block = false;
            }
            Event::Start(Tag::Item) => {
                in_item = true;
                current_text.clear();
            }
            Event::End(TagEnd::Item) => {
                nodes.push(DocumentNode::ListItem(current_text.trim().to_string()));
                current_text.clear();
                in_item = false;
            }
            Event::Start(Tag::BlockQuote(..)) => {
                in_quote = true;
                current_text.clear();
            }
            Event::End(TagEnd::BlockQuote(..)) => {
                nodes.push(DocumentNode::Quote(current_text.trim().to_string()));
                current_text.clear();
                in_quote = false;
            }
            Event::Text(text) | Event::Code(text) => {
                if in_code_block {
                    current_code.push_str(&text);
                } else {
                    current_text.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if in_code_block {
                    current_code.push('\n');
                } else {
                    current_text.push('\n');
                }
            }
            Event::End(TagEnd::Paragraph) => {
                if current_text.trim().is_empty() || in_item || in_quote {
                    current_text.clear();
                } else {
                    nodes.push(DocumentNode::Paragraph(current_text.trim().to_string()));
                    current_text.clear();
                }
            }
            _ => {}
        }
    }

    if nodes.is_empty() && !source.trim().is_empty() {
        nodes.push(DocumentNode::Paragraph(source.trim().to_string()));
    }

    nodes
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_markdown_into_render_nodes() {
        let nodes = normalize_document("# Title\n\nhello\n\n- item", &DocumentFormat::Markdown);
        assert_eq!(
            nodes,
            vec![
                DocumentNode::Heading {
                    level: 1,
                    text: "Title".into()
                },
                DocumentNode::Paragraph("hello".into()),
                DocumentNode::ListItem("item".into())
            ]
        );
    }

    #[test]
    fn strips_mintlify_frontmatter_and_components() {
        let input = "---\ntitle: Demo\n---\n<Steps>\n# Title\n\nReal text\n</Steps>";
        let nodes = normalize_document(input, &DocumentFormat::Mintlify);
        assert_eq!(
            nodes,
            vec![
                DocumentNode::Heading {
                    level: 1,
                    text: "Title".into()
                },
                DocumentNode::Paragraph("Real text".into())
            ]
        );
    }
}
