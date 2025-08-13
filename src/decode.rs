use anyhow::{bail, Context};
use serde_json::Number;

use crate::utils::{
    char_slice_to_isize, char_slice_to_usize, encode_bytes_to_string, u8_is_digit, BtError,
    BtResult,
};

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

    fn pos(&self) -> usize {
        self.pos
    }

    fn position(&self, ch: u8) -> BtResult<usize> {
        if self.ended() {
            bail!(BtError::Ended)
        }

        match self.data.iter().skip(self.pos).position(|x| x == &ch) {
            Some(v) => Ok(v),
            None => Err(BtError::CharNotFound { pos: self.pos, ch }).context("when find_pos"),
        }
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    fn peek(&self) -> Option<&u8> {
        if self.ended() {
            return None;
        }

        Some(&self.data[self.pos])
    }

    fn advance_many(&mut self, step: usize) -> Option<&[u8]> {
        if self.ended() || self.pos + step > self.data.len() {
            return None;
        }

        let ret = &self.data[self.pos..(self.pos + step)];
        self.pos += step;
        Some(ret)
    }

    fn ended(&self) -> bool {
        self.pos > self.data.len() - 1
    }

    /// Used in test.
    #[allow(unused)]
    pub fn data(&self) -> &Vec<u8> {
        &self.data
    }
}

impl From<&str> for DecodeContext {
    fn from(value: &str) -> Self {
        Self::new(value.as_bytes().to_vec())
    }
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

pub fn decode_bencoded_value(ctx: &mut DecodeContext) -> BtResult<serde_json::Value> {
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
