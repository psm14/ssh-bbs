use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Widget;

// Simple LCG RNG to avoid external deps
#[derive(Clone)]
struct Lcg(u64);
impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed)
    }
    fn next_u32(&mut self) -> u32 {
        // Numerical Recipes LCG constants
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 16) as u32
    }
    fn gen_range(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u32() % (hi - lo))
    }
    fn chance(&mut self, n: u32, d: u32) -> bool {
        (self.next_u32() % d) < n
    }
}

pub struct Life {
    pub width: usize,
    pub height: usize,
    cells: Vec<bool>,
    scratch: Vec<bool>,
    rng: Lcg,
    tick: u64,
}

impl Life {
    pub fn new(width: usize, height: usize) -> Self {
        let cap = width.saturating_mul(height);
        let mut me = Self {
            width,
            height,
            cells: vec![false; cap],
            scratch: vec![false; cap],
            rng: Lcg::new(0xC0FFEE ^ (width as u64) ^ ((height as u64) << 32)),
            tick: 0,
        };
        me.seed_initial();
        me
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        if width == self.width && height == self.height {
            return;
        }
        self.width = width;
        self.height = height;
        let cap = width.saturating_mul(height);
        self.cells = vec![false; cap];
        self.scratch = vec![false; cap];
        self.rng = Lcg::new(0xC0FFEE ^ (width as u64) ^ ((height as u64) << 32));
        self.tick = 0;
        self.seed_initial();
    }

    fn idx(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }
    pub fn get(&self, x: usize, y: usize) -> bool {
        self.cells[self.idx(x, y)]
    }
    pub fn set(&mut self, x: usize, y: usize, val: bool) {
        if x < self.width && y < self.height {
            let i = self.idx(x, y);
            self.cells[i] = val;
        }
    }

    pub fn clear(&mut self) {
        self.cells.fill(false);
    }

