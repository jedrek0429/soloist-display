use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

static QUIET: AtomicBool = AtomicBool::new(false);
static VERBOSE: AtomicBool = AtomicBool::new(false);
static OUTPUT: OnceLock<Mutex<()>> = OnceLock::new();

pub fn configure(quiet: bool, verbose: bool) {
    QUIET.store(quiet, Ordering::Relaxed);
    VERBOSE.store(verbose, Ordering::Relaxed);
}

pub fn verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

pub fn success(message: String) {
    if !QUIET.load(Ordering::Relaxed) {
        let _guard = OUTPUT.get_or_init(|| Mutex::new(())).lock();
        println!("{message}");
    }
}

pub fn progress(step: usize, total: usize, label: &str) {
    if QUIET.load(Ordering::Relaxed) { return; }
    let _guard = OUTPUT.get_or_init(|| Mutex::new(())).lock();
    if io::stderr().is_terminal() && std::env::var("TERM").as_deref() != Ok("dumb") {
        eprintln!("  {step}/{total}  {label}");
    } else {
        eprintln!("  {step}/{total}  {label}");
    }
}

pub fn log_command(description: &str, stdout: &[u8], stderr: &[u8]) {
    if !verbose() { return; }
    let _guard = OUTPUT.get_or_init(|| Mutex::new(())).lock();
    let _ = writeln!(io::stderr(), "  command: {description}");
    if !stdout.is_empty() { let _ = io::stderr().write_all(stdout); }
    if !stderr.is_empty() { let _ = io::stderr().write_all(stderr); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verbose_configuration() {
        configure(false, true);
        assert!(verbose());
        configure(false, false);
        assert!(!verbose());
    }
}
