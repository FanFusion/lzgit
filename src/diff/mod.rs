pub mod inline;
pub mod model;
#[allow(dead_code)]
pub mod parse;
pub mod side_by_side;
pub mod unified;
pub mod width;

pub use model::{
    GitDiffCell, GitDiffCellKind, GitDiffRow, InlineSpan, UnifiedDiffRow, UnifiedRowKind,
};
pub use side_by_side::build_side_by_side_rows;
pub use unified::build_unified_rows;
pub use width::{display_width, pad_to_width, slice_chars};
