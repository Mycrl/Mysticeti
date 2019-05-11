// use.
use bytes::BytesMut;
use std::sync::mpsc::channel;
use std::sync::mpsc::Sender;
use std::sync::mpsc::Receiver;
use crate::pool::Pool;
use crate::pool::CacheBytes;


/// # Media Data Transmission Interface.
#[derive(Clone)]
pub struct Matedata {
    pub name: String,
    pub key: String,
    pub value: CacheBytes
}


pub struct Crated {
    pub name: String,
    pub key: String 
}


pub enum DataType {
    Matedata(Matedata),
    BytesMut(BytesMut),
    Crated(Crated)
}


pub struct Channel {
    pub tx: Sender<BytesMut>,
    pub rx: Receiver<BytesMut>
}


/// # Flow Distributor.
pub struct Distributor {
    pub pool: Pool,
    pub channel: Channel
}


/// # Interface implemented for the encoder.
/// All encoders must implement the same interface, the same behavior.
pub trait Codec {
    fn new (address: String, sender: Sender<BytesMut>) -> Self;
    fn decoder (&mut self, bytes: BytesMut) -> ();
}


impl Distributor {

    /// # Create distributor.
    pub fn new () -> Self {
        let pool = Pool::new();
        let (tx, rx) = channel();
        Distributor { pool, channel: Channel { tx, rx } }
    }
}