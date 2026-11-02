use std::{
    any::type_name,
    io::{self, IsTerminal, Write},
    str::FromStr,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::error::CliError;

/// Set once from `--non-interactive` before any command runs. Prompts read
/// as blank input while set, and selections fail. Confirmations must treat
/// blank input as no.
static NON_INTERACTIVE: AtomicBool = AtomicBool::new(false);

pub fn set_non_interactive(non_interactive: bool) {
    NON_INTERACTIVE.store(non_interactive, Ordering::Relaxed);
}

pub fn is_non_interactive() -> bool {
    NON_INTERACTIVE.load(Ordering::Relaxed)
}

/// Whether prompts that need a terminal can be shown.
pub fn can_prompt() -> bool {
    !is_non_interactive() && io::stdin().is_terminal()
}

pub fn read_input(prompt: &str) -> io::Result<Option<String>> {
    read_input_internal(prompt, true)
}

pub fn read_password(prompt: &str) -> io::Result<Option<String>> {
    if is_non_interactive() {
        return Ok(None);
    }
    let password = rpassword::prompt_password(prompt)?;
    Ok((!password.is_empty()).then_some(password))
}

pub fn read_input_preserving_whitespace(
    prompt: &str,
) -> io::Result<Option<String>> {
    read_input_internal(prompt, false)
}

/// Reads a password from the first line of stdin, for `--password-stdin`.
pub fn read_password_stdin() -> Result<String, CliError> {
    if io::stdin().is_terminal() {
        return Err(CliError::InvalidArgument(
            "--password-stdin needs the password piped to stdin.".into(),
        ));
    }
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    let line = buf.strip_suffix('\n').unwrap_or(&buf);
    let password = line.strip_suffix('\r').unwrap_or(line);
    if password.is_empty() {
        return Err(CliError::InvalidArgument(
            "No password provided on stdin.".into(),
        ));
    }
    Ok(password.into())
}

fn read_input_internal(prompt: &str, trim: bool) -> io::Result<Option<String>> {
    if is_non_interactive() {
        return Ok(None);
    }
    let mut stdout = io::stdout();
    let stdin = io::stdin();

    stdout.write_all(prompt.as_bytes())?;
    stdout.flush()?;

    let mut buf = String::new();
    stdin.read_line(&mut buf)?;
    println!();

    let raw = buf.trim_end_matches(['\n', '\r']);

    if raw.is_empty() {
        Ok(None)
    } else if trim {
        Ok(Some(raw.trim().into()))
    } else {
        Ok(Some(raw.into()))
    }
}

pub fn unwrap_or_prompt<T: FromStr>(
    arg: Option<T>,
    prompt: &str,
) -> Result<T, CliError> {
    if let Some(arg) = arg {
        Ok(arg)
    } else {
        read_input(format!("{}: ", prompt).as_str())?
            .ok_or_else(|| {
                CliError::InvalidArgument(format!("{} is required.", prompt))
            })?
            .parse::<T>()
            .map_err(|_| {
                CliError::InvalidArgument(format!(
                    "Invalid {} input.",
                    type_name::<T>()
                ))
            })
    }
}
