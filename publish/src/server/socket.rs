use super::{transport::Transport, Tx};
use crate::codec::{Codec, Packet};
use bytes::{Bytes, BytesMut};
use futures::prelude::*;
use std::task::{Context, Poll};
use std::{marker::Unpin, pin::Pin};
use tokio::io::{AsyncRead, AsyncWrite, Error};
use tokio::net::TcpStream;

/// TcpSocket instance
///
/// Read and write TcpSocket and return data through channel.
/// The returned data is a Udp data packet. In order to adapt to MTU, 
/// the subcontracting has been completed.
pub struct Socket<T> {
    transport: Transport,
    stream: TcpStream,
    dgram: Tx,
    codec: T,
}

impl<T: Default + Codec + Unpin> Socket<T> {
    /// Create a TcpSocket instance
    ///
    /// To create an instance, you need to specify a `Codec` as the data codec.
    /// `Codec` processes Tcp data and asks for the returned Tcp data and Udp packet.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::error::Error;
    /// use tokio::net::TcpListener;
    /// use socket::Socket;
    /// use rtmp::Rtmp;
    ///
    /// async fn main() -> Result<(), Box<dyn Error>> {
    ///     let addr = "0.0.0.0:1935".parse().unwrap();
    ///     let mut listener = TcpListener::bind(&addr).await?;
    ///     
    ///     loop {
    ///         let (stream, _) = listener.accept().await?;
    ///         tokio::spawn(Socket::<Rtmp>::new(stream));
    ///     }
    /// }
    /// ```
    pub fn new(stream: TcpStream, dgram: Tx) -> Self {
        Self {
            dgram,
            stream,
            codec: T::default(),
            transport: Transport::new(1000),
        }
    }

    /// Push messages to channel
    ///
    /// Push the Udp package to the channel.
    /// The other end needs to send data to the remote UdpServer.
    ///
    /// TODO: 异常处理未完善, 未处理意外情况，可能会出现死循环;
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use bytes::Bytes;
    /// use std::error::Error;
    /// use futures::future::poll_fn;
    /// use tokio::net::TcpListener;
    /// use socket::Socket;
    /// use rtmp::Rtmp;
    ///
    /// async fn main() -> Result<(), Box<dyn Error>> {
    ///     let addr = "0.0.0.0:1935".parse().unwrap();
    ///     let mut listener = TcpListener::bind(&addr).await?;
    ///
    ///     loop {
    ///         let (stream, _) = listener.accept().await?;
    ///         let socket = Socket::<Rtmp>::new(stream);
    ///
    ///         poll_fn(|cx| {
    ///             socket.push(cx, Bytes::from(b"hello"));
    ///         });
    ///     }
    /// }
    /// ```
    #[rustfmt::skip]
    pub fn push(&mut self, data: Bytes, flgs: u8) {
        for chunk in self.transport.packet(data, flgs) {
            loop {
                match self.dgram.send(chunk.clone()) {
                    Ok(_) => { break; },
                    _ => (),
                }
            }
        }
    }

