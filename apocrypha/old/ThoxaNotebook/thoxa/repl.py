"""Interactive REPL for Thoxa."""

import sys
import readline
import atexit
from pathlib import Path
from rich.console import Console
from rich.prompt import Prompt
from rich.syntax import Syntax
from .parser import parse
from .interpreter import Interpreter, ReturnException

console = Console()


class REPL:
    """Interactive Read-Eval-Print Loop for Thoxa."""
    
    def __init__(self, debug: bool = False):
        self.debug = debug
        self.interpreter = Interpreter()
        self.history = []
        self.multiline_buffer = []
        self._setup_history()
    
    def _setup_history(self):
        """Set up readline history."""
        history_file = Path.home() / ".thoxa_history"
        
        try:
            readline.read_history_file(history_file)
        except (FileNotFoundError, PermissionError):
            pass
        
        try:
            atexit.register(lambda: readline.write_history_file(history_file))
        except (PermissionError, OSError):
            pass  # Ignore if we can't write history
    
    def run(self):
        """Run the REPL."""
        console.print("[bold green]Thoxa REPL v0.1.0[/bold green]")
        console.print("Type 'exit' or Ctrl+D to quit, 'clear' to clear screen")
        console.print()
        
        while True:
            try:
                # Get input
                if self.multiline_buffer:
                    prompt = "... "
                else:
                    prompt = ">>> "
                
                line = Prompt.ask(prompt)
                
                # Handle special commands
                if not self.multiline_buffer and line in ["exit", "quit"]:
                    console.print("[dim]Goodbye![/dim]")
                    break
                elif not self.multiline_buffer and line == "clear":
                    console.clear()
                    continue
                elif not self.multiline_buffer and line == "help":
                    self._show_help()
                    continue
                
                # Handle multiline input
                if self._is_incomplete(line):
                    self.multiline_buffer.append(line)
                    continue
                
                # Combine multiline buffer
                if self.multiline_buffer:
                    self.multiline_buffer.append(line)
                    line = "\n".join(self.multiline_buffer)
                    self.multiline_buffer = []
                
                # Skip empty lines
                if not line.strip():
                    continue
                
                # Process the input
                self._process_line(line)
                
            except EOFError:
                console.print("\n[dim]Goodbye![/dim]")
                break
            except KeyboardInterrupt:
                console.print("\n[dim]Use 'exit' or Ctrl+D to quit[/dim]")
                self.multiline_buffer = []
                continue
    
    def _is_incomplete(self, line: str) -> bool:
        """Check if input is incomplete (needs more lines)."""
        # Simple heuristic: check for unmatched braces
        open_braces = line.count('{') - line.count('}')
        open_parens = line.count('(') - line.count(')')
        open_brackets = line.count('[') - line.count(']')
        
        return (open_braces > 0 or open_parens > 0 or open_brackets > 0 or 
                line.endswith('\\'))
    
    def _process_line(self, line: str):
        """Process a line of input."""
        try:
            # Try to parse as an expression first
            if not any(line.strip().startswith(kw) for kw in 
                      ['let', 'fn', 'type', 'if', 'loop', 'while', 'for', 'return']):
                # Wrap in print for expressions
                display_line = f"println({line})"
                try:
                    ast = parse(display_line)
                    self.interpreter.interpret(ast)
                    return
                except:
                    pass  # Fall through to parse as statement
            
            # Parse as statement
            ast = parse(line)
            
            if self.debug:
                console.print("[dim]AST:[/dim]")
                console.print(ast)
            
            # Interpret
            result = self.interpreter.interpret(ast)
            
            # Don't print None results
            if result is not None:
                console.print(result)
                
        except ReturnException as e:
            # Return outside function
            console.print("[yellow]Warning:[/yellow] Return statement outside function")
            if e.value is not None:
                console.print(e.value)
        except Exception as e:
            if self.debug:
                console.print_exception()
            else:
                console.print(f"[red]Error:[/red] {e}")
    
    def _show_help(self):
        """Show help information."""
        help_text = """
[bold]Thoxa REPL Commands:[/bold]
  exit, quit    Exit the REPL
  clear         Clear the screen
  help          Show this help message
  
[bold]Language Features:[/bold]
  let x = 42                    Variable declaration
  let mut y = 10                Mutable variable
  fn add(a, b) { a + b }        Function definition
  [1, 2, 3]                     Arrays
  if x > 0 { "pos" } else { "neg" }  Conditionals
  loop { ... break ... }        Loops
  
[bold]Examples:[/bold]
  5 + 3 * 2                     Arithmetic (prints: 11)
  let nums = [1, 2, 3, 4, 5]    Array
  nums[0]                       Indexing (prints: 1)
  nums.count                    Array length (prints: 5)
  
Type expressions to evaluate them, or statements to execute them.
"""
        console.print(help_text)