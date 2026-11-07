//! Early ZillaOS GUI renderer over Limine's validated RGB32 framebuffer.
//! This draws directly into boot framebuffer memory (single-core demo only).
//! Production GUI must run in a user-space compositor after process isolation.

use core::cell::UnsafeCell;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use crate::limine::{self, Framebuffer};
use crate::wallpaper_asset;
use zillaos_kernel::desktop::{App, Desktop, Layout, Pointer};
use zillaos_kernel::locale::{text as tr, TextId};
use zillaos_kernel::fs::{self, NodeKind};
use zillaos_kernel::apps;
use zillaos_kernel::apps::notepad::EditorStatus;
use zillaos_kernel::fs::ramfs::DOCUMENTS_DIR;
use zillaos_kernel::shell::{start_menu, taskbar, task_view, status_bar};
use zillaos_kernel::shell::personalization::{
    self, Wallpaper, SettingsPage, GalleryRow, ProfileButton,
};
use zillaos_kernel::raster::{self, Region};
use zillaos_kernel::setup::{Wizard, Step, SetupPointer, Region as SetupRegion, Language};
use zillaos_kernel::window_manager::{Rect, WindowMode};

// Fixed-capacity offscreen RGB32 surface: 1920x1080x4 = 8,294,400 bytes.
// It resides in the kernel's BSS, NEVER on the stack or a heap.
// Frame rows use the actual visible GOP width as their stride, not 1920.
// Only modes larger than 1920x1080 fall back to direct drawing.
struct Backbuffer(UnsafeCell<[u32; raster::BACKBUFFER_PIXELS]>);

// SAFETY: the preemptive scheduler/IRQ handler/SMP renderer does not exist.
// Exactly one bootstrap CPU accesses this static through Screen methods.
unsafe impl Sync for Backbuffer {}
static BACKBUFFER: Backbuffer = Backbuffer(UnsafeCell::new([0; raster::BACKBUFFER_PIXELS]));
static BACKBUFFER_VALID: AtomicBool = AtomicBool::new(false);
static BACKBUFFER_WIDTH: AtomicUsize = AtomicUsize::new(0);
static BACKBUFFER_HEIGHT: AtomicUsize = AtomicUsize::new(0);

struct Screen {
    fb: Framebuffer,
    buffered: bool,
}

impl Screen {
    #[inline]
    fn rgb(&self, rgb: u32) -> u32 {
        (((rgb >> 16) & 0xff) << self.fb.red_shift)
            | (((rgb >> 8) & 0xff) << self.fb.green_shift)
            | ((rgb & 0xff) << self.fb.blue_shift)
    }

    #[inline]
    fn rectangle(&self, x: usize, y: usize, width: usize, height: usize, rgb: u32) {
        let Some(region) = Region::new(x, y, width, height)
            .clipped(self.fb.width, self.fb.height) else { return };
        let pixel = self.rgb(rgb);
        if self.buffered {
            // SAFETY: a single early-boot CPU is the sole reader/writer of
            // this static. The resolution fits 1280x800, and Region::clipped
            // guarantees each accessed span is in [0, BACKBUFFER_PIXELS).
            let buffer = unsafe { &mut *BACKBUFFER.0.get() };
            raster::fill_rect(
                buffer,
                self.fb.width,
                self.fb.width,
                self.fb.height,
                region,
                pixel,
            );
        } else {
            self.write_direct(region, pixel);
        }
    }

    /// Write pixels to the firmware-mapped display, bypassing the backing
    /// layer. Reserved for flush and drawing the hardware-visible cursor.
    fn write_direct(&self, region: Region, color: u32) {
        let Some(region) = region.clipped(self.fb.width, self.fb.height) else { return };
        for row in region.y..region.y + region.height {
            for col in region.x..region.x + region.width {
                let byte_offset = row * self.fb.pitch + col * 4;
                // SAFETY: framebuffer() validates pixel stride, alignment
                // and mapped bounds. Region stays inside width and height.
                unsafe {
                    ptr::write_volatile(self.fb.address.add(byte_offset).cast::<u32>(), color);
                }
            }
        }
    }

    /// Transfer previously composed background pixels from the BSS buffer
    /// into a clipped framebuffer area. Also restores cursor damage.
    fn flush_region(&self, region: Region) {
        if !self.buffered { return; }
        let Some(region) = region.clipped(self.fb.width, self.fb.height) else { return };
        // SAFETY: render/flush/cursor calls never run concurrently; bounded
        // Region indices are within the statically allocated backing buffer.
        let buffer = unsafe { &*BACKBUFFER.0.get() };
        for row in region.y..region.y + region.height {
            // Backing rows are tightly packed for the current GOP mode.
            // This avoids unused padded columns and keeps the 1600x900
            // framebuffer layout independent of the 1920-wide capacity.
            let start = row * self.fb.width + region.x;
            for col in 0..region.width {
                let color = buffer[start + col];
                let byte_offset = row * self.fb.pitch + (region.x + col) * 4;
                // SAFETY: firmware RGB32 surface was validated by Limine.
                unsafe {
                    ptr::write_volatile(self.fb.address.add(byte_offset).cast::<u32>(), color);
                }
            }
        }
    }


    fn round_rect(&self, x: usize, y: usize, w: usize, h: usize, radius: usize, rgb: u32) {
        if w == 0 || h == 0 {
            return;
        }
        let r = radius.min(w / 2).min(h / 2);
        for row in 0..h {
            let inset = if row < r {
                (r - row) / 2
            } else if row >= h - r {
                (row - (h - r) + 1) / 2
            } else {
                0
            };
            self.rectangle(x.saturating_add(inset), y.saturating_add(row),
                           w.saturating_sub(2 * inset), 1, rgb);
        }
    }

    fn decimal(&self, number: usize, x: usize, y: usize, rgb: u32) {
        let mut chars = [b'0'; 20];
        let mut n = number;
        let mut pos = chars.len();
        loop {
            pos -= 1;
            chars[pos] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 { break; }
        }
        if let Ok(value) = core::str::from_utf8(&chars[pos..]) {
            self.text(value, x, y, 1, rgb);
        }
    }

    fn text(&self, text: &str, x: usize, y: usize, scale: usize, rgb: u32) {
        if scale == 0 {
            return;
        }
        let mut px = x;
        for ch in text.chars() {
            if px.saturating_add(5 * scale) > self.fb.width {
                break;
            }
            let upper = ch.to_uppercase().next().unwrap_or(ch);
            let glyph = glyph_unicode(upper);
            for (row, bits) in glyph.iter().enumerate() {
                for column in 0..5 {
                    if (*bits & (1u8 << (4 - column))) != 0 {
                        self.rectangle(px + column * scale, y + row * scale,
                                       scale, scale, rgb);
                    }
                }
            }
            px = px.saturating_add(6 * scale);
        }
    }


