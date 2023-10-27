use std::io::Read;

use anyhow::Ok;
use byteorder::{LittleEndian, ReadBytesExt};

const N: usize = 4096;
const F: usize = 60;
const THRESHOLD: usize = 2;
const NIL: usize = N;

const N_CHAR: usize = 256 - THRESHOLD + F;
const T: usize = N_CHAR * 2 - 1;
const R: usize = T - 1;
const MAX_FREQ: u32 = 0x4000;

const P_LEN: [u8; 64] = [
    0x00, 0x20, 0x30, 0x40, 0x50, 0x58, 0x60, 0x68, 0x70, 0x78, 0x80, 0x88, 0x90, 0x94, 0x98, 0x9C,
    0xA0, 0xA4, 0xA8, 0xAC, 0xB0, 0xB4, 0xB8, 0xBC, 0xC0, 0xC2, 0xC4, 0xC6, 0xC8, 0xCA, 0xCC, 0xCE,
    0xD0, 0xD2, 0xD4, 0xD6, 0xD8, 0xDA, 0xDC, 0xDE, 0xE0, 0xE2, 0xE4, 0xE6, 0xE8, 0xEA, 0xEC, 0xEE,
    0xF0, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFB, 0xFC, 0xFD, 0xFE, 0xFF,
];

const P_CODE: [u8; 64] = [
    0x03, 0x04, 0x04, 0x04, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x06, 0x06, 0x06, 0x06,
    0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07,
    0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07,
    0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08,
];

const D_CODE: [u8; 256] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02,
    0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03,
    0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05,
    0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07,
    0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x09, 0x09, 0x09, 0x09, 0x09, 0x09, 0x09, 0x09,
    0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0B, 0x0B, 0x0B, 0x0B, 0x0B, 0x0B, 0x0B, 0x0B,
    0x0C, 0x0C, 0x0C, 0x0C, 0x0D, 0x0D, 0x0D, 0x0D, 0x0E, 0x0E, 0x0E, 0x0E, 0x0F, 0x0F, 0x0F, 0x0F,
    0x10, 0x10, 0x10, 0x10, 0x11, 0x11, 0x11, 0x11, 0x12, 0x12, 0x12, 0x12, 0x13, 0x13, 0x13, 0x13,
    0x14, 0x14, 0x14, 0x14, 0x15, 0x15, 0x15, 0x15, 0x16, 0x16, 0x16, 0x16, 0x17, 0x17, 0x17, 0x17,
    0x18, 0x18, 0x19, 0x19, 0x1A, 0x1A, 0x1B, 0x1B, 0x1C, 0x1C, 0x1D, 0x1D, 0x1E, 0x1E, 0x1F, 0x1F,
    0x20, 0x20, 0x21, 0x21, 0x22, 0x22, 0x23, 0x23, 0x24, 0x24, 0x25, 0x25, 0x26, 0x26, 0x27, 0x27,
    0x28, 0x28, 0x29, 0x29, 0x2A, 0x2A, 0x2B, 0x2B, 0x2C, 0x2C, 0x2D, 0x2D, 0x2E, 0x2E, 0x2F, 0x2F,
    0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x3B, 0x3C, 0x3D, 0x3E, 0x3F,
];
const D_LEN: [u8; 256] = [
    0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03,
    0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03,
    0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04,
    0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04,
    0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04,
    0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05,
    0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05,
    0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05,
    0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05,
    0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06,
    0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06,
    0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06,
    0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07,
    0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07,
    0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07,
    0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08,
];

struct Decoder<R: Read> {
    reader: R,
    output: Vec<u8>,
    out_position: usize,
    text_buf: [u8; N + F],
    match_position: i32,
    match_length: i32,
    lson: [i32; N + 1],
    rson: [i32; N + 257],
    dad: [i32; N + 1],
    code: u32,
    len: u32,
    /// frequency table
    freq: [u32; T + 1],
    /// pointers to parent nodes,
    /// except for the elements `[T..T + N_CHAR - 1]`
    /// which are used to get the positions of leaves
    /// corresponding to the codes.
    parent: [u32; T + N_CHAR + 1],
    son: [u32; T],
    tim_size: u32,
    get_buf: u32,
    get_len: u32,
    put_buf: u32,
    put_len: u32,
}

impl<R: Read> Decoder<R> {
    fn new(reader: R) -> Decoder<R> {
        Decoder {
            reader,
            output: Vec::new(),
            out_position: 0,
            text_buf: [0; N + F],
            match_position: 0,
            match_length: 0,
            lson: [0; N + 1],
            rson: [0; N + 257],
            dad: [0; N + 1],
            code: 0,
            len: 0,
            freq: [0; T + 1],
            parent: [0; T + N_CHAR + 1],
            son: [0; T],
            tim_size: 0,
            get_buf: 0,
            get_len: 0,
            put_buf: 0,
            put_len: 0,
        }
    }

