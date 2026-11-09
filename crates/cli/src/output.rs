//! Shared, dependency-free CLI presentation for Linux and desktop previews.
//! Diagnostic output stays readable when piped or when NO_COLOR is set.

use std::io::{self, IsTerminal};

fn color_enabled() -> bool {
    io::stdout().is_terminal()
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").as_deref() != Ok("dumb")
}

fn paint(text: &str, sgr: &str, color: bool) -> String {
    if color {
        format!("\x1b[{sgr}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

pub fn heading(mode: &str) {
    println!("{}", paint("Soloist Display", "1", color_enabled()));
    println!("{mode}");
    println!();
}

pub fn section(label: &str) {
    println!("{}:", paint(label, "1", color_enabled()));
}

pub fn field(label: &str, value: impl std::fmt::Display) {
    println!("  {label:<24} {value}");
}

pub fn check(label: &str, passed: bool, hint: &str) -> bool {
    let status = if passed { "OK" } else { "FAILED" };
    let colored = paint(status, if passed { "32" } else { "31" }, color_enabled());
    field(label, colored);
    if !passed && !hint.is_empty() {
        println!("    Hint: {hint}");
    }
    passed
}

#[allow(dead_code)] // The Linux installer uses clap for help; desktop uses this formatter.
pub fn command(syntax: &str, description: &str) {
    println!("  {syntax:<32} {description}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting_respects_plain_output() {
        assert_eq!(paint("FAILED", "31", false), "FAILED");
        assert_eq!(paint("OK", "32", true), "\x1b[32mOK\x1b[0m");
    }
}