    /// Guided first-boot experience. All labels are rendered by the kernel's
    /// tiny Unicode bitmap alphabet, not external fonts or assets.
    fn setup_screen(&self, wizard: &Wizard) {
        let g = wizard.geometry();
        let light = !wizard.dark;
        let fg = if light { 0x192f45 } else { 0xe9f4ff };
        let muted = if light { 0x59768b } else { 0x90acc3 };
        let surface = if light { 0xf8fbff } else { 0x17273e };
        let soft = if light { 0xe9f2f9 } else { 0x223850 };
        let accent = 0x1ed7b6;
        // Animated-free gradient keeps first boot deterministic in the VM.
        for y in 0..self.fb.height {
            let v = y * 96 / self.fb.height.max(1);
            let bg = if light {
                (0xc0 + v / 3) << 16 | (0xd6 + v / 4) << 8 | (0xe8 + v / 7)
            } else {
                (9 + v / 13) << 16 | (20 + v / 6) << 8 | (36 + v / 4)
            };
            self.rectangle(0, y, self.fb.width, 1, bg as u32);
        }
        // Background ribbons add layered depth without extra assets.
        self.round_rect(self.fb.width * 3 / 5, 50, self.fb.width / 3, 5, 2, 0x167b87);
        self.round_rect(self.fb.width / 14, self.fb.height * 4 / 5,
                        self.fb.width / 3, 3, 1, 0x207a83);
        self.round_rect(g.x + 9, g.y + 12, g.width, g.height, 19, 0x0c1a2b);
        self.round_rect(g.x, g.y, g.width, g.height, 19, surface);
        if g.sidebar > 0 {
            self.round_rect(g.x, g.y, g.sidebar, g.height, 18, 0x10243a);
            self.rectangle(g.x + g.sidebar - 18, g.y, 18, g.height, 0x10243a);
            self.round_rect(g.x + 27, g.y + 27, 39, 39, 11, accent);
            self.text("Z", g.x + 39, g.y + 37, 3, 0x0c2130);
            self.text("ZILLA OS", g.x + 78, g.y + 40, 2, 0xeaf8ff);
            self.text("FIRST RUN", g.x + 30, g.y + 105, 2, 0x76a0b8);
            for step in 0..9usize {
                let top = g.y + 155 + step * 43;
                let (name, done) = match step {
                    0 => ("START", wizard.step.index() > 0),
                    1 => ("LANGUAGE", wizard.step.index() > 1),
                    2 => ("REGION", wizard.step.index() > 2),
                    3 => ("TIME ZONE", wizard.step.index() > 3),
                    4 => ("DEVICE", wizard.step.index() > 4),
                    5 => ("THEME", wizard.step.index() > 5),
                    6 => ("STORAGE", wizard.step.index() > 6),
                    7 => ("INSTALL", wizard.step.index() > 7),
                    _ => ("FINISH", wizard.step.index() > 8),
                };
                if top + 23 >= g.y + g.height - 24 { break; }
                let active = wizard.step.index() == step;
                self.round_rect(g.x + 26, top, 25, 25, 8,
                                if active { accent } else if done { 0x265f67 } else { 0x294056 });
                let digit = match step {0=>"1",1=>"2",2=>"3",3=>"4",4=>"5",5=>"6",6=>"7",7=>"8",_=>"9"};
                self.text(digit, g.x + 35, top + 8, 1, if active { 0x102032 } else { 0xb8dbe3 });
                self.text(name, g.x + 65, top + 9, 1,
                          if active { 0xffffff } else { 0x91b0c4 });
            }
            self.text("NO DISK CHANGES", g.x + 24,
                      g.y + g.height.saturating_sub(34), 1, 0x7295a9);
        }

        // Step number, title and explanatory subtitle.
        let bx = g.body_x;
        let by = g.y;
        self.text("ZILLAOS  /  SETUP", bx, by + 25, 2, muted);
        if g.height > 300 {
            self.round_rect(bx, by + 59, g.body_width, 4, 2, soft);
            self.round_rect(bx, by + 59,
                g.body_width * wizard.step.index().min(9) / 9, 4, 2, accent);
        }
        let (uk_title, en_title, uk_sub, en_sub) = match wizard.step {
            Step::Welcome => (
                "ВІТАЄМО У ZILLAOS", "WELCOME TO ZILLAOS",
                "ВАША НОВА СИСТЕМА. ВАШІ ПРАВИЛА.",
                "A NEW SPACE TO CREATE AND EXPLORE.",
            ),
            Step::Language => (
                "МОВА СИСТЕМИ", "SYSTEM LANGUAGE",
                "ОБЕРІТЬ МОВУ ІНТЕРФЕЙСУ",
                "CHOOSE YOUR PREFERRED LANGUAGE",
            ),
            Step::Region => (
                "РЕГІОН", "YOUR REGION",
                "ОБЕРІТЬ КРАЇНУ",
                "SELECT YOUR COUNTRY",
            ),
            Step::TimeZone => (
                "ЧАСОВИЙ ПОЯС", "TIME ZONE",
                "ЧАСОВИЙ ПОЯС ДЛЯ СИСТЕМИ",
                "CHOOSE YOUR LOCAL TIME ZONE",
            ),
            Step::Device => (
                "НАЗВА КОМП'ЮТЕРА", "NAME YOUR COMPUTER",
                "ЛІТЕРИ A-Z, ЦИФРИ ТА ДЕФІС",
                "USE A-Z, NUMBERS AND DASHES",
            ),
            Step::Appearance => (
                "ОФОРМЛЕННЯ", "PERSONALIZE",
                "ОБЕРІТЬ СВІТЛУ АБО ТЕМНУ ТЕМУ",
                "CHOOSE DARK OR LIGHT APPEARANCE",
            ),
            Step::Storage => (
                "НАКОПИЧУВАЧІ", "STORAGE DISCOVERY",
                "ПОКИ ТІЛЬКИ КОНТРОЛЕРИ PCI",
                "PCI CONTROLLERS ONLY - NO DISK ACCESS",
            ),
            Step::InstallPlan => (
                "ПЛАН ВСТАНОВЛЕННЯ", "INSTALLATION PLAN",
                "БЕЗПЕЧНИЙ ОГЛЯД - ЗАПИС ВИМКНЕНО",
                "READ ONLY PREVIEW - WRITES DISABLED",
            ),
            Step::Review | Step::Finished => (
                "УСЕ ГОТОВО", "READY TO START",
                "ПЕРЕВІРТЕ НАЛАШТУВАННЯ",
                "REVIEW YOUR FIRST-RUN SETTINGS",
            ),
        };
        let (ru_title, ru_sub) = match wizard.step {
            Step::Welcome => ("ДОБРО ПОЖАЛОВАТЬ", "ВАША НОВАЯ СИСТЕМА."),
            Step::Language => ("ЯЗЫК СИСТЕМЫ", "ВЫБЕРИТЕ ЯЗЫК ИНТЕРФЕЙСА"),
            Step::Region => ("РЕГИОН", "ВЫБЕРИТЕ СТРАНУ"),
            Step::TimeZone => ("ЧАСОВОЙ ПОЯС", "ВЫБЕРИТЕ ЧАСОВОЙ ПОЯС"),
            Step::Device => ("ИМЯ КОМПЬЮТЕРА", "БУКВЫ A-Z, ЦИФРЫ И ДЕФИС"),
            Step::Appearance => ("ОФОРМЛЕНИЕ", "ВЫБЕРИТЕ СВЕТЛУЮ ИЛИ ТЁМНУЮ ТЕМУ"),
            Step::Storage => ("НАКОПИТЕЛИ", "ПОКА ТОЛЬКО КОНТРОЛЛЕРЫ PCI"),
            Step::InstallPlan => ("ПЛАН УСТАНОВКИ", "ТОЛЬКО ПРОСМОТР - БЕЗ ЗАПИСИ"),
            Step::Review | Step::Finished => ("ГОТОВО", "ПРОВЕРЬТЕ НАСТРОЙКИ"),
        };
        let title = wizard.localized3(uk_title, en_title, ru_title);
        let subtitle = wizard.localized3(uk_sub, en_sub, ru_sub);
        let scale = if g.body_width >= 465 { 3 } else if g.body_width >= 260 { 2 } else { 1 };
        self.text(title, bx, by + if g.height >= 400 { 92 } else { 67 }, scale, fg);
        if g.height >= 460 {
            self.text(subtitle, bx, by + 130, 1, muted);
        }

        match wizard.step {
            Step::Welcome => {
                let top = g.option_y.saturating_add(3);
                self.round_rect(bx, top, g.body_width, 133.min(g.footer_y.saturating_sub(top+12)),
                                14, soft);
                self.round_rect(bx + 17, top + 20, 51, 51, 14, accent);
                self.text("Z", bx + 34, top + 32, 3, 0x123246);
                if g.body_width >= 350 {
                    self.text("ZILLA DESKTOP", bx + 86, top + 28, 2, fg);
                    self.text("RUST / X86-64 / UEFI", bx + 86, top + 58, 1, muted);
                    self.text("WELCOME EXPERIENCE", bx + 23, top + 104, 1, muted);
                }
            }
            Step::Language | Step::Region | Step::TimeZone | Step::Appearance => {
                let count = wizard.option_count();
                for i in 0..count {
                    let top = g.option_y + i * g.option_gap;
                    if top + g.option_height >= g.footer_y { break; }
                    let active = wizard.selection == i;
                    let bg = if active {
                        if light { 0xd6f5ee } else { 0x195365 }
                    } else { soft };
                    self.round_rect(bx, top, g.body_width, g.option_height, 11, bg);
                    self.round_rect(bx + 13, top + (g.option_height.saturating_sub(20))/2,
                                    20, 20, 10, if active { accent } else { 0x59788c });
                    if active {
                        self.round_rect(bx + 19, top + (g.option_height.saturating_sub(8))/2,
                                        8, 8, 4, 0x0a2834);
                    }
                    let option = match wizard.step {
                        Step::Language => match i {
                            0 => "УКРАЇНСЬКА",
                            1 => "ENGLISH",
                            _ => "РУССКИЙ",
                        },
                        Step::Region => match i {
                            0 => wizard.localized3("УКРАЇНА", "UKRAINE", "УКРАИНА"),
                            1 => wizard.localized3("ПОЛЬЩА", "POLAND", "ПОЛЬША"),
                            _ => wizard.localized3("США", "UNITED STATES", "США"),
                        },
                        Step::TimeZone => match i {
                            0 => "EUROPE/KYIV", 1 => "EUROPE/WARSAW",
                            2 => "AMERICA/NEW YORK", _ => "UTC",
                        },
                        Step::Appearance => if i == 0 {
                            wizard.localized3("ТЕМНА ТЕМА", "DARK THEME", "ТЁМНАЯ ТЕМА")
                        } else {
                            wizard.localized3("СВІТЛА ТЕМА", "LIGHT THEME", "СВЕТЛАЯ ТЕМА")
                        },
                        _ => "",
                    };
                    self.text(option, bx + 46, top + 12, if g.body_width >= 460 {2} else {1}, fg);
                }
            }
            Step::Device => {
                let top = g.option_y + 5;
                self.text(wizard.localized("ІМ'Я ПРИСТРОЮ", "DEVICE NAME"),
                          bx, top, 2, muted);
                self.round_rect(bx, top + 38, g.body_width, 62, 12, soft);
                self.round_rect(bx + 2, top + 40, 4, 58, 2, accent);
                self.text(wizard.hostname.as_str(), bx + 21, top + 59, 2, fg);
                if !wizard.hostname.is_valid() {
                    self.text("3-24 A-Z 0-9 -", bx, top + 122, 1, 0xf1a58b);
                } else if g.footer_y > top + 154 {
                    self.text("ENTER TO CONTINUE", bx, top + 125, 1, muted);
                }
            }
            Step::Storage => {
                self.round_rect(bx, g.option_y, g.body_width, 62, 11, soft);
                self.round_rect(bx + 14, g.option_y + 15, 31, 31, 8, accent);
                self.text("S", bx + 24, g.option_y + 25, 2, 0x103044);
                let label = wizard.localized("ПЕРЕВІРКА PCI", "PCI DEVICE SCAN");
                self.text(label, bx + 57, g.option_y + 22, 2, fg);
                let count = match wizard.storage.count {
                    0 => "0", 1 => "1", 2 => "2", 3 => "3", 4 => "4",
                    5 => "5", 6 => "6", 7 => "7", _ => "8",
                };
                self.text(wizard.localized("ЗНАЙДЕНО КОНТРОЛЕРІВ", "CONTROLLERS FOUND"),
                          bx, g.option_y + 78, 1, muted);
                self.text(count, bx + g.body_width.saturating_sub(28),
                          g.option_y + 77, 2, accent);
                for (i, controller) in wizard.storage.iter().take(3).enumerate() {
                    let y = g.option_y + 111 + i * 45;
                    if y + 38 >= g.footer_y { break; }
                    self.round_rect(bx, y, g.body_width, 38, 9, soft);
                    self.text(controller.kind.label(), bx + 16, y + 13, 1, fg);
                    if g.body_width > 350 {
                        self.text(if controller.vendor == 0x15ad {
                            "VMWARE PCI"
                        } else {
                            "PCI DEVICE"
                        }, bx + g.body_width.saturating_sub(105), y + 13, 1, muted);
                    }
                }
                let bottom = g.footer_y.saturating_sub(67);
                if bottom > g.option_y + 115 {
                    self.rectangle(bx, bottom, 4, 47, 0xe3a862);
                    self.text("DISKS NOT VERIFIED", bx + 15, bottom + 5, 1, fg);
                    self.text("INSTALLATION DISABLED", bx + 15, bottom + 26, 1, 0xe3a862);
                }
            }
            Step::InstallPlan => {
                // The guest has no disk driver. This page visualizes missing
                // prerequisites; it NEVER suggests a real target was selected.
                let stages = [
                    ("01  PCI CONTROLLERS", "SCANNED"),
                    ("02  DISK IDENTIFICATION", "LOCKED"),
                    ("03  GPT AND FILESYSTEM", "LOCKED"),
                    ("04  EFI BOOT INSTALL", "LOCKED"),
                ];
                for (i, (title, status)) in stages.iter().enumerate() {
                    let y = g.option_y + i * g.option_gap;
                    if y + g.option_height >= g.footer_y { break; }
                    self.round_rect(bx, y, g.body_width, g.option_height, 11, soft);
                    self.round_rect(bx + 12, y + 12, 22, 22, 8,
                                    if i == 0 { 0x18746b } else { 0x5b5364 });
                    self.text(if i == 0 { "+" } else { "X" },
                              bx + 20, y + 20, 1, fg);
                    self.text(title, bx + 43, y + 13, 1, fg);
                    if g.body_width >= 430 {
                        self.text(status, bx + g.body_width.saturating_sub(69),
                                  y + 28, 1, if i == 0 { accent } else { 0xea9f7a });
                    }
                }
                let bottom = g.footer_y.saturating_sub(54);
                if bottom > g.option_y + 112 {
                    self.rectangle(bx, bottom, 4, 38, 0xe3a862);
                    self.text("NO VERIFIED DISK", bx + 15, bottom + 4, 1, fg);
                    self.text("INSTALL ACTION DISABLED", bx + 15, bottom + 23, 1, 0xe3a862);
                }
            }
            Step::Review => {
                let language = match wizard.language {
                    Language::Ukrainian => "UKRAINIAN",
                    Language::English => "ENGLISH",
                    Language::Russian => "RUSSIAN",
                };
                let region = match wizard.region {
                    SetupRegion::Ukraine => "UKRAINE",
                    SetupRegion::Poland => "POLAND",
                    SetupRegion::UnitedStates => "UNITED STATES",
                };
                let fields: [(&str, &str); 4] = [
                    ("LANGUAGE", language),
                    ("REGION", region),
                    ("TIME", wizard.timezone.identifier()),
                    ("DEVICE", wizard.hostname.as_str()),
                ];
                for (i, (label, value)) in fields.iter().enumerate() {
                    let top = g.option_y + i * g.option_gap;
                    if top + g.option_height >= g.footer_y { break; }
                    self.round_rect(bx, top, g.body_width, g.option_height, 10, soft);
                    self.text(label, bx + 15, top + 9, 1, muted);
                    self.text(value, bx + 15, top + 24, 1, fg);
                }
                // Honest onboarding: review is not a real disk installer.
                // No storage enumeration or write operation is wired here.
                let note_y = g.option_y + 4 * g.option_gap + 5;
                if note_y + 44 < g.footer_y {
                    self.round_rect(bx, note_y, g.body_width, 43, 10, soft);
                    self.rectangle(bx + 9, note_y + 8, 4, 27, 0x1ed7b6);
                    self.text("INSTALL PREVIEW ONLY", bx + 21, note_y + 9, 1, fg);
                    self.text("NO DISK WRITES", bx + 21, note_y + 27, 1, muted);
                }
            }
            Step::Finished => {}
        }

        if g.height >= 190 {
            let next_label = if wizard.step == Step::Review {
                wizard.localized("ДО РОБОЧОГО СТОЛУ", "ENTER DESKTOP")
            } else { wizard.localized("ДАЛІ", "CONTINUE") };
            if wizard.step != Step::Welcome {
                self.round_rect(bx, g.footer_y, 112, 44, 12, soft);
                self.text(wizard.localized("НАЗАД", "BACK"), bx + 23, g.footer_y + 16, 1, fg);
            }
            let next_x = bx + g.body_width.saturating_sub(162);
            self.round_rect(next_x, g.footer_y, 162, 44, 12,
                            if wizard.step == Step::Device && !wizard.hostname.is_valid() {
                                0x627787
                            } else { accent });
            self.text(next_label, next_x + 14, g.footer_y + 16,
                      1, 0x092b36);
        }
        // Explicitly avoid pretending to install the OS or set system clock.
        if g.height >= 450 {
            self.text(wizard.localized("ПАРАМЕТРИ У RAM - БЕЗ ЗАПИСУ НА ДИСК",
                                      "PREVIEW - SETTINGS IN MEMORY ONLY"),
                      bx, g.y + g.height.saturating_sub(15), 1, muted);
        }
    }

