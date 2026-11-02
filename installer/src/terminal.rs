use std::io::{self, IsTerminal, Write};
use std::sync::{atomic::{AtomicBool, AtomicUsize, Ordering}, Arc, Mutex, OnceLock};
use std::{thread, time::Duration};

static QUIET: AtomicBool = AtomicBool::new(false);
static VERBOSE: AtomicBool = AtomicBool::new(false);
static OUTPUT: OnceLock<Mutex<()>> = OnceLock::new();

fn output_lock() -> &'static Mutex<()> { OUTPUT.get_or_init(|| Mutex::new(())) }
fn interactive() -> bool {
    io::stderr().is_terminal()
        && std::env::var("TERM").as_deref() != Ok("dumb")
        && std::env::var_os("CI").is_none()
}
pub fn configure(quiet: bool, verbose: bool) {
    QUIET.store(quiet, Ordering::Relaxed);
    VERBOSE.store(verbose && !quiet, Ordering::Relaxed);
}
pub fn verbose() -> bool { VERBOSE.load(Ordering::Relaxed) }
pub fn success(message: String) {
    if !QUIET.load(Ordering::Relaxed) {
        let _guard = output_lock().lock().unwrap_or_else(|e| e.into_inner());
        println!("{message}");
    }
}
pub fn log_command(description: &str, stdout: &[u8], stderr: &[u8]) {
    if !verbose() { return; }
    let _guard = output_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut err = io::stderr().lock();
    let _ = writeln!(err, "  command: {description}");
    if !stdout.is_empty() { let _ = err.write_all(stdout); }
    if !stderr.is_empty() { let _ = err.write_all(stderr); }
}

pub struct Progress {
    quiet: bool,
    latest: Arc<Mutex<(usize, &'static str)>>,
    active: Arc<AtomicBool>,
    highest: AtomicUsize,
    worker: Option<thread::JoinHandle<()>>,
}

impl Progress {
    pub fn new(quiet: bool, _label: &'static str) -> Self {
        let quiet = quiet || QUIET.load(Ordering::Relaxed);
        let latest = Arc::new(Mutex::new((0, "")));
        let active = Arc::new(AtomicBool::new(true));
        let worker = if !quiet && interactive() && !verbose() {
            let state = latest.clone();
            let running = active.clone();
            Some(thread::spawn(move || {
                // Do not flash a progress bar for operations completed immediately.
                thread::sleep(Duration::from_millis(180));
                let frames = ["|", "/", "-", "\\"];
                let mut tick = 0usize;
                let mut drawn = false;
                while running.load(Ordering::Acquire) {
                    let (number, label) = *state.lock().unwrap_or_else(|e| e.into_inner());
                    if number > 0 {
                        let bar = format!("{}{}", "=".repeat(number * 20 / 4), "-".repeat(20 - number * 20 / 4));
                        let _guard = output_lock().lock().unwrap_or_else(|e| e.into_inner());
                        eprint!("\r\x1b[2K  {} [{}]  {number}/4  {label}", frames[tick % 4], bar);
                        let _ = io::stderr().flush();
                        drawn = true;
                    }
                    tick += 1;
                    thread::sleep(Duration::from_millis(90));
                }
                if drawn {
                    let _guard = output_lock().lock().unwrap_or_else(|e| e.into_inner());
                    eprint!("\r\x1b[2K");
                    let _ = io::stderr().flush();
                }
            }))
        } else { None };
        Self { quiet, latest, active, highest: AtomicUsize::new(0), worker }
    }
    pub fn step(&self, number: usize, label: &'static str) {
        if number == 0 || number > 4 { return; }
        let previous = self.highest.fetch_max(number, Ordering::Relaxed);
        if number <= previous { return; }
        *self.latest.lock().unwrap_or_else(|e| e.into_inner()) = (number, label);
        if !self.quiet && self.worker.is_none() {
            let _guard = output_lock().lock().unwrap_or_else(|e| e.into_inner());
            eprintln!("  {number}/4  {label}");
        }
    }
}
impl Drop for Progress {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progresses_monotonically() {
        let p = Progress::new(true, "test");
        p.step(1, "first"); p.step(4, "last"); p.step(3, "stale");
        assert_eq!(p.highest.load(Ordering::Relaxed), 4);
        assert_eq!(p.latest.lock().unwrap().0, 4);
    }
}
