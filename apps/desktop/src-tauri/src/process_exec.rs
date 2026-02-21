use std::io::{self, BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

#[cfg(unix)]
use std::os::unix::process::{CommandExt, ExitStatusExt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone)]
pub struct OutputLine {
    pub stream: OutputStream,
    pub line: String,
    pub ts: SystemTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputTransport {
    #[default]
    Pipes,
    PtyPreferred,
}

#[derive(Debug, Clone, Copy)]
pub struct ProcessOptions {
    pub timeout: Option<Duration>,
    pub output_transport: OutputTransport,
}

impl Default for ProcessOptions {
    fn default() -> Self {
        Self {
            timeout: None,
            output_transport: OutputTransport::Pipes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessExit {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub cancelled: bool,
    pub timed_out: bool,
}

pub struct ProcessHandle {
    pub pid: u32,
    child: Arc<Mutex<Child>>,
    stream_threads: Vec<JoinHandle<()>>,
    timeout_thread: Option<JoinHandle<()>>,
    finished: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    timed_out: Arc<AtomicBool>,
}

impl ProcessHandle {
    pub fn cancel(&self) -> io::Result<()> {
        self.cancelled.store(true, Ordering::SeqCst);

        #[cfg(unix)]
        {
            if let Err(err) = kill_process_group(self.pid) {
                self.kill_child_fallback().or(Err(err))
            } else {
                Ok(())
            }
        }

        #[cfg(not(unix))]
        {
            self.kill_child_fallback()
        }
    }

    pub fn wait(&mut self) -> io::Result<ProcessExit> {
        let status = {
            let mut child = self.lock_child()?;
            child.wait()?
        };

        self.finished.store(true, Ordering::SeqCst);

        if let Some(timeout_thread) = self.timeout_thread.take() {
            let _ = timeout_thread.join();
        }

        for stream_thread in self.stream_threads.drain(..) {
            let _ = stream_thread.join();
        }

        Ok(ProcessExit {
            success: status.success(),
            exit_code: status.code(),
            signal: exit_signal(&status),
            cancelled: self.cancelled.load(Ordering::SeqCst),
            timed_out: self.timed_out.load(Ordering::SeqCst),
        })
    }

    fn lock_child(&self) -> io::Result<std::sync::MutexGuard<'_, Child>> {
        self.child
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "process mutex poisoned"))
    }

    fn kill_child_fallback(&self) -> io::Result<()> {
        let mut child = self.lock_child()?;
        match child.kill() {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::InvalidInput => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        }
    }
}

pub fn spawn_process(
    program: &str,
    args: &[&str],
    options: ProcessOptions,
) -> io::Result<(ProcessHandle, mpsc::Receiver<OutputLine>)> {
    if options.output_transport == OutputTransport::PtyPreferred {
        if let Some(pty_result) = try_spawn_with_pty(program, args, options)? {
            return Ok(pty_result);
        }
    }

    spawn_with_pipes(program, args, options)
}

fn spawn_with_pipes(
    program: &str,
    args: &[&str],
    options: ProcessOptions,
) -> io::Result<(ProcessHandle, mpsc::Receiver<OutputLine>)> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(unix)]
    {
        // Spawn each child in its own process group so cancellation can kill the full tree.
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) == 0 {
                    Ok(())
                } else {
                    Err(io::Error::last_os_error())
                }
            });
        }
    }

    let mut child = command.spawn()?;
    let pid = child.id();

    let (output_tx, output_rx) = mpsc::channel();
    let mut stream_threads = Vec::new();

    if let Some(stdout) = child.stdout.take() {
        stream_threads.push(spawn_stream_reader(stdout, OutputStream::Stdout, output_tx.clone()));
    }

    if let Some(stderr) = child.stderr.take() {
        stream_threads.push(spawn_stream_reader(stderr, OutputStream::Stderr, output_tx.clone()));
    }

    drop(output_tx);

    let child = Arc::new(Mutex::new(child));
    let finished = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicBool::new(false));
    let timed_out = Arc::new(AtomicBool::new(false));

    let timeout_thread = options.timeout.map(|timeout| {
        spawn_timeout_watchdog(
            pid,
            timeout,
            Arc::clone(&child),
            Arc::clone(&finished),
            Arc::clone(&timed_out),
        )
    });

    Ok((
        ProcessHandle {
            pid,
            child,
            stream_threads,
            timeout_thread,
            finished,
            cancelled,
            timed_out,
        },
        output_rx,
    ))
}

