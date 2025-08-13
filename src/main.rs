use anyhow::Context;
use clap::{Args, Parser, Subcommand};

use crate::{
    decode::{decode_bencoded_value, DecodeContext},
    http::discover_peer,
    torrent::Torrent,
    utils::BtResult,
};

mod decode;
mod encode;
mod http;
mod torrent;
mod utils;

#[derive(Debug, Clone, Parser)]
struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Subcommand)]
enum Command {
    #[command(about = "decode bencode text data")]
    Decode(DecodeArgs),

    #[command(about = "print info in torrent file")]
    Info(InfoArgs),

    #[command(about = "work on torrent file with other peers")]
    Peer(PeerArgs),
}

#[derive(Debug, Clone, Args)]
struct DecodeArgs {
    #[arg(help = "text to decode")]
    text: String,
}

#[derive(Debug, Clone, Args)]
struct PeerArgs {
    #[arg(help = "torrent file path")]
    file_path: String,
}

#[derive(Debug, Clone, Args)]
struct InfoArgs {
    #[arg(help = "torrent file path")]
    file_path: String,
}

#[tokio::main]
async fn main() -> BtResult<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Decode(decode_args) => {
            let mut ctx = DecodeContext::from(decode_args.text.as_str());
            let decoded_value = decode_bencoded_value(&mut ctx)?;
            println!("{}", decoded_value.to_string());
        }
        Command::Info(info_args) => {
            let torrent = Torrent::parse_from_file(info_args.file_path.as_str())?;
            torrent.print_info();
        }
        Command::Peer(peer_args) => {
            let torrent = Torrent::parse_from_file(peer_args.file_path.as_str())?;
            let peer_info = discover_peer(
                torrent.tracker_url(),
                torrent.info_hash(),
                torrent.length(),
                0,
                0,
            )
            .await
            .context("failed to discover peer")?;
            for peer in peer_info.peers.iter() {
                println!("{}:{}", peer.ip, peer.port);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use serde::{Deserialize, Serialize};
    use serde_bytes::ByteBuf;

    use crate::{encode::encode_dictionary, utils::decode_bytes_from_string};

    use super::*;

    #[test]
    fn test_decode_integer() {
        let v = decode_bencoded_value(&mut DecodeContext::from("i52e")).unwrap();
        assert_eq!(v.to_string(), String::from("52"));

        let v2 = decode_bencoded_value(&mut DecodeContext::from("i-52e")).unwrap();
        assert_eq!(v2.to_string(), String::from("-52"));

        let v3 = decode_bencoded_value(&mut DecodeContext::from("i4294967300e")).unwrap();
        assert_eq!(v3.to_string(), String::from("4294967300"));
    }

    #[test]
    fn test_decode_string() {
        let v = decode_bencoded_value(&mut DecodeContext::from("5:hello")).unwrap();
        assert_eq!(v.to_string(), String::from(r#""hello""#));
    }

    #[test]
    fn test_decode_list() {
        let v = decode_bencoded_value(&mut DecodeContext::from("l5:mangoi921ee")).unwrap();
        assert_eq!(v.to_string(), String::from(r#"["mango",921]"#));
        let v2 = decode_bencoded_value(&mut DecodeContext::from("lli921e5:mangoee")).unwrap();
        assert_eq!(v2.to_string(), String::from(r#"[[921,"mango"]]"#));
        let v3 = decode_bencoded_value(&mut DecodeContext::from("lli4eei5ee")).unwrap();
        assert_eq!(v3.to_string(), String::from(r"[[4],5]"));
    }

    #[test]
    fn test_decode_dictionary() {
        let v = decode_bencoded_value(&mut DecodeContext::from("d3:foo3:bar5:helloi52ee")).unwrap();
        assert_eq!(v.to_string(), String::from(r#"{"foo":"bar","hello":52}"#));
        let v2 = decode_bencoded_value(&mut DecodeContext::from("de")).unwrap();
        assert_eq!(v2.to_string(), String::from("{}"));
        let v3 = decode_bencoded_value(&mut DecodeContext::from("d3:food3:foo3:bar5:helloi52eee"))
            .unwrap();
        assert_eq!(
            v3.to_string(),
            String::from(r#"{"foo":{"foo":"bar","hello":52}}"#)
        );
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct M {
        announce: String,
        info: MInfo,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct MInfo {
        length: usize,
        name: String,
        #[serde(rename = "piece length")]
        piece_length: usize,
        pieces: ByteBuf,
    }

    #[test]
    fn test_example() {
        let raw_data = std::fs::read("data/example.torrent").unwrap();
        let good: M = serde_bencode::from_bytes(raw_data.as_slice()).unwrap();
        let good_pieces = good.info.pieces;
        let mut ctx = DecodeContext::new(raw_data);
        // data in pieces string is hexed string.
        let decoded_value = decode_bencoded_value(&mut ctx).unwrap();
        let bad_pieces = decoded_value
            .as_object()
            .unwrap()
            .get("info")
            .and_then(|x| x.as_object())
            .unwrap()
            .get("pieces")
            .unwrap()
            .as_str()
            .unwrap();
        let bad_pieces = decode_bytes_from_string(bad_pieces);
        assert_eq!(good_pieces, bad_pieces);
        let mut ctx2 = EncodeContext::new();
        encode_dictionary(&mut ctx2, decoded_value.as_object().unwrap());
        assert_eq!(&ctx.data(), &ctx2.data());
        assert_eq!(
            String::from_utf8_lossy(&ctx.data()[170..200]),
            String::from_utf8_lossy(&ctx2.data()[170..200]),
        );
        // let mut hash_str = String::new();
        // for p in hex::encode(bad_pieces).as_bytes().to_vec().chunks_exact(40) {
        //     let pstr = p.iter().map(|x| x.to_owned() as char).collect::<String>();
        //     hash_str.push_str(pstr.as_str());
        //     hash_str.push('\n');
        // }
        // panic!("{}", hash_str);
    }
}
