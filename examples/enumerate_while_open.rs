//! Read-only release check: opens the first USB DualSense or Edge and lists
//! controllers again while that handle is open. Sends no reports.
//! Run with `cargo run --locked --example enumerate_while_open`.
use hidapi::{BusType, HidApi};
fn sony_usb(api: &HidApi) -> Vec<String> {
    api.device_list()
        .filter(|d| {
            d.vendor_id() == 0x054c
                && matches!(d.product_id(), 0x0ce6 | 0x0df2)
                && matches!(d.bus_type(), BusType::Usb)
        })
        .map(|d| d.path().to_string_lossy().into_owned())
        .collect()
}
fn main() {
    let api = HidApi::new().unwrap();
    let before = sony_usb(&api);
    println!("before open: {} USB DualSense interface(s)", before.len());
    let Some(path) = before.first() else { return };
    let c = std::ffi::CString::new(path.clone()).unwrap();
    let dev = api.open_path(&c).expect("open");
    let api2 = HidApi::new().unwrap();
    let during = sony_usb(&api2);
    println!(
        "while open: {} listed; opened path still listed: {}",
        during.len(),
        during.contains(path)
    );
    drop(dev);
}
