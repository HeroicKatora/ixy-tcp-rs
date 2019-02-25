use ixy::{IxyDevice, Packet};

/// A generic ixy device as a smoltcp phy device.
///
/// Newtype wrapper so that this struct can live in an external crate instead of ixy-rs itself.
pub struct Phy<D: IxyDevice>(D);

impl<D: IxyDevice> Phy<D> {
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

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
