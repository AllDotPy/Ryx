//! ANSI color helpers for the Ryx CLI.
//!
//! Automatically disables colours when ``NO_COLOR`` is set, ``TERM=dumb``,
//! or stdout is not a TTY.

use std::io::IsTerminal;

const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[34m";
const MAGENTA: &str = "\x1b[35m";
const CYAN: &str = "\x1b[36m";

fn use_colour() -> bool {
    if std::env::var("NO_COLOR").is_ok() {
        return false;
    }
    if matches!(std::env::var("TERM").as_deref(), Ok("dumb")) {
        return false;
    }
    std::io::stdout().is_terminal()
}

macro_rules! colour {
    ($name:ident, $code:expr) => {
        pub fn $name(s: &str) -> String {
            if use_colour() {
                format!("{}{}{}", $code, s, RESET)
            } else {
                s.to_string()
            }
        }
    };
}

colour!(red, RED);
colour!(green, GREEN);
colour!(yellow, YELLOW);
colour!(cyan, CYAN);
colour!(magenta, MAGENTA);

pub fn prefix() -> String {
    if use_colour() {
        format!("{BOLD}{BLUE}[ryx]{RESET}")
    } else {
        "[ryx]".to_string()
    }
}

pub fn ok_mark() -> String {
    if use_colour() {
        format!("{GREEN}\u{2713}{RESET}")
    } else {
        "\u{2713}".to_string()
    }
}

pub fn fail_mark() -> String {
    if use_colour() {
        format!("{RED}\u{2717}{RESET}")
    } else {
        "\u{2717}".to_string()
    }
}

pub fn warn_mark() -> String {
    if use_colour() {
        format!("{YELLOW}\u{26A0}{RESET}")
    } else {
        "\u{26A0}".to_string()
    }
}
