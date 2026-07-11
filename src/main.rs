mod decorations;
mod git;
mod unprintable;

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::str::FromStr;

use clap::{CommandFactory, Parser, ValueEnum};
use dark_light::Mode as DarkLightMode;
use decorations::DecorationConfig;
use eyre::{Result, eyre};
use syntastica::language_set::{LanguageSet, SupportedLanguage};
use syntastica::renderer::{Renderer, TerminalRenderer};
use syntastica::theme::{ResolvedTheme, THEME_KEYS};
use syntastica_highlight::{Highlight, HighlightConfiguration, HighlightEvent, Highlighter};
use syntastica_parsers_git::{Lang, LanguageSetImpl};

const STREAM_OUTPUT_BUFFER_BYTES: usize = 64 * 1024;
const STREAM_OUTPUT_FLUSH_BYTES: usize = 8 * 1024;

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum ColorWhen {
  Auto,
  Never,
  Always,
}

#[derive(Parser, Debug)]
#[command(
  name = "umber",
  version,
  about = "cat with syntax highlighting",
  long_about = "A modern replacement for cat with syntax highlighting powered by tree-sitter.\n\
                Automatically detects file types and applies appropriate syntax highlighting.\n\
                Supports 100+ programming languages and multiple color themes.",
  after_help = "EXAMPLES:\n    \
    umber main.rs                    Display a file with syntax highlighting\n    \
    umber --style=numbers config.toml  Show file with line numbers\n    \
    umber main.rs#L10-L20            Show only selected lines\n    \
    umber --language rust file.txt   Force Rust syntax highlighting\n    \
    umber --theme dracula main.js    Use Dracula color theme\n    \
    cat file.rs | umber              Read from stdin\n    \
    umber *.py                       Display multiple files\n\n\
    For available themes, see: https://docs.rs/syntastica-themes/latest/syntastica_themes/\n\n\
    To generate shell completions:\n    \
    umber --completions bash > ~/.local/share/bash-completion/completions/umber"
)]
struct Cli {
  #[arg(
    long = "completions",
    alias = "completion",
    value_enum,
    help = "Generate shell completions for the specified shell",
    long_help = "Generate shell completion script for the specified shell.\n\
                 Output the completion script to stdout, which you can then save to the\n\
                 appropriate location for your shell.\n\n\
                 Examples:\n  \
                 umber --completions bash > ~/.local/share/bash-completion/completions/umber\n  \
                 umber --completions zsh > ~/.zsh/completion/_umber\n  \
                 umber --completions fish > ~/.config/fish/completions/umber.fish"
  )]
  completions: Option<clap_complete::Shell>,

  #[arg(
    long,
    short = 'l',
    value_name = "LANG",
    help = "Force a specific programming language",
    long_help = "Override automatic language detection and force a specific language.\n\
                 Useful when the file extension doesn't match the content or for files\n\
                 without extensions.\n\n\
                 Examples:\n  \
                 umber --language rust config.txt\n  \
                 umber --language json response.log\n\n\
                 For a complete list of supported languages, see:\n\
                 https://docs.rs/syntastica-parsers-git/latest/syntastica_parsers_git/"
  )]
  language: Option<String>,

  #[arg(
    long,
    value_name = "THEME",
    default_value = "auto",
    help = "Color theme to use for syntax highlighting",
    long_help = "Specify a color theme for syntax highlighting.\n\n\
                 Use 'auto' (default) to automatically detect light/dark mode:\n  \
                 - Light mode: catppuccin-latte\n  \
                 - Dark mode: catppuccin-mocha\n\n\
                 Popular themes include:\n  \
                 dracula, nord, one-dark, one-light, gruvbox-dark, gruvbox-light,\n  \
                 solarized-dark, solarized-light, tokyo-night, catppuccin-mocha,\n  \
                 catppuccin-latte, catppuccin-frappe, catppuccin-macchiato\n\n\
                 For a complete list of available themes, see:\n\
                 https://docs.rs/syntastica-themes/latest/syntastica_themes/"
  )]
  theme: String,

  #[arg(
    long,
    short = 'n',
    value_name = "RANGE",
    help = "Show only selected lines (e.g. 10-20, 10:20, 10,20, 10)",
    long_help = "Show only selected lines from the file.\n\
                 Accepted formats: start-end, start:end, start,end, or a single line number.\n\
                 Examples:\n  \
                 umber --lines 10-20 main.rs\n  \
                 umber --lines 10:20 main.rs\n  \
                 umber --lines 10,20 main.rs\n  \
                 umber --lines 10 main.rs"
  )]
  lines: Option<String>,

  #[arg(
    long,
    value_enum,
    default_value = "auto",
    help = "Specify when to use colored output",
    long_help = "Specify when to use colored output.\n\n                 Use 'always' when piping into a pager manually, for example:\n                   umber --color always main.rs | less -R"
  )]
  color: ColorWhen,

  #[arg(long, help = "Disable colored output")]
  no_color: bool,

  #[arg(long, help = "List supported themes")]
  list_themes: bool,

  #[arg(
    long,
    short = 's',
    help = "Squeeze consecutive empty lines into a single empty line"
  )]
  squeeze_blank: bool,

  #[arg(
    long,
    value_name = "squeeze-limit",
    help = "Set the maximum number of consecutive empty lines"
  )]
  squeeze_limit: Option<usize>,

  #[arg(
    long,
    value_name = "components",
    help = "Style components (numbers, changes, headers, rich)"
  )]
  style: Option<String>,

  #[arg(long, short = 'u', help = "No-op, output is always unbuffered")]
  unbuffered: bool,

  #[arg(
    long,
    help = "Page output through less or $PAGER",
    long_help = "Page output through a pager while preserving ANSI colors.\n\n                 Uses the `PAGER` environment variable when set, otherwise falls back to `less -R`.\n                 When the pager command is `less`, `-R` is added automatically if needed.\n\n                 Examples:\n                   umber --pager src/main.rs\n                   umber --style=numbers,changes --pager Cargo.toml"
  )]
  pager: bool,

  #[arg(
    long,
    short = 'A',
    help = "Show unprintable characters (tabs as →, carriage returns as ↵, etc.)"
  )]
  show_all: bool,

  #[arg(
    long,
    help = "Generate man page",
    long_help = "Generate a manual page in roff format and print to stdout.\n\
                 You can save this to a file and install it in your man path.\n\n\
                 Example:\n  \
                 umber --man-page > umber.1\n  \
                 sudo cp umber.1 /usr/local/share/man/man1/"
  )]
  man_page: bool,

  #[arg(
    value_name = "FILE",
    help = "Files to display (use '-' or omit for stdin)",
    long_help = "One or more files to display with syntax highlighting.\n\
                 If no files are specified, or if '-' is given, reads from stdin.\n\n\
                 Examples:\n  \
                 umber main.rs lib.rs\n  \
                 umber main.rs#L10-L20\n  \
                 cat file.rs | umber\n  \
                 echo 'code' | umber --language rust"
  )]
  files: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug)]
