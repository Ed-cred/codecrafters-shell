use std::collections::HashMap;
use std::env;
use std::fmt::Display;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process;

#[derive(Debug)]
pub struct Shell {
    builtins: HashMap<&'static str, BuiltinFn>,
    path_dirs: Vec<PathBuf>,
}
type BuiltinFn = fn(&mut ExecutionContext<'_>, &[String]) -> Result<(), ShellError>;

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
}

impl Display for ShellError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShellError::Io(error) => write!(f, "I/O error: {}", error),
            ShellError::ShellMessage(s) => write!(f, "{}", s),
            ShellError::CommandNotFound(cmd) => write!(f, "{}: command not found", cmd),
            ShellError::NotFound(cmd) => write!(f, "{}: not found", cmd),
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
        let builtins = vec!["exit", "echo", "type", "pwd", "cd"];
        let path_dirs = env::var_os("PATH")
            .map(|paths| env::split_paths(&paths).collect())
            .unwrap_or_default();
        Self {
            builtins,
            path_dirs,
        }
    }

    pub fn run(mut self) {
        loop {
            print!("$ ");
            io::stdout().flush().unwrap();

            let mut buf = String::new();
            let _ = io::stdin().read_line(&mut buf).unwrap();
            let buf = buf.trim_end();

            let cmd = Command::parse(buf);
            if let Err(err) = cmd.run(&mut self) {
                match err {
                    ShellError::CommandNotFound(_) | ShellError::NotFound(_) => println!("{}", err),
                    other => eprintln!("{}", other),
                }
            }
        }
    }

    fn try_find_executable<'a>(&mut self, name: &'a str) -> Option<PathBuf> {
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

trait CommandExec {
    fn run(self, shell: &mut Shell) -> Result<(), ShellError>;
}

enum Command {
    Exit(ExitCmd),
    Echo(EchoCmd),
    Pwd(PwdCmd),
    Cd(CdCmd),
    Type(TypeCmd),
    External(ExternalCmd),
}

impl Command {
    fn parse(input: &str) -> Self {
        let tokens = tokenize(input);
        let (name, args) = match tokens.split_first() {
            Some((n, rest)) => (n.clone(), rest.to_vec()),
            None => (String::new(), Vec::new()),
        };

        match name.as_str() {
            "exit" => Command::Exit(ExitCmd),
            "echo" => Command::Echo(EchoCmd {
                args: args.join(" "),
            }),
            "type" => Command::Type(TypeCmd {
                arg: args.join(" "),
            }),
            "pwd" => Command::Pwd(PwdCmd),
            "cd" => Command::Cd(CdCmd {
                path: args.join(" "),
            }),
            other => Command::External(ExternalCmd {
                name: other.to_string(),
                args: args,
            }),
        }
    }

    fn run(self, shell: &mut Shell) -> Result<(), ShellError> {
        match self {
            Command::Exit(exit_cmd) => exit_cmd.run(shell),
            Command::Echo(echo_cmd) => echo_cmd.run(shell),
            Command::Pwd(pwd_cmd) => pwd_cmd.run(shell),
            Command::Cd(cd_cmd) => cd_cmd.run(shell),
            Command::Type(type_cmd) => type_cmd.run(shell),
            Command::External(external_cmd) => external_cmd.run(shell),
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

fn tokenize(input: &str) -> Vec<String> {
    let mut out_str = Vec::new();
    let mut current_str = String::new();

    let mut quote = Quote::None;
    let mut escaped = false;

    for c in input.chars() {
        match step(quote, escaped, c) {
            Action::Push(ch) => current_str.push(ch),
            Action::Flush => {
                if !current_str.is_empty() {
                    // no clone here, and take resets the string back to Default
                    out_str.push(std::mem::take(&mut current_str));
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
        }
    }
    if escaped {
        current_str.push('\\');
    }
    if !current_str.is_empty() {
        out_str.push(current_str);
    }
    return out_str;
}

/* struct ExitCmd;
impl CommandExec for ExitCmd {
    fn run(self, _shell: &mut Shell) -> Result<(), ShellError> {
        process::exit(0)
    }
} */
fn exit_cmd(_ctx: &mut ExecutionContext<'_>, args: &[String]) -> Result<(), ShellError> {
    if args.len() != 0 {
        return Err(ShellError::ShellMessage(
            "exit: expected exactly zero arguments".to_string(),
        ));
    }
    process::exit(0)
}
/* struct EchoCmd {
    args: String,
}
impl CommandExec for EchoCmd {
    fn run(self, _shell: &mut Shell) -> Result<(), ShellError> {
        println!("{}", self.args);
        Ok(())
    }
} */

fn echo_cmd(ctx: &mut ExecutionContext<'_>, args: &[String]) -> Result<(), ShellError> {
    writeln!(ctx.stdout, "{}", args.join(" "))?;
    Ok(())
}

/* struct TypeCmd {
    arg: String,
}
impl CommandExec for TypeCmd {
    fn run(self, shell: &mut Shell) -> Result<(), ShellError> {
        if shell.builtins.iter().any(|builtin| builtin == &self.arg) {
            println!("{} is a shell builtin", self.arg);
            return Ok(());
        } else if let Some(executable_path) = shell.try_find_executable(&self.arg) {
            println!("{} is {}", self.arg, executable_path.display());
            return Ok(());
        }
        return Err(ShellError::NotFound(self.arg.into()));
    }
} */

fn type_cmd(ctx: &mut ExecutionContext<'_>, args: &[String]) -> Result<(), ShellError> {
    if args.len() != 1 {
        return Err(ShellError::ShellMessage(
            "type: expected exactly one argument".to_string(),
        ));
    }
    let cmd_name = &args[0];
    if ctx.shell.builtins.contains_key(cmd_name.as_str()) {
        writeln!(ctx.stdout, "{} is a shell builtin", cmd_name)?;
    } else if let Some(executable_path) = ctx.shell.try_find_executable(cmd_name) {
        writeln!(ctx.stdout, "{} is {}", cmd_name, executable_path.display())?;
    }
    return Err(ShellError::NotFound(cmd_name.into()));
}

/* struct PwdCmd;
impl CommandExec for PwdCmd {
    fn run(self, _shell: &mut Shell) -> Result<(), ShellError> {
        let cwd = std::env::current_dir()?;
        println!("{}", cwd.display());
        Ok(())
    }
} */

fn pwd_cmd(ctx: &mut ExecutionContext<'_>, args: &[String]) -> Result<(), ShellError> {
    if args.len() != 0 {
        return Err(ShellError::ShellMessage(
            "pwd: expected exactly zero arguments".to_string(),
        ));
    }
    let cwd = std::env::current_dir()?;
    writeln!(ctx.stdout, "{}", cwd.display())?;
    Ok(())
}

/* struct CdCmd {
    path: String,
}
impl CommandExec for CdCmd {
    fn run(self, _shell: &mut Shell) -> Result<(), ShellError> {
        let actual_path: PathBuf = if let Some(stripped_path) = self.path.strip_prefix("~") {
            let home_path = std::env::var("HOME").expect("home should not be empty");
            PathBuf::from(home_path).join(stripped_path)
        } else {
            PathBuf::from(&self.path)
        };
        if std::env::set_current_dir(actual_path).is_err() {
            return Err(ShellError::ShellMessage(format!(
                "cd: {}: No such file or directory",
                self.path
            )));
        }
        Ok(())
    }
} */

fn cd_cmd(_ctx: &mut ExecutionContext<'_>, args: &[String]) -> Result<(), ShellError> {
    if args.len() != 1 {
        return Err(ShellError::ShellMessage(
            "cd: expected exactly one argument".to_string(),
        ));
    }
    let path_dir = &args[0];
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

struct ExternalCmd {
    name: String,
    args: Vec<String>,
}
impl CommandExec for ExternalCmd {
    fn run(self, shell: &mut Shell) -> Result<(), ShellError> {
        match shell.try_find_executable(self.name.as_str()) {
            Some(_) => {
                let output = std::process::Command::new(self.name)
                    .args(self.args)
                    .output()?;
                io::stdout().write_all(&output.stdout)?;
                Ok(())
            }
            None => return Err(ShellError::NotFound(self.name.into())),
        }
    }
}
fn external_cmd(ctx: &mut ExecutionContext<'_>, args: &[String]) -> Result<(), ShellError> {
    let (name, program_args) = args
        .split_first()
        .ok_or_else(|| ShellError::ShellMessage("missing command".to_string()))?;
    match ctx.shell.try_find_executable(name) {
        Some(_) => {
            let output = std::process::Command::new(name)
                .args(program_args)
                .output()?;
            ctx.stdout.write_all(&output.stdout)?;
            Ok(())
        }
        None => return Err(ShellError::NotFound(name.into())),
    }
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}
