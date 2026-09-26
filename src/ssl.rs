//! SSL code

use tokio::io::{AsyncRead, AsyncWrite};

use crate::{
    AndroidAutoControlMessage, AndroidAutoFrame, AndroidAutoFrameReceiver, FrameHeaderReceiver,
    FrameReceiptError, FrameTransmissionError, SendableAndroidAutoMessage,
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
        }
    }

    async fn handle_receive(&mut self, m: SslThreadData) -> Result<(), String> {
        match m {
            SslThreadData::DecryptMe(mut data) => {
                if let Err(e) = data.decrypt(&mut self.stream).await {
                    log::error!("Error receiving frame: {:?}", e);
                    return Err(format!("frame error {:?}", e));
                }
                let _ = self.dout.send(SslThreadResponse::Data(data));
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
                        let _ = self
                            .dout
                            .send(SslThreadResponse::ExitError(e.to_string()));
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

/// Reads frames from the transport until it fails, routing encrypted frames to the ssl thread
/// and plain frames straight to the consumer.
///
/// A read error ends the pump. nusb keeps a failed transfer's error in its read buffer, so once
/// the device is unplugged every later read returns that same error without waiting; retrying
/// it spins a core forever and keeps the ssl thread alive through the senders held here.
async fn pump_frames<T: AsyncRead + Unpin>(
    mut read: T,
    chan_ssl: tokio::sync::mpsc::Sender<SslThreadData>,
    chanw: tokio::sync::mpsc::UnboundedSender<SslThreadResponse>,
) {
    let mut fr = AndroidAutoFrameReceiver::new();
    loop {
        let mut fhr = FrameHeaderReceiver::new();
        let result = match fhr.read(&mut read).await {
            Ok(Some(fh)) => fr.read(&fh, &mut read).await,
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        };
        match result {
            Ok(Some(f)) => {
                if f.header.frame.get_encryption() {
                    if chan_ssl.send(SslThreadData::DecryptMe(f)).await.is_err() {
                        return;
                    }
                } else if chanw.send(SslThreadResponse::Data(f)).is_err() {
                    return;
                }
            }
            Ok(None) => {}
            Err(e) => {
                log::error!("Error reading frame: {:?}", e);
                let _ = chanw.send(SslThreadResponse::ExitError(format!("read error {:?}", e)));
                return;
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
        tokio::spawn(pump_frames(read, chan_ssl, chanw));
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

    /// Fails every read immediately, as nusb's `EndpointRead` does once the device is gone.
    struct UnpluggedEndpoint {
        reads: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl AsyncRead for UnpluggedEndpoint {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            _buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            self.reads.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            std::task::Poll::Ready(Err(std::io::Error::other("device disconnected")))
        }
    }

    #[test]
    fn pump_ends_the_session_when_the_transport_fails() {
        // Built by hand so a pump that never yields cannot hang the test in runtime shutdown.
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let reads = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let endpoint = UnpluggedEndpoint {
            reads: reads.clone(),
        };
        let (ssl_tx, _ssl_rx) = tokio::sync::mpsc::channel(1);
        let (resp_tx, mut resp_rx) = tokio::sync::mpsc::unbounded_channel();
        let outcome = rt.block_on(async move {
            let pump = tokio::spawn(pump_frames(endpoint, ssl_tx, resp_tx));
            let ended = tokio::time::timeout(std::time::Duration::from_secs(2), pump).await;
            (ended.is_ok(), resp_rx.try_recv())
        });
        rt.shutdown_background();
        let reads = reads.load(std::sync::atomic::Ordering::Relaxed);
        assert!(outcome.0, "pump still running after {reads} failed reads");
        assert_eq!(reads, 1, "pump retried a failed transport");
        assert!(
            matches!(outcome.1, Ok(SslThreadResponse::ExitError(_))),
            "consumer was not told the session ended"
        );
    }
}
