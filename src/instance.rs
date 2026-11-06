//! One Lull at a time.
//!
//! Starting a new version while an older one runs used to leave two copies:
//! the old one kept its sound, timer and global shortcuts, so the new one
//! could neither stop that sound nor receive the shortcuts. Now a newly
//! started Lull shuts down running copies of other versions, and quits if
//! the same version is already running (e.g. double-clicked twice).
//! Copies are found by executable name so versions without this file are
//! caught too.

use crate::settings;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub enum Claim {
    /// This process is now the only Lull.
    Ours,
    /// The same version is already running; this one should exit.
    AlreadyRunning,
}

pub fn claim() -> Claim {
    let me = std::process::id();
    let lock = settings::dir().map(|d| d.join("instance.txt"));
    let others: Vec<u32> = other_lulls().into_iter().filter(|&p| p != me).collect();

    let running = lock
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| {
            let (pid, ver) = s.trim().split_once(' ')?;
            Some((pid.parse::<u32>().ok()?, ver.to_string()))
        });
    if let Some((pid, ver)) = running {
        if ver == VERSION && others.contains(&pid) {
            return Claim::AlreadyRunning;
        }
    }

    for pid in others {
        terminate(pid);
    }
    if let Some(p) = lock {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, format!("{me} {VERSION}\n"));
    }
    Claim::Ours
}

/// Executable names that count as Lull: ours, plus the released name.
fn names() -> Vec<String> {
    let own = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    let released = if cfg!(windows) { "lull.exe" } else { "lull" };
    let mut v = vec![released.to_string()];
    v.extend(own.filter(|n| n != released));
    v
}

#[cfg(windows)]
fn other_lulls() -> Vec<u32> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };
    let names = names();
    let mut found = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return found };
        let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            let name = String::from_utf16_lossy(&e.szExeFile[..len]).to_lowercase();
            if names.contains(&name) {
                found.push(e.th32ProcessID);
            }
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    found
}

#[cfg(windows)]
fn terminate(pid: u32) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
    };
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, pid) {
            if TerminateProcess(h, 0).is_ok() {
                // Wait so its audio device and global shortcuts are released.
                WaitForSingleObject(h, 3000);
            }
            let _ = CloseHandle(h);
        }
    }
}

#[cfg(not(windows))]
fn other_lulls() -> Vec<u32> {
    let mut found = Vec::new();
    for name in names() {
        if let Ok(out) = std::process::Command::new("pgrep").arg("-x").arg(&name).output() {
            let text = String::from_utf8_lossy(&out.stdout);
            found.extend(text.lines().filter_map(|l| l.trim().parse::<u32>().ok()));
        }
    }
    found.sort_unstable();
    found.dedup();
    found
}

#[cfg(not(windows))]
fn terminate(pid: u32) {
    let pid = pid.to_string();
    let _ = std::process::Command::new("kill").arg(&pid).status();
    // Wait (up to 3 s) so its audio device and global shortcuts are released.
    for _ in 0..30 {
        let alive = std::process::Command::new("kill").args(["-0", &pid]).status();
        if !alive.is_ok_and(|s| s.success()) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
