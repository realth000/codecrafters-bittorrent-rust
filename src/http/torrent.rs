use std::sync::{Arc, Mutex};

use anyhow::{bail, Context};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::utils::{parallel_future, BtResult};

use super::{HandshakeMessage, Peer, Peers, PieceMessage, PEER_ID};

/// Setup connections with all available peers.
pub(super) async fn setup_connection(
    peers: &Peers,
    info_hash: &[u8; 20],
) -> BtResult<Vec<Arc<Mutex<TcpStream>>>> {
    let conns = parallel_future(peers.iter(), 3, |peer| {
        connect_peer(&peer, info_hash.clone())
    })
    .await
    .context("failed to setup peer connections")?
    .into_iter()
    .map(|conn| Arc::new(Mutex::new(conn)))
    .collect::<Vec<_>>();

    Ok(conns)
}

/// Connect a single peer.
async fn connect_peer(peer: &Peer, info_hash: [u8; 20]) -> BtResult<TcpStream> {
    /* Handshake */

    let message = HandshakeMessage::new(info_hash, PEER_ID.as_bytes().try_into().unwrap());

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
    let _ = HandshakeMessage::from_bytes(&handshake_buf).context("invalid resp message format")?;

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

    // println!(">>> send interested");

    /* Send Interested */

    wr.write(&PieceMessage::new_interested().to_bytes())
        .await
        .context("failed to write interested message")?;

    // println!(">>> waiting unchoke");

    /* Wait for Unchoke */

    let n = rd.read(&mut buf).await?;
    if n == 0 {
        bail!(" empty unchoke message");
    }

    match PieceMessage::from_bytes(&buf[0..n])? {
        PieceMessage::Unchoke => { /* Expected unchoke message */ }
        v => bail!("invalid unchoke message: id={}", v.id()),
    }

    Ok(socket)
}
