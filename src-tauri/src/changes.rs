//! 本次运行改过的文件，供界面撤回。原位置与备份只由后端记录。
use serde::Serialize;
use std::{
    collections::HashMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime},
};

const RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

use crate::error::{AppError, ErrorKind};

#[cfg_attr(feature = "gen-ipc", derive(specta::Type))]
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Change {
    pub id: String,
    pub path: PathBuf,
    pub kind: &'static str,
    pub backup: Option<PathBuf>,
    pub agent_id: String,
    #[serde(skip)]
    after: Option<(SystemTime, u64)>,
}

#[derive(Default)]
pub struct Changes(Mutex<HashMap<String, Change>>);

fn stamp(path: &Path) -> io::Result<Option<(SystemTime, u64)>> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Ok(Some((meta.modified()?, meta.len()))),
        Ok(_) => Err(io::Error::other("目标不是普通文件")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn filename(path: &Path) -> &std::ffi::OsStr {
    path.file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("文件"))
}

/// 把写好的 `from` 放到 `to`：**`to` 已经存在就报 AlreadyExists，绝不覆盖**（别的程序刚好在这一刻建了同名文件也不会被冲掉）。
/// 用系统自带的「不覆盖改名」，比硬链接通用：FAT32 / exFAT 的 U 盘、一部分网络盘不支持硬链接。
/// 改名成功后 `from` 已经不存在。
fn place_new(from: &Path, to: &Path) -> io::Result<()> {
    match rename_no_replace(from, to) {
        Err(e) if e.kind() == io::ErrorKind::Unsupported => place_new_fallback(from, to),
        r => r,
    }
}

#[cfg(windows)]
fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    let wide = |p: &Path| {
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let (f, t) = (wide(from), wide(to));
    // 标志 0 = 不带 MOVEFILE_REPLACE_EXISTING：目标已存在就失败
    if unsafe { MoveFileExW(f.as_ptr(), t.as_ptr(), 0) } != 0 {
        return Ok(());
    }
    let e = io::Error::last_os_error();
    // ERROR_ALREADY_EXISTS(183) / ERROR_FILE_EXISTS(80)
    Err(if matches!(e.raw_os_error(), Some(183) | Some(80)) {
        io::Error::from(io::ErrorKind::AlreadyExists)
    } else {
        e
    })
}

#[cfg(target_os = "macos")]
fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let c = |p: &Path| {
        std::ffi::CString::new(p.as_os_str().as_bytes())
            .map_err(|_| io::Error::other("路径里有空字符"))
    };
    let (f, t) = (c(from)?, c(to)?);
    if unsafe { libc::renamex_np(f.as_ptr(), t.as_ptr(), libc::RENAME_EXCL) } == 0 {
        return Ok(());
    }
    let e = io::Error::last_os_error();
    // 这个文件系统不支持 RENAME_EXCL（ENOTSUP / EINVAL）→ 走后备办法
    Err(
        if matches!(e.raw_os_error(), Some(libc::ENOTSUP) | Some(libc::EINVAL)) {
            io::Error::from(io::ErrorKind::Unsupported)
        } else {
            e
        },
    )
}

#[cfg(not(any(windows, target_os = "macos")))]
fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    // 没有统一的不覆盖改名（Linux 上用硬链接；不支持就走后备办法）
    match fs::hard_link(from, to) {
        Ok(()) => {
            let _ = fs::remove_file(from);
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(e),
        Err(_) => Err(io::Error::from(io::ErrorKind::Unsupported)),
    }
}

/// 后备办法（文件系统连不覆盖改名都不支持）：`create_new` 保证目标已存在就报错、绝不覆盖；
/// 代价是写到一半出错时目标位置会留下不完整的文件——所以出错就把它删掉。
fn place_new_fallback(from: &Path, to: &Path) -> io::Result<()> {
    let mut src = fs::File::open(from)?;
    let mut dst = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)?;
    let copied = io::copy(&mut src, &mut dst).and_then(|_| dst.sync_all());
    drop(dst);
    if let Err(e) = copied {
        let _ = fs::remove_file(to);
        return Err(e);
    }
    let _ = fs::remove_file(from);
    Ok(())
}

fn staged_copy(from: &Path, to: &Path, replace: bool) -> io::Result<()> {
    let parent = to
        .parent()
        .ok_or_else(|| io::Error::other("文件没有上级目录"))?;
    let staging = parent.join(format!(".floatball-{}", uuid::Uuid::new_v4()));
    if let Err(e) = fs::copy(from, &staging) {
        let _ = fs::remove_file(&staging);
        return Err(e);
    }
    let result = if replace {
        fs::rename(&staging, to)
    } else {
        place_new(&staging, to)
    };
    let _ = fs::remove_file(staging);
    result
}

