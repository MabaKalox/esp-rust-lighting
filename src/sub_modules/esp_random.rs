use esp_idf_sys::{esp_fill_random, esp_random};
use rand::RngCore;

pub struct EspRand {}

impl RngCore for EspRand {
    fn next_u32(&mut self) -> u32 {
        unsafe { esp_random() }
    }

    fn next_u64(&mut self) -> u64 {
        (self.next_u32() as u64) | (self.next_u32() as u64) << 32
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        unsafe { esp_fill_random(dest.as_mut_ptr() as *mut std::ffi::c_void, dest.len()) }
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}
