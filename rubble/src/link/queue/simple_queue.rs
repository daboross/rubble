use heapless::consts::U1;
use heapless::spsc::{self, MultiCore};

use super::{Consume, Consumer, PacketQueue, Producer};
use crate::link::data::{self, Llid};
use crate::link::{MIN_DATA_PAYLOAD_BUF, MIN_DATA_PDU_BUF};
use crate::{bytes::*, Error};

/// A simple packet queue that can hold a single packet.
///
/// This type is compatible with thumbv6 cores, which lack atomic operations that might be needed
/// for other queue implementations.
///
/// This queue also minimizes RAM usage: In addition to the raw buffer space, only minimal space is
/// needed for housekeeping.
pub struct SimpleQueue {
    inner: spsc::Queue<[u8; MIN_DATA_PDU_BUF], U1, u8, MultiCore>,
}

impl SimpleQueue {
    /// Creates a new, empty queue.
    pub const fn new() -> Self {
        Self {
            inner: spsc::Queue(heapless::i::Queue::u8()),
        }
    }
}

impl<'a> PacketQueue for &'a mut SimpleQueue {
    type Producer = SimpleProducer<'a>;

    type Consumer = SimpleConsumer<'a>;

    fn split(self) -> (Self::Producer, Self::Consumer) {
        let (p, c) = self.inner.split();
        (SimpleProducer { inner: p }, SimpleConsumer { inner: c })
    }
}

/// Producer (writer) half returned by `SimpleQueue::split`.
pub struct SimpleProducer<'a> {
    inner: spsc::Producer<'a, [u8; MIN_DATA_PDU_BUF], U1, u8, MultiCore>,
}

impl<'a> Producer for SimpleProducer<'a> {
    fn free_space(&self) -> u8 {
        // We can only have space for either 0 or 1 packets with min. payload size
        if self.inner.ready() {
            MIN_DATA_PAYLOAD_BUF as u8
        } else {
            0
        }
    }

    fn produce_dyn(
        &mut self,
        payload_bytes: u8,
        f: &mut dyn FnMut(&mut ByteWriter<'_>) -> Result<Llid, Error>,
    ) -> Result<(), Error> {
        assert!(usize::from(payload_bytes) <= MIN_DATA_PAYLOAD_BUF);

        if !self.inner.ready() {
            return Err(Error::Eof);
        }

        let mut buf = [0; MIN_DATA_PDU_BUF];
        let mut writer = ByteWriter::new(&mut buf[2..]);
        let free = writer.space_left();
        let llid = f(&mut writer)?;
        let used = free - writer.space_left();

        let mut header = data::Header::new(llid);
        header.set_payload_length(used as u8);
        header.to_bytes(&mut ByteWriter::new(&mut buf[..2]))?;

        self.inner.enqueue(buf).map_err(|_| ()).unwrap();
        Ok(())
    }
}

/// Consumer (reader) half returned by `SimpleQueue::split`.
pub struct SimpleConsumer<'a> {
    inner: spsc::Consumer<'a, [u8; MIN_DATA_PDU_BUF], U1, u8, MultiCore>,
}

impl<'a> Consumer for SimpleConsumer<'a> {
    fn has_data(&self) -> bool {
        self.inner.ready()
    }

    fn consume_raw_with<R>(
        &mut self,
        f: impl FnOnce(data::Header, &[u8]) -> Consume<R>,
    ) -> Result<R, Error> {
        if let Some(packet) = self.inner.peek() {
            let mut bytes = ByteReader::new(packet);
            let raw_header: [u8; 2] = bytes.read_array().unwrap();
            let header = data::Header::parse(&raw_header);
            let pl_len = usize::from(header.payload_length());
            let raw_payload = bytes.read_slice(pl_len)?;

            let res = f(header, raw_payload);
            if res.consume {
                self.inner.dequeue().unwrap(); // can't fail
            }
            res.result
        } else {
            Err(Error::Eof)
        }
    }
}

#[test]
fn simple_queue() {
    run_tests(&mut SimpleQueue::new());
}
