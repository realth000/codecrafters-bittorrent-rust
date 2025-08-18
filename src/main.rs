use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use regex::Regex;

use crate::{
    decode::{decode_bencoded_value, DecodeContext},
    http::{discover_peer, download_file, download_piece, handshake, HandshakeMessage, PEER_ID},
    magnet::Magnet,
    torrent::Torrent,
    utils::BtResult,
};

mod decode;
mod encode;
mod http;
mod magnet;
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
    Peers(PeersArgs),

    #[command(about = "handshake with a target")]
    Handshake(HandshakeArgs),

    #[command(name = "download_piece", about = "download a piece of file")]
    DownloadPiece(DownloadPieceArgs),

    #[command(name = "download", about = "download whole file of torrent")]
    Download(DownloadArgs),

    #[command(name = "magnet_parse", about = "parse info from magnet string")]
    MagnetParse(MagnetParseArgs),
}

#[derive(Debug, Clone, Args)]
struct DecodeArgs {
    #[arg(help = "text to decode")]
    text: String,
}

#[derive(Debug, Clone, Args)]
struct PeersArgs {
    #[arg(help = "torrent file path")]
    file_path: String,
}

#[derive(Debug, Clone, Args)]
struct InfoArgs {
    #[arg(help = "torrent file path")]
    file_path: String,
}

#[derive(Debug, Clone, Args)]
struct HandshakeArgs {
    #[arg(help = "torrent file path")]
    file_path: String,

    /// IP and port, joined by ':'.
    #[arg(help = "ip and port to handshake, in format <ip>:<port>", value_parser=validate_ip_port)]
    ip_port: (String, u16),
}

#[derive(Debug, Clone, Args)]
struct DownloadPieceArgs {
    #[arg(short = 'o', long = "output", help = "path to save the piece of file")]
    output: String,

    #[arg(help = "torrent file path")]
    file_path: String,

    #[arg(help = "piece index")]
    index: usize,
}

#[derive(Debug, Clone, Args)]
struct DownloadArgs {
    #[arg(
        short = 'o',
        long = "output",
        help = "path to save the whole downloaded file"
    )]
    output: String,

    #[arg(help = "torrent file path")]
    file_path: String,
}

#[derive(Debug, Clone, Args)]
struct MagnetParseArgs {
    #[arg(help = "magnet string to parse")]
    magnet_str: String,
}

fn validate_ip_port(s: &str) -> Result<(String, u16), &'static str> {
    match s.split_once(':') {
        Some((ip, port)) => {
            let ip_re = Regex::new(r#"^((25[0-5]|(2[0-4]|1\d|[1-9]|)\d)\.?\b){4}$"#).unwrap();
            if !ip_re.is_match(ip) {
                return Err("invalid ip");
            }
            let port = if let Ok(p) = port.parse::<u16>() {
                p
            } else {
                return Err("invalid port");
            };

            Ok((ip.to_string(), port))
        }
        None => Err("invalid ip port format, expected to be <ip>:<port>, e.g. 192.168.0.1:54321"),
    }
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
        Command::Peers(peer_args) => {
            let torrent = Torrent::parse_from_file(peer_args.file_path.as_str())?;
            let peer_info = discover_peer(
                torrent.tracker_url(),
                torrent.info_hash(),
                0,
                0,
                torrent.length(),
            )
            .await
            .context("failed to discover peer")?;
            for peer in peer_info.peers.iter() {
                println!("{}:{}", peer.ip, peer.port);
            }
        }
        Command::Handshake(handshake_args) => {
            let torrent = Torrent::parse_from_file(handshake_args.file_path.as_str())?;
            let message = HandshakeMessage::new(
                torrent.info_hash().clone(),
                PEER_ID.as_bytes().try_into().unwrap(),
            );
            let resp = handshake(
                handshake_args.ip_port.0.as_str(),
                handshake_args.ip_port.1,
                message,
            )
            .await
            .context("handshake failed")?;
            println!("Peer ID: {}", hex::encode(resp.peer_id));
        }
        Command::DownloadPiece(download_piece_args) => {
            let torrent = Torrent::parse_from_file(download_piece_args.file_path.as_str())?;
            let peer_info = discover_peer(
                torrent.tracker_url(),
                torrent.info_hash(),
                0,
                0,
                torrent.length(),
            )
            .await
            .context("failed to discover peer")?;
            if peer_info.peers.is_empty() {
                eprintln!("no peers found");
                return Ok(());
            }
            download_piece(
                &torrent,
                &peer_info.peers,
                download_piece_args.output,
                download_piece_args.index,
            )
            .await?;
        }
        Command::Download(download_args) => {
            let torrent = Torrent::parse_from_file(download_args.file_path.as_str())?;
            let peer_info = discover_peer(
                torrent.tracker_url(),
                torrent.info_hash(),
                0,
                0,
                torrent.length(),
            )
            .await
            .context("failed to discover peer")?;
            if peer_info.peers.is_empty() {
                eprintln!("no peers found");
                return Ok(());
            }
            download_file(&torrent, &peer_info.peers, download_args.output).await?;
        }
        Command::MagnetParse(magnet_parse_args) => {
            let manget =
                Magnet::new(&magnet_parse_args.magnet_str).context("invalid magset string")?;
            manget.print_info();
        }
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use serde::{Deserialize, Serialize};
    use serde_bytes::ByteBuf;

    use crate::{
        encode::{encode_dictionary, EncodeContext},
        utils::decode_bytes_from_string,
    };

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