    /// Send data to TcpSocket
    ///
    /// Write Tcp data to TcpSocket.
    /// Check whether the writing is completed, 
    // if not completely written, write the rest.
    ///
    /// TODO: 异常处理未完善, 未处理意外情况，可能会出现死循环;
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::error::Error;
    /// use futures::future::poll_fn;
    /// use tokio::net::TcpListener;
    /// use socket::Socket;
    /// use rtmp::Rtmp;
    ///
    /// async fn main() -> Result<(), Box<dyn Error>> {
    ///     let addr = "0.0.0.0:1935".parse().unwrap();
    ///     let mut listener = TcpListener::bind(&addr).await?;
    ///
    ///     loop {
    ///         let (stream, _) = listener.accept().await?;
    ///         let socket = Socket::<Rtmp>::new(stream);
    ///
    ///         poll_fn(|cx| {
    ///             socket.send(cx, &[0, 1, 2]);
    ///         });
    ///     }
    /// }
    /// ```
    #[rustfmt::skip]
    pub fn send<'b>(&mut self, ctx: &mut Context<'b>, data: &[u8]) {
        let mut offset: usize = 0;
        let length = data.len();
        loop {
            match Pin::new(&mut self.stream).poll_write(ctx, &data) {
                Poll::Ready(Ok(s)) => match &offset + &s >= length {
                    false => { offset += s; },
                    true => { break; }
                }, _ => (),
            }
        }
    }

    /// Read data from TcpSocket
    ///
    /// TODO: 目前存在重复申请缓冲区的情况，有优化空间；
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::error::Error;
    /// use futures::future::poll_fn;
    /// use tokio::net::TcpListener;
    /// use socket::Socket;
    /// use rtmp::Rtmp;
    ///
    /// async fn main() -> Result<(), Box<dyn Error>> {
    ///     let addr = "0.0.0.0:1935".parse().unwrap();
    ///     let mut listener = TcpListener::bind(&addr).await?;
    ///
    ///     loop {
    ///         let (stream, _) = listener.accept().await?;
    ///         let socket = Socket::<Rtmp>::new(stream);
    ///
    ///         poll_fn(|cx| {
    ///             socket.read(cx);
    ///         });
    ///     }
    /// }
    /// ```
    #[rustfmt::skip]
    pub fn read<'b>(&mut self, ctx: &mut Context<'b>) -> Option<BytesMut> {
        let mut receiver = [0u8; 2048];
        match Pin::new(&mut self.stream).poll_read(ctx, &mut receiver) {
            Poll::Ready(Ok(s)) if s > 0 => Some(BytesMut::from(&receiver[0..s])),
            _ => None,
        }
    }

    /// Refresh the TcpSocket buffer
    ///
    /// After writing data to TcpSocket, you need to refresh 
    /// the buffer and send the data to the peer.
    ///
    /// TODO: 异常处理未完善, 未处理意外情况，可能会出现死循环;
    ///s
    /// # Examples
    ///
    /// ```no_run
    /// use std::error::Error;
    /// use futures::future::poll_fn;
    /// use tokio::net::TcpListener;
    /// use socket::Socket;
    /// use rtmp::Rtmp;
    ///
    /// async fn main() -> Result<(), Box<dyn Error>> {
    ///     let addr = "0.0.0.0:1935".parse().unwrap();
    ///     let mut listener = TcpListener::bind(&addr).await?;
    ///
    ///     loop {
    ///         let (stream, _) = listener.accept().await?;
    ///         let socket = Socket::<Rtmp>::new(stream);
    ///
    ///         poll_fn(|cx| {
    ///             socket.read(cx, 128);
    ///             socket.flush(cx);
    ///         });
    ///     }
    /// }
    /// ```
    #[rustfmt::skip]
    pub fn flush<'b>(&mut self, ctx: &mut Context<'b>) {
        loop {
            match Pin::new(&mut self.stream).poll_flush(ctx) {
                Poll::Ready(Ok(_)) => { break; },
                _ => (),
            }
        }
    }

    /// Try to process TcpSocket data
    ///
    /// Use `Codec` to handle TcpSocket data,
    /// Write the returned data to TcpSocket or UdpSocket correctly.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::error::Error;
    /// use futures::future::poll_fn;
    /// use tokio::net::TcpListener;
    /// use socket::Socket;
    /// use rtmp::Rtmp;
    ///
    /// async fn main() -> Result<(), Box<dyn Error>> {
    ///     let addr = "0.0.0.0:1935".parse().unwrap();
    ///     let mut listener = TcpListener::bind(&addr).await?;
    ///     
    ///     loop {
    ///         let (stream, _) = listener.accept().await?;
    ///         let socket = Socket::<Rtmp>::new(stream);
    ///
    ///         poll_fn(|cx| {
    ///             socket.process(cx);
    ///         });
    ///     }
    /// }
    /// ```
    pub fn process<'b>(&mut self, ctx: &mut Context<'b>) {
        while let Some(mut chunk) = self.read(ctx) {
            for packet in self.codec.parse(&mut chunk) {
                match packet {
                    Packet::Tcp(data) => self.send(ctx, &data),
                    Packet::Udp(data, flgs) => self.push(data, flgs),
                }
            }

            // Refresh the TcpSocket buffer. In order to increase efficiency, 
            // all the returned data of the current task will be written and 
            // then refreshed in a unified manner to avoid unnecessary frequent operations.
            self.flush(ctx);
        }
    }
}

impl<T: Default + Codec + Unpin> Future for Socket<T> {
    type Output = Result<(), Error>;
    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        self.get_mut().process(ctx);
        Poll::Pending
    }
}
