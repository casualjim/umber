# umber

A modern replacement for `cat` with syntax highlighting and automatic language detection.

## Features

- **Syntax highlighting** for 100+ languages using tree-sitter parsers
- **Automatic language detection** based on file extension and content
- **Line numbers** with `--style=numbers`
- **Git change indicators** showing added, modified, and removed lines with `--style=changes`
- **Rich highlighting** (language injections / embedded languages) with `--style=rich`
- **Show unprintable characters** with `-A` / `--show-all` (tabs as →, carriage returns as ↵, etc.) - **unlike bat, syntax highlighting is preserved!**
- **Theme support** with automatic dark/light mode detection
- **Stdin support** for piping commands
- **Fast** - built with Rust and tree-sitter
- **Shell completions** for bash, zsh, fish, and more

## Installation

### From source

```bash
cargo install --path .
```

Or using `mise` (recommended for development):

```bash
mise install
mise build:debug
```

## Usage

### Getting help

```bash
# Quick help
umber -h

# Detailed help with examples
umber --help

# View man page (after installation)
man umber
```

### Basic usage

Display a file with syntax highlighting:

```bash
umber main.rs
```

Display multiple files:

```bash
umber main.rs lib.rs
```

Show unprintable characters (tabs as →, line feeds as ␊):

```bash
umber -A main.rs
```

Read from stdin:

```bash
echo 'fn main() { println!("Hello!"); }' | umber
# or explicitly:
cat main.rs | umber -
```

Page through `less` with colors preserved:

```bash
umber --color always main.rs | less -R

# or let umber launch the pager for you
umber --pager main.rs
```

### Line numbers

Show line numbers with the `numbers` style component:

```bash
umber --style=numbers main.rs
```

### Decorations (line numbers, git changes)

Control which decorations to display with the `--style` flag:

```bash
# Show line numbers with grid separator
umber --style=numbers main.rs

# Show git change indicators (+, ~, -)
umber --style=changes main.rs

# Combine multiple decorations
umber --style=numbers,changes main.rs

# Enable richer highlighting (language injections / embedded languages)
umber --style=rich main.rs
```

Note: `--style=rich` can be significantly slower on very large files.

Git change indicators show:
- `+` (green) - added lines
- `~` (yellow) - modified lines
- `-` (red) - removed lines

### Show unprintable characters

Display tabs, carriage returns, line feeds, and other non-printable characters with `-A` / `--show-all`:

```bash
umber -A main.rs
```

This shows:
- `·` (middle dot) for spaces
- `→` (right arrow) for tabs
- `␊` (line feed symbol) at the end of lines
- `↵` (carriage return symbol) for `\r`
- `␛` (escape symbol) for escape characters
- `␀`, `␁`, `␂`, etc. for other control characters

**Unlike `bat -A`**, umber maintains full syntax highlighting while showing unprintable characters!

```bash
# Combine with line numbers
umber -A -n main.rs

# Works with git change indicators too
umber -A --style=changes,numbers main.rs
```

### Language override

Force a specific language when auto-detection fails:

```bash
umber --language rust config.txt
umber --language json response.log
```

