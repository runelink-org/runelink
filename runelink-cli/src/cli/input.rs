use std::{
    any::type_name,
    io::{self, Write},
    str::FromStr,
};

use crate::error::CliError;

pub fn read_input(prompt: &str) -> io::Result<Option<String>> {
    let mut stdout = io::stdout();
    let stdin = io::stdin();

    stdout.write_all(prompt.as_bytes())?;
    stdout.flush()?;

    let mut buf = String::new();
    stdin.read_line(&mut buf)?;
    println!();

    let trimmed = buf.trim();

    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.into()))
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
