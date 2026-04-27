use std::fmt::Display;

#[derive(Debug)]
pub struct MagnetLink {
    pub hash: String,
    pub name: Option<String>,
    pub bootstrap: Option<String>,
    pub trackers: Vec<String>,
    pub web_seed: Option<String>,
}

impl MagnetLink {
    pub fn new(link: &str) -> Option<Self> {
        let query = link.strip_prefix("magnet:?")?;

        let mut hash = None;
        let mut name = None;
        let mut bootstrap = None;
        let mut trackers = Vec::new();
        let mut web_seed = None;

        for pair in query.split('&') {
            let (key, value) = pair.split_once('=')?;

            match key {
                "xt" => hash = value.strip_prefix("urn:bqti:").map(str::to_string),
                "dn" => name = Some(value.to_string()),
                "bs" => bootstrap = Some(value.to_string()),
                "tr" => trackers.push(value.to_string()),
                "ws" => web_seed = Some(value.to_string()),
                _ => {}
            }
        }

        Some(MagnetLink {
            hash: hash?,
            name,
            bootstrap,
            trackers,
            web_seed,
        })
    }
}

impl Display for MagnetLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "magnet:?xt=urn:bqti:{}", self.hash)?;

        if let Some(name) = &self.name {
            write!(f, "&dn={}", name)?;
        }

        if let Some(bootstrap) = &self.bootstrap {
            write!(f, "&bs={}", bootstrap)?;
        }

        for tracker in &self.trackers {
            write!(f, "&tr={}", tracker)?;
        }

        if let Some(seed) = &self.web_seed {
            write!(f, "&ws={}", seed)?;
        }

        Ok(())
    }
}
