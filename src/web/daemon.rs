use std::{
    env, fs,
    io::{self, ErrorKind},
    process::{Command, Stdio},
    time::Duration,
};

use super::runtime::{
    clear_runtime_files, info_path, is_process_running, log_path, pid_path, read_pid,
    read_runtime_info, send_signal,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Foreground,
    DaemonParent,
    DaemonChild,
    Stop,
    Status,
    Restart,
    Logs { lines: usize },
}

pub fn parse_mode() -> Result<RunMode, Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut selected: Option<RunMode> = None;
    let mut log_lines = 100_usize;

    let mut idx = 0_usize;
    while idx < args.len() {
        let arg = args[idx].as_str();

        match arg {
            "--daemon" => set_mode_once(&mut selected, RunMode::DaemonParent)?,
            "--run-daemon" => set_mode_once(&mut selected, RunMode::DaemonChild)?,
            "--stop" => set_mode_once(&mut selected, RunMode::Stop)?,
            "--status" => set_mode_once(&mut selected, RunMode::Status)?,
            "--restart" => set_mode_once(&mut selected, RunMode::Restart)?,
            "--logs" => set_mode_once(&mut selected, RunMode::Logs { lines: log_lines })?,
            "--lines" => {
                if !matches!(selected, Some(RunMode::Logs { .. })) {
                    return Err("--lines can only be used together with --logs".into());
                }
                idx += 1;
                let Some(value) = args.get(idx) else {
                    return Err("--lines requires a numeric value".into());
                };
                log_lines = parse_log_lines(value)?;
            }
            other if other.starts_with("--logs=") => {
                let value = other.trim_start_matches("--logs=");
                log_lines = parse_log_lines(value)?;
                set_mode_once(&mut selected, RunMode::Logs { lines: log_lines })?;
            }
            other => return Err(format!("unknown argument: {other}").into()),
        }

        idx += 1;
    }

    let mode = match selected {
        Some(RunMode::Logs { .. }) => RunMode::Logs { lines: log_lines },
        Some(other) => other,
        None => RunMode::Foreground,
    };

    Ok(mode)
}

pub fn handle_lifecycle_mode(mode: RunMode) -> Result<bool, Box<dyn std::error::Error>> {
    match mode {
        RunMode::Stop => {
            stop_daemon()?;
            Ok(true)
        }
        RunMode::Status => {
            print_status()?;
            Ok(true)
        }
        RunMode::Restart => {
            restart_daemon()?;
            Ok(true)
        }
        RunMode::Logs { lines } => {
            print_logs(lines)?;
            Ok(true)
        }
        RunMode::DaemonParent => {
            start_daemon_parent()?;
            Ok(true)
        }
        RunMode::Foreground | RunMode::DaemonChild => Ok(false),
    }
}

fn set_mode_once(
    selected: &mut Option<RunMode>,
    mode: RunMode,
) -> Result<(), Box<dyn std::error::Error>> {
    if selected.is_some() {
        return Err("use only one mode flag at a time".into());
    }
    *selected = Some(mode);
    Ok(())
}

fn parse_log_lines(value: &str) -> Result<usize, Box<dyn std::error::Error>> {
    let lines = value
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("invalid log line count: {value}"))?;
    if lines == 0 {
        return Err("log line count must be greater than zero".into());
    }
    Ok(lines)
}

