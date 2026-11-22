//! 本次运行改过的文件，供界面撤回。原位置与备份只由后端记录。
use serde::Serialize;
use std::{collections::HashMap, fs, io::{self, Write}, path::{Path, PathBuf}, sync::Mutex, time::{Duration, SystemTime}};

const RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

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
    path.file_name().unwrap_or_else(|| std::ffi::OsStr::new("文件"))
}

fn staged_copy(from: &Path, to: &Path, replace: bool) -> io::Result<()> {
    let parent = to.parent().ok_or_else(|| io::Error::other("文件没有上级目录"))?;
    let staging = parent.join(format!(".floatball-{}", uuid::Uuid::new_v4()));
    if let Err(e) = fs::copy(from, &staging) {
        let _ = fs::remove_file(&staging);
        return Err(e);
    }
    let result = if replace { fs::rename(&staging, to) } else { fs::hard_link(&staging, to) };
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
    pub fn write(&self, path: &Path, content: &str, directory: &Path, sequence: &str, agent_id: &str) -> io::Result<Change> {
        let mut entries = self.0.lock().unwrap();
        let path = std::path::absolute(path)?;
        let parent = path.parent().ok_or_else(|| io::Error::other("文件没有上级目录"))?;
        let id = uuid::Uuid::new_v4().to_string();
        let before = stamp(&path)?;
        let backup = if before.is_some() {
            fs::create_dir_all(directory)?;
            let mut name = std::ffi::OsString::from(format!("{sequence}-"));
            name.push(filename(&path));
            let backup = directory.join(name);
            fs::copy(&path, &backup)?;
            Some(backup)
        } else { None };
        fs::create_dir_all(parent)?;
        let staging = parent.join(format!(".floatball-{id}"));
        let result: io::Result<_> = (|| {
            let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&staging)?;
            file.write_all(content.as_bytes())?;
            if let Some(backup) = &backup { file.set_permissions(fs::metadata(backup)?.permissions())?; }
            drop(file);
            let after = stamp(&staging)?;
            if stamp(&path)? != before { return Err(io::Error::other("文件已经变化，请重新尝试")); }
            if before.is_some() { fs::rename(&staging, &path)?; }
            else { fs::hard_link(&staging, &path)?; }
            Ok(after)
        })();
        let _ = fs::remove_file(staging);
        let after = result?;
        let change = Change { id, path: path.clone(), kind: if before.is_some() { "修改" } else { "新建" }, backup, agent_id: agent_id.into(), after };
        entries.insert(change.id.clone(), change.clone());
        Ok(change)
    }

    pub fn delete(&self, path: &Path, trash: &Path, agent_id: &str) -> io::Result<Change> {
        let mut entries = self.0.lock().unwrap();
        let path = std::path::absolute(path)?;
        if stamp(&path)?.is_none() { return Err(io::Error::other("找不到这个文件（文件夹不能删）")); }
        let backup = to_trash(&path, trash)?;
        let change = Change { id: uuid::Uuid::new_v4().to_string(), path, kind: "删除", backup: Some(backup), agent_id: agent_id.into(), after: None };
        entries.insert(change.id.clone(), change.clone());
        Ok(change)
    }

    pub fn undo(&self, id: &str, confirm_changed: bool, trash: &Path) -> Result<bool, String> {
        let mut entries = self.0.lock().unwrap();
        let change = entries.get(id).ok_or("找不到这条改动，可能已经撤回")?;
        let path = &change.path;
        let current = stamp(path).map_err(|e| e.to_string())?;
        if change.kind == "删除" && current.is_some() {
            return Err("原位置已经有同名文件，没有撤回".into());
        }
        if change.after != current && !confirm_changed { return Ok(false); }
        match change.kind {
            "新建" if current.is_some() => { to_trash(path, trash).map_err(|e| e.to_string())?; }
            "新建" => {}
            "修改" | "删除" => {
                let backup = change.backup.as_deref().ok_or("找不到备份")?;
                staged_copy(backup, path, change.kind == "修改" && current.is_some()).map_err(|e| {
                    if change.kind == "删除" && e.kind() == io::ErrorKind::AlreadyExists { "原位置已经有同名文件，没有撤回".into() }
                    else { e.to_string() }
                })?;
                if change.kind == "删除" { let _ = fs::remove_file(backup); }
            }
            _ => return Err("未知改动".into()),
        }
        entries.remove(id);
        Ok(true)
    }
}

pub fn prune(root: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) && entry.metadata().and_then(|meta| meta.modified()).is_ok_and(|t| now.duration_since(t).is_ok_and(|age| age > RETENTION)) {
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
        let change = changes.write(&path, "新的", &dir.path().join("backup"), "1", "lead").unwrap();
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
        let created = changes.write(&path, "初稿", &dir.path().join("backup"), "1", "lead").unwrap();
        assert_eq!(created.kind, "新建");
        assert_eq!(changes.undo(&created.id, false, &trash), Ok(true));
        assert!(!path.exists());
        let moved = fs::read_dir(&trash).unwrap().next().unwrap().unwrap().path();
        assert_eq!(fs::read_to_string(moved).unwrap(), "初稿");
        fs::write(&path, "待删除").unwrap();
        let deleted = changes.delete(&path, &trash, "lead").unwrap();
        assert_eq!(deleted.kind, "删除");
        fs::write(&path, "同名新文件").unwrap();
        assert_eq!(changes.undo(&deleted.id, false, &trash), Err("原位置已经有同名文件，没有撤回".into()));
        fs::remove_file(&path).unwrap();
        assert_eq!(changes.undo(&deleted.id, false, &trash), Ok(true));
        assert_eq!(fs::read_to_string(path).unwrap(), "待删除");
        assert!(!deleted.backup.unwrap().exists());
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
