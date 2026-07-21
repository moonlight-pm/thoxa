# Thoxa VSCode Extension

This extension provides syntax highlighting and basic language support for Thoxa (.thx) files.

## Features

- Syntax highlighting for:
  - Keywords (fn, type, if, match, etc.)
  - Types (Int, Float, String, etc.)
  - Strings with interpolation support
  - Numbers (including binary/hex literals)
  - Comments
  - Operators

- Auto-closing pairs for brackets, quotes
- Indentation rules
- Folding support

## Installation

1. Copy this folder to `~/.vscode/extensions/thoxa-0.0.1`
2. Restart VSCode
3. Open any .thx file to see syntax highlighting

## Development

To modify the extension:
1. Edit `syntaxes/thoxa.tmLanguage.json` for syntax rules
2. Edit `language-configuration.json` for bracket matching, etc.
3. Reload VSCode to see changes