fn spawn_stream_reader<R: Read + Send + 'static>(
    stream: R,
    stream_name: OutputStream,
    output_tx: mpsc::Sender<OutputLine>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let reader = BufReader::new(stream);

        for line_result in reader.lines() {
            let Ok(line) = line_result else {
                break;
            };

            if output_tx
                .send(OutputLine {
                    stream: stream_name,
                    line,
                    ts: SystemTime::now(),
                })
                .is_err()
            {
                break;
            }
        }
    })
}

fn spawn_timeout_watchdog(
    pid: u32,
    timeout: Duration,
    child: Arc<Mutex<Child>>,
    finished: Arc<AtomicBool>,
    timed_out: Arc<AtomicBool>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        thread::sleep(timeout);

        if finished.load(Ordering::SeqCst) {
            return;
        }

        let kill_result = kill_by_pid_or_child(pid, &child);
        if kill_result.is_ok() {
            timed_out.store(true, Ordering::SeqCst);
        }
    })
}

fn kill_by_pid_or_child(pid: u32, child: &Arc<Mutex<Child>>) -> io::Result<()> {
    #[cfg(unix)]
    {
        return kill_process_group(pid).or_else(|_| kill_child(child));
    }

    #[cfg(not(unix))]
    {
        let _ = pid;
        kill_child(child)
    }
}

fn kill_child(child: &Arc<Mutex<Child>>) -> io::Result<()> {
    let mut child = child
        .lock()
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "process mutex poisoned"))?;

    match child.kill() {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::InvalidInput => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

#[cfg(unix)]
fn kill_process_group(pid: u32) -> io::Result<()> {
    let rc = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) };
    if rc == 0 {
        return Ok(());
    }

    let err = io::Error::last_os_error();
    if matches!(err.raw_os_error(), Some(code) if code == libc::ESRCH) {
        return Ok(());
    }

    Err(err)
}

#[cfg(not(unix))]
fn kill_process_group(_pid: u32) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "process groups are not supported on this platform",
    ))
}

#[cfg(unix)]
fn exit_signal(status: &std::process::ExitStatus) -> Option<i32> {
    status.signal()
}

#[cfg(not(unix))]
fn exit_signal(_status: &std::process::ExitStatus) -> Option<i32> {
    None
}

fn try_spawn_with_pty(
    _program: &str,
    _args: &[&str],
    _options: ProcessOptions,
) -> io::Result<Option<(ProcessHandle, mpsc::Receiver<OutputLine>)>> {
    // PTY transport stays as an abstraction point; MVP falls back to standard pipes.
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::{spawn_process, OutputStream, ProcessOptions};
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn streams_stdout_lines_for_a_short_process() {
        let (mut handle, output_rx) = spawn_process(
            "bash",
            &["-lc", "for i in 1 2 3; do echo hi$i; sleep 0.1; done"],
            ProcessOptions::default(),
        )
        .expect("spawn process");

        let exit = handle.wait().expect("wait for process");
        assert!(exit.success, "process should succeed: {exit:?}");

        let stdout_lines = output_rx
            .into_iter()
            .filter(|event| event.stream == OutputStream::Stdout)
            .map(|event| event.line)
            .collect::<Vec<_>>();

        assert_eq!(stdout_lines, vec!["hi1", "hi2", "hi3"]);
    }

    #[test]
    fn cancel_terminates_a_long_running_process() {
        let (mut handle, _output_rx) =
            spawn_process("bash", &["-lc", "sleep 30"], ProcessOptions::default())
                .expect("spawn process");

        thread::sleep(Duration::from_millis(150));

        let started = Instant::now();
        handle.cancel().expect("cancel process");
        let exit = handle.wait().expect("wait for process");

        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(exit.cancelled);
        assert!(!exit.success, "cancelled process must not succeed: {exit:?}");
    }
}
