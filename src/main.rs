#![no_std]
#![no_main]
#![feature(default_alloc_error_handler, asm, naked_functions, lang_items)]

extern crate alloc;

#[macro_use]
mod output;
mod cli;
mod directory;
mod style;
mod time;
mod utils;

use crate::{
    cli::{App, DisplayMode, ShowAll, SortField},
    directory::{DirEntry, DirEntryExt},
    output::*,
    style::Style,
};
use alloc::{vec, vec::Vec};
use veneer::{
    fs::{DType, Directory},
    syscalls, CStr, Error,
};

#[lang = "eh_personality"]
#[no_mangle]
pub extern "C" fn eh_personality() {}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    use core::fmt::Write;
    let _ = core::writeln!(&mut crate::output::OutputBuffer::to_fd(2), "{}", info);
    let _ = crate::syscalls::kill(0, libc::SIGABRT);
    crate::syscalls::exit(-1)
}

#[cfg(not(target_os = "linux"))]
compile_error!("This crate is only implemented for linux");

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!("This crate is only implemented for x86_64 and aarch64");

#[cfg(target_arch = "x86_64")]
#[no_mangle]
#[naked]
unsafe extern "C" fn _start() {
    // Just move argc and argv into the right registers and call main
    asm!(
        "mov rdi, [rsp]", // The value of rsp is actually a pointer to argc
        "mov rsi, rsp",
        "add rsi, 8", // But for argv we just increment the rsp pointer by 1 (offset by 8)
        "call main",
        options(noreturn)
    )
}

#[cfg(target_arch = "aarch64")]
#[no_mangle]
#[naked]
unsafe extern "C" fn _start() {
    // Just move argc and argv into the right registers and call main
    asm!(
        "ldr x0, [sp]",
        "mov x1, sp",
        "add x1, x1, 0x8",
        "bl main",
        options(noreturn)
    )
}

#[no_mangle]
unsafe extern "C" fn main(argc: isize, argv: *const *const u8) -> ! {
    let ret = match run((0..argc).map(|i| CStr::from_ptr(*argv.offset(i)))) {
        Ok(()) => 0,
        Err(e) => e.0 as i32,
    };
    veneer::syscalls::exit(ret)
}

fn run(args: impl Iterator<Item = CStr<'static>>) -> Result<(), Error> {
    let mut app = App::from_arguments(args)?;

    let mut dirs = Vec::new();
    let mut files = Vec::new();

    let default_args = &[CStr::from_bytes(b".\0")];
    let args = if app.args.is_empty() {
        default_args
    } else {
        app.args.as_slice()
    };

    if app.list_directory_contents {
        for arg in args.iter().copied() {
            match Directory::open(arg) {
                Ok(d) => dirs.push((arg, d)),
                Err(Error(20)) => files.push((
                    DirEntry {
                        name: arg,
                        inode: 0,
                        d_type: DType::UNKNOWN,
                    },
                    None,
                )),
                Err(_) => {
                    if let Err(err) = veneer::syscalls::fstatat(libc::AT_FDCWD, arg) {
                        access_error(&arg, err);
                    } else {
                        files.push((
                            DirEntry {
                                name: arg,
                                inode: 0,
                                d_type: DType::UNKNOWN,
                            },
                            None,
                        ));
                    }
                }
            }
        }
    } else {
        for arg in args.iter().copied() {
            files.push((
                DirEntry {
                    name: arg,
                    inode: 0,
                    d_type: DType::UNKNOWN,
                },
                None,
            ));
        }
    }

    if !files.is_empty() {
        let dir = Directory::open(CStr::from_bytes(b".\0")).unwrap();
        if app.needs_details {
            for e in &mut files {
                let status = if app.follow_symlinks == cli::FollowSymlinks::Always {
                    syscalls::fstatat(dir.raw_fd(), e.name())
                } else {
                    syscalls::lstatat(dir.raw_fd(), e.name())
                }
                .map(|status| app.convert_status(status));
                match status {
                    Ok(s) => e.1 = Some(s),
                    Err(err) => {
                        access_error(&e.name(), err);
                    }
                }
            }
        }

        sort_entries(&mut files, &app);

        match app.display_mode {
            DisplayMode::Grid(width) => write_grid(&files, &dir, &mut app, width),
            DisplayMode::Long => write_details(&files, &dir, &mut app),
            DisplayMode::SingleColumn => write_single_column(&files, &dir, &mut app),
            DisplayMode::Stream => write_stream(&files, &dir, &mut app),
        }
    }

    if !dirs.is_empty() && !files.is_empty() {
        app.out.push(b'\n');
    }

    for (n, (name, dir)) in dirs.iter().enumerate() {
        let mut path = Vec::new();
        path.extend(name.as_bytes());
        let status = syscalls::fstat(dir.raw_fd()).unwrap();
        let mut stack = vec![(status.st_dev, status.st_ino)];
        list_dir_contents(&mut stack, &mut path, dir, &mut app);
        // When recursing the recursion handles newlines, if not we need to check if we're on the
        // last and print a newline
        if !app.recurse && (n != dirs.len() - 1) {
            app.out.push(b'\n');
        }
    }

    Ok(())
}