struct LineRange {
  start: usize,
  end: usize,
}

#[derive(Clone, Debug)]
struct FileSpec {
  path: PathBuf,
  line_range: Option<LineRange>,
}

#[derive(Clone, Copy, Debug, Default)]
struct StyleConfig {
  decoration_config: DecorationConfig,
  highlight_locals: bool,
  highlight_injections: bool,
}

struct RenderContext<'a> {
  decoration_config: DecorationConfig,
  highlight_locals: bool,
  highlight_injections: bool,
  use_color: bool,
  squeeze_blank: bool,
  squeeze_limit: usize,
  show_all: bool,
  language_set: &'a LanguageSetImpl,
  theme: &'a ResolvedTheme,
}

struct RenderState {
  highlighter: Highlighter,
  highlights_only_configs: HashMap<Lang, HighlightConfiguration>,
  locals_configs: HashMap<Lang, HighlightConfiguration>,
  renderer: TerminalRenderer,
}

impl RenderState {
  fn new() -> Self {
    Self {
      highlighter: Highlighter::new(),
      highlights_only_configs: HashMap::new(),
      locals_configs: HashMap::new(),
      renderer: TerminalRenderer::new(None),
    }
  }
}

struct DecorationsStreamSettings<'a> {
  decoration_config: DecorationConfig,
  line_number_start: usize,
  git_changes: &'a [Option<git::LineChange>],
  theme: &'a ResolvedTheme,
  show_all: bool,
}

enum OutputTarget {
  Stdout(io::Stdout),
  Pager { stdin: ChildStdin, child: Child },
}

impl OutputTarget {
  fn new(use_pager: bool) -> Result<Self> {
    if !use_pager {
      return Ok(Self::Stdout(io::stdout()));
    }

    let (program, args) = resolve_pager_command()?;
    let command = format_command(&program, &args);
    let mut child = Command::new(&program)
      .args(&args)
      .stdin(Stdio::piped())
      .spawn()
      .map_err(|err| eyre!("failed to start pager `{command}`: {err}"))?;
    let stdin = child
      .stdin
      .take()
      .ok_or_else(|| eyre!("failed to open stdin for pager `{command}`"))?;

    Ok(Self::Pager { stdin, child })
  }

  fn finish(mut self) -> Result<()> {
    self.flush()?;

    match self {
      Self::Stdout(_) => Ok(()),
      Self::Pager { stdin, mut child } => {
        drop(stdin);
        let status = child.wait()?;
        if status.success() {
          Ok(())
        } else {
          Err(eyre!("pager exited with status {status}"))
        }
      }
    }
  }
}

impl Write for OutputTarget {
  fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
    match self {
      Self::Stdout(stdout) => stdout.write(buf),
      Self::Pager { stdin, .. } => stdin.write(buf),
    }
  }

  fn flush(&mut self) -> io::Result<()> {
    match self {
      Self::Stdout(stdout) => stdout.flush(),
      Self::Pager { stdin, .. } => stdin.flush(),
    }
  }
}

struct StreamBuffer<'a, W> {
  out: &'a mut W,
  buf: String,
}

impl<'a, W: Write> StreamBuffer<'a, W> {
  fn new(out: &'a mut W) -> Self {
    Self {
      out,
      buf: String::with_capacity(STREAM_OUTPUT_BUFFER_BYTES),
    }
  }

  fn len(&self) -> usize {
    self.buf.len()
  }

  fn push(&mut self, s: &str) -> std::result::Result<(), StreamHighlightError> {
    self.buf.push_str(s);
    self.flush_if_full()
  }

  fn flush_if_at_least(&mut self, bytes: usize) -> std::result::Result<(), StreamHighlightError> {
    if self.buf.len() >= bytes {
      self.flush()?;
    }
    Ok(())
  }

  fn flush_if_full(&mut self) -> std::result::Result<(), StreamHighlightError> {
    if self.buf.len() >= STREAM_OUTPUT_BUFFER_BYTES {
      self.flush()?;
    }
    Ok(())
  }

