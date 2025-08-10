use anyhow::{bail, Context};
use serde_json::{self, Number};
use sha1::{Digest, Sha1};
use std::env;
use thiserror::Error;

pub type BtResult<T> = anyhow::Result<T, anyhow::Error>;

#[derive(Debug, Error)]
enum BtError {
    #[error("data already consumed")]
    Ended,

    #[error("invalid string at {0}")]
    InvalidString(usize),

    #[error("invalid integer at {0}")]
    InvalidInterger(usize),

    #[error("invalid list at {0}")]
    InvalidList(usize),

    #[error("invalid map at {0}")]
    InvalidMap(usize),

    #[error("invalid key of map {1} at {0}")]
    InvalidMapKey(usize, serde_json::Value),

    #[error("char {ch} not found from pos {pos}")]
    CharNotFound { pos: usize, ch: u8 },
}

pub struct DecodeContext {
    /// The raw data to decode.
    data: Vec<u8>,

    /// Index of [data] currently decoding.
    pos: usize,
}

impl DecodeContext {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn position(&self, ch: u8) -> BtResult<usize> {
        if self.ended() {
            bail!(BtError::Ended)
        }

        match self.data.iter().skip(self.pos).position(|x| x == &ch) {
            Some(v) => Ok(v),
            None => Err(BtError::CharNotFound { pos: self.pos, ch }).context("when find_pos"),
        }
    }

    pub fn advance(&mut self) {
        self.pos += 1;
    }

    pub fn peek(&self) -> Option<&u8> {
        if self.ended() {
            return None;
        }

        Some(&self.data[self.pos])
    }

    pub fn next(&mut self) -> Option<&u8> {
        if self.ended() {
            return None;
        }
        let ch = &self.data[self.pos];
        self.pos += 1;
        return Some(ch);
    }

    pub fn advance_many(&mut self, step: usize) -> Option<&[u8]> {
        if self.ended() || self.pos + step > self.data.len() {
            return None;
        }

        let ret = &self.data[self.pos..(self.pos + step)];
        self.pos += step;
        Some(ret)
    }

    pub fn advance_to(&mut self, end: usize) -> Option<&[u8]> {
        if self.ended() || end > self.data.len() {
            return None;
        }

        let ret = &self.data[self.pos..end];
        self.pos = end;
        Some(ret)
    }

    pub fn ended(&self) -> bool {
        self.pos > self.data.len() - 1
    }
}

pub struct EncodeContext {
    data: Vec<u8>,
}

impl EncodeContext {
    pub fn new() -> Self {
        Self { data: vec![] }
    }

    pub fn push_char(&mut self, v: char) {
        self.data.push(v as u8);
    }

    pub fn push_usize(&mut self, v: usize) {
        let s = v.to_string();
        let chars = s.chars();
        let mut x = vec![];
        for ch in chars.into_iter().rev() {
            x.insert(0, ch as u8);
        }

        self.data.append(&mut x);
    }

    pub fn append(&mut self, mut data: Vec<u8>) {
        self.data.append(&mut data);
    }

    pub fn data(&self) -> &Vec<u8> {
        &self.data
    }
}

impl From<String> for DecodeContext {
    fn from(value: String) -> Self {
        Self::new(value.as_bytes().to_vec())
    }
}

impl From<&str> for DecodeContext {
    fn from(value: &str) -> Self {
        Self::new(value.as_bytes().to_vec())
    }
}

fn encode_bytes_to_string(d: &Vec<u8>) -> String {
    hex::encode(d)
}

fn decode_bytes_from_string(s: &str) -> Vec<u8> {
    hex::decode(s).unwrap()
}

fn u8_is_digit(n: &u8) -> bool {
    n >= &b'0' && n <= &b'9'
}

fn char_slice_to_usize(data: &[u8]) -> Option<usize> {
    let mut ret = 0;

    for (idx, d) in data.iter().rev().enumerate() {
        if u8_is_digit(d) {
            ret += (d.to_owned() as usize - 48) * 10_usize.pow(idx as u32);
        } else {
            return None;
        }
    }

    Some(ret)
}

fn char_slice_to_isize(data: &[u8]) -> Option<isize> {
    let mut ret = 0;
    let neg = if let Some(b'-') = data.iter().next() {
        true
    } else {
        false
    };

    let it = if neg {
        data.iter().skip(1)
    } else {
        data.iter().skip(0)
    };

    let mut p = if neg { data.len() - 1 } else { data.len() } as u32;

    for i in it {
        p -= 1;
        if u8_is_digit(i) {
            ret += (i.to_owned() as isize - 48) * 10_isize.pow(p);
        } else {
            return None;
        }
    }

    if neg {
        ret *= -1;
    }

    Some(ret)
}

