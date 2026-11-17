use crate::i18n::tr;
use argon2::Argon2;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_ID: &str = "default";
pub const AVATAR: &str = "avatar.png";
const PROFILE_FILE: &str = "profile.json";
const DATA_FILE: &str = "data.enc";
const WORK_DIR: &str = "work";
const MAGIC: &[u8; 8] = b"DLCENC01";
const CHECK_TEXT: &[u8] = b"dream-llmchatbox profile check";
const NONCE_LEN: usize = 24;
const NOT_DATA: [&str; 4] = [PROFILE_FILE, AVATAR, DATA_FILE, WORK_DIR];

pub type Key = [u8; 32];

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Profile {
    #[serde(skip)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub salt: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub check: String,
}

pub fn profiles_dir() -> PathBuf {
    crate::storage::base_dir().join("profiles")
}

pub fn profile_dir_in(base: &Path, id: &str) -> PathBuf {
    base.join(id)
}

pub fn avatar_path_in(base: &Path, id: &str) -> PathBuf {
    profile_dir_in(base, id).join(AVATAR)
}

pub fn load_in(base: &Path, id: &str) -> Option<Profile> {
    if id.is_empty() || id.starts_with('.') {
        return None;
    }
    let text = fs::read_to_string(profile_dir_in(base, id).join(PROFILE_FILE)).ok()?;
    let mut profile: Profile = serde_json::from_str(&text).ok()?;
    profile.id = id.to_string();
    Some(profile)
}

pub fn list_in(base: &Path) -> Vec<Profile> {
    let mut list: Vec<Profile> = fs::read_dir(base)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| load_in(base, &entry.file_name().to_string_lossy()))
                .collect()
        })
        .unwrap_or_default();
    list.sort_by(|a, b| {
        (a.id != DEFAULT_ID)
            .cmp(&(b.id != DEFAULT_ID))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.id.cmp(&b.id))
    });
    list
}

pub fn save_in(base: &Path, profile: &Profile) -> Result<(), String> {
    let dir = profile_dir_in(base, &profile.id);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(profile).map_err(|e| e.to_string())?;
    write_atomic(&dir.join(PROFILE_FILE), json.as_bytes())
}

pub fn create_in(base: &Path, name: &str) -> Result<Profile, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(tr("Please enter a name"));
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let mut id = format!("p{stamp}");
    let mut counter = 1;
    while profile_dir_in(base, &id).exists() {
        counter += 1;
        id = format!("p{stamp}-{counter}");
    }
    let profile = Profile {
        id,
        name: name.to_string(),
        ..Default::default()
    };
    save_in(base, &profile)?;
    Ok(profile)
}

pub fn delete_in(base: &Path, id: &str) -> Result<(), String> {
    if load_in(base, id).is_none() {
        return Err(tr("Profile not found"));
    }
    fs::remove_dir_all(profile_dir_in(base, id)).map_err(|e| e.to_string())
}

fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

pub fn derive_key(password: &str, salt: &[u8]) -> Result<Key, String> {
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| e.to_string())?;
    Ok(key)
}

fn seal(key: &Key, data: &[u8]) -> Result<Vec<u8>, String> {
    let nonce: [u8; NONCE_LEN] = random()?;
    let cipher = XChaCha20Poly1305::new(&(*key).into());
    let encrypted = cipher
        .encrypt(&XNonce::from(nonce), data)
        .map_err(|_| tr("Encryption failed"))?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&encrypted);
    Ok(out)
}

fn unseal(key: &Key, data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < NONCE_LEN {
        return Err(tr("Encrypted data is damaged"));
    }
    let (nonce, encrypted) = data.split_at(NONCE_LEN);
    let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| tr("Encrypted data is damaged"))?;
    let cipher = XChaCha20Poly1305::new(&(*key).into());
    cipher
        .decrypt(&XNonce::from(nonce), encrypted)
        .map_err(|_| tr("Wrong password or damaged data"))
}

pub fn unlock_key(profile: &Profile, password: &str) -> Result<Key, String> {
    let salt = STANDARD.decode(&profile.salt).map_err(|e| e.to_string())?;
    let check = STANDARD.decode(&profile.check).map_err(|e| e.to_string())?;
    let key = derive_key(password, &salt)?;
    match unseal(&key, &check) {
        Ok(text) if text == CHECK_TEXT => Ok(key),
        _ => Err(tr("Wrong password")),
    }
}

