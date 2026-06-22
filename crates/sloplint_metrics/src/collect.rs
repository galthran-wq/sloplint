//! AST collectors: gather every function (methods + nested) and every class reachable in a
//! body, so each unit is measured independently. Both recurse through compound statements and
//! treat a nested def/class as a collected unit *and* descend into it.

use sloplint_python::ast::{ExceptHandler, Stmt, StmtClassDef, StmtFunctionDef};

/// Collect functions (methods + nested) so each is measured independently.
pub(crate) fn collect_functions<'a>(body: &'a [Stmt], out: &mut Vec<&'a StmtFunctionDef>) {
    for stmt in body {
        match stmt {
            Stmt::FunctionDef(function) => {
                out.push(function);
                collect_functions(&function.body, out);
            }
            Stmt::ClassDef(node) => collect_functions(&node.body, out),
            Stmt::If(node) => {
                collect_functions(&node.body, out);
                for clause in &node.elif_else_clauses {
                    collect_functions(&clause.body, out);
                }
            }
            Stmt::For(node) => {
                collect_functions(&node.body, out);
                collect_functions(&node.orelse, out);
            }
            Stmt::While(node) => {
                collect_functions(&node.body, out);
                collect_functions(&node.orelse, out);
            }
            Stmt::With(node) => collect_functions(&node.body, out),
            Stmt::Try(node) => {
                collect_functions(&node.body, out);
                for handler in &node.handlers {
                    let ExceptHandler::ExceptHandler(handler) = handler;
                    collect_functions(&handler.body, out);
                }
                collect_functions(&node.orelse, out);
                collect_functions(&node.finalbody, out);
            }
            Stmt::Match(node) => {
                for case in &node.cases {
                    collect_functions(&case.body, out);
                }
            }
            _ => {}
        }
    }
}

/// Recursively collect every class definition (top-level, nested in functions, classes, or
/// compound statements), mirroring [`collect_functions`].
pub(crate) fn collect_classes<'a>(body: &'a [Stmt], out: &mut Vec<&'a StmtClassDef>) {
    for stmt in body {
        match stmt {
            Stmt::ClassDef(node) => {
                out.push(node);
                collect_classes(&node.body, out);
            }
            Stmt::FunctionDef(node) => collect_classes(&node.body, out),
            Stmt::If(node) => {
                collect_classes(&node.body, out);
                for clause in &node.elif_else_clauses {
                    collect_classes(&clause.body, out);
                }
            }
            Stmt::For(node) => {
                collect_classes(&node.body, out);
                collect_classes(&node.orelse, out);
            }
            Stmt::While(node) => {
                collect_classes(&node.body, out);
                collect_classes(&node.orelse, out);
            }
            Stmt::With(node) => collect_classes(&node.body, out),
            Stmt::Try(node) => {
                collect_classes(&node.body, out);
                for handler in &node.handlers {
                    let ExceptHandler::ExceptHandler(handler) = handler;
                    collect_classes(&handler.body, out);
                }
                collect_classes(&node.orelse, out);
                collect_classes(&node.finalbody, out);
            }
            Stmt::Match(node) => {
                for case in &node.cases {
                    collect_classes(&case.body, out);
                }
            }
            _ => {}
        }
    }
}
