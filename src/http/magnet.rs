use std::borrow::Cow;

use anyhow::{bail, Context};
use reqwest::{StatusCode, Url};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::{
    decode::{decode_bencoded_value, DecodeContext},
    magnet::Magnet,
    torrent::TorrentInfo,
    utils::{BtError, BtResult},
};

use super::{HandshakeMessage, Peer, PeerInfo, PieceMessage, EXT_ID_MAP, PEER_ID, PORT};

use self::metadata::MessageType;

mod metadata {
    use anyhow::{bail, Context};
    use serde_json::json;

    use crate::{
        decode::{decode_bencoded_value, DecodeContext},
        encode::{encode_dictionary, EncodeContext},
        http::EXT_METADATA_ID,
        torrent::TorrentInfo,
    };

    /// The message id follows BitTorrent protocol.
    ///
    /// For message implemented by extension, the value is always 20.
    const MESSAGE_ID: u8 = 20;

    #[derive(Debug, PartialEq, Eq)]
    pub(super) enum MessageType {
        /// Requests a piece of metadata from the peer
        Request,

        /// Sends a piece of metadata to the peer
        Data,

        /// Signals that the peer doesn't have the piece of metadata that was requested
        Reject,
    }

    impl MessageType {
        const fn id(&self) -> u8 {
            match self {
                MessageType::Request => 0,
                MessageType::Data => 1,
                MessageType::Reject => 2,
            }
        }
    }

    impl TryFrom<u8> for MessageType {
        type Error = anyhow::Error;

        fn try_from(value: u8) -> Result<Self, Self::Error> {
            match value {
                0 => Ok(Self::Request),
                1 => Ok(Self::Data),
                2 => Ok(Self::Reject),
                v => bail!("invalid message type {v}"),
            }
        }
    }

    pub(super) struct Message {
        /// The id of metadata extension, received from the other peer.
        ext_id: u8,

        /// Type of the message.
        msg_type: MessageType,
    }

    impl Message {
        pub(super) fn new(ext_id: u8, msg_type: MessageType) -> Self {
            Self { ext_id, msg_type }
        }

        pub(super) fn to_bytes(&self) -> Vec<u8> {
            let mut buf = vec![];

            let dict = json!({
                "msg_type": self.msg_type.id(),
                "piece": 0,
            });

            let mut ctx = EncodeContext::new();
            encode_dictionary(&mut ctx, &dict.as_object().unwrap());
            let mut dict_bytes = ctx.consume();
            // Add length.
            // Length is 1(message id) + 1(extension message id) + dict_bytes.len()
            buf.extend((1 + 1 + dict_bytes.len() as u32).to_be_bytes());

            // Add message id.
            buf.push(MESSAGE_ID);

            // Add extension message id.
            buf.push(self.ext_id);

            // Add bencoded dictionay
            buf.append(&mut dict_bytes);

            buf
        }

