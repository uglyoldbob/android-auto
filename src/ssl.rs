//! SSL code

use tokio::io::{AsyncRead, AsyncWrite};

use crate::{
    AndroidAutoControlMessage, AndroidAutoFrame, AndroidAutoFrameReceiver, FrameAssembler,
    FrameHeaderReceiver, FrameReceiptError, FrameTransmissionError, SendableAndroidAutoMessage,
};

/// A message sent to the ssl thread
pub enum SslThreadData {
    /// The handshake is starting
    HandshakeStart,
    /// Data to send out for handshake process
    HandshakeData(Vec<u8>),
    /// A message to write to the writer
    PlainData(SendableAndroidAutoMessage),
    /// A frame to write to the writer
    Frame(AndroidAutoFrame),
    /// A message to decrypt
    DecryptMe(AndroidAutoFrame),
}

/// The response from the ssl thread
pub enum SslThreadResponse {
    /// A decrypted frame received from the read object
    Data(AndroidAutoFrame),
    /// The handshake is complete
    HandshakeComplete,
    /// The ssl thread is exiting with an error
    ExitError(String),
}

struct SslStreamThread<U: AsyncWrite + Unpin> {
    stream: rustls::client::ClientConnection,
    hs_started: bool,
    hs_completed: bool,
    hs: Option<tokio::sync::mpsc::Receiver<SslThreadData>>,
    dout: tokio::sync::mpsc::UnboundedSender<SslThreadResponse>,
    write: U,
    /// Reassembles decrypted fragments, per channel
    assembler: FrameAssembler,
}

impl<U: AsyncWrite + Unpin> SslStreamThread<U> {
    fn new(
        rcv: tokio::sync::mpsc::Receiver<SslThreadData>,
        dout: tokio::sync::mpsc::UnboundedSender<SslThreadResponse>,
        conn: rustls::client::ClientConnection,
        write: U,
    ) -> Self {
        Self {
            stream: conn,
            hs_started: false,
            hs_completed: false,
            hs: Some(rcv),
            dout,
            write,
            assembler: FrameAssembler::new(),
        }
    }