  fn flush(&mut self) -> std::result::Result<(), StreamHighlightError> {
    if !self.buf.is_empty() {
      self.out.write_all(self.buf.as_bytes())?;
      self.buf.clear();
    }
    Ok(())
  }
}

fn main() -> Result<()> {
  let cli = Cli::parse();
  if let Some(shell) = cli.completions {
    write_completions(shell)?;
    return Ok(());
  }
  if cli.man_page {
    write_man_page()?;
    return Ok(());
  }
  if cli.list_themes {
    for theme in syntastica_themes::THEMES {
      println!("{theme}");
    }
    return Ok(());
  }
  let stdout_is_terminal = io::stdout().is_terminal();
  if cli.pager && !stdout_is_terminal {
    return Err(eyre!("--pager requires stdout to be a terminal"));
  }

  let mut use_color = stdout_is_terminal || cli.pager;
  // Check --no-color flag and NO_COLOR environment variable (https://no-color.org/)
  if cli.no_color || std::env::var("NO_COLOR").is_ok() {
    use_color = false;
  }
  match cli.color {
    ColorWhen::Auto => {}
    ColorWhen::Never => use_color = false,
    ColorWhen::Always => use_color = true,
  }
  let language_set = LanguageSetImpl::new();
  let theme = resolve_theme(&cli.theme);
  let style_config = parse_style_components(cli.style.as_deref());
  let decoration_config = style_config.decoration_config;
  let highlight_locals = style_config.highlight_locals;
  let highlight_injections = style_config.highlight_injections;
  let squeeze_limit = cli.squeeze_limit.unwrap_or(1);
  let squeeze_blank = cli.squeeze_blank || cli.squeeze_limit.is_some();
  let language_override = match cli.language.as_deref() {
    Some(name) => Some(
      resolve_language(name, &language_set).ok_or_else(|| eyre!("Unsupported language: {name}"))?,
    ),
    None => None,
  };

  let files = if cli.files.is_empty() {
    vec![PathBuf::from("-")]
  } else {
    cli.files
  };

  let global_line_range = match cli.lines.as_deref() {
    Some(raw) => Some(parse_line_range_arg(raw)?),
    None => None,
  };

  let mut had_error = false;
  let mut file_specs = Vec::with_capacity(files.len());
  for path in files {
    match parse_file_spec(path, global_line_range) {
      Ok(spec) => file_specs.push(spec),
      Err(err) => {
        eprintln!("umber: {err}");
        had_error = true;
      }
    }
  }

  let ctx = RenderContext {
    decoration_config,
    highlight_locals,
    highlight_injections,
    use_color,
    squeeze_blank,
    squeeze_limit,
    show_all: cli.show_all,
    language_set: &language_set,
    theme: &theme,
  };
  let mut state = RenderState::new();
  let mut stdout = OutputTarget::new(cli.pager)?;
  let mut stdin = io::stdin();
  let mut stdin_consumed = false;
  let mut wrote_output = false;
  let multiple_files = file_specs.len() > 1;

  for spec in file_specs {
    // Show file header between files when headers are enabled
    if ctx.decoration_config.show_headers && multiple_files {
      if wrote_output {
        writeln!(stdout)?;
      }
      let display_name = display_name_for_spec(&spec);
      // Get terminal width, default to 80 if unavailable
      let term_width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);
      // Create a prominent header that spans the terminal width
      let border = "─".repeat(term_width);
      writeln!(stdout, "{border}")?;
      // Center the filename in the header
      let padding = (term_width.saturating_sub(display_name.len())) / 2;
      writeln!(
        stdout,
        "{}{}{}",
        " ".repeat(padding),
        display_name,
        " ".repeat(term_width - display_name.len() - padding)
      )?;
      writeln!(stdout, "{border}")?;
    }

    if spec.path == Path::new("-") {
      if stdin_consumed {
        continue;
      }
      stdin_consumed = true;
      let mut buf = Vec::new();
      if let Err(err) = stdin.read_to_end(&mut buf) {
        eprintln!("umber: -: {err}");
        had_error = true;
        continue;
      }
      emit_bytes(
        &mut stdout,
        buf,
        None,
        spec.line_range,
        language_override,
        &ctx,
        &mut state,
      )?;
      wrote_output = true;
      continue;
    }

    match fs::read(&spec.path) {
      Ok(buf) => {
        emit_bytes(
          &mut stdout,
          buf,
          Some(&spec.path),
          spec.line_range,
          language_override,
          &ctx,
          &mut state,
        )?;
        wrote_output = true;
      }
      Err(err) => {
        eprintln!("umber: {}: {err}", spec.path.display());
        had_error = true;
      }
    }
  }

  stdout.finish()?;
  if had_error {
    std::process::exit(1);
  }
  Ok(())
}

fn resolve_pager_command() -> Result<(String, Vec<String>)> {
  resolve_pager_command_from_env(std::env::var("PAGER").ok().as_deref())
}

fn resolve_pager_command_from_env(pager: Option<&str>) -> Result<(String, Vec<String>)> {
  let Some(raw) = pager.map(str::trim).filter(|value| !value.is_empty()) else {
    return Ok(("less".to_string(), vec!["-R".to_string()]));
  };

  let parts = shlex::split(raw).ok_or_else(|| eyre!("invalid PAGER command: {raw}"))?;
  let (program, args) = parts
    .split_first()
    .ok_or_else(|| eyre!("invalid PAGER command: {raw}"))?;

  let mut args = args.to_vec();
  if pager_uses_less(program) && !less_supports_raw_control_chars(&args) {
    args.push("-R".to_string());
  }

  Ok((program.clone(), args))
}

