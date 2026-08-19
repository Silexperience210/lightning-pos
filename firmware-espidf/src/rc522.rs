//! Driver MFRC522 (RC522) en SPI — lecture carte NTAG (Bolt Card).
//!
//! Protocole (datasheet NXP MFRC522 + lib Arduino de référence) :
//! - SPI mode 0, ~4 MHz. Octet d'adresse : bit7 = lecture(1)/écriture(0),
//!   bits[6:1] = adresse registre décalée de 1, bit0 = 0.
//!   Écriture reg `r` → `(r << 1) & 0x7E`, lecture → `((r << 1) & 0x7E) | 0x80`.
//! - Transceive : flush FIFO, écrit les données, commande Transceive, StartSend,
//!   puis poll ComIrqReg sur RxIRq/IdleIRq (succès) et TimerIRq (timeout).
//!
//! NOTE (bits d'interruption) : la consigne initiale indiquait
//! « bit6 = RxIRq, bit3 = timer, bit0 = idle ». Ces positions ne correspondent
//! PAS à la datasheet NXP MFRC522 (vérifié sur le PDF officiel) ni à la lib
//! Arduino éprouvée. Les vraies positions (ComIrqReg, adresse 04h) sont :
//!   bit6=TxIRq, bit5=RxIRq, bit4=IdleIRq, bit3=HiAlertIRq,
//!   bit2=LoAlertIRq, bit1=ErrIRq, bit0=TimerIRq.
//! On utilise donc RxIRq=0x20, IdleIRq=0x10, TimerIRq=0x01.

use esp_idf_hal::gpio::{Output, PinDriver};
use esp_idf_hal::spi::{Operation, SpiDeviceDriver, SpiDriver};

// ---------------------------------------------------------------------------
// Registres (adresses logiques ; décalées dans l'octet d'adresse SPI).
// ---------------------------------------------------------------------------
const COMMAND_REG: u8 = 0x01;
const COMIEN_REG: u8 = 0x02;
const COMIRQ_REG: u8 = 0x04;
const DIV_IRQ_REG: u8 = 0x05;
const ERROR_REG: u8 = 0x06;
const STATUS2_REG: u8 = 0x08;
const FIFO_DATA_REG: u8 = 0x09;
const FIFO_LEVEL_REG: u8 = 0x0A;
const BIT_FRAMING_REG: u8 = 0x0D;
#[allow(dead_code)]
const COLL_REG: u8 = 0x0E;
const MODE_REG: u8 = 0x11;
const TX_MODE_REG: u8 = 0x12;
const RX_MODE_REG: u8 = 0x13;
const TX_CONTROL_REG: u8 = 0x14;
const TX_ASK_REG: u8 = 0x15;
#[allow(dead_code)]
const RF_CFG_REG: u8 = 0x26;
const T_MODE_REG: u8 = 0x2A;
const T_PRESCALER_REG: u8 = 0x2B;
const T_RELOAD_REG_H: u8 = 0x2C;
const T_RELOAD_REG_L: u8 = 0x2D;
const VERSION_REG: u8 = 0x37;
// Registres de résultat CRC : 0x21 = CRCResultRegM (MSB), 0x22 = CRCResultRegL (LSB).
// (0x38/0x39 étaient AnalogTestReg/TestDAC1Reg — le self-test lisait du bruit.)
const CRC_RESULT_REG_M: u8 = 0x21;
const CRC_RESULT_REG_L: u8 = 0x22;

// ---------------------------------------------------------------------------
// Commandes (CommandReg).
// ---------------------------------------------------------------------------
const CMD_IDLE: u8 = 0x00;
#[allow(dead_code)]
const CMD_CALC_CRC: u8 = 0x03;
const CMD_TRANSCEIVE: u8 = 0x0C;
/// MFAuthent : authentification Crypto-1 MIFARE Classic (datasheet §10.3.1.9).
const CMD_AUTHENT: u8 = 0x0E;
const CMD_SOFT_RESET: u8 = 0x0F;

// ---------------------------------------------------------------------------
// MIFARE Classic (Crypto-1).
// ---------------------------------------------------------------------------
/// AUTHENT1A : authentification avec la clé A du secteur (0x61 = clé B).
const PICC_AUTHENT1A: u8 = 0x60;
/// Bit MFCrypto1On de Status2Reg : 1 = la liaison est chiffrée Crypto-1.
const STATUS2_MFCRYPTO1_ON: u8 = 0x08;
/// Clé A « sortie d'usine » d'un badge vierge.
const KEY_DEFAULT: [u8; 6] = [0xFF; 6];
/// Clé A publique NFC Forum des secteurs NDEF d'une carte formatée NDEF
/// (NFC Forum Type 2 Tag Mapping for MIFARE Classic, AN1304).
const KEY_NFC_FORUM: [u8; 6] = [0xD3, 0xF7, 0xD3, 0xF7, 0xD3, 0xF7];
/// Clé A publique du secteur 0 (MIFARE Application Directory, AN10787).
const KEY_MAD: [u8; 6] = [0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5];

// ---------------------------------------------------------------------------
// Bits d'interruption (ComIrqReg) — datasheet NXP MFRC522 (adresse 04h).
// ---------------------------------------------------------------------------
const IRQ_RX: u8 = 0x20; // RxIRq   : fin de réception (données dispo dans FIFO)
const IRQ_IDLE: u8 = 0x10; // IdleIRq  : commande terminée
const IRQ_TIMER: u8 = 0x01; // TimerIRq : timer expiré (aucune réponse)

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------
pub struct Rc522<'d> {
    spi: SpiDeviceDriver<'d, SpiDriver<'d>>,
    rst: PinDriver<'d, Output>,
    /// Timeout matériel courant en ms (suit le reload du timer, cf. `set_timer_ms`).
    /// Sert aussi de borne au polling logiciel de ComIrqReg.
    timeout_ms: u32,
    /// Numéro de bloc ISO-DEP (T=CL) : bascule 0/1 à chaque I-block échangé.
    iso_block: u8,
    /// UID de la dernière carte sélectionnée. L'authentification Crypto-1
    /// (MIFARE Classic) en a besoin, et une auth ratée oblige à resélectionner
    /// la carte : on garde donc l'UID sous la main pendant toute la session.
    last_uid: Vec<u8>,
}

impl<'d> Rc522<'d> {
    pub fn new(spi: SpiDeviceDriver<'d, SpiDriver<'d>>, rst: PinDriver<'d, Output>) -> Self {
        Self {
            spi,
            rst,
            timeout_ms: 25,
            iso_block: 0,
            last_uid: Vec::new(),
        }
    }

    /// Réinitialise le MFRC522 (équivalent PCD_Init) : hard reset RST,
    /// SoftReset, config timer, TxASK/Mode, antenne ON.
    pub fn init(&mut self) -> Result<(), String> {
        // Hard reset : RST bas 50 ms → haut → attente 50 ms.
        self.rst.set_low().map_err(|e| format!("rst low: {e}"))?;
        std::thread::sleep(std::time::Duration::from_millis(50));
        self.rst.set_high().map_err(|e| format!("rst high: {e}"))?;
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Soft reset + attente.
        self.write_reg(COMMAND_REG, CMD_SOFT_RESET)?;
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Timer : TAuto + prescaler 0xA9, reload 0x03E8 (~25 ms).
        self.write_reg(T_MODE_REG, 0x80)?;
        self.write_reg(T_PRESCALER_REG, 0xA9)?;
        self.set_timer_ms(25)?;

        // Modulation ASK 100 % + mode transmetteur/récepteur.
        self.write_reg(TX_ASK_REG, 0x40)?;
        self.write_reg(MODE_REG, 0x3D)?;

        // Mode réception : 106 kbps, pas de CRC auto en RX (PCD_Init Arduino écrit
        // RxModeReg = 0x00). Le défaut après reset est déjà 0x00 mais on le force.
        self.write_reg(RX_MODE_REG, 0x00)?;
        // Gain récepteur : défaut silicium = 0x48 (RxGain 100b = 33 dB). La lib
        // Arduino ne touche pas RFCfgReg et laisse donc 0x48. 0x00 = 18 dB (plancher)
        // rend le récepteur sourd pour une NTAG à antenne faible ; 0x70 = 48 dB
        // sature avec une carte posée dessus (UID corrompu). 0x48 est le bon point.
        self.write_reg(RF_CFG_REG, 0x48)?;

        // Antenne ON.
        self.write_reg(TX_CONTROL_REG, 0x83)?;

        println!("[RC522] init OK");
        // Debug : CRC_A matériel de [0x93, 0x20] et [0x30, 0x04] (sans carte).
        match self.calc_crc(&[0x93, 0x20]) {
            Ok(c) => println!("[RC522] calc_crc(93 20) = {:02X?}", c),
            Err(e) => println!("[RC522] calc_crc err: {e}"),
        }
        match self.calc_crc(&[0x30, 0x04]) {
            Ok(c) => println!("[RC522] calc_crc(30 04) = {:02X?}", c),
            Err(e) => println!("[RC522] calc_crc err: {e}"),
        }
        Ok(())
    }

