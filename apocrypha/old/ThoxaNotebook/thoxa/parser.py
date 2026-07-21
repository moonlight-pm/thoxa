"""Lark-based parser for Thoxa language."""

from lark import Lark, Transformer, v_args, Token
from .ast_nodes import *

# Thoxa grammar in Lark's EBNF format
GRAMMAR = r"""
?start: program

program: statement*

?statement: let_stmt
          | mut_stmt
          | function_def
          | type_def
          | loop_stmt
          | while_stmt
          | for_stmt
          | return_stmt
          | break_stmt
          | continue_stmt
          | simple_stmt

let_stmt: "let" "mut"? IDENTIFIER (":" type)? "=" expression ";"?
mut_stmt: "mut" IDENTIFIER (":" type)? "=" expression ";"?
function_def: "fn" IDENTIFIER "(" params? ")" ("->" type)? block
type_def: "type" IDENTIFIER ("(" constructor_params? ")")? type_body
loop_stmt: "loop" block ";"?
while_stmt: "while" expression block ";"?
for_stmt: "for" IDENTIFIER "in" expression block ";"?
return_stmt: "return" expression? ";"?
break_stmt: "break" ";"?
continue_stmt: "continue" ";"?

// Try to parse as assignment first, then fall back to expression
simple_stmt: IDENTIFIER "=" expression ";"?                -> simple_assignment
           | IDENTIFIER "+=" expression ";"?               -> compound_add_assignment
           | IDENTIFIER "-=" expression ";"?               -> compound_sub_assignment
           | IDENTIFIER "*=" expression ";"?               -> compound_mul_assignment
           | IDENTIFIER "/=" expression ";"?               -> compound_div_assignment
           | postfix_expr "." IDENTIFIER "=" expression ";"?  -> member_assignment
           | postfix_expr "[" expression "]" "=" expression ";"?  -> index_assignment
           | expression ";"?                          -> expr_stmt

block: "{" statement* "}"
type_body: "{" type_member* "}"
type_member: "export" "mut" IDENTIFIER (":" type)? ("=" expression)? ","?   -> export_mut_member
           | "export" "let" IDENTIFIER (":" type)? ("=" expression)? ","?   -> export_let_member
           | "export" function_def ","?                                     -> export_method_member

params: param ("," param)*
param: IDENTIFIER "!"? (":" type)? ("=" expression)?

constructor_params: constructor_param ("," constructor_param)*
constructor_param: IDENTIFIER (":" type)? ("=" expression)?

type: IDENTIFIER ("<" type_list ">")?
type_list: type ("," type)*

?expression: or_expr

?or_expr: and_expr (OR and_expr)*
?and_expr: equality_expr (AND equality_expr)*
?equality_expr: comparison_expr ((EQ | NE) comparison_expr)*
?comparison_expr: additive_expr ((LT | GT | LE | GE) additive_expr)*
?additive_expr: multiplicative_expr ((PLUS | MINUS) multiplicative_expr)*
?multiplicative_expr: unary_expr ((STAR | SLASH | PERCENT) unary_expr)*

OR: "or"
AND: "and"
EQ: "=="
NE: "!="
LT: "<"
GT: ">"
LE: "<="
GE: ">="
PLUS: "+"
MINUS: "-"
STAR: "*"
SLASH: "/"
PERCENT: "%"

?unary_expr: "not" unary_expr  -> not_expr
           | "-" unary_expr     -> neg_expr
           | postfix_expr

?postfix_expr: primary_expr
             | postfix_expr "(" args? ")"           -> call
             | postfix_expr "[" expression "]"      -> index
             | postfix_expr "." IDENTIFIER          -> member

?primary_expr: literal
             | IDENTIFIER               -> identifier
             | "(" expression ")"
             | array_literal
             | if_expr
             | match_expr
             | block

if_expr: "if" expression block ("else" "if" expression block)* ("else" block)?
match_expr: "match" expression "{" match_arm* "}" ("else" block)?
match_arm: pattern "{" statement* expression? "}"
pattern: IDENTIFIER ":" IDENTIFIER  -> type_pattern
       | literal                    -> literal_pattern
       | IDENTIFIER                 -> bind_pattern

array_literal: "[" (expression ("," expression)*)? "]"

args: arg ("," arg)*
arg: (IDENTIFIER ":")? expression

literal: NUMBER         -> number
       | STRING         -> string
       | "true"         -> true
       | "false"        -> false

IDENTIFIER: /[a-zA-Z_][a-zA-Z0-9_]*/
NUMBER: /\d+(\.\d+)?([eE][+-]?\d+)?/
STRING: /"([^"\\]|\\.)*"/

COMMENT: /#[^\n]*/

%import common.WS
%ignore WS
%ignore COMMENT
"""


