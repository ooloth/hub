//! What a child started by `disclaimed_spawn::spawn` gets, observed in real processes.

// The crate is empty off macOS (decision 031), so there is nothing here to test.
#![cfg(target_os = "macos")]

use std::io;
use std::time::{Duration, Instant};

/// Whether `pid` is still in the process table, as a running process or a zombie.
fn exists(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

#[tokio::test]
async fn output_returns_what_the_child_wrote_and_its_exit_code() {
    let child =
        disclaimed_spawn::spawn("/bin/sh", &["-c", "printf out; printf err >&2; exit 3"]).unwrap();

    let output = child.output().await.unwrap();

    assert_eq!(output.stdout, b"out");
    assert_eq!(output.stderr, b"err");
    assert_eq!(output.status.code(), Some(3));
}

#[tokio::test]
async fn a_mebibyte_on_each_stream_comes_back_whole() {
    // stderr is written first and in full, so a reader that drains stdout before stderr would
    // leave the child blocked on a full stderr pipe and never see stdout's end.
    let script = "head -c 1048576 /dev/zero >&2; head -c 1048576 /dev/zero";
    let child = disclaimed_spawn::spawn("/bin/sh", &["-c", script]).unwrap();

    let output = tokio::time::timeout(Duration::from_secs(20), child.output())
        .await
        .expect("output stalled on a full pipe")
        .unwrap();

    assert_eq!(output.stdout.len(), 1_048_576);
    assert_eq!(output.stderr.len(), 1_048_576);
    assert!(output.status.success());
}

#[tokio::test]
async fn dropping_output_before_the_child_exits_ends_the_child() {
    let child = disclaimed_spawn::spawn("/bin/sleep", &["30"]).unwrap();
    let pid = child.pid();
    assert!(exists(pid), "the child was not running to begin with");

    let unfinished = tokio::time::timeout(Duration::from_millis(200), child.output()).await;
    assert!(unfinished.is_err(), "sleep 30 finished within 200ms");

    // The child is killed and reaped, so its pid leaves the process table: a zombie would still
    // answer `kill -0`.
    let deadline = Instant::now() + Duration::from_secs(5);
    while exists(pid) {
        assert!(
            Instant::now() < deadline,
            "pid {pid} still there 5s after the drop"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn a_program_given_as_a_path_takes_arguments_given_as_strings() {
    let program = std::path::PathBuf::from("/bin/echo");

    let output = disclaimed_spawn::spawn(program, &["hi"])
        .unwrap()
        .output()
        .await
        .unwrap();

    assert_eq!(output.stdout, b"hi\n");
}

#[tokio::test]
async fn a_program_that_does_not_exist_is_not_found() {
    let no_arguments: [&str; 0] = [];

    let error = disclaimed_spawn::spawn("hub-no-such-program", &no_arguments).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

mod responsibility_and_descriptors {
    use std::os::fd::AsRawFd;
    use std::time::Duration;

    /// The process macOS holds responsible for `pid`.
    #[allow(unsafe_code)]
    fn responsible_for(pid: u32) -> i32 {
        extern "C" {
            fn responsibility_get_pid_responsible_for_pid(pid: libc::pid_t) -> libc::pid_t;
        }
        let pid = libc::pid_t::try_from(pid).unwrap();
        // SAFETY: the function takes a pid by value and returns one; it touches no memory of ours.
        unsafe { responsibility_get_pid_responsible_for_pid(pid) }
    }

    #[tokio::test]
    async fn a_spawned_child_is_its_own_responsible_process() {
        let child = disclaimed_spawn::spawn("/bin/sleep", &["5"]).unwrap();
        let pid = child.pid();
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert_eq!(responsible_for(pid), i32::try_from(pid).unwrap());
    }

    /// Without this, the test above could pass because the oracle answers every pid with itself.
    #[test]
    fn a_child_started_the_ordinary_way_answers_to_its_parents_responsible_process() {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("5")
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));

        let child_answers_to = responsible_for(child.id());
        let parent_answers_to = responsible_for(std::process::id());
        child.kill().unwrap();
        let _ = child.wait().unwrap();

        assert_eq!(child_answers_to, parent_answers_to);
        assert_ne!(child_answers_to, i32::try_from(child.id()).unwrap());
    }

    /// A copy of `file` on the lowest free descriptor from 50 up, without close-on-exec, so an
    /// ordinary spawn passes it to the child. Taking a free number, rather than forcing 50, never
    /// closes a descriptor something else in the process holds.
    #[allow(unsafe_code)]
    fn inheritable_copy(file: &std::fs::File) -> std::os::fd::OwnedFd {
        use std::os::fd::FromRawFd;
        // SAFETY: F_DUPFD reads `file`'s descriptor, which stays open for the call, and returns a
        // new one or -1.
        let copy = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD, 50) };
        assert!(copy >= 50, "{}", std::io::Error::last_os_error());
        // SAFETY: `copy` was just created and nothing else owns it.
        unsafe { std::os::fd::OwnedFd::from_raw_fd(copy) }
    }

    #[tokio::test]
    async fn a_descriptor_the_parent_left_inheritable_does_not_reach_the_child() {
        let file = std::fs::File::open("/dev/null").unwrap();
        let copy = inheritable_copy(&file);
        let number = copy.as_raw_fd().to_string();
        let listing = "ls /dev/fd";

        let ordinary = std::process::Command::new("/bin/sh")
            .args(["-c", listing])
            .output()
            .unwrap();
        let disclaimed = disclaimed_spawn::spawn("/bin/sh", &["-c", listing])
            .unwrap()
            .output()
            .await
            .unwrap();

        let has_copy = |stdout: &[u8]| {
            String::from_utf8_lossy(stdout)
                .lines()
                .any(|line| line == number)
        };
        assert!(
            has_copy(&ordinary.stdout),
            "the ordinary spawn did not inherit {number}, so this proves nothing"
        );
        assert!(
            !has_copy(&disclaimed.stdout),
            "{}",
            String::from_utf8_lossy(&disclaimed.stdout)
        );
    }
}
