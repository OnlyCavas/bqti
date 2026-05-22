use std::{
    fs::{self, File},
    io,
    os::unix::fs::{DirBuilderExt, symlink},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use directories::ProjectDirs;

use crate::{
    bit_torrent::torrent::metainfo::{Metainfo, TorrentFile},
    types::UnixDate,
};

const UPLOADS_DIRECTORY: &'static str = "uploads";
const DOWNLOADS_DIRECTORY: &'static str = "downloads";

pub fn bqti_data_dir() -> Option<PathBuf> {
    if let Some(base) = std::env::var_os("XDG_DATA_HOME") {
        return Some(PathBuf::from(base).join("bqti"));
    }

    ProjectDirs::from("", "", "bqti").map(|proj| proj.data_dir().to_path_buf())
}

pub fn uploads_dir() -> Option<PathBuf> {
    let uploads_dir = bqti_data_dir()?.join(UPLOADS_DIRECTORY);
    uploads_dir.exists().then_some(uploads_dir)
}

pub fn downloads_dir() -> Option<PathBuf> {
    let downloads_dir = bqti_data_dir()?.join(DOWNLOADS_DIRECTORY);
    downloads_dir.exists().then_some(downloads_dir)
}

pub async fn link(user_downloads_dir: PathBuf, info_hash_hex: String) -> io::Result<PathBuf> {
    let internal_dir = bqti_data_dir()
        .map(|p| p.join("downloads").join(&info_hash_hex))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Could not find bqti data dir"))?;

    tokio::task::spawn_blocking(move || {
        use std::os::unix::fs::PermissionsExt;

        for entry in fs::read_dir(&internal_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                let mut perms = fs::metadata(&path)?.permissions();
                perms.set_mode(0o400);
                fs::set_permissions(&path, perms)?;
            }
        }

        let mut perms = fs::metadata(&internal_dir)?.permissions();

        perms.set_mode(0o700);

        fs::set_permissions(&internal_dir, perms)?;

        let dest_symlink = user_downloads_dir.join("download");

        if dest_symlink.exists() || dest_symlink.is_symlink() {
            fs::remove_file(&dest_symlink).or_else(|_| fs::remove_dir_all(&dest_symlink))?;
        }

        symlink(&internal_dir, &dest_symlink)?;

        Ok(dest_symlink)
    })
    .await
    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
}

// pub fn ensure_upload_dir(user_base: &Path, metadata: &TorrentFile) -> Option<PathBuf> {
//     let bqti_dir = bqti_data_dir()?;
//     let internal_base = bqti_dir
//         .join(UPLOADS_DIRECTORY)
//         .join(metadata.info_hash().to_string());
//
//     fs::DirBuilder::new()
//         .recursive(true)
//         .mode(0o700)
//         .create(&internal_base)
//         .map_err(|e| warn!("failed to create {}: {e}", internal_base.display()))
//         .ok()?;
//
//     for file in metadata.files() {
//         let rel_path = file.to_path();
//
//         if !user_base.exists() {
//             return None;
//         }
//
//         let is_single = rel_path.as_os_str().is_empty() || rel_path == Path::new(".");
//
//         let target = if is_single {
//             internal_base.join(user_base.file_name()?)
//         } else {
//             internal_base.join(&rel_path)
//         };
//
//         let source: PathBuf = if is_single {
//             user_base.to_path_buf()
//         } else {
//             user_base.join(&rel_path).components().collect()
//         };
//
//         if !target.exists() {
//             if let Err(_) = fs::hard_link(&source, &target) {
//                 return None;
//             }
//         }
//     }
//
//     Some(internal_base)
// }

pub fn ensure_upload_dir(user_base: &Path, metadata: &TorrentFile) -> Option<PathBuf> {
    let bqti_dir = bqti_data_dir()?;

    let internal_base = bqti_dir
        .join(UPLOADS_DIRECTORY)
        .join(metadata.info_hash().to_string());

    for file in metadata.files() {
        let rel_path = file.to_path();

        if !user_base.exists() {
            return None;
        }

        let target = internal_base.join(&rel_path);
        info!("target: {}", target.to_string_lossy());

        if let Some(parent) = target.parent() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)
                .ok()?;
        }

        let target = if rel_path.as_os_str().is_empty() || rel_path == Path::new(".") {
            internal_base.join(user_base.file_name()?)
        } else {
            internal_base.join(&rel_path)
        };

        if !target.exists() {
            let source = user_base.join(rel_path);
            let source = source.components().collect::<PathBuf>();

            fs::hard_link(&source, &target).ok()?;
        }
    }

    Some(internal_base)
}

pub fn ensure_download_dir(entry: impl Into<PathBuf>) -> Option<PathBuf> {
    let base = bqti_data_dir()?;
    let path = base.join(DOWNLOADS_DIRECTORY).join(entry.into());

    if !path.exists() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&path)
            .ok()?;
    }

    Some(path)
}

pub fn certs_dir() -> Option<PathBuf> {
    let data = bqti_data_dir()?;
    Some(data.join("certs"))
}

pub async fn ensure_dir(path: &PathBuf) -> Result<()> {
    tokio::fs::create_dir_all(path)
        .await
        .with_context(|| format!("Failed to create directory {}", path.display()))
}

pub fn version() -> String {
    format!("{} v{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
}

pub fn fetch_current_timestamp() -> UnixDate {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub fn preallocate(file: &File, length: u64) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::AsRawFd;
        let ret = unsafe { libc::fallocate(file.as_raw_fd(), 0, 0, length as libc::off_t) };

        if ret != 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        file.set_len(length)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        file.set_len(length)
    }
}

pub fn ensure_parent_dirs(path: &PathBuf) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    Ok(())
}
