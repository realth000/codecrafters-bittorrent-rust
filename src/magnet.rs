use anyhow::{bail, Context};

#[derive(Debug)]
pub struct Magnet {
    /// Hash of the info dictionary.
    pub info_hash: [u8; 20],

    /// Optional downloaded file name.
    pub download_name: Option<String>,

    /// Optional tracker url.
    pub tracker_url: Option<String>,
}

impl Magnet {
    pub fn new(magnet_str: &str) -> anyhow::Result<Self> {
        if !magnet_str.starts_with("magnet:?xt=urn:btih:") || magnet_str.len() < 20 + 40 {
            bail!("invalid prefix or too short")
        }

        let mut download_name = None;
        let mut tracker_url = None;

        let (_, magnet_str) = magnet_str.split_at(20);
        let (info_hash, magnet_str) = magnet_str.split_at(40);
        let info_hash = hex::decode(info_hash)
            .context("invalid info hash hex code")?
            .try_into()
            .unwrap();
        if magnet_str.is_empty() {
            return Ok(Self {
                info_hash,
                download_name,
                tracker_url,
            });
        }

        let segments = serde_urlencoded::from_str::<Vec<(String, String)>>(magnet_str)
            .context("invalid magnet str segments")?;
        for (name, value) in segments {
            match name.as_str() {
                "dn" => download_name = Some(value),
                "tr" => tracker_url = Some(value),
                _ => continue,
            }
        }

        Ok(Self {
            info_hash,
            download_name,
            tracker_url,
        })
    }

    pub fn print_info(&self) {
        if let Some(url) = &self.tracker_url {
            println!("Tracker URL: {}", url);
        }
        println!("Info Hash: {}", hex::encode(self.info_hash));
    }
}
