use std::{
    fmt::Display,
    fs::{File, OpenOptions},
    io,
    path::PathBuf,
};

use crate::ShellError;

pub(crate) struct Command {
    pub(crate) name: String,
    pub(crate) args: Vec<String>,
    pub(crate) stdout_path: Option<Target>,
    pub(crate) stderr_path: Option<Target>,
}

impl Command {
    pub(crate) fn parse(input: &str) -> Result<Self, ParseError> {
        let mut argv = tokenize(input).into_iter();
        let mut words: Vec<String> = Vec::new();
        let mut stdout_path = None;
        let mut stderr_path = None;
        while let Some(token) = argv.next() {
            match token {
                Token::Word(w) => words.push(w),
                Token::Redirect { fd, append } => {
                    let Some(Token::Word(path)) = argv.next() else {
                        return Err(ParseError::MissingRedirectTarget);
                    };
                    match fd {
                        1 => {
                            stdout_path = Some(Target {
                                path: PathBuf::from(path),
                                append,
                            })
                        }
                        2 => {
                            stderr_path = Some(Target {
                                path: PathBuf::from(path),
                                append,
                            })
                        }
                        _ => return Err(ParseError::UnsupportedRedirect),
                    }
                }
            }
        }

        let name = words.first().ok_or(ParseError::MalformedInput)?;
        let args = &words[1..];

        Ok(Self {
            name: name.to_string(),
            args: args.to_vec(),
            stdout_path,
            stderr_path,
        })
    }

    pub(crate) fn open_redirects(&self) -> io::Result<(Option<File>, Option<File>)> {
        let stdout = self.stdout_path.as_ref().map(Target::open).transpose()?;
        let stderr = self.stderr_path.as_ref().map(Target::open).transpose()?;

        Ok((stdout, stderr))
    }
}

pub(crate) struct Target {
    path: PathBuf,
    append: bool,
}

impl Target {
    fn open(&self) -> io::Result<File> {
        OpenOptions::new()
            .write(true)
            .create(true)
            .append(self.append)
            .truncate(!self.append)
            .open(&self.path)
    }
}
#[derive(Debug)]
pub(crate) enum ParseError {
    MissingRedirectTarget,
    UnsupportedRedirect,
    MalformedInput,
}

impl std::error::Error for ParseError {}

impl From<ParseError> for ShellError {
    fn from(value: ParseError) -> Self {
        ShellError::Parse(value)
    }
}

impl Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::MissingRedirectTarget => write!(f, "missing redirect target"),
            ParseError::UnsupportedRedirect => {
                write!(f, "unsupported redirect target")
            }
            ParseError::MalformedInput => write!(f, "malformed command input"),
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

#[derive(Debug, PartialEq)]
enum Token {
    Word(String),
    Redirect { fd: u8, append: bool },
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
    let mut out_str: Vec<Token> = Vec::new();
    let mut current_str = String::new();

    let mut quote = Quote::None;
    let mut escaped = false;

    let mut input_iter = input.chars().peekable();
    while let Some(c) = input_iter.next() {
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
                let word = std::mem::take(&mut current_str);
                let append = input_iter.next_if_eq(&'>').is_some();
                match word.as_str() {
                    "1" | "" => out_str.push(Token::Redirect { fd: 1, append }),
                    "2" => out_str.push(Token::Redirect { fd: 2, append }),
                    _ => {
                        out_str.push(Token::Word(word));
                    }
                };
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

#[cfg(test)]
mod tests {
    use crate::parser::Token::{Redirect, Word};

    use super::*;

    #[test]
    fn redirect_operator_carries_its_fd() {
        for (line, fd, append) in [
            ("echo a > f", 1, false),
            ("echo a 1> f", 1, false),
            ("echo a 2> f", 2, false),
            ("echo a >> f", 1, true),
            ("echo a 2>> f", 2, true),
        ] {
            assert_eq!(
                tokenize(line),
                vec![
                    Word("echo".into()),
                    Word("a".into()),
                    Redirect { fd, append },
                    Word("f".into())
                ],
                "input: {line}",
            );
        }
    }

    #[test]
    fn unknown_redirect_prefix_skips_token() {
        let line = "echo boom 3> f";
        assert_eq!(
            tokenize(line),
            vec![
                Word("echo".into()),
                Word("boom".into()),
                Word("3".into()),
                Word("f".into())
            ],
        )
    }
}
