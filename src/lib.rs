use std::env;
use std::fmt::Display;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::fd::FromRawFd;
use std::path::PathBuf;
use std::process::{self, Stdio};

#[derive(Debug)]
pub struct Shell {
    path_dirs: Vec<PathBuf>,
}

type BuiltinFn = fn(&mut ExecutionContext<'_>, &Command) -> Result<(), ShellError>;
static BUILTINS: &[(&str, BuiltinFn)] = &[
    ("exit", exit_cmd as BuiltinFn),
    ("echo", echo_cmd as BuiltinFn),
    ("type", type_cmd as BuiltinFn),
    ("pwd", pwd_cmd as BuiltinFn),
    ("cd", cd_cmd as BuiltinFn),
];

fn find_builtin(name: &str) -> Option<BuiltinFn> {
    BUILTINS
        .iter()
        .find_map(|&(builtin_name, func)| (builtin_name == name).then_some(func))
}

struct ExecutionContext<'a> {
    shell: &'a mut Shell,
    stdin: &'a mut dyn Read,
    stdout: &'a mut dyn Write,
    stderr: &'a mut dyn Write,
}

#[derive(Debug)]
pub enum ShellError {
    Io(std::io::Error),
    ShellMessage(String),
    CommandNotFound(String),
    NotFound(String),
    Parse(ParseError),
}

impl Display for ShellError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShellError::Io(error) => write!(f, "I/O error: {}", error),
            ShellError::ShellMessage(s) => write!(f, "{}", s),
            ShellError::CommandNotFound(cmd) => write!(f, "{}: command not found", cmd),
            ShellError::NotFound(cmd) => write!(f, "{}: not found", cmd),
            ShellError::Parse(parse_error) => write!(f, "{}", parse_error),
        }
    }
}

impl From<std::io::Error> for ShellError {
    fn from(value: std::io::Error) -> Self {
        ShellError::Io(value)
    }
}

impl Shell {
    pub fn new() -> Self {
        let path_dirs = env::var_os("PATH")
            .map(|paths| env::split_paths(&paths).collect())
            .unwrap_or_default();
        Self { path_dirs }
    }

    pub fn run(mut self) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            print!("$ ");
            io::stdout().flush().unwrap();

            let mut buf = String::new();
            match io::stdin().read_line(&mut buf) {
                Ok(0) => break Ok(()),
                Ok(_) => {}
                Err(e) => {
                    writeln!(io::stderr(), "read error: {e}").unwrap();
                    continue;
                }
            };
            let buf = buf.trim_end();
            let cmd = match Command::parse(buf) {
                Ok(command) => command,
                Err(err) => {
                    writeln!(io::stderr(), "{}", err);
                    continue;
                }
            };
            let mut file = match &cmd.redirect_path {
                Some(path) => Some(File::create(path)?),
                None => None,
            };
            let mut stdout = io::stdout();
            let sink: &mut dyn Write = match file.as_mut() {
                Some(f) => f,
                None => &mut stdout,
            };
            let mut ctx = ExecutionContext {
                shell: &mut self,
                stdin: &mut io::stdin(),
                stdout: sink,
                stderr: &mut io::stderr(),
            };

            let handler = find_builtin(&cmd.name).unwrap_or(external_cmd as BuiltinFn);
            let res = handler(&mut ctx, &cmd);

            if let Err(err) = res {
                match err {
                    ShellError::CommandNotFound(_) | ShellError::NotFound(_) => {
                        writeln!(ctx.stdout, "{}", err).unwrap();
                    }
                    other => writeln!(ctx.stderr, "{}", other).unwrap(),
                }
            }
        }
    }

    fn try_find_executable(&self, name: &str) -> Option<PathBuf> {
        for dir in &self.path_dirs {
            let candidate_path = dir.join(name);
            #[cfg(unix)]
            if let Ok(metadata) = fs::metadata(&candidate_path) {
                if metadata.is_file() && is_executable(&metadata) {
                    return Some(candidate_path);
                }
            }
        }
        None
    }
}

struct Command {
    name: String,
    args: Vec<String>,
    redirect_path: Option<PathBuf>,
}

impl Command {
    fn parse(input: &str) -> Result<Self, ParseError> {
        let mut argv = tokenize(input).into_iter();
        let mut words: Vec<String> = Vec::new();
        let mut redirect_path = None;
        while let Some(token) = argv.next() {
            match token {
                Token::Word(w) => words.push(w),
                Token::Redirect => {
                    let Some(Token::Word(path)) = argv.next() else {
                        return Err(ParseError::MissingRedirectTarget);
                    };
                    redirect_path = Some(PathBuf::from(path));
                }
            }
        }

        let name = words.first().unwrap();
        let args = &words[1..];

        Ok(Self {
            name: name.to_string(),
            args: args.to_vec(),
            redirect_path,
        })
    }
}

#[derive(Debug)]
enum ParseError {
    MissingRedirectTarget,
}

impl From<ParseError> for ShellError {
    fn from(value: ParseError) -> Self {
        ShellError::Parse(value)
    }
}