fn pager_uses_less(program: &str) -> bool {
  Path::new(program)
    .file_name()
    .and_then(|name| name.to_str())
    .is_some_and(|name| name.eq_ignore_ascii_case("less"))
}

fn less_supports_raw_control_chars(args: &[String]) -> bool {
  args
    .iter()
    .any(|arg| less_arg_supports_raw_control_chars(arg))
}

fn less_arg_supports_raw_control_chars(arg: &str) -> bool {
  if arg.eq_ignore_ascii_case("--RAW-CONTROL-CHARS") {
    return true;
  }

  if let Some(short_flags) = arg.strip_prefix('-')
    && !short_flags.starts_with('-')
  {
    return short_flags.chars().any(|flag| matches!(flag, 'R' | 'r'));
  }

  false
}

fn format_command(program: &str, args: &[String]) -> String {
  let mut rendered = program.to_string();
  for arg in args {
    rendered.push(' ');
    rendered.push_str(arg);
  }
  rendered
}

fn write_completions(shell: clap_complete::Shell) -> Result<()> {
  let mut cmd = Cli::command();
  clap_complete::generate(shell, &mut cmd, "umber", &mut io::stdout());
  Ok(())
}

fn write_man_page() -> Result<()> {
  let cmd = Cli::command();
  let man = clap_mangen::Man::new(cmd);
  man.render(&mut io::stdout())?;
  Ok(())
}

fn emit_bytes(
  stdout: &mut impl Write,
  bytes: Vec<u8>,
  path: Option<&Path>,
  line_range: Option<LineRange>,
  language_override: Option<Lang>,
  ctx: &RenderContext<'_>,
  state: &mut RenderState,
) -> Result<bool> {
  let bytes = if let Some(range) = line_range {
    slice_bytes_by_line_range(&bytes, range)
  } else {
    bytes
  };
  let bytes = if ctx.squeeze_blank {
    squeeze_blank_lines_bytes(&bytes, ctx.squeeze_limit)
  } else {
    bytes
  };
  let line_number_start = line_range.map(|range| range.start).unwrap_or(1);
  let ended_with_newline = bytes.last() == Some(&b'\n') || bytes.is_empty();
  let decoration_config = ctx.decoration_config;
  let show_all = ctx.show_all;
  let use_color = ctx.use_color;

  // Handle show_all flag for non-color, non-decoration case
  if !use_color && !decoration_config.has_decorations() {
    if show_all {
      if let Ok(text) = String::from_utf8(bytes.clone()) {
        let transformed = unprintable::show_unprintable(&text, unprintable::get_char_style());
        stdout.write_all(transformed.as_bytes())?;
      } else {
        // Invalid UTF-8, write as-is
        stdout.write_all(&bytes)?;
      }
    } else {
      stdout.write_all(&bytes)?;
    }
    return Ok(ended_with_newline);
  }

  // Fetch git changes if needed (only for actual file paths, not stdin)
  let git_changes = if decoration_config.show_changes {
    // Only check git for real file paths (not stdin "-")
    if let Some(p) = path {
      if p != Path::new("-") {
        // Convert to absolute path for git detection
        let abs_path = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        git::get_git_line_changes(&abs_path).unwrap_or_default()
      } else {
        Vec::new()
      }
    } else {
      Vec::new()
    }
  } else {
    Vec::new()
  };

  if use_color {
    match String::from_utf8(bytes) {
      Ok(text) => {
        let language = language_override.or_else(|| detect_language(path, &text, ctx.language_set));
        write_rendered_text(
          stdout,
          &text,
          language,
          line_number_start,
          &git_changes,
          ctx,
          state,
        )?;
        return Ok(ended_with_newline);
      }
      Err(err) => {
        let bytes = err.into_bytes();
        if decoration_config.show_numbers {
          write_numbered_bytes(stdout, &bytes, line_number_start)?;
        } else if show_all {
          // Try to convert what we can, handling invalid UTF-8
          let text = String::from_utf8_lossy(&bytes);
          let transformed = unprintable::show_unprintable(&text, unprintable::get_char_style());
          stdout.write_all(transformed.as_bytes())?;
        } else {
          stdout.write_all(&bytes)?;
        }
        return Ok(ended_with_newline);
      }
    }
  }

  if decoration_config.show_numbers {
    if show_all {
      // Use number_plain_text when show_all is enabled
      if let Ok(text) = String::from_utf8(bytes.clone()) {
        let numbered = number_plain_text(&text, line_number_start, show_all);
        stdout.write_all(numbered.as_bytes())?;
      } else {
        write_numbered_bytes(stdout, &bytes, line_number_start)?;
      }
    } else {
      write_numbered_bytes(stdout, &bytes, line_number_start)?;
    }
  } else if show_all {
    // Handle show_all for non-color case with decorations
    if let Ok(text) = String::from_utf8(bytes.clone()) {
      let transformed = unprintable::show_unprintable(&text, unprintable::get_char_style());
      stdout.write_all(transformed.as_bytes())?;
    } else {
      stdout.write_all(&bytes)?;
    }
  } else {
    stdout.write_all(&bytes)?;
  }
  Ok(ended_with_newline)
}

