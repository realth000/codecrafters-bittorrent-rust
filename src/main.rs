use anyhow::{bail, Context};
use sha1::{Digest, Sha1};
use std::env;

use crate::{
    decode::{decode_bencoded_value, DecodeContext},
    encode::{encode_dictionary, EncodeContext},
    utils::BtResult,
};

mod client;
mod decode;
mod encode;
mod utils;

fn main() -> BtResult<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        bail!("no args specified")
    }
    let command = &args[1];

    if command == "decode" {
        let mut ctx = DecodeContext::from(args[2].as_str());
        let decoded_value = decode_bencoded_value(&mut ctx)?;
        println!("{}", decoded_value.to_string());
    } else if command == "info" {
        let content =
            std::fs::read(&args[2]).with_context(|| format!("failed to read file from"))?;
        let mut ctx = DecodeContext::new(content);
        let decoded_value = decode_bencoded_value(&mut ctx)?;
        match decoded_value.as_object() {
            None => {
                println!("invalid info file map");
                std::process::exit(1);
            }
            Some(v) => {
                let announce = v.get("announce").and_then(|x| x.as_str()).unwrap();
                let info_map = v.get("info").and_then(|x| x.as_object()).unwrap();
                let length = info_map.get("length").and_then(|x| x.as_i64()).unwrap();
                println!("Tracker URL: {announce}");
                println!("Length: {length}");

                let mut ctx = EncodeContext::new();
                encode_dictionary(&mut ctx, info_map);
                let mut hasher = Sha1::new();
                hasher.update(&ctx.data());
                let hash = hex::encode(hasher.finalize());
                println!("Info Hash: {hash}");
                let piece_length = info_map
                    .get("piece length")
                    .and_then(|x| x.as_i64())
                    .unwrap();
                println!("Piece Length: {}", piece_length);
                let pieces = info_map.get("pieces").and_then(|x| x.as_str()).unwrap();
                println!("Piece Hashs:");
                for p in pieces.as_bytes().chunks_exact(40) {
                    let pstr = p.iter().map(|x| x.to_owned() as char).collect::<String>();
                    println!("{}", pstr);
                }
            }
        }
    } else {
        println!("unknown command: {}", args[1])
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