impl Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::MissingRedirectTarget => write!(f, "missing redirect target"),
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum Quote {
    None,
    Single,
    Double,
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum Action {
    Push(char),
    Flush,
    SetQuote(Quote),
    StartEscape,
    Consume(char),
    Redirect,
}

enum Token {
    Word(String),
    Redirect,
}

fn step(quote: Quote, escaped: bool, c: char) -> Action {
    if escaped {
        return Action::Consume(c);
    }
    match quote {
        Quote::None => match c {
            '\\' => Action::StartEscape,
            '\'' => Action::SetQuote(Quote::Single),
            '\"' => Action::SetQuote(Quote::Double),
            '>' => Action::Redirect,
            c if c.is_whitespace() => Action::Flush,
            _ => Action::Push(c),
        },
        Quote::Single => match c {
            '\'' => Action::SetQuote(Quote::None),
            _ => Action::Push(c),
        },
        Quote::Double => match c {
            '\"' => Action::SetQuote(Quote::None),
            '\\' => Action::StartEscape,
            _ => Action::Push(c),
        },
    }
}

// "echo hi 1> out.txt"
// "echo hi > out.txt"
fn tokenize(input: &str) -> Vec<Token> {
    let mut out_str = Vec::new();
    let mut current_str = String::new();

    let mut quote = Quote::None;
    let mut escaped = false;

    for c in input.chars() {
        match step(quote, escaped, c) {
            Action::Push(ch) => current_str.push(ch),
            Action::Flush => {
                if !current_str.is_empty() {
                    out_str.push(Token::Word(std::mem::take(&mut current_str)));
                }
            }
            Action::SetQuote(q) => quote = q,
            Action::StartEscape => escaped = true,
            Action::Consume(ch) => {
                match quote {
                    Quote::None => {
                        current_str.push(ch);
                    }
                    Quote::Single => unreachable!("backslash does not escape inside single quotes"),
                    Quote::Double => match ch {
                        '"' | '\\' => current_str.push(ch),
                        _ => {
                            current_str.push('\\');
                            current_str.push(ch);
                        }
                    },
                }
                escaped = false;
            }
            Action::Redirect => {
                current_str.clear();
                out_str.push(Token::Redirect);
            }
        }
    }
    if escaped {
        current_str.push('\\');
    }
    if !current_str.is_empty() {
        out_str.push(Token::Word(current_str));
    }
    out_str
}

fn exit_cmd(_ctx: &mut ExecutionContext<'_>, cmd: &Command) -> Result<(), ShellError> {
    if !cmd.args.is_empty() {
        return Err(ShellError::ShellMessage(
            "exit: expected exactly zero arguments".to_string(),
        ));
    }
    process::exit(0)
}

fn echo_cmd(ctx: &mut ExecutionContext<'_>, cmd: &Command) -> Result<(), ShellError> {
    writeln!(ctx.stdout, "{}", cmd.args.join(" "))?;
    Ok(())
}

fn type_cmd(ctx: &mut ExecutionContext<'_>, cmd: &Command) -> Result<(), ShellError> {
    if cmd.args.len() != 1 {
        return Err(ShellError::ShellMessage(
            "type: expected exactly one argument".to_string(),
        ));
    }
    let queried_program = cmd.args.first().unwrap();
    if let Some(_builtin) = find_builtin(&queried_program) {
        writeln!(ctx.stdout, "{} is a shell builtin", queried_program)?;
        Ok(())
    } else if let Some(executable_path) = ctx.shell.try_find_executable(queried_program) {
        writeln!(
            ctx.stdout,
            "{} is {}",
            queried_program,
            executable_path.display()
        )?;
        Ok(())
    } else {
        Err(ShellError::NotFound(queried_program.to_string()))
    }
}

fn pwd_cmd(ctx: &mut ExecutionContext<'_>, cmd: &Command) -> Result<(), ShellError> {
    if !cmd.args.is_empty() {
        return Err(ShellError::ShellMessage(
            "pwd: expected exactly zero arguments".to_string(),
        ));
    }
    let cwd = std::env::current_dir()?;
    writeln!(ctx.stdout, "{}", cwd.display())?;
    Ok(())
}

fn cd_cmd(_ctx: &mut ExecutionContext<'_>, cmd: &Command) -> Result<(), ShellError> {
    if cmd.args.len() != 1 {
        return Err(ShellError::ShellMessage(
            "cd: expected exactly one argument".to_string(),
        ));
    }
    let path_dir = &cmd.args[0];
    let actual_path: PathBuf = if let Some(stripped_path) = path_dir.strip_prefix("~") {
        let home_path = std::env::var("HOME").expect("home should not be empty");
        PathBuf::from(home_path).join(stripped_path)
    } else {
        PathBuf::from(path_dir)
    };
    if std::env::set_current_dir(actual_path).is_err() {
        return Err(ShellError::ShellMessage(format!(
            "cd: {}: No such file or directory",
            path_dir
        )));
    }
    Ok(())
}

fn external_cmd(ctx: &mut ExecutionContext<'_>, cmd: &Command) -> Result<(), ShellError> {
    match ctx.shell.try_find_executable(&cmd.name) {
        Some(_) => {
            let output = std::process::Command::new(cmd.name.clone())
                .args(cmd.args.clone())
                .output()?;
            ctx.stdout.write_all(&output.stdout)?;
            ctx.stderr.write_all(&output.stderr)?;
            Ok(())
        }
        None => Err(ShellError::NotFound(cmd.name.clone())),
    }
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}
