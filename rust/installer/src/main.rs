//! CottonCandy's installer. One program does both jobs: with the browser appended to it, it installs;
//! as `uninstall.exe` in the install folder (the same program without the browser), it uninstalls.
//!
//!   cottoncandy-setup [--dir <folder>] [--no-desktop] [--no-start-menu] [--silent]
//!   uninstall [--silent] [--remove-data]

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod platform;
mod ui;
mod work;

use std::path::PathBuf;

#[derive(Default, Clone)]
pub struct Args {
    pub dir: Option<PathBuf>,
    pub silent: bool,
    pub uninstall: bool,
    pub from_temp: bool,
    pub target: Option<PathBuf>,
    pub remove_data: bool,
    pub no_desktop: bool,
    pub no_start_menu: bool,
    /// Start installing straight away (after the installer restarted itself as administrator).
    pub go: bool,
    /// Show one screen with made-up progress, for screenshots: welcome, installing, done, uninstall.
    pub demo: Option<String>,
}

fn parse() -> Args {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--dir" => a.dir = it.next().map(PathBuf::from),
            "--target" => a.target = it.next().map(PathBuf::from),
            "--silent" | "/S" => a.silent = true,
            "--uninstall" => a.uninstall = true,
            "--from-temp" => a.from_temp = true,
            "--remove-data" => a.remove_data = true,
            "--no-desktop" => a.no_desktop = true,
            "--no-start-menu" => a.no_start_menu = true,
            "--go" => a.go = true,
            "--demo" => a.demo = it.next(),
            _ => {}
        }
    }
    a
}

fn main() {
    let mut args = parse();
    let has_payload = std::env::current_exe()
        .ok()
        .and_then(|p| std::fs::File::open(p).ok())
        .and_then(|mut f| work::payload_range(&mut f).ok().flatten())
        .is_some();
    if !has_payload && args.demo.is_none() {
        args.uninstall = true;
    }
    if args.uninstall {
        let target = args.target.clone().unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .unwrap_or_default()
        });
        args.target = Some(target.clone());
        if !args.from_temp {
            let mut extra = Vec::new();
            if args.silent {
                extra.push("--silent".to_string());
            }
            if args.remove_data {
                extra.push("--remove-data".to_string());
            }
            if platform::hand_off_uninstall(&target, &extra) {
                return;
            }
        }
    }
    if args.silent {
        std::process::exit(run_silent(&args));
    }
    ui::run(args);
}

fn run_silent(args: &Args) -> i32 {
    let progress: work::Shared = Default::default();
    let result = if args.uninstall {
        let r = work::uninstall(
            args.target.as_deref().unwrap_or(std::path::Path::new(".")),
            args.remove_data,
            &progress,
        );
        if args.from_temp {
            platform::delete_self_later();
        }
        r
    } else {
        let opts = work::Options {
            dir: args
                .dir
                .as_deref()
                .map(work::install_dir_in)
                .unwrap_or_else(work::default_dir),
            desktop: !args.no_desktop,
            start_menu: !args.no_start_menu,
        };
        work::install(&opts, &progress)
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}