    async fn handle_receive(&mut self, m: SslThreadData) -> Result<(), String> {
        match m {
            SslThreadData::DecryptMe(mut data) => {
                if let Err(e) = data.decrypt(&mut self.stream).await {
                    log::error!("Error receiving frame: {:?}", e);
                    return Err(format!("frame error {:?}", e));
                }
                if let Some(f) = self.assembler.push(data) {
                    let _ = self.dout.send(SslThreadResponse::Data(f));
                }
            }
            SslThreadData::HandshakeStart => {
                if self.hs_started {
                    unimplemented!();
                } else {
                    let mut buf = Vec::new();
                    self.stream
                        .write_tls(&mut buf)
                        .map_err(|e| format!("write_tls: {e}"))?;
                    {
                        use tokio::io::AsyncWriteExt;
                        let f: AndroidAutoFrame =
                            AndroidAutoControlMessage::SslHandshake(buf).into();
                        let d2: Vec<u8> = f
                            .build_vec(Some(&mut self.stream))
                            .await
                            .map_err(|e| format!("{:?}", e))?;
                        self.write
                            .write_all(&d2)
                            .await
                            .map_err(|e| match e.kind() {
                                std::io::ErrorKind::TimedOut => "write timed out".to_string(),
                                std::io::ErrorKind::UnexpectedEof => {
                                    "write disconnected".to_string()
                                }
                                _ => format!("write error: {e}"),
                            })?;
                        let _ = self.write.flush().await;
                        self.hs_started = true;
                    }
                }
            }
            SslThreadData::HandshakeData(data) => {
                let mut dc = std::io::Cursor::new(data);
                self.stream
                    .read_tls(&mut dc)
                    .map_err(|e| format!("read_tls: {e}"))?;
                let state = self
                    .stream
                    .process_new_packets()
                    .map_err(|e| format!("{:?}", e))?;

                if state.peer_has_closed() {
                    return Err("peer closed connection during handshake".to_string());
                }
                if !self.stream.is_handshaking() && !self.hs_completed {
                    self.hs_completed = true;
                    self.dout
                        .send(SslThreadResponse::HandshakeComplete)
                        .map_err(|e| e.to_string())?;
                }

                if self.stream.wants_write() {
                    use tokio::io::AsyncWriteExt;
                    let mut s = Vec::new();
                    self.stream
                        .write_tls(&mut s)
                        .map_err(|e| format!("write_tls: {e}"))?;
                    {
                        let f: AndroidAutoFrame = AndroidAutoControlMessage::SslHandshake(s).into();
                        let d2: Vec<u8> = f
                            .build_vec(Some(&mut self.stream))
                            .await
                            .map_err(|e| format!("{:?}", e))?;
                        self.write
                            .write_all(&d2)
                            .await
                            .map_err(|e| match e.kind() {
                                std::io::ErrorKind::TimedOut => "Timed out".to_string(),
                                std::io::ErrorKind::UnexpectedEof => "Disconnected".to_string(),
                                _ => format!("write error: {e}"),
                            })?;
                        let _ = self.write.flush().await;
                    }
                }
            }
            SslThreadData::PlainData(f) => {
                use tokio::io::AsyncWriteExt;
                let d2: Vec<u8> = f
                    .into_frame()
                    .await
                    .build_vec(Some(&mut self.stream))
                    .await
                    .map_err(|e| format!("{:?}", e))?;
                let a = self.write.write_all(&d2).await.map_err(|e| match e.kind() {
                    std::io::ErrorKind::TimedOut => FrameTransmissionError::Timeout,
                    std::io::ErrorKind::UnexpectedEof => FrameTransmissionError::Disconnected,
                    _ => FrameTransmissionError::Unexpected(e),
                });
                let _ = self.write.flush().await;
                a.map_err(|e| format!("{:?}", e))?;
            }
            SslThreadData::Frame(f) => {
                use tokio::io::AsyncWriteExt;
                let d2: Vec<u8> = f
                    .build_vec(Some(&mut self.stream))
                    .await
                    .map_err(|e| format!("{:?}", e))?;
                let a = self.write.write_all(&d2).await.map_err(|e| match e.kind() {
                    std::io::ErrorKind::TimedOut => FrameTransmissionError::Timeout,
                    std::io::ErrorKind::UnexpectedEof => FrameTransmissionError::Disconnected,
                    _ => FrameTransmissionError::Unexpected(e),
                });
                let _ = self.write.flush().await;
                a.map_err(|e| format!("{:?}", e))?;
            }
        }
        Ok(())
    }

    async fn run(mut self) -> Result<(), String> {
        let mut hs = self
            .hs
            .take()
            .expect("SslStreamThread::run called without receiver");
        loop {
            match hs.recv().await {
                Some(m) => {
                    if let Err(e) = self.handle_receive(m).await {
                        let _ = self.dout.send(SslThreadResponse::ExitError(e.to_string()));
                        return Err(e);
                    }
                }
                None => {
                    return Ok(());
                }
            }
        }
    }
}

pub struct StreamMux {
    send: tokio::sync::mpsc::Sender<SslThreadData>,
    recv: tokio::sync::mpsc::UnboundedReceiver<SslThreadResponse>,
}

pub struct ReadHalf {
    recv: tokio::sync::mpsc::UnboundedReceiver<SslThreadResponse>,
}

#[derive(Clone)]
pub struct WriteHalf {
    send: tokio::sync::mpsc::Sender<SslThreadData>,
}