fn detect_language(
  path: Option<&Path>,
  content: &str,
  language_set: &LanguageSetImpl,
) -> Option<Lang> {
  let file_type = if let Some(path) = path {
    palate::detect(path, content)
  } else {
    palate::detect("", content)
  };

  Lang::for_file_type(file_type, language_set)
}

fn resolve_language(name: impl AsRef<str>, language_set: &LanguageSetImpl) -> Option<Lang> {
  let name = name.as_ref().trim();

  // Parse the name as a FileType, then convert directly to Lang
  let file_type = palate::FileType::from_str(name).ok()?;
  Lang::for_file_type(file_type, language_set)
}

#[derive(Debug)]
enum StreamHighlightError {
  Highlight,
  Io(io::Error),
}

impl From<io::Error> for StreamHighlightError {
  fn from(value: io::Error) -> Self {
    Self::Io(value)
  }
}

fn write_rendered_text(
  stdout: &mut impl Write,
  text: &str,
  language: Option<Lang>,
  line_number_start: usize,
  git_changes: &[Option<git::LineChange>],
  ctx: &RenderContext<'_>,
  state: &mut RenderState,
) -> Result<()> {
  let decoration_config = ctx.decoration_config;
  let show_all = ctx.show_all;

  let Some(language) = language else {
    let out = if decoration_config.show_numbers {
      number_plain_text(text, line_number_start, show_all)
    } else if show_all {
      unprintable::show_unprintable(text, unprintable::get_char_style())
    } else {
      text.to_string()
    };
    stdout.write_all(out.as_bytes())?;
    return Ok(());
  };

  match write_highlighted_text_stream(
    stdout,
    text,
    language,
    line_number_start,
    git_changes,
    ctx,
    state,
  ) {
    Ok(()) => Ok(()),
    Err(StreamHighlightError::Highlight) => {
      let out = if decoration_config.show_numbers {
        number_plain_text(text, line_number_start, show_all)
      } else if show_all {
        unprintable::show_unprintable(text, unprintable::get_char_style())
      } else {
        text.to_string()
      };
      stdout.write_all(out.as_bytes())?;
      Ok(())
    }
    Err(StreamHighlightError::Io(err)) => Err(err.into()),
  }
}

fn write_highlighted_text_stream(
  stdout: &mut impl Write,
  text: &str,
  language: Lang,
  line_number_start: usize,
  git_changes: &[Option<git::LineChange>],
  ctx: &RenderContext<'_>,
  state: &mut RenderState,
) -> std::result::Result<(), StreamHighlightError> {
  let language_set = ctx.language_set;
  let decoration_config = ctx.decoration_config;
  let theme = ctx.theme;
  let show_all = ctx.show_all;
  let highlight_locals = ctx.highlight_locals;
  let highlight_injections =
    should_highlight_injections(language, ctx.highlight_injections, language_set);

  let highlight_config = if highlight_injections {
    language_set
      .get_language(language)
      .map_err(|_| StreamHighlightError::Highlight)?
  } else if highlight_locals {
    get_locals_config(&mut state.locals_configs, language)?
  } else {
    get_highlights_only_config(&mut state.highlights_only_configs, language)?
  };

  let iter = state
    .highlighter
    .highlight(
      highlight_config,
      text.as_bytes(),
      None,
      |lang_name: &str| {
        if !highlight_injections {
          return None;
        }

        let lang_name = lang_name.to_ascii_lowercase();
        Lang::for_name(&lang_name, language_set)
          .ok()
          .or_else(|| Lang::for_injection(&lang_name, language_set))
          .or_else(|| {
            lang_name.rsplit_once('/').and_then(|(_, name)| {
              Lang::for_name(name, language_set)
                .ok()
                .or_else(|| Lang::for_injection(name, language_set))
            })
          })
          .and_then(|lang| language_set.get_language(lang).ok())
      },
    )
    .map_err(|_| StreamHighlightError::Highlight)?;

  if decoration_config.has_decorations() {
    write_highlight_iter_with_decorations(
      stdout,
      text,
      iter,
      &mut state.renderer,
      DecorationsStreamSettings {
        decoration_config,
        line_number_start,
        git_changes,
        theme,
        show_all,
      },
    )
  } else {
    write_highlight_iter_plain(stdout, text, iter, &mut state.renderer, theme, show_all)
  }
}

fn current_style_key(style_stack: &[usize]) -> Option<&'static str> {
  style_stack
    .last()
    .and_then(|idx| THEME_KEYS.get(*idx).copied())
    .and_then(|key| (key != "none").then_some(key))
}

fn get_highlights_only_config(
  configs: &mut HashMap<Lang, HighlightConfiguration>,
  lang: Lang,
) -> std::result::Result<&HighlightConfiguration, StreamHighlightError> {
  use std::collections::hash_map::Entry;

  match configs.entry(lang) {
    Entry::Occupied(entry) => Ok(entry.into_mut()),
    Entry::Vacant(entry) => {
      let mut conf =
        HighlightConfiguration::new(lang.get(), lang.as_ref(), lang.highlights_query(), "", "")
          .map_err(|_| StreamHighlightError::Highlight)?;
      conf.configure(THEME_KEYS);
      Ok(entry.insert(conf))
    }
  }
}

fn get_locals_config(
  configs: &mut HashMap<Lang, HighlightConfiguration>,
  lang: Lang,
) -> std::result::Result<&HighlightConfiguration, StreamHighlightError> {
  use std::collections::hash_map::Entry;

  match configs.entry(lang) {
    Entry::Occupied(entry) => Ok(entry.into_mut()),
    Entry::Vacant(entry) => {
      let mut conf = HighlightConfiguration::new(
        lang.get(),
        lang.as_ref(),
        lang.highlights_query(),
        "",
        lang.locals_query(),
      )
      .map_err(|_| StreamHighlightError::Highlight)?;
      conf.configure(THEME_KEYS);
      Ok(entry.insert(conf))
    }
  }
}

