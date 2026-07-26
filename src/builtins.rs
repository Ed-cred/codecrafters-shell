use std::{path::PathBuf, process};

use crate::{parser::Command, OutputStreams, Shell, ShellError};

pub(crate) type BuiltinFn = fn(&mut OutputStreams<'_>, &Shell, &Command) -> Result<(), ShellError>;
pub(crate) static BUILTINS: &[(&str, BuiltinFn)] = &[
    ("exit", exit_cmd as BuiltinFn),
    ("echo", echo_cmd as BuiltinFn),
    ("type", type_cmd as BuiltinFn),
    ("pwd", pwd_cmd as BuiltinFn),
    ("cd", cd_cmd as BuiltinFn),
];

pub(crate) fn find_builtin(name: &str) -> Option<BuiltinFn> {
    BUILTINS
        .iter()
        .find_map(|&(builtin_name, func)| (builtin_name == name).then_some(func))
}

pub(crate) fn exit_cmd(
    _ctx: &mut OutputStreams<'_>,
    _shell: &Shell,
    cmd: &Command,
) -> Result<(), ShellError> {
    if !cmd.args.is_empty() {
        return Err(ShellError::ShellMessage(
            "exit: expected exactly zero arguments".to_string(),
        ));
    }
    process::exit(0)
}

pub(crate) fn echo_cmd(
    ctx: &mut OutputStreams<'_>,
    _shell: &Shell,
    cmd: &Command,
) -> Result<(), ShellError> {
    writeln!(ctx.stdout, "{}", cmd.args.join(" "))?;
    Ok(())
}

pub(crate) fn type_cmd(
    ctx: &mut OutputStreams<'_>,
    shell: &Shell,
    cmd: &Command,
) -> Result<(), ShellError> {
    if cmd.args.len() != 1 {
        return Err(ShellError::ShellMessage(
            "type: expected exactly one argument".to_string(),
        ));
    }
    let queried_program = cmd.args.first().unwrap();
    if let Some(_builtin) = find_builtin(queried_program) {
        writeln!(ctx.stdout, "{} is a shell builtin", queried_program)?;
        Ok(())
    } else if let Some(executable_path) = shell.try_find_executable(queried_program) {
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

pub(crate) fn pwd_cmd(
    ctx: &mut OutputStreams<'_>,
    _shell: &Shell,
    cmd: &Command,
) -> Result<(), ShellError> {
    if !cmd.args.is_empty() {
        return Err(ShellError::ShellMessage(
            "pwd: expected exactly zero arguments".to_string(),
        ));
    }
    let cwd = std::env::current_dir()?;
    writeln!(ctx.stdout, "{}", cwd.display())?;
    Ok(())
}

pub(crate) fn cd_cmd(
    _ctx: &mut OutputStreams<'_>,
    _shell: &Shell,
    cmd: &Command,
) -> Result<(), ShellError> {
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

pub(crate) fn external_cmd(
    ctx: &mut OutputStreams<'_>,
    shell: &Shell,
    cmd: &Command,
) -> Result<(), ShellError> {
    match shell.try_find_executable(&cmd.name) {
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