    /// Scale an immutable, bootloader-decoded BMP into the current RGB32
    /// GOP framebuffer. No disk access, heap or GPU acceleration. The
    /// Limine module bytes were copied to BSS during early startup.
    fn custom_background(&self) -> bool {
        let Some(image) = wallpaper_asset::current() else { return false; };
        let width = self.fb.width;
        let height = self.fb.height;
        if self.buffered {
            // SAFETY: the renderer runs on the bootstrap CPU only; the
            // image is immutable and framebuffer fits static backing BSS.
            let dst = unsafe { &mut *BACKBUFFER.0.get() };
            for y in 0..height {
                let sy = (y as u64 * image.height as u64 / height as u64) as usize;
                let from = sy * image.width;
                let to = y * width;
                for x in 0..width {
                    let sx = (x as u64 * image.width as u64 / width as u64) as usize;
                    dst[to + x] = self.rgb(image.pixels[from + sx]);
                }
            }
        } else {
            // SAFETY: validated Limine framebuffer spans width*height and
            // pitch bytes; per-pixel indices are inside visible bounds.
            for y in 0..height {
                let sy = (y as u64 * image.height as u64 / height as u64) as usize;
                let from = sy * image.width;
                for x in 0..width {
                    let sx = (x as u64 * image.width as u64 / width as u64) as usize;
                    let offset = y * self.fb.pitch + x * 4;
                    unsafe {
                        ptr::write_volatile(
                            self.fb.address.add(offset).cast::<u32>(),
                            self.rgb(image.pixels[from + sx]),
                        );
                    }
                }
            }
        }
        true
    }