    pub fn step(&mut self) {
        let w = self.width as isize;
        let h = self.height as isize;
        for y in 0..h {
            for x in 0..w {
                let mut n = 0;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nx = x + dx;
                        let ny = y + dy;
                        if nx >= 0 && ny >= 0 && nx < w && ny < h {
                            if self.get(nx as usize, ny as usize) {
                                n += 1;
                            }
                        }
                    }
                }
                let alive = self.get(x as usize, y as usize);
                let next = (alive && (n == 2 || n == 3)) || (!alive && n == 3);
                let idx = (y as usize) * self.width + (x as usize);
                self.scratch[idx] = next;
            }
        }
        std::mem::swap(&mut self.cells, &mut self.scratch);
        self.tick = self.tick.wrapping_add(1);
    }

    pub fn maybe_spawn(&mut self) {
        // More frequent spawns to keep things active
        // Every ~30 ticks (~2.4s at 12 FPS), ~66% chance to spawn something
        if self.tick % 30 == 0 && self.rng.chance(2, 3) {
            let choice = self.rng.gen_range(0, 6);
            match choice {
                0 | 1 => self.spawn_glider_inward(),
                2 => self.spawn_lwss_inward(),
                _ => self.spawn_oscillator_random(),
            }
            // Occasionally do a second spawn for extra activity
            if self.rng.chance(1, 4) {
                self.spawn_oscillator_random();
            }
        }
    }

    pub fn seed_initial(&mut self) {
        self.clear();
        if self.width < 10 || self.height < 7 {
            return;
        }
        // Place more gliders and oscillators across the field
        let w = self.width as u32;
        let h = self.height as u32;
        // Corners
        let spots = [
            (2, 2, 0u8),
            (w.saturating_sub(6), 2, 1u8),
            (2, h.saturating_sub(6), 2u8),
            (w.saturating_sub(6), h.saturating_sub(6), 3u8),
        ];
        for (x, y, dir) in spots.into_iter() {
            self.seed_glider(x as usize, y as usize, dir);
        }
        // Along top and bottom rows, every ~12 cols
        let mut x = 2u32;
        while x + 6 < w {
            self.seed_glider(x as usize, 2usize, 2);
            self.seed_glider(x as usize, h.saturating_sub(6) as usize, 0);
            x += 12;
        }
        // A bunch of oscillators randomly placed
        for _ in 0..8 {
            self.spawn_oscillator_random();
        }
    }

    fn spawn_glider_inward(&mut self) {
        if self.width < 6 || self.height < 6 {
            return;
        }
        let side = self.rng.gen_range(0, 4); // 0=top,1=right,2=bottom,3=left
        match side {
            0 => {
                // top, move down
                let x = self.rng.gen_range(1, (self.width as u32).saturating_sub(6)) as usize;
                self.seed_glider(x, 1, 2); // down-right
            }
            1 => {
                // right, move left
                let y = self
                    .rng
                    .gen_range(1, (self.height as u32).saturating_sub(6))
                    as usize;
                let x = self.width.saturating_sub(6);
                self.seed_glider(x, y, 3); // up-left
            }
            2 => {
                // bottom, move up
                let x = self.rng.gen_range(1, (self.width as u32).saturating_sub(6)) as usize;
                let y = self.height.saturating_sub(6);
                self.seed_glider(x, y, 0); // up-right
            }
            _ => {
                // left, move right
                let y = self
                    .rng
                    .gen_range(1, (self.height as u32).saturating_sub(6))
                    as usize;
                self.seed_glider(1, y, 1); // down-left
            }
        }
    }

    fn spawn_lwss_inward(&mut self) {
        if self.width < 8 || self.height < 6 {
            return;
        }
        let side = self.rng.gen_range(0, 4);
        match side {
            0 => {
                let x = self.rng.gen_range(1, (self.width as u32).saturating_sub(7)) as usize;
                self.seed_lwss(x, 1, 2); // downward
            }
            1 => {
                let y = self
                    .rng
                    .gen_range(1, (self.height as u32).saturating_sub(5))
                    as usize;
                let x = self.width.saturating_sub(7);
                self.seed_lwss(x, y, 3); // leftward
            }
            2 => {
                let x = self.rng.gen_range(1, (self.width as u32).saturating_sub(7)) as usize;
                let y = self.height.saturating_sub(5);
                self.seed_lwss(x, y, 0); // upward
            }
            _ => {
                let y = self
                    .rng
                    .gen_range(1, (self.height as u32).saturating_sub(5))
                    as usize;
                self.seed_lwss(1, y, 1); // rightward
            }
        }
    }

    fn spawn_blinker_random(&mut self) {
        if self.width < 3 || self.height < 3 {
            return;
        }
        let x = self.rng.gen_range(2, (self.width as u32).saturating_sub(2)) as usize;
        let y = self
            .rng
            .gen_range(2, (self.height as u32).saturating_sub(2)) as usize;
        let d = if self.rng.chance(1, 2) { 0 } else { 1 };
        self.seed_blinker(x, y, d);
    }

    fn spawn_oscillator_random(&mut self) {
        let choice = self.rng.gen_range(0, 3);
        match choice {
            0 => self.spawn_blinker_random(),
            1 => self.spawn_toad_random(),
            _ => self.spawn_beacon_random(),
        }
    }

    fn spawn_toad_random(&mut self) {
        if self.width < 6 || self.height < 4 {
            return;
        }
        let x = self.rng.gen_range(2, (self.width as u32).saturating_sub(4)) as usize;
        let y = self
            .rng
            .gen_range(1, (self.height as u32).saturating_sub(3)) as usize;
        let dir = (self.rng.next_u32() % 2) as u8;
        self.seed_toad(x, y, dir);
    }

    fn spawn_beacon_random(&mut self) {
        if self.width < 6 || self.height < 6 {
            return;
        }
        let x = self.rng.gen_range(2, (self.width as u32).saturating_sub(5)) as usize;
        let y = self
            .rng
            .gen_range(2, (self.height as u32).saturating_sub(5)) as usize;
        self.seed_beacon(x, y);
    }

    pub fn seed_glider(&mut self, x: usize, y: usize, dir: u8) {
        // Base glider (moves down-right):
        // . # .
        // . . #
        // # # #
        let mut pts = [(1isize, 0isize), (2, 1), (0, 2), (1, 2), (2, 2)];
        // Rotate: 0=up-right,1=down-left,2=down-right,3=up-left
        let rot = match dir % 4 {
            0 => 90,
            1 => 270,
            2 => 0,
            _ => 180,
        };
        if rot != 0 {
            for p in &mut pts {
                // rotate around (1,1)
                let cx = p.0 - 1;
                let cy = p.1 - 1;
                let (rx, ry) = match rot {
                    90 => (-cy, cx),
                    180 => (-cx, -cy),
                    270 => (cy, -cx),
                    _ => (cx, cy),
                };
                p.0 = rx + 1;
                p.1 = ry + 1;
            }
        }
        for (dx, dy) in pts {
            self.set((x as isize + dx) as usize, (y as isize + dy) as usize, true);
        }
    }

    pub fn seed_blinker(&mut self, x: usize, y: usize, dir: u8) {
        // 3 in a line, horizontal if dir==0 else vertical
        if dir % 2 == 0 {
            for dx in 0..3 {
                self.set(x + dx, y, true);
            }
        } else {
            for dy in 0..3 {
                self.set(x, y + dy, true);
            }
        }
    }

    pub fn seed_lwss(&mut self, x: usize, y: usize, dir: u8) {
        // Lightweight spaceship (points chosen for rightward movement base)
        // Pattern width 5, height 4
        let base = [
            (1, 0),
            (4, 0),
            (0, 1),
            (0, 2),
            (4, 2),
            (0, 3),
            (1, 3),
            (2, 3),
            (3, 3),
        ];
        // Rotate: 0=up,1=right,2=down,3=left relative to base moving right
        let rot = (dir % 4) as i32;
        for (dx, dy) in base {
            let (mut rx, mut ry) = (dx as i32, dy as i32);
            for _ in 0..rot {
                // rotate around approx center (2,1)
                let cx = rx - 2;
                let cy = ry - 1;
                let nx = -cy;
                let ny = cx;
                rx = nx + 2;
                ry = ny + 1;
            }
            if rx >= 0 && ry >= 0 {
                self.set(x + rx as usize, y + ry as usize, true);
            }
        }
    }

    pub fn seed_toad(&mut self, x: usize, y: usize, dir: u8) {
        // Toad oscillator (period 2), base horizontal orientation
        // . ###
        // ## .
        let pts = [(1, 0), (2, 0), (3, 0), (0, 1), (1, 1), (2, 1)];
        if dir % 2 == 0 {
            for (dx, dy) in pts {
                self.set(x + dx, y + dy, true);
            }
        } else {
            // vertical rotation
            for (dx, dy) in pts {
                self.set(x + dy, y + dx, true);
            }
        }
    }

    pub fn seed_beacon(&mut self, x: usize, y: usize) {
        // Beacon oscillator (period 2): two 2x2 blocks at opposite corners
        for dy in 0..2 {
            for dx in 0..2 {
                self.set(x + dx, y + dy, true);
                self.set(x + 3 + dx, y + 3 + dy, true);
            }
        }
    }
}

pub struct LifeWidget<'a> {
    pub life: &'a Life,
    pub color: Color,
}

impl<'a> LifeWidget<'a> {
    pub fn new(life: &'a Life) -> Self {
        Self {
            life,
            color: Color::DarkGray,
        }
    }
}

impl<'a> Widget for LifeWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let w = self.life.width.min(area.width as usize);
        let h = self.life.height.min(area.height as usize);
        let style = Style::default().fg(self.color);
        for y in 0..h {
            for x in 0..w {
                if self.life.get(x, y) {
                    let cx = area.x + x as u16;
                    let cy = area.y + y as u16;
                    let cell = buf.get_mut(cx, cy);
                    cell.set_style(style);
                    cell.set_symbol("·");
                }
            }
        }
    }
}