fn sort_entries(entries: &mut [(DirEntry, Option<Status>)], app: &App) {
    if let Some(field) = app.sort_field {
        entries.sort_unstable_by(|a, b| {
            let mut ordering = match field {
                SortField::Time => b
                    .time()
                    .cmp(&a.time())
                    .then_with(|| vercmp(a.name(), b.name())),
                SortField::Size => {
                    b.1.clone()
                        .unwrap_or_default()
                        .size
                        .cmp(&a.1.clone().unwrap_or_default().size)
                        .then_with(|| vercmp(a.name(), b.name()))
                }
                SortField::Name => vercmp(a.name(), b.name()),
            };
            if app.reverse_sorting {
                ordering = ordering.reverse();
            }
            ordering
        });
    }
}

fn list_dir_contents(
    stack: &mut Vec<(libc::dev_t, libc::ino_t)>,
    path: &mut Vec<u8>,
    dir: &Directory,
    app: &mut App,
) {
    let contents = match dir.read() {
        Ok(c) => c,
        Err(err) => {
            access_error(path, err);
            return;
        }
    };
    let hint = contents.iter().size_hint();
    let mut entries = Vec::new();
    entries.reserve(hint.1.unwrap_or(hint.0));

    match app.show_all {
        ShowAll::No => {
            for e in contents.iter().filter(|e| e.name().get(0) != Some(b'.')) {
                entries.push((e.into(), None));
            }
        }
        ShowAll::Almost => {
            for e in contents.iter() {
                if e.name().as_bytes() != b".." && e.name().as_bytes() != b"." {
                    entries.push((e.into(), None));
                }
            }
        }
        ShowAll::Yes => {
            for e in contents.iter() {
                entries.push((e.into(), None));
            }
        }
    }

    if app.args.len() > 1 || app.recurse {
        if path.len() > 1 && path.last() == Some(&0) {
            path.pop();
        }
        if path.len() > 1 && path.last() == Some(&b'/') {
            path.pop();
        }
        app.out.write(&path).write(b":\n");
    }

    if app.needs_details {
        for e in &mut entries {
            let status = if app.follow_symlinks == cli::FollowSymlinks::Always {
                syscalls::fstatat(dir.raw_fd(), e.name())
            } else {
                syscalls::lstatat(dir.raw_fd(), e.name())
            }
            .map(|status| app.convert_status(status));
            match status {
                Ok(s) => e.1 = Some(s),
                Err(err) => {
                    access_error(&e.name(), err);
                }
            }
        }
    }

    sort_entries(&mut entries, app);

    match app.display_mode {
        DisplayMode::Grid(width) => write_grid(&entries, dir, app, width),
        DisplayMode::Long => write_details(&entries, dir, app),
        DisplayMode::SingleColumn => write_single_column(&entries, dir, app),
        DisplayMode::Stream => write_stream(&entries, dir, app),
    }
    app.out.flush();

    if app.recurse {
        app.out.push(b'\n');
        for e in entries
            .iter()
            .filter_map(|(e, status)| {
                if let Some(st) = status {
                    if st.mode & libc::S_IFMT == libc::S_IFDIR {
                        Some(e)
                    } else {
                        None
                    }
                } else {
                    Some(e)
                }
            })
            .filter(|e| e.name.as_bytes() != b"..")
            .filter(|e| e.name.as_bytes() != b".")
            .filter(|e| e.d_type == DType::DIR || e.d_type == DType::UNKNOWN)
        {
            if path.last() == Some(&0) {
                path.pop();
            }
            if path.last() != Some(&b'/') {
                path.push(b'/');
            }
            path.extend(e.name.as_bytes());
            path.push(0);
            match Directory::open(CStr::from_bytes(path)) {
                Ok(dir) => {
                    let status = syscalls::fstat(dir.raw_fd()).unwrap();
                    if !stack.contains(&(status.st_dev, status.st_ino)) {
                        stack.push((status.st_dev, status.st_ino));
                        list_dir_contents(stack, path, &dir, app);
                        stack.pop();
                    }
                }
                Err(err) => {
                    access_error(&path[..path.len() - 1], err);
                }
            }
            while path.last() != Some(&b'/') {
                path.pop();
            }
        }
    }
    if path.last() == Some(&b'/') {
        path.pop();
    }
}

#[inline(never)]
fn access_error(item: &[u8], error: Error) {
    let mut out = crate::output::OutputBuffer::to_fd(2);
    out.write(&b"Unable to access '"[..])
        .write(item)
        .write(&b"': OS Error "[..]);
    error.0.write(&mut out);
    out.push(b'\n');
}

#[derive(Default, Clone)]
pub struct Status {
    pub device: libc::dev_t,
    pub links: libc::nlink_t,
    pub mode: libc::mode_t,
    pub size: libc::off_t,
    pub blocks: libc::blkcnt64_t,
    pub block_size: libc::blksize_t,
    pub uid: libc::uid_t,
    pub gid: libc::gid_t,
    pub time: libc::time_t,
    pub inode: libc::ino_t,
}
