use std::collections::VecDeque;
use std::rc::Rc;

use ixy::IxyDevice;
use ixy::memory::{self, Mempool, Packet};

use smoltcp::{Error as NetError, Result as NetResult};
use smoltcp::time::Instant;
use smoltcp::phy;

/// A generic ixy device as a smoltcp phy device.
///
/// Newtype wrapper so that this struct can live in an external crate instead of ixy-rs itself.
pub struct Phy<D> {
    /// The underlying device.
    device: D,

    /// Packets to be processed in receive.
    rx_queue: VecDeque<Packet>,

    /// Packets which can be used for sending.
    tx_empty: VecDeque<Packet>,

    /// Packets ready for sending but waiting to be batched.
    tx_queue: VecDeque<Packet>,

    /// Memory pool to use for allocation.
    pool: Rc<Mempool>,
}

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
    pub fn inner(&self) -> &D {
        &self.device
    }

    pub fn into_inner(self) -> D {
        self.device
    }

    /// Empty the send buffer.
    ///
    /// The network stack of `smoltcp` only gives an interface for sending single packets. In order
    /// to do efficient batching, packets are buffered internally until they have reached the
    /// specific size. Just call this periodically, e.g. each loop iteration.
    ///
    /// Returns the number of packets sent due to this call to flush.
    pub fn flush(&mut self) -> usize where D: IxyDevice {
        self.device.tx_batch(0, &mut self.tx_queue)
    }

    fn rx(&mut self) -> Option<Packet> where D: IxyDevice {
        if self.rx_queue.is_empty() {
            self.device.rx_batch(0, &mut self.rx_queue, Self::BATCH_SIZE);
        }

        // Receive in correct time order.
        self.rx_queue.pop_front()
    }

    fn unrx(&mut self, packet: Packet) {
        self.rx_queue.push_front(packet)
    }

    fn tx(&mut self) -> Option<Packet> where D: IxyDevice {
        if self.tx_empty.is_empty() {
            let max_size = self.pool.entry_size();
            memory::alloc_pkt_batch(&self.pool, &mut self.tx_empty, Self::BATCH_SIZE, max_size);
        }

        // Back is the last sent packet, best chance to still be in TLB/mmio cache?
        self.tx_empty.pop_back()
    }

    fn untx(&mut self, packet: Packet) {
        assert!(packet.len() == self.pool.entry_size());
        self.tx_empty.push_back(packet)
    }
}

/// Private trait implementing batched sending.
///
/// Used by `TxToken` as an abstraction so that it does not require the type implementing
/// `IxyDevice` in its interface.
trait Sender {
    /// Add a packet for sending.
    ///
    /// It might not send it immediately to batch multiple calls.
    fn enqueue(&mut self, packet: Packet) -> NetResult<()>;
}

pub struct RxToken {
    packet: Packet,
}

pub struct TxToken<'a> {
    packet: Packet,
    queue: &'a mut Sender,
}

impl<'a, D: IxyDevice> phy::Device<'a> for Phy<D> {
    type RxToken = RxToken;
    type TxToken = TxToken<'a>;

    fn receive(&'a mut self) -> Option<(RxToken, TxToken)> {
        match (self.rx(), self.tx()) {
            (Some(rx), Some(tx)) => {
                Some((RxToken::from(rx), TxToken::from(tx, self)))
            },
            (Some(rx), None) => {
                self.unrx(rx);
                None
            },
            (None, Some(tx)) => {
                self.untx(tx);
                None
            },
            (None, None) => None,
        }
    }

    fn transmit(&'a mut self) -> Option<TxToken> {
        match self.tx() {
            None => None,
            Some(tx) => Some(TxToken::from(tx, self))
        }
    }

    fn capabilities(&self) -> phy::DeviceCapabilities {
        let mut capabilities = phy::DeviceCapabilities::default();
        capabilities.checksum.udp = phy::Checksum::None;
        capabilities.max_transmission_unit = self.pool.entry_size();
        // FIXME: no idea what this exactly does. May need to return the allocation size of the
        // buffer here.
        capabilities.max_burst_size = None;
        capabilities
    }
}

impl<'a, D: IxyDevice> Sender for Phy<D> {
    fn enqueue(&mut self, packet: Packet) -> NetResult<()> {
        self.tx_queue.push_back(packet);
        if self.tx_queue.len() >= Self::BATCH_SIZE {
            self.flush();
        }
        Ok(())
    }
}

impl RxToken {
    /// Create an rx token.
    ///
    /// Not public through `convert::From` as it should only be created by `Phy`.
    pub(crate) fn from(packet: Packet) -> Self {
        RxToken { packet }
    }
}

impl<'a> TxToken<'a> {
    /// Create a tx token.
    ///
    /// Not public through `convert::From` as it should only be created by `Phy` and we may have
    /// additional invariants.
    pub(crate) fn from(packet: Packet, queue: &'a mut Sender) -> Self {
        TxToken { packet, queue }
    }
}

impl phy::RxToken for RxToken {
    fn consume<R, F>(self, _ts: Instant, f: F) -> NetResult<R>
        where F: FnOnce(&[u8]) -> NetResult<R>
    {
        f(&self.packet)
    }
}

impl<'a> phy::TxToken for TxToken<'a> {
    fn consume<R, F>(mut self, _ts: Instant, length: usize, f: F) -> NetResult<R>
        where F: FnOnce(&mut [u8]) -> NetResult<R>
    {
        if self.packet.len() <= length {
            // Assume that the packet was chosen as long as possible.  This needs to change if we
            // allow using a received packet directly but the packet allocator makes them as long
            // as possible, I think.
            return Err(NetError::Illegal)
        }

        // resize the packet to the requested size.
        self.packet.truncate(length);

        // TODO: evaluate if we should initialize memory in `packet` before. Currently, it may
        // still contain the contents of a previous packet (but not actually uninitialized
        // content, still may be a security vulnerability as this basically bypasses the borrow
        // checker as a custom allocator).
        assert!(self.packet.len() == length);
        let r = f(&mut self.packet[..])?;

        self.queue.enqueue(self.packet).map(move |_| r)
    }
}

#[cfg(test)]
mod tests {
}
