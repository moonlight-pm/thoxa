"""Hand-written recursive-descent + Pratt parser for Thoxa.

Chosen over a grammar because Thoxa uses angle-bracket generics and no statement
separators — two classic ambiguity sources — and because friendly errors are core
to the language's pitch. Generic-argument lists (`f<Int64>(x)`) are disambiguated from
comparison (`a < b`) by a bounded speculative parse that only commits when a `(`
follows the closing `>`.
"""

from __future__ import annotations

from . import ast
from .errors import Diagnostic, Source, Span, ThoxaError
from .lexer import Lexer
from .tokens import T, Token

_ASSIGN_OPS = {T.EQ: "=", T.PLUSEQ: "+=", T.MINUSEQ: "-=", T.STAREQ: "*=", T.SLASHEQ: "/="}
_LVALUE = (ast.Name, ast.FieldAccess, ast.Index)


class Parser:
    def __init__(self, tokens: list[Token], source: Source) -> None:
        self.toks = tokens
        self.src = source
        self.pos = 0
        self._split_undo = []

    # -- cursor -----------------------------------------------------------
    def cur(self) -> Token:
        return self.toks[self.pos]

    def peek(self, k: int = 1) -> Token:
        j = self.pos + k
        return self.toks[j] if j < len(self.toks) else self.toks[-1]

    def advance(self) -> Token:
        t = self.toks[self.pos]
        if t.kind is not T.EOF:
            self.pos += 1
        return t

    def check(self, kind: T) -> bool:
        return self.cur().kind is kind

    def match(self, kind: T) -> bool:
        if self.check(kind):
            self.advance()
            return True
        return False

    def error(self, msg: str, span: Span | None = None, notes=None):
        raise ThoxaError(Diagnostic(msg, span or self.cur().span, self.src, notes or []))

    def expect(self, kind: T, what: str) -> Token:
        if not self.check(kind):
            self.error(f"expected {what}, found {self._describe(self.cur())}")
        return self.advance()

    @staticmethod
    def _describe(tok: Token) -> str:
        if tok.kind is T.EOF:
            return "end of file"
        if tok.kind in (T.LOWER, T.UPPER):
            return f"'{tok.value}'"
        return f"'{tok.kind.value}'"

    # -- program / statements --------------------------------------------
    def parse_program(self) -> ast.Module:
        span = self.cur().span
        body = []
        while not self.check(T.EOF):
            body.append(self.statement())
        return ast.Module(body, span)

    def statement(self):
        k = self.cur().kind
        if k is T.LET:
            return self.let_binding(mutable=False)
        if k is T.MUT:
            return self.let_binding(mutable=True)
        if k is T.FN:
            return self.function_def(exported=False)
        if k is T.TYPE:
            return self.type_decl(exported=False)
        if k is T.EXPORT:
            self.advance()
            if self.check(T.FN):
                return self.function_def(exported=True)
            if self.check(T.TYPE):
                return self.type_decl(exported=True)
            if self.check(T.LET):
                return self.let_binding(mutable=False, exported=True)
            if self.check(T.MUT):
                self.error(
                    "module state (`mut`) cannot be exported; "
                    "expose it through an exported function"
                )
            if self.check(T.EXTERN):
                return self.extern_fn(exported=True)
            self.error("expected 'fn', 'type', 'let', or 'extern' after 'export'")
        if k is T.RETURN:
            sp = self.advance().span
            value = None if self.cur().kind in (T.RBRACE, T.EOF) else self.expr()
            return ast.Return(value, sp)
        if k is T.BREAK:
            return ast.Break(self.advance().span)
        if k is T.CONTINUE:
            return ast.Continue(self.advance().span)
        if k is T.IMPORT:
            return self.import_stmt()
        if k is T.EXTERN:
            return self.extern_fn()
        # expression or assignment
        e = self.expr()
        if self.cur().kind in _ASSIGN_OPS:
            op_tok = self.advance()
            if not isinstance(e, _LVALUE):
                self.error("cannot assign to this expression", op_tok.span)
            rhs = self.expr()
            return ast.Assign(e, _ASSIGN_OPS[op_tok.kind], rhs, op_tok.span)
        return e

    def let_binding(self, mutable: bool, exported: bool = False):
        sp = self.advance().span  # let / mut
        name = self.expect(T.LOWER, "a value name (camelCase)").value
        type_ann = self.parse_type() if self.match(T.COLON) else None
        self.expect(T.EQ, "'=' in binding")
        value = self.expr()
        return ast.LetBinding(name, mutable, type_ann, value, exported, sp)

    def function_def(self, exported: bool):
        sp = self.expect(T.FN, "'fn'").span
        name = self.expect(T.LOWER, "a function name (camelCase)").value
        type_params = self.type_params_opt()
        params = self.parse_params()
        return_type = self.parse_type() if self.match(T.ARROW) else None
        body = self.block()
        return ast.FunctionDef(name, type_params, params, return_type, body, exported, sp)

    def extern_fn(self, exported: bool = False):
        sp = self.expect(T.EXTERN, "'extern'").span
        self.expect(T.FN, "'fn' after 'extern'")
        name = self.expect(T.LOWER, "a function name (camelCase)").value
        params = self.parse_params()
        return_type = self.parse_type() if self.match(T.ARROW) else None
        self.expect(T.FROM, "the library clause, e.g. 'from \\\"c\\\"'")
        library = self._plain_string(self.expect(T.STRING, "a library name string"))
        symbol = self._plain_string(self.advance()) if self.check(T.STRING) else None
        return ast.ExternFn(name, params, return_type, library, symbol, exported, sp)

    @staticmethod
    def _plain_string(tok) -> str:
        """The literal text of a string token, ignoring any interpolation parts."""
        return "".join(p[1] for p in tok.value if p[0] == "str")

    def type_decl(self, exported: bool):
        sp = self.expect(T.TYPE, "'type'").span
        name = self.expect(T.UPPER, "a type name (PascalCase)").value
        type_params = self.type_params_opt()
        if self.match(T.EQ):
            variants = [self.parse_type()]
            while self.match(T.PIPE):
                variants.append(self.parse_type())
            return ast.TypeAlias(name, type_params, variants, exported, sp)
        # Valued nominal type: `type ReadOnly 0` — a literal in place of a body gives
        # the type an intrinsic scalar (int/float/string/char/bool), read back out
        # with a cast (`x as Int32`, `x as String`, …).
        if self.cur().kind in (T.INT, T.MINUS, T.FLOAT, T.STRING, T.CHAR, T.TRUE, T.FALSE):
            value = self.p_unary()
            return ast.TypeDef(name, type_params, [], [], exported, sp, value=value)
        ctor_params = (
            self.parse_params(allow_export=True) if self.check(T.LPAREN) else []
        )
        members = self.type_body()
        return ast.TypeDef(name, type_params, ctor_params, members, exported, sp)

    def type_body(self):
        self.expect(T.LBRACE, "'{' to open the type body")
        members = []
        while not self.check(T.RBRACE):
            if self.check(T.EOF):
                self.error("unterminated type body, expected '}'")
            exported = self.match(T.EXPORT)
            if self.check(T.FN):
                fn = self.function_def(exported)
                members.append(fn)
            elif self.cur().kind in (T.LET, T.MUT):
                members.append(self.parse_field(exported))
            else:
                self.error("expected a field ('let'/'mut') or method ('fn') in the type body")
        self.expect(T.RBRACE, "'}'")
        return members

    def parse_field(self, exported: bool):
        mutable = self.cur().kind is T.MUT
        sp = self.advance().span
        name = self.expect(T.LOWER, "a field name (camelCase)").value
        type_ann = self.parse_type() if self.match(T.COLON) else None
        default = self.expr() if self.match(T.EQ) else None
        return ast.Field(name, mutable, type_ann, default, exported, sp)

    def import_stmt(self):
        sp = self.expect(T.IMPORT, "'import'").span
        names = None
        namespace = None
        alias = None
        if self.match(T.LBRACE):
            names = []
            while not self.check(T.RBRACE):
                names.append(self._import_item())
                if not self.match(T.COMMA):
                    break
            self.expect(T.RBRACE, "'}'")
        else:
            # A namespace import binds a singleton *instance* of the module, so the
            # binding is a value name — camelCase. The name is chosen freely by the
            # importer (there is no `as` to override, and nothing to override it
            # against); a PascalCase binding names a type, which is the mistake.
            written = self._import_ident("a namespace name")
            if written.kind is not T.LOWER:
                self.error(
                    "a module import binds an instance — use a camelCase name, "
                    f"not '{written.value}'",
                    written.span,
                )
            namespace = written.value
        self.expect(T.FROM, "'from'")
        mod_tok = self.expect(T.STRING, "a module path string")
        module = "".join(p[1] for p in mod_tok.value if p[0] == "str")
        return ast.Import(names, namespace, alias, module, sp)

    def _import_item(self) -> tuple[str, str]:
        """Parse `name [as local]` inside a selective import's braces.

        Returns the (imported, local) pair; `local` defaults to `imported`. An
        alias may not cross casing kinds (a value stays camelCase, a type stays
        PascalCase)."""
        imported = self._import_ident("an imported name")
        if self.match(T.AS):
            local = self._import_ident("an alias name")
            self._check_alias_casing(imported, local)
            return (imported.value, local.value)
        return (imported.value, imported.value)

    def _import_ident(self, what: str) -> Token:
        if not (self.check(T.LOWER) or self.check(T.UPPER)):
            self.error(f"expected {what}, found {self._describe(self.cur())}")
        return self.advance()

    def _check_alias_casing(self, imported: Token, local: Token) -> None:
        if local.kind is not imported.kind:
            self.error(
                f"import alias '{local.value}' can't change casing kind from "
                f"'{imported.value}' — a value stays camelCase, a type PascalCase",
                local.span,
            )

    # -- params & types ---------------------------------------------------
    def type_params_opt(self) -> list[str]:
        if not self.check(T.LT):
            return []
        self.advance()
        names = [self.expect(T.UPPER, "a type parameter (PascalCase)").value]
        while self.match(T.COMMA):
            names.append(self.expect(T.UPPER, "a type parameter (PascalCase)").value)
        self.expect(T.GT, "'>' to close type parameters")
        return names

    def parse_params(self, *, allow_export: bool = False) -> list[ast.Param]:
        self.expect(T.LPAREN, "'('")
        params = []
        while not self.check(T.RPAREN):
            params.append(self.one_param(allow_export=allow_export))
            if not self.match(T.COMMA):
                break
        self.expect(T.RPAREN, "')'")
        return params

    def one_param(self, *, allow_export: bool = False) -> ast.Param:
        sp = self.cur().span
        exported = False
        if self.check(T.EXPORT):
            if not allow_export:
                self.error(
                    "export is only valid on type constructor parameters",
                    sp,
                )
            self.advance()
            exported = True
        variadic = self.match(T.ELLIPSIS)
        name = self.expect(T.LOWER, "a parameter name (camelCase)").value
        positional = self.match(T.BANG)
        type_ann = self.parse_type() if self.match(T.COLON) else None
        default = self.expr() if self.match(T.EQ) else None
        return ast.Param(
            name, type_ann, default, positional, variadic, sp, exported=exported
        )

    def parse_type(self) -> ast.TypeRef:
        tok = self.expect(T.UPPER, "a type name (PascalCase)")
        sp = tok.span
        name = tok.value
        if name == "Fn" and self.check(T.LPAREN):
            self.advance()
            args = []
            while not self.check(T.RPAREN):
                args.append(self.parse_type())
                if not self.match(T.COMMA):
                    break
            self.expect(T.RPAREN, "')'")
            self.expect(T.ARROW, "'->' in function type")
            ret = self.parse_type()
            return ast.TypeRef("Fn", args, ret, sp)
        args = []
        if self.check(T.LT):
            self.advance()
            args.append(self._type_arg())
            while self.match(T.COMMA):
                args.append(self._type_arg())
            self._expect_gt("'>' to close type arguments")
        return ast.TypeRef(name, args, None, sp)

    def _type_arg(self) -> ast.TypeRef:
        """One generic argument: a type, or an integer literal size (Array<Byte, 32>)."""
        if self.check(T.INT):
            tok = self.advance()
            return ast.TypeRef("", [], None, tok.span, int_value=tok.value[0])
        return self.parse_type()

    # -- expressions (Pratt) ---------------------------------------------
    def expr(self):
        return self.p_or()

    def p_or(self):
        left = self.p_and()
        while self.check(T.OR):
            sp = self.advance().span
            left = ast.Logical("or", left, self.p_and(), sp)
        return left

    def p_and(self):
        left = self.p_equality()
        while self.check(T.AND):
            sp = self.advance().span
            left = ast.Logical("and", left, self.p_equality(), sp)
        return left

    def p_equality(self):
        left = self.p_comparison()
        while self.cur().kind in (T.EQEQ, T.NEQ):
            op = self.advance()
            left = ast.BinaryOp(op.kind.value, left, self.p_comparison(), op.span)
        return left

    def p_comparison(self):
        left = self.p_range()
        while self.cur().kind in (T.LT, T.LE, T.GT, T.GE):
            op = self.advance()
            left = ast.BinaryOp(op.kind.value, left, self.p_range(), op.span)
        return left

    def p_range(self):
        left = self.p_bitor()
        if self.cur().kind in (T.DOTDOT, T.DOTDOTEQ):
            inclusive = self.cur().kind is T.DOTDOTEQ
            sp = self.advance().span
            return ast.RangeExpr(left, self.p_bitor(), inclusive, sp)
        return left

    def p_bitor(self):
        left = self.p_bitxor()
        while self.check(T.PIPE):
            op = self.advance()
            left = ast.BinaryOp(op.kind.value, left, self.p_bitxor(), op.span)
        return left

    def p_bitxor(self):
        left = self.p_bitand()
        while self.check(T.CARET):
            op = self.advance()
            left = ast.BinaryOp(op.kind.value, left, self.p_bitand(), op.span)
        return left

    def p_bitand(self):
        left = self.p_shift()
        while self.check(T.AMP):
            op = self.advance()
            left = ast.BinaryOp(op.kind.value, left, self.p_shift(), op.span)
        return left

    def p_shift(self):
        left = self.p_add()
        while self.cur().kind in (T.SHL, T.SHR):
            op = self.advance()
            left = ast.BinaryOp(op.kind.value, left, self.p_add(), op.span)
        return left

    def p_add(self):
        left = self.p_mul()
        while self.cur().kind in (T.PLUS, T.MINUS):
            op = self.advance()
            left = ast.BinaryOp(op.kind.value, left, self.p_mul(), op.span)
        return left

    def p_mul(self):
        left = self.p_unary()
        while self.cur().kind in (T.STAR, T.SLASH, T.PERCENT):
            op = self.advance()
            left = ast.BinaryOp(op.kind.value, left, self.p_unary(), op.span)
        return left

    def p_unary(self):
        if self.check(T.NOT):
            sp = self.advance().span
            return ast.UnaryOp("not", self.p_unary(), sp)
        if self.check(T.MINUS):
            sp = self.advance().span
            return ast.UnaryOp("-", self.p_unary(), sp)
        if self.check(T.TILDE):
            sp = self.advance().span
            return ast.UnaryOp("~", self.p_unary(), sp)
        return self.p_postfix()

    def p_postfix(self):
        e = self.primary()
        while True:
            k = self.cur().kind
            if k is T.LPAREN:
                e = self.finish_call(e, [])
            elif k is T.LBRACKET:
                sp = self.advance().span
                idx = self.expr()
                self.expect(T.RBRACKET, "']'")
                e = ast.Index(e, idx, sp)
            elif k is T.DOT:
                sp = self.advance().span
                # A member may be lowercase (field/method/value) or PascalCase (a
                # type reached through a module namespace, e.g. `strings.Builder`).
                tok = self.cur()
                if tok.kind not in (T.LOWER, T.UPPER):
                    self.error(f"expected a field, method, or type name, found {self._describe(tok)}")
                e = ast.FieldAccess(e, self.advance().value, sp)
            elif k is T.QUESTION:
                sp = self.advance().span
                e = ast.Try(e, sp)
            elif k is T.AS:
                sp = self.advance().span
                e = ast.Cast(e, self.parse_type(), sp)
            elif k is T.CAN:
                sp = self.advance().span
                e = ast.CanCheck(e, self.parse_type(), sp)
            elif k is T.IS:
                sp = self.advance().span
                e = ast.IsCheck(e, self.parse_type(), sp)
            elif k is T.LT:
                saved = self.pos
                mark = len(self._split_undo)
                targs = self.try_type_args()
                if targs is not None and self.check(T.LPAREN):
                    e = self.finish_call(e, targs)
                else:
                    self.pos = saved
                    self._undo_splits(mark)
                    break
            else:
                break
        return e

    def _expect_gt(self, msg: str):
        """Close a generic argument list, splitting a `>>` into two `>`.

        With shift operators, `Array<Array<T>>` lexes its trailing `>>` as one
        SHR token. When a generic close wants `>` but finds `>>`, consume one
        `>` and rewrite the token to the leftover `>` for the enclosing list.
        The rewrite is recorded so a speculative caller can undo it."""
        tok = self.cur()
        if tok.kind is T.GT:
            return self.advance().span
        if tok.kind is T.SHR:
            self._split_undo.append((self.pos, tok))
            self.toks[self.pos] = Token(T.GT, ">", tok.span)
            return tok.span
        return self.expect(T.GT, msg)

    def _undo_splits(self, mark: int) -> None:
        while len(self._split_undo) > mark:
            pos, tok = self._split_undo.pop()
            self.toks[pos] = tok

    def try_type_args(self):
        saved = self.pos
        try:
            self.expect(T.LT, "'<'")
            args = [self.parse_type()]
            while self.match(T.COMMA):
                args.append(self.parse_type())
            self._expect_gt("'>'")
            return args
        except ThoxaError:
            self.pos = saved
            return None

    def finish_call(self, callee, type_args):
        sp = self.expect(T.LPAREN, "'('").span
        args = []
        while not self.check(T.RPAREN):
            args.append(self.parse_arg())
            if not self.match(T.COMMA):
                break
        self.expect(T.RPAREN, "')'")
        return ast.Call(callee, args, type_args, sp)

    def parse_arg(self) -> ast.Arg:
        sp = self.cur().span
        spread = self.match(T.ELLIPSIS)
        label = None
        if self.cur().kind is T.LOWER and self.peek().kind is T.COLON:
            label = self.advance().value
            self.advance()  # ':'
        return ast.Arg(label, self.expr(), spread, sp)

    # -- primaries --------------------------------------------------------
    def primary(self):
        t = self.cur()
        k = t.kind
        if k is T.INT:
            self.advance()
            return ast.IntLit(t.value[0], t.value[1], t.span)
        if k is T.FLOAT:
            self.advance()
            return ast.FloatLit(t.value[0], t.value[1], t.span)
        if k is T.STRING:
            self.advance()
            return self.build_string(t)
        if k is T.CHAR:
            self.advance()
            return ast.CharLit(t.value, t.span)
        if k is T.BYTE:
            self.advance()
            return ast.ByteLit(t.value, t.span)
        if k is T.TRUE:
            self.advance()
            return ast.BoolLit(True, t.span)
        if k is T.FALSE:
            self.advance()
            return ast.BoolLit(False, t.span)
        if k is T.LOWER:
            self.advance()
            return ast.Name(t.value, t.span)
        if k is T.UPPER:
            self.advance()
            if t.value == "Void":
                return ast.VoidLit(t.span)
            return ast.TypeValue(t.value, t.span)
        if k is T.LPAREN:
            self.advance()
            e = self.expr()
            self.expect(T.RPAREN, "')'")
            return e
        if k is T.LBRACKET:
            return self.array_literal()
        if k is T.LBRACE:
            return self.block()
        if k is T.IF:
            return self.if_expr()
        if k is T.MATCH:
            return self.match_expr()
        if k is T.LOOP:
            sp = self.advance().span
            return ast.Loop(self.block(), sp)
        if k is T.WHILE:
            sp = self.advance().span
            cond = self.expr()
            return ast.While(cond, self.block(), sp)
        if k is T.FOR:
            sp = self.advance().span
            var = self.expect(T.LOWER, "a loop variable (camelCase)").value
            self.expect(T.IN, "'in'")
            iterable = self.expr()
            return ast.For(var, iterable, self.block(), sp)
        self.error(f"expected an expression, found {self._describe(t)}")

    def array_literal(self):
        sp = self.expect(T.LBRACKET, "'['").span
        elems = []
        while not self.check(T.RBRACKET):
            elems.append(self.expr())
            if not self.match(T.COMMA):
                break
        self.expect(T.RBRACKET, "']'")
        return ast.ArrayLit(elems, sp)

    def block(self) -> ast.Block:
        sp = self.expect(T.LBRACE, "'{'").span
        stmts = []
        while not self.check(T.RBRACE):
            if self.check(T.EOF):
                self.error("unterminated block, expected '}'", sp)
            stmts.append(self.statement())
        self.expect(T.RBRACE, "'}'")
        return ast.Block(stmts, sp)

    def if_expr(self):
        sp = self.expect(T.IF, "'if'").span
        cond = self.expr()
        then = self.block()
        otherwise = None
        if self.match(T.ELSE):
            otherwise = self.if_expr() if self.check(T.IF) else self.block()
        return ast.If(cond, then, otherwise, sp)

    def match_expr(self):
        sp = self.expect(T.MATCH, "'match'").span
        scrutinee = self.expr()
        self.expect(T.LBRACE, "'{' to open match arms")
        arms = []
        else_body = None
        while not self.check(T.RBRACE):
            if self.check(T.EOF):
                self.error("unterminated match, expected '}'", sp)
            if self.check(T.ELSE):
                self.advance()
                else_body = self.block()
                break
            pat = self.pattern()
            arms.append(ast.MatchArm(pat, self.block(), pat.span))
        self.expect(T.RBRACE, "'}'")
        return ast.Match(scrutinee, arms, else_body, sp)

    def pattern(self) -> ast.Pattern:
        t = self.cur()
        k = t.kind
        if k in (T.INT, T.FLOAT, T.STRING, T.CHAR, T.BYTE, T.TRUE, T.FALSE):
            node = self.primary()
            return ast.LiteralPattern(node, t.span)
        if k is T.MINUS:
            node = self.p_unary()
            return ast.LiteralPattern(node, t.span)
        if k is T.LOWER:
            self.advance()
            return ast.BindingPattern(t.value, t.span)
        if k is T.UPPER:
            self.advance()
            subs = []
            if self.match(T.LPAREN):
                while not self.check(T.RPAREN):
                    subs.append(self.pattern())
                    if not self.match(T.COMMA):
                        break
                self.expect(T.RPAREN, "')'")
            return ast.ConstructorPattern(t.value, subs, t.span)
        self.error(f"expected a pattern, found {self._describe(t)}")

    # -- string interpolation --------------------------------------------
    def build_string(self, tok: Token) -> ast.StringLit:
        parts = []
        for part in tok.value:
            if part[0] == "str":
                parts.append(("str", part[1]))
            else:  # ("expr", src, span)
                _, src, span = part
                node = parse_expr_source(src, self.src.filename, span)
                parts.append(("expr", node))
        return ast.StringLit(parts, tok.span)


