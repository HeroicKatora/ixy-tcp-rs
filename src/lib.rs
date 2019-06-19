use std::collections::{VecDeque, vec_deque::IterMut};
use std::rc::Rc;

use ixy::IxyDevice;
use ixy::memory::{self, Mempool, Packet as IxyPacket};

use ethox::layer::Result as NicResult;
use ethox::nic;
use ethox::wire;
use ethox::time::Instant;

/// A generic ixy device as an ethox phy device.
///
/// Newtype wrapper so that this struct can live in an external crate instead of ixy-rs itself.
pub struct Phy<D> {
    /// The underlying device.
    device: D,

    /// Packets to be processed in receive.
    rx_queue: VecDeque<IxyPacket>,

    /// Packets which can be used for sending.
    tx_empty: VecDeque<IxyPacket>,

    /// Packets ready for sending but waiting to be batched.
    tx_queue: VecDeque<IxyPacket>,

    /// Memory pool to use for allocation.
    pool: Rc<Mempool>,
}

#[derive(Clone, Copy, Debug)]
pub struct Handle {
    queued: bool,
    timestamp: Instant,
}

#[repr(transparent)]
pub struct Packet(IxyPacket);

impl<D> Phy<D> {
    const BATCH_SIZE: usize = 32;

    pub fn new(device: D, pool: Rc<Mempool>) -> Self where D: IxyDevice {
        Phy {
            device,
            rx_queue: VecDeque::with_capacity(Self::BATCH_SIZE),
            tx_empty: VecDeque::with_capacity(Self::BATCH_SIZE),
            tx_queue: VecDeque::with_capacity(Self::BATCH_SIZE),
            pool,
        }
    }

    /// Inspect the inner device.
    ///
    /// Useful to gather the stats or link metadata.
    pub fn ixy(&self) -> &D {
        &self.device
    }

    pub fn into_inner(self) -> D {
        self.device
    }
}

impl<D: IxyDevice> Phy<D> {
    /// Empty the send buffer.
    ///
    /// The network stack of `smoltcp` only gives an interface for sending single packets. In order
    /// to do efficient batching, packets are buffered internally until they have reached the
    /// specific size. Just call this periodically, e.g. each loop iteration.
    ///
    /// Returns the number of packets sent due to this call to flush.
    pub fn flush(&mut self) -> usize {
        self.device.tx_batch(0, &mut self.tx_queue)
    }

    fn get_rx(&mut self) -> IterMut<IxyPacket> {
        if self.rx_queue.is_empty() {
            self.device.rx_batch(0, &mut self.rx_queue, Self::BATCH_SIZE);
        }

        // Receive in correct time order.
        self.rx_queue.iter_mut()
    }

    fn get_tx(&mut self) -> IterMut<IxyPacket> {
        if self.tx_empty.is_empty() {
            let max_size = self.pool.entry_size();
            memory::alloc_pkt_batch(&self.pool, &mut self.tx_empty, Self::BATCH_SIZE, max_size);
        }

        // Back is the last sent packet, best chance to still be in TLB/mmio cache?
        self.tx_empty.iter_mut()
    }
}

impl Handle {
    fn new(now: Instant) -> Self {
        Handle {
            queued: false,
            timestamp: now,
        }
    }
}

impl Packet {
    fn from_mut(ixy: &mut IxyPacket) -> &mut Self {
        // Safety: marked with `repr(transparent)`. Doesn't change mutability.
        unsafe { core::mem::transmute(ixy) }
    }
}

impl<D: IxyDevice> nic::Device for Phy<D> {
    type Handle = Handle;
    type Payload = Packet;

    fn personality(&self) -> nic::Personality {
        nic::Personality::baseline()
    }

    fn tx(&mut self, max: usize, mut sender: impl nic::Send<Self::Handle, Self::Payload>)
        -> NicResult<usize>
    {
        let now = Instant::now();
        let mut handles = [Handle::new(now); 32];
        
        // Provide packets to the sender.
        let packets = self
            .get_tx()
            .zip(handles.iter_mut())
            .map(|(packet, handle)| {
                nic::Packet {
                    handle,
                    payload: Packet::from_mut(packet),
                }
            })
            .take(max);

        let count = packets.len();
        sender.sendv(packets);

        // Gather potentially sent and step through those that were marked as sent.
        let tx_queue = &mut self.tx_queue;
        let sent = self.tx_empty
            .drain(..count)
            .zip(handles.iter())
            .fold(0, |count, (packet, handle)| {
                count + if handle.queued {
                    tx_queue.push_back(packet);
                    1
                } else {
                    // Drops packet
                    0
                }
            });
        self.flush();
        Ok(sent)
    }

    fn rx(&mut self, max: usize, mut receptor: impl nic::Recv<Self::Handle, Self::Payload>)
        -> NicResult<usize>
    {
        let now = Instant::now();
        let mut handles = [Handle::new(now); 32];

        // Provide packets to the receiver.
        let packets = self
            .get_rx()
            .zip(handles.iter_mut())
            .map(|(packet, handle)| {
                nic::Packet {
                    handle,
                    payload: Packet::from_mut(packet),
                }
            })
            .take(max);
        let count = packets.len();
        receptor.receivev(packets);

        // Gather those sent again immediately
        let tx_queue = &mut self.tx_queue;
        let sent = self.tx_empty
            .drain(..count)
            .zip(handles.iter())
            .fold(0, |count, (packet, handle)| {
                count + if handle.queued {
                    tx_queue.push_back(packet);
                    1
                } else {
                    // Drops packet
                    0
                }
            });
        self.flush();
        Ok(sent)

    }
}

impl nic::Handle for Handle {
    fn queue(&mut self) -> NicResult<()> {
        Ok(self.queued = true)
    }

    fn info(&self) -> &nic::Info {
        self
    }
}

impl nic::Info for Handle {
    fn timestamp(&self) -> Instant {
       self.timestamp 
    }

    fn capabilities(&self) -> nic::Capabilities {
        nic::Capabilities::no_support()
    }
}

impl wire::Payload for Packet {
    fn payload(&self) -> &wire::payload {
        self.0.as_ref().into()
    }
}
