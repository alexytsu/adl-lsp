use async_lsp::lsp_types::{Position, Range};
use tree_sitter::{Point, Range as TsRange};

pub fn ts_to_lsp_position(p: &Point) -> Position {
    Position {
        line: p.row as u32,
        character: p.column as u32,
    }
}

pub fn ts_to_lsp_range(r: &TsRange) -> Range {
    Range {
        start: ts_to_lsp_position(&r.start_point),
        end: ts_to_lsp_position(&r.end_point),
    }
}

#[allow(unused)]
pub fn lsp_to_ts_point(p: &Position) -> Point {
    Point {
        row: p.line as usize,
        column: p.character as usize,
    }
}
