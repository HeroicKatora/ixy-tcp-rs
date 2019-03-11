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

    /// Memory pool to use for allocation.
    pool: Rc<Mempool>,
}

impl<D> Phy<D> {
    const BATCH_SIZE: usize = 32;

    pub fn new(device: D, pool: Rc<Mempool>) -> Self {
        Phy {
            device,
            rx_queue: VecDeque::with_capacity(Self::BATCH_SIZE),
            tx_empty: VecDeque::with_capacity(Self::BATCH_SIZE),
            pool,
        }
    }

    pub fn inner(&self) -> &D {
        &self.device
    }

    pub fn into_inner(self) -> D {
        self.device
    }
}

pub struct RxToken {
    packet: Packet,
}

pub struct TxToken {
    packet: Packet,
}

impl<'a, D: IxyDevice> phy::Device<'a> for Phy<D> {
    type RxToken = RxToken;
    type TxToken = TxToken;

    fn receive(&'a mut self) -> Option<(RxToken, TxToken)> {
        unimplemented!()
    }

    fn transmit(&'a mut self) -> Option<TxToken> {
        unimplemented!()
    }

    fn capabilities(&self) -> phy::DeviceCapabilities {
        let mut capabilities = phy::DeviceCapabilities::default();
        capabilities.max_transmission_unit = self.pool.entry_size();
        // FIXME: no idea what this exactly does. May need to return the allocation size of the
        // buffer here.
        capabilities.max_burst_size = None;
        capabilities
    }
}

impl phy::RxToken for RxToken {
    fn consume<R, F>(self, _ts: Instant, f: F) -> NetResult<R>
        where F: FnOnce(&[u8]) -> NetResult<R>
    {
        f(&self.packet)
    }
}

impl phy::TxToken for TxToken {
    fn consume<R, F>(mut self, _ts: Instant, length: usize, f: F) -> NetResult<R>
        where F: FnOnce(&mut [u8]) -> NetResult<R>
    {
        if self.packet.len() <= length {
            // Assume that the packet was chosen as long as possible.  This needs to change if we
            // allow using a received packet directly but the packet allocator makes them as long
            // as possible, I think.
            return Err(NetError::Illegal)
        }

        f(&mut self.packet[..length])
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
