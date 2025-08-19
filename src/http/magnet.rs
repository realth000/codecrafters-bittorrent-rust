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
    utils::{BtError, BtResult},
};

use super::{HandshakeMessage, Peer, PeerInfo, PieceMessage, EXT_ID_MAP, PEER_ID, PORT};

/// Connect a single peer.
async fn connect_peer(peer: &Peer, info_hash: [u8; 20]) -> BtResult<HandshakeMessage> {
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

    let n = rd.read(&mut buf).await?;
    if n == 0 {
        bail!("empty bitfield message");
    }

    match PieceMessage::from_bytes(&buf[0..n])? {
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
    return Ok(handshake_resp);
}

/// Magnet handshake queries peer info from tracker and handshake with peer to get peer id.
pub(super) async fn handshake(magnet: &Magnet) -> BtResult<HandshakeMessage> {
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
    let resp = connect_peer(peer, magnet.info_hash)
        .await
        .context("peer handshake failed")?;
    Ok(resp)
}
