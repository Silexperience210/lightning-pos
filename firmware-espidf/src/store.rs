//! Stockage NVS des produits (15 entrées).
//!
//! Chaque produit est rangé sous deux clés dans le namespace NVS `products` :
//! - `n{i}` : nom (chaîne)
//! - `c{i}` : prix en centimes (u32)

use esp_idf_svc::nvs::EspDefaultNvs;
use esp_idf_svc::sys::EspError;

/// Nombre de produits configurables.
pub const NUM_PRODUCTS: usize = 15;

/// Un produit : nom ASCII (majuscules, sans accents) + prix en centimes.
#[derive(Debug, Clone)]
pub struct Product {
    pub name: String,
    pub cents: u32,
}

/// Noms par défaut (ASCII majuscule pour la fonte 5×7 embarquée).
const DEFAULT_NAMES: [&str; NUM_PRODUCTS] = [
    "BIERE", "VIN", "CAFE", "SODA", "PUNCH", "TSHIRT", "GOODIES", "EAU", "JUS", "THE",
    "COCKTAIL", "BURGER", "FRITES", "DESSERT", "MENU",
];

/// Prix par défaut, en centimes.
const DEFAULT_CENTS: [u32; NUM_PRODUCTS] = [
    400, 800, 200, 300, 600, 2500, 1000, 100, 300, 200, 900, 1200, 400, 600, 1500,
];

/// Renvoie les 15 produits par défaut.
pub fn defaults() -> Vec<Product> {
    DEFAULT_NAMES
        .iter()
        .zip(DEFAULT_CENTS.iter())
        .map(|(name, cents)| Product {
            name: (*name).to_string(),
            cents: *cents,
        })
        .collect()
}

/// Lit les 15 produits depuis NVS ; retombe sur la valeur par défaut si une
/// clé est absente ou illisible.
pub fn load(nvs: &EspDefaultNvs) -> Vec<Product> {
    let defaults = defaults();

    (0..NUM_PRODUCTS)
        .map(|i| {
            let name_key = format!("n{i}");
            let price_key = format!("c{i}");

            let mut buf = [0u8; 64];
            let name = nvs
                .get_str(&name_key, &mut buf)
                .ok()
                .flatten()
                .map(|s| s.to_string())
                .unwrap_or_else(|| defaults[i].name.clone());

            let cents = nvs
                .get_u32(&price_key)
                .ok()
                .flatten()
                .unwrap_or(defaults[i].cents);

            Product { name, cents }
        })
        .collect()
}

/// Écrit les 15 noms + prix dans NVS. Les entrées manquantes de `products`
/// sont remplacées par les valeurs par défaut.
pub fn save(nvs: &EspDefaultNvs, products: &[Product]) -> Result<(), EspError> {
    let defaults = defaults();

    for i in 0..NUM_PRODUCTS {
        let product = products.get(i).unwrap_or(&defaults[i]);
        nvs.set_str(&format!("n{i}"), &product.name)?;
        nvs.set_u32(&format!("c{i}"), product.cents)?;
    }

    Ok(())
}
