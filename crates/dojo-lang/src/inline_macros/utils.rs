use std::collections::HashMap;
use std::sync::Mutex;

use cairo_lang_syntax::node::ast::{ExprPath, ExprStructCtorCall};
use cairo_lang_syntax::node::kind::SyntaxKind;
use cairo_lang_syntax::node::SyntaxNode;

type ModuleName = String;
type FunctionName = String;
lazy_static::lazy_static! {
    pub static ref SYSTEM_WRITES: Mutex<HashMap<ModuleName, HashMap<FunctionName, Vec<SystemRWOpRecord>>>> = Default::default();
    pub static ref SYSTEM_READS: Mutex<HashMap<ModuleName, Vec<String>>> = Default::default();
}

pub enum SystemRWOpRecord {
    StructCtor(ExprStructCtorCall),
    Path(ExprPath),
}

pub fn parent_of_kind(
    db: &dyn cairo_lang_syntax::node::db::SyntaxGroup,
    target: &SyntaxNode,
    kind: SyntaxKind,
) -> Result<SyntaxNode, String> {
    let mut new_target = target.clone();
    let mut log = "".to_string();
    while let Some(parent) = new_target.parent() {
        if kind == parent.kind(db) {
            return Ok(parent);
        }
        new_target = parent;
        log.push_str(&format!("{}\n", new_target.kind(db)));
    }
    Err(log)
}
