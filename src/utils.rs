use thiserror::Error;

pub type BtResult<T> = anyhow::Result<T, anyhow::Error>;

#[derive(Debug, Error)]
pub enum BtError {
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

    #[error("invalid json value")]
    SerializationError(#[from] serde_json::Error),

    #[error("char {ch} not found from pos {pos}")]
    CharNotFound { pos: usize, ch: u8 },

    #[error("http request failed with status code {0}")]
    NetworkError(u16),
}

pub fn u8_is_digit(n: &u8) -> bool {
    n >= &b'0' && n <= &b'9'
}

pub fn char_slice_to_usize(data: &[u8]) -> Option<usize> {
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

pub fn char_slice_to_isize(data: &[u8]) -> Option<isize> {
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

pub fn decode_bytes_from_string(s: &str) -> Vec<u8> {
    hex::decode(s).unwrap()
}

pub fn encode_bytes_to_string(d: &Vec<u8>) -> String {
    hex::encode(d)
}