fn highlight_line_count(text: &str) -> usize {
  text
    .as_bytes()
    .iter()
    .filter(|byte| **byte == b'\n')
    .count()
    .saturating_add(1)
}

fn write_highlight_iter_plain(
  stdout: &mut impl Write,
  text: &str,
  iter: impl Iterator<Item = std::result::Result<HighlightEvent, syntastica_highlight::Error>>,
  renderer: &mut TerminalRenderer,
  theme: &ResolvedTheme,
  show_all: bool,
) -> std::result::Result<(), StreamHighlightError> {
  let mut out = StreamBuffer::new(stdout);
  out.push(renderer.head().as_ref())?;
  out.flush()?;

  let char_style = unprintable::get_char_style();
  let lf_marker = if matches!(char_style, unprintable::CharStyle::Unicode) {
    "␊"
  } else {
    "$"
  };

  let mut style_stack = Vec::new();
  let mut line_has_content = false;
  let mut flushed_visible_output = false;

  for event in iter {
    let event = event.map_err(|_| StreamHighlightError::Highlight)?;
    match event {
      HighlightEvent::HighlightStart(Highlight(highlight)) => style_stack.push(highlight),
      HighlightEvent::HighlightEnd => {
        style_stack.pop();
      }
      HighlightEvent::Source { start, end } => {
        let source = &text[start..end];
        let ends_with_newline = source.ends_with('\n');
        let mut lines = source.lines().peekable();

        while let Some(line) = lines.next() {
          if !line.is_empty() {
            line_has_content = true;
          }

          let style_key = current_style_key(&style_stack);

          if show_all {
            let transformed = unprintable::show_unprintable(line, char_style);
            if let Some(key) = style_key
              && let Some(style_obj) = theme.get(key)
            {
              let rendered = renderer.styled(transformed.as_str(), *style_obj);
              out.push(rendered.as_ref())?;
            } else {
              out.push(&transformed)?;
            }
          } else {
            let escaped = renderer.escape(line);
            let rendered = match style_key.and_then(|key| theme.find_style(key)) {
              Some(style) => renderer.styled(&escaped, style),
              None => renderer.unstyled(&escaped),
            };
            out.push(rendered.as_ref())?;
          }

          if !flushed_visible_output && out.len() >= STREAM_OUTPUT_FLUSH_BYTES {
            out.flush()?;
            flushed_visible_output = true;
          }

          let newline_after = lines.peek().is_some() || ends_with_newline;
          if newline_after {
            if show_all && line_has_content {
              out.push(lf_marker)?;
            }
            out.push(renderer.newline().as_ref())?;
            if !flushed_visible_output {
              out.flush()?;
              flushed_visible_output = true;
            } else {
              out.flush_if_at_least(STREAM_OUTPUT_FLUSH_BYTES)?;
            }
            line_has_content = false;
          }
        }
      }
    }
  }

  if show_all && line_has_content {
    out.push(lf_marker)?;
  }

  out.push(renderer.tail().as_ref())?;
  out.flush()?;
  Ok(())
}

fn write_highlight_iter_with_decorations(
  stdout: &mut impl Write,
  text: &str,
  iter: impl Iterator<Item = std::result::Result<HighlightEvent, syntastica_highlight::Error>>,
  renderer: &mut TerminalRenderer,
  settings: DecorationsStreamSettings<'_>,
) -> std::result::Result<(), StreamHighlightError> {
  let decoration_config = settings.decoration_config;
  let line_number_start = settings.line_number_start;
  let git_changes = settings.git_changes;
  let theme = settings.theme;
  let show_all = settings.show_all;

  // Only show git margin if there are actual changes
  let has_git_changes = git_changes.iter().any(|c| c.is_some());
  let effective_config = if has_git_changes {
    decoration_config
  } else {
    DecorationConfig {
      show_changes: false,
      ..decoration_config
    }
  };

  // Match Processor output: number of highlight lines is newlines + 1.
  let line_count = highlight_line_count(text);
  let last_line_no = line_number_start.saturating_add(line_count.saturating_sub(1));
  let width = line_number_width(last_line_no);

  let mut out = StreamBuffer::new(stdout);
  out.push(renderer.head().as_ref())?;
  out.flush()?;

  let char_style = unprintable::get_char_style();
  let lf_marker = if matches!(char_style, unprintable::CharStyle::Unicode) {
    "␊"
  } else {
    "$"
  };

  let mut style_stack = Vec::new();
  let mut line_no = line_number_start;
  let mut line_index = 0usize;
  let mut line_has_content = false;
  let mut line_content: Vec<(Cow<'_, str>, Option<&'static str>)> = Vec::new();
  let mut flushed_visible_output = false;

  for event in iter {
    let event = event.map_err(|_| StreamHighlightError::Highlight)?;
    match event {
      HighlightEvent::HighlightStart(Highlight(highlight)) => style_stack.push(highlight),
      HighlightEvent::HighlightEnd => {
        style_stack.pop();
      }
      HighlightEvent::Source { start, end } => {
        let source = &text[start..end];
        let ends_with_newline = source.ends_with('\n');
        let mut lines = source.lines().peekable();

        while let Some(line) = lines.next() {
          if !line.is_empty() {
            line_has_content = true;
          }

          let style_key = current_style_key(&style_stack);
          let piece = if show_all {
            Cow::Owned(unprintable::show_unprintable(line, char_style))
          } else {
            Cow::Borrowed(line)
          };
          line_content.push((piece, style_key));

          let newline_after = lines.peek().is_some() || ends_with_newline;
          if newline_after {
            let line_change = git_changes.get(line_index).copied().flatten();
            let rendered = decorations::render_decorated_line(
              &line_content,
              line_no,
              &effective_config,
              line_change,
              renderer,
              theme,
              width,
            );
            out.push(&rendered)?;

            if show_all && line_has_content {
              out.push(lf_marker)?;
            }

            out.push(renderer.newline().as_ref())?;
            if !flushed_visible_output {
              out.flush()?;
              flushed_visible_output = true;
            } else {
              out.flush_if_at_least(STREAM_OUTPUT_FLUSH_BYTES)?;
            }

            line_content.clear();
            line_has_content = false;
            line_no += 1;
            line_index += 1;
          }
        }
      }
    }
  }

  // Flush final line (even if empty) to match existing decoration behavior.
  let line_change = git_changes.get(line_index).copied().flatten();
  let rendered = decorations::render_decorated_line(
    &line_content,
    line_no,
    &effective_config,
    line_change,
    renderer,
    theme,
    width,
  );
  out.push(&rendered)?;
  if show_all && line_has_content {
    out.push(lf_marker)?;
  }

  out.push(renderer.tail().as_ref())?;
  out.flush()?;
  Ok(())
}

