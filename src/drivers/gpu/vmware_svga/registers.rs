//! VMware SVGA II register indices from svga_reg.h.
pub const ID: u32 = 0;
pub const MAX_WIDTH: u32 = 4;
pub const MAX_HEIGHT: u32 = 5;
pub const FB_OFFSET: u32 = 14;
pub const VRAM_SIZE: u32 = 15;
pub const FB_SIZE: u32 = 16;
pub const CAPABILITIES: u32 = 17;
pub const MEM_SIZE: u32 = 19;
pub const SVGA_ID_0: u32 = 0x9000_0000;
pub const SVGA_ID_1: u32 = 0x9000_0001;
pub const SVGA_ID_2: u32 = 0x9000_0002;
pub const fn known_id(id: u32) -> bool {
    matches!(id, SVGA_ID_0 | SVGA_ID_1 | SVGA_ID_2)
}
