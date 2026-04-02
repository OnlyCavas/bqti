use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct TorrentPath {
    base_path: PathBuf,
    pub paths: Vec<PathBuf>,
}

impl TorrentPath {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let input_path = path.into();

        let base_path = input_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from(""));

        Self {
            base_path,
            paths: Vec::new(),
        }
    }

    pub fn add(mut self, current: impl AsRef<Path>) -> Self {
        let current = current.as_ref();

        if current.is_file() {
            self.paths.push(current.to_path_buf());

            return self;
        }

        let Ok(entries) = fs::read_dir(current) else {
            return self;
        };

        for entry in entries {
            if let Ok(e) = entry {
                self = self.add(e.path());
            }
        }

        self
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    pub fn build(self) -> Vec<(PathBuf, PathBuf)> {
        self.paths
            .into_iter()
            .map(|abs| {
                let rel = abs
                    .strip_prefix(&self.base_path)
                    .unwrap_or(&abs)
                    .to_path_buf();

                (abs, rel)
            })
            .collect()
    }
}