fn resolve_theme(theme: &str) -> ResolvedTheme {
  if let Some(user_theme) = syntastica_themes::from_str(theme.trim()) {
    return user_theme;
  }
  resolve_auto_theme()
}

fn resolve_auto_theme() -> ResolvedTheme {
  match dark_light::detect() {
    Ok(DarkLightMode::Light) => syntastica_themes::catppuccin::latte(),
    Ok(DarkLightMode::Dark) => syntastica_themes::catppuccin::mocha(),
    Ok(DarkLightMode::Unspecified) => syntastica_themes::catppuccin::mocha(),
    Err(_) => syntastica_themes::catppuccin::mocha(),
  }
}

fn number_plain_text(text: &str, line_number_start: usize, show_all: bool) -> String {
  let line_count = count_lines_bytes(text.as_bytes());
  if line_count == 0 {
    return String::new();
  }

  let last_line_no = line_number_start.saturating_add(line_count.saturating_sub(1));
  let width = line_number_width(last_line_no);
  let mut out = String::new();
  let mut line_no = line_number_start;

  for chunk in text.split_inclusive('\n') {
    let _ = write!(out, "{:>width$}  ", line_no, width = width);
    let content = if show_all {
      unprintable::show_unprintable(chunk, unprintable::get_char_style())
    } else {
      chunk.to_string()
    };
    out.push_str(&content);
    line_no += 1;
  }
  out
}

fn write_numbered_bytes(
  stdout: &mut impl Write,
  bytes: &[u8],
  line_number_start: usize,
) -> Result<()> {
  let line_count = count_lines_bytes(bytes);
  if line_count == 0 {
    return Ok(());
  }

  let last_line_no = line_number_start.saturating_add(line_count.saturating_sub(1));
  let width = line_number_width(last_line_no);
  let mut line_no = line_number_start;
  write_prefix(stdout, line_no, width)?;
  for (index, byte) in bytes.iter().enumerate() {
    stdout.write_all(&[*byte])?;
    if *byte == b'\n' && index + 1 < bytes.len() {
      line_no += 1;
      write_prefix(stdout, line_no, width)?;
    }
  }
  Ok(())
}

fn write_prefix(stdout: &mut impl Write, line_no: usize, width: usize) -> Result<()> {
  write!(stdout, "{:>width$}  ", line_no, width = width)?;
  Ok(())
}

fn count_lines_bytes(bytes: &[u8]) -> usize {
  if bytes.is_empty() {
    return 0;
  }
  let count = bytes.iter().filter(|byte| **byte == b'\n').count();
  if bytes.last() == Some(&b'\n') {
    count
  } else {
    count + 1
  }
}

fn line_number_width(line_count: usize) -> usize {
  let width = line_count.to_string().len();
  if width == 0 { 1 } else { width }
}

/// Parse style components from the --style flag.
/// Supports: "numbers", "changes", "headers", "rich"
fn parse_style_components(style: Option<&str>) -> StyleConfig {
  let mut config = StyleConfig {
    highlight_locals: true,
    ..StyleConfig::default()
  };
  for raw in style.unwrap_or_default().split(',') {
    let token = raw.trim();
    match token {
      "numbers" => config.decoration_config.show_numbers = true,
      "changes" => config.decoration_config.show_changes = true,
      "headers" => config.decoration_config.show_headers = true,
      "rich" => config.highlight_injections = true,
      _ => {}
    }
  }

  if config.highlight_injections {
    config.highlight_locals = true;
  }

  config
}

fn display_name_for_spec(spec: &FileSpec) -> String {
  if spec.path == Path::new("-") {
    "-".to_string()
  } else {
    spec.path.to_string_lossy().to_string()
  }
}

fn should_highlight_injections(
  language: Lang,
  highlight_injections: bool,
  language_set: &LanguageSetImpl,
) -> bool {
  let resolved = resolve_language("markdown", language_set);
  highlight_injections || resolved == Some(language)
}