def parse(text: str, filename: str = "<input>") -> ast.Module:
    source = Source(text, filename)
    tokens = Lexer(source).tokenize()
    return Parser(tokens, source).parse_program()


def _offset_interp_spans(node, base: Span) -> None:
    """Shift every span in a re-parsed interpolation subtree from fragment-relative
    to file-absolute. The fragment's (1,1) maps to (base.line, base.col); a fragment
    line L>1 aligns to file line base.line + L - 1 at the SAME column (the captured
    fragment preserves post-newline columns verbatim)."""
    import dataclasses
    if isinstance(node, (list, tuple)):
        for it in node:
            _offset_interp_spans(it, base)
        return
    if not dataclasses.is_dataclass(node) or isinstance(node, type):
        return
    for f in dataclasses.fields(node):
        val = getattr(node, f.name)
        if isinstance(val, Span):
            if val.line == 1:
                setattr(node, f.name, Span(base.line, base.col + val.col - 1, val.length))
            else:
                setattr(node, f.name, Span(base.line + val.line - 1, val.col, val.length))
        elif isinstance(val, (list, tuple)):
            _offset_interp_spans(val, base)
        elif dataclasses.is_dataclass(val) and not isinstance(val, type):
            _offset_interp_spans(val, base)


def parse_expr_source(src: str, filename: str, span: Span):
    """Parse a single expression from interpolation source."""
    source = Source(src, filename)
    tokens = Lexer(source).tokenize()
    p = Parser(tokens, source)
    node = p.expr()
    if not p.check(T.EOF):
        p.error("unexpected trailing tokens in interpolation")
    _offset_interp_spans(node, span)
    return node