    /// Règle le timeout matériel de réception (TReloadReg).
    /// Avec TPrescalerReg = 0xA9, la période du timer est
    /// (2*0xA9+1)/13.56 MHz = 25 µs → reload = ms * 40.
    /// 25 ms suffit pour REQA/anticollision/READ ; les échanges T=CL
    /// (ISO14443-4) demandent bien plus (FWT + WTX).
    fn set_timer_ms(&mut self, ms: u32) -> Result<(), String> {
        let reload = (ms * 40).clamp(1, 0xFFFF) as u16 - 1;
        self.write_reg(T_RELOAD_REG_H, (reload >> 8) as u8)?;
        self.write_reg(T_RELOAD_REG_L, reload as u8)?;
        self.timeout_ms = ms;
        Ok(())
    }

    /// Lit le registre VersionReg (0x37). v2 → 0x92, v1 → 0x91,
    /// 0x00/0xFF → module muet (non câblé / SPI HS).
    pub fn get_version(&mut self) -> Result<u8, String> {
        let v = self.read_reg(VERSION_REG)?;
        println!("[RC522] VersionReg = 0x{:02X}", v);
        Ok(v)
    }

    /// Coupe réellement l'alimentation de la carte entre deux scans : soft reset
    /// complet du MFRC522 (CommandReg = SoftReset) → les drivers d'antenne
    /// retombent à leur défaut (OFF) → le champ RF s'effondre → la carte se
    /// dépower → sa session SDM (et son compteur anti-replay) repart à zéro au
    /// tap suivant. Le simple toggle TxControlReg ne suffit pas sur le clone
    /// FM17522 (le champ ne s'effondre pas franchement).
    fn power_cycle(&mut self) -> Result<(), String> {
        // Soft reset du chip (équivalent au reset de init, sans le hard reset RST).
        self.write_reg(COMMAND_REG, CMD_SOFT_RESET)?;
        std::thread::sleep(std::time::Duration::from_millis(50));
        // Re-configuration identique à init().
        self.write_reg(T_MODE_REG, 0x80)?;
        self.write_reg(T_PRESCALER_REG, 0xA9)?;
        self.set_timer_ms(25)?;
        self.write_reg(TX_ASK_REG, 0x40)?;
        self.write_reg(MODE_REG, 0x3D)?;
        self.write_reg(RX_MODE_REG, 0x00)?;
        self.write_reg(RF_CFG_REG, 0x48)?;
        self.write_reg(TX_CONTROL_REG, 0x83)?;
        // Guard time avant REQA/WUPA (boot de la carte).
        std::thread::sleep(std::time::Duration::from_millis(20));
        Ok(())
    }

    /// Détecte une carte NTAG (Bolt Card), lit sa mémoire et extrait l'URI
    /// NDEF (LNURL-withdraw). `Ok(None)` = aucune carte.
    pub fn read_ndef_uri(&mut self) -> Result<Option<String>, String> {
        // Diagnostic : relire l'état antenne/gain avant le scan. Si TxControl n'est
        // plus 0x83, le chip s'est reset depuis init() → RST/alim à vérifier.
        let tx = self.read_reg(TX_CONTROL_REG).unwrap_or(0);
        let rf = self.read_reg(RF_CFG_REG).unwrap_or(0);
        let v = self.read_reg(VERSION_REG).unwrap_or(0);
        println!("[RC522] scan: TxControl=0x{tx:02X} RFCfg=0x{rf:02X} Version=0x{v:02X}");
        // Power-cycle complet : la NTAG424 garde sa session SDM tant qu'elle est
        // alimentée, et son compteur anti-replay ne s'incrémente qu'UNE fois par
        // session (AN12196 §3.2). Sans coupure réelle du champ, chaque tap est le
        // replay du premier → « already used ». Le soft reset garantit l'extinction.
        self.power_cycle()?;
        // NB : un REQA (0x26) de diagnostic envoyé ici marchait (preuve que le
        // power-cycle reboote bien la carte), mais il mettait la carte en READY
        // et le WUPA suivant échouait → détection morte. Le WUPA seul suffit
        // (il réveille aussi les cartes HALT) et fonctionne sur carte fraîche.
        // 1) WUPA (trame 7 bits) → ATQA.
        // WUPA (0x52) et non REQA (0x26) : REQA n'est entendu que par une carte
        // à l'état IDLE. Après un cycle de lecture on laisse la carte à l'état
        // HALT (S(DESELECT) en fin de lecture Type 4), et seul WUPA l'en sort.
        // Avec REQA, un second passage de la boucle de scan ne verrait plus rien.
        let atqa = match self.transceive(&[0x52], 7, false)? {
            Some(r) if r.len() >= 2 => {
                let a = r[..2].to_vec();
                println!("[RC522] WUPA ATQA={:02X?}", a);
                a
            }
            Some(r) => {
                println!("[RC522] WUPA réponse courte {:02X?}", r);
                return Ok(None);
            }
            None => {
                println!("[RC522] WUPA timeout (pas de carte)");
                return Ok(None);
            }
        };

        // 2) Anticollision + select (cascade complet) → UID + SAK.
        let (uid, sak) = match self.select_card()? {
            Some(x) => x,
            None => return Ok(None),
        };
        self.last_uid = uid.clone();
        println!(
            "[RC522] carte UID={:02X?} SAK=0x{:02X} ATQA={:02X?}",
            uid, sak, atqa
        );

        // 3) Aiguillage selon le SAK — c'est LE point qui manquait.
        // SAK bit 0x20 = carte conforme ISO/IEC 14443-4 (cf. PICC_GetType de la lib
        // Arduino : `case 0x20: return PICC_TYPE_ISO_14443_4`). Ici ATQA=0x0344 +
        // SAK=0x20 + UID 7 octets commençant par 0x04 (NXP) = NTAG 424 DNA / DESFire,
        // c'est-à-dire exactement ce qu'est une Bolt Card — et PAS une NTAG213.
        //
        // Ces cartes n'implémentent pas la commande propriétaire MIFARE READ (0x30) :
        // à l'état ACTIVE elles l'ignorent silencieusement, d'où le TimerIRq observé.
        // La mémoire NDEF s'y lit en NFC Forum Type 4 : RATS → T=CL → APDU ISO7816.
        // (La lib Arduino elle-même refuse de dumper ce type : « Dumping memory
        // contents not implemented for that PICC type. »)
        if sak & 0x20 != 0 {
            println!(
                "[RC522] carte ISO14443-4 (SAK=0x{sak:02X}) → lecture NDEF Type 4 (RATS + APDU)"
            );
            return self.read_ndef_type4();
        }
        if sak == 0x00 {
            println!(
                "[RC522] carte Type 2 (NTAG/MIFARE Ultralight, SAK=0x00) → lecture READ (0x30)"
            );
            return self.read_ndef_type2();
        }
        // MIFARE Classic : SAK 0x08 = 1K, 0x18 = 4K, 0x88 = 1K à UID « NXP » (bit 7
        // positionné par certaines cartes). La lecture passe par une authentification
        // Crypto-1 secteur par secteur ; READ (0x30) est ensuite identique au Type 2,
        // mais chiffré par le MFRC522. Seuls les secteurs 0..15 (4 blocs) sont
        // adressés, ce qui reste valable sur une 4K (ses 32 premiers secteurs font
        // aussi 4 blocs) car le MAD1 ne décrit que les secteurs 1..15.
        if sak == 0x08 || sak == 0x18 || sak == 0x88 {
            println!(
                "[RC522] carte MIFARE Classic (SAK=0x{sak:02X}) → auth Crypto-1 + READ (0x30)"
            );
            return self.read_ndef_mifare_classic();
        }
        println!("[RC522] SAK=0x{sak:02X} non supporté");
        Ok(None)
    }

    // -----------------------------------------------------------------------
    // Accès SPI bas niveau
    // -----------------------------------------------------------------------
    fn write_reg(&mut self, reg: u8, val: u8) -> Result<(), String> {
        let addr = (reg << 1) & 0x7E;
        self.spi
            .write(&[addr, val])
            .map_err(|e| format!("write_reg 0x{reg:02X}: {e}"))
    }

