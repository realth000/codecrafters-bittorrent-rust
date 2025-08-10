use anyhow::{bail, Context};
use serde_json::{self, Number};
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

    #[error("char {ch} not found from pos {pos}")]
    CharNotFound { pos: usize, ch: char },
}

pub struct DecodeContext {
    /// The raw data to decode.
    data: Vec<char>,

    /// Index of [data] currently decoding.
    pos: usize,
}

impl DecodeContext {
    pub fn new(data: &str) -> Self {
        Self {
            data: data.chars().collect(),
            pos: 0,
        }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn position(&self, ch: char) -> BtResult<usize> {
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

    pub fn peek(&self) -> Option<&char> {
        if self.ended() {
            return None;
        }

        Some(&self.data[self.pos])
    }

    pub fn next(&mut self) -> Option<&char> {
        if self.ended() {
            return None;
        }
        let ch = &self.data[self.pos];
        self.pos += 1;
        return Some(ch);
    }

    pub fn advance_many(&mut self, step: usize) -> Option<&[char]> {
        if self.ended() || self.pos + step > self.data.len() {
            return None;
        }

        let ret = &self.data[self.pos..(self.pos + step)];
        self.pos += step;
        Some(ret)
    }

    pub fn advance_to(&mut self, end: usize) -> Option<&[char]> {
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

fn char_slice_to_usize(data: &[char]) -> Option<usize> {
    let mut ret = 0;

    for (idx, d) in data.iter().rev().enumerate() {
        if d.is_digit(10) {
            ret += (d.to_owned() as usize - 48) * 10_usize.pow(idx as u32);
        } else {
            return None;
        }
    }

    Some(ret)
}

fn char_slice_to_isize(data: &[char]) -> Option<isize> {
    let mut ret = 0;
    let neg = if let Some('-') = data.iter().next() {
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
        if i.is_digit(10) {
            ret += (i.to_owned() as isize - 48) * 10_isize.pow(p);
        } else {
            return None;
        }
    }

    Some(ret)
}

/// String "5:hello" -> "hello"
fn decode_string(ctx: &mut DecodeContext) -> BtResult<String> {
    if ctx.peek().map(|x| x.is_digit(10)) != Some(true) {
        bail!(BtError::InvalidString(ctx.pos()))
    }

    let col_idx = ctx
        .position(':')
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
        .collect::<String>();
    Ok(s.to_owned())
}

/// Interger "i52e" -> 52; "i-52e" -> -52
fn decode_integer(ctx: &mut DecodeContext) -> BtResult<isize> {
    if ctx.peek() != Some(&'i') {
        bail!(BtError::InvalidInterger(ctx.pos()))
    }

    let interger_end_pos = ctx.position('e').unwrap();
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
    if ctx.peek() != Some(&'l') {
        bail!(BtError::InvalidList(ctx.pos()))
    }
    // Pass the head of list "l".
    ctx.advance();

    let mut values = vec![];

    loop {
        match ctx.peek() {
            Some(v) if v == &'e' => break,
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

fn decode_bencoded_value(ctx: &mut DecodeContext) -> BtResult<serde_json::Value> {
    let flag = ctx.peek().context("reached the end of data")?;
    if flag.is_digit(10) {
        let s = decode_string(ctx).context("failed to decode string")?;
        return Ok(serde_json::Value::String(s));
    } else if flag == &'i' {
        let n = decode_integer(ctx).context("failed to decode interger")?;
        return Ok(serde_json::Value::Number(Number::from(n)));
    } else if flag == &'l' {
        return decode_list(ctx);
    } else {
        panic!("unsupported format");
    }
}

fn main() -> BtResult<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        bail!("no args specified")
    }
    let command = &args[1];

    if command == "decode" {
        let encoded_value = &args[2];
        let mut ctx = DecodeContext::new(&encoded_value);
        let decoded_value = decode_bencoded_value(&mut ctx)?;
        println!("{}", decoded_value.to_string());
    } else {
        println!("unknown command: {}", args[1])
    }

    Ok(())
}