To see all supported languages, check the [syntastica documentation](https://docs.rs/syntastica-parsers/latest/syntastica_parsers/).

### Themes

Specify a theme with `--theme`:

```bash
umber --theme dracula main.rs
umber --theme gruvbox-light main.rs
```

By default, `umber` uses `auto` which detects your system's dark/light mode preference:
- **Light mode**: Catppuccin Latte
- **Dark mode**: Catppuccin Mocha

#### Available themes

See the full list of themes in the [syntastica-themes documentation](https://docs.rs/syntastica-themes/latest/syntastica_themes/).

Popular themes include:
- `dracula`
- `gruvbox-dark` / `gruvbox-light`
- `nord`
- `one-dark` / `one-light`
- `catppuccin-mocha` / `catppuccin-latte` / `catppuccin-frappe` / `catppuccin-macchiato`
- `solarized-dark` / `solarized-light`
- `tokyo-night`

### Paging

When you pipe into a pager manually, force ANSI colors and tell `less` to pass them through:

```bash
umber --color always src/main.rs | less -R
```

`umber` also has a built-in pager mode that uses `$PAGER` when set, or falls back to `less -R`:

```bash
umber --pager src/main.rs
umber --style=numbers,changes --pager Cargo.toml
```

### Shell completions

Generate shell completions for your shell:

```bash
# Bash
umber --completions bash > ~/.local/share/bash-completion/completions/umber

# Zsh
umber --completions zsh > ~/.zsh/completion/_umber

# Fish
umber --completions fish > ~/.config/fish/completions/umber.fish

# PowerShell
umber --completions powershell > umber.ps1
```

### Man page

Generate and install a man page:

```bash
# Generate man page
umber --man-page > umber.1

# Install system-wide (requires sudo)
sudo cp umber.1 /usr/local/share/man/man1/

# View the man page
man umber
```

## Examples

```bash
# View a Rust file with line numbers
umber --style=numbers src/main.rs

# View JSON with syntax highlighting
umber package.json

# Pipe git diff through umber
git diff | umber --language diff

# Compare files side by side with syntax highlighting
diff <(umber file1.js) <(umber file2.js)

# View logs with Python syntax highlighting
umber --language python app.log

# Use a specific theme
umber --theme nord config.yaml

# View multiple files with line numbers
umber --style=numbers *.rs

# Show unprintable characters (tabs, line feeds, etc.) with syntax highlighting
umber -A main.rs

# Debug a file with mixed line endings
umber -A --style=numbers problem_file.txt
```

## Supported Languages

`umber` supports 100+ programming languages through tree-sitter parsers, including:

- C, C++, C#, Objective-C
- Rust, Go, Zig
- JavaScript, TypeScript, JSX, TSX
- Python, Ruby, Perl, PHP
- Java, Kotlin, Scala
- Swift, Dart
- HTML, CSS, SCSS, Vue
- Bash, Fish, PowerShell
- SQL, GraphQL
- YAML, TOML, JSON, XML
- Markdown, reStructuredText
- And many more...

For a complete list, see the [syntastica-parsers documentation](https://docs.rs/syntastica-parsers/latest/syntastica_parsers/).

## Why umber?

- **Modern syntax highlighting** using tree-sitter for accurate, grammar-aware highlighting
- **Show unprintable characters with colors** - unique feature that maintains highlighting while showing tabs, line feeds, and control characters
- **Smart defaults** with automatic theme and language detection
- **Familiar interface** - works just like `cat` but with colors
- **Fast and reliable** - built with Rust for performance and safety
- **No configuration needed** - works out of the box with sensible defaults

## Comparison with other tools

| Feature | umber | bat | ccat | highlight |
|---------|------|-----|------|-----------|
| Syntax highlighting | ✅ Tree-sitter | ✅ syntect | ✅ pygments | ✅ |
| Auto language detection | ✅ | ✅ | ✅ | ✅ |
| Line numbers | ✅ | ✅ | ❌ | ✅ |
| Themes | ✅ Many | ✅ Many | ✅ Few | ✅ Many |
| Git integration | ✅ | ✅ | ❌ | ❌ |
| Show unprintable chars | ✅ **with colors** | ✅ *no colors* | ❌ | ❌ |
| Paging | ✅ | ✅ | ❌ | ❌ |
| Binary files | ❌ | ✅ | ❌ | ❌ |

**Key difference:** umber shows unprintable characters (tabs, line feeds, control chars) **while maintaining syntax highlighting** - unlike `bat -A` which disables colors.

`umber` focuses on being a simple, fast `cat` replacement with excellent syntax highlighting and git change indicators. If you need heavier features like binary file detection, check out [bat](https://github.com/sharkdp/bat).

## Development

This project uses `mise` for development:

```bash
# Install dependencies
mise install

# Build
mise build:debug

# Run tests
mise test

# Format code
mise format
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

MIT License - see [LICENSE](LICENSE) file for details.

## Acknowledgments

- [syntastica](https://github.com/RubixDev/syntastica) - Syntax highlighting library
- [tree-sitter](https://tree-sitter.github.io/) - Parser generator tool
- [clap](https://github.com/clap-rs/clap) - Command line argument parser
