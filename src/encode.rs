use crate::utils::decode_bytes_from_string;

pub struct EncodeContext {
    data: Vec<u8>,
}

impl EncodeContext {
    pub fn new() -> Self {
        Self { data: vec![] }
    }

    fn push_char(&mut self, v: char) {
        self.data.push(v as u8);
    }

    fn push_usize(&mut self, v: usize) {
        let s = v.to_string();
        let chars = s.chars();
        let mut x = vec![];
        for ch in chars.into_iter().rev() {
            x.insert(0, ch as u8);
        }

        self.data.append(&mut x);
    }

    fn append(&mut self, mut data: Vec<u8>) {
        self.data.append(&mut data);
    }

    pub fn data(&self) -> &Vec<u8> {
        &self.data
    }

    pub fn consume(self) -> Vec<u8> {
        self.data
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
pub fn encode_dictionary(ctx: &mut EncodeContext, v: &serde_json::Map<String, serde_json::Value>) {
    ctx.push_char('d');
    for (k, v) in v.iter() {
        encode_string(ctx, k);
        if ["pieces", "peers"].contains(&k.as_str()) {
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
