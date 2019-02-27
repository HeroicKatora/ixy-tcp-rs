use ixy::{IxyDevice, memory::Packet};
use smoltcp::Result as NetResult;
use smoltcp::time::Instant;
use smoltcp::phy;

/// A generic ixy device as a smoltcp phy device.
///
/// Newtype wrapper so that this struct can live in an external crate instead of ixy-rs itself.
pub struct Phy<D>(D);

impl<D> Phy<D> {
    pub fn inner(&self) -> &D {
        &self.0
    }

    pub fn into_inner(self) -> D {
        self.0
    }
}

impl<D: IxyDevice> From<D> for Phy<D> {
    fn from(device: D) -> Self {
        Phy(device)
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
        unimplemented!()
    }
}

impl phy::RxToken for RxToken {
    fn consume<R, F>(self, ts: Instant, f: F) -> NetResult<R>
        where F: FnOnce(&[u8]) -> NetResult<R>
    {
        unimplemented!()
    }
}

impl phy::TxToken for TxToken {
    fn consume<R, F>(self, ts: Instant, length: usize, f: F) -> NetResult<R>
        where F: FnOnce(&mut [u8]) -> NetResult<R>
    {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
