//! Render an [`ast::TranslationUnit`] back to compilable C.
//!
//! Expressions are printed **fully parenthesised**: every operator wraps its
//! operands in parentheses. The output is noisier than the source, but it is
//! correct by construction — the printer never has to reason about operator
//! precedence or associativity, so a transform that rearranges the tree can
//! never produce mis-grouped code. The result is valid C regardless.
//!
//! Statements are printed with modest whitespace (newlines and indentation)
//! so the output stays readable and compiles cleanly; squeezing it is the
//! minifier's job, not this module's.

use super::ast::*;

/// Render a whole translation unit to C source text.
pub fn print_unit(unit: &TranslationUnit) -> String {
    let mut out = String::new();
    for item in &unit.items {
        print_item(item, &mut out);
        out.push('\n');
    }
    out
}

fn print_item(item: &Item, out: &mut String) {
    match item {
        Item::Directive(line) => out.push_str(line),
        Item::Declaration(decl) => {
            print_declaration(decl, out);
            out.push(';');
        }
        Item::Function(func) => print_function(func, out),
        Item::Raw(text) => out.push_str(text),
    }
}

fn print_function(func: &Function, out: &mut String) {
    print_type(&func.return_type, out);
    out.push(' ');
    out.push_str(&func.name);
    out.push('(');
    print_params(&func.params, func.variadic, out);
    out.push(')');
    match &func.body {
        None => out.push(';'),
        Some(body) => {
            out.push(' ');
            print_block(body, 0, out);
        }
    }
}

fn print_params(params: &[Param], variadic: bool, out: &mut String) {
    for (i, param) in params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        print_type(&param.ty, out);
        if let Some(name) = &param.name {
            out.push(' ');
            out.push_str(name);
        }
    }
    if variadic {
        if params.is_empty() {
            out.push_str("...");
        } else {
            out.push_str(", ...");
        }
    }
}

fn print_type(ty: &Type, out: &mut String) {
    out.push_str(&ty.specifiers.join(" "));
    for _ in 0..ty.pointers {
        out.push('*');
    }
}

fn print_declaration(decl: &Declaration, out: &mut String) {
    print_type(&decl.ty, out);
    out.push(' ');
    for (i, d) in decl.declarators.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        for _ in 0..d.pointers {
            out.push('*');
        }
        out.push_str(&d.name);
        for dim in &d.array_dims {
            out.push('[');
            if let Some(e) = dim {
                print_expr(e, out);
            }
            out.push(']');
        }
        if let Some(init) = &d.init {
            out.push('=');
            print_expr(init, out);
        }
    }
}

fn indent(level: usize, out: &mut String) {
    for _ in 0..level {
        out.push_str("    ");
    }
}

fn print_block(block: &Block, level: usize, out: &mut String) {
    out.push_str("{\n");
    for stmt in &block.stmts {
        indent(level + 1, out);
        print_stmt(stmt, level + 1, out);
        out.push('\n');
    }
    indent(level, out);
    out.push('}');
}

/// Print a statement. `level` is the indentation of the statement itself; a
/// nested block opened by the statement indents one deeper.
fn print_stmt(stmt: &Stmt, level: usize, out: &mut String) {
    match stmt {
        Stmt::Declaration(decl) => {
            print_declaration(decl, out);
            out.push(';');
        }
        Stmt::Expr(e) => {
            print_expr(e, out);
            out.push(';');
        }
        Stmt::Return(None) => out.push_str("return;"),
        Stmt::Return(Some(e)) => {
            out.push_str("return ");
            print_expr(e, out);
            out.push(';');
        }
        Stmt::Break => out.push_str("break;"),
        Stmt::Continue => out.push_str("continue;"),
        Stmt::Empty => out.push(';'),
        Stmt::Block(block) => print_block(block, level, out),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            out.push_str("if(");
            print_expr(cond, out);
            out.push(')');
            print_branch(then_branch, level, out);
            if let Some(else_branch) = else_branch {
                out.push_str(" else");
                // `else if` chains read better without an extra brace level.
                if matches!(else_branch.as_ref(), Stmt::If { .. }) {
                    out.push(' ');
                    print_stmt(else_branch, level, out);
                } else {
                    print_branch(else_branch, level, out);
                }
            }
        }
        Stmt::While { cond, body } => {
            out.push_str("while(");
            print_expr(cond, out);
            out.push(')');
            print_branch(body, level, out);
        }
        Stmt::DoWhile { body, cond } => {
            out.push_str("do");
            print_branch(body, level, out);
            out.push_str(" while(");
            print_expr(cond, out);
            out.push_str(");");
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            out.push_str("for(");
            match init {
                Some(stmt) => print_stmt(stmt, level, out), // prints its own `;`
                None => out.push(';'),
            }
            if let Some(cond) = cond {
                print_expr(cond, out);
            }
            out.push(';');
            if let Some(step) = step {
                print_expr(step, out);
            }
            out.push(')');
            print_branch(body, level, out);
        }
        Stmt::Switch { cond, body } => {
            out.push_str("switch(");
            print_expr(cond, out);
            out.push(')');
            print_branch(body, level, out);
        }
        // Labels are printed inline with the statement they prefix; the
        // statement may itself be another label (fallthrough) or a block.
        Stmt::Case { value, body } => {
            out.push_str("case ");
            print_expr(value, out);
            out.push_str(": ");
            print_stmt(body, level, out);
        }
        Stmt::Default { body } => {
            out.push_str("default: ");
            print_stmt(body, level, out);
        }
        Stmt::Label { name, body } => {
            out.push_str(name);
            out.push_str(": ");
            print_stmt(body, level, out);
        }
        Stmt::Goto(name) => {
            out.push_str("goto ");
            out.push_str(name);
            out.push(';');
        }
    }
}

