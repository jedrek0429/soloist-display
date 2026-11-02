use std::io::{self, IsTerminal, Write};
use std::fs::{self, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex, OnceLock,
};
use std::{thread, time::Duration};

static QUIET: AtomicBool = AtomicBool::new(false);
static VERBOSE: AtomicBool = AtomicBool::new(false);
static OUTPUT: OnceLock<Mutex<()>> = OnceLock::new();
static LOG_PATH: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

pub fn set_log_path(path: PathBuf) {
    *LOG_PATH.get_or_init(|| Mutex::new(None)).lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
}

pub fn log_event(operation: &str, status: &str, detail: &str) {
    let Some(path) = LOG_PATH.get().and_then(|p| p.lock().ok().and_then(|g| g.clone())) else { return; };
    if let Some(parent) = path.parent() { if fs::create_dir_all(parent).is_err() { return; } }
    let _guard = output_lock().lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).mode(0o600).open(path) {
        let entry = serde_json::json!({"timestamp": crate::state::now(), "operation": operation, "status": status, "detail": detail});
        let _ = writeln!(file, "{entry}");
    }
}


fn output_lock() -> &'static Mutex<()> {
    OUTPUT.get_or_init(|| Mutex::new(()))
}
fn interactive() -> bool {
    io::stderr().is_terminal()
        && std::env::var("TERM").as_deref() != Ok("dumb")
        && std::env::var_os("CI").is_none()
}
pub fn configure(quiet: bool, verbose: bool) {
    QUIET.store(quiet, Ordering::Relaxed);
    VERBOSE.store(verbose && !quiet, Ordering::Relaxed);
}
pub fn verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}
pub fn success(message: String) {
    if !QUIET.load(Ordering::Relaxed) {
        let _guard = output_lock().lock().unwrap_or_else(|e| e.into_inner());
        println!("{message}");
    }
}
pub fn log_command(description: &str, stdout: &[u8], stderr: &[u8]) {
    log_event(description, if stderr.is_empty() { "command" } else { "command-output" }, &String::from_utf8_lossy(stderr).chars().take(8192).collect::<String>());
    if !verbose() {
        return;
    }
    let _guard = output_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut err = io::stderr().lock();
    let _ = writeln!(err, "  command: {description}");
    if !stdout.is_empty() {
        let _ = err.write_all(stdout);
    }
    if !stderr.is_empty() {
        let _ = err.write_all(stderr);
    }
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
                        let bar = format!(
                            "{}{}",
                            "=".repeat(number * 20 / 4),
                            "-".repeat(20 - number * 20 / 4)
                        );
                        let _guard = output_lock().lock().unwrap_or_else(|e| e.into_inner());
                        eprint!(
                            "\r\x1b[2K  {} [{}]  {number}/4  {label}",
                            frames[tick % 4],
                            bar
                        );
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
        } else {
            None
        };
        Self {
            quiet,
            latest,
            active,
            highest: AtomicUsize::new(0),
            worker,
        }
    }
    pub fn step(&self, number: usize, label: &'static str) {
        if number == 0 || number > 4 {
            return;
        }
        let previous = self.highest.fetch_max(number, Ordering::Relaxed);
        if number <= previous {
            return;
        }
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
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_pty_probe() {
        let Ok(mode) = std::env::var("SOLOIST_DISPLAY_PTY_PROBE") else { return; };
        configure(mode == "quiet", false);
        {
            let p = Progress::new(mode == "quiet", "probe");
            p.step(1, "Checking release");
            std::thread::sleep(Duration::from_millis(350));
        }
        success("Probe complete.".to_string());
    }
    #[test]
    fn terminal_pty_integration() {
        // A real pseudoterminal catches output races hidden by pipe-based tests.
        let script = r#"
import os, pty, re, select, subprocess, sys, time
master, slave = pty.openpty()
env = os.environ.copy()
env.pop("CI", None)
env["TERM"] = "xterm"
env["SOLOIST_DISPLAY_PTY_PROBE"] = sys.argv[2]
p = subprocess.Popen([sys.argv[1], "--exact", "terminal::tests::terminal_pty_probe", "--nocapture"],
                     stdin=slave, stdout=slave, stderr=slave, env=env)
os.close(slave)
raw = b""
deadline = time.monotonic() + 10
while time.monotonic() < deadline:
    readable, _, _ = select.select([master], [], [], .1)
    if readable:
        try:
            chunk = os.read(master, 8192)
            if not chunk: break
            raw += chunk
        except OSError: break
    if p.poll() is not None and not readable: break
p.wait(timeout=2)
os.close(master)
assert p.returncode == 0, raw
text = raw.decode("utf-8", "replace")
if sys.argv[2] == "quiet":
    assert "Checking release" not in text, text
    assert "Probe complete." not in text, text
else:
    assert "Probe complete." in text, text
    assert "\r\x1b[2K" in text, repr(text)
    assert text.index("Checking release") < text.index("Probe complete."), text
    assert re.search(r"\r\x1b\[2K(?:\r?\n)?Probe complete\.", text), repr(text)
"#;
        for mode in ["normal", "quiet"] {
            let result = std::process::Command::new("python3")
                .arg("-c").arg(script)
                .arg(std::env::current_exe().unwrap())
                .arg(mode).output().expect("python3 is required for PTY tests");
            assert!(result.status.success(), "PTY {mode}: {}", String::from_utf8_lossy(&result.stderr));
        }
    }
    #[test]
    fn progresses_monotonically() {
        let p = Progress::new(true, "test");
        p.step(1, "first");
        p.step(4, "last");
        p.step(3, "stale");
        assert_eq!(p.highest.load(Ordering::Relaxed), 4);
        assert_eq!(p.latest.lock().unwrap().0, 4);
    }
}