        /// Build a [`Message::Data`] from bytes with known length.
        ///
        /// * `length` is the length of `data`.
        /// * `data` contains all data we need to build a `Message` and the length should be `length`.
        pub(super) fn parse_torrent_data(length: u32, data: &[u8]) -> anyhow::Result<TorrentInfo> {
            let length = length as usize;
            if length < 3 {
                bail!("too short message response, len={length}");
            }

            if data.len() != length {
                bail!(
                    "invalid bytes length, declared as {}, but got {}",
                    length,
                    data.len()
                );
            }

            if data[0] != 20 {
                bail!("expected extension piece message, got id {}", data[0])
            }
            let ext_id = data[1];
            if ext_id as usize != EXT_METADATA_ID {
                bail!("invalid metadata extension id, we have {EXT_METADATA_ID} but peer responded {ext_id}")
            }
            // In the response message, two dictionaries are joined together:
            //
            // * Info dictionary contains "msg_type", "piece", and "total_size" where "total_size"
            //   is the length of second dictionary.
            // * Metadata piece contents bencoded into dictionary.
            //   * Tip in stage ns5: If metadata is larger than 16kb, you would need to request multiple
            //     pieces, but for the purposes of this challenge there will only be one piece.
            let mut tmp_buf = Vec::with_capacity(length - 2 + 2);
            tmp_buf.push(b'l');
            tmp_buf.append(&mut data[2..length].to_vec());
            tmp_buf.push(b'e');
            let mut ctx = DecodeContext::new(tmp_buf);
            let data_value =
                decode_bencoded_value(&mut ctx).context("failed to decode response")?;
            let content_list = data_value.as_array().context("response is not an array")?;
            // Info dictionary of metadata.
            let info_dict = content_list[0]
                .as_object()
                .context("response is not a map")?;
            let msg_type = MessageType::try_from(
                info_dict
                    .get("msg_type")
                    .context("msg_type not found")?
                    .as_number()
                    .context("invalid msg_type")?
                    .as_u64()
                    .unwrap() as u8,
            )?;
            // Skip validating peice.
            // Skip validating total_size: in the challenge metadata is small enough to send by only one piece.
            if msg_type != MessageType::Data {
                bail!("invalid data message id, got {:?}", msg_type)
            }

            let torrent_info: TorrentInfo = serde_json::from_value(content_list[1].to_owned())
                .context("invalid torrent info")?;

            Ok(torrent_info)
        }
    }
}

pub struct MagnetHandshakeResult {
    pub message: HandshakeMessage,
    pub ut_metadata_id: u32,
    pub torrent_info: Option<TorrentInfo>,
}

/// Connect a single peer.
async fn connect_peer(
    peer: &Peer,
    info_hash: [u8; 20],
    request_metadata: bool,
) -> BtResult<MagnetHandshakeResult> {
    /* Handshake */

    let message = HandshakeMessage::with_ext(
        info_hash,
        PEER_ID.as_bytes().try_into().unwrap(),
        [0, 0, 0, 0, 0, 0x10, 0, 0],
    );

    println!(">>> handshake: ip={}, port={}", peer.ip, peer.port);
    let handshake_message_bytes = message.to_bytes();
    // println!(">>> handshake request: {:?}", handshake_message_bytes);

    let mut socket = TcpStream::connect(format!("{}:{}", peer.ip, peer.port).as_str())
        .await
        .context("failed to dial")?;
    let (mut rd, mut wr) = socket.split();
    if let Err(e) = wr.write_all(&handshake_message_bytes).await {
        bail!("failed to send handshake message: {e}")
    }

    // Tempoary buffer.
    let mut buf = [0u8; 2048];

    let mut handshake_buf = vec![0u8; HandshakeMessage::length()];
    rd.read_exact(&mut handshake_buf).await?;
    // Here we ignore the handshake returned.
    let handshake_resp =
        HandshakeMessage::from_bytes(&handshake_buf).context("invalid resp message format")?;

    // println!(">>> wait for bitfield");

    /* Wait for Bitfield */

    let mut bitfield_buf = [0u8; 5];
    let n = rd.read_exact(&mut bitfield_buf).await?;
    if n == 0 {
        bail!("empty bitfield message");
    }
    // Read the payload of bitfield so the reader is clean.
    let l = u32::from_be_bytes([
        bitfield_buf[0],
        bitfield_buf[1],
        bitfield_buf[2],
        bitfield_buf[3],
    ]) - 1;
    let mut tmp_buf = vec![0u8; l as usize];
    rd.read_exact(&mut tmp_buf).await?;

    match PieceMessage::from_bytes(&bitfield_buf)? {
        PieceMessage::Bitfield => { /* Expected bitfield message */ }
        v => bail!("invalid bitfield message: id={}", v.id()),
    }

    // Only do the extension handshake if peer support.
    if !handshake_resp.has_ext() {
        bail!("peer does not support extension");
    }

    let bytes = PieceMessage::new_extension(&EXT_ID_MAP).to_bytes();
    println!(">>> [ext] start handshake: {:?}", &bytes);
    wr.write(&bytes)
        .await
        .context("failed to send extension message")?;
    println!(">>> [ext] waiting response");
    // Read the extension handshake response.
    let n = rd.read(&mut buf).await?;
    println!(">>> [ext] finish handshake, got: {:?}", &buf[0..n]);
    match PieceMessage::from_bytes(&buf[0..n])? {
        PieceMessage::Extension { extensions } => {
            let mut ctx = DecodeContext::new(extensions[1..].to_vec());
            let v = decode_bencoded_value(&mut ctx)
                .context("failed to decode handshake response from bencode")?;
            let outer_dict = v.as_object().unwrap();
            let inner_dict = outer_dict.get("m").unwrap().as_object().unwrap();
            let ut_metadata_id = inner_dict
                .get("ut_metadata")
                .and_then(|x| x.as_i64())
                .context("invalid ut_metadata id")? as u8;
            let torrent_info;
            if request_metadata {
                println!(">>> [ext] send metadata request message");
                let req = metadata::Message::new(ut_metadata_id, MessageType::Request);
                let req_bytes = req.to_bytes();
                println!(">>> [ext] request: {:?}", req_bytes);
                wr.write(&req_bytes)
                    .await
                    .context("failed to send metadata request")?;
                let resp_len = rd
                    .read_u32()
                    .await
                    .context("failed to read response length")?;
                let mut resp_buf = vec![0u8; resp_len as usize];
                rd.read_exact(&mut resp_buf)
                    .await
                    .context("failed to read response")?;
                torrent_info = Some(metadata::Message::parse_torrent_data(
                    resp_len,
                    resp_buf.as_slice(),
                )?);
            } else {
                torrent_info = None;
            }
            Ok(MagnetHandshakeResult {
                message: handshake_resp,
                ut_metadata_id: ut_metadata_id as u32,
                torrent_info,
            })
        }
        v => bail!(">>> [ext] unexpected handshake message id={}", v.id()),
    }
}