#[cfg(test)]
fn new_lock(password: &str) -> Result<(Key, String, String), String> {
    if password.is_empty() {
        return Err("Please enter a password".to_string());
    }
    let salt: [u8; 16] = random()?;
    let key = derive_key(password, &salt)?;
    let check = seal(&key, CHECK_TEXT)?;
    Ok((key, STANDARD.encode(salt), STANDARD.encode(check)))
}

fn collect_files(root: &Path, dir: &Path, top: bool, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if top && NOT_DATA.contains(&name.as_str()) {
            continue;
        }
        if name.ends_with("-shm") || name.ends_with(".tmp") {
            continue;
        }
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            collect_files(root, &path, false, out);
        } else if kind.is_file()
            && let Ok(relative) = path.strip_prefix(root)
        {
            let relative: Vec<String> = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect();
            out.push((relative.join("/"), path));
        }
    }
}

fn data_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut files = Vec::new();
    collect_files(dir, dir, true, &mut files);
    files.sort();
    files
}

pub fn pack(dir: &Path) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for (name, path) in data_files(dir) {
        let bytes = fs::read(&path).map_err(|e| format!("{name}: {e}"))?;
        out.extend_from_slice(&(name.len() as u32).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        out.extend_from_slice(&bytes);
    }
    Ok(out)
}

fn safe_relative(name: &str) -> Option<PathBuf> {
    let mut path = PathBuf::new();
    for part in name.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains('\\') {
            return None;
        }
        path.push(part);
    }
    Some(path)
}

fn take<'a>(rest: &mut &'a [u8], len: usize) -> Result<&'a [u8], String> {
    if rest.len() < len {
        return Err(tr("The archive is damaged"));
    }
    let (head, tail) = rest.split_at(len);
    *rest = tail;
    Ok(head)
}

