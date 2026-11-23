//! Own a runtime's process group so descendants cannot keep pipes or work alive.
use std::process::{Child, Command, Stdio};
use std::time::Duration;

pub(crate) fn isolate(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(not(unix))]
    let _ = command;
}

pub(crate) fn terminate(child: &mut Child) {
    #[cfg(unix)]
    {
        // Signal before reaping the leader: its PID cannot be reused as a new
        // process group while it remains our child. Never target Tauros's group.
        let group = format!("-{}", child.id());
        for signal in ["-TERM", "-KILL"] {
            let _ = Command::new("/bin/kill")
                .args([signal, "--", &group])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if signal == "-TERM" {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read};

    #[test]
    fn termination_closes_inherited_pipes_even_if_a_descendant_ignores_term() {
        let mut command = Command::new("/usr/bin/python3");
        command.args(["-c", "import os,signal,time\nif os.fork()==0:\n signal.signal(signal.SIGTERM,signal.SIG_IGN)\n print('child-ready',flush=True)\n time.sleep(60)\nelse:\n time.sleep(60)"]);
        isolate(&mut command);
        let mut child = command.stdout(Stdio::piped()).spawn().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "child-ready");
        terminate(&mut child);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut rest = String::new();
            tx.send(output.read_to_string(&mut rest)).unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_secs(2)).unwrap().is_ok());
        assert!(child.try_wait().unwrap().is_some());
    }
}
