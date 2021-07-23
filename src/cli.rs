use crate::{
    output::OutputBuffer,
    veneer::{syscalls::*, CStr, Error},
};
#[cfg(feature = "no-libc")]
use alloc::vec::Vec;
use tinyvec::TinyVec;

pub struct App {
    pub print_inode: bool,
    pub block_size_is_kilobytes: bool,
    pub replace_unprintable_bytes: bool,
    pub reverse_sorting: bool,
    pub grid_sort_direction: SortDirection,
    pub display_size_in_blocks: bool,
    pub display_mode: DisplayMode,
    pub show_all: ShowAll,
    pub suffixes: Suffixes,
    pub follow_symlinks: FollowSymlinks,
    pub recurse: bool,
    pub sort_field: Option<SortField>,
    pub time_field: TimeField,
    pub list_directory_contents: bool,
    pub out: OutputBuffer,
    pub convert_id_to_name: bool,
    pub print_owner: bool,
    pub print_group: bool,
    pub color: Color,

    pub args: TinyVec<[CStr<'static>; 1]>,

    #[cfg(feature = "no-libc")]
    etc_passwd: &'static [u8],
    #[cfg(feature = "no-libc")]
    uid_names: Vec<(u32, (usize, usize))>,
    #[cfg(feature = "no-libc")]
    etc_group: &'static [u8],
    #[cfg(feature = "no-libc")]
    gid_names: Vec<(u32, (usize, usize))>,

    #[cfg(feature = "no-libc")]
    tzinfo: Option<crate::time::Tzinfo>,

    pub needs_details: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Always,
    Auto,
    Never,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TimeField {
    Modified,
    StatusModified,
    Created,
    Accessed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    Name,
    Size,
    Time,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FollowSymlinks {
    Never,
    WhenExplicit,
    Always,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Suffixes {
    None,
    Directories,
    All,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ShowAll {
    Yes,
    No,
    Almost,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    Grid(usize),
    Long,
    SingleColumn,
    Stream,
}

impl App {
    #[inline(never)]
    pub fn from_arguments(
        raw_args: impl Iterator<Item = CStr<'static>>,
    ) -> Result<Self, crate::Error> {
        let mut print_version = false;
        let mut switches: TinyVec<[u8; 24]> = TinyVec::new();
        let mut args_valid = true;

        let mut hit_only_arg_marker = false;

        let mut app = App {
            print_inode: false,
            block_size_is_kilobytes: false,
            replace_unprintable_bytes: false,
            reverse_sorting: false,
            grid_sort_direction: SortDirection::Horizontal,
            display_size_in_blocks: false,
            display_mode: DisplayMode::Grid(0),
            show_all: ShowAll::No,
            suffixes: Suffixes::None,
            follow_symlinks: FollowSymlinks::Never,
            recurse: false,
            sort_field: Some(SortField::Name),
            time_field: TimeField::Modified,
            list_directory_contents: true,
            convert_id_to_name: true,
            print_owner: true,
            print_group: true,
            color: Color::Auto,
            out: OutputBuffer::to_fd(1),
            args: TinyVec::new(),
            #[cfg(feature = "no-libc")]
            uid_names: Vec::new(),
            #[cfg(feature = "no-libc")]
            gid_names: Vec::new(),
            #[cfg(feature = "no-libc")]
            etc_passwd: &[],
            #[cfg(feature = "no-libc")]
            etc_group: &[],
            needs_details: false,
            #[cfg(feature = "no-libc")]
            tzinfo: None,
        };

        for arg in raw_args.skip(1) {
            if arg.as_bytes() == b"--" {
                hit_only_arg_marker = true;
            } else if hit_only_arg_marker {
                app.args.push(arg);
            // Things like --color=always
            } else if arg.as_bytes().starts_with(b"--") {
                match arg.as_bytes() {
                    b"--color=never" => app.color = Color::Never,
                    b"--color=auto" => app.color = Color::Auto,
                    b"--color=always" => app.color = Color::Always,
                    b"--version" => print_version = true,
                    _ => error!("unrecognized option \'", arg, "\'\n"),
                }
            // Things like -R
            } else if arg.get(0) == Some(b'-') {
                switches.extend(arg.as_bytes().iter().copied().skip(1));
            } else {
                app.args.push(arg);
            }
        }

        #[allow(non_snake_case)]
        let mut switches_contains_H_or_L = false;
        for s in &switches {
            if s == &b'H' || s == &b'L' {
                switches_contains_H_or_L = true;
                break;
            }
        }

        for switch in switches.iter().copied() {
            match switch {
                b'A' => {
                    app.show_all = ShowAll::Almost;
                }
                b'C' => {
                    app.display_mode = DisplayMode::Grid(0);
                    app.grid_sort_direction = SortDirection::Horizontal;
                }
                b'F' => {
                    if !switches_contains_H_or_L {
                        app.follow_symlinks = FollowSymlinks::Never;
                    }
                    app.suffixes = Suffixes::All;
                }
                b'H' => {
                    app.follow_symlinks = FollowSymlinks::WhenExplicit;
                }
                b'L' => {
                    app.follow_symlinks = FollowSymlinks::Always;
                }
                b'R' => {
                    app.recurse = true;
                }
                b'S' => {
                    app.sort_field = Some(SortField::Size);
                }
                b'a' => {
                    app.show_all = ShowAll::Yes;
                }
                b'c' => {
                    app.time_field = TimeField::StatusModified;
                    app.sort_field = Some(SortField::Time);
                }
                b'd' => {
                    if !switches_contains_H_or_L {
                        app.follow_symlinks = FollowSymlinks::Never;
                    }
                    app.list_directory_contents = false;
                }
                b'f' => {
                    app.sort_field = None;
                    app.show_all = ShowAll::Yes;
                }
                b'g' => {
                    app.display_mode = DisplayMode::Long;
                    app.print_owner = false;
                }
                b'i' => {
                    app.print_inode = true;
                }
                b'k' => {
                    app.block_size_is_kilobytes = true;
                }
                b'l' => {
                    app.display_mode = DisplayMode::Long;
                }
                b'm' => {
                    app.display_mode = DisplayMode::Stream;
                }
                b'n' => {
                    app.display_mode = DisplayMode::Long;
                    app.convert_id_to_name = false;
                }
                b'o' => {
                    app.display_mode = DisplayMode::Long;
                    app.print_group = false;
                }
                b'p' => {
                    app.suffixes = Suffixes::Directories;
                }
                b'q' => {
                    app.replace_unprintable_bytes = true;
                }
                b'r' => {
                    app.reverse_sorting = true;
                }
                b's' => {
                    app.display_size_in_blocks = true;
                }
                b't' => {
                    app.time_field = TimeField::Modified;
                    app.sort_field = Some(SortField::Time);
                }
                b'u' => {
                    app.time_field = TimeField::Accessed;
                    app.sort_field = Some(SortField::Time);
                }
                b'x' => {
                    app.grid_sort_direction = SortDirection::Horizontal;
                }
                b'1' => match app.display_mode {
                    DisplayMode::Long => {}
                    _ => app.display_mode = DisplayMode::SingleColumn,
                },
                s => {
                    error!("invalid option \'", s, "\'\n");
                    args_valid = false;
                }
            }
        }

        if !args_valid {
            return Err(Error(-1));
        }

        if print_version {
            app.out
                .write(b"fls ")
                .write(env!("CARGO_PKG_VERSION").as_bytes())
                .write(b" ")
                .write(git_version::git_version!(prefix = "git:").as_bytes())
                .write(b"\n")
                .flush();
            exit(0);
        }

        let terminal_width = winsize().ok().map(|d| d.ws_col as usize);

        match (terminal_width, app.display_mode) {
            (Some(width), DisplayMode::Grid(_)) => app.display_mode = DisplayMode::Grid(width),
            (None, DisplayMode::Grid(_)) => app.display_mode = DisplayMode::SingleColumn,
            _ => {}
        }

        if terminal_width.is_none() && app.color == Color::Auto {
            app.color = Color::Never;
        }
        if app.color == Color::Never {
            app.out.color = false;
        }

        #[cfg(feature = "no-libc")]
        if app.display_mode == DisplayMode::Long {
            Self::init_id_map(
                &b"/etc/passwd\0"[..],
                &mut app.etc_passwd,
                &mut app.uid_names,
            )?;
            Self::init_id_map(&b"/etc/group\0"[..], &mut app.etc_group, &mut app.gid_names)?;
            app.tzinfo = Some(crate::time::Tzinfo::new());
        }

        app.needs_details = app.display_mode == DisplayMode::Long
            || app.sort_field == Some(SortField::Time)
            || app.sort_field == Some(SortField::Size)
            || app.display_size_in_blocks;

        Ok(app)
    }

    #[cfg(feature = "no-libc")]
    pub fn convert_to_localtime(&self, time: i64) -> crate::time::LocalTime {
        self.tzinfo.as_ref().unwrap().convert_to_localtime(time)
    }

    #[cfg(not(feature = "no-libc"))]
    pub fn convert_to_localtime(&self, time: i64) -> crate::time::LocalTime {
        let tm = unsafe {
            let mut tm: libc::tm = core::mem::zeroed();
            libc::localtime_r(&time as *const libc::time_t, &mut tm as *mut libc::tm);
            tm
        };

        crate::time::LocalTime {
            year: tm.tm_year,
            month: tm.tm_mon,
            day_of_month: tm.tm_mday,
            hour: tm.tm_hour,
            minute: tm.tm_min,
        }
    }

    #[cfg(feature = "no-libc")]
    #[inline(never)]
    fn init_id_map(
        path: &'static [u8],
        slab: &mut &'static [u8],
        map: &mut Vec<(u32, (usize, usize))>,
    ) -> Result<(), Error> {
        use crate::utils::atoi;
        let contents = crate::utils::fs_read(CStr::from_bytes(path))?;
        *slab = alloc::boxed::Box::leak(contents.into_boxed_slice());

        let mut offset = 0;
        for line in slab.split(|b| *b == b'\n') {
            if line.is_empty() {
                offset += line.len() + 1;
                continue;
            }
            let mut it = line.split(|b| *b == b':');
            let name = it.next().unwrap();
            let _passwd = it.next().unwrap();
            let uid = atoi(it.next().unwrap()) as u32;

            map.push((uid, (offset, offset + name.len())));

            offset += line.len() + 1;
        }
        Ok(())
    }

    #[cfg(feature = "no-libc")]
    pub fn getpwuid(&self, uid: u32) -> &'static [u8] {
        self.uid_names
            .iter()
            .find(|(id, _)| *id == uid)
            .map(|(_id, (start, end))| &self.etc_passwd[*start..*end])
            .unwrap_or_default()
    }

    #[cfg(not(feature = "no-libc"))]
    pub fn getpwuid(&self, uid: u32) -> &'static [u8] {
        unsafe {
            let passwd = libc::getpwuid(uid);
            CStr::from_ptr((*passwd).pw_name as *const u8).as_bytes()
        }
    }

    #[cfg(feature = "no-libc")]
    pub fn getgrgid(&self, gid: u32) -> &'static [u8] {
        self.gid_names
            .iter()
            .find(|(id, _)| *id == gid)
            .map(|(_id, (start, end))| &self.etc_group[*start..*end])
            .unwrap_or_default()
    }

    #[cfg(not(feature = "no-libc"))]
    pub fn getgrgid(&self, uid: u32) -> &'static [u8] {
        unsafe {
            let group = libc::getgrgid(uid);
            CStr::from_ptr((*group).gr_name as *const u8).as_bytes()
        }
    }

    pub fn convert_status(&self, status: libc::stat64) -> crate::Status {
        use TimeField::*;
        crate::Status {
            device: status.st_dev,
            links: status.st_nlink,
            mode: status.st_mode,
            size: status.st_size,
            blocks: status.st_blocks,
            block_size: status.st_blksize,
            uid: status.st_uid,
            gid: status.st_gid,
            inode: status.st_ino,
            time: match self.time_field {
                Accessed => status.st_atime,
                Created => status.st_ctime,
                Modified | StatusModified => status.st_mtime,
            },
        }
    }
}