    fn read_reg(&mut self, reg: u8) -> Result<u8, String> {
        let addr = ((reg << 1) & 0x7E) | 0x80;
        let mut rx = [0u8; 2];
        let tx = [addr, 0x00];
        self.spi
            .transaction(&mut [Operation::Transfer(&mut rx[..], &tx[..])])
            .map_err(|e| format!("read_reg 0x{reg:02X}: {e}"))?;
        Ok(rx[1])
    }

    /// Écrit plusieurs octets dans la FIFO en une seule transaction SPI
    /// (adresse FIFODataReg puis les données).
    fn write_fifo(&mut self, data: &[u8]) -> Result<(), String> {
        let addr = (FIFO_DATA_REG << 1) & 0x7E;
        let mut buf = Vec::with_capacity(1 + data.len());
        buf.push(addr);
        buf.extend_from_slice(data);
        self.spi.write(&buf).map_err(|e| format!("write_fifo: {e}"))
    }

    /// Lit `n` octets de la FIFO — lecture octet par octet (le clone FM17522 ne
    /// supporte pas fiablement la lecture en rafale : les octets après le 1er
    /// reviennent à 0). Chaque octet renvoie l'adresse de lecture + 1 octet.
    fn read_fifo(&mut self, n: usize) -> Result<Vec<u8>, String> {
        let addr = ((FIFO_DATA_REG << 1) & 0x7E) | 0x80;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let mut rx = [0u8; 2];
            let tx = [addr, 0x00];
            self.spi
                .transaction(&mut [Operation::Transfer(&mut rx[..], &tx[..])])
                .map_err(|e| format!("read_fifo: {e}"))?;
            out.push(rx[1]);
        }
        Ok(out)
    }

    /// Calcule le CRC_A matériel (commande CalcCRC) → [octet bas, octet haut].
    fn calc_crc(&mut self, data: &[u8]) -> Result<[u8; 2], String> {
        self.write_reg(COMMAND_REG, CMD_IDLE)?;
        self.write_reg(DIV_IRQ_REG, 0x04)?; // CRCIRq
        self.write_reg(COMIRQ_REG, 0x7F)?;
        self.write_reg(FIFO_LEVEL_REG, 0x80)?; // flush FIFO
        self.write_fifo(data)?;
        self.write_reg(COMMAND_REG, CMD_CALC_CRC)?;
        for _ in 0..50 {
            let irq = self.read_reg(DIV_IRQ_REG)?;
            if irq & 0x04 != 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        // Termine la commande CalcCRC avant de lire le résultat.
        self.write_reg(COMMAND_REG, CMD_IDLE)?;
        let lo = self.read_reg(CRC_RESULT_REG_L)?;
        let hi = self.read_reg(CRC_RESULT_REG_M)?;
        Ok([lo, hi])
    }

    // -----------------------------------------------------------------------
    // Transceive générique (équivalent PCD_CommunicateWithPICC / Transceive).
    // `last_bits` = 0 (octets pleins) ou 7 (trame courte REQA).
    // Retourne Some(reponse) si des données sont reçues, None si timeout.
    // -----------------------------------------------------------------------
    fn transceive(
        &mut self,
        data: &[u8],
        last_bits: u8,
        use_crc: bool,
    ) -> Result<Option<Vec<u8>>, String> {
        // RxAlign (BitFramingReg[6:4]) reste 0 : pour une lecture propre (une seule
        // carte, pas de collision) la réponse est alignée sur l'octet. La lib Arduino
        // ne met rxAlign ≠ 0 que pendant la résolution de collision (PICC_Select).
        let rx_align: u8 = 0;
        // Interruptions nécessaires : IRqInv + TxIEn + RxIEn + IdleIEn + LoAlertIEn + ErrIEn + TimerIEn.
        self.write_reg(COMIEN_REG, 0x77 | 0x80)?;
        // Stoppe toute commande en cours.
        self.write_reg(COMMAND_REG, CMD_IDLE)?;
        // Efface les 7 bits d'interruption.
        self.write_reg(COMIRQ_REG, 0x7F)?;
        // Flush FIFO (bit7 = FlushBuffer).
        self.write_reg(FIFO_LEVEL_REG, 0x80)?;
        // TxCRCEn (bit 7 de TxModeReg) : CRC auto matériel pendant l'émission.
        // PEU FIABLE sur certains clones (VersionReg 0x82) → on préfère calculer le
        // CRC_A en logiciel (crc_a) et appeler transceive(..., use_crc=false).
        self.write_reg(TX_MODE_REG, if use_crc { 0x80 } else { 0x00 })?;
        // Données → FIFO.
        self.write_fifo(data)?;
        // Framing = (RxAlign << 4) | TxLastBits, sans StartSend.
        let framing = ((rx_align & 0x07) << 4) | (last_bits & 0x07);
        self.write_reg(BIT_FRAMING_REG, framing)?;
        // Exécute Transceive.
        self.write_reg(COMMAND_REG, CMD_TRANSCEIVE)?;
        // StartSend = 1 → démarre l'émission.
        self.write_reg(BIT_FRAMING_REG, framing | 0x80)?;

        // Poll ComIrqReg : succès = RxIRq|IdleIRq (0x30, comme PCD_TransceiveData),
        // timeout = TimerIRq. La borne logicielle doit rester supérieure au timeout
        // matériel (sinon on abandonnerait avant que TimerIRq n'ait pu se lever —
        // cas des échanges T=CL où l'on monte le timer à 200 ms).
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_millis(self.timeout_ms as u64 + 50);
        loop {
            let irq = self.read_reg(COMIRQ_REG)?;
            if irq & (IRQ_RX | IRQ_IDLE) != 0 {
                // Vérifie les erreurs de réception (BufferOvfl|ParityErr|ProtocolErr).
                // CollErr (0x08) n'est pas une erreur ici (une seule carte).
                let err = self.read_reg(ERROR_REG)?;
                if err & 0x13 != 0 {
                    println!("[RC522] transceive ErrReg=0x{err:02X}");
                    return Ok(None);
                }
                let n = (self.read_reg(FIFO_LEVEL_REG)? & 0x7F) as usize;
                let r = self.read_fifo(n)?;
                println!("[RC522] transceive OK irq=0x{irq:02X} n={n} r={:02X?}", r);
                return Ok(Some(r));
            }
            if irq & IRQ_TIMER != 0 {
                println!("[RC522] transceive TIMER irq=0x{irq:02X}");
                return Ok(None);
            }
            if std::time::Instant::now() >= deadline {
                println!("[RC522] transceive poll-timeout");
                return Ok(None);
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    // -----------------------------------------------------------------------
    // Niveau ISO14443A
    // -----------------------------------------------------------------------

    /// Anticollision d'un niveau (SEL + NVB=0x20) → réponse 5 octets
    /// [X, b0, b1, b2, BCC]. X = CT (0x88) si le niveau suivant existe (UID 7/10
    /// octets), sinon premier octet d'UID. PAS de CRC (le CRC n'est utilisé que
    /// lorsque les bits UID sont connus, cf. PICC_Select).
    fn anticoll_level(&mut self, sel: u8) -> Result<Option<Vec<u8>>, String> {
        // ValuesAfterColl=0 : les bits reçus après une collision ne sont pas stockés.
        self.write_reg(COLL_REG, 0x00)?;
        match self.transceive(&[sel, 0x20], 0, false)? {
            Some(r) if r.len() >= 5 => Ok(Some(r[..5].to_vec())),
            Some(r) => {
                println!("[RC522] anticoll SEL={sel:02X} réponse courte {:02X?}", r);
                Ok(None)
            }
            None => {
                println!("[RC522] anticoll SEL={sel:02X} timeout");
                Ok(None)
            }
        }
    }

    /// Anticollision + sélection complètes (cascade niveaux 1/2/3) — équivalent
    /// PICC_Select de la lib Arduino. Gère les UID 4, 7 et 10 octets.
    /// Retourne (UID complet, SAK final). Le CRC_A du SELECT est calculé en logiciel
    /// (crc_a) et ajouté manuellement (TxCRCEn matériel peu fiable sur ce clone).
    fn select_card(&mut self) -> Result<Option<(Vec<u8>, u8)>, String> {
        let mut uid: Vec<u8> = Vec::new();
        let mut sak: u8 = 0;
        // SEL par niveau de cascade : CL1 = 0x93, CL2 = 0x95, CL3 = 0x97.
        let sel_by_level = [0x93u8, 0x95, 0x97];

        for (level, &sel) in sel_by_level.iter().enumerate() {
            // 1) ANTICOLLISION de ce niveau.
            let resp = match self.anticoll_level(sel)? {
                Some(r) => r,
                None => return Ok(None),
            };
            // BCC = XOR des 4 octets de données (resp[0..4]) — CT inclus si présent.
            let bcc = resp[0] ^ resp[1] ^ resp[2] ^ resp[3];
            if bcc != resp[4] {
                // La réponse est incohérente : UID probablement mal aligné à la
                // réception (erreur de parité / gain trop faible). On log mais on
                // continue : le select qui suit validera (ou pas) la trame.
                println!(
                    "[RC522] ANTICOLL BCC MISMATCH resp={:02X?} calc={:02X} (UID peut être mal aligné)",
                    resp, bcc
                );
            }

            // 2) SELECT (NVB=0x70) : SEL + 0x70 + [X,b0,b1,b2] + BCC + CRC_A.
            let mut cmd = vec![sel, 0x70];
            cmd.extend_from_slice(&resp[..4]);
            cmd.push(bcc);
            let crc = crc_a(&cmd);
            cmd.extend_from_slice(&crc);
            let sakr = match self.transceive(&cmd, 0, false)? {
                Some(r) if r.len() >= 3 => r[..3].to_vec(),
                Some(r) => {
                    println!("[RC522] select SEL={sel:02X} réponse courte {:02X?}", r);
                    return Ok(None);
                }
                None => {
                    println!("[RC522] select SEL={sel:02X} timeout (UID faux ou CRC KO)");
                    return Ok(None);
                }
            };
            // sakr = [SAK, crc_lo, crc_hi].
            let sak_now = sakr[0];
            let sak_crc = crc_a(&[sak_now]);
            if sakr.len() >= 3 && (sakr[1] != sak_crc[0] || sakr[2] != sak_crc[1]) {
                println!(
                    "[RC522] SAK CRC mismatch {:02X?} calc={:02X?}",
                    sakr, sak_crc
                );
            }

            // 3) UID de ce niveau : CT (0x88) n'est PAS un octet d'UID.
            if resp[0] == 0x88 {
                uid.extend_from_slice(&resp[1..4]); // 3 octets
            } else {
                uid.extend_from_slice(&resp[..4]); // 4 octets
            }
            println!(
                "[RC522] cascade {} UID={:02X?} SAK=0x{:02X}",
                level + 1,
                uid,
                sak_now
            );

            // 4) Bit cascade (0x04) → UID incomplet, niveau suivant. Sinon terminé.
            sak = sak_now;
            if sak & 0x04 == 0 {
                return Ok(Some((uid, sak)));
            }
        }
        // 3 niveaux traités (UID 10 octets) — on retourne ce qu'on a.
        Ok(Some((uid, sak)))
    }

    /// READ NTAG (0x30) → 16 octets (4 pages). CRC_A calculé en logiciel.
    fn read_block(&mut self, block: u8) -> Result<Option<[u8; 16]>, String> {
        let mut cmd = vec![0x30u8, block];
        let crc = crc_a(&cmd);
        cmd.extend_from_slice(&crc);
        match self.transceive(&cmd, 0, false)? {
            Some(r) => {
                // Réponse READ : 16 octets de données + 2 octets CRC_A (RxCRCEn=0 sur ce
                // clone, donc le CRC est inclus dans la FIFO). On le vérifie.
                // Une réponse tronquée ou un CRC KO sont des anomalies (collision,
                // carte retirée) — surtout pas une fin de données : on les remonte
                // en `Err`, `Ok(None)` restant réservé à l'absence de réponse.
                if r.len() < 18 {
                    return Err(format!("READ page {block}: réponse courte ({} o)", r.len()));
                }
                let data = &r[..16];
                if crc_a(data) != [r[16], r[17]] {
                    return Err(format!("READ page {block}: CRC KO"));
                }
                let mut out = [0u8; 16];
                out.copy_from_slice(data);
                Ok(Some(out))
            }
            None => Ok(None),
        }
    }

    /// READ avec 3 tentatives : un CRC KO est transitoire (collision, main qui
    /// bouge) et ne doit pas faire abandonner la lecture au premier essai.
    fn read_block_retry(&mut self, block: u8) -> Result<Option<[u8; 16]>, String> {
        let mut last = String::new();
        for _ in 0..3 {
            match self.read_block(block) {
                Ok(v) => return Ok(v),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// HALT (S(DESELECT)) : met une carte Type 2 en HALT. La carte ne répond pas
    /// (timeout), ce qui est le cas nominal.
    fn halt_a(&mut self) -> Result<(), String> {
        let mut cmd = vec![0x50u8, 0x00];
        let crc = crc_a(&cmd);
        cmd.extend_from_slice(&crc);
        let _ = self.transceive(&cmd, 0, false)?;
        Ok(())
    }

    /// Lecture NDEF d'une carte Type 2 (NTAG21x / MIFARE Ultralight, SAK=0x00).
    /// Lit la page 3 (Capability Container) pour dériver la taille mémoire, puis
    /// lit les pages de données par blocs de 4 jusqu'au terminateur 0xFE.
    fn read_ndef_type2(&mut self) -> Result<Option<String>, String> {
        let cc = match self.read_block_retry(3)? {
            Some(p) => p,
            None => return Ok(None),
        };
        // CC : octet 0 = magic 0xE1 (tag formaté NDEF), octet 2 = taille mémoire
        // / 8 octets (NTAG213=18→144 o, 215=63, 216=111).
        if cc[0] != 0xE1 || cc[2] == 0 {
            println!("[RC522] CC non NDEF ({:02X?})", &cc[..4]);
            let _ = self.halt_a();
            return Ok(None);
        }
        // Borné à 0x100 : le numéro de page de READ tient sur 8 bits, un CC hostile
        // (cc[2]=0xFF → 514) ferait sinon boucler la lecture indéfiniment.
        let last_page = (4usize + (cc[2] as usize) * 2).min(0x100);
        let mut data = Vec::new();
        let mut block = 4usize;
        while block < last_page {
            match self.read_block_retry(block as u8)? {
                Some(p) => data.extend_from_slice(&p),
                None => break,
            }
            // Arrêt piloté par la longueur annoncée du TLV NDEF (03 <len> ou
            // 03 FF <len_hi> <len_lo>) : un 0xFE quelconque dans un payload n'est
            // pas un terminateur et tronquerait le message.
            let need = ndef_tlv_total_len(&data);
            if data.len() >= need {
                break;
            }
            block += 4;
        }
        let _ = self.halt_a();
        match parse_ndef_tlv(&data) {
            Some(uri) => {
                println!("[RC522] NDEF URI = {uri}");
                Ok(Some(uri))
            }
            None => Ok(None),
        }
    }

    // -----------------------------------------------------------------------
    // ISO/IEC 14443-4 (T=CL) + NFC Forum Type 4 — NTAG 424 DNA / DESFire.
    // Chemin utilisé par les Bolt Cards (SAK & 0x20), qui ignorent READ (0x30).
    // -----------------------------------------------------------------------

    /// Paramètre de la commande RATS : FSDI (quartet haut) | CID (quartet bas).
    /// 0x80 = FSDI 8 (FSD 256 octets annoncés), CID 0.
    /// ATTENTION : la FIFO du MFRC522 ne fait que 64 octets, donc annoncer 256
    /// est optimiste. On borne toutes les lectures à `RD_CHUNK` pour qu'aucune
    /// trame retour n'approche cette limite ; si la carte dépassait quand même,
    /// le garde BufferOvfl d'ErrorReg (0x13) dans transceive() en fait un échec
    /// propre. La valeur strictement sûre serait 0x50 (FSDI 5 = 64 octets).
    const RATS_PARAM: u8 = 0x80;

    /// Le max demandé à READ BINARY : PCB(1) + 48 + SW(2) + CRC(2) = 53 ≤ 64.
    const RD_CHUNK: u8 = 48;

    /// SELECT de l'application NDEF par nom de DF (AID D2760000850101).
    const SEL_NDEF_APP: [u8; 13] = [
        0x00, 0xA4, 0x04, 0x00, 0x07, 0xD2, 0x76, 0x00, 0x00, 0x85, 0x01, 0x01, 0x00,
    ];

    /// Lecture NDEF d'une carte NFC Forum Type 4 : RATS → APDU ISO7816.
    fn read_ndef_type4(&mut self) -> Result<Option<String>, String> {
        // Les échanges T=CL dépassent largement 25 ms (FWT d'une NTAG 424 DNA,
        // plus les éventuels S(WTX)) → timer élargi le temps de la session.
        self.set_timer_ms(200)?;
        let res = self.type4_session();
        // Quoi qu'il arrive : S(DESELECT), sinon la carte reste à l'état PROTOCOL
        // où elle ne répond plus ni à WUPA ni à REQA (scan suivant aveugle).
        let _ = self.iso_dep_deselect();
        let _ = self.set_timer_ms(25);
        res
    }

    /// Corps de la session Type 4 (encadré par read_ndef_type4).
    fn type4_session(&mut self) -> Result<Option<String>, String> {
        // 1) RATS : état ACTIVE → PROTOCOL.
        let ats = match self.rats()? {
            Some(a) => a,
            None => return Ok(None),
        };
        if ats.len() >= 2 {
            // T0 : quartet bas = FSCI (taille max des trames émises par la carte).
            println!(
                "[RC522] ATS TL={} T0=0x{:02X} FSCI={}",
                ats[0],
                ats[1],
                ats[1] & 0x0F
            );
        }

        // 2) SELECT de l'application NDEF par nom de DF (AID D2760000850101).
        if self
            .apdu_data(&Self::SEL_NDEF_APP, "SELECT AID NDEF")?
            .is_none()
        {
            return Ok(None);
        }

        // 3) Capability Container (FID E103) : il donne l'identifiant réel du
        // fichier NDEF et la taille max lisible (MLe).
        if self.select_ef(0xE103, "SELECT CC E103")?.is_none() {
            return Ok(None);
        }
        let cc = match self.read_binary(0, 15, "READ CC")? {
            Some(d) => d,
            None => return Ok(None),
        };
        println!("[RC522] CC = {:02X?}", cc);
        let cci = CcInfo::parse(&cc);
        let (ndef_fid, mle) = (cci.ndef_fid, cci.mle);
        let chunk = cap_chunk(Self::RD_CHUNK, mle);
        println!("[RC522] fichier NDEF FID=0x{ndef_fid:04X} MLe={mle} chunk={chunk}");

        // Diagnostique : réglages SDM + compteur anti-replay du fichier NDEF.
        // GetFileSettings : 90 F5 00 00 01 <FileNr> → octet 4 = SDMOptions.
        // GetFileCounters : 90 F6 00 00 01 <FileNr> → SDMReadCtr 3 o LSB first.
        // Le numéro de fichier NDEF n'est pas connu a priori → essaie 2, 1, 3.
        for nr in [2u8, 1, 3] {
            match self.apdu_data(&[0x90, 0xF5, 0x00, 0x00, 0x01, nr, 0x00], "GETFILESETTINGS")? {
                Some(fs) if fs.len() >= 8 => {
                    println!(
                        "[RC522] FileSettings(#{nr}) ({} o) = {:02X?} SDMOptions=0x{:02X}",
                        fs.len(),
                        fs,
                        fs[7]
                    );
                    break;
                }
                _ => {}
            }
        }
        for nr in [2u8, 1, 3] {
            match self.apdu_data(&[0x90, 0xF6, 0x00, 0x00, 0x01, nr, 0x00], "GETFILECOUNTERS")? {
                Some(ct) if ct.len() >= 3 => {
                    let ctr = u32::from_le_bytes([ct[0], ct[1], ct[2], 0]);
                    println!("[RC522] SDMReadCtr(#{nr}) = {ctr}");
                    break;
                }
                _ => {}
            }
        }

        // 4) SELECT du fichier NDEF, puis NLEN (ses 2 premiers octets).
        if self.select_ef(ndef_fid, "SELECT fichier NDEF")?.is_none() {
            return Ok(None);
        }
        let nlen_r = match self.read_binary(0, 2, "READ NLEN")? {
            Some(d) if d.len() >= 2 => d,
            _ => return Ok(None),
        };
        let nlen = u16::from_be_bytes([nlen_r[0], nlen_r[1]]) as usize;
        println!("[RC522] NLEN={nlen}");
        if nlen == 0 {
            return Err("fichier NDEF vide".into());
        }

        // 5) Corps du message NDEF, par tranches (offset 2 = juste après NLEN).
        let mut msg = Vec::with_capacity(nlen);
        while msg.len() < nlen {
            let off = 2 + msg.len();
            let want = (nlen - msg.len()).min(chunk as usize) as u8;
            match self.read_binary(off as u16, want, "READ NDEF")? {
                Some(d) if !d.is_empty() => msg.extend_from_slice(&d),
                _ => break,
            }
        }
        println!("[RC522] NDEF ({} o) = {:02X?}", msg.len(), msg);

        // 6) Un fichier Type 4 contient le message NDEF brut, précédé du seul
        // NLEN (déjà retiré) : pas d'enveloppe TLV comme sur NTAG21x. On décode
        // donc directement le record URI, avec repli sur le parcours TLV
        // uniquement si le message commence réellement par un TLV NDEF (0x03) —
        // sinon l'octet de flags (0xD1…) serait pris pour un type TLV.
        let parsed = parse_ndef_records(&msg).or_else(|| {
            if msg.first() == Some(&0x03) {
                parse_ndef_tlv(&msg)
            } else {
                None
            }
        });
        match parsed {
            Some(uri) => {
                println!("[RC522] NDEF URI = {uri}");
                Ok(Some(uri))
            }
            None => Err("TLV NDEF introuvable".into()),
        }
    }

    /// RATS (ISO14443-4 §5.6) → ATS. Réinitialise le numéro de bloc T=CL.
    fn rats(&mut self) -> Result<Option<Vec<u8>>, String> {
        let mut cmd = vec![0xE0u8, Self::RATS_PARAM];
        let crc = crc_a(&cmd);
        cmd.extend_from_slice(&crc);
        self.iso_block = 0;
        match self.transceive(&cmd, 0, false)? {
            // ATS = TL [T0 [TA TB TC]] [octets historiques] + CRC_A
            // (RxCRCEn est désactivé, le CRC arrive donc dans la FIFO).
            Some(r) if r.len() >= 3 => {
                let ats = r[..r.len() - 2].to_vec();
                println!("[RC522] RATS ATS={:02X?}", ats);
                Ok(Some(ats))
            }
            Some(r) => {
                println!("[RC522] RATS réponse courte {:02X?}", r);
                Ok(None)
            }
            None => {
                println!("[RC522] RATS timeout");
                Ok(None)
            }
        }
    }

    /// Échange une APDU ISO7816 sur T=CL : encapsulation en I-block, suivi du
    /// numéro de bloc, réponse aux S(WTX) et réassemblage du chaînage.
    ///
    /// Trame I-block : [PCB][CID?][NAD?] + APDU + CRC_A. CID et NAD sont des
    /// champs CONDITIONNELS, présents seulement si le PCB porte les bits 0x08
    /// (CID) et 0x04 (NAD). RATS ayant négocié CID=0, on les omet à l'émission :
    /// les inclure avec PCB=0x02 décalerait l'APDU d'un octet et la carte
    /// rejetterait la trame. À la réception, on les saute s'ils sont présents.
    fn iso_dep_apdu(&mut self, apdu: &[u8]) -> Result<Option<Vec<u8>>, String> {
        let mut frame = Vec::with_capacity(apdu.len() + 3);
        frame.push(0x02 | self.iso_block); // I-block, sans chaînage
        frame.extend_from_slice(apdu);
        let crc = crc_a(&frame);
        frame.extend_from_slice(&crc);

        let mut out: Vec<u8> = Vec::new();
        // Borne les allers-retours S(WTX) / chaînage pour ne pas boucler.
        for _ in 0..16 {
            let r = match self.transceive(&frame, 0, false)? {
                Some(r) if r.len() >= 3 => r,
                Some(r) => {
                    println!("[RC522] T=CL trame courte {:02X?}", r);
                    return Ok(None);
                }
                None => {
                    println!("[RC522] T=CL timeout (PCB=0x{:02X})", frame[0]);
                    return Ok(None);
                }
            };
            let body = &r[..r.len() - 2]; // retire le CRC_A
            let pcb = body[0];
            let mut inf = 1;
            if pcb & 0x08 != 0 {
                inf += 1; // CID
            }
            if pcb & 0x04 != 0 {
                inf += 1; // NAD
            }
            if inf > body.len() {
                println!("[RC522] T=CL trame incohérente {:02X?}", body);
                return Ok(None);
            }

            match pcb & 0xC0 {
                // I-block (0b00xx xxxx) : données applicatives.
                0x00 => {
                    out.extend_from_slice(&body[inf..]);
                    self.iso_block ^= 1;
                    if pcb & 0x10 == 0 {
                        return Ok(Some(out)); // pas de chaînage → réponse complète
                    }
                    // Chaînage : on acquitte par un R(ACK) portant le nouveau
                    // numéro de bloc, la carte enverra la suite.
                    frame = vec![0xA2 | self.iso_block];
                }
                // S-block (0b11xx xxxx) : seul S(WTX) est attendu ici.
                0xC0 => {
                    if pcb & 0x30 != 0x30 {
                        println!("[RC522] T=CL S-block inattendu {:02X?}", body);
                        return Ok(None);
                    }
                    // S(WTX) : la carte demande un délai, on renvoie le même WTXM.
                    let wtxm = body.get(inf).copied().unwrap_or(1) & 0x3F;
                    println!("[RC522] T=CL WTX x{wtxm}");
                    frame = vec![0xF2, wtxm];
                }
                // R-block (0b10xx xxxx) : inattendu, le PCD ne chaîne pas ici.
                _ => {
                    println!("[RC522] T=CL R-block {:02X?}", body);
                    return Ok(None);
                }
            }
            let crc = crc_a(&frame);
            frame.extend_from_slice(&crc);
        }
        println!("[RC522] T=CL trop d'échanges (WTX/chaînage)");
        Ok(None)
    }

    /// Envoie une APDU et sépare données / status word. `Some(data)` seulement
    /// si SW = 0x9000.
    fn apdu_data(&mut self, apdu: &[u8], what: &str) -> Result<Option<Vec<u8>>, String> {
        let r = match self.iso_dep_apdu(apdu)? {
            Some(r) if r.len() >= 2 => r,
            Some(r) => {
                println!("[RC522] {what} : réponse tronquée {:02X?}", r);
                return Ok(None);
            }
            None => {
                println!("[RC522] {what} : pas de réponse");
                return Ok(None);
            }
        };
        let sw = u16::from_be_bytes([r[r.len() - 2], r[r.len() - 1]]);
        let data = r[..r.len() - 2].to_vec();
        println!("[RC522] {what} SW=0x{sw:04X} ({} o)", data.len());
        if sw != 0x9000 {
            return Ok(None);
        }
        Ok(Some(data))
    }

    /// SELECT EF par identifiant de fichier (P1=0x00, P2=0x0C : pas de FCI).
    fn select_ef(&mut self, fid: u16, what: &str) -> Result<Option<Vec<u8>>, String> {
        let apdu = [0x00u8, 0xA4, 0x00, 0x0C, 0x02, (fid >> 8) as u8, fid as u8];
        self.apdu_data(&apdu, what)
    }

    /// READ BINARY (00 B0 offset_hi offset_lo Le).
    fn read_binary(&mut self, off: u16, le: u8, what: &str) -> Result<Option<Vec<u8>>, String> {
        let apdu = [0x00u8, 0xB0, (off >> 8) as u8, off as u8, le];
        self.apdu_data(&apdu, what)
    }

    /// S(DESELECT) : PROTOCOL → HALT, pour que le WUPA du scan suivant réveille
    /// à nouveau la carte sans devoir la sortir du champ.
    fn iso_dep_deselect(&mut self) -> Result<(), String> {
        let mut frame = vec![0xC2u8];
        let crc = crc_a(&frame);
        frame.extend_from_slice(&crc);
        match self.transceive(&frame, 0, false)? {
            Some(r) => println!("[RC522] DESELECT ok {:02X?}", r),
            None => println!("[RC522] DESELECT sans réponse"),
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // MIFARE Classic 1K (SAK=0x08) — authentification Crypto-1 + MAD + NDEF.
    //
    // Cartographie 1K : 16 secteurs × 4 blocs × 16 octets.
    //   secteur 0 : bloc 0 = constructeur (UID+BCC), blocs 1-2 = MAD, bloc 3 = trailer
    //   secteur s : blocs 4s..4s+2 = données, bloc 4s+3 = trailer (clés + bits d'accès)
    // Chaque secteur doit être authentifié avant lecture ; une fois MFCrypto1On à 1,
    // READ (0x30) est la même commande qu'en Type 2, chiffrée par le MFRC522.
    // -----------------------------------------------------------------------

    /// Authentification Crypto-1 avec la clé A d'usine (6×0xFF) — équivalent
    /// `PCD_Authenticate` de la lib Arduino, cas du badge vierge.
    fn mifare_auth(&mut self, cmd: u8, block: u8, uid: &[u8]) -> Result<bool, String> {
        self.mifare_auth_key(cmd, block, &KEY_DEFAULT, uid)
    }

    /// Idem avec une clé explicite (les cartes formatées NDEF utilisent les clés
    /// publiques MAD / NFC Forum, pas la clé d'usine).
    ///
    /// ATTENTION (ordre des octets) : la consigne initiale décrivait « clé puis
    /// cmd+bloc+UID ». Ce n'est PAS ce qu'attend le MFRC522. La datasheet NXP
    /// (§10.3.1.9, commande MFAuthent) impose exactement 12 octets dans la FIFO :
    ///   [cmd][adresse de bloc][clé 0..5][UID 0..3]
    /// — même ordre que `PCD_Authenticate` de la lib Arduino. Toute autre
    /// disposition fait échouer l'authentification silencieusement.
    ///
    /// `uid` : les 4 octets d'UID utilisés par Crypto-1 (une Classic a un UID
    /// 4 octets ; sur un UID plus long ce sont les 4 DERNIERS qui comptent).
    fn mifare_auth_key(
        &mut self,
        cmd: u8,
        block: u8,
        key: &[u8; 6],
        uid: &[u8],
    ) -> Result<bool, String> {
        if uid.len() < 4 {
            return Err(format!(
                "auth bloc {block}: UID trop court ({} o)",
                uid.len()
            ));
        }
        let uid4 = &uid[uid.len() - 4..];

        // Stoppe toute commande en cours, efface les IRQ, vide la FIFO.
        self.write_reg(COMMAND_REG, CMD_IDLE)?;
        self.write_reg(COMIRQ_REG, 0x7F)?;
        self.write_reg(FIFO_LEVEL_REG, 0x80)?;

        let mut buf = Vec::with_capacity(12);
        buf.push(cmd);
        buf.push(block);
        buf.extend_from_slice(key);
        buf.extend_from_slice(uid4);
        self.write_fifo(&buf)?;

        // Pas de trame courte ni de StartSend : MFAuthent gère l'échange seul.
        self.write_reg(BIT_FRAMING_REG, 0x00)?;
        self.write_reg(COMMAND_REG, CMD_AUTHENT)?;

        // Succès = MFCrypto1On (Status2Reg bit 3) passe à 1. Un échec se traduit
        // par TimerIRq (la carte ne répond pas) ou par l'expiration de la borne
        // logicielle : dans les deux cas MFCrypto1On reste à 0.
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_millis(self.timeout_ms as u64 + 25);
        loop {
            if self.read_reg(STATUS2_REG)? & STATUS2_MFCRYPTO1_ON != 0 {
                self.write_reg(COMMAND_REG, CMD_IDLE)?;
                return Ok(true);
            }
            let irq = self.read_reg(COMIRQ_REG)?;
            if irq & IRQ_TIMER != 0 {
                println!("[RC522] auth bloc {block} : TimerIRq (clé refusée ?)");
                break;
            }
            if std::time::Instant::now() >= deadline {
                println!("[RC522] auth bloc {block} : poll-timeout (irq=0x{irq:02X})");
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        self.write_reg(COMMAND_REG, CMD_IDLE)?;
        Ok(false)
    }

    /// Coupe le chiffrement Crypto-1 côté lecteur (équivalent `PCD_StopCrypto1`).
    /// Sans cet appel, toute trame suivante (HALT, WUPA…) partirait chiffrée et la
    /// carte resterait sourde jusqu'au prochain reset du module.
    fn stop_crypto1(&mut self) -> Result<(), String> {
        let v = self.read_reg(STATUS2_REG)?;
        self.write_reg(STATUS2_REG, v & !STATUS2_MFCRYPTO1_ON)
    }

    /// Remet la carte à l'état ACTIVE après un échec d'authentification : une auth
    /// refusée fait cesser toute réponse du PICC, il faut le réveiller (WUPA) et
    /// refaire l'anticollision/select avant de retenter avec une autre clé.
    fn mifare_reselect(&mut self) -> Result<bool, String> {
        self.stop_crypto1()?;
        let _ = self.halt_a();
        if self.transceive(&[0x52], 7, false)?.is_none() {
            println!("[RC522] reselect : WUPA sans réponse");
            return Ok(false);
        }
        match self.select_card()? {
            Some((uid, _sak)) => {
                self.last_uid = uid;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Authentifie un secteur en essayant les clés A candidates dans l'ordre le
    /// plus probable. Authentifier le premier bloc suffit : les droits Crypto-1
    /// portent sur le secteur entier.
    fn mifare_auth_sector(&mut self, sector: u8) -> Result<bool, String> {
        let block = sector * 4;
        // Secteur 0 = MAD (clé publique A0A1…) ; secteurs de données d'une carte
        // formatée NDEF = clé publique NFC Forum ; badge vierge = clé d'usine.
        let keys: [[u8; 6]; 3] = if sector == 0 {
            [KEY_MAD, KEY_DEFAULT, KEY_NFC_FORUM]
        } else {
            [KEY_DEFAULT, KEY_NFC_FORUM, KEY_MAD]
        };
        for key in keys.iter() {
            // Re-sélection avant CHAQUE tentative : MFCrypto1On est collant (jamais
            // remis à 0 par le matériel) et la carte reste chiffrée après une auth
            // réussie — seul halt+WUPA+select remet les deux côtés à zéro. Sans ça,
            // une 2e auth lirait un bit rémanent et « réussirait » à tort.
            if !self.mifare_reselect()? {
                return Ok(false);
            }
            let uid = self.last_uid.clone();
            let ok = if *key == KEY_DEFAULT {
                self.mifare_auth(PICC_AUTHENT1A, block, &uid)?
            } else {
                self.mifare_auth_key(PICC_AUTHENT1A, block, key, &uid)?
            };
            if ok {
                println!("[RC522] secteur {sector} authentifié (clé A {:02X?})", key);
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Lit le MIFARE Application Directory (secteur 0, blocs 1-2) et renvoie les
    /// secteurs déclarés NDEF. Une entrée MAD fait 2 octets et l'entrée d'indice
    /// `s` décrit le secteur `s` : c'est la POSITION qui donne le secteur, la
    /// valeur étant l'AID de l'application. L'AID NFC Forum vaut 0x03E1
    /// (cluster 0x03), stocké petit-boutiste → octets `E1 03`.
    /// Vecteur vide = MAD absent ou illisible (l'appelant se replie).
    fn read_mad(&mut self) -> Result<Vec<u8>, String> {
        if !self.mifare_auth_sector(0)? {
            println!("[RC522] MAD : authentification du secteur 0 impossible");
            let _ = self.mifare_reselect()?;
            return Ok(Vec::new());
        }
        // Bloc 1 : [CRC][info] puis les entrées des secteurs 1..7.
        // Bloc 2 : les entrées des secteurs 8..15. L'entrée du secteur s est donc
        // à l'offset 2*s de la concaténation des deux blocs.
        let mut raw = Vec::with_capacity(32);
        for b in 1..=2u8 {
            match self.read_block_retry(b)? {
                Some(d) => raw.extend_from_slice(&d),
                None => {
                    println!("[RC522] MAD : bloc {b} illisible");
                    let _ = self.mifare_reselect()?;
                    return Ok(Vec::new());
                }
            }
        }
        let mut out = Vec::new();
        for sector in 1..16usize {
            let (lo, hi) = (raw[2 * sector], raw[2 * sector + 1]);
            // Tolérance sur l'ordre : quelques encodeurs écrivent l'AID gros-boutiste.
            if (lo == 0xE1 && hi == 0x03) || (lo == 0x03 && hi == 0xE1) {
                out.push(sector as u8);
            }
        }
        println!("[RC522] MAD lu, secteurs NDEF = {:?}", out);
        Ok(out)
    }

    /// Lecture NDEF d'une carte MIFARE Classic 1K (SAK=0x08).
    fn read_ndef_mifare_classic(&mut self) -> Result<Option<String>, String> {
        let res = self.mifare_session();
        // Quoi qu'il arrive : couper Crypto-1 puis remettre la carte en HALT, sinon
        // le scan suivant émettrait un WUPA chiffré dans le vide.
        let _ = self.stop_crypto1();
        let _ = self.halt_a();
        res
    }

    /// Corps de la session MIFARE Classic (encadré par `read_ndef_mifare_classic`).
    fn mifare_session(&mut self) -> Result<Option<String>, String> {
        // 1) MAD → secteurs porteurs du message NDEF. Repli sur le secteur 1, qui
        // est l'emplacement conventionnel quand le MAD est absent/illisible.
        let mut sectors = self.read_mad()?;
        if sectors.is_empty() {
            println!("[RC522] MAD sans entrée NDEF → repli sur les secteurs 1..15");
            sectors.extend(1..16u8);
        }

        // 2) Concaténation des blocs de données des secteurs NDEF : ils portent un
        // TLV [0x03][LEN][message NDEF][0xFE], LEN sur 1 octet (<0xFF) ou 3 octets
        // (0xFF + 2 octets big-endian). Le message peut s'étaler sur plusieurs
        // secteurs, d'où la lecture incrémentale bornée par la longueur annoncée.
        let mut data: Vec<u8> = Vec::new();
        for sector in sectors {
            // Le secteur 0 ne porte jamais de NDEF (blocs 1-2 = MAD).
            if sector == 0 || sector >= 16 {
                continue;
            }
            if !self.mifare_auth_sector(sector)? {
                println!("[RC522] secteur {sector} : aucune clé connue, lecture arrêtée");
                break;
            }
            let first = sector * 4;
            for b in 0..3u8 {
                match self.read_block_retry(first + b)? {
                    Some(d) => data.extend_from_slice(&d),
                    None => {
                        println!("[RC522] bloc {} illisible", first + b);
                        break;
                    }
                }
            }
            if data.len() >= ndef_tlv_total_len(&data) {
                break;
            }
        }
        println!("[RC522] MIFARE Classic : {} o de données", data.len());

        // 3) Le TLV est décodé par le même parseur que le Type 2 (il gère les deux
        // formats de longueur et délègue à parse_ndef_records) ; repli sur un
        // message NDEF brut si le secteur ne commence pas par un TLV.
        let parsed = parse_ndef_tlv(&data).or_else(|| parse_ndef_records(&data));
        match parsed {
            Some(uri) => {
                println!("[RC522] NDEF URI = {uri}");
                Ok(Some(uri))
            }
            None => {
                println!("[RC522] MIFARE Classic : aucun TLV NDEF exploitable");
                Ok(None)
            }
        }
    }
}

/// Champs utiles du Capability Container Type 4 :
/// CCLEN(2) Version(1) MLe(2) MLc(2) puis TLV « NDEF File Control »
/// T=0x04 L=0x06 FileID(2) MaxSize(2) Read(1) Write(1).
/// Seuls les champs utiles à la LECTURE sont retenus : MLc, MaxSize et la
/// condition d'accès en écriture ne servaient qu'à l'ancienne réécriture du
/// compteur, supprimée (le compteur d'une NTAG 424 est matériel et s'incrémente
/// tout seul — le lecteur ne doit rien réécrire).
struct CcInfo {
    ndef_fid: u16,
    /// Taille max d'une réponse R-APDU (0 = non annoncée).
    mle: u16,
}

impl CcInfo {
    fn parse(cc: &[u8]) -> Self {
        if cc.len() >= 11 && cc[7] == 0x04 {
            Self {
                ndef_fid: u16::from_be_bytes([cc[9], cc[10]]),
                mle: u16::from_be_bytes([cc[3], cc[4]]),
            }
        } else {
            println!("[RC522] CC illisible → valeurs par défaut (FID E104)");
            Self {
                ndef_fid: 0xE104,
                mle: 0,
            }
        }
    }
}

/// Taille totale (en-tête compris) du TLV NDEF `03 <len>` en tête de `data`, ou
/// `usize::MAX` tant qu'on ne peut pas trancher. Sert à arrêter une lecture
/// incrémentale sur la longueur annoncée plutôt que sur un 0xFE rencontré dans
/// un payload (qui tronquerait le message).
fn ndef_tlv_total_len(data: &[u8]) -> usize {
    match (data.first(), data.get(1)) {
        (Some(&0x03), Some(&0xFF)) if data.len() >= 4 => {
            4 + (((data[2] as usize) << 8) | data[3] as usize)
        }
        (Some(&0x03), Some(&l)) if l != 0xFF => 2 + l as usize,
        // Pas de TLV NDEF en tête : on lit tout et on laisse le parseur trancher.
        _ => usize::MAX,
    }
}

/// Borne une tranche APDU par la valeur annoncée dans le CC (0 = non annoncée),
/// sans jamais descendre à 0 (qui ferait boucler les lectures/écritures).
fn cap_chunk(max: u8, announced: u16) -> u8 {
    if announced == 0 {
        max
    } else {
        max.min(announced.min(255) as u8).max(1)
    }
}

/// CRC_A ISO/IEC 14443-3 (polynôme 0x1021 réfléchi = 0x8408, init 0x6363).
/// Retourne [octet bas, octet haut] (ordre de transmission LSB-first).
fn crc_a(data: &[u8]) -> [u8; 2] {
    let mut crc: u16 = 0x6363;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0x8408
            } else {
                crc >> 1
            };
        }
    }
    [crc as u8, (crc >> 8) as u8]
}

// ---------------------------------------------------------------------------
// Parsing NDEF
// ---------------------------------------------------------------------------

/// Parcourt les TLV (0x03 = message NDEF, 0xFE = terminateur, 0x00 = TLV vide)
/// et extrait l'URI du premier record NDEF exploitable.
fn parse_ndef_tlv(data: &[u8]) -> Option<String> {
    let mut i = 0usize;
    while i < data.len() {
        let t = data[i];
        if t == 0xFE {
            break;
        }
        if t == 0x00 {
            i += 1; // TLV vide, pas de champ longueur
            continue;
        }
        if i + 1 >= data.len() {
            break;
        }
        // Champ longueur : 1 octet (0x00..0xFE), ou 0xFF suivi de 2 octets (big-endian).
        let (len, hdr) = if data[i + 1] == 0xFF {
            if i + 4 > data.len() {
                break;
            }
            (((data[i + 2] as usize) << 8) | (data[i + 3] as usize), 4)
        } else {
            (data[i + 1] as usize, 2)
        };
        let start = i + hdr;
        let end = (start + len).min(data.len());
        if t == 0x03 {
            if let Some(uri) = parse_ndef_records(&data[start..end]) {
                return Some(uri);
            }
        }
        i = end;
    }
    None
}

/// Vrai si la chaîne est une **forme LNURL** exploitable par le POS, c'est-à-dire
/// `lnurlw://…` ou le bech32 `lnurl1…`, éventuellement préfixés de `lightning:`.
///
/// Les anciens critères (« contient lnurl / withdraw / boltcard ») laissaient
/// passer n'importe quelle URL portant l'un de ces mots : un tag NFC quelconque
/// suffisait à faire émettre au POS un GET vers l'hôte de son choix. La
/// validation finale du schéma et de l'hôte est faite par `normalize_lnurl`.
fn looks_like_lnurl(s: &str) -> bool {
    let mut l = s.trim().to_ascii_lowercase();
    for p in ["lightning://", "lightning:"] {
        if let Some(rest) = l.strip_prefix(p) {
            l = rest.to_string();
            break;
        }
    }
    l.starts_with("lnurlw://") || l.starts_with("lnurl1")
}

/// Itère sur les records d'un message NDEF et extrait l'URI du premier record qui
/// est une forme LNURL (cf. [`looks_like_lnurl`]). Gère TNF=1 type "U" (préfixe
/// URI), TNF=1 type "T" (texte), et TNF=3 (URI absolue, l'URI est dans le champ
/// TYPE). Champs SR/IL décodés proprement, drapeaux ME/CF respectés.
///
/// Aucun repli sur « la première URI rencontrée » : ce repli faisait remonter
/// l'URL d'un tag NFC quelconque jusqu'au GET du flux de paiement.
fn parse_ndef_records(ndef: &[u8]) -> Option<String> {
    let mut i = 0usize;
    while i < ndef.len() {
        let flags = ndef[i];
        let tnf = flags & 0x07;
        let sr = flags & 0x10 != 0;
        let il = flags & 0x08 != 0;
        // CF (record chaîné) : le recollage n'est pas géré, on ne devine pas.
        if flags & 0x20 != 0 {
            break;
        }
        if i + 2 > ndef.len() {
            break;
        }
        let type_len = ndef[i + 1] as usize;
        let (payload_len, plen_bytes) = if sr {
            (ndef[i + 2] as usize, 1)
        } else {
            if i + 6 > ndef.len() {
                break;
            }
            let l = ((ndef[i + 2] as usize) << 24)
                | ((ndef[i + 3] as usize) << 16)
                | ((ndef[i + 4] as usize) << 8)
                | (ndef[i + 5] as usize);
            (l, 4)
        };
        let mut p = i + 2 + plen_bytes;
        let id_len = if il {
            if p >= ndef.len() {
                break;
            }
            let l = ndef[p] as usize;
            p += 1;
            l
        } else {
            0
        };
        // Bornes calculées en arithmétique vérifiée : `payload_len` va jusqu'à
        // 0xFFFF_FFFF (SR=0) et `usize` fait 32 bits ici, un simple `+` wrapperait
        // et la garde `> ndef.len()` laisserait passer un slice invalide.
        let type_start = p;
        let type_end = match type_start.checked_add(type_len) {
            Some(v) if v <= ndef.len() => v,
            _ => break,
        };
        let payload_start = match type_end.checked_add(id_len) {
            Some(v) if v <= ndef.len() => v,
            _ => break,
        };
        if payload_len > ndef.len() - payload_start {
            break;
        }
        let payload_end = payload_start + payload_len;
        let rtype = &ndef[type_start..type_end];
        let payload = &ndef[payload_start..payload_end];

        // Un record inexploitable ne condamne pas le message : les badges NTAG
        // portent souvent plusieurs records (URI + AAR + texte).
        let candidate: Option<String> = match tnf {
            1 if rtype == b"U" && !payload.is_empty() => {
                let prefix = URI_PREFIXES.get(payload[0] as usize).copied().unwrap_or("");
                core::str::from_utf8(&payload[1..])
                    .ok()
                    .map(|rest| format!("{prefix}{}", rest.trim()))
            }
            1 if rtype == b"T" && !payload.is_empty() => {
                let lang_len = (payload[0] & 0x3F) as usize;
                let start = 1 + lang_len;
                if start > payload.len() {
                    None
                } else {
                    core::str::from_utf8(&payload[start..])
                        .ok()
                        .map(str::trim)
                        .filter(|s| {
                            let l = s.to_ascii_lowercase();
                            l.starts_with("http")
                                || l.starts_with("lnurl")
                                || l.starts_with("lightning:")
                        })
                        .map(str::to_string)
                }
            }
            // TNF=3 (Absolute URI, RFC 3986) : l'URI est dans le champ TYPE ; le
            // payload est applicatif (souvent vide). Repli sur le payload pour les
            // encodeurs qui s'écartent de la spec.
            3 => core::str::from_utf8(rtype)
                .ok()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    core::str::from_utf8(payload)
                        .ok()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                }),
            _ => None,
        };

        if let Some(uri) = candidate {
            if looks_like_lnurl(&uri) {
                return Some(uri);
            }
        }

        // ME : dernier record du message, la suite n'est que du padding.
        if flags & 0x40 != 0 {
            break;
        }
        i = payload_end;
    }
    None
}

/// Table des préfixes URI NFC Forum (0x00..0x23).
const URI_PREFIXES: [&str; 36] = [
    "",                           // 0x00
    "http://www.",                // 0x01
    "https://www.",               // 0x02
    "http://",                    // 0x03
    "https://",                   // 0x04
    "tel:",                       // 0x05
    "mailto:",                    // 0x06
    "ftp://anonymous:anonymous@", // 0x07
    "ftp://ftp.",                 // 0x08
    "ftps://",                    // 0x09
    "sftp://",                    // 0x0A
    "smb://",                     // 0x0B
    "nfs://",                     // 0x0C
    "ftp://",                     // 0x0D
    "dav://",                     // 0x0E
    "news:",                      // 0x0F
    "telnet://",                  // 0x10
    "imap:",                      // 0x11
    "rtsp://",                    // 0x12
    "urn:",                       // 0x13
    "pop:",                       // 0x14
    "sip:",                       // 0x15
    "sips:",                      // 0x16
    "tftp:",                      // 0x17
    "btspp://",                   // 0x18
    "btl2cap://",                 // 0x19
    "btgoep://",                  // 0x1A
    "tcpobex://",                 // 0x1B
    "irdaobex://",                // 0x1C
    "file://",                    // 0x1D
    "urn:epc:id:",                // 0x1E
    "urn:epc:tag:",               // 0x1F
    "urn:epc:pat:",               // 0x20
    "urn:epc:raw:",               // 0x21
    "urn:epc:",                   // 0x22
    "urn:nfc:",                   // 0x23
];
