pub mod interactive;
pub mod table;

use colored::Colorize;

pub fn info(msg: &str) {
    eprintln!(" {}  {}", "ℹ".blue().bold(), msg);
}

pub fn success(msg: &str) {
    eprintln!(" {}  {}", "✓".green().bold(), msg);
}

pub fn warn(msg: &str) {
    eprintln!(" {}  {}", "⚠".yellow().bold(), msg);
}

pub fn error(msg: &str) {
    eprintln!(" {}  {}", "✗".red().bold(), msg);
}

pub fn hint(msg: &str) {
    eprintln!(" {}  {}", "→".cyan().bold(), msg);
}

pub fn section(title: &str) {
    eprintln!();
    eprintln!("{}", format!("╭─ {} ─", title).cyan().bold());
}

pub fn subsection(title: &str) {
    eprintln!("{}", format!("{} ", title).yellow().bold());
}
