use super::*;
use std::cell::Cell;

// A suspension-free match retains its pattern bindings and its scope. Route
// jumps to the enclosing user loop after leaving that scope, so its shadows
// and owned locals cannot leak into the frame on the outgoing edge.
pub(super) fn wrap(builder: &mut Builder<'_>, statement: &mut Stmt) -> Result<()> {
    let (Stmt::Expr(expr) | Stmt::Semi(expr)) = statement else {
        return Ok(());
    };
    if !matches!(expr, Expr::Match { .. }) || nested_await(expr)? {
        return Ok(());
    }
    let flag = Ident {
        name: builder.fresh("match_exit"),
        span: expr.span(),
    };
    let jumps = Cell::new(0u8);
    rewrite(expr, &flag, &jumps)?;
    if jumps.get() == 0 {
        return Ok(());
    }
    let span = flag.span;
    let original = std::mem::replace(expr, unit(span));
    let mut stmts = vec![
        Stmt::Let {
            name: flag.clone(),
            mutable: true,
            ty: Some(Type::Named("i32".into(), span)),
            value: Some(Expr::Literal(Literal::Int(0, span))),
            span,
        },
        Stmt::Semi(Expr::Loop {
            body: Block {
                stmts: vec![Stmt::Semi(original), Stmt::Break { span }],
                span,
            },
            span,
        }),
    ];
    for (code, jump) in [(1, Stmt::Break { span }), (2, Stmt::Continue { span })] {
        if jumps.get() & code != 0 {
            stmts.push(Stmt::Semi(Expr::If {
                cond: Box::new(flag_op(&flag, BinOp::Eq, code)),
                then_block: Block {
                    stmts: vec![jump],
                    span,
                },
                else_block: None,
                span,
            }));
        }
    }
    *expr = Expr::Block(Block { stmts, span });
    Ok(())
}

fn flag_op(flag: &Ident, op: BinOp, value: u8) -> Expr {
    Expr::Binary {
        left: Box::new(Expr::Ident(flag.clone())),
        op,
        right: Box::new(Expr::Literal(Literal::Int(value.into(), flag.span))),
        span: flag.span,
    }
}

fn rewrite(expr: &mut Expr, flag: &Ident, jumps: &Cell<u8>) -> Result<()> {
    match expr {
        Expr::Closure { .. } | Expr::Loop { .. } => Ok(()),
        Expr::While { cond, .. } => rewrite(cond, flag, jumps),
        Expr::For { iter, .. } => rewrite(iter, flag, jumps),
        _ => children(expr, &mut |e| rewrite(e, flag, jumps), &mut |block| {
            for s in &mut block.stmts {
                match s {
                    Stmt::Break { span } | Stmt::Continue { span } => {
                        let span = *span;
                        let code = if matches!(s, Stmt::Break { .. }) {
                            1
                        } else {
                            2
                        };
                        jumps.set(jumps.get() | code);
                        *s = Stmt::Semi(Expr::Block(Block {
                            stmts: vec![
                                Stmt::Semi(flag_op(flag, BinOp::Assign, code)),
                                Stmt::Break { span },
                            ],
                            span,
                        }));
                    }
                    Stmt::Expr(e)
                    | Stmt::Semi(e)
                    | Stmt::Let { value: Some(e), .. }
                    | Stmt::Return { value: Some(e), .. } => rewrite(e, flag, jumps)?,
                    _ => {}
                }
            }
            Ok(())
        }),
    }
}