/// Print the body of an `if`/`while`/`for`/`do`. A block is placed on the same
/// line after a space; any other statement goes on its own indented line.
fn print_branch(stmt: &Stmt, level: usize, out: &mut String) {
    if let Stmt::Block(block) = stmt {
        out.push(' ');
        print_block(block, level, out);
    } else {
        out.push('\n');
        indent(level + 1, out);
        print_stmt(stmt, level + 1, out);
    }
}

fn print_expr(expr: &Expr, out: &mut String) {
    match expr {
        Expr::Int(n) => out.push_str(&n.to_string()),
        Expr::Str(s) => {
            out.push('"');
            out.push_str(s);
            out.push('"');
        }
        Expr::Char(s) => {
            out.push('\'');
            out.push_str(s);
            out.push('\'');
        }
        Expr::Ident(name) => out.push_str(name),
        Expr::Raw(text) => out.push_str(text),

        Expr::Unary { op, expr } => {
            out.push('(');
            out.push_str(op.text());
            print_expr(expr, out);
            out.push(')');
        }
        Expr::Postfix { op, expr } => {
            out.push('(');
            print_expr(expr, out);
            out.push_str(op.text());
            out.push(')');
        }
        Expr::Binary { op, lhs, rhs } => {
            out.push('(');
            print_expr(lhs, out);
            out.push_str(op.text());
            print_expr(rhs, out);
            out.push(')');
        }
        Expr::Assign { op, target, value } => {
            out.push('(');
            print_expr(target, out);
            out.push_str(op.text());
            print_expr(value, out);
            out.push(')');
        }
        Expr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => {
            out.push('(');
            print_expr(cond, out);
            out.push('?');
            print_expr(then_expr, out);
            out.push(':');
            print_expr(else_expr, out);
            out.push(')');
        }
        Expr::Cast { ty, expr } => {
            out.push_str("((");
            print_type(ty, out);
            out.push(')');
            print_expr(expr, out);
            out.push(')');
        }
        Expr::Call { callee, args } => {
            // A bare-identifier callee is printed without wrapping parens.
            // Besides being tidier, `(name)(args)` would break a call to a
            // function-like macro: the preprocessor expands `name` only when
            // it is immediately followed by `(`, which `(name)` prevents.
            match callee.as_ref() {
                Expr::Ident(name) => out.push_str(name),
                other => {
                    out.push('(');
                    print_expr(other, out);
                    out.push(')');
                }
            }
            out.push('(');
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                print_expr(arg, out);
            }
            out.push(')');
        }
        Expr::Index { base, index } => {
            out.push('(');
            print_expr(base, out);
            out.push(')');
            out.push('[');
            print_expr(index, out);
            out.push(']');
        }
        Expr::Member { base, arrow, field } => {
            out.push('(');
            print_expr(base, out);
            out.push(')');
            out.push_str(if *arrow { "->" } else { "." });
            out.push_str(field);
        }
        Expr::Comma { lhs, rhs } => {
            out.push('(');
            print_expr(lhs, out);
            out.push(',');
            print_expr(rhs, out);
            out.push(')');
        }
        Expr::InitList(elems) => {
            out.push('{');
            for (i, e) in elems.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                print_expr(e, out);
            }
            out.push('}');
        }
    }
}
