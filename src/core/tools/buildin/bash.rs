use crate::core::Tool;
use crate::core::tools::ExecutableTool;
use serde_json::{Value, json};
use std::fs::{self, File};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_OUTPUT_BYTES: usize = 50 * 1024;
const MAX_OUTPUT_LINES: usize = 2_000;
const MAX_TIMEOUT_SECONDS: f64 = 2_147_483.647;

pub(super) fn make_bash_tool() -> ExecutableTool {
    ExecutableTool {
		definition: Tool {
			name: "bash".to_string(),
			description: "Execute a bash command in the current working directory. Returns stdout and stderr. Output is truncated to the last 2000 lines or 50KB, with the full output saved to a temp file. Optionally provide a timeout in seconds.".to_string(),
			parameters: json!({
				"type": "object",
				"properties": {
					"command": { "type": "string", "description": "Bash command to execute." },
					"timeout": { "type": "number", "description": "Timeout in seconds (optional, no default timeout)." }
				},
				"required": ["command"],
				"additionalProperties": false
			}),
			constrained_sampling: None,
		},
		handler: Box::new(|_, parameters: Value| run_bash(parameters)),
	}
}

fn run_bash(parameters: Value) -> Result<Value, String> {
    let command = parameters["command"].as_str().unwrap();
    let timeout = parameters["timeout"].as_f64();
    if let Some(seconds) = timeout {
        if !seconds.is_finite() || seconds <= 0.0 || seconds > MAX_TIMEOUT_SECONDS {
            return Err(format!(
                "Invalid timeout: must be a finite number greater than 0 and at most {MAX_TIMEOUT_SECONDS} seconds"
            ));
        }
    }
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let mut shell = Command::new("bash");
    #[cfg(unix)]
    shell.process_group(0);
    let mut child = shell
        .args(["-c", command])
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let output_path = std::env::temp_dir().join(format!(
        "pi-bash-{}-{}.log",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut full_output = File::create(&output_path).map_err(|error| error.to_string())?;
    let (sender, receiver) = mpsc::channel();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    thread::spawn({
        let sender = sender.clone();
        move || forward_output(stdout, sender)
    });
    thread::spawn({
        let sender = sender.clone();
        move || forward_output(stderr, sender)
    });
    drop(sender);
    let started = Instant::now();
    let deadline = timeout.map(|seconds| started + Duration::from_secs_f64(seconds));
    let mut tail = Vec::new();
    let mut timed_out = false;
    let mut output_closed = false;
    let mut status = None;
    loop {
        match receiver.recv_timeout(Duration::from_millis(10)) {
            Ok(chunk) => {
                full_output
                    .write_all(&chunk)
                    .map_err(|error| error.to_string())?;
                tail.extend_from_slice(&chunk);
                if tail.len() > MAX_OUTPUT_BYTES * 2 {
                    tail.drain(..tail.len() - MAX_OUTPUT_BYTES * 2);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => output_closed = true,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if status.is_none() {
            status = child.try_wait().map_err(|error| error.to_string())?;
        }
        if status.is_none()
            && !timed_out
            && deadline.is_some_and(|deadline| Instant::now() >= deadline)
        {
            timed_out = true;
            kill_process_tree(&mut child);
        }
        if status.is_some() && output_closed {
            break;
        }
    }
    let status = status.unwrap();
    let wall_time_seconds = (started.elapsed().as_secs_f64() * 10.0).round() / 10.0;
    let output = String::from_utf8_lossy(&tail).into_owned();
    let (output, truncated) = truncate_output(&output);
    full_output.flush().map_err(|error| error.to_string())?;
    drop(full_output);
    if !truncated {
        fs::remove_file(&output_path).map_err(|error| error.to_string())?;
    }
    let full_output_path = truncated.then(|| output_path.to_string_lossy().into_owned());
    let display_output = if output.is_empty() {
        "(no output)"
    } else {
        &output
    };
    let display_output = full_output_path.as_ref().map_or_else(
        || display_output.to_string(),
        |path| format!("{display_output}\n\n[Output truncated. Full output: {path}]"),
    );
    if timed_out {
        return Err(format!(
            "{display_output}\n\nCommand timed out after {} seconds",
            timeout.unwrap()
        ));
    }
    let exit_code = status.code().unwrap_or(1);
    if exit_code != 0 {
        return Err(format!(
            "{display_output}\n\nCommand exited with code {exit_code}"
        ));
    }
    Ok(json!({
        "output": output,
        "truncated": truncated,
        "full_output_path": full_output_path,
        "exit_code": exit_code,
        "wall_time_seconds": wall_time_seconds
    }))
}

fn truncate_output(output: &str) -> (String, bool) {
    let lines: Vec<_> = output.lines().collect();
    let line_truncated = lines.len() > MAX_OUTPUT_LINES;
    let tail = lines[lines.len().saturating_sub(MAX_OUTPUT_LINES)..].join("\n");
    let byte_truncated = tail.len() > MAX_OUTPUT_BYTES;
    if byte_truncated {
        let start = tail
            .char_indices()
            .find(|(index, _)| tail.len() - index <= MAX_OUTPUT_BYTES)
            .map_or(tail.len(), |(index, _)| index);
        return (tail[start..].to_string(), true);
    }
    (tail, line_truncated)
}

fn forward_output<R: Read>(mut stream: R, sender: Sender<Vec<u8>>) {
    let mut buffer = [0; 8192];
    while let Ok(length) = stream.read(&mut buffer) {
        if length == 0 || sender.send(buffer[..length].to_vec()).is_err() {
            break;
        }
    }
}

#[cfg(unix)]
fn kill_process_tree(child: &mut std::process::Child) {
    unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
}

#[cfg(not(unix))]
fn kill_process_tree(child: &mut std::process::Child) {
    let _ = child.kill();
}