impl WriteHalf {
    pub async fn write_message(
        &self,
        m: SendableAndroidAutoMessage,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<SslThreadData>> {
        self.send.send(SslThreadData::PlainData(m)).await
    }

    pub async fn write_frame(
        &self,
        f: AndroidAutoFrame,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<SslThreadData>> {
        self.send.send(SslThreadData::Frame(f)).await
    }

    pub async fn start_handshake(
        &self,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<SslThreadData>> {
        self.send.send(SslThreadData::HandshakeStart).await
    }

    pub async fn do_handshake(
        &self,
        data: Vec<u8>,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<SslThreadData>> {
        self.send.send(SslThreadData::HandshakeData(data)).await
    }
}

impl ReadHalf {
    pub async fn recv(&mut self) -> Option<SslThreadResponse> {
        self.recv.recv().await
    }
}

impl StreamMux {
    pub fn new<T: AsyncRead + Send + Unpin + 'static, U: AsyncWrite + Send + Unpin + 'static>(
        conn: rustls::client::ClientConnection,
        write: U,
        mut read: T,
    ) -> Self {
        // `chan` carries both inbound frames waiting to be decrypted and outbound frames
        // waiting to be encrypted; the consumer of `chan2` writes its replies (acks) back into
        // `chan`. With both channels bounded, sustained traffic in both directions (audio out
        // to the head unit while the microphone streams in) fills both and the three tasks
        // wait on each other forever. Making the TLS->consumer leg unbounded guarantees the
        // TLS task can always drain `chan`, which breaks the cycle; the shared queue is also
        // deepened so bursts of small frames do not stall the writer.
        let chan = tokio::sync::mpsc::channel(256);
        let chan2 = tokio::sync::mpsc::unbounded_channel();
        let chanw = chan2.0.clone();
        let stream = SslStreamThread::new(chan.1, chan2.0, conn, write);
        tokio::spawn(stream.run());
        let chan_ssl = chan.0.clone();
        tokio::spawn(async move {
            let mut fr = AndroidAutoFrameReceiver::new();
            let mut plain = FrameAssembler::new();
            loop {
                let mut fhr = FrameHeaderReceiver::new();
                if let Ok(Some(fh)) = fhr.read(&mut read).await {
                    if let Ok(Some(f)) = fr.read(&fh, &mut read).await {
                        if f.header.frame.get_encryption() {
                            chan_ssl.send(SslThreadData::DecryptMe(f)).await;
                        } else if let Some(f) = plain.push(f) {
                            let _ = chanw.send(SslThreadResponse::Data(f));
                        }
                    }
                }
            }
        });
        Self {
            send: chan.0,
            recv: chan2.1,
        }
    }

    pub fn split(self) -> (ReadHalf, WriteHalf) {
        (ReadHalf { recv: self.recv }, WriteHalf { send: self.send })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct AcceptAll;

    impl rustls::client::danger::ServerCertVerifier for AcceptAll {
        fn verify_server_cert(
            &self,
            _: &rustls::pki_types::CertificateDer<'_>,
            _: &[rustls::pki_types::CertificateDer<'_>],
            _: &rustls::pki_types::ServerName<'_>,
            _: &[u8],
            _: rustls::pki_types::UnixTime,
        ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self,
            _: &[u8],
            _: &rustls::pki_types::CertificateDer<'_>,
            _: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }
        fn verify_tls13_signature(
            &self,
            _: &[u8],
            _: &rustls::pki_types::CertificateDer<'_>,
            _: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }
        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            rustls::crypto::ring::default_provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }

    /// A handshaken client (the head unit) and server (standing in for the phone), in memory.
    fn tls_pair() -> (rustls::ClientConnection, rustls::ServerConnection) {
        use rustls::pki_types::pem::PemObject;
        let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
        let cert =
            rustls::pki_types::CertificateDer::from_pem_slice(crate::cert::CERTIFICATE.as_bytes())
                .unwrap();
        let key =
            rustls::pki_types::PrivateKeyDer::from_pem_slice(crate::cert::PRIVATE_KEY.as_bytes())
                .unwrap();
        let scfg = rustls::ServerConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .unwrap();
        let ccfg = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(std::sync::Arc::new(AcceptAll))
            .with_no_client_auth();
        let mut c = rustls::ClientConnection::new(
            std::sync::Arc::new(ccfg),
            "idontknow.com".try_into().unwrap(),
        )
        .unwrap();
        let mut s = rustls::ServerConnection::new(std::sync::Arc::new(scfg)).unwrap();
        for _ in 0..10 {
            let mut buf = Vec::new();
            c.write_tls(&mut buf).unwrap();
            s.read_tls(&mut &buf[..]).unwrap();
            s.process_new_packets().unwrap();
            let mut buf = Vec::new();
            s.write_tls(&mut buf).unwrap();
            c.read_tls(&mut &buf[..]).unwrap();
            c.process_new_packets().unwrap();
            if !c.is_handshaking() && !s.is_handshaking() {
                return (c, s);
            }
        }
        panic!("handshake did not complete");
    }

    /// One wire frame as the phone sends it: each fragment's payload is its own TLS output.
    fn wire_frame(
        s: &mut rustls::ServerConnection,
        ch: u8,
        ftype: u8,
        total: u32,
        plain: &[u8],
    ) -> Vec<u8> {
        use std::io::Write;
        s.writer().write_all(plain).unwrap();
        let mut ct = Vec::new();
        s.write_tls(&mut ct).unwrap();
        let mut f = vec![ch, 0x08 | ftype];
        f.extend_from_slice(&(ct.len() as u16).to_be_bytes());
        if ftype == 1 {
            f.extend_from_slice(&total.to_be_bytes());
        }
        f.extend_from_slice(&ct);
        f
    }

    #[test]
    fn a_frame_from_another_channel_inside_a_multi_frame_message_decrypts() {
        // Seen on the desk 2026-09-26: a 35-byte mic ack on channel 7 arrived between the
        // First and Last fragments of a video message on channel 3, and the next decrypt
        // failed with DecryptError, ending the session. TLS records carry an implicit
        // sequence number, so they must reach rustls in the order they arrived.
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let (client, mut server) = tls_pair();
        let video_a = vec![0xAAu8; 3000];
        let video_b = vec![0xBBu8; 1200];
        let ack = vec![0x07u8; 12];
        let total = (video_a.len() + video_b.len()) as u32;
        let mut wire = Vec::new();
        wire.extend(wire_frame(&mut server, 3, 1, total, &video_a));
        wire.extend(wire_frame(&mut server, 7, 3, 0, &ack));
        wire.extend(wire_frame(&mut server, 3, 2, 0, &video_b));
        let got = rt.block_on(async move {
            use tokio::io::AsyncWriteExt;
            // A pipe that stays open, as a connected device does: no end-of-file to handle.
            let (mut phone, hu_read) = tokio::io::duplex(1 << 20);
            phone.write_all(&wire).await.unwrap();
            let (mut rx, _tx) = StreamMux::new(client, tokio::io::sink(), hu_read).split();
            let mut frames = Vec::new();
            let mut exits = Vec::new();
            while let Ok(Some(r)) =
                tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv()).await
            {
                match r {
                    SslThreadResponse::Data(f) => frames.push((f.header.channel_id, f.data)),
                    SslThreadResponse::ExitError(e) => exits.push(e),
                    SslThreadResponse::HandshakeComplete => {}
                }
            }
            drop(phone);
            (frames, exits)
        });
        rt.shutdown_background();
        let (frames, exits) = got;
        assert!(
            !exits.iter().any(|e| e.contains("frame error")),
            "session ended on a decrypt: {exits:?}"
        );
        let mut video = video_a.clone();
        video.extend(&video_b);
        assert_eq!(
            frames.len(),
            2,
            "expected the ack and the reassembled video"
        );
        assert!(frames.contains(&(7, ack)), "mic ack lost or corrupted");
        assert!(
            frames.contains(&(3, video)),
            "video message lost or corrupted"
        );
    }

    fn plain(ch: u8, ftype: crate::FrameHeaderType, data: &[u8]) -> AndroidAutoFrame {
        let mut h = crate::FrameHeader {
            channel_id: ch,
            frame: crate::FrameHeaderContents::new(false, crate::FrameHeaderType::Single, false),
        };
        h.frame.set_frame_type(ftype);
        AndroidAutoFrame {
            header: h,
            data: data.to_vec(),
        }
    }

    #[test]
    fn interleaved_multi_frame_messages_on_two_channels_stay_separate() {
        use crate::FrameHeaderType::*;
        let mut a = FrameAssembler::new();
        assert!(a.push(plain(3, First, b"v1")).is_none());
        assert!(a.push(plain(5, First, b"s1")).is_none());
        assert!(a.push(plain(3, Middle, b"v2")).is_none());
        let s = a.push(plain(5, Last, b"s2")).expect("speech message");
        assert_eq!((s.header.channel_id, s.data), (5, b"s1s2".to_vec()));
        let v = a.push(plain(3, Last, b"v3")).expect("video message");
        assert_eq!((v.header.channel_id, v.data), (3, b"v1v2v3".to_vec()));
    }
}