/// Magnet handshake queries peer info from tracker and handshake with peer to get peer id.
pub(super) async fn handshake(
    magnet: &Magnet,
    request_metadata: bool,
) -> BtResult<MagnetHandshakeResult> {
    let mut tracker_url = match &magnet.tracker_url {
        Some(v) => Url::parse(v).context("invalid url")?,
        None => bail!("tracker url not provided"),
    };

    println!(">>> magnet handshake: tracker={}", tracker_url);
    tracker_url
        .query_pairs_mut()
        .encoding_override(Some(&|input| {
            // Ref: https://app.codecrafters.io/courses/bittorrent/stages/fi9
            if input == "{{info_hash}}" {
                Cow::Owned(magnet.info_hash.to_vec())
            } else {
                Cow::Borrowed(input.as_bytes())
            }
        }))
        .append_pair("info_hash", "{{info_hash}}")
        .append_pair("uploaded", "0")
        .append_pair("downloaded", "0")
        .append_pair("left", "1")
        .append_pair("compact", "1")
        .append_pair("peer_id", PEER_ID)
        .append_pair("port", PORT)
        .finish();

    let resp = reqwest::get(tracker_url)
        .await
        .context("http request failed")?;
    if resp.status() != StatusCode::OK {
        bail!(BtError::NetworkError(resp.status().as_u16()))
    }

    let peer_info = resp
        .bytes()
        .await
        .context("invalid resp data")
        .and_then(|data| {
            decode_bencoded_value(&mut DecodeContext::new(data.as_ref().to_vec()))
                .context("bencode decode failed")
        })
        .and_then(|value| {
            serde_json::from_value::<PeerInfo>(value).context("failed to deserialize peer info")
        })?;

    let peer = &peer_info.peers[0];
    let resp = connect_peer(peer, magnet.info_hash, request_metadata)
        .await
        .context("peer handshake failed")?;
    Ok(resp)
}
