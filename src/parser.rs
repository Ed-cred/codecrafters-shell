use std::{fmt::Display, path::PathBuf};

use crate::ShellError;

pub(crate) struct Command {
    pub(crate) name: String,
    pub(crate) args: Vec<String>,
    pub(crate) redirect_path: Option<PathBuf>,
}

impl Command {
    pub(crate) fn parse(input: &str) -> Result<Self, ParseError> {
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

        let name = words.first().ok_or(ParseError::MalformedInput)?;
        let args = &words[1..];

        Ok(Self {
            name: name.to_string(),
            args: args.to_vec(),
            redirect_path,
        })
    }
}

#[derive(Debug)]
pub(crate) enum ParseError {
    MissingRedirectTarget,
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
