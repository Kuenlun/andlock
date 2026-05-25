// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Drives `andlock 4x4 -f 100 --human` under a PTY, sends Ctrl+C while the DP
//! is still running, and asserts the output carries: the memory clamp warning,
//! a single `Len  Count` table whose rows sum to the printed `Total`, a
//! `Points` line, and an `Interrupted at length N` footer pointing at the
//! last completed length.
//!
//! Two views are inspected. The raw byte stream is used for content that the
//! binary emits once and never rewrites (the clamp warning, the interrupted
//! footer), so transient cursor games during SIGINT cleanup cannot hide it.
//! The vt100-rendered screen is used for the table, separator and summary,
//! where deduplicating the live multi-line bar against the final report
//! requires a real terminal model.
//!
//! Shape of the rendered output this test inspects:
//!
//! ```text
//! ● ● ● ●    ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★
//! ● ● ● ●    ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★
//! ● ● ● ●    ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★
//! ● ● ● ●    ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★ ★
//!
//! warning: insufficient memory, run limited to --max-length 5 (need 16.00 EiB, only 5.16 GiB available)
//!   Len        Count
//!     0            1
//!     1          116
//!     2        13272
//!     3      1505544
//!     4    169290680
//!   ──────────────────
//!   Total  170809613
//!   Points       116
//!   Interrupted at length 4 after 7.28s
//! ```

#![cfg(unix)]
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::{Read, Write};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

const ROWS: u16 = 60;
const COLS: u16 = 120;
const ARM_TIMEOUT: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(200);
const SIGINT_EXIT: u32 = 130;

// PTY + timing dependent: live-bar output reaches the test thread fast enough
// on developer machines but not on virtualised CI runners. Run locally with
// `cargo nextest run --include-ignored` (or `cargo test -- --ignored`).
#[test]
#[ignore = "PTY-timing dependent, opt in locally with --include-ignored"]
fn sigint_renders_coherent_partial_report() {
    let pty = native_pty_system()
        .openpty(PtySize {
            rows: ROWS,
            cols: COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open pty");

    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_andlock"));
    cmd.args(["4x4", "-f", "100", "--human"]);

    let mut child = pty.slave.spawn_command(cmd).expect("spawn child");
    drop(pty.slave);

    let mut reader = pty.master.try_clone_reader().expect("clone reader");
    let mut writer = pty.master.take_writer().expect("take writer");

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let reader_handle = thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });

    let mut raw = Vec::<u8>::new();
    let mut parser = vt100::Parser::new(ROWS, COLS, 0);
    let started = Instant::now();
    // Wait until the live multi-line bar shows at least two finalised rows, so
    // SIGINT lands while the DP is still chewing through later lengths.
    loop {
        assert!(
            started.elapsed() < ARM_TIMEOUT,
            "live table never advanced past length 1",
        );
        match rx.recv_timeout(POLL) {
            Ok(chunk) => {
                parser.process(&chunk);
                raw.extend_from_slice(&chunk);
                if data_rows(&parser.screen().contents()).len() >= 2 {
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => panic!("PTY closed before progress armed"),
        }
    }

    writer.write_all(b"\x03").expect("send ctrl+c");
    writer.flush().ok();
    let status = child.wait().expect("wait child");
    drop(writer);
    drop(pty.master);
    for chunk in &rx {
        parser.process(&chunk);
        raw.extend_from_slice(&chunk);
    }
    reader_handle.join().ok();

    assert_eq!(
        status.exit_code(),
        SIGINT_EXIT,
        "expected SIGINT exit {SIGINT_EXIT}, got {status:?}",
    );

    let raw_text = String::from_utf8_lossy(&raw);
    let screen = parser.screen().contents();
    assert_report(&raw_text, &screen);
}

fn assert_report(raw: &str, screen: &str) {
    // 1. Memory clamp warning. Emitted once via eprintln before any bars draw;
    //    look for it in the raw stream so cursor movements in the SIGINT
    //    cleanup path cannot scrub it from the visible screen. `warning:` is
    //    styled separately by `console`, so its trailing ANSI reset splits it
    //    from the rest of the sentence: each needle has to be checked as its
    //    own contiguous run of bytes.
    for needle in [
        "warning:",
        "insufficient memory",
        "run limited to --max-length",
        "(need ",
        " available)",
    ] {
        assert!(
            raw.contains(needle),
            "clamp warning lacks {needle:?} in raw stream:\n{raw}",
        );
    }

    // 2. Exactly one `Len  Count` header in the rendered screen. The live bar
    //    redraws the header in place; vt100 collapses them back to one.
    let headers = screen
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("Len ") && t.contains("Count")
        })
        .count();
    assert_eq!(headers, 1, "expected one `Len  Count` header in:\n{screen}");

    // 3. Contiguous data rows starting at length 0.
    let rows = data_rows(screen);
    assert!(!rows.is_empty(), "no data rows in:\n{screen}");
    for (i, &(len, _)) in rows.iter().enumerate() {
        assert_eq!(len, i, "lengths not contiguous from 0: {rows:?}");
    }

    // 4. `Total` value equals the sum of the printed rows. We only take the
    //    first token after the label, since the SIGINT cleanup path may leave
    //    stale progress-bar text further right on this row.
    let total = screen
        .lines()
        .find_map(|l| {
            l.trim_start()
                .strip_prefix("Total")?
                .split_whitespace()
                .next()
                .and_then(parse_human)
        })
        .unwrap_or_else(|| panic!("missing `Total` row in:\n{screen}"));
    let expected: u128 = rows.iter().map(|&(_, c)| c).sum();
    assert_eq!(total, expected, "Total {total} != sum of rows {expected}");

    // 5. `Points` row present.
    assert!(
        screen.lines().any(|l| l.trim_start().starts_with("Points")),
        "missing `Points` row in:\n{screen}",
    );

    // 6. `Interrupted at length N after T` footer matches the last row. Match
    //    on the raw stream for the same reason as the warning.
    let last = rows.last().expect("rows is non-empty").0;
    let footer = format!("Interrupted at length {last} after ");
    assert!(
        raw.contains(&footer),
        "missing footer {footer:?} in raw stream:\n{raw}",
    );
}

fn data_rows(screen: &str) -> Vec<(usize, u128)> {
    screen
        .lines()
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            let len: usize = parts.next()?.parse().ok()?;
            let count = parse_human(parts.next()?)?;
            parts.next().is_none().then_some((len, count))
        })
        .collect()
}

fn parse_human(s: &str) -> Option<u128> {
    s.chars()
        .filter(|c| *c != '_')
        .collect::<String>()
        .parse()
        .ok()
}