@v_args(inline=True)
class ThoxaTransformer(Transformer):
    """Transform Lark parse tree into Thoxa AST."""
    
    def program(self, *statements):
        return Program(list(statements))
    
    def let_stmt(self, *args):
        mutable = False
        name = None
        type_annotation = None
        value = None
        
        for arg in args:
            if isinstance(arg, Token):
                if arg.type == 'IDENTIFIER':
                    if name is None:
                        name = str(arg)
            elif isinstance(arg, str):
                if arg == "mut":
                    mutable = True
            elif isinstance(arg, ASTNode):
                value = arg
            # TODO: Handle type annotations
        
        return Let(name=name, value=value, mutable=mutable, type_annotation=type_annotation)
    
    def mut_stmt(self, *args):
        """Handle standalone mut declarations."""
        name = None
        type_annotation = None
        value = None
        
        for arg in args:
            if isinstance(arg, Token):
                if arg.type == 'IDENTIFIER':
                    if name is None:
                        name = str(arg)
            elif isinstance(arg, ASTNode):
                value = arg
            # TODO: Handle type annotations
        
        return Let(name=name, value=value, mutable=True, type_annotation=type_annotation)
    
    def function_def(self, name, *args):
        params = []
        body = None
        return_type = None
        
        for arg in args:
            if isinstance(arg, Block):
                body = arg
            elif isinstance(arg, list):  # params
                params = arg
            elif isinstance(arg, Token) and arg.type == 'IDENTIFIER':
                # This is the return type
                return_type = str(arg)
            elif isinstance(arg, str):
                # This is the return type from the type transformer
                return_type = arg
        
        return Function(name=str(name), params=params, body=body, return_type=return_type)
    
    def type_def(self, name, *args):
        """Handle type definition."""
        params = []
        body = None
        
        for arg in args:
            if isinstance(arg, Block):
                body = arg
            elif isinstance(arg, list):  # constructor params
                params = arg
        
        return TypeDef(name=str(name), constructor_params=params, body=body)
    
    def type_body(self, *members):
        """Handle type body with members."""
        # For now, just return a Block with the members as statements
        return Block(list(members))
    
    def export_flag(self):
        """Handle export flag."""
        return "export"
    
    def mut_flag(self):
        """Handle mut flag."""
        return "mut"
    
    def export_mut_member(self, *args):
        """Handle export mut field."""
        name = None
        type_annotation = None
        value = None
        
        for arg in args:
            if isinstance(arg, Token) and arg.type == 'IDENTIFIER':
                if name is None:
                    name = str(arg)
            elif isinstance(arg, str):
                # Type annotation
                type_annotation = arg
            elif isinstance(arg, ASTNode):
                value = arg
        
        return ('field', name, type_annotation, value, True, True)  # is_mutable=True, is_export=True
    
    def export_let_member(self, *args):
        """Handle export let field."""
        name = None
        type_annotation = None
        value = None
        
        for arg in args:
            if isinstance(arg, Token) and arg.type == 'IDENTIFIER':
                if name is None:
                    name = str(arg)
            elif isinstance(arg, str):
                # Type annotation
                type_annotation = arg
            elif isinstance(arg, ASTNode):
                value = arg
        
        return ('field', name, type_annotation, value, False, True)  # is_mutable=False, is_export=True
    
    def export_method_member(self, method):
        """Handle export method."""
        return ('method', method)
    
    def params(self, *params):
        """Handle function parameters."""
        return list(params)
    
    def constructor_params(self, *params):
        """Handle constructor parameters."""
        return list(params)
    
    def constructor_param(self, *args):
        """Handle a single constructor parameter."""
        name = None
        type_annotation = None
        default_value = None
        
        for arg in args:
            if isinstance(arg, Token) and arg.type == 'IDENTIFIER':
                if name is None:
                    name = str(arg)
            elif isinstance(arg, str):
                # This is the type
                type_annotation = arg
            elif isinstance(arg, ASTNode):
                default_value = arg
        
        return (name, type_annotation, default_value)
    
    def param(self, *args):
        """Handle a single function parameter."""
        name = None
        type_annotation = None
        default_value = None
        
        for arg in args:
            if isinstance(arg, Token) and arg.type == 'IDENTIFIER':
                if name is None:
                    name = str(arg)
                else:
                    # This is the type
                    type_annotation = str(arg)
            elif isinstance(arg, ASTNode):
                default_value = arg
            elif isinstance(arg, str):
                # This is the type returned by the type transformer
                type_annotation = arg
        
        # Return a tuple of (name, type, default)
        return (name, type_annotation, default_value)
    
    def type(self, *args):
        """Handle type annotations."""
        # For now, just return the type name as a string
        for arg in args:
            if isinstance(arg, Token) and arg.type == 'IDENTIFIER':
                return str(arg)
        return None
    
    def block(self, *statements):
        return Block(list(statements))
    
    def if_expr(self, condition, then_branch, *rest):
        else_branch = None
        if rest and isinstance(rest[-1], Block):
            else_branch = rest[-1]
        return If(condition, then_branch, else_branch)
    
    def loop_stmt(self, body):
        return Loop(body)
    
    def while_stmt(self, condition, body):
        return While(condition, body)
    
    def for_stmt(self, var, iterable, body):
        return For(str(var), iterable, body)
    
    def return_stmt(self, value=None):
        return Return(value)
    
    def break_stmt(self):
        return Break()
    
    def continue_stmt(self):
        return Continue()
    
    def simple_assignment(self, name, value):
        """Handle simple identifier assignment."""
        return Assignment(Identifier(str(name)), value)
    
    def compound_add_assignment(self, name, value):
        """Handle += compound assignment."""
        var_ref = Identifier(str(name))
        add_expr = BinaryOp(var_ref, '+', value)
        return Assignment(Identifier(str(name)), add_expr)
    
    def compound_sub_assignment(self, name, value):
        """Handle -= compound assignment."""
        var_ref = Identifier(str(name))
        sub_expr = BinaryOp(var_ref, '-', value)
        return Assignment(Identifier(str(name)), sub_expr)
    
    def compound_mul_assignment(self, name, value):
        """Handle *= compound assignment."""
        var_ref = Identifier(str(name))
        mul_expr = BinaryOp(var_ref, '*', value)
        return Assignment(Identifier(str(name)), mul_expr)
    
    def compound_div_assignment(self, name, value):
        """Handle /= compound assignment."""
        var_ref = Identifier(str(name))
        div_expr = BinaryOp(var_ref, '/', value)
        return Assignment(Identifier(str(name)), div_expr)
    
    def member_assignment(self, obj, member, value):
        """Handle member assignment."""
        return Assignment(Member(obj, str(member)), value)
    
    def index_assignment(self, obj, idx, value):
        """Handle index assignment."""
        return Assignment(Index(obj, idx), value)
    
    def expr_stmt(self, expr):
        return expr
    
    # Expressions
    
    def identifier(self, name):
        return Identifier(str(name))
    
    def number(self, n):
        value = float(n) if '.' in n or 'e' in n.lower() else int(n)
        return Literal(value)
    
    def string(self, s):
        # Remove quotes and handle escape sequences
        value = s[1:-1].encode().decode('unicode_escape')
        return Literal(value)
    
    def true(self):
        return Literal(True)
    
    def false(self):
        return Literal(False)
    
    def array_literal(self, *elements):
        return Array(list(elements))
    
    def args(self, *args):
        """Handle function arguments."""
        return list(args)
    
    def arg(self, *items):
        """Handle a single function argument."""
        # For now, just return the expression part
        for item in items:
            if isinstance(item, ASTNode):
                return item
        return items[-1] if items else None
    
    def call(self, func, args=None):
        args_list = []
        if args:
            if isinstance(args, list):
                args_list = args
            else:
                args_list = [args]
        return Call(func, args_list)
    
    def index(self, obj, idx):
        return Index(obj, idx)
    
    def member(self, obj, member):
        return Member(obj, str(member))
    
    def not_expr(self, operand):
        return UnaryOp("not", operand)
    
    def neg_expr(self, operand):
        return UnaryOp("-", operand)
    
    def additive_expr(self, *args):
        """Handle addition and subtraction."""
        if len(args) == 1:
            return args[0]
        result = args[0]
        for i in range(1, len(args), 2):
            op = str(args[i]) if i < len(args) else None
            right = args[i+1] if i+1 < len(args) else None
            if op and right:
                result = BinaryOp(result, op, right)
        return result
    
    def multiplicative_expr(self, *args):
        """Handle multiplication, division, modulo."""
        if len(args) == 1:
            return args[0]
        result = args[0]
        for i in range(1, len(args), 2):
            op = str(args[i]) if i < len(args) else None
            right = args[i+1] if i+1 < len(args) else None
            if op and right:
                result = BinaryOp(result, op, right)
        return result
    
    def or_expr(self, *args):
        """Handle logical OR."""
        if len(args) == 1:
            return args[0]
        result = args[0]
        for i in range(1, len(args), 2):
            right = args[i] if i < len(args) else None
            if right:
                result = BinaryOp(result, "or", right)
        return result
    
    def and_expr(self, *args):
        """Handle logical AND."""
        if len(args) == 1:
            return args[0]
        result = args[0]
        for i in range(1, len(args), 2):
            right = args[i] if i < len(args) else None
            if right:
                result = BinaryOp(result, "and", right)
        return result
    
    def equality_expr(self, *args):
        """Handle equality operators."""
        if len(args) == 1:
            return args[0]
        result = args[0]
        for i in range(1, len(args), 2):
            op = str(args[i]) if i < len(args) else None
            right = args[i+1] if i+1 < len(args) else None
            if op and right:
                result = BinaryOp(result, op, right)
        return result
    
    def comparison_expr(self, *args):
        """Handle comparison operators."""
        if len(args) == 1:
            return args[0]
        result = args[0]
        for i in range(1, len(args), 2):
            op = str(args[i]) if i < len(args) else None
            right = args[i+1] if i+1 < len(args) else None
            if op and right:
                result = BinaryOp(result, op, right)
        return result


# Parser instance
parser = Lark(GRAMMAR, parser='lalr', transformer=ThoxaTransformer())


def parse(source: str) -> Program:
    """Parse Thoxa source code and return AST."""
    try:
        ast = parser.parse(source)
        
        # Type check the AST
        from .type_checker import TypeChecker
        checker = TypeChecker()
        if not checker.check_program(ast):
            errors = checker.get_errors()
            error_msg = "Type errors:\n" + "\n".join(f"  - {e}" for e in errors)
            raise TypeError(error_msg)
        
        return ast
    except TypeError:
        # Re-raise type errors as-is
        raise
    except Exception as e:
        # Replace tabs with spaces in error messages
        error_msg = str(e).replace('\t', '    ')
        # Create a new exception with the modified message
        new_error = Exception(error_msg)
        raise new_error from None