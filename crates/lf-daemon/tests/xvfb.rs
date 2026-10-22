//! End-to-end test of the X11 backend on a private Xvfb server.
//!
//! Needs `Xvfb` and `setxkbmap` (Ubuntu: `xvfb x11-xkb-utils`; Arch:
//! `xorg-server-xvfb xorg-setxkbmap`). Run with:
//!
//! ```text
//! cargo test -p lf-daemon --test xvfb -- --ignored --nocapture
//! ```
//!
//! It never touches the desktop you are logged in to: every process gets
//! `DISPLAY` pointing at the private server.
#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto;
use x11rb::protocol::xtest::ConnectionExt as _;

const TIMEOUT: Duration = Duration::from_secs(15);

/// evdev codes: s g h l, then the default trigger f u c k k k.
const SGHL: [u8; 4] = [31, 34, 35, 38];
const TRIGGER: [u8; 6] = [33, 22, 46, 37, 37, 37];

/// Kills its processes when dropped, even if the test panics.
#[derive(Default)]
struct Procs(Vec<Child>);

impl Procs {
    fn add(&mut self, c: Child) {
        self.0.push(c);
    }
}

impl Drop for Procs {
    fn drop(&mut self) {
        for c in self.0.iter_mut().rev() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// Forward each line of `r` into a channel.
fn lines(r: impl Read + Send + 'static) -> Receiver<String> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(r).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}

/// Wait for a line matching `want`; returns every line seen so far.
fn wait_for(rx: &Receiver<String>, seen: &mut Vec<String>, want: impl Fn(&str) -> bool) -> bool {
    if seen.iter().any(|l| want(l)) {
        return true;
    }
    let deadline = Instant::now() + TIMEOUT;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(line) => {
                let hit = want(&line);
                seen.push(line);
                if hit {
                    return true;
                }
            }
            Err(_) => return false,
        }
    }
    false
}

fn free_display() -> u32 {
    (99..200)
        .find(|n| {
            !Path::new(&format!("/tmp/.X11-unix/X{n}")).exists()
                && !Path::new(&format!("/tmp/.X{n}-lock")).exists()
        })
        .expect("a free X display number")
}

fn x_env(cmd: &mut Command, display: &str) {
    cmd.env("DISPLAY", display)
        .env("XDG_SESSION_TYPE", "x11")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("XDG_CURRENT_DESKTOP");
}

/// Build the test field next to the daemon (same profile) and return its path.
fn testfield() -> PathBuf {
    let daemon = PathBuf::from(env!("CARGO_BIN_EXE_lf-daemon"));
    let dir = daemon.parent().expect("target dir").to_owned();
    let mut build = Command::new(env!("CARGO"));
    build.args(["build", "-p", "lf-x11-testfield"]);
    if dir.ends_with("release") {
        build.arg("--release");
    }
    let ok = build.status().expect("run cargo build").success();
    assert!(ok, "building lf-x11-testfield failed");
    dir.join("lf-x11-testfield")
}

fn type_keys(conn: &impl Connection, root: xproto::Window, evdev: &[u8]) {
    for &code in evdev {
        let kc = code + 8;
        for kind in [xproto::KEY_PRESS_EVENT, xproto::KEY_RELEASE_EVENT] {
            conn.xtest_fake_input(kind, kc, x11rb::CURRENT_TIME, root, 0, 0, 0)
                .expect("fake input");
        }
        conn.flush().expect("flush");
        thread::sleep(Duration::from_millis(30));
    }
}

#[test]
#[ignore = "needs Xvfb and setxkbmap; run in CI or with --ignored"]
fn sghl_then_trigger_types_salam_and_trigger_again_undoes() {
    let field_bin = testfield();
    let n = free_display();
    let display = format!(":{n}");
    let mut procs = Procs::default();

    let xvfb = Command::new("Xvfb")
        .args([&display, "-screen", "0", "640x480x24", "-nolisten", "tcp"])
        // Without -noreset the server resets (and forgets setxkbmap) whenever
        // its last client disconnects.
        .arg("-noreset")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start Xvfb (is it installed?)");
    procs.add(xvfb);
    let socket = format!("/tmp/.X11-unix/X{n}");
    let deadline = Instant::now() + TIMEOUT;
    while !Path::new(&socket).exists() {
        assert!(Instant::now() < deadline, "Xvfb did not start");
        thread::sleep(Duration::from_millis(50));
    }

    let ok = Command::new("setxkbmap")
        .args(["-display", &display, "-rules", "evdev", "-model", "pc105"])
        .args(["-layout", "us,ir"])
        .status()
        .expect("run setxkbmap")
        .success();
    assert!(ok, "setxkbmap failed");

    let mut cmd = Command::new(&field_bin);
    x_env(&mut cmd, &display);
    let mut field = cmd
        .stdout(Stdio::piped())
        .spawn()
        .expect("start test field");
    let field_out = lines(field.stdout.take().expect("stdout"));
    procs.add(field);
    let mut field_seen = Vec::new();
    assert!(
        wait_for(&field_out, &mut field_seen, |l| l == "READY"),
        "test field not ready: {field_seen:?}"
    );

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_lf-daemon"));
    x_env(&mut cmd, &display);
    let mut daemon = cmd
        .args(["--pair", "us,ir", "--log", "debug"])
        .stderr(Stdio::piped())
        .spawn()
        .expect("start lf-daemon");
    let daemon_err = lines(daemon.stderr.take().expect("stderr"));
    procs.add(daemon);
    let mut daemon_seen = Vec::new();
    assert!(
        wait_for(&daemon_err, &mut daemon_seen, |l| l.contains("running")),
        "daemon did not start: {daemon_seen:?}"
    );

    let (conn, screen) = x11rb::connect(Some(&display)).expect("connect to Xvfb");
    let root = conn.setup().roots[screen].root;

    type_keys(&conn, root, &SGHL);
    type_keys(&conn, root, &TRIGGER);
    let fixed = wait_for(&field_out, &mut field_seen, |l| l == "TEXT:سلام");
    while let Ok(l) = daemon_err.try_recv() {
        daemon_seen.push(l);
    }
    assert!(
        fixed,
        "expected سلام\nfield: {field_seen:?}\ndaemon: {daemon_seen:?}"
    );

    // The trigger again (now typed on the Persian layout) undoes the fix.
    field_seen.clear();
    type_keys(&conn, root, &TRIGGER);
    let undone = wait_for(&field_out, &mut field_seen, |l| l == "TEXT:sghl");
    while let Ok(l) = daemon_err.try_recv() {
        daemon_seen.push(l);
    }
    assert!(
        undone,
        "expected sghl after undo\nfield: {field_seen:?}\ndaemon: {daemon_seen:?}"
    );
}
