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
    dout: tokio::sync::mpsc::Sender<SslThreadResponse>,
    write: U,
}

impl<U: AsyncWrite + Unpin> SslStreamThread<U> {
    fn new(
        rcv: tokio::sync::mpsc::Receiver<SslThreadData>,
        dout: tokio::sync::mpsc::Sender<SslThreadResponse>,
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
                data.decrypt(&mut self.stream).await;
                self.dout.send(SslThreadResponse::Data(data)).await;
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
                        .await
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
                            .send(SslThreadResponse::ExitError(e.to_string()))
                            .await;
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
    recv: tokio::sync::mpsc::Receiver<SslThreadResponse>,
}

pub struct ReadHalf {
    recv: tokio::sync::mpsc::Receiver<SslThreadResponse>,
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
        let chan = tokio::sync::mpsc::channel(15);
        let chan2 = tokio::sync::mpsc::channel(15);
        let chanw = chan2.0.clone();
        let stream = SslStreamThread::new(chan.1, chan2.0, conn, write);
        tokio::spawn(stream.run());
        let chan_ssl = chan.0.clone();
        tokio::spawn(async move {
            loop {
                let mut fhr = FrameHeaderReceiver::new();
                if let Ok(Some(fh)) = fhr.read(&mut read).await {
                    let mut fr = AndroidAutoFrameReceiver::new();
                    if let Ok(Some(f)) = fr.read(&fh, &mut read).await {
                        if f.header.frame.get_encryption() {
                            chan_ssl.send(SslThreadData::DecryptMe(f)).await;
                        } else {
                            chanw.send(SslThreadResponse::Data(f)).await;
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
