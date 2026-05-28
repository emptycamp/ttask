use crate::error::{Error, Result};
use std::io::{self, BufRead, Write};

pub trait Prompt {
    fn confirm(&self, msg: &str) -> Result<bool>;
}

pub struct StdinPrompt;

impl Prompt for StdinPrompt {
    fn confirm(&self, msg: &str) -> Result<bool> {
        print!("{msg} [y/N] ");
        io::stdout().flush().map_err(Error::Io)?;
        let mut line = String::new();
        io::stdin().lock().read_line(&mut line).map_err(Error::Io)?;
        Ok(matches!(line.trim().to_lowercase().as_str(), "y" | "yes"))
    }
}

pub struct AutoConfirm;

impl Prompt for AutoConfirm {
    fn confirm(&self, _msg: &str) -> Result<bool> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_confirm_always_returns_true() {
        assert!(AutoConfirm.confirm("test?").unwrap());
    }
}