/// String "5:hello" -> "hello"
fn decode_string(ctx: &mut DecodeContext) -> BtResult<String> {
    if ctx.peek().map(|x| u8_is_digit(x)) != Some(true) {
        bail!(BtError::InvalidString(ctx.pos()))
    }

    let col_idx = ctx
        .position(b':')
        .context("failed to find the end of length of string")?;
    let string_len = ctx
        .advance_many(col_idx)
        .with_context(|| format!("string length hint pos {col_idx} out of range"))
        .and_then(|x| char_slice_to_usize(x).context("invalid string length"))?;
    // Pass the ':' character.
    ctx.advance();
    let s = &ctx
        .advance_many(string_len)
        .with_context(|| format!("string idx {} out of range", string_len))?
        .iter()
        .map(|x| x.to_owned() as char)
        .collect::<String>();
    Ok(s.to_owned())
}

/// String "5:hello" -> "hello"
///
/// Like string, but contents are not valid utf8.
fn decode_bytes(ctx: &mut DecodeContext) -> BtResult<Vec<u8>> {
    if ctx.peek().map(|x| u8_is_digit(x)) != Some(true) {
        bail!(BtError::InvalidString(ctx.pos()))
    }

    let col_idx = ctx
        .position(b':')
        .context("failed to find the end of length of string")?;
    let string_len = ctx
        .advance_many(col_idx)
        .with_context(|| format!("string length hint pos {col_idx} out of range"))
        .and_then(|x| char_slice_to_usize(x).context("invalid string length"))?;
    // Pass the ':' character.
    ctx.advance();
    let s = &ctx
        .advance_many(string_len)
        .with_context(|| format!("string idx {} out of range", string_len))?
        .iter()
        .map(|x| x.to_owned())
        .collect::<Vec<u8>>();
    Ok(s.to_owned())
}

/// Interger "i52e" -> 52; "i-52e" -> -52
fn decode_integer(ctx: &mut DecodeContext) -> BtResult<isize> {
    if ctx.peek() != Some(&b'i') {
        bail!(BtError::InvalidInterger(ctx.pos()))
    }

    let interger_end_pos = ctx.position(b'e').unwrap();
    // When convert string to integer, do not include the trailing 'e'.
    ctx.advance();
    let number = ctx
        .advance_many(interger_end_pos - 1)
        .context("out of range")
        .and_then(|x| {
            char_slice_to_isize(x).with_context(|| format!("invalid isize value \"{x:?}\""))
        })
        .context("invalid integer number")?;
    ctx.advance();

    Ok(number)
}

/// List starts with "l" and ends with "e".
/// "l5:helloi52ee" ["hello", 52]
///
/// Returns a json array.
fn decode_list(ctx: &mut DecodeContext) -> BtResult<serde_json::Value> {
    if ctx.peek() != Some(&b'l') {
        bail!(BtError::InvalidList(ctx.pos()))
    }
    // Pass the head of list "l".
    ctx.advance();

    let mut values = vec![];

    loop {
        match ctx.peek() {
            Some(b'e') => break,
            None => break,
            _ => { /* continue parsing list */ }
        }

        let value = decode_bencoded_value(ctx)
            .with_context(|| format!("failed to decode list element at pos {}", ctx.pos()))?;
        values.push(value);
    }
    ctx.advance();

    let ret = serde_json::Value::Array(values);
    Ok(ret)
}

/// Dictionary
///
/// d<key1><value1>...<keyN><valueN>e
/// "d3:foo3:bar5:helloi52ee" -> {"hello": 52, "foo":"bar"}
///
/// Key must be string and sorted.
fn decode_dictionary(ctx: &mut DecodeContext) -> BtResult<serde_json::Value> {
    if ctx.peek() != Some(&b'd') {
        bail!(BtError::InvalidMap(ctx.pos()))
    }
    // Pass the heading "d".
    ctx.advance();

    #[derive(PartialEq, Eq)]
    enum ParseState {
        None,
        Key(String),
    }

    let mut state = ParseState::None;
    let mut values = serde_json::Map::new();
    loop {
        match ctx.peek() {
            Some(&b'e') => break,
            None => break,
            _ => { /* Continue parsing map */ }
        }

        match state {
            ParseState::None => {
                let value = decode_bencoded_value(ctx)
                    .with_context(|| format!("failed to decode dictionary at {}", ctx.pos()))?;
                match value.as_str() {
                    Some(v) => {
                        state = ParseState::Key(v.to_string());
                    }
                    None => return Err(BtError::InvalidMapKey(ctx.pos, value).into()),
                }
            }
            ParseState::Key(k) => {
                if k == "pieces" {
                    let value = decode_bytes(ctx)
                        .with_context(|| format!("failed to decode dictionary at {}", ctx.pos()))?;
                    values.insert(k, serde_json::Value::String(encode_bytes_to_string(&value)));
                    state = ParseState::None;
                } else {
                    let value = decode_bencoded_value(ctx)
                        .with_context(|| format!("failed to decode dictionary at {}", ctx.pos()))?;
                    values.insert(k, value);
                    state = ParseState::None;
                }
            }
        }
    }

    let ret = serde_json::Value::Object(values);
    Ok(ret)
}