    fn background(&self, state: &Desktop) {
        if state.wallpaper == Wallpaper::Custom && self.custom_background() {
            return;
        }
        let h = self.fb.height;
        let w = self.fb.width;
        let (top, bottom, horizon) = state.wallpaper.palette(state.dark);
        for row in 0..h {
            let fraction = ((row as u64 * 255) / h.max(1) as u64) as u32;
            let color = personalization::gradient(top, bottom, fraction);
            self.rectangle(0, row, w, 1, color);
        }
        match state.wallpaper {
            Wallpaper::Ocean | Wallpaper::Custom => {
                self.rectangle(w / 6, h * 62 / 100, w * 2 / 3, 2, horizon);
                self.rectangle(w / 5, h * 64 / 100, w * 3 / 5, 1, horizon);
                self.rectangle(w / 3, h * 20 / 100, 3, h / 3, horizon);
            }
            Wallpaper::Sunset => {
                self.round_rect(w * 68 / 100, h * 22 / 100,
                    (w / 12).max(20), (w / 12).max(20), (w / 12).max(20),
                    if state.dark { 0xf5bd83 } else { 0xffe3a8 });
                self.rectangle(0, h * 64 / 100, w, 3, horizon);
                self.rectangle(w / 8, h * 70 / 100, w * 3 / 4, 2, horizon);
            }
            Wallpaper::Forest => {
                self.rectangle(0, h * 71 / 100, w, h * 29 / 100, horizon);
                for i in 0..11usize {
                    let x = (i * w / 10).min(w.saturating_sub(2));
                    let tree_h = h * (12 + (i * 7) % 24) / 100;
                    self.rectangle(x, h * 71 / 100 - tree_h, 8, tree_h + 4,
                        if state.dark { 0x133c37 } else { 0x3c7465 });
                }
            }
        }
    }

    fn top_panel(&self, state: &Desktop, layout: &Layout) {
        let fg = if state.dark { 0xebf5ff } else { 0x142739 };
        let background = if state.dark { 0x122138 } else { 0xf5faff };
        self.rectangle(0, 0, layout.width, layout.top_bar, background);
        self.rectangle(0, layout.top_bar - 2, layout.width, 2, state.accent.rgb());
        self.round_rect(16, 9, 23, 23, 7, state.accent.rgb());
        self.text("Z", 22, 13, 2, 0x0b2535);
        self.text("ZILLAOS", 49, 15, 2, fg);
        if layout.width >= 720 {
            let mut menu_x = 170;
            for index in 0..taskbar::MENU_COUNT {
                let title = taskbar::menu_label(state.language, index);
                self.text(title, menu_x, 18, 1, fg);
                menu_x = menu_x.saturating_add(title.chars().count() * 6 + 16);
            }
        }
        // One deterministic right-aligned layout shared with host-side tests.
        // Optional fields disappear rather than overwrite ZILLA-PC or menu.
        let status = status_bar::status(
            *layout, state.language, state.hostname.as_str().chars().count(),
        );
        if let Some(slot) = status.microphone {
            self.round_rect(slot.x, 9, slot.width, 25, 6,
                if state.dark { 0x263a50 } else { 0xdce9ee });
            self.text("MIC N/A", slot.x + 11, 18, 1, 0x83adb9);
        }
        if let Some(slot) = status.hostname {
            self.text(state.hostname.as_str(), slot.x, 17, 1, fg);
        }
        if let Some(slot) = status.date {
            if let Some(date) = state.date {
                let bytes = date.ascii();
                if let Ok(label) = core::str::from_utf8(&bytes) {
                    self.text(label, slot.x, 17, 1, fg);
                }
            } else {
                self.text("DATE N/A", slot.x, 17, 1, 0x83a8ba);
            }
        }
        if let Some(clock) = state.clock {
            let bytes = clock.ascii();
            if let Ok(time) = core::str::from_utf8(&bytes) {
                self.text(time, status.time.x, 17, 1, fg);
            }
        } else {
            self.text("--:--:--", status.time.x, 17, 1, fg);
        }
        self.text("RTC", status.rtc.x, 17, 1, 0x58c5ce);
    }

    fn wallpaper_label(&self, state: &Desktop, layout: &Layout) {
        let fg = if state.dark { 0xecfcff } else { 0x17344b };
        let small = if state.dark { 0x80adc4 } else { 0x35566d };
        if layout.width >= 660 && layout.height >= 480 {
            self.text(tr(state.language, TextId::WallpaperTitle),
                      42, layout.top_bar + 46, 2, fg);
            self.text(tr(state.language, TextId::WallpaperSubtitle),
                      44, layout.top_bar + 75, 1, small);
        }
    }

    fn desktop_shortcuts(&self, state: &Desktop, layout: &Layout) {
        if layout.width < 500 || layout.height < 440 { return; }
        for (i, descriptor) in apps::BUILTIN_APPS.iter().enumerate() {
            let (label, icon, color) = (descriptor.title_key, descriptor.glyph, descriptor.color);
            let x = 30;
            let y = layout.top_bar + 145 + i * 78;
            self.round_rect(x, y, 100, 66, 11,
                if state.dark { 0x1d3149 } else { 0xe1f0f8 });
            self.round_rect(x + 35, y + 6, 30, 30, 9, color);
            self.text(icon, x + 45, y + 14, 2, 0xffffff);
            let title = tr(state.language, label);
            let txt_x = x + 50usize.saturating_sub(title.chars().count() * 3);
            self.text(title, txt_x, y + 47, 1,
                if state.dark { 0xe6f2ff } else { 0x183144 });
        }
    }

    fn desktop_folders(&self, state: &Desktop, _layout: &Layout) {
        // These icons represent paths inside the immutable ZillaOS RAMFS;
        // dragging moves only the icon geometry, never files on host disks.
        if !state.icons.visible() { return; }
        let labels = [TextId::Documents, TextId::Pictures,
                      TextId::Downloads, TextId::ProgramFiles];
        let fg = if state.dark { 0xe8f5ff } else { 0x1d394e };
        for (index, point) in state.icons.positions.iter().enumerate() {
            let selected = state.icons.dragging == Some(index);
            let background = if selected {
                0x326178
            } else if state.dark { 0x1e344d } else { 0xddecf7 };
            self.round_rect(point.x, point.y, 92, 70, 10, background);
            self.round_rect(point.x + 28, point.y + 13, 36, 27, 5, 0x31b8c3);
            self.rectangle(point.x + 33, point.y + 9, 21, 8, 0x6be5e1);
            let name = tr(state.language, labels[index]);
            let width = name.chars().count().saturating_mul(6);
            self.text(name, point.x + 46usize.saturating_sub(width / 2),
                      point.y + 52, 1, fg);
        }
    }

    /// Hybrid Windows taskbar / macOS dock. Buttons share hit-test geometry.
    fn taskbar(&self, state: &Desktop, layout: &Layout) {
        let y = layout.height.saturating_sub(layout.dock_height + 12);
        self.rectangle(0, y, layout.width, layout.height.saturating_sub(y),
                       if state.dark { 0x12243a } else { 0xd9ebf3 });
        self.rectangle(0, y, layout.width, 1,
                       if state.dark { 0x294860 } else { 0xb2d1d9 });
        for action in [taskbar::Action::Start, taskbar::Action::Tasks] {
            let Some(rect) = taskbar::button(*layout, action) else { continue };
            let active = match action {
                taskbar::Action::Start => state.launcher,
                taskbar::Action::Tasks => state.active() == Some(App::TaskManager),
            };
            let bg = if active { 0x1b777d } else if state.dark { 0x28425a } else { 0xc8dee8 };
            self.round_rect(rect.x, rect.y, rect.width, rect.height, 10, bg);
            let fg = if state.dark || active { 0xeaf8ff } else { 0x244458 };
            match action {
                taskbar::Action::Start => {
                    self.round_rect(rect.x + 9, rect.y + 11, 23, 23, 7, state.accent.rgb());
                    self.text("Z", rect.x + 16, rect.y + 17, 1, 0x102236);
                    self.text("START", rect.x + 36, rect.y + 20, 1, fg);
                }
                taskbar::Action::Tasks => {
                    self.round_rect(rect.x + 8, rect.y + 11, 23, 23, 6, 0x147f91);
                    for i in 0..2 { for j in 0..2 {
                        self.rectangle(rect.x + 13 + i * 8, rect.y + 16 + j * 8, 5, 5, 0xe5fdff);
                    } }
                    self.text("TASKS", rect.x + 37, rect.y + 20, 1, fg);
                }
            }
        }
        if layout.width >= 960 {
            let right = layout.width.saturating_sub(146);
            self.text("RTC", right, y + 17, 1, 0x8fc6d1);
            self.text("MIC --", right + 36, y + 17, 1, 0x80a7b9);
            self.text("CTRL SHIFT ESC", right.saturating_sub(24), y + 33, 1, 0x7595aa);
        }
    }

