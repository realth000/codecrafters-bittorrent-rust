use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::{
    decode::{decode_bencoded_value, DecodeContext},
    encode::{encode_dictionary, EncodeContext},
    utils::BtResult,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Torrent {
    #[serde(rename = "announce")]
    tracker_url: String,

    info: TorrentInfo,

    #[serde(skip_serializing, skip_deserializing)]
    info_hash: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct TorrentInfo {
    length: usize,

    name: String,

    #[serde(rename = "piece length")]
    piece_length: usize,

    pieces: String,

    #[serde(skip_serializing, skip_deserializing)]
    piece_hashes: Vec<Vec<u8>>,
}

impl Torrent {
    pub fn parse_from_file(file_path: &str) -> BtResult<Torrent> {
        let content =
            std::fs::read(file_path).with_context(|| format!("failed to read file from"))?;
        let mut ctx = DecodeContext::new(content);
        let torrent: Torrent = decode_bencoded_value(&mut ctx)
            .context("bencode decode failed")
            .and_then(serde_json::Value::try_into)?;
        Ok(torrent)
    }

    pub fn print_info(&self) {
        println!("Tracker URL: {}", self.tracker_url);
        println!("Length: {}", self.info.length);
        println!("Info Hash: {}", self.info_hash);
        println!("Piece Length: {}", self.info.piece_length);
        println!("Piece Hashs:");
        for ph in self.info.piece_hashes.iter() {
            let pstr = ph.iter().map(|x| x.to_owned() as char).collect::<String>();
            println!("{}", pstr);
        }
    }

    pub fn tracker_url(&self) -> &str {
        &self.tracker_url
    }

    pub fn info_hash(&self) -> &str {
        &self.info_hash
    }

    pub fn length(&self) -> usize {
        self.info.length
    }
}

impl TryFrom<serde_json::Value> for Torrent {
    type Error = anyhow::Error;

    fn try_from(value: serde_json::Value) -> Result<Self, Self::Error> {
        let info_map = value
            .get("info")
            .and_then(|x| x.as_object())
            .context("info map not found")?;
        let mut ctx = EncodeContext::new();
        encode_dictionary(&mut ctx, info_map);
        let mut hasher = Sha1::new();
        hasher.update(&ctx.data());
        let info_hash = hex::encode(hasher.finalize());

        let mut torrent = serde_json::from_value::<Self>(value)?;
        torrent.info_hash = info_hash;

        let mut piece_hashes = vec![];
        for p in torrent.info.pieces.as_bytes().chunks_exact(40) {
            let pstr = p.iter().map(|x| x.to_owned() as char).collect::<String>();
            piece_hashes.push(pstr);
        }
        torrent.info.piece_hashes = torrent
            .info
            .pieces
            .as_bytes()
            .chunks_exact(40)
            .map(|x| x.to_vec())
            .collect();

        Ok(torrent)
    }
}