fn to_trash(path: &Path, trash: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(trash)?;
    let mut name = std::ffi::OsString::from(format!("{}-", uuid::Uuid::new_v4()));
    name.push(filename(path));
    let dest = trash.join(name);
    if fs::rename(path, &dest).is_err() {
        staged_copy(path, &dest, false)?;
        if let Err(e) = fs::remove_file(path) {
            let _ = fs::remove_file(&dest);
            return Err(e);
        }
    }
    Ok(dest)
}

impl Changes {
    pub fn write(
        &self,
        path: &Path,
        content: &str,
        directory: &Path,
        sequence: &str,
        agent_id: &str,
    ) -> io::Result<Change> {
        let mut entries = self.0.lock().unwrap();
        let path = std::path::absolute(path)?;
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("文件没有上级目录"))?;
        let id = uuid::Uuid::new_v4().to_string();
        let before = stamp(&path)?;
        let backup = if before.is_some() {
            fs::create_dir_all(directory)?;
            let mut name = std::ffi::OsString::from(format!("{sequence}-"));
            name.push(filename(&path));
            let backup = directory.join(name);
            fs::copy(&path, &backup)?;
            Some(backup)
        } else {
            None
        };
        fs::create_dir_all(parent)?;
        let staging = parent.join(format!(".floatball-{id}"));
        let result: io::Result<_> = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&staging)?;
            file.write_all(content.as_bytes())?;
            if let Some(backup) = &backup {
                file.set_permissions(fs::metadata(backup)?.permissions())?;
            }
            drop(file);
            let after = stamp(&staging)?;
            if stamp(&path)? != before {
                return Err(io::Error::other("文件已经变化，请重新尝试"));
            }
            if before.is_some() {
                fs::rename(&staging, &path)?;
            } else {
                place_new(&staging, &path)?;
            }
            Ok(after)
        })();
        let _ = fs::remove_file(staging);
        let after = result?;
        let change = Change {
            id,
            path: path.clone(),
            kind: if before.is_some() { "修改" } else { "新建" },
            backup,
            agent_id: agent_id.into(),
            after,
        };
        entries.insert(change.id.clone(), change.clone());
        Ok(change)
    }

    pub fn delete(&self, path: &Path, trash: &Path, agent_id: &str) -> io::Result<Change> {
        let mut entries = self.0.lock().unwrap();
        let path = std::path::absolute(path)?;
        if stamp(&path)?.is_none() {
            return Err(io::Error::other("找不到这个文件（文件夹不能删）"));
        }
        let backup = to_trash(&path, trash)?;
        let change = Change {
            id: uuid::Uuid::new_v4().to_string(),
            path,
            kind: "删除",
            backup: Some(backup),
            agent_id: agent_id.into(),
            after: None,
        };
        entries.insert(change.id.clone(), change.clone());
        Ok(change)
    }

    pub fn undo(&self, id: &str, confirm_changed: bool, trash: &Path) -> Result<bool, AppError> {
        let mut entries = self.0.lock().unwrap();
        let change = entries
            .get(id)
            .ok_or_else(|| AppError::new(ErrorKind::Conflict, "找不到这条改动，可能已经撤回"))?;
        let path = &change.path;
        let current = stamp(path)?;
        if change.kind == "删除" && current.is_some() {
            return Err(AppError::new(
                ErrorKind::Conflict,
                "原位置已经有同名文件，没有撤回",
            ));
        }
        if change.after != current && !confirm_changed {
            return Ok(false);
        }
        match change.kind {
            "新建" if current.is_some() => {
                to_trash(path, trash)?;
            }
            "新建" => {}
            "修改" | "删除" => {
                let backup = change
                    .backup
                    .as_deref()
                    .ok_or_else(|| AppError::new(ErrorKind::Internal, "找不到备份"))?;
                staged_copy(backup, path, change.kind == "修改" && current.is_some()).map_err(
                    |e| {
                        if change.kind == "删除" && e.kind() == io::ErrorKind::AlreadyExists {
                            AppError::new(ErrorKind::Conflict, "原位置已经有同名文件，没有撤回")
                        } else {
                            e.into()
                        }
                    },
                )?;
                if change.kind == "删除" {
                    let _ = fs::remove_file(backup);
                }
            }
            _ => return Err(AppError::new(ErrorKind::Invalid, "未知改动")),
        }
        entries.remove(id);
        Ok(true)
    }
}