    fn dock(&self, state: &Desktop, layout: &Layout) {
        let width = layout.width.min(500).saturating_sub(22);
        let x = layout.width.saturating_sub(width) / 2;
        let h = layout.dock_height;
        let y = layout.height.saturating_sub(h + 10);
        let background = if state.dark { 0x192c42 } else { 0xe5f4f8 };
        self.round_rect(x + 3, y + 6, width, h, 17, 0x102036);
        self.round_rect(x, y, width, h, 17, background);
        let icon = h.saturating_sub(18).min(46);
        let spacing = icon + 14;
        let base = x + width.saturating_sub(App::COUNT * spacing) / 2 + 7;
        for (i, descriptor) in apps::BUILTIN_APPS.iter().enumerate() {
            let px = base + i * spacing;
            let (app, color, letter) = (descriptor.id, descriptor.color, descriptor.glyph);
            self.round_rect(px, y + 8, icon, icon, 11, color);
            self.text(letter, px + (icon.saturating_sub(10)) / 2, y + 17, 2, 0xffffff);
            if state.active() == Some(app) {
                self.rectangle(px + 8, y + h - 5, icon.saturating_sub(16), 3, 0x1cd9bd);
            } else if state.windows.window(app).mode == WindowMode::Minimized {
                self.rectangle(px + icon / 2, y + h - 5, 4, 3, 0xffc35a);
            } else if state.windows.window(app).visible() {
                self.rectangle(px + icon / 2, y + h - 5, 4, 3, 0x7d9cb7);
            }
        }
    }

