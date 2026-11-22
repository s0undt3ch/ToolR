//! The one yes/no question toolr asks on a terminal.

use std::io::{BufRead, Write};

/// Ask `question` and read one answer line. `y` and `yes` (any case) accept, `n` and `no`
/// decline, a bare Enter takes `default`, and anything else, EOF or a read error declines.
/// EOF echoes no newline, so one is written to keep what follows off the prompt line.
pub fn yes_no(input: &mut dyn BufRead, out: &mut dyn Write, question: &str, default: bool) -> bool {
    let hint = if default { "[Y/n]" } else { "[y/N]" };
    let _ = write!(out, "{question} {hint} ");
    let _ = out.flush();
    let mut answer = String::new();
    match input.read_line(&mut answer) {
        Ok(n) if n > 0 => match answer.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => true,
            "" => default,
            _ => false,
        },
        _ => {
            let _ = writeln!(out);
            false
        }
    }
}

/// [`yes_no`] on the process's stdin and stderr.
pub fn yes_no_stdin(question: &str, default: bool) -> bool {
    yes_no(
        &mut std::io::stdin().lock(),
        &mut std::io::stderr(),
        question,
        default,
    )
}

#[cfg(test)]
mod tests;
