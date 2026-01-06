use embassy_usb::driver::{Driver, EndpointError};
use embassy_usb::Builder;
use embassy_usb::driver::{Endpoint, EndpointIn, EndpointOut};

const USB_CLASS_VENDOR: u8 = 0xff;
const USB_SUBCLASS_NONE: u8 = 0x00;
const USB_PROTOCOL_NONE: u8 = 0x00;

pub struct VendorClass<'d, D: Driver<'d>> {
    read_ep: D::EndpointOut,
    write_ep: D::EndpointIn,
    max_packet_size: u16,
}

impl<'d, D: Driver<'d>> VendorClass<'d, D> {
    pub fn new(builder: &mut Builder<'d, D>, max_packet_size: u16) -> Self {
        let mut function = builder.function(USB_CLASS_VENDOR, USB_SUBCLASS_NONE, USB_PROTOCOL_NONE);
        let mut interface = function.interface();
        let mut alt = interface.alt_setting(USB_CLASS_VENDOR, USB_SUBCLASS_NONE, USB_PROTOCOL_NONE, None);
        let read_ep = alt.endpoint_bulk_out(None, max_packet_size);
        let write_ep = alt.endpoint_bulk_in(None, max_packet_size);
        drop(function);

        Self {
            read_ep,
            write_ep,
            max_packet_size,
        }
    }

    pub fn split(self) -> (VendorSender<'d, D>, VendorReceiver<'d, D>) {
        let sender = VendorSender {
            write_ep: self.write_ep,
            max_packet_size: self.max_packet_size,
        };
        let receiver = VendorReceiver {
            read_ep: self.read_ep,
            max_packet_size: self.max_packet_size,
        };
        (sender, receiver)
    }
}

pub struct VendorReceiver<'d, D: Driver<'d>> {
    read_ep: D::EndpointOut,
    max_packet_size: u16,
}

impl<'d, D: Driver<'d>> VendorReceiver<'d, D> {
    pub async fn wait_connection(&mut self) {
        self.read_ep.wait_enabled().await;
    }

    pub async fn read_packet(&mut self, data: &mut [u8]) -> Result<usize, EndpointError> {
        let mut n = 0;
        loop {
            // BUG: This whole function is kinda wack
            let Some(buf) = data.get_mut(n..) else {
                return Ok(n);
            };
            let i = self.read_ep.read(buf).await?;
            n = n.saturating_add(i);
            if i < self.max_packet_size as usize {
                return Ok(n);
            }
        }
    }
}

pub struct VendorSender<'d, D: Driver<'d>> {
    write_ep: D::EndpointIn,
    max_packet_size: u16,
}

impl<'d, D: Driver<'d>> VendorSender<'d, D> {
    pub async fn wait_connection(&mut self) {
        self.write_ep.wait_enabled().await;
    }

    pub async fn write_packet(&mut self, data: &[u8]) -> Result<(), EndpointError> {
        for chunk in data.chunks(self.max_packet_size as usize) {
            self.write_ep.write(chunk).await?;
        }
        if data.len().is_multiple_of(self.max_packet_size as usize) {
            self.write_ep.write(&[]).await?;
        }
        Ok(())
    }
}