    fn window(&self, state: &Desktop, app: App, rect: Rect, focused: bool) {
        let x = rect.x;
        let y = rect.y;
        let w = rect.width;
        let h = rect.height;
        let bg = if state.dark { 0x17253a } else { 0xf5faff };
        let title_bar = match (state.dark, focused) {
            (true, true) => 0x2f5270,
            (true, false) => 0x253b54,
            (false, true) => 0xd0e7f2,
            (false, false) => 0xdce9f2,
        };
        let fg = if state.dark { 0xedf5fc } else { 0x20394e };
        let secondary = if state.dark { 0x9eb5cc } else { 0x587084 };
        self.round_rect(x + 8, y + 11, w, h, 13, 0x0b192b);
        if focused {
            self.round_rect(x.saturating_sub(2), y.saturating_sub(2),
                            w.saturating_add(4), h.saturating_add(4), 13, 0x18788d);
        }
        self.round_rect(x, y, w, h, 13, bg);
        let bar_h = if h < 170 { 30 } else { 45 };
        self.round_rect(x, y, w, bar_h, 13, title_bar);
        self.rectangle(x, y + bar_h - 12, w, 12, title_bar);
        for (i, dot) in [0xfd6872, 0xffcb55, 0x45c784].iter().enumerate() {
            self.round_rect(x + 14 + i * 19, y + 13, 11, 11, 5, *dot);
        }
        let title = tr(state.language, apps::descriptor(app).title_key);
        self.text(title, x + 87, y + 14, if w >= 360 { 2 } else { 1 }, fg);
        if focused && w >= 310 {
            self.text(tr(state.language, TextId::Active), x + w.saturating_sub(70), y + 18, 1, 0x1bd5ba);
        }

        let body = y + bar_h + 8;
        let content_h = h.saturating_sub(bar_h + 16);
        if content_h < 36 {
            return;
        }
        let side_width = (w / 4).clamp(70, 170);
        self.round_rect(x + 10, body, side_width, content_h, 9,
                        if state.dark { 0x20344c } else { 0xe7f2f8 });
        let ty = body + 15;
        self.text(tr(state.language, TextId::Home), x + 19, ty, 1, secondary);
        if content_h > 105 {
            self.text(tr(state.language, TextId::Files), x + 19, ty + 24, 1, fg);
            self.text(tr(state.language, TextId::Apps), x + 19, ty + 46, 1, fg);
            self.text(tr(state.language, TextId::About), x + 19, ty + 68, 1, fg);
        }

        let cx = x + side_width + 27;
        let cw = w.saturating_sub(side_width + 44);
        match app {
            App::Files => {
                // True in-memory path navigation, not decorative fake folders.
                // The virtual tree is read-only and has no disk driver yet.
                let current = fs::node(state.files.current);
                let location = current.map(|n| n.name).unwrap_or("ZillaOS");
                if content_h > 35 {
                    self.text(location, cx, body + 11, if cw > 250 { 2 } else { 1 }, fg);
                }
                if let Some(preview_id) = state.files.preview_file {
                    if content_h > 80 {
                        self.text("BACKSPACE / UP", cx, body + 31, 1, secondary);
                        if let Some(file) = fs::node(preview_id) {
                            self.text(file.name, cx, body + 58, 1, 0x1dd9b4);
                            if let Ok(contents) = core::str::from_utf8(file.data) {
                                for (row, line) in contents.lines().take(5).enumerate() {
                                    let pos_y = body + 86 + row * 23;
                                    if pos_y + 15 < body + content_h {
                                        self.text(line, cx, pos_y, 1, fg);
                                    }
                                }
                            }
                        }
                    }
                } else if content_h > 95 {
                    self.text(if state.files.current == fs::ROOT {
                        "Z:/  RAMFS"
                    } else { "BACKSPACE / UP" }, cx, body + 31, 1, secondary);
                    for row in 0..7 {
                        let top = body + 42 + row * 47;
                        if top + 40 >= body + content_h.saturating_sub(22) {
                            break;
                        }
                        if let Some((_, entry)) = fs::child_at(state.files.current, row) {
                            self.folder_card(cx, top, cw, entry.name, state.dark,
                                             entry.kind == NodeKind::File);
                        } else if state.files.current == DOCUMENTS_DIR {
                            let static_count = fs::child_count(DOCUMENTS_DIR);
                            if row < static_count { break; }
                            let Some((_, memory_file)) = state.ramfs.nth(row - static_count)
                                else { break; };
                            self.folder_card(cx, top, cw, memory_file.name(), state.dark, true);
                        } else {
                            break;
                        }
                    }
                    self.text("RAMFS / READ ONLY / NO DISK WRITE",
                              cx, body + content_h.saturating_sub(18), 1, secondary);
                }
            }
            App::Terminal => {
                self.round_rect(cx, body + 1, cw, content_h.saturating_sub(3), 8, 0x0b1726);
                self.text(tr(state.language, TextId::TerminalPreview),
                          cx + 12, body + 12, 1, 0x1dd9b4);
                if content_h > 105 {
                    self.text("> ZILLAOS 0.0.2", cx + 12, body + 39, 1, 0xc4dbea);
                    self.text("> RUST / X86_64", cx + 12, body + 58, 1, 0xc4dbea);
                    self.text("> DEMO / NO SHELL", cx + 12, body + 77, 1, 0x1dd9b4);
                }
            }
            App::AppCenter => {
                self.text(tr(state.language, TextId::AppCenter), cx, body + 9, 2, fg);
                if content_h > 98 {
                    self.text("INSTALLED BUILT-IN APPS", cx, body + 31, 1, secondary);
                    for (index, entry) in apps::BUILTIN_APPS.iter().enumerate() {
                        let py = body + 47 + index * 43;
                        if py + 36 >= body + content_h.saturating_sub(25) { break; }
                        self.round_rect(cx, py, cw, 36, 7,
                                        if state.dark { 0x263d54 } else { 0xdceef4 });
                        self.round_rect(cx + 7, py + 7, 23, 23, 5, entry.color);
                        self.text(entry.glyph, cx + 15, py + 13, 1, 0xffffff);
                        self.text(tr(state.language, entry.title_key),
                                  cx + 40, py + 13, 1, fg);
                    }
                    if content_h > 385 {
                        self.text("LINUX / WINDOWS / ANDROID: NO RUNTIME",
                                  cx, body + 355, 1, 0xffc35a);
                        self.text("USER TASKS: EXPERIMENTAL TEST ISO",
                                  cx, body + 373, 1, secondary);
                    }
                }
            }
            App::Notepad => {
                self.notepad_body(state, cx, body, cw, content_h, fg, secondary);
            }
            App::TaskManager => {
                // These are kernel-embedded GUI windows, not independent
                // processes. Do not invent PID, CPU %, memory, or threads.
                self.text("TASK MANAGER", cx, body + 9, 2, fg);
                self.round_rect(cx, body + 36, cw, 49, 8,
                    if state.dark { 0x234254 } else { 0xd7f0ed });
                self.text("KERNEL: RING 0 / BOOT CPU", cx + 10, body + 46, 1, 0x22d1b7);
                self.text("SMP OFF  |  RING3: TEST ISO ONLY", cx + 10,
                          body + 65, 1, secondary);
                self.text("EMBEDDED GUI WINDOWS  /  NOT PROCESSES", cx,
                          body + 89, 1, secondary);
                for (i, descriptor) in apps::BUILTIN_APPS.iter().enumerate() {
                    let top = body + task_view::ROW_START + i * task_view::ROW_GAP;
                    if top + task_view::ROW_HEIGHT >= body + content_h.saturating_sub(8) { break; }
                    let win = state.windows.window(descriptor.id);
                    let status = if state.active() == Some(descriptor.id) { "FOCUSED" }
                        else { match win.mode {
                            WindowMode::Normal | WindowMode::Maximized => "VISIBLE",
                            WindowMode::Minimized => "MINIMIZED",
                            WindowMode::Hidden => "CLOSED",
                        } };
                    let card = if state.active() == Some(descriptor.id) { 0x275669 }
                        else if state.dark { 0x263b52 } else { 0xe4f1f5 };
                    self.round_rect(cx, top, cw, task_view::ROW_HEIGHT, 7, card);
                    self.round_rect(cx + 6, top + 7, 23, 23, 6, descriptor.color);
                    self.text(descriptor.glyph, cx + 14, top + 14, 1, 0xffffff);
                    self.text(descriptor.id.label(), cx + 38, top + 14, 1, fg);
                    if cw >= 320 {
                        self.text(status, cx + cw.saturating_sub(78), top + 14, 1,
                                  if status == "CLOSED" { secondary } else { 0x44dfbe });
                    }
                }
                if content_h >= 380 {
                    self.text("CLICK ROW TO OPEN / FOCUS WINDOW", cx,
                              body + content_h.saturating_sub(28), 1, secondary);
                    self.text("CPU / RAM / THREAD METRICS: N/A", cx,
                              body + content_h.saturating_sub(13), 1, 0xd6b36e);
                }
            }

            App::Settings => {
                match state.settings_page {
                    SettingsPage::Overview => {
                        self.text(tr(state.language, TextId::Settings), cx, body + 9, 2, fg);
                        if content_h > 98 {
                            self.round_rect(cx, body + 42, cw, 51, 8,
                                if state.dark { 0x243b52 } else { 0xe1edf5 });
                            self.text(tr(state.language, TextId::Appearance),
                                      cx + 9, body + 49, 1, secondary);
                            self.text(tr(state.language, if state.dark {
                                TextId::ThemeDark
                            } else { TextId::ThemeLight }), cx + 9, body + 68, 1, fg);
                        }
                        if content_h > 168 {
                            self.round_rect(cx, body + 105, cw, 52, 8,
                                if state.dark { 0x243b52 } else { 0xe1edf5 });
                            self.text(tr(state.language, TextId::Language),
                                      cx + 9, body + 112, 1, secondary);
                            let lang = match state.language {
                                Language::Ukrainian => TextId::LangUkrainian,
                                Language::English => TextId::LangEnglish,
                                Language::Russian => TextId::LangRussian,
                            };
                            self.text(tr(state.language, lang), cx + 9, body + 133, 1, fg);
                        }
                        if content_h > 232 {
                            self.round_rect(cx, body + 175, cw, 51, 8,
                                if state.dark { 0x243b52 } else { 0xe1edf5 });
                            self.round_rect(cx + 10, body + 187, 27, 27, 8, state.accent.rgb());
                            self.text(tr(state.language, TextId::Personalization),
                                      cx + 45, body + 193, 1, fg);
                        }
                        if content_h > 290 {
                            self.text(tr(state.language, TextId::BootResolution),
                                      cx, body + 240, 1, secondary);
                            self.decimal(self.fb.width, cx, body + 258, fg);
                            self.text("X", cx + 29, body + 258, 1, fg);
                            self.decimal(self.fb.height, cx + 39, body + 258, fg);
                            self.text(tr(state.language, TextId::BootHint),
                                      cx, body + 277, 1, state.accent.rgb());
                        }
                        if content_h > 340 {
                            self.text(state.video.name(), cx, body + 306, 1, fg);
                            self.text("GPU ACCEL: NOT ENABLED", cx, body + 324, 1, 0xffc35a);
                        }
                    }
                    SettingsPage::Personalization => {
                        self.text(tr(state.language, TextId::Personalization),
                                  cx, body + 9, if cw > 240 { 2 } else { 1 }, fg);
                        if content_h > 35 {
                            self.text(tr(state.language, TextId::Back),
                                      cx + cw.saturating_sub(65), body + 14, 1, state.accent.rgb());
                        }
                        if content_h > 110 {
                            self.round_rect(cx, body + 56, cw, 51, 8,
                                if state.dark { 0x243b52 } else { 0xe1edf5 });
                            self.text(tr(state.language, TextId::Wallpaper),
                                      cx + 10, body + 63, 1, secondary);
                            self.text(state.wallpaper.name(state.language),
                                      cx + 10, body + 83, 1, fg);
                            for (index, wallpaper) in personalization::WALLPAPER_OPTIONS
                                .iter().copied().enumerate()
                            {
                                if let Some(tile) = personalization::preset_tile(
                                    rect, GalleryRow::Wallpaper, index,
                                ) {
                                    if wallpaper == state.wallpaper {
                                        self.round_rect(tile.x.saturating_sub(2),
                                            tile.y.saturating_sub(2),
                                            tile.width + 4, tile.height + 4, 8, state.accent.rgb());
                                    }
                                    if wallpaper == Wallpaper::Custom {
                                        // A missing user asset is clearly disabled-looking,
                                        // but remains clickable to explain the requirement.
                                        if let Some(image) = wallpaper_asset::current() {
                                            for line in 2..tile.height.saturating_sub(2) {
                                                let iy = (line * image.height / tile.height)
                                                    .min(image.height - 1);
                                                for column in 3..tile.width.saturating_sub(3) {
                                                    let ix = (column * image.width / tile.width)
                                                        .min(image.width - 1);
                                                    self.rectangle(tile.x + column,
                                                        tile.y + line, 1, 1,
                                                        image.pixels[iy * image.width + ix]);
                                                }
                                            }
                                        } else {
                                            self.round_rect(tile.x, tile.y, tile.width,
                                                            tile.height, 6, 0x59616e);
                                            self.text("N/A", tile.x + 12,
                                                      tile.y + 13, 1, 0xf7f7f7);
                                        }
                                    } else {
                                        let (top, bottom, horizon) = wallpaper.palette(state.dark);
                                        self.round_rect(tile.x, tile.y, tile.width, tile.height,
                                                        6, bottom);
                                        for line in 2..tile.height.saturating_sub(2) {
                                            let t = (line * 255 / tile.height) as u32;
                                            self.rectangle(tile.x + 3, tile.y + line,
                                                tile.width.saturating_sub(6), 1,
                                                personalization::gradient(top, bottom, t));
                                        }
                                        self.rectangle(tile.x + 3, tile.y + tile.height * 2 / 3,
                                            tile.width.saturating_sub(6), 2, horizon);
                                    }
                                }
                            }
                            if cw < 420 && cw >= 160 {
                                let (top, bottom, _) = state.wallpaper.palette(state.dark);
                                self.round_rect(cx + cw - 61, body + 64, 48, 34, 6,
                                    personalization::gradient(top, bottom, 130));
                            }
                        }
                        if content_h > 173 {
                            self.round_rect(cx, body + 119, cw, 51, 8,
                                if state.dark { 0x243b52 } else { 0xe1edf5 });
                            self.text(tr(state.language, TextId::AccentColor),
                                      cx + 10, body + 126, 1, secondary);
                            self.text(state.accent.name(state.language),
                                      cx + 10, body + 146, 1, fg);
                            for (index, color) in personalization::ACCENT_OPTIONS
                                .iter().copied().enumerate()
                            {
                                if let Some(tile) = personalization::preset_tile(
                                    rect, GalleryRow::Accent, index,
                                ) {
                                    if color == state.accent {
                                        self.round_rect(tile.x.saturating_sub(2),
                                            tile.y.saturating_sub(2),
                                            tile.width + 4, tile.height + 4, 8, fg);
                                    }
                                    self.round_rect(tile.x, tile.y,
                                        tile.width, tile.height, 6, color.rgb());
                                }
                            }
                            if cw < 420 && cw >= 160 {
                                self.round_rect(cx + cw - 51, body + 132,
                                                30, 28, 7, state.accent.rgb());
                            }
                        }
                        if content_h > 236 {
                            self.round_rect(cx, body + 182, cw, 51, 8,
                                if state.dark { 0x243b52 } else { 0xe1edf5 });
                            self.text(tr(state.language, TextId::Appearance),
                                      cx + 10, body + 189, 1, secondary);
                            self.text(tr(state.language, if state.dark {
                                TextId::ThemeDark
                            } else { TextId::ThemeLight }), cx + 10, body + 209, 1, fg);
                        }
                        if content_h > 268 {
                            self.text(tr(state.language, TextId::SessionOnly),
                                      cx, body + 250, 1, secondary);
                        }
                        if content_h > 291 {
                            self.text(state.profile_status.localized(state.language),
                                      cx, body + 273, 1, secondary);
                        }
                        for button in [
                            ProfileButton::Save, ProfileButton::Load, ProfileButton::Reset,
                        ] {
                            let text = button.label(state.language);
                            if let Some(r) = personalization::profile_button(rect, button) {
                                let color = match button {
                                    ProfileButton::Save => 0x207e77,
                                    ProfileButton::Load => 0x345e86,
                                    ProfileButton::Reset => 0x875369,
                                };
                                self.round_rect(r.x, r.y, r.width, r.height, 8, color);
                                let width = text.chars().count() * 6;
                                self.text(text, r.x + r.width.saturating_sub(width) / 2,
                                          r.y + 15, 1, 0xffffff);
                            }
                        }
                    }
                }
            }
        }
    }

