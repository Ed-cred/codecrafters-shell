use std::env;
use std::fmt::Display;
use std::fs::{self, File};
use std::io::{self, Error, Write};
use std::path::PathBuf;

use crate::builtins::{external_cmd, find_builtin, BuiltinFn};
use crate::parser::{Command, ParseError};

mod builtins;
mod parser;

#[derive(Debug)]
pub struct Shell {
    path_dirs: Vec<PathBuf>,
}

struct OutputStreams<'a> {
    stdout: &'a mut dyn Write,
    stderr: &'a mut dyn Write,
}

#[derive(Debug)]
pub enum ShellError {
    Io(Error),
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

impl std::error::Error for ShellError {}

impl From<Error> for ShellError {
    fn from(value: Error) -> Self {
        ShellError::Io(value)
    }
}

impl ShellError {
    pub fn is_stdout_err(&self) -> bool {
        matches!(
            self,
            ShellError::CommandNotFound(_) | ShellError::NotFound(_)
        )
    }
}

impl Shell {
    pub fn new() -> Self {
        let path_dirs = env::var_os("PATH")
            .map(|paths| env::split_paths(&paths).collect())
            .unwrap_or_default();
        Self { path_dirs }
    }

    pub fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
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
                    report(&err.into(), &mut io::stdout(), &mut io::stderr())?;
                    continue;
                }
            };
            let (mut stdout_file, mut stderr_file) = cmd.open_redirects()?;
            let mut stdout_real = io::stdout();
            let mut stderr_real = io::stderr();
            let mut ctx = OutputStreams {
                stdout: sink(stdout_file.as_mut(), &mut stdout_real),
                stderr: sink(stderr_file.as_mut(), &mut stderr_real),
            };

            let res = self.execute_command(&mut ctx, cmd);

            if let Err(err) = res {
                report(&err, ctx.stdout, ctx.stderr)?;
            }
        }
    }

    fn execute_command(&self, ctx: &mut OutputStreams, cmd: Command) -> Result<(), ShellError> {
        let handler = find_builtin(&cmd.name).unwrap_or(external_cmd as BuiltinFn);

        handler(ctx, self, &cmd)
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

fn sink<'a>(f: Option<&'a mut File>, fallback: &'a mut dyn Write) -> &'a mut dyn Write {
    match f {
        Some(file) => file,
        None => fallback,
    }
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

fn report(err: &ShellError, stdout: &mut dyn Write, stderr: &mut dyn Write) -> io::Result<()> {
    if err.is_stdout_err() {
        writeln!(stdout, "{}", err)
    } else {
        writeln!(stderr, "{}", err)
    }
}