fn squeeze_blank_lines_bytes(bytes: &[u8], limit: usize) -> Vec<u8> {
  if bytes.is_empty() {
    return Vec::new();
  }
  let mut out = Vec::with_capacity(bytes.len());
  let mut blank_count = 0usize;
  let mut start = 0usize;
  for (index, byte) in bytes.iter().enumerate() {
    if *byte == b'\n' {
      let line = &bytes[start..=index];
      let mut content_end = index;
      if content_end > start && bytes[content_end - 1] == b'\r' {
        content_end -= 1;
      }
      let is_blank = content_end == start;
      if is_blank {
        blank_count += 1;
        if blank_count <= limit {
          out.extend_from_slice(line);
        }
      } else {
        blank_count = 0;
        out.extend_from_slice(line);
      }
      start = index + 1;
    }
  }
  if start < bytes.len() {
    let line = &bytes[start..];
    let mut content_end = bytes.len();
    if content_end > start && bytes[content_end - 1] == b'\r' {
      content_end -= 1;
    }
    let is_blank = content_end == start;
    if is_blank {
      blank_count += 1;
      if blank_count <= limit {
        out.extend_from_slice(line);
      }
    } else {
      out.extend_from_slice(line);
    }
  }
  out
}

fn parse_file_spec(path: PathBuf, default_range: Option<LineRange>) -> Result<FileSpec> {
  let raw = path.to_string_lossy();
  if let Some((path_part, line_range)) = parse_line_range_suffix(&raw)? {
    let parsed_path = PathBuf::from(path_part);
    return Ok(FileSpec {
      path: parsed_path,
      line_range: Some(line_range),
    });
  }
  Ok(FileSpec {
    path,
    line_range: default_range,
  })
}

fn parse_line_range_suffix(raw: &str) -> Result<Option<(String, LineRange)>> {
  let (path_part, range_part) = match raw.rsplit_once("#L").or_else(|| raw.rsplit_once("#l")) {
    Some(parts) => parts,
    None => return Ok(None),
  };
  if path_part.is_empty() {
    return Err(eyre!("missing file path before line range"));
  }
  if range_part.is_empty() {
    return Err(eyre!("missing line range after #L"));
  }
  let line_range = parse_line_range(range_part).ok_or_else(|| {
    eyre!(
      "invalid line range '#L{range_part}' (expected #L<start>-<end>, #L<start>:<end>, #L<start>,<end>, or #L<start>)"
    )
  })?;
  Ok(Some((path_part.to_string(), line_range)))
}

fn parse_line_range_arg(raw: &str) -> Result<LineRange> {
  parse_line_range(raw).ok_or_else(|| {
    eyre!("invalid line range '{raw}' (expected start-end, start:end, start,end, or start)")
  })
}

fn parse_line_range(raw: &str) -> Option<LineRange> {
  let raw = raw.trim();
  let raw = raw
    .strip_prefix('L')
    .or_else(|| raw.strip_prefix('l'))
    .unwrap_or(raw);
  if raw.is_empty() {
    return None;
  }
  let (start_raw, end_raw) = match split_line_range(raw) {
    Some(parts) => parts,
    None => {
      let line = raw.parse::<usize>().ok()?;
      if line == 0 {
        return None;
      }
      return Some(LineRange {
        start: line,
        end: line,
      });
    }
  };
  if start_raw.is_empty() || end_raw.is_empty() {
    return None;
  }
  let start_raw = start_raw.trim();
  let end_raw = end_raw.trim();
  let start = start_raw.parse::<usize>().ok()?;
  let end_raw = end_raw
    .strip_prefix('L')
    .or_else(|| end_raw.strip_prefix('l'))
    .unwrap_or(end_raw);
  let end = end_raw.parse::<usize>().ok()?;
  if start == 0 || end == 0 || end < start {
    return None;
  }
  Some(LineRange { start, end })
}

fn split_line_range(raw: &str) -> Option<(&str, &str)> {
  for separator in ['-', ':', ','] {
    if let Some(parts) = raw.split_once(separator) {
      return Some(parts);
    }
  }
  None
}

fn slice_bytes_by_line_range(bytes: &[u8], range: LineRange) -> Vec<u8> {
  if bytes.is_empty() {
    return Vec::new();
  }
  let mut out = Vec::new();
  let mut line_no = 1usize;
  let mut start = 0usize;
  for (index, byte) in bytes.iter().enumerate() {
    if *byte == b'\n' {
      let line_end = index + 1;
      if line_no >= range.start && line_no <= range.end {
        out.extend_from_slice(&bytes[start..line_end]);
      }
      line_no += 1;
      if line_no > range.end {
        return out;
      }
      start = line_end;
    }
  }
  if start < bytes.len() && line_no >= range.start && line_no <= range.end {
    out.extend_from_slice(&bytes[start..]);
  }
  out
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn rich_highlighting_is_enabled_by_default_for_markdown() {
    let language_set = LanguageSetImpl::new();
    let markdown = resolve_language("markdown", &language_set).expect("markdown language");

    assert!(should_highlight_injections(markdown, false, &language_set));
  }

  #[test]
  fn rich_highlighting_stays_opt_in_for_non_markdown_languages() {
    let language_set = LanguageSetImpl::new();
    let rust = resolve_language("rust", &language_set).expect("rust language");

    assert!(!should_highlight_injections(rust, false, &language_set));
    assert!(should_highlight_injections(rust, true, &language_set));
  }
}
