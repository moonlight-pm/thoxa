"""Command-line interface for Thoxa interpreter."""

import sys
from pathlib import Path
import click
from rich.console import Console
from rich.syntax import Syntax
from rich.traceback import install
from .parser import parse
from .interpreter import Interpreter

# Install rich traceback for better error messages
install()
console = Console()


@click.command()
@click.argument('file', required=False, type=click.Path(exists=True))
@click.option('--ast', is_flag=True, help='Show AST instead of executing')
@click.option('--debug', is_flag=True, help='Enable debug mode')
def main(file, ast, debug):
    """Thoxa programming language interpreter.
    
    Run a Thoxa file or start an interactive REPL.
    """
    if file:
        run_file(file, show_ast=ast, debug=debug)
    else:
        run_repl(debug=debug)


def run_file(filename: str, show_ast: bool = False, debug: bool = False):
    """Run a Thoxa file."""
    try:
        source = Path(filename).read_text()
        
        # Parse
        ast_tree = parse(source)
        
        if show_ast:
            console.print(ast_tree)
            return
        
        # Interpret
        interpreter = Interpreter()
        result = interpreter.interpret(ast_tree)
        
        if debug and result is not None:
            console.print(f"[dim]Result:[/dim] {result}")
        
        # Use result as exit code if it's an integer
        if isinstance(result, int):
            sys.exit(result)
            
    except FileNotFoundError:
        console.print(f"[red]Error:[/red] File '{filename}' not found")
        sys.exit(1)
    except Exception as e:
        if debug:
            console.print_exception()
        else:
            console.print(f"[red]Error:[/red] {e}")
        sys.exit(1)


def run_repl(debug: bool = False):
    """Run the interactive REPL."""
    from .repl import REPL
    repl = REPL(debug=debug)
    repl.run()


if __name__ == "__main__":
    main()