pub fn unpack(data: &[u8], dir: &Path) -> Result<(), String> {
    let damaged = || tr("The archive is damaged");
    let mut rest = data;
    let mut files = Vec::new();
    while !rest.is_empty() {
        let len = u32::from_le_bytes(take(&mut rest, 4)?.try_into().map_err(|_| damaged())?) as usize;
        let name = std::str::from_utf8(take(&mut rest, len)?).map_err(|_| damaged())?.to_string();
        let size = u64::from_le_bytes(take(&mut rest, 8)?.try_into().map_err(|_| damaged())?) as usize;
        let bytes = take(&mut rest, size)?;
        let relative = safe_relative(&name).ok_or_else(damaged)?;
        if NOT_DATA.contains(&name.as_str()) {
            return Err(damaged());
        }
        files.push((relative, bytes));
    }
    for (relative, bytes) in files {
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(&path, bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    let result = (|| {
        let mut file = fs::File::create(&tmp)?;
        std::io::Write::write_all(&mut file, bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map_err(|e| e.to_string())
}

pub fn write_data_in(base: &Path, id: &str, key: &Key, archive: &[u8]) -> Result<(), String> {
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&seal(key, archive)?);
    write_atomic(&profile_dir_in(base, id).join(DATA_FILE), &out)
}

pub fn read_data_in(base: &Path, id: &str, key: &Key) -> Result<Vec<u8>, String> {
    let path = profile_dir_in(base, id).join(DATA_FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = fs::read(path).map_err(|e| e.to_string())?;
    let body = data.strip_prefix(MAGIC.as_slice()).ok_or_else(|| tr("Unknown file format"))?;
    unseal(key, body)
}

#[cfg(test)]
fn remove_data_files(dir: &Path) -> Result<(), String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if NOT_DATA.contains(&name.as_str()) {
            continue;
        }
        let path = entry.path();
        let result = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        result.map_err(|e| format!("{name}: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
pub fn set_password_in(base: &Path, id: &str, password: &str) -> Result<Key, String> {
    let mut profile = load_in(base, id).ok_or_else(|| tr("Profile not found"))?;
    if profile.locked {
        return Err(tr("The profile already has a password"));
    }
    let (key, salt, check) = new_lock(password)?;
    let dir = profile_dir_in(base, id);
    let archive = pack(&dir)?;
    write_data_in(base, id, &key, &archive)?;
    profile.locked = true;
    profile.salt = salt;
    profile.check = check;
    save_in(base, &profile)?;
    remove_data_files(&dir)?;
    Ok(key)
}

#[cfg(test)]
pub fn change_password_in(base: &Path, id: &str, archive: &[u8], password: &str) -> Result<Key, String> {
    let mut profile = load_in(base, id).ok_or_else(|| tr("Profile not found"))?;
    let (key, salt, check) = new_lock(password)?;
    let old = profile_dir_in(base, id).join(DATA_FILE);
    let backup = old.with_extension("old");
    if old.exists() {
        fs::copy(&old, &backup).map_err(|e| e.to_string())?;
    }
    write_data_in(base, id, &key, archive)?;
    profile.locked = true;
    profile.salt = salt;
    profile.check = check;
    if let Err(e) = save_in(base, &profile) {
        if backup.exists() {
            let _ = fs::rename(&backup, &old);
        }
        return Err(e);
    }
    let _ = fs::remove_file(backup);
    Ok(key)
}

pub fn remove_password_in(base: &Path, id: &str, archive: &[u8]) -> Result<(), String> {
    let mut profile = load_in(base, id).ok_or_else(|| tr("Profile not found"))?;
    let dir = profile_dir_in(base, id);
    unpack(archive, &dir)?;
    profile.locked = false;
    profile.salt.clear();
    profile.check.clear();
    save_in(base, &profile)?;
    let _ = fs::remove_file(dir.join(DATA_FILE));
    Ok(())
}

pub fn work_dir_in(runtime: Option<&Path>, base: &Path, id: &str) -> (PathBuf, bool) {
    match runtime.filter(|dir| dir.is_dir()) {
        Some(dir) => (dir.join("dream-llmchatbox").join(id), true),
        None => (profile_dir_in(base, id).join(WORK_DIR), false),
    }
}

pub fn work_dir(id: &str) -> (PathBuf, bool) {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    work_dir_in(runtime.as_deref(), &profiles_dir(), id)
}

fn newest_change(dir: &Path) -> Option<SystemTime> {
    data_files(dir)
        .iter()
        .filter_map(|(_, path)| fs::metadata(path).and_then(|m| m.modified()).ok())
        .max()
}

fn create_private_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Some(parent) = dir.parent() {
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

pub fn unlock_into(base: &Path, id: &str, key: &Key, work: &Path) -> Result<bool, String> {
    let data = profile_dir_in(base, id).join(DATA_FILE);
    let saved = fs::metadata(&data).and_then(|m| m.modified()).ok();
    if let Some(changed) = newest_change(work)
        && saved.is_none_or(|saved| changed > saved)
    {
        return Ok(true);
    }
    let archive = read_data_in(base, id, key)?;
    if work.exists() {
        fs::remove_dir_all(work).map_err(|e| e.to_string())?;
    }
    create_private_dir(work)?;
    unpack(&archive, work)?;
    Ok(false)
}

pub fn fingerprint(dir: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    for (name, path) in data_files(dir) {
        name.hash(&mut hasher);
        if let Ok(meta) = fs::metadata(&path) {
            meta.len().hash(&mut hasher);
            meta.modified().ok().hash(&mut hasher);
        }
    }
    hasher.finish()
}

fn move_path(from: &Path, to: &Path) -> Result<(), String> {
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    copy_all(from, to)?;
    if from.is_dir() {
        fs::remove_dir_all(from).map_err(|e| e.to_string())
    } else {
        fs::remove_file(from).map_err(|e| e.to_string())
    }
}

fn copy_all(from: &Path, to: &Path) -> Result<(), String> {
    if from.is_dir() {
        fs::create_dir_all(to).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(from).map_err(|e| e.to_string())?.flatten() {
            copy_all(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(from, to).map(|_| ()).map_err(|e| e.to_string())
    }
}

pub fn migrate_in(data: &Path, config: &Path, name: &str, about: &str) -> Result<(), String> {
    crate::storage::move_old_database(config, data);
    let base = data.join("profiles");
    let old_groups = data.join("groups");
    let old_args = config.join("start_args.json");
    let has_old = data.join("chats.db").exists() || old_groups.exists() || old_args.exists();
    if !has_old && !list_in(&base).is_empty() {
        return Ok(());
    }
    let mut profile = load_in(&base, DEFAULT_ID).unwrap_or_else(|| Profile {
        id: DEFAULT_ID.to_string(),
        name: if name.trim().is_empty() { "Me".to_string() } else { name.trim().to_string() },
        about: about.trim().to_string(),
        ..Default::default()
    });
    profile.id = DEFAULT_ID.to_string();
    save_in(&base, &profile)?;
    if profile.locked {
        return Ok(());
    }
    let dest = profile_dir_in(&base, DEFAULT_ID);
    if data.join("chats.db").exists() && !dest.join("chats.db").exists() {
        for file in crate::storage::DB_FILES {
            let from = data.join(file);
            if from.exists() {
                move_path(&from, &dest.join(file))?;
            }
        }
    }
    if old_groups.is_dir() {
        let target = dest.join("groups");
        if target.exists() {
            for entry in fs::read_dir(&old_groups).map_err(|e| e.to_string())?.flatten() {
                let to = target.join(entry.file_name());
                if !to.exists() {
                    move_path(&entry.path(), &to)?;
                }
            }
            if fs::read_dir(&old_groups).is_ok_and(|mut entries| entries.next().is_none()) {
                let _ = fs::remove_dir(&old_groups);
            }
        } else {
            move_path(&old_groups, &target)?;
        }
    }
    if old_args.exists() && !dest.join("start_args.json").exists() {
        move_path(&old_args, &dest.join("start_args.json"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dlc-profiles-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn create_list_delete() {
        let base = temp("list");
        assert!(create_in(&base, "  ").is_err());
        let anna = create_in(&base, "Anna").unwrap();
        let bob = create_in(&base, "bob").unwrap();
        assert_ne!(anna.id, bob.id);
        save_in(&base, &Profile { id: DEFAULT_ID.into(), name: "Zed".into(), ..Default::default() }).unwrap();
        let names: Vec<String> = list_in(&base).into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["Zed", "Anna", "bob"]);
        delete_in(&base, &bob.id).unwrap();
        assert_eq!(list_in(&base).len(), 2);
        assert!(load_in(&base, "..").is_none());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn archive_roundtrip_and_safety() {
        let base = temp("pack");
        let dir = base.join("a");
        fs::create_dir_all(dir.join("groups/g1/memory")).unwrap();
        fs::write(dir.join("chats.db"), b"db").unwrap();
        fs::write(dir.join("chats.db-shm"), b"shm").unwrap();
        fs::write(dir.join("groups/g1/memory/overview.md"), "# Hi ü").unwrap();
        fs::write(dir.join(AVATAR), b"png").unwrap();
        let archive = pack(&dir).unwrap();
        let out = base.join("b");
        unpack(&archive, &out).unwrap();
        assert_eq!(fs::read(out.join("chats.db")).unwrap(), b"db");
        assert_eq!(fs::read_to_string(out.join("groups/g1/memory/overview.md")).unwrap(), "# Hi ü");
        assert!(!out.join(AVATAR).exists());
        assert!(!out.join("chats.db-shm").exists());

        let mut evil = Vec::new();
        let name = b"../x";
        evil.extend_from_slice(&(name.len() as u32).to_le_bytes());
        evil.extend_from_slice(name);
        evil.extend_from_slice(&1u64.to_le_bytes());
        evil.push(1);
        assert!(unpack(&evil, &base.join("c")).is_err());
        assert!(unpack(&archive[..archive.len() - 1], &base.join("d")).is_err());
        assert!(unpack(&archive[..2], &base.join("e")).is_err());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn password_lock_unlock() {
        let base = temp("crypt");
        let runtime = temp("runtime");
        let profile = create_in(&base, "Secret").unwrap();
        let dir = profile_dir_in(&base, &profile.id);
        fs::write(dir.join("chats.db"), b"very secret chat").unwrap();
        fs::create_dir_all(dir.join("groups/g/memory")).unwrap();
        fs::write(dir.join("groups/g/memory/overview.md"), b"memory").unwrap();
        fs::write(dir.join("start_args.json"), b"[]").unwrap();
        fs::write(dir.join(AVATAR), b"png").unwrap();

        assert!(set_password_in(&base, &profile.id, "").is_err());
        let key = set_password_in(&base, &profile.id, "hunter2").unwrap();
        assert!(!dir.join("chats.db").exists());
        assert!(!dir.join("groups").exists());
        assert!(dir.join(AVATAR).exists());
        let raw = fs::read(dir.join(DATA_FILE)).unwrap();
        assert!(!raw.windows(6).any(|w| w == b"secret"));

        let locked = load_in(&base, &profile.id).unwrap();
        assert!(locked.locked);
        assert_eq!(unlock_key(&locked, "wrong").unwrap_err(), "Wrong password");
        assert_eq!(unlock_key(&locked, "hunter2").unwrap(), key);
        assert!(read_data_in(&base, &profile.id, &[7u8; 32]).is_err());

        let (work, in_ram) = work_dir_in(Some(&runtime), &base, &profile.id);
        assert!(in_ram);
        assert!(!unlock_into(&base, &profile.id, &key, &work).unwrap());
        assert_eq!(fs::read(work.join("chats.db")).unwrap(), b"very secret chat");
        fs::write(work.join("start_args.json"), b"[1]").unwrap();
        let before = fingerprint(&work);
        fs::write(work.join("new.txt"), b"x").unwrap();
        assert_ne!(before, fingerprint(&work));
        write_data_in(&base, &profile.id, &key, &pack(&work).unwrap()).unwrap();
        fs::remove_dir_all(&work).unwrap();
        assert!(!work.exists());

        assert!(!unlock_into(&base, &profile.id, &key, &work).unwrap());
        assert_eq!(fs::read(work.join("start_args.json")).unwrap(), b"[1]");
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(work.join("crash.txt"), b"after crash").unwrap();
        assert!(unlock_into(&base, &profile.id, &key, &work).unwrap());
        assert!(work.join("crash.txt").exists());

        let archive = pack(&work).unwrap();
        let new_key = change_password_in(&base, &profile.id, &archive, "new").unwrap();
        let changed = load_in(&base, &profile.id).unwrap();
        assert!(unlock_key(&changed, "hunter2").is_err());
        assert_eq!(unlock_key(&changed, "new").unwrap(), new_key);
        assert_eq!(read_data_in(&base, &profile.id, &new_key).unwrap(), archive);

        remove_password_in(&base, &profile.id, &archive).unwrap();
        let plain = load_in(&base, &profile.id).unwrap();
        assert!(!plain.locked && plain.salt.is_empty());
        assert!(!dir.join(DATA_FILE).exists());
        assert_eq!(fs::read(dir.join("chats.db")).unwrap(), b"very secret chat");
        assert!(dir.join("crash.txt").exists());

        let (fallback, in_ram) = work_dir_in(None, &base, &profile.id);
        assert!(!in_ram);
        assert_eq!(fallback, dir.join(WORK_DIR));
        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&runtime);
    }

    #[test]
    fn migration_moves_old_data_once() {
        let data = temp("mig-data");
        let config = temp("mig-config");
        fs::write(data.join("chats.db"), b"db").unwrap();
        fs::write(data.join("chats.db-wal"), b"wal").unwrap();
        fs::create_dir_all(data.join("groups/g1/memory")).unwrap();
        fs::write(data.join("groups/g1/group.json"), b"{}").unwrap();
        fs::write(config.join("start_args.json"), b"[]").unwrap();

        migrate_in(&data, &config, "Yak", "About").unwrap();
        let base = data.join("profiles");
        let dest = profile_dir_in(&base, DEFAULT_ID);
        let profile = load_in(&base, DEFAULT_ID).unwrap();
        assert_eq!((profile.name.as_str(), profile.about.as_str()), ("Yak", "About"));
        assert_eq!(fs::read(dest.join("chats.db")).unwrap(), b"db");
        assert!(dest.join("chats.db-wal").exists());
        assert!(dest.join("groups/g1/group.json").exists());
        assert!(dest.join("start_args.json").exists());
        assert!(!data.join("chats.db").exists() && !data.join("groups").exists());
        assert!(!config.join("start_args.json").exists());

        fs::create_dir_all(data.join("groups/g2")).unwrap();
        migrate_in(&data, &config, "Other", "").unwrap();
        assert_eq!(load_in(&base, DEFAULT_ID).unwrap().name, "Yak");
        assert!(dest.join("groups/g2").exists());
        assert!(dest.join("groups/g1/group.json").exists());
        migrate_in(&data, &config, "Other", "").unwrap();
        assert_eq!(list_in(&base).len(), 1);

        let fresh = temp("mig-fresh");
        migrate_in(&fresh, &config, "", "").unwrap();
        assert_eq!(load_in(&fresh.join("profiles"), DEFAULT_ID).unwrap().name, "Me");
        for dir in [data, config, fresh] {
            let _ = fs::remove_dir_all(dir);
        }
    }
}