    fn notepad_body(&self, state: &Desktop, cx: usize, body: usize,
                    cw: usize, content_h: usize, fg: u32, secondary: u32) {
        let note = &state.notepad;
        self.text(note.filename(), cx, body + 8, 2, fg);
        if note.dirty { self.text("*", cx + 180, body + 11, 2, 0xffc35a); }
        for (i, (name, color)) in [("NEW", 0x365b77), ("SAVE", 0x278f87),
                                    ("DOCUMENTS", 0x4c6486)].iter().enumerate() {
            let x = cx + match i { 0 => 0, 1 => 88, _ => 176 };
            let w = if i == 2 { 100 } else { 80 };
            if x + w < cx + cw {
                self.round_rect(x, body + 37, w, 35, 6, *color);
                self.text(name, x + 11, body + 50, 1, 0xffffff);
            }
        }
        let status = match note.status {
            EditorStatus::Ready => "READY - ASCII / US PS2",
            EditorStatus::Modified => "UNSAVED CHANGES",
            EditorStatus::Saved => "SAVED IN RAMFS",
            EditorStatus::UnsavedChanges => "SAVE BEFORE NEW / OPEN",
            EditorStatus::StorageError(_) => "RAMFS FULL / SAVE FAILED",
            EditorStatus::BufferFull => "EDITOR BUFFER FULL",
        };
        self.text(status, cx, body + content_h.saturating_sub(32), 1, secondary);
        self.text("F11 / CTRL+S SAVE  F12 NEW  VOLATILE",
                  cx, body + content_h.saturating_sub(17), 1, secondary);

        if content_h < 150 || cw < 80 { return; }
        let edit_y = body + 88;
        let rows = content_h.saturating_sub(140) / 17;
        if rows == 0 { return; }
        let columns = (cw.saturating_sub(28) / 6).max(1);
        self.round_rect(cx, edit_y - 5, cw, rows * 17 + 12, 6,
                        if state.dark { 0x0d1c2e } else { 0xeaf3fb });
        let (cursor_row, cursor_col) = note.cursor_visual(columns);
        let scroll = cursor_row.saturating_sub(rows - 1);
        let bytes = note.bytes();
        let mut visual_row = 0usize;
        let mut col = 0usize;
        let mut from = 0usize;
        for index in 0..=bytes.len() {
            let at_end = index == bytes.len();
            let newline = !at_end && bytes[index] == b'\n';
            let wrap = col >= columns;
            if at_end || newline || wrap {
                if visual_row >= scroll && visual_row - scroll < rows && index > from {
                    if let Ok(line) = core::str::from_utf8(&bytes[from..index]) {
                        self.text(line, cx + 10, edit_y + (visual_row - scroll) * 17,
                                  1, fg);
                    }
                }
                visual_row += 1;
                from = if newline { index + 1 } else { index };
                col = 0;
            }
            if !at_end && !newline { col += 1; }
        }
        if cursor_row >= scroll && cursor_row - scroll < rows {
            self.rectangle(cx + 10 + cursor_col * 6,
                           edit_y + (cursor_row - scroll) * 17,
                           2, 10, 0x20d5bd);
        }
    }

    fn folder_card(&self, x: usize, y: usize, max_width: usize, title: &str,
                   dark: bool, file: bool) {
        if max_width < 70 { return; }
        self.round_rect(x, y, max_width.min(360), 40, 7,
                        if dark { 0x283e55 } else { 0xe1edf5 });
        if file {
            self.round_rect(x + 12, y + 9, 16, 22, 2, 0x64aaf0);
            self.rectangle(x + 15, y + 15, 10, 2, 0xe5f5ff);
            self.rectangle(x + 15, y + 21, 8, 2, 0xe5f5ff);
        } else {
            self.round_rect(x + 10, y + 12, 19, 15, 3, 0x2bc5be);
            self.rectangle(x + 13, y + 9, 11, 5, 0x58dce0);
        }
        self.text(title, x + 38, y + 16, 1, if dark { 0xebf5ff } else { 0x173047 });
    }

    fn launcher(&self, state: &Desktop, layout: &Layout) {
        if state.launcher_step == 0 { return; }
        // Real slide animation from taskbar. Pointer hit-testing uses exactly
        // the same geometry, supplied by the no_std shell module.
        let rect = start_menu::panel(*layout, state.launcher_step);
        let (x, y, w, h) = (rect.x, rect.y, rect.width, rect.height);
        self.round_rect(x + 5, y + 7, w, h, 12, 0x071526);
        self.round_rect(x, y, w, h, 12,
                        if state.dark { 0x233850 } else { 0xf0faff });
        let fg = if state.dark { 0xffffff } else { 0x183144 };
        self.text(tr(state.language, TextId::Launcher), x + 18, y + 17, 2, fg);
        if h >= 110 {
            for (index, entry) in apps::BUILTIN_APPS.iter().enumerate() {
                let py = y + 55 + index * 24;
                if py + 14 >= y + h { break; }
                self.text(tr(state.language, entry.title_key), x + 38, py, 1, fg);
            }
        }
    }


    /// Composed above windows, taskbar and launcher but below the cursor.
    /// Menu::hit in shell/personalization.rs owns the identical geometry.
    fn desktop_context_menu(&self, state: &Desktop) {
        let Some(menu) = state.context_menu else { return };
        self.round_rect(menu.x + 4, menu.y + 4, menu.width, menu.height, 11, 0x07101b);
        self.round_rect(menu.x, menu.y, menu.width, menu.height, 11,
                        if state.dark { 0x20344c } else { 0xf6faff });
        for (i, title) in [TextId::Personalization, TextId::DesktopDisplay]
            .iter().enumerate()
        {
            let y = menu.y + 8 + i * 39;
            self.round_rect(menu.x + 7, y, menu.width.saturating_sub(14), 35, 7,
                            if state.dark { 0x2b4761 } else { 0xe2eff7 });
            self.round_rect(menu.x + 14, y + 7, 21, 21, 6, state.accent.rgb());
            self.text(if i == 0 { "P" } else { "D" }, menu.x + 21,
                      y + 14, 1, 0x091d2b);
            self.text(tr(state.language, *title), menu.x + 43,
                      y + 14, 1, if state.dark { 0xf0f7ff } else { 0x173047 });
        }
    }

    /// A small software cursor. Always rendered LAST so no window or
    /// launcher can cover it. Rectangle clipping keeps every pixel safe.
    fn cursor(&self, state: &Desktop) {
        if state.pointer.visible {
            self.draw_pointer(state.pointer.x, state.pointer.y, state.accent.rgb());
        }
    }

    fn draw_pointer(&self, x: usize, y: usize, accent: u32) {
        // Compact 12x20 pointer with a one-pixel dark outline, bright body and
        // teal accent on the arrow stem. Transparent cells are never written.
        // Cursor damage bounds in raster::Region remain exactly 12x20.
        const SHAPE: [&[u8]; 20] = [
            b"X", b"XX", b"XWX", b"XWWX", b"XWWWX",
            b"XWWWWX", b"XWWWWWX", b"XWWWWWWX", b"XWWWWWWWX",
            b"XWWWWWWWWX", b"XWWWWWXXXX", b"XWWXWWX",
            b"XWX XTWX", b"XX  XTWX", b"X   XTWX",
            b"    XTWX", b"     XX", b"", b"", b"",
        ];
        for (dy, row) in SHAPE.iter().enumerate() {
            for (dx, pixel) in row.iter().enumerate() {
                let color = match pixel {
                    b'X' => 0x061622,
                    b'W' => 0xf4fbff,
                    b'T' => accent,
                    _ => continue,
                };
                self.write_direct(
                    Region::new(x.saturating_add(dx), y.saturating_add(dy), 1, 1),
                    self.rgb(color),
                );
            }
        }
    }


}

/// Draw one entire desktop frame. The caller drives redraw on input events.
/// Returns false when the bootloader framebuffer cannot be used.
pub fn render(state: &Desktop) -> bool {
    let Some(fb) = limine::framebuffer() else { return false };
    let Some(layout) = Layout::for_size(fb.width, fb.height) else { return false };
    let buffered = raster::supports_backbuffer(fb.width, fb.height);
    BACKBUFFER_VALID.store(false, Ordering::Release);
    let screen = Screen { fb, buffered };
    screen.background(state);
    screen.wallpaper_label(state, &layout);
    screen.desktop_shortcuts(state, &layout);
    screen.desktop_folders(state, &layout);
    screen.top_panel(state, &layout);
    for app in state.windows.back_to_front() {
        if state.windows.window(app).visible() {
            screen.window(state, app, state.windows.rect(app), state.active() == Some(app));
        }
    }
    screen.taskbar(state, &layout);
    screen.dock(state, &layout);
    screen.launcher(state, &layout);
    screen.desktop_context_menu(state);

    if buffered {
        // Compose all GUI layers before a single complete screen transfer.
        screen.flush_region(Region::new(0, 0, screen.fb.width, screen.fb.height));
        BACKBUFFER_WIDTH.store(screen.fb.width, Ordering::Relaxed);
        BACKBUFFER_HEIGHT.store(screen.fb.height, Ordering::Relaxed);
        BACKBUFFER_VALID.store(true, Ordering::Release);
    }
    screen.cursor(state);
    true
}