fn decode_bencoded_value(ctx: &mut DecodeContext) -> BtResult<serde_json::Value> {
    let flag = ctx.peek().context("reached the end of data")?;
    if u8_is_digit(flag) {
        let s = decode_string(ctx).context("failed to decode string")?;
        return Ok(serde_json::Value::String(s));
    } else if flag == &b'i' {
        let n = decode_integer(ctx).context("failed to decode interger")?;
        return Ok(serde_json::Value::Number(Number::from(n)));
    } else if flag == &b'l' {
        return decode_list(ctx);
    } else if flag == &b'd' {
        return decode_dictionary(ctx);
    } else {
        panic!("unsupported format");
    }
}

/// String "5:hello" -> "hello"
fn encode_string(ctx: &mut EncodeContext, s: &str) {
    ctx.push_usize(s.len());
    ctx.push_char(':');
    ctx.append(s.as_bytes().to_vec());
}

/// Interger "i52e" -> 52; "i-52e" -> -52
fn encode_integer(ctx: &mut EncodeContext, i: isize) {
    ctx.push_char('i');
    if i < 0 {
        ctx.push_char('-');
        ctx.push_usize(i as usize);
    } else {
        ctx.push_usize(i.abs() as usize);
    }
    ctx.push_char('e');
}

/// List starts with "l" and ends with "e".
/// "l5:helloi52ee" ["hello", 52]
fn encode_list(ctx: &mut EncodeContext, v: &Vec<serde_json::Value>) {
    ctx.push_char('l');
    for vv in v {
        encode_json_value(ctx, vv);
    }
    ctx.push_char('e');
}

/// Dictionary
///
/// d<key1><value1>...<keyN><valueN>e
/// "d3:foo3:bar5:helloi52ee" -> {"hello": 52, "foo":"bar"}
///
/// Key must be string and sorted.
fn encode_dictionary(ctx: &mut EncodeContext, v: &serde_json::Map<String, serde_json::Value>) {
    ctx.push_char('d');
    for (k, v) in v.iter() {
        encode_string(ctx, k);
        if k == "pieces" {
            let bs = decode_bytes_from_string(v.as_str().unwrap());
            ctx.push_usize(bs.len());
            ctx.push_char(':');
            ctx.append(bs);
        } else {
            encode_json_value(ctx, v);
        }
    }
    ctx.push_char('e');
}

fn encode_json_value(ctx: &mut EncodeContext, v: &serde_json::Value) {
    match v {
        serde_json::Value::Number(number) => encode_integer(ctx, number.as_i64().unwrap() as isize),
        serde_json::Value::String(s) => encode_string(ctx, s),
        serde_json::Value::Array(values) => encode_list(ctx, values),
        serde_json::Value::Object(map) => encode_dictionary(ctx, map),
        _ => panic!("unsupported data"),
    }
}

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
                // let name = info_map.get("name").and_then(|x| x.as_str()).unwrap();
                // let pieces_length = info_map
                //     .get("piece length")
                //     .and_then(|x| x.as_i64())
                //     .unwrap();
                // let pieces = info_map.get("pieces").and_then(|x| x.as_str()).unwrap();
                println!("Tracker URL: {announce}");
                println!("Length: {length}");

                let mut ctx = EncodeContext::new();
                encode_dictionary(&mut ctx, info_map);
                let mut hasher = Sha1::new();
                hasher.update(ctx.data.to_owned());
                let hash = hex::encode(hasher.finalize());
                println!("Info Hash: {hash}");
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
        assert_eq!(&ctx.data, ctx2.data());
        assert_eq!(
            String::from_utf8_lossy(&ctx.data[170..200]),
            String::from_utf8_lossy(&ctx2.data()[170..200]),
        );
        // panic!("{:?}\n{:?}", ctx.data, ctx2.data());
    }
}
