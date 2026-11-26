//! Measures the watch of a worktree during a burst of file changes.
//!
//! The program makes a repository with `files` committed files in a
//! temporary directory, starts a watch and changes each file one time. It
//! prints the number of status computations that the watch made and the time
//! from the last change to the event with the final status. Then it measures
//! one status computation.
//!
//! ```sh
//! cargo run --release --example watch_burst -- <files> <quiet ms> <longest ms> <burst ms>
//! ```
//!
//! `burst ms` is the time over which the program spreads the changes. With 0,
//! it writes as fast as it can.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use nomux_git::watch::{Debounce, RepositoryWatch};

const FILES_IN_A_DIRECTORY: usize = 100;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args()
        .skip(1)
        .map(|argument| argument.parse::<u64>());
    let mut argument = |default| arguments.next().unwrap_or(Ok(default));
    let files = argument(10_000)? as usize;
    let debounce = Debounce {
        quiet: Duration::from_millis(argument(300)?),
        longest: Duration::from_millis(argument(1500)?),
    };
    let burst = Duration::from_millis(argument(0)?);

    let directory = tempfile::tempdir()?;
    let worktree = directory.path().canonicalize()?;
    write_files(&worktree, files, "before\n", Duration::ZERO)?;
    for arguments in [
        &["init", "--quiet"][..],
        &["add", "--all"],
        &[
            "-c",
            "user.name=n",
            "-c",
            "user.email=e",
            "commit",
            "--quiet",
            "--message",
            "files",
        ],
    ] {
        let status = Command::new("git")
            .current_dir(&worktree)
            .args(arguments)
            .status()?;
        assert!(status.success(), "git {arguments:?} failed");
    }

    let (sender, statuses) = mpsc::channel();
    let started = Instant::now();
    let (watch, _) = RepositoryWatch::start(&worktree, debounce, move |status| {
        sender.send((Instant::now(), status.files.len())).is_ok()
    })?;
    println!(
        "start of the watch with the first status: {:?}",
        started.elapsed()
    );

    let started = Instant::now();
    write_files(&worktree, files, "after\n", burst)?;
    let last_change = Instant::now();
    println!("{files} file changes in {:?}", last_change - started);

    let mut events = 0;
    loop {
        let (at, changed) = statuses.recv_timeout(Duration::from_secs(60))?;
        events += 1;
        if changed == files {
            println!(
                "last change to the event with the final status: {:?}",
                at - last_change
            );
            break;
        }
    }
    // A later computation would start in this time.
    std::thread::sleep(debounce.longest + Duration::from_secs(1));
    println!(
        "status computations for the burst: {}",
        watch.computations() - 1
    );
    println!("events for the burst: {events}");
    let started = Instant::now();
    nomux_git::status::status(&gix::open(&worktree)?)?;
    println!(
        "one status computation with {files} changed files: {:?}",
        started.elapsed()
    );
    Ok(())
}

fn write_files(
    worktree: &Path,
    files: usize,
    content: &str,
    spread: Duration,
) -> std::io::Result<()> {
    let started = Instant::now();
    for file in 0..files {
        let directory = worktree.join(format!("source/{}", file / FILES_IN_A_DIRECTORY));
        fs::create_dir_all(&directory)?;
        fs::write(directory.join(format!("{file}.txt")), content)?;
        let due = spread.mul_f64((file + 1) as f64 / files as f64);
        std::thread::sleep(due.saturating_sub(started.elapsed()));
    }
    Ok(())
}
