use async_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Range};

use crate::node::NodeKind;
use crate::parser::tree::Tree;
use crate::parser::ts_lsp_interop::ts_to_lsp_position;

use super::ParsedTree;

impl ParsedTree {
    pub fn collect_parse_diagnostics(&self) -> Vec<Diagnostic> {
        self.find_first_node(NodeKind::is_error)
            .into_iter()
            .map(|n| Diagnostic {
                range: Range {
                    start: ts_to_lsp_position(&n.start_position()),
                    end: ts_to_lsp_position(&n.end_position()),
                },
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("adl-vscode".to_string()), // TODO: maybe make this set at CLI?
                message: "Syntax error".to_string(),
                ..Default::default()
            })
            .collect()
    }

    // pub fn collect_import_diagnostics(
    //     &self,
    //     content: &[u8],
    //     import: Vec<String>,
    // ) -> Vec<Diagnostic> {
    // }
}

#[cfg(test)]
mod test {
    use async_lsp::lsp_types::Url;
    use insta::assert_yaml_snapshot;

    use crate::parser::AdlParser;

    #[test]
    fn test_collect_parse_error() {
        let url: Url = "file://foo/error.adl".parse().unwrap();
        let contents = include_str!("input/error.adl");

        let parsed = AdlParser::new().parse(url.clone(), contents);
        assert!(parsed.is_some());
        assert_yaml_snapshot!(parsed.unwrap().collect_parse_diagnostics());
    }
}