fn start_daemon_parent() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(pid) = read_pid()? {
        if is_process_running(pid) {
            println!("web_server already running (pid {pid}).");
            print_status()?;
            return Ok(());
        }
        let _ = clear_runtime_files();
    }

    let log_path = log_path().ok_or("unable to resolve log path")?;
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let log_file_err = log_file.try_clone()?;

    let exe = env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg("--run-daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    let child = cmd.spawn()?;
    println!("web_server daemon starting (pid {}).", child.id());

    for _ in 0..80 {
        if let Some(info) = read_runtime_info()? {
            println!("local web UI: {}", info.local_url);
            if let Some(url) = info.public_url.as_ref() {
                println!("public web UI: {url}");
            }
            if let Some(path) = info.auth_file.as_ref() {
                println!("web auth file: {path}");
            }
            println!("log file: {}", log_path.display());
            println!("restart with: web_server --restart");
            println!("view logs with: web_server --logs");
            println!("stop with: web_server --stop");
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    println!(
        "daemon started but startup info was not written yet. check logs: {}",
        log_path.display()
    );
    Ok(())
}

fn stop_daemon() -> Result<(), Box<dyn std::error::Error>> {
    let Some(pid) = read_pid()? else {
        println!("web_server is not running (no pid file).");
        if let Some(path) = pid_path() {
            println!("pid path: {}", path.display());
        }
        return Ok(());
    };

    if !is_process_running(pid) {
        println!("stale pid file found for pid {pid}; cleaning up runtime files.");
        if let Some(path) = pid_path() {
            println!("removed stale pid file: {}", path.display());
        }
        clear_runtime_files()?;
        return Ok(());
    }

    send_signal(pid, libc::SIGTERM)?;
    for _ in 0..50 {
        if !is_process_running(pid) {
            clear_runtime_files()?;
            println!("web_server stopped.");
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    println!("graceful stop timed out; sending SIGKILL.");
    let _ = send_signal(pid, libc::SIGKILL);
    clear_runtime_files()?;
    println!("web_server stopped.");
    Ok(())
}

fn restart_daemon() -> Result<(), Box<dyn std::error::Error>> {
    let pid = read_pid()?;
    match pid {
        Some(pid) if is_process_running(pid) => {
            println!("restarting web_server daemon (pid {pid})...");
            stop_daemon()?;
        }
        Some(pid) => {
            println!("found stale pid {pid}; cleaning up before restart.");
            clear_runtime_files()?;
        }
        None => {
            println!("web_server is not running; starting daemon.");
        }
    }

    start_daemon_parent()
}

fn print_logs(lines: usize) -> Result<(), Box<dyn std::error::Error>> {
    let path = log_path().ok_or("unable to resolve log path")?;
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            println!("log file not found: {}", path.display());
            println!("start daemon with: web_server --daemon");
            return Ok(());
        }
        Err(err) => return Err(err.into()),
    };

    let rows: Vec<&str> = text.lines().collect();
    if rows.is_empty() {
        println!("log file is empty: {}", path.display());
        return Ok(());
    }

    let start = rows.len().saturating_sub(lines);
    println!(
        "showing last {} line(s) from {}",
        rows.len() - start,
        path.display()
    );
    for row in rows.into_iter().skip(start) {
        println!("{row}");
    }

    Ok(())
}

fn print_status() -> Result<(), Box<dyn std::error::Error>> {
    let pid = read_pid()?;
    let info = read_runtime_info()?;

    match pid {
        Some(pid) if is_process_running(pid) => {
            println!("web_server status: running (pid {pid})");
            if let Some(info) = info {
                println!("local web UI: {}", info.local_url);
                if let Some(url) = info.public_url.as_ref() {
                    println!("public web UI: {url}");
                }
                if let Some(path) = info.auth_file.as_ref() {
                    println!("web auth file: {path}");
                }
                if let Some(path) = info.log_file.as_ref() {
                    println!("log file: {path}");
                }
            } else {
                println!("runtime info file missing or unreadable.");
                if let Some(path) = info_path() {
                    println!("expected runtime info at: {}", path.display());
                }
            }
            println!("restart with: web_server --restart");
            println!("view logs with: web_server --logs");
        }
        Some(pid) => {
            println!("web_server status: not running (stale pid file for pid {pid})");
            if let Some(path) = pid_path() {
                println!("stale pid file: {}", path.display());
            }
            if info.is_some() {
                if let Some(path) = info_path() {
                    println!("stale runtime info file: {}", path.display());
                }
            }
            println!("cleanup with: web_server --stop");
            println!("or restart with: web_server --restart");
        }
        None => {
            if info.is_some() {
                println!("web_server status: not running (runtime info exists without pid)");
                if let Some(path) = info_path() {
                    println!("stale runtime info file: {}", path.display());
                }
                println!("cleanup with: web_server --stop");
            } else {
                println!("web_server status: not running");
            }
        }
    }

    Ok(())
}
