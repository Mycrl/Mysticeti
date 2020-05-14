use super::{State::Callback, State};
use super::message::{CONNECT, CREATE_STREAM, PUBLISH};
use rml_rtmp::chunk_io::{ChunkDeserializer, ChunkSerializer, Packet};
use rml_rtmp::{messages::RtmpMessage, time::RtmpTimestamp};
use rml_amf0::{Amf0Value, serialize};
use bytes::{BufMut, BytesMut};

/// Handle Rtmp session information
///
/// Decode Rtmp buffer and encode Rtmp data 
/// block to reply to the peer.
pub struct Session {
    decoder: ChunkDeserializer,
    encoder: ChunkSerializer,
    app: Option<String>,
    stream: Option<String>,
}

impl Session {
    /// Create Rtmp session information
    ///
    /// Handle Rtmp's session flow, including stream 
    /// push disconnection, permission control, etc.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use session::Session;
    ///
    /// Session::new();
    /// ```
    pub fn new() -> Self {
        Self {
            app: None,
            stream: None,
            decoder: ChunkDeserializer::new(),
            encoder: ChunkSerializer::new(),
        }
    }

    /// Processing Rtmp packets
    ///
    /// Decode the buffer and return the data that 
    /// needs to be returned to the peer.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use session::Session;
    /// use bytes::Bytes;
    ///
    /// let mut session = Session::new();
    /// session.process(Bytes::from(b""))
    /// ```
    pub fn process(&mut self, buffer: &[u8]) -> Option<Vec<State>> {
        let mut receiver = Vec::new();
        let mut is_first = true;

        // Get and process the message
        // Get the call result and push it to the return result list
        // Only deal with getting messages
        // If the acquisition fails, jump out of the loop, indicating 
        // that the current consumption has been completed.
        while let Some(message) = self.get_message(if is_first { buffer } else { &[] }) {
            is_first = false;
            if let Some(state) = self.process_message(message) {
                receiver.push(state);
            }
        }

        match &receiver.is_empty() {
            false => Some(receiver),
            true => None,
        }
    }

    /// Get messages
    ///
    /// Consume Tcp data and get Rtmp messages.
    /// Convert Result to Option.
    fn get_message(&mut self, chunk: &[u8]) -> Option<RtmpMessage> {
        if let Ok(Some(payload)) = self.decoder.get_next_message(chunk) {
            return payload.to_rtmp_message().ok();
        }

        None
    }

    /// Set the maximum shard size
    ///
    /// Set the maximum fragment size for decoding and encoding.
    /// The fragment size is the maximum length of the Rtmp message.
    ///
    /// TODO: 内部使用了unwrap来解决错误，应该使用其他方法;
    fn set_max_size(&mut self, size: u32) -> Option<State> {
        let timestamp = RtmpTimestamp { value: 0 };
        self.decoder.set_max_chunk_size(size as usize).unwrap();
        self.encoder.set_max_chunk_size(size, timestamp).unwrap();
        None
    }

    /// Handling connection events
    /// 
    /// Get the application name of the push stream and store it inside the instance.
    /// Return the subsequent return event package.
    fn connect_event(&mut self, object: Amf0Value) -> Option<State> {
        if let Amf0Value::Object(info) = object {
            if let Some(Amf0Value::Utf8String(app_name)) = info.get("app") {
                self.app = Some(app_name.to_string());
            }
        }

        self.from_message(CONNECT.to_vec())
    }

    /// Handling release stream events
    /// 
    /// Get the stream name of the push stream and store it inside the instance.
    fn release_stream_event(&mut self, args: Vec<Amf0Value>) -> Option<State> {
        if let Some(Amf0Value::Utf8String(stream_name)) = args.get(0) {
            self.stream = Some(stream_name.to_string());
        }

        None
    }

