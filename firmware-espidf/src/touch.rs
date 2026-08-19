//! Driver tactile AXS15231B (I2C 0x3B) — lecture des points de contact.
//! Protocole : écrire la commande 0xb5 0xab 0xa5 0x5a … puis lire 14 octets.
//! data[1] = nb contacts ; point 1 = ((d[2]&0x0F)<<8)|d[3], ((d[4]&0x0F)<<8)|d[5].

use esp_idf_hal::i2c::I2cDriver;

const TOUCH_ADDR: u8 = 0x3B;
const READ_CMD: [u8; 11] = [
    0xb5, 0xab, 0xa5, 0x5a, 0x00, 0x00, 0x00, 0x0e, 0x00, 0x00, 0x00,
];

pub struct Touch<'d> {
    i2c: I2cDriver<'d>,
}

pub struct TouchPoint {
    pub x: u16,
    pub y: u16,
}

impl<'d> Touch<'d> {
    pub fn new(i2c: I2cDriver<'d>) -> Self {
        Self { i2c }
    }

    /// Lit l'état du tactile → Some(point) si un contact est actif.
    pub fn read(&mut self) -> Option<TouchPoint> {
        if self.i2c.write(TOUCH_ADDR, &READ_CMD, 1000).is_err() {
            return None;
        }
        let mut data = [0u8; 14];
        if self.i2c.read(TOUCH_ADDR, &mut data, 1000).is_err() {
            return None;
        }
        let touches = data[1];
        // État idle = 0x70 (112) partout ; valide = 1..2 contacts
        if touches == 0 || touches > 2 {
            return None;
        }
        let raw_x = (((data[2] & 0x0F) as u16) << 8) | data[3] as u16;
        let raw_y = (((data[4] & 0x0F) as u16) << 8) | data[5] as u16;
        // Filtre les valeurs invalides (273 = pas de contact, >4000 = parasite)
        if (raw_x == 273 && raw_y == 273) || raw_x > 4000 || raw_y > 4000 {
            return None;
        }
        Some(TouchPoint {
            x: raw_x.min(320),
            y: raw_y.min(480),
        })
    }
}
