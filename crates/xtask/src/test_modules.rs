//! `cargo xtask check-test-modules`: Rust unit tests live in their own `tests.rs` file.
//! Parses with `syn`, so attribute order, comments and strings can't hide or fake a test module.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{Attribute, ItemMod, Meta, Token};

/// Fail if any of `files` holds an inline module gated on `cfg(test)`.
pub fn run(files: &[PathBuf]) -> Result<()> {
    let mut found = Vec::new();
    for file in files {
        let source =
            fs::read_to_string(file).with_context(|| format!("read {}", file.display()))?;
        let names =
            inline_test_modules(&source).with_context(|| format!("parse {}", file.display()))?;
        found.extend(names.into_iter().map(|name| (file, name)));
    }
    if found.is_empty() {
        return Ok(());
    }
    for (file, name) in &found {
        eprintln!("{}: inline test module `{name}`", file.display());
    }
    bail!(
        "move each test module into its own `tests.rs` and declare it with `mod tests;` \
         (see \"Tests\" in CONTRIBUTING.md)"
    )
}

/// Names of the inline modules in `source` that `cfg(test)` gates, wherever they sit: at the top
/// level, in other inline modules, or in a function or `const` body.
fn inline_test_modules(source: &str) -> Result<Vec<String>> {
    let file = syn::parse_file(source)?;
    let mut finder = Finder::default();
    finder.visit_file(&file);
    Ok(finder.found)
}

#[derive(Default)]
struct Finder {
    found: Vec<String>,
}

impl<'ast> Visit<'ast> for Finder {
    fn visit_item_mod(&mut self, module: &'ast ItemMod) {
        if module.content.is_some() && module.attrs.iter().any(gates_on_test) {
            self.found.push(module.ident.to_string());
        } else {
            visit::visit_item_mod(self, module);
        }
    }
}

/// Whether `attr` is a `cfg` naming `test`. One `syn` can't read counts as test, so the check fails
/// closed.
fn gates_on_test(attr: &Attribute) -> bool {
    attr.path().is_ident("cfg") && attr.parse_args::<Meta>().map_or(true, |p| names_test(&p))
}

/// Whether `test` appears in a cfg predicate outside `not(...)`.
fn names_test(predicate: &Meta) -> bool {
    match predicate {
        Meta::Path(path) => path.is_ident("test"),
        Meta::List(list) if list.path.is_ident("not") => false,
        Meta::List(list) => list
            .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
            .map_or(true, |inner| inner.iter().any(names_test)),
        Meta::NameValue(_) => false,
    }
}

#[cfg(test)]
mod tests;