    /// Handling push events
    /// 
    /// Send push events to other business backends through Udp 
    /// to facilitate business backends to do push flow processing.
    fn publish_event(&mut self, args: Vec<Amf0Value>) -> Option<State> {
        if let Some(Amf0Value::Utf8String(stream_name)) = args.get(0) {
            let value = BytesMut::from(stream_name.as_str()).freeze();
            return Some(State::Event(value, super::Flag_Publish));
        }

        None
    }

    /// Handling stop push events
    /// 
    /// Send stop push events to other business backends through Udp 
    /// to facilitate business backends to do push flow processing.
    fn unpublish_event(&mut self, args: Vec<Amf0Value>) -> Option<State> {
        if let Some(Amf0Value::Utf8String(stream_name)) = args.get(0) {
            let value = BytesMut::from(stream_name.as_str()).freeze();
            return Some(State::Event(value, super::Flag_UnPublish));
        }

        None
    }

    /// Create Rtmp message
    ///
    /// Give Rtmp message result, serialize Rtmp message data.
    fn from(&mut self, message: RtmpMessage) -> Option<Packet> {
        let timestamp = RtmpTimestamp { value: 0 };
        if let Ok(payload) = message.into_message_payload(timestamp, 0) {
            if let Ok(packet) = self.encoder.serialize(&payload, false, false) {
                return Some(packet);
            }
        }

        None
    }

    /// Create Rtmp message data
    ///
    /// Usually, there is not necessarily only one Rtmp message for an 
    /// interaction, so multiple Rtmp messages are assembled here, 
    /// and merged and returned to the peer through TcpSocket.
    fn from_message(&mut self, messages: Vec<RtmpMessage>) -> Option<State> {
        let mut buffer = BytesMut::new();
        for message in messages {
            if let Some(packet) = self.from(message.clone()) {
                buffer.put(&packet.bytes[..]);
            }
        }

        match &buffer.is_empty() {
            false => Some(Callback(buffer.freeze())),
            true => None,
        }
    }
    
    /// Processing AMF data
    ///
    /// Here only to get the media wrapper information, 
    /// send the original data of the information to the business backend.
    fn process_data(&mut self, args: Vec<Amf0Value>) -> Option<State> {
        if let Some(Amf0Value::Utf8String(name)) = args.get(0) {
            if name.as_str() == "@setDataFrame" {
                if let Ok(vec) = serialize(&args) {
                    let value = BytesMut::from(&vec[16..]).freeze();
                    return Some(State::Event(value, super::Flag_Frame));
                }
            }
        }

        None
    }

    /// Handle Rtmp control messages
    ///
    /// Currently only the connection, create flow, and push flow events 
    /// are processed, because currently only basic messages are processed 
    /// to make push flow normal.
    ///
    /// TODO: 处理不健壮，而且没有将推流的session信息广播出去;
    fn process_command(
        &mut self,
        command: &str,
        object: Amf0Value,
        args: Vec<Amf0Value>,
    ) -> Option<State> {
        match command {
            "connect" => self.connect_event(object),
            "releaseStream" => self.release_stream_event(args),
            "FCPublish" => self.publish_event(args),
            "FCUnpublish" => self.unpublish_event(args),
            "createStream" => self.from_message(CREATE_STREAM.to_vec()),
            "publish" => self.from_message(PUBLISH.to_vec()),
            _ => None,
        }
    }

    /// Processing Rtmp messages
    ///
    /// Currently only deals with basic messages,
    /// Only to extract audio and video data.
    ///
    /// TODO: 健壮性问题;
    fn process_message(&mut self, message: RtmpMessage) -> Option<State> {
        match message {
            RtmpMessage::Amf0Command {
                command_name: n,
                command_object: o,
                additional_arguments: s, ..
            } => self.process_command(n.as_str(), o, s),
            RtmpMessage::SetChunkSize { size } => self.set_max_size(size),
            RtmpMessage::AudioData { data } => Some(State::Event(data, super::Flag_Audio)),
            RtmpMessage::VideoData { data } => Some(State::Event(data, super::Flag_Video)),
            RtmpMessage::Amf0Data { values } => self.process_data(values),
            _ => None,
        }
    }
}