    fn decode(&mut self) -> anyhow::Result<()> {
        let text_size = self.reader.read_u32::<LittleEndian>()?;

        if text_size == 0 {
            return Ok(());
        }

        self.init_output(text_size)?;

        self.start_huff();
        for i in 0..(N - F) {
            self.text_buf[i] = 0x20;
        }

        let mut r = N - F;

        let mut count = 0;
        while count < text_size {
            let mut c = self.decode_char();
            if c < 256 {
                self.putb(c);
                self.text_buf[r] = c as u8;
                r = (r + 1) & (N - 1);
                count += 1;
            } else {
                let pos = self.decode_position() as usize;
                let i = r.wrapping_sub(pos + 1) & (N - 1);
                let j = c as usize - 255 + THRESHOLD;
                for k in 0..j {
                    c = self.text_buf[(i + k) & (N - 1)] as u32;
                    self.putb(c);
                    self.text_buf[r] = c as u8;
                    r = (r + 1) & (N - 1);
                    count += 1;
                }
            }
        }

        self.tim_size = count;

        Ok(())
    }

    fn init_output(&mut self, text_size: u32) -> anyhow::Result<()> {
        self.output = Vec::with_capacity(text_size as usize);
        self.out_position = 0;

        Ok(())
    }

    fn getb(&mut self) -> u32 {
        self.reader.read_u8().map(|x| x as u32).unwrap_or_default()
    }

    fn putb(&mut self, c: u32) {
        self.output.push((c & 0xFF) as u8);
    }

    fn start_huff(&mut self) {
        for i in 0..N_CHAR {
            self.freq[i] = 1;
            self.son[i] = (i + T) as u32;
            self.parent[i + T] = i as u32;
        }

        let mut i = 0;
        let mut j = N_CHAR;
        while j <= R {
            self.freq[j] = self.freq[i] + self.freq[i + 1];
            self.son[j] = i as u32;
            self.parent[i] = j as u32;
            self.parent[i + 1] = j as u32;
            i += 2;
            j += 1;
        }
        self.freq[T] = 0xFFFF;
        self.parent[R] = 0;
    }

    fn decode_char(&mut self) -> u32 {
        log::trace!("decode_char");
        let mut c = self.son[R];

        while (c as usize) < T {
            c += self.get_bit();
            c = self.son[c as usize];
        }

        c -= T as u32;

        self.update(c);

        c
    }

    fn decode_position(&mut self) -> u32 {
        log::trace!("decode_position");
        let mut i = self.get_byte();
        let c = (D_CODE[i as usize] as u32) << 6;
        let mut j = D_LEN[i as usize];

        j -= 2;
        while j > 0 {
            i = (i << 1) + self.get_bit();
            j -= 1;
        }

        c | (i & 0x3F)
    }

    fn get_bit(&mut self) -> u32 {
        let mut i = 0;

        while self.get_len <= 8 {
            i = self.getb();
            self.get_buf |= i << (8 - self.get_len);
            self.get_len += 8;
        }

        i = self.get_buf;
        self.get_buf <<= 1;
        self.get_len -= 1;

        (i & 0x8000) >> 15
    }

    fn get_byte(&mut self) -> u32 {
        let mut i = 0;

        while self.get_len <= 8 {
            i = self.getb();
            self.get_buf |= i << (8 - self.get_len);
            self.get_len += 8;
        }

        i = self.get_buf;
        self.get_buf <<= 8;
        self.get_len -= 8;

        (i & 0xFF00) >> 8
    }

    fn reconst(&mut self) {
        let mut j = 0;
        for i in 0..T {
            if (self.son[i] as usize) >= T {
                self.freq[j] = (self.freq[i] + 1) / 2;
                self.son[j] = self.son[i];
                j += 1;
            }
        }

        let mut i = 0;
        for j in N_CHAR..T {
            let mut k = i + 1;
            self.freq[j] = self.freq[i] + self.freq[k];
            let f = self.freq[j];
            k = j - 1;
            while f < self.freq[k] {
                k -= 1;
            }
            k += 1;
            let l = j - k;

            self.freq.copy_within(k..(k + l), k + 1);
            self.freq[k] = f;

            self.son.copy_within(k..(k + l), k + 1);
            self.son[k] = i as u32;

            i += 2;
        }

        for i in 0..(T as u32) {
            let k = self.son[i as usize] as usize;

            self.parent[k] = i;
            if k < T {
                self.parent[k + 1] = i;
            }
        }
    }

    fn update(&mut self, mut c: u32) {
        if self.freq[R] == MAX_FREQ {
            self.reconst();
        }

        c = self.parent[c as usize + T];

        loop {
            self.freq[c as usize] += 1;
            let k = self.freq[c as usize];

            let mut l = c + 1;
            if k > self.freq[l as usize] {
                l += 1;
                while k > self.freq[l as usize] {
                    l += 1;
                }
                l -= 1;

                self.freq[c as usize] = self.freq[l as usize];
                self.freq[l as usize] = k;

                let i = self.son[c as usize];
                self.parent[i as usize] = l;
                if (i as usize) < T {
                    self.parent[i as usize + 1] = l;
                }

                let j = self.son[l as usize];
                self.son[l as usize] = i;

                self.parent[j as usize] = c;
                if (j as usize) < T {
                    self.parent[j as usize + 1] = c;
                }
                self.son[c as usize] = j;

                c = l;
            }

            c = self.parent[c as usize];

            if c == 0 {
                break;
            }
        }
    }
}

pub fn decompress<R: Read>(reader: R) -> anyhow::Result<Vec<u8>> {
    let mut decoder = Decoder::new(reader);
    decoder.decode()?;
    Ok(decoder.output)
}