pub fn prune(root: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_dir())
            && entry
                .metadata()
                .and_then(|meta| meta.modified())
                .is_ok_and(|t| now.duration_since(t).is_ok_and(|age| age > RETENTION))
        {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_and_undo_modified_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.txt");
        let trash = dir.path().join("trash");
        let changes = Changes::default();
        fs::write(&path, "旧的").unwrap();
        let change = changes
            .write(&path, "新的", &dir.path().join("backup"), "1", "lead")
            .unwrap();
        assert_eq!(change.kind, "修改");
        assert_eq!(fs::read_to_string(change.backup.unwrap()).unwrap(), "旧的");
        fs::write(&path, "外部又改过了").unwrap();
        assert_eq!(changes.undo(&change.id, false, &trash), Ok(false));
        assert_eq!(fs::read_to_string(&path).unwrap(), "外部又改过了");
        assert_eq!(changes.undo(&change.id, true, &trash), Ok(true));
        assert_eq!(fs::read_to_string(&path).unwrap(), "旧的");
        assert!(changes.undo(&change.id, true, &trash).is_err());
    }

    #[test]
    fn undo_creation_and_deletion() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.txt");
        let trash = dir.path().join("trash");
        let changes = Changes::default();
        let created = changes
            .write(&path, "初稿", &dir.path().join("backup"), "1", "lead")
            .unwrap();
        assert_eq!(created.kind, "新建");
        assert_eq!(changes.undo(&created.id, false, &trash), Ok(true));
        assert!(!path.exists());
        let moved = fs::read_dir(&trash)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(fs::read_to_string(moved).unwrap(), "初稿");
        fs::write(&path, "待删除").unwrap();
        let deleted = changes.delete(&path, &trash, "lead").unwrap();
        assert_eq!(deleted.kind, "删除");
        fs::write(&path, "同名新文件").unwrap();
        assert_eq!(
            changes.undo(&deleted.id, false, &trash),
            Err(AppError::new(
                ErrorKind::Conflict,
                "原位置已经有同名文件，没有撤回"
            ))
        );
        fs::remove_file(&path).unwrap();
        assert_eq!(changes.undo(&deleted.id, false, &trash), Ok(true));
        assert_eq!(fs::read_to_string(path).unwrap(), "待删除");
        assert!(!deleted.backup.unwrap().exists());
    }

    #[test]
    fn place_new_never_overwrites_and_moves_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("from"), dir.path().join("to"));
        fs::write(&from, "新内容").unwrap();
        place_new(&from, &to).unwrap();
        assert_eq!(fs::read_to_string(&to).unwrap(), "新内容");
        assert!(!from.exists(), "放过去后源文件不再存在");
        // 目标已经有文件：报 AlreadyExists，目标原样、源文件还在
        fs::write(&from, "另一份").unwrap();
        let err = place_new(&from, &to).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&to).unwrap(), "新内容");
        assert_eq!(fs::read_to_string(&from).unwrap(), "另一份");
    }

    #[test]
    fn fallback_for_filesystems_without_rename_excl_never_overwrites() {
        // 硬链接和 RENAME_EXCL 都不支持的盘（FAT32 / exFAT / 部分网络盘）走这条路
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("from"), dir.path().join("to"));
        fs::write(&from, "完整内容").unwrap();
        place_new_fallback(&from, &to).unwrap();
        assert_eq!(fs::read_to_string(&to).unwrap(), "完整内容");
        assert!(!from.exists());
        fs::write(&from, "另一份").unwrap();
        let err = place_new_fallback(&from, &to).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read_to_string(&to).unwrap(),
            "完整内容",
            "后备办法也不覆盖已有文件"
        );
        // 源文件打不开：不留下目标
        let missing = dir.path().join("nope");
        let to2 = dir.path().join("to2");
        assert!(place_new_fallback(&missing, &to2).is_err());
        assert!(!to2.exists());
    }

    #[test]
    fn new_file_write_leaves_no_staging_files() {
        let dir = tempfile::tempdir().unwrap();
        let (target, backups) = (dir.path().join("新.txt"), dir.path().join("b"));
        let changes = Changes::default();
        let change = changes
            .write(&target, "你好", &backups, "1", "lead")
            .unwrap();
        assert_eq!(change.kind, "新建");
        assert_eq!(fs::read_to_string(&target).unwrap(), "你好");
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().all(|n| !n.starts_with(".floatball-")),
            "临时文件已清理 {names:?}"
        );
        // 同一位置刚好被别的程序建了文件：写入失败，对方的文件原样
        let other = dir.path().join("别人的.txt");
        let staged = other.parent().unwrap().join(".floatball-x");
        fs::write(&other, "别人的").unwrap();
        fs::write(&staged, "我的").unwrap();
        assert_eq!(
            place_new(&staged, &other).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read_to_string(&other).unwrap(), "别人的");
    }

    #[test]
    fn prune_only_expired_directories() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("task");
        let file = dir.path().join("keep");
        fs::create_dir(&old).unwrap();
        fs::write(&file, "keep").unwrap();
        let modified = fs::metadata(&old).unwrap().modified().unwrap();
        prune(dir.path(), modified + RETENTION);
        assert!(old.exists());
        prune(dir.path(), modified + RETENTION + Duration::from_secs(1));
        assert!(!old.exists());
        assert!(file.exists());
    }
}
