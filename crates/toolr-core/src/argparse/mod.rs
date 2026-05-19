//! Built-in argparse scanner: AST-walks Python files declared in
//! `[tool.toolr.argparse.*]` and grafts their `parser.add_argument`
//! calls as manifest children of user-declared dispatcher commands.

pub mod attach;
pub mod config;
pub mod scan;

pub use config::{ArgparseBlock, Attachment, parse_blocks, parse_blocks_from_pyproject};
