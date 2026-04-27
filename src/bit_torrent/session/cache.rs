use std::{path::PathBuf, sync::Arc};

use crate::{
    load, save,
    session::{BitField, resume::ResumeFile},
    torrent::metainfo::{Metainfo, TorrentFile},
    utils::{
        self,
        bqti::{ensure_download_dir, ensure_upload_dir},
    },
};

const TORRENT_FILE: &str = ".torrent";

pub enum CachingMode {
    Download {
        metafile: Arc<TorrentFile>,
    },
    Seed {
        user_space: PathBuf,
        metafile: Arc<TorrentFile>,
    },
}

pub struct SessionCache {
    pub dir: PathBuf,
    pub torrent: Arc<TorrentFile>,
    pub resume: Option<ResumeFile>,
}

impl SessionCache {
    pub async fn new(mode: CachingMode) -> Option<Self> {
        let (cache_dir, metafile) = match mode {
            CachingMode::Download { metafile } => (
                ensure_download_dir(metafile.info_hash().to_string())?,
                metafile,
            ),
            CachingMode::Seed {
                user_space,
                metafile,
            } => (ensure_upload_dir(&user_space, &metafile)?, metafile),
        };

        let resume = ResumeFile::open(&cache_dir, &metafile.info_hash()).await;

        let cache = Self {
            dir: cache_dir,
            torrent: metafile,
            resume,
        };

        Some(cache)
    }

    pub async fn load_from_cache() -> impl Iterator<Item = CachingMode> {
        let mut cache = vec![];

        let section: [(_, fn(PathBuf, Arc<TorrentFile>) -> CachingMode); 2] = [
            (utils::bqti::downloads_dir(), |_, metafile| {
                CachingMode::Download { metafile }
            }),
            (utils::bqti::uploads_dir(), |user_space, metafile| {
                CachingMode::Seed {
                    user_space,
                    metafile,
                }
            }),
        ];

        for (cache_dir, make_mode) in section {
            let Some(path) = cache_dir else { continue };

            let mut entries = match tokio::fs::read_dir(path).await {
                Ok(entries) => entries,
                Err(_) => continue,
            };

            while let Ok(Some(entry)) = entries.next_entry().await {
                let info_hash = entry.path();

                if !info_hash.is_dir() {
                    continue;
                }

                let Some(torrent_file) = Self::load_torrent(&info_hash).await else {
                    continue;
                };

                cache.push(make_mode(info_hash, torrent_file))
            }
        }

        cache.into_iter()
    }

    async fn load_torrent(dir: &PathBuf) -> Option<Arc<TorrentFile>> {
        load(dir.join(TORRENT_FILE)).ok().map(Arc::new)
    }

    pub async fn persist_torrent(&self) {
        let path = self.dir.join(TORRENT_FILE);

        if path.exists() {
            return;
        }

        match save(&path, &self.torrent) {
            Ok(_) => debug!("persisted {}", TORRENT_FILE),
            Err(_) => error!("failed to persist {}", TORRENT_FILE),
        }
    }

    pub async fn persist_resume(&self, bitfield: BitField) {
        let data = ResumeFile::new(&self.torrent.info_hash(), bitfield, &self.dir);

        if let Err(e) = data.persist(&self.dir).await {
            warn!("failed to persist resume: {}", e);
        }
    }
}
