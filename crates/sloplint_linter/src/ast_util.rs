//! Small AST-traversal helpers shared by the structural rules.

use sloplint_python::ast::{ExceptHandler, Stmt, StmtFunctionDef};

/// Visit every statement in `body`, descending through classes and all compound
/// statements. The visitor sees each statement exactly once.
pub fn walk_statements<'a>(body: &'a [Stmt], visit: &mut dyn FnMut(&'a Stmt)) {
    for stmt in body {
        visit(stmt);
        match stmt {
            Stmt::FunctionDef(node) => walk_statements(&node.body, visit),
            Stmt::ClassDef(node) => walk_statements(&node.body, visit),
            Stmt::If(node) => {
                walk_statements(&node.body, visit);
                for clause in &node.elif_else_clauses {
                    walk_statements(&clause.body, visit);
                }
            }
            Stmt::For(node) => {
                walk_statements(&node.body, visit);
                walk_statements(&node.orelse, visit);
            }
            Stmt::While(node) => {
                walk_statements(&node.body, visit);
                walk_statements(&node.orelse, visit);
            }
            Stmt::With(node) => walk_statements(&node.body, visit),
            Stmt::Try(node) => {
                walk_statements(&node.body, visit);
                for handler in &node.handlers {
                    let ExceptHandler::ExceptHandler(handler) = handler;
                    walk_statements(&handler.body, visit);
                }
                walk_statements(&node.orelse, visit);
                walk_statements(&node.finalbody, visit);
            }
            Stmt::Match(node) => {
                for case in &node.cases {
                    walk_statements(&case.body, visit);
                }
            }
            _ => {}
        }
    }
}

/// Collect every function definition reachable in `body` (methods + nested functions).
pub fn collect_functions<'a>(body: &'a [Stmt], out: &mut Vec<&'a StmtFunctionDef>) {
    walk_statements(body, &mut |stmt| {
        if let Stmt::FunctionDef(function) = stmt {
            out.push(function);
        }
    });
}