/// Whether the most recent complete scene is cached in BSS.
pub fn has_backbuffer() -> bool {
    BACKBUFFER_VALID.load(Ordering::Acquire)
}

/// Fast path for pointer movement without changing windows/theme/launcher.
/// Restore the old and new cursor rectangles only (<=480 pixels total).
/// Returns false when no cached background is available, in which case the
/// caller MUST redraw the complete desktop.
pub fn redraw_cursor(previous: Pointer, state: &Desktop) -> bool {
    if !state.pointer.visible || !previous.visible || !has_backbuffer() {
        return false;
    }
    let Some(fb) = limine::framebuffer() else { return false };
    if !raster::supports_backbuffer(fb.width, fb.height)
        || BACKBUFFER_WIDTH.load(Ordering::Relaxed) != fb.width
        || BACKBUFFER_HEIGHT.load(Ordering::Relaxed) != fb.height
    {
        return false;
    }

    let screen = Screen { fb, buffered: true };
    if let Some(region) = Region::cursor_at(previous.x, previous.y,
                                            screen.fb.width, screen.fb.height) {
        screen.flush_region(region);
    }
    if let Some(region) = Region::cursor_at(state.pointer.x, state.pointer.y,
                                            screen.fb.width, screen.fb.height) {
        screen.flush_region(region);
    }
    screen.cursor(state);
    true
}

/// Draw setup wizard first, before Desktop. Returns false for unsupported
/// displays so the kernel can fall back to serial-only desktop.
pub fn render_setup(wizard: &Wizard) -> bool {
    let Some(fb) = limine::framebuffer() else { return false };
    if Layout::for_size(fb.width, fb.height).is_none() { return false; }
    let buffered = raster::supports_backbuffer(fb.width, fb.height);
    BACKBUFFER_VALID.store(false, Ordering::Release);
    let screen = Screen { fb, buffered };
    screen.setup_screen(wizard);
    if buffered {
        screen.flush_region(Region::new(0, 0, screen.fb.width, screen.fb.height));
        BACKBUFFER_WIDTH.store(screen.fb.width, Ordering::Relaxed);
        BACKBUFFER_HEIGHT.store(screen.fb.height, Ordering::Relaxed);
        BACKBUFFER_VALID.store(true, Ordering::Release);
    }
    if wizard.pointer.visible {
        screen.draw_pointer(wizard.pointer.x, wizard.pointer.y, 0x19d9bd);
    }
    true
}

/// Restore cursor rectangles for the setup screen without repainting cards.
pub fn redraw_setup_cursor(previous: SetupPointer, wizard: &Wizard) -> bool {
    if !previous.visible || !wizard.pointer.visible || !has_backbuffer() {
        return false;
    }
    let Some(fb) = limine::framebuffer() else { return false };
    if !raster::supports_backbuffer(fb.width, fb.height)
        || BACKBUFFER_WIDTH.load(Ordering::Relaxed) != fb.width
        || BACKBUFFER_HEIGHT.load(Ordering::Relaxed) != fb.height
    { return false }
    let screen = Screen { fb, buffered: true };
    if let Some(region) = Region::cursor_at(previous.x, previous.y,
                                            screen.fb.width, screen.fb.height) {
        screen.flush_region(region);
    }
    if let Some(region) = Region::cursor_at(wizard.pointer.x, wizard.pointer.y,
                                            screen.fb.width, screen.fb.height) {
        screen.flush_region(region);
    }
    screen.draw_pointer(wizard.pointer.x, wizard.pointer.y, 0x19d9bd);
    true
}

fn glyph_unicode(ch: char) -> [u8; 7] {
    if ch.is_ascii() {
        return glyph(ch as u8);
    }
    match ch {
        'А' => [14,17,17,31,17,17,17],
        'Б' => [31,16,16,30,17,17,30],
        'В' => [30,17,17,30,17,17,30],
        'Г' => [31,16,16,16,16,16,16],
        'Ґ' => [3,31,16,16,16,16,16],
        'Д' => [14,10,10,10,10,31,17],
        'Е' | 'Є' => [31,16,16,30,16,16,31],
        'Ж' => [21,21,14,4,14,21,21],
        'З' => [30,1,1,14,1,1,30],
        'И' => [17,17,19,21,25,17,17],
        'І' => [31,4,4,4,4,4,31],
        'Ї' => [10,0,31,4,4,4,31],
        'Й' => [10,4,17,19,21,25,17],
        'К' => [17,18,20,24,20,18,17],
        'Л' => [7,9,9,9,9,9,17],
        'М' => [17,27,21,21,17,17,17],
        'Н' => [17,17,17,31,17,17,17],
        'О' => [14,17,17,17,17,17,14],
        'П' => [31,17,17,17,17,17,17],
        'Р' => [30,17,17,30,16,16,16],
        'С' => [15,16,16,16,16,16,15],
        'Т' => [31,4,4,4,4,4,4],
        'У' => [17,17,17,15,1,1,30],
        'Ф' => [4,14,21,21,14,4,4],
        'Х' => [17,17,10,4,10,17,17],
        'Ц' => [17,17,17,17,17,31,1],
        'Ч' => [17,17,17,15,1,1,1],
        'Ш' => [21,21,21,21,21,21,31],
        'Щ' => [21,21,21,21,21,31,1],
        'Ь' => [16,16,16,30,17,17,30],
        'Ъ' => [24,8,8,14,9,9,14],
        'Ы' => [17,17,17,29,21,21,29],
        'Э' => [30,1,1,15,1,1,30],
        'Ё' => [10,0,31,16,30,16,31],
        'Ю' => [18,21,21,29,21,21,18],
        'Я' => [15,17,17,15,5,9,17],
        ' ' => [0;7],
        _ => [31,17,17,17,17,17,31],
    }
}

fn glyph(c: u8) -> [u8; 7] {
    match c {
        b'A' => [14,17,17,31,17,17,17],
        b'B' => [30,17,17,30,17,17,30],
        b'C' => [15,16,16,16,16,16,15],
        b'D' => [30,17,17,17,17,17,30],
        b'E' => [31,16,16,30,16,16,31],
        b'F' => [31,16,16,30,16,16,16],
        b'G' => [15,16,16,23,17,17,15],
        b'H' => [17,17,17,31,17,17,17],
        b'I' => [31,4,4,4,4,4,31],
        b'J' => [7,2,2,2,18,18,12],
        b'K' => [17,18,20,24,20,18,17],
        b'L' => [16,16,16,16,16,16,31],
        b'M' => [17,27,21,21,17,17,17],
        b'N' => [17,25,21,19,17,17,17],
        b'O' => [14,17,17,17,17,17,14],
        b'P' => [30,17,17,30,16,16,16],
        b'Q' => [14,17,17,17,21,18,13],
        b'R' => [30,17,17,30,20,18,17],
        b'S' => [15,16,16,14,1,1,30],
        b'T' => [31,4,4,4,4,4,4],
        b'U' => [17,17,17,17,17,17,14],
        b'V' => [17,17,17,17,17,10,4],
        b'W' => [17,17,17,21,21,21,10],
        b'X' => [17,17,10,4,10,17,17],
        b'Y' => [17,17,10,4,4,4,4],
        b'Z' => [31,1,2,4,8,16,31],
        b'0' => [14,17,19,21,25,17,14],
        b'1' => [4,12,4,4,4,4,14],
        b'2' => [14,17,1,2,4,8,31],
        b'3' => [30,1,1,14,1,1,30],
        b'4' => [2,6,10,18,31,2,2],
        b'5' => [31,16,16,30,1,1,30],
        b'6' => [14,16,16,30,17,17,14],
        b'7' => [31,1,2,4,8,8,8],
        b'8' => [14,17,17,14,17,17,14],
        b'9' => [14,17,17,15,1,1,14],
        b'.' => [0,0,0,0,0,12,12],
        b':' => [0,12,12,0,12,12,0],
        b'/' => [1,2,2,4,8,8,16],
        b'-' => [0,0,0,31,0,0,0],
        b'>' => [16,8,4,2,4,8,16],
        b'_' => [0,0,0,0,0,0,31],
        b'\'' => [4,4,2,0,0,0,0],
        b'!' => [4,4,4,4,4,0,4],
        b'?' => [14,17,1,2,4,0,4],
        b'=' => [0,31,0,31,0,0,0],
        b' ' => [0;7],
        _ => [0;7],
    }
}
